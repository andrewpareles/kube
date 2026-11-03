//! One persistent workspace per worker, which Node keeps for as long as the editor runs. JSON lines on stdin/stdout are
//! private IPC with Node; shell output uses a separate file. Only sandboxed children execute generated commands.
#[cfg(target_os = "linux")]
mod sandbox;
#[cfg(target_os = "linux")]
mod watchdog;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use base64::{Engine, engine::general_purpose::STANDARD};
    use linux_sandbox::{MAX_IMAGE_BYTES, shrink_png_for_ai};
    use sandbox::{FILE_BYTES, SANDBOX_UID, check_workspace_filesystem, exec_command};
    use serde::Deserialize;
    use serde_json::json;
    use std::{
        collections::HashSet,
        ffi::CString,
        fs::File,
        io::{BufRead, Read, Seek, Write},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{fs::PermissionsExt, process::CommandExt},
        },
        process::{Command, Stdio},
        sync::Mutex,
        time::{Duration, Instant},
    };
    use watchdog::watch_session;

    ensure!(unsafe { libc::close_range(3, u32::MAX, 0) } == 0, "Cannot close inherited worker descriptors.");
    let mut args = std::env::args().skip(1).peekable();
    // Only the dev image selects this mode: Docker already isolates its files from the editor.
    let is_isolated_container = args.next_if(|arg| arg == "--isolated-container").is_some();
    match args.next().as_deref() {
        Some("--sandbox") => {
            let workspace = args.next().context("Missing workspace")?;
            let parent_pid = args.next().context("Missing supervisor pid")?.parse()?;
            return exec_command(&workspace, parent_pid, &args.next().context("Missing command")?, is_isolated_container);
        }
        Some("--watch") => {
            let workspace = args.next().context("Missing workspace")?;
            return watch_session(&workspace, args.next().context("Missing session")?.parse()?);
        }
        Some(_) => anyhow::bail!("Unknown worker argument."),
        None => {}
    }
    let parent_pid = unsafe { libc::getppid() };
    ensure!(
        unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } == 0,
        "Cannot attach worker lifetime to its parent."
    );
    ensure!(unsafe { libc::getppid() } == parent_pid, "Worker parent exited.");
    // The worker leads its own session, and each command gets its own process group in it. Sandboxed children can
    // leave neither.
    if unsafe { libc::getsid(0) } != std::process::id() as i32 {
        ensure!(unsafe { libc::setsid() } >= 0, "Cannot create worker session.");
    }
    check_workspace_filesystem()?;
    let workspace = tempfile::Builder::new().prefix("kube-linux-sandbox-").tempdir_in("/dev/shm")?;
    let directory = workspace.path().to_path_buf();
    let path = directory.to_str().context("Invalid workspace path")?;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    std::os::unix::fs::chown(&directory, Some(SANDBOX_UID), Some(SANDBOX_UID))?;
    let directory_fd = File::open(&directory)?;
    // The watchdog has its own session and survives worker SIGKILL. Its stdin closes
    // automatically on worker exit, even on a crash; it then kills descendants and deletes files.
    let mut watchdog = Command::new(std::env::current_exe()?)
        .args(["--watch", path, &std::process::id().to_string()])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()?;
    let lifetime = watchdog.stdin.take().context("Missing watchdog pipe")?;
    // The watchdog owns cleanup from here so files remain until all sandbox processes are dead.
    let _workspace_path = workspace.keep();

    #[derive(Deserialize)]
    #[serde(tag = "operation", rename_all = "snake_case")]
    enum Request {
        Run { command: String, timeout_seconds: u64 },
        StopRun { run_id: u64 },
        ReadImage { path: String },
        ReadFile { path: String },
        WriteFile { path: String, base64: String },
    }
    // Requests run at the same time and can finish in any order, so each one carries an id that its reply repeats.
    #[derive(Deserialize)]
    struct RequestId {
        id: u64,
    }
    // openat2 checks every path component in the kernel, including during concurrent renames.
    // No absolute paths, '..' escapes, symlinks, device files, or FIFOs can expose editor files.
    fn read_workspace_file(directory: &File, path: &str, limit: usize) -> anyhow::Result<Vec<u8>> {
        let path = CString::new(path)?;
        let mut how: libc::open_how = unsafe { std::mem::zeroed() };
        how.flags = (libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NONBLOCK) as u64;
        how.resolve = libc::RESOLVE_BENEATH | libc::RESOLVE_NO_SYMLINKS;
        let fd = unsafe { libc::syscall(libc::SYS_openat2, directory.as_raw_fd(), path.as_ptr(), &how, size_of::<libc::open_how>()) };
        ensure!(fd >= 0, "Cannot read the file: {}", std::io::Error::last_os_error());
        let file = unsafe { File::from_raw_fd(fd as i32) };
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file() && metadata.len() > 0 && metadata.len() <= limit as u64,
            "The file is empty, too large, or not a regular file."
        );
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= limit, "The file grew past its limit.");
        Ok(bytes)
    }

    // Each request runs on its own thread, so a long command does not hold up the others. The threads borrow the
    // workspace from here.
    let (directory, directory_fd) = (&directory, &directory_fd);
    // A run whose id is here stops at its next check, the way one past its timeout does.
    let stopped_run_ids = &Mutex::new(HashSet::new());
    let lock_stopped_run_ids = || stopped_run_ids.lock().map_err(|_| anyhow::anyhow!("Cannot read the stopped runs."));
    std::thread::scope(|scope| -> anyhow::Result<()> {
        let mut input = std::io::stdin().lock();
        loop {
            // TODO: AI-PICKED-VALUE: 100 KB commands, 64 KB returned logs, and up to ten minutes per command.
            let mut line = String::new();
            // A written file may fill the 16 MiB file limit, plus base64 and JSON overhead.
            if input.by_ref().take(FILE_BYTES.div_ceil(3) * 4 + 1024).read_line(&mut line)? == 0 {
                break;
            }
            ensure!(line.ends_with('\n'), "Worker request exceeded its limit.");
            ensure!(watchdog.try_wait()?.is_none(), "Sandbox watchdog exited.");
            let RequestId { id } = serde_json::from_str(&line)?;
            scope.spawn(move || {
                let result = (|| -> anyhow::Result<serde_json::Value> {
                    match serde_json::from_str::<Request>(&line)? {
                        Request::Run { command, timeout_seconds } => {
                            ensure!(!command.trim().is_empty() && command.len() <= 100_000, "Command is empty or too large.");
                            ensure!((1..=600).contains(&timeout_seconds), "The timeout must be from 1 to 600 seconds.");
                            let mut log = tempfile::tempfile_in(&directory)?;
                            // The command leads its own process group, and nothing it starts can leave that group.
                            let mut child = Command::new(std::env::current_exe()?)
                                .args(is_isolated_container.then_some("--isolated-container"))
                                .args(["--sandbox", path, &std::process::id().to_string(), &command])
                                .env_clear()
                                .env("PATH", "/usr/bin:/bin:/usr/local/bin")
                                .env("HOME", path)
                                .env("TMPDIR", path)
                                .env("OMP_NUM_THREADS", "2")
                                .env("OPENBLAS_NUM_THREADS", "2")
                                .process_group(0)
                                .stdin(Stdio::null())
                                .stdout(log.try_clone()?)
                                .stderr(log.try_clone()?)
                                .spawn()?;
                            let started = Instant::now();
                            let is_timed_out = loop {
                                // Waiting without reaping the command keeps its group's id from being reused before the
                                // group is killed below.
                                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                                ensure!(
                                    unsafe { libc::waitid(libc::P_PID, child.id(), &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT) } == 0,
                                    "Cannot wait for the command."
                                );
                                if unsafe { info.si_pid() } != 0 {
                                    break false;
                                }
                                let stopped_run_ids_lock = lock_stopped_run_ids()?;
                                let is_stopped = stopped_run_ids_lock.contains(&id);
                                drop(stopped_run_ids_lock);
                                if is_stopped {
                                    break false;
                                }
                                if started.elapsed() > Duration::from_secs(timeout_seconds) {
                                    break true;
                                }
                                std::thread::sleep(Duration::from_millis(20));
                            };
                            // Killing the group stops a command that ran past its timeout, and anything a command left
                            // running in the background.
                            unsafe {
                                libc::kill(-(child.id() as i32), libc::SIGKILL);
                            }
                            let status = child.wait()?;
                            let mut stopped_run_ids_lock = lock_stopped_run_ids()?;
                            stopped_run_ids_lock.remove(&id);
                            drop(stopped_run_ids_lock);
                            log.rewind()?;
                            let mut output = Vec::new();
                            log.take(64 * 1024 + 1).read_to_end(&mut output)?;
                            let is_truncated = output.len() > 64 * 1024;
                            output.truncate(64 * 1024);
                            Ok(json!({"exit_code": status.code(), "output": String::from_utf8_lossy(&output), "is_truncated": is_truncated, "is_timed_out": is_timed_out}))
                        }
                        // Node stops a run whose tool call was stopped. The run then replies as usual, and Node ignores it.
                        Request::StopRun { run_id } => {
                            let mut stopped_run_ids_lock = lock_stopped_run_ids()?;
                            stopped_run_ids_lock.insert(run_id);
                            drop(stopped_run_ids_lock);
                            Ok(json!({}))
                        }
                        // Images reach the AI the way the editor's screenshots do, which are PNGs.
                        Request::ReadImage { path } => {
                            let bytes = read_workspace_file(&directory_fd, &path, MAX_IMAGE_BYTES)?;
                            let reader = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?;
                            ensure!(reader.format() == Some(image::ImageFormat::Png), "Use a PNG image.");
                            let (width, height) = reader.into_dimensions()?;
                            // TODO: AI-PICKED-VALUE: Permit contact sheets up to 4096 pixels per side.
                            ensure!(width > 0 && height > 0 && width <= 4096 && height <= 4096, "The image is too large.");
                            let (png, shown_width, shown_height) = shrink_png_for_ai(bytes, width, height)?;
                            Ok(json!({"base64": STANDARD.encode(png), "width": width, "height": height, "shown_width": shown_width, "shown_height": shown_height}))
                        }
                        // The file tools read a workspace file to send it to the editor.
                        Request::ReadFile { path } => {
                            let bytes = read_workspace_file(&directory_fd, &path, FILE_BYTES as usize)?;
                            Ok(json!({"base64": STANDARD.encode(bytes)}))
                        }
                        // The file tools write a file from the editor into the workspace, replacing any file already
                        // there. Like reads, openat2 keeps the write inside the workspace and refuses links.
                        Request::WriteFile { path, base64 } => {
                            let bytes = STANDARD.decode(base64)?;
                            ensure!(bytes.len() as u64 <= FILE_BYTES, "File is too large.");
                            let path = CString::new(path)?;
                            let mut how: libc::open_how = unsafe { std::mem::zeroed() };
                            how.flags = (libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC | libc::O_NONBLOCK) as u64;
                            how.mode = 0o644;
                            how.resolve = libc::RESOLVE_BENEATH | libc::RESOLVE_NO_SYMLINKS;
                            let fd = unsafe { libc::syscall(libc::SYS_openat2, directory_fd.as_raw_fd(), path.as_ptr(), &how, size_of::<libc::open_how>()) };
                            ensure!(fd >= 0, "Cannot write file: {}", std::io::Error::last_os_error());
                            let mut file = unsafe { File::from_raw_fd(fd as i32) };
                            ensure!(file.metadata()?.is_file(), "Only a regular file can be written.");
                            ensure!(
                                unsafe { libc::fchown(file.as_raw_fd(), SANDBOX_UID, SANDBOX_UID) } == 0,
                                "Cannot set the file's owner."
                            );
                            file.write_all(&bytes)?;
                            Ok(json!({}))
                        }
                    }
                })();
                let reply = match result {
                    Ok(value) => json!({"id": id, "result": value}),
                    Err(error) => json!({"id": id, "error": format!("{error:#}")}),
                };
                // Holding the lock for the whole line keeps replies from different threads apart.
                let mut stdout_lock = std::io::stdout().lock();
                if let Err(error) = writeln!(stdout_lock, "{reply}").and_then(|()| stdout_lock.flush()) {
                    eprintln!("Cannot send a reply: {error}");
                    std::process::exit(1);
                }
                drop(stdout_lock);
            });
        }
        // Closing the pipe tells the watchdog to end the session, which also stops any commands still running.
        drop(lifetime);
        Ok(())
    })
}

#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("linux_sandbox_worker runs on Linux. Use ./scripts/build_linux_sandbox.sh for the dev Docker image.")
}

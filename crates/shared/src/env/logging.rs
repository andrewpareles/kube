#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::{
    backtrace::Backtrace,
    fs::{self, File},
    io::{self, Seek, SeekFrom, Write},
    path::PathBuf,
};

const CLIENT_LOG_FILE_NAME: &str = "client.log";
const MAX_CLIENT_LOG_FILE_BYTES: u64 = 10 * 1024 * 1024;

pub fn initialize_logging() {
    create_logger_builder().init();
}

fn create_logger_builder() -> env_logger::Builder {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,rig::completions=warn"))
}

// logs the client's log to a file for debugging purposes, since there is no terminal to read
pub fn initialize_prod_logging() {
    match open_client_log_file() {
        Ok(Some((client_log_file, client_log_path))) => {
            #[cfg(unix)]
            {
                match redirect_process_output_to_client_log_file(&client_log_file) {
                    Ok(()) => {
                        create_logger_builder().init();
                        log::info!("Writing client logs to {}", client_log_path.display());
                        log::info!("Redirecting process stdout/stderr to {}", client_log_path.display());
                        install_client_panic_hook();
                        return;
                    }
                    Err(error) => {
                        create_logger_builder()
                            .target(env_logger::Target::Pipe(Box::new(ClientLogWriter {
                                client_log_file,
                                client_log_bytes_written: 0,
                            })))
                            .init();
                        log::info!("Writing client logs to {}", client_log_path.display());
                        log::error!("Could not redirect process stdout/stderr to the client log file: {}", error);
                        install_client_panic_hook();
                        return;
                    }
                }
            }

            #[cfg(not(unix))]
            {
                create_logger_builder()
                    .target(env_logger::Target::Pipe(Box::new(ClientLogWriter {
                        client_log_file,
                        client_log_bytes_written: 0,
                    })))
                    .init();
                log::info!("Writing client logs to {}", client_log_path.display());
                install_client_panic_hook();
            }
        }
        Ok(None) => {
            create_logger_builder().init();
            install_client_panic_hook();
        }
        Err(error) => {
            create_logger_builder().init();
            log::error!("Could not open the client log file: {}", error);
            install_client_panic_hook();
        }
    }
}

fn install_client_panic_hook() {
    // Install a panic hook here so crash reporting stays coupled to the logging setup
    // instead of leaking into each client entrypoint.
    let default_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let panic_payload = if let Some(payload_str) = panic_info.payload().downcast_ref::<&str>() {
            (*payload_str).to_string()
        } else if let Some(payload_string) = panic_info.payload().downcast_ref::<String>() {
            payload_string.clone()
        } else {
            "panic payload was not a string".to_string()
        };
        let panic_location = panic_info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_else(|| "unknown location".to_string());
        let current_thread = std::thread::current();
        let thread_name = current_thread.name().unwrap_or("unnamed");
        let panic_backtrace = Backtrace::force_capture();

        eprintln!("CLIENT PANIC on thread {thread_name} at {panic_location}: {panic_payload}");
        eprintln!("CLIENT PANIC BACKTRACE:\n{panic_backtrace}");

        default_panic_hook(panic_info);
    }));
}

struct ClientLogWriter {
    client_log_file: File,
    client_log_bytes_written: u64,
}

impl Write for ClientLogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        std::io::stderr().write_all(bytes)?;

        let bytes_len_u64 = bytes.len() as u64;
        if bytes_len_u64 >= MAX_CLIENT_LOG_FILE_BYTES {
            self.reset_client_log_file()?;
            let first_byte_to_keep = bytes.len() - MAX_CLIENT_LOG_FILE_BYTES as usize;
            self.client_log_file.write_all(&bytes[first_byte_to_keep..])?;
            self.client_log_bytes_written = MAX_CLIENT_LOG_FILE_BYTES;
            return Ok(bytes.len());
        }

        if self.client_log_bytes_written + bytes_len_u64 > MAX_CLIENT_LOG_FILE_BYTES {
            self.reset_client_log_file()?;
        }

        self.client_log_file.write_all(bytes)?;
        self.client_log_bytes_written += bytes_len_u64;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        std::io::stderr().flush()?;
        self.client_log_file.flush()
    }
}

impl ClientLogWriter {
    fn reset_client_log_file(&mut self) -> io::Result<()> {
        self.client_log_file.set_len(0)?;
        self.client_log_file.seek(SeekFrom::Start(0))?;
        self.client_log_bytes_written = 0;
        Ok(())
    }
}

fn open_client_log_file() -> io::Result<Option<(File, PathBuf)>> {
    let Some(client_logs_directory_path) = client_logs_directory_path() else {
        return Ok(None);
    };
    fs::create_dir_all(&client_logs_directory_path)?;

    let client_log_path = client_logs_directory_path.join(CLIENT_LOG_FILE_NAME);
    let client_log_file = File::options().create(true).write(true).truncate(true).open(&client_log_path)?;
    Ok(Some((client_log_file, client_log_path)))
}

#[cfg(unix)]
fn redirect_process_output_to_client_log_file(client_log_file: &File) -> io::Result<()> {
    // Redirect the real process output first so native crash text from Rust, V8, and other libraries
    // lands in the same file even when the app was launched from a deeplink or Finder.
    let client_log_fd = client_log_file.as_raw_fd();
    let stdout_dup_result = unsafe { libc::dup2(client_log_fd, libc::STDOUT_FILENO) };
    if stdout_dup_result == -1 {
        return Err(io::Error::last_os_error());
    }
    let stderr_dup_result = unsafe { libc::dup2(client_log_fd, libc::STDERR_FILENO) };
    if stderr_dup_result == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn client_logs_directory_path() -> Option<PathBuf> {
    let home_path = std::env::var_os("HOME")?;
    Some(PathBuf::from(home_path).join("Library/Logs/Kube.new"))
}

#[cfg(target_os = "windows")]
fn client_logs_directory_path() -> Option<PathBuf> {
    let local_app_data_path = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local_app_data_path).join("Kube.new/Logs"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn client_logs_directory_path() -> Option<PathBuf> {
    None
}

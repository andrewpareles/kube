//! Linux restrictions used in Edgegap and the dev Docker image. Missing kernel support is an error.
use anyhow::{Context, Result, ensure};
use landlock::{ABI, Access, AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus};
use seccompiler::{BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter, SeccompRule, apply_filter};
use std::{collections::BTreeMap, os::unix::process::CommandExt, path::Path, process::Command};

// TODO: AI-PICKED-VALUE: A dedicated unprivileged uid, 128 processes/threads, and 16 MiB per file fit the initial editor
// deployment and small game assets.
pub const SANDBOX_UID: u32 = 65534;
// Commands get 2 GiB of the editor container's 4096 MB, which leaves room for the editor, Node, and a screenshot.
pub const MEMORY_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_PROCESSES: u64 = 128;
pub const FILE_BYTES: u64 = 16 * 1024 * 1024;
const WORKSPACE_BYTES: u64 = 64 * 1024 * 1024;

pub fn check_workspace_filesystem() -> Result<()> {
    let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
    ensure!(
        unsafe { libc::statfs(c"/dev/shm".as_ptr(), stats.as_mut_ptr()) } == 0,
        "Cannot inspect the sandbox's tmpfs."
    );
    let stats = unsafe { stats.assume_init() };
    ensure!(
        stats.f_type == libc::TMPFS_MAGIC && stats.f_blocks * stats.f_bsize as u64 <= WORKSPACE_BYTES,
        "The sandbox requires /dev/shm to be a tmpfs of at most 64 MiB."
    );
    Ok(())
}

pub fn exec_command(workspace: &str, parent_pid: libc::pid_t, command: &str, is_isolated_container: bool) -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "The sandbox launcher must be able to drop to its dedicated uid."
    );
    // The worker can legitimately be PID 1 in the dev container; check its supplied pid.
    ensure!(unsafe { libc::getppid() } == parent_pid, "The command's supervisor already exited.");
    for (resource, limit) in [
        (libc::RLIMIT_AS, MEMORY_BYTES),
        (libc::RLIMIT_NPROC, MAX_PROCESSES),
        (libc::RLIMIT_FSIZE, FILE_BYTES),
        (libc::RLIMIT_CORE, 0),
    ] {
        ensure!(
            unsafe {
                libc::setrlimit(
                    resource,
                    &libc::rlimit {
                        rlim_cur: limit,
                        rlim_max: limit,
                    },
                )
            } == 0,
            "Cannot set the command's resource limit."
        );
    }
    // Oversized writes still fail with EFBIG, without terminating the whole process.
    ensure!(
        unsafe { libc::signal(libc::SIGXFSZ, libc::SIG_IGN) } != libc::SIG_ERR,
        "Cannot set the command's file-limit handling."
    );
    // Commands run at the lowest CPU priority, so they never slow the editor down. The command's own timeout bounds how
    // long they run.
    ensure!(
        unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 19) } == 0,
        "Cannot lower the command's priority."
    );
    // When the container runs out of memory, Linux stops commands first, like screenshots, so the editor keeps running.
    // A command can't change this after it drops to its uid.
    std::fs::write("/proc/self/oom_score_adj", "1000").context("Cannot mark the command to be stopped first.")?;
    ensure!(unsafe { libc::setgroups(0, std::ptr::null()) } == 0, "Cannot clear the command's groups.");
    ensure!(
        unsafe { libc::setgid(SANDBOX_UID) } == 0 && unsafe { libc::setuid(SANDBOX_UID) } == 0,
        "Cannot drop the command's privileges."
    );
    // setuid clears PDEATHSIG, so install it afterwards and check the parent has not exited.
    ensure!(
        unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } == 0,
        "Cannot attach the command's lifetime to its supervisor."
    );
    ensure!(unsafe { libc::getppid() } == parent_pid, "The command's supervisor exited.");
    ensure!(
        unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } == 0,
        "Cannot prevent privilege escalation."
    );

    // Edgegap shares its filesystem with the editor and always requires Landlock. The dev
    // container instead has an isolated, read-only filesystem with no editor files or mounts.
    if !is_isolated_container {
        let abi = ABI::V3;
        let all = AccessFs::from_all(abi);
        let mut ruleset = Ruleset::default().handle_access(all)?.create()?;
        for path in ["/usr", "/lib", "/lib64", "/etc/fonts", "/sys/devices/system/cpu"] {
            if Path::new(path).exists() {
                ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(path)?, AccessFs::from_read(abi)))?;
            }
        }
        for path in [
            "/etc/ld.so.cache",
            "/etc/localtime",
            "/proc/cpuinfo",
            "/proc/meminfo",
            "/dev/urandom",
            "/dev/random",
        ] {
            if Path::new(path).exists() {
                ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(path)?, AccessFs::ReadFile))?;
            }
        }
        ruleset = ruleset
            .add_rule(PathBeneath::new(PathFd::new("/dev/null")?, AccessFs::ReadFile | AccessFs::WriteFile))?
            .add_rule(PathBeneath::new(PathFd::new(workspace)?, all & !(AccessFs::MakeChar | AccessFs::MakeBlock)))?;
        let restriction = ruleset.restrict_self()?;
        ensure!(
            restriction.ruleset == RulesetStatus::FullyEnforced && restriction.no_new_privs,
            "The kernel cannot fully enforce the sandbox's filesystem restrictions."
        );
    }

    // Children inherit these restrictions. Keep them in their command's process group, inside the worker's session,
    // so the worker can stop one command and the watchdog can kill the entire session, including
    // detached/background shell commands.
    let mut rules = BTreeMap::new();
    for syscall in [
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_pidfd_getfd,
        libc::SYS_io_uring_setup,
        libc::SYS_setsid,
        libc::SYS_setpgid,
        libc::SYS_unshare,
        libc::SYS_setns,
        libc::SYS_mount,
        libc::SYS_bpf,
        libc::SYS_perf_event_open,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
        // Kernel IPC objects can outlive the command; anonymous memory files bypass the workspace quota.
        libc::SYS_shmget,
        libc::SYS_semget,
        libc::SYS_msgget,
        libc::SYS_mq_open,
        libc::SYS_memfd_create,
    ] {
        rules.insert(syscall, vec![]);
    }
    // Ordinary fork/exec is allowed. New namespaces are not; clone3 uses libc's clone fallback.
    for flag in [
        libc::CLONE_NEWUSER,
        libc::CLONE_NEWNS,
        libc::CLONE_NEWPID,
        libc::CLONE_NEWNET,
        libc::CLONE_NEWIPC,
        libc::CLONE_NEWUTS,
        libc::CLONE_NEWCGROUP,
    ] {
        rules.entry(libc::SYS_clone).or_default().push(SeccompRule::new(vec![SeccompCondition::new(
            0,
            SeccompCmpArgLen::Qword,
            SeccompCmpOp::MaskedEq(flag as u64),
            flag as u64,
        )?])?);
    }
    let filter: BpfProgram = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        std::env::consts::ARCH.try_into()?,
    )?
    .try_into()?;
    apply_filter(&filter)?;
    let filter: BpfProgram = SeccompFilter::new(
        [(libc::SYS_clone3, vec![])].into(),
        SeccompAction::Allow,
        SeccompAction::Errno(libc::ENOSYS as u32),
        std::env::consts::ARCH.try_into()?,
    )?
    .try_into()?;
    apply_filter(&filter)?;
    std::env::set_current_dir(workspace)?;
    // Only the workspace, installed tools, and explicitly supplied environment reach the shell.
    let error = Command::new("/bin/sh").args(["-c", command]).exec();
    Err(error).context("Cannot execute sandbox command")
}

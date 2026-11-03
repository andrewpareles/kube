//! This trusted process survives worker failure. Sandboxed processes cannot signal it or escape
//! their process group. No network listener or generated code runs here.
use crate::sandbox::MEMORY_BYTES;
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, time::Duration};

pub fn watch_session(workspace: &str, session: i32) -> Result<()> {
    // Count only live processes; zombies cannot write files or allocate more memory. Every process in the worker's
    // session belongs to the sandbox: the worker leads one process group, and each command leads another.
    fn read_bytes_of_group(session: i32, page_bytes: u64) -> Result<BTreeMap<i32, u64>> {
        let mut bytes_of_group = BTreeMap::new();
        for entry in std::fs::read_dir("/proc")?.filter_map(Result::ok) {
            let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
                continue;
            };
            let Some((_, fields)) = stat.rsplit_once(") ") else { continue };
            let fields: Vec<_> = fields.split_whitespace().collect();
            if fields.get(3).and_then(|value| value.parse::<i32>().ok()) == Some(session) && fields.first() != Some(&"Z") {
                let group: i32 = fields.get(2).context("Missing process group")?.parse()?;
                // Killing group 1 or lower would signal processes outside the sandbox.
                ensure!(group > 1, "Unexpected process group {group} in the sandbox session.");
                let bytes: &mut u64 = bytes_of_group.entry(group).or_insert(0);
                *bytes = bytes.saturating_add(
                    fields
                        .get(21)
                        .and_then(|value| value.parse::<u64>().ok())
                        .unwrap_or(0)
                        .saturating_mul(page_bytes),
                );
            }
        }
        Ok(bytes_of_group)
    }
    // A session of its own keeps the watchdog out of the session it kills.
    ensure!(unsafe { libc::setsid() } >= 0, "Cannot create watchdog session.");
    let page_bytes = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64;
    let mut lifetime = libc::pollfd {
        fd: 0,
        events: libc::POLLIN | libc::POLLHUP,
        revents: 0,
    };
    let result = (|| -> Result<()> {
        loop {
            // The pipe has no messages; EOF means the owner exited or closed the session.
            if unsafe { libc::poll(&mut lifetime, 1, 100) } != 0 {
                break;
            }
            // This is an aggregate RSS guard, not a kernel cgroup quota. Per-process rlimits
            // remain in force, but a rapid allocation can overshoot between these checks.
            let bytes_of_group = read_bytes_of_group(session, page_bytes)?;
            if bytes_of_group.values().sum::<u64>() > MEMORY_BYTES {
                // Only the command using the most memory is stopped. The workspace and the other commands keep going.
                let biggest_command = bytes_of_group.iter().filter(|(group, _)| **group != session).max_by_key(|(_, bytes)| **bytes);
                if let Some((group, _)) = biggest_command {
                    unsafe {
                        libc::kill(-group, libc::SIGKILL);
                    }
                }
            }
        }
        Ok(())
    })();
    // Each pass kills every group that still has a live process, which also catches a command the worker started
    // after the previous pass.
    loop {
        let bytes_of_group = read_bytes_of_group(session, page_bytes)?;
        if bytes_of_group.is_empty() {
            break;
        }
        for group in bytes_of_group.keys() {
            unsafe {
                libc::kill(-group, libc::SIGKILL);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    std::fs::remove_dir_all(workspace)?;
    result
}

//! Each run gets a CPU budget. Only time spent polling its future counts; awaits pause it.
//! A separate thread reads the JS thread's OS CPU clock and interrupts runaway V8 execution.

use deno_core::{JsRuntime, v8};
use std::{
    future::Future,
    io,
    sync::{Arc, Condvar, Mutex},
    task::Poll,
    thread::JoinHandle,
    time::Duration,
};

// Keep the previous execution limits, now measured in CPU time.
pub const JS_CPU_BUDGET: Duration = Duration::from_millis(100);
pub const TEMPORARY_SCRIPT_CPU_BUDGET: Duration = Duration::from_secs(30);
// TODO: AI-PICKED-VALUE: Check active execution every 10 ms to limit overshoot without busy-waiting.
const CHECK_INTERVAL: Duration = Duration::from_millis(10);
// TODO: AI-PICKED-VALUE: V8 gets 64 MB past the heap limit to finish the allocation that reached it before the script stops.
// Node gives its workers 16 MB the same way; more room covers a large array in flight.
const OUT_OF_MEMORY_HEADROOM_BYTES: usize = 64 * 1024 * 1024;

// ---------------------- Thread CPU handle ----------------------

/// A handle to one thread's CPU clock, readable by the watchdog thread.
/// The watchdog is joined before the JS thread can exit or release this handle.

pub struct ThreadCpuHandle {
    #[cfg(target_os = "macos")]
    thread: libc::mach_port_t,
    #[cfg(target_os = "linux")]
    clock_id: libc::clockid_t,
    #[cfg(windows)]
    thread: std::os::windows::io::OwnedHandle,
}

impl ThreadCpuHandle {
    pub fn for_current_thread() -> io::Result<Self> {
        #[cfg(target_os = "macos")]
        let cpu_handle = Self {
            // SAFETY: This creates an owned send right for the calling thread.
            thread: unsafe { mach2::mach_init::mach_thread_self() },
        };
        #[cfg(target_os = "linux")]
        let cpu_handle = {
            let mut clock_id = 0;
            // SAFETY: pthread_self is alive, and clock_id is a valid output pointer.
            let error = unsafe { libc::pthread_getcpuclockid(libc::pthread_self(), &mut clock_id) };
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error));
            }
            Self { clock_id }
        };
        #[cfg(windows)]
        let cpu_handle = {
            use std::os::windows::io::{FromRawHandle, OwnedHandle};
            use windows_sys::Win32::{Foundation::DuplicateHandle, System::Threading::*};
            let mut thread = std::ptr::null_mut();
            // SAFETY: Duplicate the pseudo-handle so it still names the JS thread on another thread.
            let did_succeed = unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    GetCurrentThread(),
                    GetCurrentProcess(),
                    &mut thread,
                    THREAD_QUERY_LIMITED_INFORMATION,
                    0,
                    0,
                )
            };
            if did_succeed == 0 {
                return Err(io::Error::last_os_error());
            }
            Self {
                thread: unsafe { OwnedHandle::from_raw_handle(thread) },
            }
        };
        cpu_handle.read()?;
        Ok(cpu_handle)
    }

    pub fn read(&self) -> io::Result<Duration> {
        #[cfg(target_os = "macos")]
        {
            let mut info = std::mem::MaybeUninit::<libc::thread_basic_info>::uninit();
            let mut count = libc::THREAD_BASIC_INFO_COUNT;
            // SAFETY: The owned port names the live JS thread; the buffer and count match this flavor.
            let error = unsafe { libc::thread_info(self.thread, libc::THREAD_BASIC_INFO as u32, info.as_mut_ptr().cast(), &mut count) };
            if error != libc::KERN_SUCCESS {
                return Err(io::Error::other(format!("thread_info failed: {error}")));
            }
            let info = unsafe { info.assume_init() };
            Ok(Duration::from_secs(info.user_time.seconds as u64 + info.system_time.seconds as u64)
                + Duration::from_micros(info.user_time.microseconds as u64 + info.system_time.microseconds as u64))
        }
        #[cfg(target_os = "linux")]
        {
            let mut time = std::mem::MaybeUninit::<libc::timespec>::uninit();
            // SAFETY: The JS thread stays alive, and time is a valid output buffer.
            if unsafe { libc::clock_gettime(self.clock_id, time.as_mut_ptr()) } != 0 {
                return Err(io::Error::last_os_error());
            }
            let time = unsafe { time.assume_init() };
            Ok(Duration::new(time.tv_sec as u64, time.tv_nsec as u32))
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::{Foundation::FILETIME, System::Threading::GetThreadTimes};
            let mut creation = FILETIME::default();
            let mut exit = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            // SAFETY: The handle is owned, and all four output pointers are valid.
            if unsafe { GetThreadTimes(self.thread.as_raw_handle(), &mut creation, &mut exit, &mut kernel, &mut user) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let kernel_ticks = (u64::from(kernel.dwHighDateTime) << 32) | u64::from(kernel.dwLowDateTime);
            let user_ticks = (u64::from(user.dwHighDateTime) << 32) | u64::from(user.dwLowDateTime);
            Ok(Duration::from_nanos((kernel_ticks + user_ticks) * 100))
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for ThreadCpuHandle {
    fn drop(&mut self) {
        // SAFETY: Release the send right obtained by mach_thread_self exactly once.
        unsafe { mach2::mach_port::mach_port_deallocate(mach2::traps::mach_task_self(), self.thread) };
    }
}

// ---------------------- JS CPU watchdog ----------------------

#[derive(Default)]
struct State {
    deadline: Option<Duration>,
    error: Option<anyhow::Error>,
    did_stop: bool,
}

struct Shared {
    cpu_handle: ThreadCpuHandle,
    isolate: v8::IsolateHandle,
    // A synchronous mutex is needed in Future::poll and by the OS watchdog thread.
    // Termination and disarming hold the same lock, so an old run cannot interrupt a new one.
    state_lock: Mutex<State>,
    wake: Condvar,
}

pub struct JsCpuWatchdog {
    shared_arc: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl JsCpuWatchdog {
    pub fn new(js_runtime: &mut JsRuntime) -> anyhow::Result<Self> {
        let shared_arc = Arc::new(Shared {
            cpu_handle: ThreadCpuHandle::for_current_thread()?,
            isolate: js_runtime.v8_isolate().thread_safe_handle(),
            state_lock: Mutex::new(State::default()),
            wake: Condvar::new(),
        });
        let worker_shared_arc = Arc::clone(&shared_arc);
        let worker = std::thread::Builder::new().name("js-cpu-watchdog".into()).spawn(move || {
            let mut state_lock = worker_shared_arc.state_lock.lock().unwrap();
            while !state_lock.did_stop {
                let Some(deadline) = state_lock.deadline.filter(|_| state_lock.error.is_none()) else {
                    state_lock = worker_shared_arc.wake.wait(state_lock).unwrap();
                    continue;
                };
                state_lock.error = match worker_shared_arc.cpu_handle.read() {
                    Ok(now) if now >= deadline => Some(anyhow::anyhow!("JavaScript exceeded its CPU time budget")),
                    Ok(_) => None,
                    Err(error) => Some(error.into()),
                };
                if state_lock.error.is_some() {
                    worker_shared_arc.isolate.terminate_execution();
                }
                let (next_state_lock, _) = worker_shared_arc.wake.wait_timeout(state_lock, CHECK_INTERVAL).unwrap();
                state_lock = next_state_lock;
            }
            drop(state_lock);
        })?;
        // A script that reaches its heap limit is stopped like one that runs out of CPU time, instead of V8 ending the process.
        let memory_shared_arc = Arc::clone(&shared_arc);
        js_runtime.add_near_heap_limit_callback(move |current_limit, _initial_limit| {
            memory_shared_arc.state_lock.lock().unwrap().error = Some(anyhow::anyhow!("JavaScript exceeded its memory limit"));
            memory_shared_arc.isolate.terminate_execution();
            current_limit + OUT_OF_MEMORY_HEADROOM_BYTES
        });
        Ok(Self {
            shared_arc,
            worker: Some(worker),
        })
    }

    pub async fn run<T>(js_runtime: &mut JsRuntime, budget: Duration, run: impl AsyncFnOnce(&mut JsRuntime) -> anyhow::Result<T>) -> anyhow::Result<T> {
        let shared_arc = Arc::clone(&js_runtime.op_state().borrow().borrow::<Self>().shared_arc);
        let future = run(js_runtime);
        let mut future = std::pin::pin!(future);
        let mut remaining = budget;
        std::future::poll_fn(|cx| {
            // The guard also disarms on panic. Nothing stays armed while this future is pending.
            let mut error = None;
            let active_poll = ActivePoll::start(&shared_arc, &mut remaining, &mut error)?;
            let result = future.as_mut().poll(cx);
            drop(active_poll);
            match error {
                Some(error) => Poll::Ready(Err(error)),
                None => result,
            }
        })
        .await
    }

    // For JS that has to finish without awaiting, like the step callbacks the client play loop runs inside a physics step.
    pub fn run_sync<T>(js_runtime: &mut JsRuntime, budget: Duration, run: impl FnOnce(&mut JsRuntime) -> anyhow::Result<T>) -> anyhow::Result<T> {
        let shared_arc = Arc::clone(&js_runtime.op_state().borrow().borrow::<Self>().shared_arc);
        let mut remaining = budget;
        let mut error = None;
        let active_poll = ActivePoll::start(&shared_arc, &mut remaining, &mut error)?;
        let result = run(js_runtime);
        drop(active_poll);
        match error {
            Some(error) => Err(error),
            None => result,
        }
    }
}

struct ActivePoll<'a> {
    shared: &'a Shared,
    started: Duration,
    remaining: &'a mut Duration,
    error: &'a mut Option<anyhow::Error>,
}

impl<'a> ActivePoll<'a> {
    // Arms the watchdog with what is left of the budget. Dropping the guard disarms it.
    fn start(shared: &'a Shared, remaining: &'a mut Duration, error: &'a mut Option<anyhow::Error>) -> io::Result<Self> {
        let started = shared.cpu_handle.read()?;
        let mut state_lock = shared.state_lock.lock().unwrap();
        assert!(state_lock.deadline.is_none(), "CPU budgets must not be nested");
        state_lock.deadline = Some(started + *remaining);
        shared.wake.notify_one();
        drop(state_lock);
        Ok(Self {
            shared,
            started,
            remaining,
            error,
        })
    }
}

impl Drop for ActivePoll<'_> {
    fn drop(&mut self) {
        let mut state_lock = self.shared.state_lock.lock().unwrap();
        *self.error = state_lock.error.take();
        match self.shared.cpu_handle.read() {
            Ok(now) => {
                *self.remaining = self.remaining.saturating_sub(now.saturating_sub(self.started));
                if self.remaining.is_zero() {
                    *self.error = Some(anyhow::anyhow!("JavaScript exceeded its CPU time budget"));
                }
            }
            Err(error) => *self.error = Some(error.into()),
        }
        state_lock.deadline = None;
        self.shared.isolate.cancel_terminate_execution();
        drop(state_lock);
    }
}

impl Drop for JsCpuWatchdog {
    fn drop(&mut self) {
        let mut state_lock = self.shared_arc.state_lock.lock().unwrap();
        state_lock.did_stop = true;
        self.shared_arc.wake.notify_one();
        drop(state_lock);
        // The clock must stop being read before the JS thread exits (especially on Linux).
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

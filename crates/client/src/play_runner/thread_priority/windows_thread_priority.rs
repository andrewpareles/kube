use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST};

// TODO: AI-PICKED-VALUE: HIGHEST, above every normal thread on the machine but short of TIME_CRITICAL, which can starve the window's thread.
pub fn raise_current_thread_priority() {
    if let Err(error) = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST) } {
        log::warn!("Could not raise the play loop's thread priority: {error}");
    }
}

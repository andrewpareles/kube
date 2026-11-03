// The play loop has to run the moment its tick comes, so it asks each OS to put its thread ahead of ordinary work.
#[cfg(target_os = "macos")]
mod macos_thread_priority;
#[cfg(target_os = "macos")]
pub use self::macos_thread_priority::raise_current_thread_priority;
#[cfg(target_os = "windows")]
mod windows_thread_priority;
#[cfg(target_os = "windows")]
pub use self::windows_thread_priority::raise_current_thread_priority;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn raise_current_thread_priority() {}

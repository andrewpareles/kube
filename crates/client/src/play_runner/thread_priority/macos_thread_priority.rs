// TODO: AI-PICKED-VALUE: USER_INTERACTIVE, the class macOS gives an app's main thread, so the play loop gets the fast cores like the window does.
pub fn raise_current_thread_priority() {
    let result = unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0) };
    if result != 0 {
        log::warn!("Could not raise the play loop's thread priority: error {result}");
    }
}

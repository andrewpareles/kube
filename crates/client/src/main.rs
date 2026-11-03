// this tells windows to open without a terminal window also opening first:
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![allow(non_snake_case)]

use client::app_runner::{AppRunner, start_txing_app_runner_inputs};
use shared::env::{
    initialize_env::initialize_env,
    logging::{initialize_logging, initialize_prod_logging},
};
use std::env::args;
use tokio::sync::mpsc;

#[tokio::main]
pub async fn main() {
    // The desktop app receives play links; opening it without a link opens the website.
    initialize_env();
    let should_open_separately = args().any(|arg| arg == "--open-separately");
    // A separate copy writes to the output it inherited from the copy that started it, which is that copy's log file,
    // so it never opens the file itself and wipes it.
    if should_open_separately {
        initialize_logging();
    } else {
        initialize_prod_logging();
    }
    // Windows wakes timers on its 15.6 ms clock by default, so the 60 Hz play loop's ticks landed up to a whole tick late.
    // A 1 ms clock keeps them within a few ms. Windows only honors it while the window can be seen.
    #[cfg(target_os = "windows")]
    {
        if unsafe { windows::Win32::Media::timeBeginPeriod(1) } != windows::Win32::Media::TIMERR_NOERROR {
            log::warn!("Could not set the Windows timer to 1 ms");
        }
    }
    let (app_runner_inputs_tx, app_runner_inputs_rx) = mpsc::unbounded_channel();
    start_txing_app_runner_inputs(app_runner_inputs_tx, should_open_separately).await;

    let mut app_runner = AppRunner::new_window(app_runner_inputs_rx);
    app_runner.create_and_run_event_loop_forever();
}

// await a shutdown signal from the OS
pub async fn await_shutdown_signal() {
    #[cfg(unix)]
    {
        let Ok(mut terminate_signal) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) else {
            let _ = tokio::signal::ctrl_c().await;
            log::info!("Received shutdown signal: SIGINT");
            return;
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                log::info!("Received shutdown signal: SIGINT");
            }
            _ = terminate_signal.recv() => {
                log::info!("Received shutdown signal: SIGTERM");
            }
        };
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        log::info!("Received shutdown signal: SIGINT");
    }
}

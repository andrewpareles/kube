use crate::deep_link_receiver::save_launch_link;
use anyhow::Context;
use cargo_packager_updater::{Config, WindowsConfig, WindowsUpdateInstallMode, check_update, semver::Version};
use shared::env::env_vars::{SITE_URL, UPDATER_PUBLIC_KEY};
use game_types::consts::CLIENT_UPDATER_PATH;
#[cfg(target_os = "macos")]
use std::process::Command;
use url::Url;

// returns true if update is available and the current app process should stop
pub async fn quit_and_install_update_if_available(launch_link: Option<Url>) -> anyhow::Result<bool> {
    // A dev build runs from the build folder, which the updater would replace with the released app.
    if cfg!(debug_assertions) {
        return Ok(false);
    }
    tokio::task::spawn_blocking(move || check_and_install_update_blocking(launch_link))
        .await
        .context("Client updater thread panicked")?
}

fn check_and_install_update_blocking(launch_link: Option<Url>) -> anyhow::Result<bool> {
    let updater_endpoint = format!("{}{}", SITE_URL.as_str(), CLIENT_UPDATER_PATH);
    let updater_endpoint = updater_endpoint.parse().context("Failed to parse updater endpoint")?;
    let updater_public_key = UPDATER_PUBLIC_KEY.as_str().to_string();
    let updater_config = Config {
        endpoints: vec![updater_endpoint],
        pubkey: updater_public_key,
        windows: Some(WindowsConfig {
            installer_args: None,
            install_mode: Some(WindowsUpdateInstallMode::Passive),
        }),
    };
    let current_version = Version::parse(env!("CARGO_PKG_VERSION")).expect("client version should always be a valid semver");
    let available_update = check_update(current_version, updater_config).context("Failed checking for client update")?;
    let Some(available_update) = available_update else {
        return Ok(false);
    };

    #[cfg(target_os = "macos")]
    let current_exe_path = std::env::current_exe().context("Failed getting current client executable path")?;

    let update_bytes = available_update.download().context("Failed downloading client update")?;
    // The updated app opens without this app's launch link, so save the link for it.
    if let Some(launch_link) = launch_link {
        save_launch_link(&launch_link).context("Failed saving the launch link for the updated client")?;
    }
    available_update.install(update_bytes).context("Failed installing client update")?;

    // macOS replaces the app bundle in place, so reopen the updated app here.
    #[cfg(target_os = "macos")]
    Command::new(current_exe_path).spawn().context("Failed relaunching updated client")?;

    Ok(true)
}

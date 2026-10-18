//! Updates from the project's GitHub releases. Every release carries a
//! `latest.json` with the version, the notes and a package per platform,
//! each signed with the project's update key. A package is installed only if
//! its signature matches the public key built into the app.
//!
//! Desktop uses Tauri's updater (an installer on Windows, the app bundle on
//! macOS, the AppImage on Linux). Android downloads the APK, checks it the
//! same way and hands it to the system installer, which also requires the
//! same signing key as the installed app.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

type CmdResult<T> = Result<T, String>;

/// Where people download Ropeway by hand (`.deb`/`.rpm` installs, iOS …).
pub const RELEASES_PAGE: &str = "https://github.com/FreyByyy/ropewayapp/releases/latest";

#[derive(Serialize, Clone, Debug)]
pub struct UpdateInfo {
    pub version: String,
    /// The release notes, in the format of CHANGELOG.md.
    pub notes: String,
    /// Ropeway can install it itself; otherwise the UI links to the page.
    pub can_install: bool,
    pub page: String,
}

#[derive(Serialize, Clone)]
struct Progress {
    done: u64,
    total: Option<u64>,
}

fn report(app: &AppHandle, done: u64, total: Option<u64>) {
    let _ = app.emit("ropeway-update-progress", Progress { done, total });
}

/// Opens the page with every release in the browser.
#[tauri::command]
pub fn open_releases_page(app: AppHandle) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url(RELEASES_PAGE, None::<&str>).map_err(|e| e.to_string())
}

/// A newer version, if there is one.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> CmdResult<Option<UpdateInfo>> {
    imp::check(&app).await.map_err(|e| e.to_string())
}

/// Downloads and installs the newer version, then restarts into it (on
/// Android: opens the system installer).
#[tauri::command]
pub async fn install_update(app: AppHandle) -> CmdResult<()> {
    imp::install(&app).await.map_err(|e| e.to_string())
}

#[cfg(desktop)]
mod imp {
    use anyhow::{anyhow, Result};
    use tauri::{AppHandle, Manager};
    use tauri_plugin_updater::UpdaterExt;

    use super::{report, UpdateInfo, RELEASES_PAGE};

    /// Packages from a system package manager update through it.
    fn can_install() -> bool {
        !cfg!(target_os = "linux") || std::env::var_os("APPIMAGE").is_some()
    }

    pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>> {
        let Some(update) = app.updater()?.check().await? else { return Ok(None) };
        Ok(Some(UpdateInfo {
            version: update.version.clone(),
            notes: update.body.clone().unwrap_or_default(),
            can_install: can_install(),
            page: RELEASES_PAGE.into(),
        }))
    }

    pub async fn install(app: &AppHandle) -> Result<()> {
        if !can_install() {
            return Err(anyhow!("this copy is updated by the system package manager"));
        }
        let update = app.updater()?.check().await?.ok_or_else(|| anyhow!("no update is available"))?;
        let mut done = 0u64;
        update
            .download_and_install(
                |chunk, total| {
                    done += chunk as u64;
                    report(app, done, total);
                },
                || {},
            )
            .await?;
        // Windows closes the app for the installer, which starts the new
        // version; elsewhere the new version is in place now.
        if let Some(node) = app.try_state::<ropeway_core::Node>() {
            crate::ipc::unpublish(node.data_dir());
        }
        app.restart();
    }
}

#[cfg(target_os = "android")]
mod imp {
    use anyhow::{anyhow, bail, Context, Result};
    use base64::Engine;
    use serde::Deserialize;
    use tauri::{AppHandle, Manager};

    use super::{report, UpdateInfo, RELEASES_PAGE};

    #[derive(Deserialize)]
    struct Manifest {
        version: String,
        #[serde(default)]
        notes: String,
        platforms: std::collections::HashMap<String, Package>,
    }

    #[derive(Deserialize)]
    struct Package {
        url: String,
        signature: String,
    }

    /// The key in `latest.json` for this phone's APK.
    fn platform() -> &'static str {
        if cfg!(target_arch = "x86_64") {
            "android-x86_64"
        } else {
            "android-aarch64"
        }
    }

    /// The updater settings in tauri.conf.json, shared with desktop.
    fn config(app: &AppHandle) -> Result<(String, String)> {
        let updater = app.config().plugins.0.get("updater").ok_or_else(|| anyhow!("updates are not configured"))?;
        let pubkey = updater["pubkey"].as_str().ok_or_else(|| anyhow!("no update key"))?.to_owned();
        let endpoint = updater["endpoints"][0].as_str().ok_or_else(|| anyhow!("no update address"))?.to_owned();
        Ok((pubkey, endpoint))
    }

    fn newer(remote: &str, current: &str) -> bool {
        let parse = |v: &str| -> Vec<u64> { v.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect() };
        parse(remote) > parse(current)
    }

    async fn manifest(app: &AppHandle) -> Result<Option<Manifest>> {
        let (_, endpoint) = config(app)?;
        let m: Manifest = reqwest::get(&endpoint).await?.error_for_status()?.json().await?;
        let current = app.package_info().version.to_string();
        Ok((newer(&m.version, &current) && m.platforms.contains_key(platform())).then_some(m))
    }

    pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>> {
        Ok(manifest(app).await?.map(|m| UpdateInfo {
            version: m.version,
            notes: m.notes,
            can_install: true,
            page: RELEASES_PAGE.into(),
        }))
    }

    /// Checks `data` against a signature made by `tauri signer sign`: both
    /// the key and the signature are base64 of minisign's text files.
    fn verify(pubkey: &str, signature: &str, data: &[u8]) -> Result<()> {
        let b64 = base64::engine::general_purpose::STANDARD;
        let key_text = String::from_utf8(b64.decode(pubkey.trim())?)?;
        let key_line = key_text.lines().nth(1).ok_or_else(|| anyhow!("bad update key"))?;
        let key = minisign_verify::PublicKey::from_base64(key_line.trim())?;
        let sig_text = String::from_utf8(b64.decode(signature.trim())?)?;
        let sig = minisign_verify::Signature::decode(&sig_text)?;
        key.verify(data, &sig, false).context("the update's signature does not match")?;
        Ok(())
    }

    pub async fn install(app: &AppHandle) -> Result<()> {
        let (pubkey, _) = config(app)?;
        let m = manifest(app).await?.ok_or_else(|| anyhow!("no update is available"))?;
        let package = &m.platforms[platform()];
        let mut response = reqwest::get(&package.url).await?.error_for_status()?;
        let total = response.content_length();
        let mut data = Vec::with_capacity(total.unwrap_or(0).min(256 << 20) as usize);
        while let Some(chunk) = response.chunk().await? {
            data.extend_from_slice(&chunk);
            if data.len() > 512 << 20 {
                bail!("the update is too large");
            }
            report(app, data.len() as u64, total);
        }
        verify(&pubkey, &package.signature, &data)?;
        let dir = app.path().app_cache_dir()?.join("updates");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("Ropeway-{}.apk", m.version));
        std::fs::write(&path, &data)?;
        crate::android::install_apk(&path.to_string_lossy())
    }
}

#[cfg(not(any(desktop, target_os = "android")))]
mod imp {
    use anyhow::{bail, Result};
    use tauri::AppHandle;

    use super::UpdateInfo;

    pub async fn check(_app: &AppHandle) -> Result<Option<UpdateInfo>> {
        Ok(None)
    }

    pub async fn install(_app: &AppHandle) -> Result<()> {
        bail!("updates come from the App Store")
    }
}

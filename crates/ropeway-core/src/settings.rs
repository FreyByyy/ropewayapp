use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TrustedDevice {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub device_name: String,
    /// UI language code: "ru" or "en".
    pub language: String,
    pub autostart: bool,
    pub trusted: Vec<TrustedDevice>,
    /// Addresses added by hand ("192.168.1.20:47801") for networks where
    /// broadcast discovery is blocked. Reconnected automatically.
    pub manual_peers: Vec<String>,
    /// Where received files go. `None`: Downloads\Ropeway.
    pub download_dir: Option<PathBuf>,
    /// Folder shown to everyone on the network. `None`: Documents\Ropeway Shared.
    pub shared_dir: Option<PathBuf>,
    pub sharing_enabled: bool,
    /// Only trusted devices may see the shared folder and ask for changes.
    pub shared_trusted_only: bool,
    /// Video settings last used for screen sharing; also used when someone
    /// asks to see or control this screen.
    pub screen_video: crate::screen::VideoSettings,
    /// UI appearance (theme, light/dark, accent, motion). Owned by the UI.
    pub appearance: serde_json::Value,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_name: default_device_name(),
            language: "ru".into(),
            autostart: true,
            trusted: Vec::new(),
            manual_peers: Vec::new(),
            download_dir: None,
            shared_dir: None,
            sharing_enabled: true,
            shared_trusted_only: false,
            screen_video: Default::default(),
            appearance: serde_json::Value::Null,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        write_atomic(path, &serde_json::to_vec_pretty(self)?)
    }

    pub fn is_trusted(&self, id: &str) -> bool {
        self.trusted.iter().any(|t| t.id == id)
    }

    pub fn download_dir(&self) -> PathBuf {
        #[cfg(target_os = "android")]
        if self.download_dir.is_none() {
            return PathBuf::from(ANDROID_STORAGE).join("Download").join("Ropeway");
        }
        self.download_dir.clone().unwrap_or_else(|| {
            dirs::download_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(std::env::temp_dir)
                .join("Ropeway")
        })
    }

    pub fn shared_dir(&self) -> PathBuf {
        #[cfg(target_os = "android")]
        if self.shared_dir.is_none() {
            return PathBuf::from(ANDROID_STORAGE).join("Documents").join("Ropeway Shared");
        }
        self.shared_dir.clone().unwrap_or_else(|| {
            dirs::document_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(std::env::temp_dir)
                .join("Ropeway Shared")
        })
    }
}

/// Shared storage on Android. Apps may create files in its Download and
/// Documents folders without a storage permission (Android 11+).
#[cfg(target_os = "android")]
const ANDROID_STORAGE: &str = "/storage/emulated/0";

fn default_device_name() -> String {
    // Android's host name is "localhost"; the phone's model says more.
    #[cfg(target_os = "android")]
    if let Some(model) = std::process::Command::new("getprop")
        .arg("ro.product.model")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
    {
        return model;
    }
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "Ropeway PC".into())
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp: PathBuf = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

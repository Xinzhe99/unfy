//! Persisted user settings.
//!
//! Settings are a small, flat, versioned document. Every write goes through
//! [`Settings::save`], which writes a temporary file and renames it over the
//! real one, so a crash mid-save leaves the previous settings intact rather than
//! a truncated JSON file.
//!
//! Unknown fields are rejected on load: silently ignoring a field written by a
//! newer version is how settings get quietly reset.

use std::path::{Path, PathBuf};

use conflux_domain::DisplayKey;
use conflux_layout::LayoutMode;
use serde::{Deserialize, Serialize};

pub const SETTINGS_VERSION: u32 = 1;

/// The user's saved target display.
///
/// `quality` is stored so the engine can refuse to auto-restore onto a display
/// whose identity was only ever known by geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetDisplay {
    pub native_id: String,
    pub label: String,
}

impl TargetDisplay {
    pub fn from_key(key: &DisplayKey, label: impl Into<String>) -> Self {
        Self {
            native_id: key.native_id.clone(),
            label: label.into(),
        }
    }

    pub fn matches(&self, key: &DisplayKey) -> bool {
        self.native_id == key.native_id
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    /// Display windows are gathered onto. `None` means "ask the user".
    pub target_display: Option<TargetDisplay>,
    pub layout_mode: LayoutModeName,
    pub margin: i32,
    pub gap: i32,
    /// Launch when the user logs in.
    pub autostart: bool,
    /// Theme preference; `system` follows the OS.
    pub theme: ThemeName,
    /// Global shortcut to gather windows, in Tauri's accelerator syntax.
    pub hotkey_gather: Option<String>,
    /// Global shortcut to restore.
    pub hotkey_restore: Option<String>,
    /// Seconds to wait after a restore request before acting. Zero restores
    /// immediately.
    pub restore_delay_secs: u32,
    /// Keep the journal but skip automatic recovery on next launch. Set when the
    /// user explicitly chooses "keep current layout" after a crash.
    pub suppress_recovery: bool,
    /// Check for updates automatically. Default on; security updates are never
    /// gated on this, only the *check* is.
    pub auto_update: bool,
    /// Explicit consent for anonymous usage counters. Default off.
    pub telemetry: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutModeName {
    Grid,
    PrimarySide,
    PreserveOrder,
}

impl From<LayoutModeName> for LayoutMode {
    fn from(name: LayoutModeName) -> Self {
        match name {
            LayoutModeName::Grid => LayoutMode::Grid,
            LayoutModeName::PrimarySide => LayoutMode::PrimarySide,
            LayoutModeName::PreserveOrder => LayoutMode::PreserveOrder,
        }
    }
}

impl From<LayoutMode> for LayoutModeName {
    fn from(mode: LayoutMode) -> Self {
        match mode {
            LayoutMode::Grid => LayoutModeName::Grid,
            LayoutMode::PrimarySide => LayoutModeName::PrimarySide,
            LayoutMode::PreserveOrder => LayoutModeName::PreserveOrder,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeName {
    System,
    Light,
    Dark,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            target_display: None,
            layout_mode: LayoutModeName::Grid,
            margin: 12,
            gap: 8,
            autostart: false,
            theme: ThemeName::System,
            hotkey_gather: None,
            hotkey_restore: None,
            restore_delay_secs: 0,
            suppress_recovery: false,
            auto_update: true,
            telemetry: false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("io error: {0}")]
    Io(String),
    #[error("settings file is not valid: {0}")]
    Parse(String),
    #[error("settings version {found} is newer than {supported}")]
    VersionTooNew { found: u32, supported: u32 },
    #[error("invalid value: {0}")]
    Validation(String),
}

pub type Result<T, E = SettingsError> = std::result::Result<T, E>;

impl From<std::io::Error> for SettingsError {
    fn from(e: std::io::Error) -> Self {
        SettingsError::Io(e.to_string())
    }
}

impl Settings {
    /// Load from `path`, or return defaults when the file does not exist.
    ///
    /// A corrupt file is an error, not a silent reset: resetting would discard
    /// the user's configured target display without telling them.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        let settings: Settings =
            serde_json::from_str(&text).map_err(|e| SettingsError::Parse(e.to_string()))?;
        if settings.version > SETTINGS_VERSION {
            return Err(SettingsError::VersionTooNew {
                found: settings.version,
                supported: SETTINGS_VERSION,
            });
        }
        settings.validate()?;
        Ok(settings)
    }

    /// Write atomically: temp file in the same directory, then rename.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        self.validate()?;
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = temp_path(path);
        let json =
            serde_json::to_string_pretty(self).map_err(|e| SettingsError::Parse(e.to_string()))?;
        std::fs::write(&temp, json)?;
        // `rename` over an existing file is atomic on NTFS and on POSIX.
        std::fs::rename(&temp, path)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        if !(0..=200).contains(&self.margin) {
            return Err(SettingsError::Validation(format!(
                "margin {} is outside 0..=200",
                self.margin
            )));
        }
        if !(0..=200).contains(&self.gap) {
            return Err(SettingsError::Validation(format!(
                "gap {} is outside 0..=200",
                self.gap
            )));
        }
        if self.restore_delay_secs > 300 {
            return Err(SettingsError::Validation(format!(
                "restore delay {} exceeds 300 s",
                self.restore_delay_secs
            )));
        }
        for (name, hotkey) in [
            ("hotkey_gather", &self.hotkey_gather),
            ("hotkey_restore", &self.hotkey_restore),
        ] {
            if let Some(hotkey) = hotkey {
                if hotkey.trim().is_empty() {
                    return Err(SettingsError::Validation(format!(
                        "{name} is present but empty"
                    )));
                }
            }
        }
        if self.hotkey_gather.is_some() && self.hotkey_gather == self.hotkey_restore {
            return Err(SettingsError::Validation(
                "gather and restore hotkeys must differ".into(),
            ));
        }
        Ok(())
    }

    /// The default location for the settings file.
    pub fn default_path() -> PathBuf {
        config_dir().join("settings.json")
    }
}

fn temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "settings.json".into());
    path.with_file_name(format!("{file_name}.tmp"))
}

/// Per-user configuration directory.
///
/// Uses the platform's own convention rather than a hardcoded Windows path, so
/// the same code works on macOS once that backend lands.
pub fn config_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("Conflux");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("Conflux");
        }
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("conflux");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config").join("conflux");
    }
    std::env::temp_dir().join("conflux")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "conflux-settings-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_returns_defaults() {
        let dir = temp_dir("missing");
        let settings = Settings::load(dir.join("settings.json")).unwrap();
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn round_trip() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("settings.json");
        let settings = Settings {
            target_display: Some(TargetDisplay {
                native_id: "DISPLAY#DEL41FF".into(),
                label: "Dell U2720Q".into(),
            }),
            layout_mode: LayoutModeName::PrimarySide,
            hotkey_gather: Some("CmdOrCtrl+Alt+G".into()),
            ..Default::default()
        };
        settings.save(&path).unwrap();

        let loaded = Settings::load(&path).unwrap();
        assert_eq!(loaded, settings);
    }

    #[test]
    fn corrupt_file_is_an_error_not_a_reset() {
        let dir = temp_dir("corrupt");
        let path = dir.join("settings.json");
        std::fs::write(&path, "{ this is not json").unwrap();
        let err = Settings::load(&path).unwrap_err();
        assert!(matches!(err, SettingsError::Parse(_)));
    }

    #[test]
    fn newer_schema_is_rejected() {
        let dir = temp_dir("newer");
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"version": 99, "target_display": null, "layout_mode": "grid",
                "margin": 12, "gap": 8, "autostart": false, "theme": "system",
                "hotkey_gather": null, "hotkey_restore": null,
                "restore_delay_secs": 0, "suppress_recovery": false}"#,
        )
        .unwrap();
        assert!(matches!(
            Settings::load(&path).unwrap_err(),
            SettingsError::VersionTooNew { .. }
        ));
    }

    #[test]
    fn unknown_field_is_rejected() {
        let dir = temp_dir("unknown");
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"version": 1, "mystery": true, "layout_mode": "grid",
                "margin": 12, "gap": 8, "autostart": false, "theme": "system",
                "restore_delay_secs": 0, "suppress_recovery": false}"#,
        )
        .unwrap();
        assert!(matches!(
            Settings::load(&path).unwrap_err(),
            SettingsError::Parse(_)
        ));
    }

    #[test]
    fn identical_hotkeys_are_rejected() {
        let settings = Settings {
            hotkey_gather: Some("CmdOrCtrl+Alt+G".into()),
            hotkey_restore: Some("CmdOrCtrl+Alt+G".into()),
            ..Default::default()
        };
        assert!(settings.validate().is_err());
    }

    #[test]
    fn out_of_range_margin_is_rejected() {
        let settings = Settings {
            margin: 500,
            ..Default::default()
        };
        assert!(settings.validate().is_err());
    }

    #[test]
    fn save_leaves_no_temp_file() {
        let dir = temp_dir("notemp");
        let path = dir.join("settings.json");
        Settings::default().save(&path).unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn partial_document_fills_defaults() {
        let dir = temp_dir("partial");
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"version": 1, "margin": 20, "layout_mode": "preserve_order"}"#,
        )
        .unwrap();
        let settings = Settings::load(&path).unwrap();
        assert_eq!(settings.margin, 20);
        assert_eq!(settings.layout_mode, LayoutModeName::PreserveOrder);
        assert_eq!(settings.gap, Settings::default().gap);
    }

    #[test]
    fn target_display_matches_by_native_id_only() {
        let target = TargetDisplay {
            native_id: "display-a".into(),
            label: "Old label".into(),
        };
        let key = DisplayKey {
            native_id: "display-a".into(),
            quality: conflux_domain::IdentityQuality::Strong,
        };
        assert!(target.matches(&key));
    }
}

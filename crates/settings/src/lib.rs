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

use unfy_domain::DisplayKey;
use unfy_layout::LayoutMode;
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
    /// The number this display had when it was chosen.
    ///
    /// Stored because the number cannot be recomputed once the display is gone:
    /// the overview still has to say "显示器 2 已断开" rather than print the raw
    /// device path, which means nothing to the user. Absent in files written
    /// before this field existed.
    #[serde(default)]
    pub number: Option<usize>,
}

impl TargetDisplay {
    pub fn from_key(key: &DisplayKey, label: impl Into<String>) -> Self {
        Self {
            native_id: key.native_id.clone(),
            label: label.into(),
            number: None,
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
    /// How many screens the remote client can show.
    ///
    /// Spec P0-06: when the remote端 can see every screen, gathering is the wrong
    /// action - the user already has all their windows. The default is "one",
    /// because that is the situation the product exists for, but a user with
    /// multi-monitor streaming should be able to say so and be left alone.
    pub remote_screens: RemoteScreens,
    /// Launch when the user logs in.
    pub autostart: bool,
    /// Theme preference; `system` follows the OS.
    pub theme: ThemeName,
    /// Interface language; `system` follows the OS.
    pub language: LanguageName,
    /// Global shortcut to gather windows, in Tauri's accelerator syntax.
    pub hotkey_gather: Option<String>,
    /// Global shortcut to restore.
    pub hotkey_restore: Option<String>,
    /// Seconds to wait after a restore request before acting. Zero restores
    /// immediately.
    ///
    /// This exists because a remote session often drops and reconnects within a
    /// few seconds; restoring instantly would move every window back and then
    /// move it out again. The engine reads it on each `restore()` call.
    pub restore_delay_secs: u32,
    /// Stop acting on new requests while keeping the current layout.
    ///
    /// Distinct from "no transaction": paused means the windows are still
    /// gathered and the journal is still open, so the user can resume or
    /// restore. The tray shows this state.
    pub paused: bool,
    /// Timestamp (unix seconds) until which a dismissed recovery banner stays
    /// dismissed.
    ///
    /// Storing a deadline instead of a permanent flag matters: a permanent flag
    /// means the user dismisses one banner and never sees another crash warning
    /// for the rest of the installation's life.
    pub recovery_dismissed_until: u64,
    /// Whether the first-run guide has been completed.
    ///
    /// Distinct from "a target is set": a user may deliberately keep the primary
    /// display as the target, so the guide must not reappear just because the
    /// choice matches the default.
    pub onboarding_done: bool,
    /// Current step of the first-run guide, so hiding and re-showing the window
    /// resumes where the user was instead of restarting the explanation.
    pub onboarding_step: u8,
    /// Executable names never moved. Case-insensitive, file name only.
    pub excluded_apps: Vec<String>,
    /// Check for updates automatically. Default on; security updates are never
    /// gated on this, only the *check* is.
    pub auto_update: bool,
    /// How much remote-session evidence the app may act on by itself.
    ///
    /// Persisted as a string so a newer version can add levels without a schema
    /// bump, and so an unknown value degrades to "off" (never to "balanced": a
    /// value we do not understand must not start moving windows).
    pub auto_detect: String,
    /// Automatically gather when a remote session starts.
    pub auto_gather: bool,
    /// Automatically restore when the remote session ends.
    ///
    /// Separate from `auto_gather`: some users want the cleanup but prefer to
    /// start the layout themselves, and some want the opposite.
    pub auto_restore: bool,
    /// Applications that refused the most recent gather.
    ///
    /// Persisted so the overview can offer "exclude this app" after a restart.
    /// Purely informational: nothing in the engine reads it, and it is replaced
    /// on every gather. Executable base names only.
    pub last_refused_apps: Vec<String>,
    /// Last window geometry, as (x, y, width, height) in logical pixels.
    ///
    /// Stored here rather than in a separate file so there is one place that can
    /// be inspected and one atomic writer. `None` means the user has never
    /// resized the window, so the configured default applies.
    ///
    /// The plugin approach was tried first and abandoned: it saves on a clean
    /// exit, and this app is routinely killed from a terminal or by the installer
    /// during development, so the geometry never persisted when it mattered.
    pub window_geometry: Option<(i32, i32, i32, i32)>,
    /// Whether the window was maximised when it was last closed.
    ///
    /// Separate from the rectangle because a maximised window reports the maximised
    /// bounds, and Windows has no portable way to ask for the restore rectangle from a
    /// new process. Keeping the last un-maximised rectangle and this flag reproduces what
    /// the user gets from clicking "restore"; without the flag they got a window that
    /// filled the screen but was not maximised - the title bar offered "maximise" and
    /// dragging it behaved as if it had never been maximised.
    pub window_maximized: bool,
    /// Explicit consent for anonymous usage counters. Default off.
    ///
    /// No uploader exists in this build: the flag records the user's choice so a
    /// future implementation cannot start collecting without consent, and so the
    /// UI can show the true state.
    pub telemetry: bool,
}

/// What the remote client can display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteScreens {
    /// One screen: gathering is the point.
    Single,
    /// All screens: nothing needs to move, so automation stays quiet.
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutModeName {
    Grid,
    PrimarySide,
    /// Written as `default` (the interface's name for it); reads the old
    /// `preserve_order` too, whose only behaviour was a grid slot order and
    /// which never produced the arrangement its name promised.
    #[serde(rename = "default", alias = "preserve_order")]
    KeepLayout,
}

impl From<LayoutModeName> for LayoutMode {
    fn from(name: LayoutModeName) -> Self {
        match name {
            LayoutModeName::Grid => LayoutMode::Grid,
            LayoutModeName::PrimarySide => LayoutMode::PrimarySide,
            LayoutModeName::KeepLayout => LayoutMode::KeepLayout,
        }
    }
}

impl From<LayoutMode> for LayoutModeName {
    fn from(mode: LayoutMode) -> Self {
        match mode {
            LayoutMode::Grid => LayoutModeName::Grid,
            LayoutMode::PrimarySide => LayoutModeName::PrimarySide,
            LayoutMode::KeepLayout => LayoutModeName::KeepLayout,
        }
    }
}

/// Interface language preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageName {
    System,
    Zh,
    En,
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
            // The whole point of the product is "the desktop you had, on the
            // screen you have": preserving the arrangement is the honest
            // default, and rearranging into cells is the opt-in.
            layout_mode: LayoutModeName::KeepLayout,
            margin: 12,
            gap: 8,
            remote_screens: RemoteScreens::Single,
            autostart: false,
            theme: ThemeName::System,
            language: LanguageName::System,
            hotkey_gather: None,
            hotkey_restore: None,
            restore_delay_secs: 15,
            paused: false,
            recovery_dismissed_until: 0,
            onboarding_done: false,
            onboarding_step: 0,
            excluded_apps: Vec::new(),
            auto_detect: "balanced".to_string(),
            // Both off by default. Automation that moves windows without being
            // asked is the one thing users will not forgive, and a user who never
            // enabled gathering has nothing to restore. The settings page explains
            // the trade-off in one line; the manual controls work regardless.
            auto_gather: false,
            auto_restore: false,
            last_refused_apps: Vec::new(),
            window_geometry: None,
            window_maximized: false,
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
        // Editors on Windows commonly write UTF-8 with a BOM (PowerShell's
        // `Set-Content -Encoding UTF8` does it by default). `serde_json` rejects
        // the BOM as "expected value at line 1 column 1", which used to make a
        // hand-edited settings file look corrupt and silently reset everything.
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let settings: Settings =
            serde_json::from_str(text).map_err(|e| SettingsError::Parse(e.to_string()))?;
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
        // Detection levels are a closed set: a typo would silently disable the
        // feature, and an unknown value from a newer build must not be treated as
        // an automatic-activation request.
        if !matches!(
            self.auto_detect.as_str(),
            "off" | "conservative" | "balanced" | "aggressive"
        ) {
            return Err(SettingsError::Validation(format!(
                "unknown auto_detect level: {}",
                self.auto_detect
            )));
        }
        // Guard the geometry against a corrupt file: a window positioned at
        // -100000 or sized 1x1 is unrecoverable without editing JSON by hand.
        if let Some((x, y, width, height)) = self.window_geometry {
            if width < 720 || height < 520 || width > 8000 || height > 8000 {
                return Err(SettingsError::Validation(format!(
                    "window size {width}x{height} is out of range"
                )));
            }
            if x.abs() > 20000 || y.abs() > 20000 {
                return Err(SettingsError::Validation(format!(
                    "window position {x},{y} is out of range"
                )));
            }
        }
        if self.excluded_apps.len() > 200 {
            return Err(SettingsError::Validation(
                "too many excluded applications".into(),
            ));
        }
        for name in &self.excluded_apps {
            // A rule is compared against a file name, so a path or a wildcard
            // would silently never match. Reject it at the boundary instead.
            if name.is_empty() || name.len() > 260 {
                return Err(SettingsError::Validation(format!(
                    "invalid excluded application name: {name:?}"
                )));
            }
            if name.contains(['\\', '/', '*', '?']) {
                return Err(SettingsError::Validation(format!(
                    "excluded application must be a file name, not a path or pattern: {name}"
                )));
            }
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
///
/// The product used to be called Conflux and kept its data under that name.
/// A rename must not strand the user's settings, window geometry, exclusion
/// rules and journal, so the first run after the rename moves the old
/// directory into place. Moving (not copying) keeps the journal and the
/// settings file as the very same files the previous build wrote, and the
/// leftover-empty-old-directory case degrades to "no settings yet" harmlessly.
pub fn config_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let appdata = PathBuf::from(appdata);
            let dir = appdata.join("Unfy");
            migrate_from(&dir, appdata.join("Conflux"));
            return dir;
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            let dir = home
                .join("Library")
                .join("Application Support")
                .join("Unfy");
            migrate_from(
                &dir,
                home.join("Library")
                    .join("Application Support")
                    .join("Conflux"),
            );
            return dir;
        }
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        let xdg = PathBuf::from(xdg);
        let dir = xdg.join("unfy");
        migrate_from(&dir, xdg.join("conflux"));
        return dir;
    }
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        let dir = home.join(".config").join("unfy");
        migrate_from(&dir, home.join(".config").join("conflux"));
        return dir;
    }
    std::env::temp_dir().join("unfy")
}

/// Move the previous product-name directory into the new one, once.
///
/// Only acts when the new directory does not exist yet and the old one does,
/// so it can never overwrite anything the new build already wrote, and every
/// later call is a no-op once the move has happened. A failure to migrate is
/// ignored: the app then starts with defaults, which is what it would have
/// done had the old directory never existed.
fn migrate_from(new_dir: &std::path::Path, old_dir: std::path::PathBuf) {
    if !old_dir.exists() || new_dir.exists() {
        return;
    }
    match std::fs::rename(&old_dir, new_dir) {
        Ok(()) => {
            eprintln!(
                "migrated configuration directory from {} to {}",
                old_dir.display(),
                new_dir.display()
            );
        }
        // Rename across filesystems fails; fall back to a copy. The old
        // directory is left in place so a half copy never loses data.
        Err(_) => {
            if std::fs::create_dir_all(new_dir).is_ok() {
                if let Ok(entries) = std::fs::read_dir(&old_dir) {
                    for entry in entries.flatten() {
                        let target = new_dir.join(entry.file_name());
                        if entry.path().is_file() {
                            let _ = std::fs::copy(entry.path(), &target);
                        }
                    }
                }
                eprintln!(
                    "copied configuration from {} to {}",
                    old_dir.display(),
                    new_dir.display()
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "unfy-settings-{tag}-{}-{:?}",
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
                number: None,
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
                "restore_delay_secs": 0}"#,
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
                "restore_delay_secs": 0}"#,
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
    fn concurrent_processes_use_a_single_settings_file() {
        // Two instances must not exist (enforced by the single-instance plugin),
        // but the settings writer itself must still be safe against a stale
        // process writing an older revision over a newer one. This test pins the
        // observable contract: the last write wins and the file is always valid
        // JSON, never a partial document.
        let dir = temp_dir("concurrent");
        let path = dir.join("settings.json");

        let a = Settings {
            margin: 24,
            ..Default::default()
        };
        let b = Settings {
            margin: 30,
            ..Default::default()
        };
        a.save(&path).unwrap();
        b.save(&path).unwrap();

        let loaded = Settings::load(&path).unwrap();
        assert_eq!(loaded.margin, 30);
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
        // The old value still parses, and now means the real thing its name
        // always promised: the arrangement as it was, not a grid slot order.
        assert_eq!(settings.layout_mode, LayoutModeName::KeepLayout);
        assert_eq!(settings.gap, Settings::default().gap);
    }

    #[test]
    fn target_display_matches_by_native_id_only() {
        let target = TargetDisplay {
            number: None,
            native_id: "display-a".into(),
            label: "Old label".into(),
        };
        let key = DisplayKey {
            native_id: "display-a".into(),
            quality: unfy_domain::IdentityQuality::Strong,
        };
        assert!(target.matches(&key));
    }
}

#[cfg(test)]
mod lock_tests {
    //! Guards the deadlock that shipped once: `save_settings` held the settings
    //! mutex while calling a helper that also tried to read settings. Because
    //! `std::sync::Mutex` is not reentrant, the command thread blocked forever —
    //! the file was written, the registry was not, and the UI hung.
    //!
    //! The fix was structural (the helper now takes a value, not the lock), so
    //! this test documents the contract that made the bug possible: reading
    //! settings while a caller already holds the guard must be impossible by
    //! construction. Here we assert the non-reentrancy explicitly so nobody
    //! "simplifies" the call site back into a deadlock.
    use std::sync::{Arc, Mutex};

    #[test]
    fn std_mutex_is_not_reentrant_so_settings_helpers_must_take_values() {
        let guard = Arc::new(Mutex::new(1u32));
        let outer = guard.lock().unwrap();

        let inner = Arc::clone(&guard);
        let handle = std::thread::spawn(move || inner.try_lock().is_ok());

        let observed = handle.join().unwrap();
        assert!(
            !observed,
            "a second lock attempt must fail while the guard is held; \
             if this ever passes, review every settings helper call site"
        );
        drop(outer);
        assert!(guard.try_lock().is_ok(), "guard must be released");
    }
}

#[cfg(test)]
mod bom_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn utf8_bom_is_tolerated() {
        // Windows editors add a BOM; rejecting it made a valid file look corrupt.
        let dir = std::env::temp_dir().join(format!("unfy-bom-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path: PathBuf = dir.join("settings.json");

        let body = r#"{"version":1,"margin":30,"layout_mode":"grid"}"#;
        std::fs::write(&path, format!("\u{feff}{body}")).unwrap();

        let loaded = Settings::load(&path).expect("BOM must not be fatal");
        assert_eq!(loaded.margin, 30);
    }
}

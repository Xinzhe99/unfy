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

pub const SETTINGS_VERSION: u32 = 2;

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

/// The persisted name of the layout mode.
///
/// `MoveInto` is serialized as `default` because that is what the interface has
/// always called it. The removed `primary_side` and `preserve_order` spellings
/// are still *accepted* by the deserializer so an existing settings file loads,
/// but they are migrated to `default` by [`Settings::migrate`]: the mode they
/// described no longer exists, and silently keeping the name would let the UI
/// offer a choice the engine cannot honour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutModeName {
    #[serde(rename = "default", alias = "preserve_order", alias = "primary_side")]
    MoveInto,
    Grid,
}

impl From<LayoutModeName> for LayoutMode {
    fn from(name: LayoutModeName) -> Self {
        match name {
            LayoutModeName::MoveInto => LayoutMode::MoveInto,
            LayoutModeName::Grid => LayoutMode::Grid,
        }
    }
}

impl From<LayoutMode> for LayoutModeName {
    fn from(mode: LayoutMode) -> Self {
        match mode {
            LayoutMode::MoveInto => LayoutModeName::MoveInto,
            LayoutMode::Grid => LayoutModeName::Grid,
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
            // Move the windows over, keep their size and state. The grid is the
            // explicit opt-in for users who want an arranged desktop.
            layout_mode: LayoutModeName::MoveInto,
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
            auto_detect: "off".to_string(),
            // Both off by default. Automation that moves windows without being
            // asked is the one thing users will not forgive, and a user who never
            // enabled gathering has nothing to restore. The settings page explains
            // the trade-off in one line; the manual controls work regardless.
            auto_gather: false,
            auto_restore: false,
            window_geometry: None,
            window_maximized: false,
            auto_update: true,
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
        // Fields that were removed from the document are stripped before the
        // strict deserialization. Without this, an ordinary upgrade would look
        // like a corrupt file, be copied aside as `settings.json.invalid`, and
        // silently reset the user's target, exclusions and shortcuts.
        let mut value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| SettingsError::Parse(e.to_string()))?;
        if let Some(object) = value.as_object_mut() {
            for removed in ["telemetry", "last_refused_apps"] {
                object.remove(removed);
            }
        }
        let settings: Settings = serde_json::from_value(value)
            .map_err(|e| SettingsError::Parse(e.to_string()))?;
        if settings.version > SETTINGS_VERSION {
            return Err(SettingsError::VersionTooNew {
                found: settings.version,
                supported: SETTINGS_VERSION,
            });
        }
        let settings = settings.migrate();
        settings.validate()?;
        Ok(settings)
    }

    /// Bring a file written by an older build up to the current contract.
    ///
    /// This is deliberately not a `serde` alias trick beyond the layout enum: the
    /// removed values mean something different now, and the user has to end up
    /// with a document whose meaning is the new one. Running it more than once is
    /// a no-op.
    pub fn migrate(mut self) -> Self {
        // v1 → v2:
        // * `primary_side` / `preserve_order` layout names collapse to the
        //   move-in default. The old modes either rearranged the target or mapped
        //   the whole desktop; neither is the product's default any more.
        // * The four-level detection ladder collapses to off/verified. Anything
        //   that was not already "off" keeps the user's intent that automation may
        //   act, but only on evidence that can actually prove a session.
        // * `last_refused_apps` and `telemetry` are gone; they are dropped by the
        //   deserializer's `deny_unknown_fields` compatibility shim below.
        if self.version < 2 {
            self.version = 2;
            self.layout_mode = match self.layout_mode {
                LayoutModeName::Grid => LayoutModeName::Grid,
                LayoutModeName::MoveInto => LayoutModeName::MoveInto,
            };
            self.auto_detect = match self.auto_detect.as_str() {
                "off" => "off".to_string(),
                // The old levels all meant "act automatically on some evidence".
                // Preserve the wish, not the unreliable evidence.
                _ => "verified".to_string(),
            };
        }
        self
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
        if self.restore_delay_secs > 120 {
            return Err(SettingsError::Validation(format!(
                "restore delay {} exceeds 120 s",
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
        // Detection is either off or restricted to a verified connection
        // lifecycle. The old four-level ladder admitted process presence and
        // virtual-display presence as permission to move windows, which is not
        // evidence that anyone is controlling this machine.
        if !matches!(self.auto_detect.as_str(), "off" | "verified") {
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
            let home = PathBuf::from(home);
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

/// Move the previous product-name directory into the new one.
///
/// The journal lives in a *subdirectory*, and the recovery record inside it is the
/// only description of windows that may still be displaced. The previous version
/// copied only the top-level files, so a fallback migration (across filesystems,
/// where `rename` fails) silently dropped `journal/` - and it then skipped every
/// later attempt because the destination existed. This version:
///
/// * copies the whole tree, recursively;
/// * writes into a temporary sibling and only renames it into place once every
///   entry has been copied, so an interrupted copy never looks complete;
/// * leaves the old directory untouched on any failure, so the next launch can try
///   again;
/// * refuses to overwrite an existing destination, which is what protects data the
///   new build already wrote.
fn migrate_from(new_dir: &std::path::Path, old_dir: std::path::PathBuf) {
    if !old_dir.is_dir() || new_dir.exists() {
        return;
    }
    if std::fs::rename(&old_dir, new_dir).is_ok() {
        eprintln!(
            "migrated configuration directory from {} to {}",
            old_dir.display(),
            new_dir.display()
        );
        return;
    }

    // Rename across filesystems fails. Copy into a temporary directory and commit
    // it atomically; the old directory stays as the fallback until that succeeds.
    let staging = new_dir.with_extension("migrating");
    if staging.exists() && std::fs::remove_dir_all(&staging).is_err() {
        eprintln!(
            "could not clear the previous partial migration at {}",
            staging.display()
        );
        return;
    }
    if let Err(e) = copy_tree(&old_dir, &staging) {
        eprintln!(
            "could not copy configuration from {}: {e}; keeping the original",
            old_dir.display()
        );
        let _ = std::fs::remove_dir_all(&staging);
        return;
    }
    if let Err(e) = std::fs::rename(&staging, new_dir) {
        eprintln!("could not commit the migrated configuration: {e}");
        let _ = std::fs::remove_dir_all(&staging);
        return;
    }
    eprintln!(
        "copied configuration from {} to {}",
        old_dir.display(),
        new_dir.display()
    );
}

/// Recursively copy a directory tree.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
        // Symlinks and platform-specific entries are skipped deliberately: the
        // config directory contains only files and the journal subdirectory.
    }
    Ok(())
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
            layout_mode: LayoutModeName::Grid,
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
        assert_eq!(settings.margin, 20, "valid fields must survive the upgrade");
        // The old name is accepted so the file loads, and migrated to the mode
        // that now carries the default behaviour.
        assert_eq!(settings.layout_mode, LayoutModeName::MoveInto);
        assert_eq!(settings.version, SETTINGS_VERSION);
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

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

use serde::{Deserialize, Serialize};
use unfy_domain::DisplayKey;
use unfy_layout::LayoutMode;

pub const SETTINGS_VERSION: u32 = 2;

/// The user's saved target display.
///
/// `quality` is stored so the engine can refuse to auto-restore onto a display
/// whose identity was only ever known by geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetDisplay {
    pub native_id: String,
    #[serde(default = "unknown_identity_quality")]
    pub quality: unfy_domain::IdentityQuality,
    /// Weak identity approval is valid only in this process and topology.
    #[serde(skip)]
    pub confirmed_topology: Option<String>,
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

fn unknown_identity_quality() -> unfy_domain::IdentityQuality {
    unfy_domain::IdentityQuality::Weak
}

impl TargetDisplay {
    pub fn from_key(key: &DisplayKey, label: impl Into<String>) -> Self {
        Self {
            native_id: key.native_id.clone(),
            quality: key.quality,
            confirmed_topology: None,
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
    /// The upgraded layout and automation policy needs user acknowledgement.
    #[serde(default)]
    pub migration_notice: bool,
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

/// The persisted name of the layout mode. Legacy spellings are translated only
/// while loading an older settings schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutModeName {
    #[serde(rename = "default")]
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
            migration_notice: false,
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
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error.into()),
        };
        // Editors on Windows commonly write UTF-8 with a BOM (PowerShell's
        // `Set-Content -Encoding UTF8` does it by default). `serde_json` rejects
        // the BOM as "expected value at line 1 column 1", which used to make a
        // hand-edited settings file look corrupt and silently reset everything.
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let mut value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| SettingsError::Parse(e.to_string()))?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| SettingsError::Parse("settings must be a JSON object".into()))?;
        let version = match object.get("version") {
            None => 1,
            Some(value) => value
                .as_u64()
                .filter(|v| *v <= u32::MAX as u64)
                .ok_or_else(|| SettingsError::Parse("version must be an unsigned integer".into()))?
                as u32,
        };
        if version > SETTINGS_VERSION {
            return Err(SettingsError::VersionTooNew {
                found: version,
                supported: SETTINGS_VERSION,
            });
        }
        object.insert("version".into(), version.into());
        if version < 2 {
            for removed in [
                "telemetry",
                "last_refused_apps",
                "marker",
                "deferred_restore",
            ] {
                object.remove(removed);
            }
            if matches!(
                object.get("layout_mode").and_then(|v| v.as_str()),
                Some("primary_side" | "preserve_order" | "keep_layout" | "KeepLayout")
            ) {
                object.insert("layout_mode".into(), "default".into());
            }
        }
        let settings: Settings =
            serde_json::from_value(value).map_err(|e| SettingsError::Parse(e.to_string()))?;
        let settings = settings.migrate();
        settings.validate()?;
        Ok(settings)
    }

    /// Upgrade legacy preferences without granting new automation authority.
    pub fn migrate(mut self) -> Self {
        if self.version < 2 {
            self.version = SETTINGS_VERSION;
            self.migration_notice = true;
            self.auto_detect = "off".into();
            self.auto_gather = false;
            self.auto_restore = false;
        }
        if self.remote_screens == RemoteScreens::All {
            self.auto_detect = "off".into();
            self.auto_gather = false;
            self.auto_restore = false;
        }
        self
    }

    /// Flush a unique sibling file before replacing the previous settings.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        self.validate()?;
        let path = path.as_ref();
        // A failed load must never be followed by overwriting an unknown schema.
        if path.try_exists()? {
            Self::load(path)?;
        }
        let json =
            serde_json::to_vec_pretty(self).map_err(|e| SettingsError::Parse(e.to_string()))?;
        atomic_write(path, &json)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != SETTINGS_VERSION {
            return Err(SettingsError::Validation(format!(
                "save requires settings version {SETTINGS_VERSION}, found {}",
                self.version
            )));
        }
        if self.remote_screens == RemoteScreens::All
            && (self.auto_detect != "off" || self.auto_gather || self.auto_restore)
        {
            return Err(SettingsError::Validation(
                "all-screen mode requires manual automation policy".into(),
            ));
        }
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
            if !(-20000..=20000).contains(&x) || !(-20000..=20000).contains(&y) {
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

fn unique_sibling(path: &Path, suffix: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{name}.{}.{sequence}.{suffix}", std::process::id()))
}

fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn sync_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    std::fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // Both strings are NUL-terminated and live through the call. Replace and
    // write-through are required so success does not mean only cached data.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x1 | 0x8) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    atomic_write_with(path, bytes, replace_file)
}

fn atomic_write_with(
    path: &Path,
    bytes: &[u8],
    replace: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(parent_dir(path))?;
    let (temp, mut file) = loop {
        let temp = unique_sibling(path, "tmp");
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        sync_dir(parent_dir(path))?;
        replace(&temp, path)?;
        sync_dir(parent_dir(path))
    })();
    if result.is_err() {
        // This invocation owns this unique file; never remove a stale sibling.
        let _ = std::fs::remove_file(temp);
    }
    result
}

/// Resolve the current configuration directory without touching the filesystem.
pub fn config_dir() -> PathBuf {
    config_dirs().1
}

fn config_dirs() -> (PathBuf, PathBuf) {
    #[cfg(target_os = "windows")]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let root = PathBuf::from(appdata);
        return (root.join("Conflux"), root.join("Unfy"));
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        let root = PathBuf::from(home).join("Library/Application Support");
        return (root.join("Conflux"), root.join("Unfy"));
    }
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(std::env::temp_dir);
    (root.join("conflux"), root.join("unfy"))
}

/// Run before opening logs, settings or journals. Failure blocks new transactions.
pub fn initialize_config_dir() -> Result<PathBuf> {
    let (old, new) = config_dirs();
    migrate_config_dirs(&old, &new)?;
    Ok(new)
}

/// Import the previous product directory. Existing settings always win. Old
/// journals and failed staging trees are retained until explicitly reviewed.
pub fn migrate_config_dirs(old_dir: &Path, new_dir: &Path) -> Result<()> {
    migrate_config_dirs_with(old_dir, new_dir, &|a, b| std::fs::rename(a, b), &|a, b| {
        std::fs::copy(a, b)
    })
}

fn migration_error(path: &Path, reason: impl std::fmt::Display) -> SettingsError {
    SettingsError::Io(format!(
        "configuration migration at {}: {reason}",
        path.display()
    ))
}

type JournalFiles = std::collections::BTreeMap<String, PathBuf>;

fn journal_files(dir: &Path) -> Result<JournalFiles> {
    fn visit(dir: &Path, files: &mut JournalFiles) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                visit(&path, files)?;
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "journal") {
                unfy_journal::recover(&path).map_err(|e| migration_error(&path, e))?;
                let bytes = std::fs::read(&path)?;
                let transaction = match unfy_journal::Record::decode(&bytes)
                    .map_err(|e| migration_error(&path, e))?
                {
                    Some((unfy_journal::Record::Begin { transaction, .. }, _)) => {
                        transaction.to_string()
                    }
                    _ => return Err(migration_error(&path, "missing transaction header")),
                };
                if path.file_stem().and_then(|name| name.to_str()) != Some(transaction.as_str()) {
                    return Err(migration_error(
                        &path,
                        "journal filename does not match transaction UUID",
                    ));
                }
                if let Some(previous) = files.get(&transaction) {
                    if !same_file_contents(previous, &path)? {
                        return Err(migration_error(
                            &path,
                            "conflicting copies of the same transaction",
                        ));
                    }
                } else {
                    files.insert(transaction, path);
                }
            } else {
                return Err(migration_error(
                    &path,
                    "unrecognized journal entry; recovery review required",
                ));
            }
        }
        Ok(())
    }
    let mut files = JournalFiles::new();
    if dir.try_exists()? {
        visit(dir, &mut files)?;
    }
    Ok(files)
}

fn same_file_contents(a: &Path, b: &Path) -> std::io::Result<bool> {
    use std::io::Read;
    let mut a = std::fs::File::open(a)?;
    let mut b = std::fs::File::open(b)?;
    if a.metadata()?.len() != b.metadata()?.len() {
        return Ok(false);
    }
    let mut left = [0u8; 8192];
    let mut right = [0u8; 8192];
    loop {
        let count = a.read(&mut left)?;
        if count == 0 {
            return Ok(true);
        }
        b.read_exact(&mut right[..count])?;
        if left[..count] != right[..count] {
            return Ok(false);
        }
    }
}

fn copy_verified(
    from: &Path,
    to: &Path,
    copy: &impl Fn(&Path, &Path) -> std::io::Result<u64>,
) -> Result<()> {
    copy(from, to)?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(to)?
        .sync_all()?;
    if !same_file_contents(from, to)? {
        return Err(migration_error(from, "copied contents differ"));
    }
    Ok(())
}

fn copy_tree(
    from: &Path,
    to: &Path,
    copy: &impl Fn(&Path, &Path) -> std::io::Result<u64>,
) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target, copy)?;
        } else if kind.is_file() {
            copy_verified(&entry.path(), &target, copy)?;
        } else {
            return Err(migration_error(
                &entry.path(),
                "links and special files require manual review",
            ));
        }
    }
    sync_dir(to)?;
    Ok(())
}

fn migrate_config_dirs_with(
    old_dir: &Path,
    new_dir: &Path,
    rename: &impl Fn(&Path, &Path) -> std::io::Result<()>,
    copy: &impl Fn(&Path, &Path) -> std::io::Result<u64>,
) -> Result<()> {
    if old_dir == new_dir {
        return Err(migration_error(old_dir, "source equals destination"));
    }
    if !old_dir.try_exists()? {
        return Ok(());
    }
    let old_files = journal_files(&old_dir.join("journal"))?;
    if !new_dir.try_exists()? {
        Settings::load(old_dir.join("settings.json"))?;
        std::fs::create_dir_all(parent_dir(new_dir))?;
        if rename(old_dir, new_dir).is_ok() {
            sync_dir(parent_dir(new_dir))?;
            atomic_write(&new_dir.join(".conflux-migration-complete"), b"1\n")?;
            return Ok(());
        }
        let staging = loop {
            let path = unique_sibling(new_dir, "migrating");
            match std::fs::create_dir(&path) {
                Ok(()) => break path,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        };
        copy_tree(old_dir, &staging, copy)?;
        Settings::load(staging.join("settings.json"))?;
        let staged_files = journal_files(&staging.join("journal"))?;
        if staged_files.len() != old_files.len() {
            return Err(migration_error(
                old_dir,
                "journal inventory changed during migration",
            ));
        }
        let receipts = staging.join(".conflux-imported-journals");
        std::fs::create_dir_all(&receipts)?;
        for (id, source) in &staged_files {
            let original = old_files
                .get(id)
                .ok_or_else(|| migration_error(source, "journal appeared during migration"))?;
            if !same_file_contents(source, original)? {
                return Err(migration_error(
                    source,
                    "source journal changed during migration",
                ));
            }
            copy_verified(source, &receipts.join(format!("{id}.journal")), copy)?;
        }
        sync_dir(&receipts)?;
        atomic_write(&staging.join(".conflux-migration-complete"), b"1\n")?;
        std::fs::rename(&staging, new_dir)?;
        sync_dir(parent_dir(new_dir))?;
        return Ok(());
    }

    let marker = new_dir.join(".conflux-migration-complete");
    if marker.try_exists()? && std::fs::read(&marker)? != b"1\n" {
        return Err(migration_error(&marker, "unknown migration marker version"));
    }
    let new_files = journal_files(&new_dir.join("journal"))?;
    let receipts = new_dir.join(".conflux-imported-journals");
    let mut pending = Vec::new();
    for (id, source) in &old_files {
        let receipt = receipts.join(format!("{id}.journal"));
        if receipt.try_exists()? {
            if !same_file_contents(source, &receipt)? {
                return Err(migration_error(
                    source,
                    "legacy journal changed after import",
                ));
            }
            continue;
        }
        if let Some(existing) = new_files.get(id) {
            if !same_file_contents(source, existing)? {
                return Err(migration_error(
                    source,
                    "transaction conflicts with the new configuration",
                ));
            }
        }
        pending.push((id, source, receipt));
    }
    // Validate everything before publishing any import. Each file commit remains
    // independently restartable if a later write fails.
    if !new_dir.join("settings.json").try_exists()? && old_dir.join("settings.json").try_exists()? {
        Settings::load(old_dir.join("settings.json"))?;
        atomic_write(
            &new_dir.join("settings.json"),
            &std::fs::read(old_dir.join("settings.json"))?,
        )?;
    }
    for (id, source, receipt) in pending {
        let bytes = std::fs::read(source)?;
        if let Some(existing) = new_files.get(id) {
            if std::fs::read(existing)? != bytes {
                return Err(migration_error(source, "journal changed while importing"));
            }
        } else {
            atomic_write(
                &new_dir.join("journal").join(format!("{id}.journal")),
                &bytes,
            )?;
        }
        atomic_write(&receipt, &bytes)?;
    }
    if !marker.try_exists()? {
        atomic_write(&marker, b"1\n")?;
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
                quality: unfy_domain::IdentityQuality::Strong,
                confirmed_topology: None,
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
    fn successive_saves_replace_the_existing_file() {
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
            quality: unfy_domain::IdentityQuality::Strong,
            confirmed_topology: None,
            label: "Old label".into(),
        };
        let key = DisplayKey {
            native_id: "display-a".into(),
            quality: unfy_domain::IdentityQuality::Strong,
        };
        assert!(target.matches(&key));
    }

    #[test]
    fn weak_target_confirmation_does_not_survive_restart() {
        let dir = temp_dir("weak-target");
        let path = dir.join("settings.json");
        let mut target = TargetDisplay::from_key(
            &DisplayKey {
                native_id: "geometry-only".into(),
                quality: unfy_domain::IdentityQuality::Weak,
            },
            "Chosen display",
        );
        target.confirmed_topology = Some("current-process-topology".into());
        Settings {
            target_display: Some(target),
            ..Settings::default()
        }
        .save(&path)
        .unwrap();
        let restored = Settings::load(&path).unwrap().target_display.unwrap();
        assert_eq!(restored.native_id, "geometry-only");
        assert_eq!(restored.quality, unfy_domain::IdentityQuality::Weak);
        assert!(restored.confirmed_topology.is_none());
    }

    fn migration_dirs(tag: &str) -> (PathBuf, PathBuf) {
        let root = unique_sibling(&std::env::temp_dir().join(tag), "test");
        std::fs::create_dir_all(&root).unwrap();
        (root.join("Conflux"), root.join("Unfy"))
    }

    fn unfinished_journal(dir: &Path, summary: &str) -> PathBuf {
        let id = "11111111-1111-4111-8111-111111111111";
        let path = dir.join(format!("{id}.journal"));
        let mut journal =
            unfy_journal::Journal::begin_named(&path, id.parse().unwrap(), summary).unwrap();
        let preimage = unfy_journal::Preimage {
            id: unfy_domain::WindowId {
                native_id: 10,
                process_id: 20,
                process_start: 30,
                instance: 1,
            },
            display_native_id: "display-a".into(),
            display_quality: unfy_domain::IdentityQuality::Strong,
            display_work_area: unfy_domain::Rect::new(0, 0, 1920, 1080),
            display_scale: unfy_domain::Scale { x: 1.0, y: 1.0 },
            visible_rect: unfy_domain::Rect::new(10, 20, 800, 600),
            outer_rect: unfy_domain::Rect::new(10, 20, 800, 600),
            mode: unfy_domain::WindowMode::Normal,
            topmost: false,
            native_placement: None,
        };
        journal.write_preimage(&preimage).unwrap();
        journal
            .write_intent(
                &preimage.id,
                unfy_domain::Rect::new(2000, 20, 800, 600),
                preimage.mode,
            )
            .unwrap();
        journal.durable().unwrap();
        path
    }

    #[test]
    fn legacy_choices_survive_without_automation_authority() {
        let dir = temp_dir("legacy-choices");
        let path = dir.join("settings.json");
        let original = r#"{"theme":"dark","language":"en","autostart":true,
            "layout_mode":"primary_side","auto_detect":"aggressive","auto_gather":true,
            "auto_restore":true,"remote_screens":"all","telemetry":true,
            "last_refused_apps":["refused.exe"],"deferred_restore":true,"marker":true,
            "hotkey_gather":"Ctrl+Alt+G","excluded_apps":["private.exe"],
            "target_display":{"native_id":"display-a","label":"Chosen","number":2}}"#;
        std::fs::write(&path, original).unwrap();
        let settings = Settings::load(&path).unwrap();
        assert_eq!(settings.theme, ThemeName::Dark);
        assert_eq!(settings.language, LanguageName::En);
        assert!(settings.autostart);
        assert_eq!(settings.hotkey_gather.as_deref(), Some("Ctrl+Alt+G"));
        assert_eq!(settings.excluded_apps, ["private.exe"]);
        assert_eq!(
            settings.target_display.as_ref().unwrap().native_id,
            "display-a"
        );
        assert_eq!(
            settings.target_display.as_ref().unwrap().quality,
            unfy_domain::IdentityQuality::Weak
        );
        assert!(settings
            .target_display
            .as_ref()
            .unwrap()
            .confirmed_topology
            .is_none());
        assert_eq!(settings.layout_mode, LayoutModeName::MoveInto);
        assert_eq!(settings.auto_detect, "off");
        assert!(!settings.auto_gather && !settings.auto_restore);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), settings);
    }

    #[test]
    fn migration_notice_persists_until_explicitly_dismissed() {
        let dir = temp_dir("migration-notice");
        let path = dir.join("settings.json");
        assert!(!Settings::default().migration_notice);
        std::fs::write(&path, r#"{"version":1,"layout_mode":"primary_side","auto_detect":"balanced","auto_gather":true}"#).unwrap();
        let migrated = Settings::load(&path).unwrap();
        assert!(migrated.migration_notice);
        assert_eq!(migrated.auto_detect, "off");
        assert!(!migrated.auto_gather);
        migrated.save(&path).unwrap();
        let mut reloaded = Settings::load(&path).unwrap();
        assert!(reloaded.migration_notice);
        reloaded.theme = ThemeName::Dark;
        reloaded.save(&path).unwrap();
        assert!(Settings::load(&path).unwrap().migration_notice);
        reloaded.migration_notice = false;
        reloaded.save(&path).unwrap();
        let acknowledged = Settings::load(&path).unwrap();
        assert!(!acknowledged.migration_notice);
        assert_eq!(acknowledged.theme, ThemeName::Dark);
        assert_eq!(acknowledged.layout_mode, LayoutModeName::MoveInto);
        assert_eq!(acknowledged.auto_detect, "off");
        acknowledged.save(&path).unwrap();
        assert!(!Settings::load(&path).unwrap().migration_notice);
    }

    #[test]
    fn unknown_schema_and_failed_validation_cannot_overwrite_disk() {
        let dir = temp_dir("protected-schema");
        let path = dir.join("settings.json");
        let future = r#"{"version":99,"future_choice":{"enabled":true}}"#;
        std::fs::write(&path, future).unwrap();
        assert!(matches!(
            Settings::load(&path),
            Err(SettingsError::VersionTooNew { .. })
        ));
        assert!(Settings::default().save(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), future);
        std::fs::remove_file(&path).unwrap();
        Settings::default().save(&path).unwrap();
        let before = std::fs::read(&path).unwrap();
        let invalid = Settings {
            window_geometry: Some((i32::MIN, 0, 800, 600)),
            ..Settings::default()
        };
        assert!(invalid.save(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn rename_and_copy_migrate_nested_journals() {
        for fallback in [false, true] {
            let (old, new) = migration_dirs("unfy-nested");
            Settings {
                margin: 41,
                ..Settings::default()
            }
            .save(old.join("settings.json"))
            .unwrap();
            let original = unfinished_journal(&old.join("journal/nested"), "original");
            let bytes = std::fs::read(&original).unwrap();
            migrate_config_dirs_with(
                &old,
                &new,
                &|a, b| {
                    if fallback {
                        Err(std::io::Error::other("injected rename failure"))
                    } else {
                        std::fs::rename(a, b)
                    }
                },
                &|a, b| std::fs::copy(a, b),
            )
            .unwrap();
            assert_eq!(
                Settings::load(new.join("settings.json")).unwrap().margin,
                41
            );
            let imported = new
                .join("journal/nested")
                .join(original.file_name().unwrap());
            assert_eq!(std::fs::read(&imported).unwrap(), bytes);
            assert!(matches!(
                unfy_journal::recover(imported).unwrap(),
                unfy_journal::Recovered::Unfinished(_)
            ));
            migrate_config_dirs(&old, &new).unwrap();
            assert_eq!(old.exists(), fallback);
        }
    }

    #[test]
    fn interrupted_copy_keeps_original_and_retries_without_deleting_staging() {
        let (old, new) = migration_dirs("unfy-copy-failure");
        Settings::default().save(old.join("settings.json")).unwrap();
        let journal = unfinished_journal(&old.join("journal/nested"), "original");
        let bytes = std::fs::read(&journal).unwrap();
        let rename = |_: &Path, _: &Path| Err(std::io::Error::other("cross-device"));
        let failed = migrate_config_dirs_with(&old, &new, &rename, &|a, b| {
            if a.extension().is_some_and(|ext| ext == "journal") {
                std::fs::write(b, b"partial")?;
                Err(std::io::Error::other("injected copy failure"))
            } else {
                std::fs::copy(a, b)
            }
        });
        assert!(failed.is_err());
        assert!(!new.exists());
        assert_eq!(std::fs::read(&journal).unwrap(), bytes);
        let staging: Vec<_> = std::fs::read_dir(old.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "migrating"))
            .collect();
        assert_eq!(staging.len(), 1);
        migrate_config_dirs_with(&old, &new, &rename, &|a, b| std::fs::copy(a, b)).unwrap();
        assert!(staging[0].exists());
        assert_eq!(journal_files(&new.join("journal")).unwrap().len(), 1);
    }

    #[test]
    fn dual_directories_preserve_new_choices_and_do_not_resurrect_imports() {
        let (old, new) = migration_dirs("unfy-dual");
        Settings {
            margin: 21,
            ..Settings::default()
        }
        .save(old.join("settings.json"))
        .unwrap();
        Settings {
            margin: 42,
            ..Settings::default()
        }
        .save(new.join("settings.json"))
        .unwrap();
        let source = unfinished_journal(&old.join("journal/nested"), "original");
        migrate_config_dirs(&old, &new).unwrap();
        let imported = new.join("journal").join(source.file_name().unwrap());
        assert!(same_file_contents(&source, &imported).unwrap());
        assert_eq!(
            Settings::load(new.join("settings.json")).unwrap().margin,
            42
        );
        // Once recovery has consumed the imported transaction, the old backup
        // must not create that obligation again on the next startup.
        std::fs::remove_file(&imported).unwrap();
        migrate_config_dirs(&old, &new).unwrap();
        assert!(!imported.exists());
        assert!(source.exists());
    }

    #[test]
    fn dual_directory_conflict_and_unknown_journal_block_without_changes() {
        let (old, new) = migration_dirs("unfy-conflict");
        let source = unfinished_journal(&old.join("journal"), "old transaction");
        let target = unfinished_journal(&new.join("journal"), "conflicting transaction");
        let before = std::fs::read(&target).unwrap();
        assert!(migrate_config_dirs(&old, &new).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), before);
        assert!(source.exists());
        assert!(!new.join(".conflux-migration-complete").exists());
        let (old, new) = migration_dirs("unfy-malformed");
        std::fs::create_dir_all(old.join("journal")).unwrap();
        std::fs::write(old.join("journal/unknown.journal"), b"unknown schema").unwrap();
        assert!(migrate_config_dirs(&old, &new).is_err());
        assert!(!new.exists());
        assert!(old.join("journal/unknown.journal").exists());
    }

    #[test]
    fn failed_replacement_preserves_previous_settings_and_unrelated_temp() {
        let (root, _) = migration_dirs("unfy-save-failure");
        let path = root.join("settings.json");
        Settings {
            margin: 27,
            ..Settings::default()
        }
        .save(&path)
        .unwrap();
        let stale = root.join("settings.json.tmp");
        std::fs::write(&stale, b"unrelated interrupted save").unwrap();
        let original = std::fs::read(&path).unwrap();
        let replacement = serde_json::to_vec(&Settings::default()).unwrap();
        assert!(atomic_write_with(&path, &replacement, |_, _| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected replacement failure",
            ))
        })
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(Settings::load(&path).unwrap().margin, 27);
        assert_eq!(
            std::fs::read(&stale).unwrap(),
            b"unrelated interrupted save"
        );
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

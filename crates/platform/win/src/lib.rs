//! Windows implementation of the Unfy platform traits.
//!
//! The three rules that this module exists to enforce:
//!
//! * every call that reads a rectangle first switches the calling thread to
//!   Per-Monitor-V2 awareness, so a DPI-unaware target window cannot make us
//!   believe it lives at 96 DPI;
//! * visible rectangles come from `DWMWA_EXTENDED_FRAME_BOUNDS`, never from
//!   `GetWindowRect`, which includes ~7 px of invisible resize border per side;
//! * mode transitions use asynchronous native placement, retaining the normal
//!   restore rectangle without activating ShowWindow calls.

mod displays;
mod dpi;
mod labels;
mod thumbnails;
mod windows_impl;

pub use displays::{display_at, identify_window, WinDisplays};
pub use windows_impl::{capture_placement, nudge_window, RawWindow, WinWindows, WindowStyleFlags};

use unfy_platform::{DisplayProvider, Platform, WindowBackend};

/// The Windows platform pair.
#[derive(Debug, Default)]
pub struct WinPlatform {
    displays: WinDisplays,
    windows: WinWindows,
}

impl WinPlatform {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Platform for WinPlatform {
    type Displays = WinDisplays;
    type Windows = WinWindows;

    fn displays(&self) -> &Self::Displays {
        &self.displays
    }

    fn windows(&self) -> &Self::Windows {
        &self.windows
    }
}

impl DisplayProvider for WinPlatform {
    fn topology(&self) -> unfy_platform::Result<Vec<unfy_domain::DisplaySnapshot>> {
        self.displays.topology()
    }

    fn scale_of(&self, key: &unfy_domain::DisplayKey) -> unfy_platform::Result<unfy_domain::Scale> {
        self.displays.scale_of(key)
    }

    fn display_for(
        &self,
        id: &unfy_domain::WindowId,
    ) -> unfy_platform::Result<unfy_domain::DisplayKey> {
        self.displays.display_for(id)
    }
}

impl WindowBackend for WinPlatform {
    fn enumerate(&self) -> unfy_platform::Result<Vec<unfy_domain::WindowSnapshot>> {
        self.windows.enumerate()
    }

    fn capabilities(&self, id: &unfy_domain::WindowId) -> unfy_platform::Capabilities {
        self.windows.capabilities(id)
    }

    fn snapshot(
        &self,
        id: &unfy_domain::WindowId,
    ) -> unfy_platform::Result<unfy_domain::WindowSnapshot> {
        self.windows.snapshot(id)
    }

    fn eligibility(&self, id: &unfy_domain::WindowId) -> Option<unfy_domain::SkipReason> {
        self.windows.eligibility(id)
    }

    fn apply_async(
        &self,
        id: &unfy_domain::WindowId,
        plan: &unfy_platform::WindowPlan,
    ) -> unfy_platform::Result<()> {
        self.windows.apply_async(id, plan)
    }

    fn restore_native_state(
        &self,
        id: &unfy_domain::WindowId,
        placement: &[u8],
        topmost: bool,
    ) -> unfy_platform::Result<()> {
        self.windows.restore_native_state(id, placement, topmost)
    }

    fn native_placement_matches(&self, actual: &[u8], expected: &[u8]) -> bool {
        self.windows.native_placement_matches(actual, expected)
    }

    fn transform_native_placement(
        &self,
        placement: &[u8],
        source_work: unfy_domain::Rect,
        source_scale: unfy_domain::Scale,
        target: &unfy_domain::DisplaySnapshot,
    ) -> unfy_platform::Result<Vec<u8>> {
        self.windows
            .transform_native_placement(placement, source_work, source_scale, target)
    }

    fn apply(
        &self,
        id: &unfy_domain::WindowId,
        plan: &unfy_platform::WindowPlan,
    ) -> unfy_platform::Result<()> {
        self.windows.apply(id, plan)
    }

    fn read_back(
        &self,
        id: &unfy_domain::WindowId,
    ) -> unfy_platform::Result<unfy_platform::ObservedWindow> {
        self.windows.read_back(id)
    }
}

/// Show a large numbered badge on each display for `seconds`.
///
/// The badge is a borderless, click-through, topmost window per display. It is
/// removed automatically, so it cannot be left behind even if the app crashes
/// (the thread that owns it is detached and exits on its own timer).
pub fn flash_display_labels(seconds: u64) -> crate::labels::FlashOutcome {
    crate::labels::flash(seconds)
}

pub use labels::FlashOutcome;
pub use thumbnails::DisplayThumbnail;

/// Capture a thumbnail of every display, for the display picker.
///
/// Thumbnails are returned in memory as PNG data URLs and are never written to
/// disk or sent anywhere.
pub fn display_thumbnails() -> Vec<DisplayThumbnail> {
    thumbnails::display_thumbnails()
}

/// Executables that currently own at least one visible window.
///
/// Used by the exclusion picker so the user selects from what is actually
/// running instead of typing a file name they have to guess.
pub fn running_applications() -> Vec<String> {
    use unfy_platform::WindowBackend;

    let backend = crate::WinWindows::new();
    let mut names: Vec<String> = backend
        .enumerate()
        .unwrap_or_default()
        .into_iter()
        .map(|w| w.app_name)
        .filter(|name| !name.is_empty())
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

/// Relaunch this executable elevated, via the UAC prompt.
///
/// Windows will not let a normal-integrity process move a window owned by an
/// elevated one; `SetWindowPos` across that boundary fails with access denied.
/// Rather than leaving those windows permanently out of reach, the user can
/// relaunch elevated. `ShellExecuteW` with the `runas` verb is the documented way
/// to request that: the shell shows the consent prompt and starts a new process
/// at high integrity.
///
/// Returns false when the user dismisses the prompt or the shell refuses. The
/// caller must not treat false as success.
pub fn relaunch_elevated() -> bool {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let operation = HSTRING::from("runas");
    let target = HSTRING::from(exe.as_os_str().to_string_lossy().to_string());
    // SAFETY: all strings live for the duration of the call; the shell retains
    // nothing after it returns.
    let result = unsafe { ShellExecuteW(None, &operation, &target, None, None, SW_SHOWNORMAL) };
    // ShellExecuteW returns a value > 32 on success; 5 (SE_ERR_ACCESSDENIED) is
    // the user declining the UAC prompt.
    result.0 as isize > 32
}

/// Whether this process is running with administrator rights.
///
/// Reported so the interface can explain why elevated windows stay put, and so
/// the elevation offer disappears once it has been taken.
pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token = HANDLE::default();
    // SAFETY: the handle is closed below; the buffer is sized by the API contract.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            core::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// Open a directory in the user's file manager.
///
/// Uses `ShellExecuteW` with `explorer`: the documented way to ask the shell to
/// reveal a path, and it honours the user's per-folder settings. Returns false when
/// the call is rejected (path missing, no shell) rather than pretending it worked.
pub fn open_directory(path: &std::path::Path) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation = HSTRING::from("open");
    let target = HSTRING::from(path.as_os_str().to_string_lossy().to_string());
    // SAFETY: all strings live for the duration of the call; no buffers are
    // retained by the shell.
    let result = unsafe { ShellExecuteW(None, &operation, &target, None, None, SW_SHOWNORMAL) };
    // ShellExecuteW returns a value > 32 on success (an HINSTANCE cast to isize).
    result.0 as isize > 32
}

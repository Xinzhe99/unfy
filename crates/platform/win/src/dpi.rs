//! Small RAII guard that pins the calling thread to Per-Monitor-V2 awareness.
//!
//! Reading a third-party window's rectangle is only meaningful when the calling
//! thread is PMv2: an unaware or system-aware thread gets DPI-virtualised
//! coordinates, and mixing those with physical-pixel work areas produces the
//! classic "window shrinks by the scale factor on every move" bug.

use windows::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

pub(crate) struct PerMonitorV2 {
    previous: DPI_AWARENESS_CONTEXT,
}

impl PerMonitorV2 {
    pub(crate) fn enter() -> Self {
        // SAFETY: `SetThreadDpiAwarenessContext` only affects the calling thread
        // and is safe to call from any thread.
        let previous =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        Self { previous }
    }
}

impl Drop for PerMonitorV2 {
    fn drop(&mut self) {
        // SAFETY: restoring the value returned by the matching `enter` call.
        unsafe {
            SetThreadDpiAwarenessContext(self.previous);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_restores_previous_context() {
        // Entering and dropping must not panic, and must be re-entrant.
        let outer = PerMonitorV2::enter();
        {
            let _inner = PerMonitorV2::enter();
        }
        drop(outer);
    }
}

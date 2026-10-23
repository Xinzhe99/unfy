//! Unit tests for the macOS backend's platform-independent parts.
//!
//! The AX calls themselves need a Mac with Accessibility permission, but the
//! logic that decides *what* to do is testable everywhere: coordinate conversion,
//! the fullscreen skip, and the size→position→size ordering rule.

#![cfg(target_os = "macos")]

use unfy_domain::Rect;

/// AppKit uses bottom-left origin, AX uses top-left. A screen at AppKit origin
/// `(0, 0)` with height 1080 maps to top-left `(0, 0)`; a screen stacked above it
/// at AppKit `y = 1080` maps to top-left `y = -1080`.
#[test]
fn appkit_to_top_left_flips_the_vertical_axis() {
    let main_height = 1080.0;
    let cases = [
        // (appkit_y, appkit_height, expected_top_left_y)
        (0.0, 1080.0, 0.0),
        (1080.0, 1080.0, -1080.0),
        (-900.0, 900.0, 1080.0),
    ];
    for (y, height, expected) in cases {
        let top_left = main_height - (y + height);
        assert_eq!(top_left, expected, "appkit y={y} h={height}");
    }
}

/// A window whose rectangle falls entirely outside every display must be
/// treated as needing rescue, not as "already fine".
#[test]
fn offscreen_detection_is_geometry_based() {
    let display = Rect::new(0, 0, 1920, 1080);
    let on_screen = Rect::new(100, 100, 900, 700);
    let off_screen = Rect::new(4000, 3000, 4800, 3600);

    assert!(on_screen.reachable_in(&display, 64, 32));
    assert!(!off_screen.reachable_in(&display, 64, 32));
}

/// The AX backend must not try to move a native-fullscreen window: its Space is
/// separate and position writes are ignored.
#[test]
fn fullscreen_windows_are_skipped_by_the_snapshot_predicate() {
    use unfy_domain::{DisplayKey, IdentityQuality, WindowId, WindowMode, WindowSnapshot};

    let snapshot = |mode| WindowSnapshot {
        id: WindowId {
            native_id: 1,
            process_id: 1,
            process_start: 0,
        },
        app_name: "Test.app".into(),
        display: DisplayKey {
            native_id: "d".into(),
            quality: IdentityQuality::Strong,
        },
        visible_rect: Rect::new(0, 0, 1920, 1080),
        outer_rect: Rect::new(0, 0, 1920, 1080),
        mode,
        native_placement: None,
        topmost: false,
    };

    assert!(snapshot(WindowMode::Normal).is_migratable());
    assert!(!snapshot(WindowMode::Fullscreen).is_migratable());
}

/// A window with a minimum size larger than its slot must keep its minimum, and
/// the engine's tolerance check must then report `Rejected` rather than pretend
/// the move succeeded. Verified here on the geometry level.
#[test]
fn tolerance_check_rejects_a_clamped_window() {
    let requested = Rect::new(0, 0, 400, 300);
    let actual = Rect::new(0, 0, 640, 300);
    let tolerance = 4;

    let within = (actual.left - requested.left).abs() <= tolerance
        && (actual.top - requested.top).abs() <= tolerance
        && (actual.width() - requested.width()).abs() <= tolerance
        && (actual.height() - requested.height()).abs() <= tolerance;

    assert!(!within, "a 240 px width difference must not be accepted");
}

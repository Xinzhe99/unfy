//! Compile contracts for the native backend; these tests never query or move windows.

#![cfg(target_os = "macos")]

#[test]
fn mac_platform_can_be_shared_with_desktop_workers() {
    fn requires_send_sync<T: Send + Sync>() {}
    requires_send_sync::<unfy_platform_mac::MacPlatform>();
}

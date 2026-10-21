// Hide the console window in release builds on Windows. In debug builds the
// console is useful for `tracing` output.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    conflux_desktop_lib::run();
}

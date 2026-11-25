//! fogu-desktop: the desktop executable. Everything is in lib.rs (the same
//! code is the library that `fogu desktop build --android/--ios` links into
//! the mobile app).

// no console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    fogu_desktop_lib::run();
}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod app;
mod features;
mod platform;

fn main() {
    if let Err(error) = app::run() {
        let message = format!("crosshair-rs could not start.\n\n{error:#}");
        eprintln!("{message}");
        let text = crosshair_platform::identity::wide(&message);
        unsafe {
            use windows::{
                Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW},
                core::PCWSTR,
            };
            MessageBoxW(
                None,
                PCWSTR(text.as_ptr()),
                windows::core::w!("crosshair-rs"),
                MB_OK | MB_ICONERROR,
            );
        }
        std::process::exit(1);
    }
}

use std::{mem::size_of, time::Instant};

use anyhow::{Result, ensure};
use serde::Serialize;
use windows::Win32::{
    Foundation::RECT,
    UI::{
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
        WindowsAndMessaging::{
            DispatchMessageW, GWL_EXSTYLE, GetForegroundWindow, GetMessageW, GetWindowLongW,
            GetWindowRect, KillTimer, MSG, SetTimer, TranslateMessage, WM_TIMER,
        },
    },
};

use crate::overlay::{DesktopOverlay, Monitor, enumerate_monitors};

#[derive(Serialize)]
struct Report {
    version: &'static str,
    monitors: Vec<Monitor>,
    window_bounds: [i32; 4],
    window_extended_styles: u32,
    initial_focus_preserved: bool,
    presentation_micros: u128,
    duration_seconds: u32,
    message_size: usize,
}

pub fn run(arguments: &[String]) -> Result<()> {
    let seconds = arguments
        .first()
        .map_or(Ok(5), |value| value.parse::<u32>())?;
    ensure!(
        (1..=120).contains(&seconds),
        "Probe duration must be 1-120 seconds"
    );
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)?;
    }
    let monitors = enumerate_monitors()?;
    let monitor = monitors
        .iter()
        .find(|monitor| monitor.primary)
        .unwrap_or(&monitors[0]);
    let foreground = unsafe { GetForegroundWindow() };
    let mut overlay = DesktopOverlay::new()?;
    let mut rgba = vec![0; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            if ((15..17).contains(&x) && ((2..12).contains(&y) || (20..30).contains(&y)))
                || ((15..17).contains(&y) && ((2..12).contains(&x) || (20..30).contains(&x)))
            {
                rgba[(y * 32 + x) * 4..(y * 32 + x + 1) * 4].copy_from_slice(&[249, 115, 22, 255]);
            }
        }
    }
    let started = Instant::now();
    overlay.present([32, 32], &rgba, monitor.center())?;
    let presentation_micros = started.elapsed().as_micros();
    let mut bounds = RECT::default();
    unsafe {
        GetWindowRect(overlay.hwnd(), &mut bounds)?;
    }
    let report = Report {
        version: env!("CARGO_PKG_VERSION"),
        monitors,
        window_bounds: [bounds.left, bounds.top, bounds.right, bounds.bottom],
        window_extended_styles: unsafe { GetWindowLongW(overlay.hwnd(), GWL_EXSTYLE) as u32 },
        initial_focus_preserved: foreground == unsafe { GetForegroundWindow() },
        presentation_micros,
        duration_seconds: seconds,
        message_size: size_of::<MSG>(),
    };
    unsafe {
        let timer = SetTimer(None, 0, seconds * 1000, None);
        ensure!(timer != 0, "Could not create probe shutdown timer");
        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, None, 0, 0).0;
            ensure!(result != -1, "Windows message dispatch failed");
            if result == 0 || (message.message == WM_TIMER && message.wParam.0 == timer) {
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        KillTimer(None, timer)?;
    }
    overlay.hide();
    let json = serde_json::to_string_pretty(&report)?;
    if let Some(path) = arguments.get(1) {
        std::fs::write(path, &json)?;
    }
    println!("{json}");
    Ok(())
}

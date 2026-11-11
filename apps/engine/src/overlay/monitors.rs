use std::mem::size_of;

use anyhow::{Result, ensure};
use serde::Serialize;
use windows::Win32::{
    Foundation::{LPARAM, RECT},
    Graphics::Gdi::{
        DISPLAY_DEVICEW, EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR,
        MONITORINFO, MONITORINFOEXW,
    },
    UI::WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME,
};
use windows::core::{BOOL, PCWSTR};

#[derive(Clone, Debug, Serialize)]
pub struct Monitor {
    pub id: String,
    pub name: String,
    pub bounds: [i32; 4],
    pub primary: bool,
}

impl Monitor {
    pub fn center(&self) -> [i32; 2] {
        [
            self.bounds[0] + (self.bounds[2] - self.bounds[0]) / 2,
            self.bounds[1] + (self.bounds[3] - self.bounds[1]) / 2,
        ]
    }
}

pub fn enumerate_monitors() -> Result<Vec<Monitor>> {
    let mut monitors = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(enumerate),
            LPARAM((&mut monitors as *mut Vec<Monitor>) as isize),
        )
        .ok()?;
    }
    ensure!(!monitors.is_empty(), "No displays are available");
    Ok(monitors)
}

unsafe extern "system" fn enumerate(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    unsafe {
        if GetMonitorInfoW(
            monitor,
            (&mut info as *mut MONITORINFOEXW).cast::<MONITORINFO>(),
        )
        .as_bool()
        {
            let end = info
                .szDevice
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(info.szDevice.len());
            let name = String::from_utf16_lossy(&info.szDevice[..end]);
            let mut device = DISPLAY_DEVICEW {
                cb: size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            let available = EnumDisplayDevicesW(
                PCWSTR(info.szDevice.as_ptr()),
                0,
                &mut device,
                EDD_GET_DEVICE_INTERFACE_NAME,
            )
            .as_bool();
            let read = |units: &[u16]| {
                String::from_utf16_lossy(
                    &units[..units.iter().position(|u| *u == 0).unwrap_or(units.len())],
                )
            };
            let stable = available
                .then(|| read(&device.DeviceID))
                .filter(|id| !id.is_empty());
            let label = available
                .then(|| read(&device.DeviceString))
                .filter(|name| !name.is_empty());
            let rect = info.monitorInfo.rcMonitor;
            let result = &mut *(data.0 as *mut Vec<Monitor>);
            result.push(Monitor {
                id: stable.unwrap_or_else(|| name.clone()),
                name: label.unwrap_or(name),
                bounds: [rect.left, rect.top, rect.right, rect.bottom],
                primary: info.monitorInfo.dwFlags & 1 != 0,
            });
        }
    }
    BOOL(1)
}

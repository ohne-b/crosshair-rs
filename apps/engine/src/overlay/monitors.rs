use std::{collections::HashMap, mem::size_of};

use anyhow::{Result, ensure};
use serde::Serialize;
use windows::Win32::{
    Devices::Display::*,
    Foundation::{ERROR_INSUFFICIENT_BUFFER, LPARAM, RECT},
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
    let mut monitors: Vec<Monitor> = Vec::new();
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
    let names = display_names();
    for (index, monitor) in monitors.iter_mut().enumerate() {
        monitor.name = monitor_label(
            names.get(&monitor.name).map(String::as_str),
            index + 1,
            monitor.bounds,
        );
    }
    let labels: Vec<_> = monitors
        .iter()
        .map(|monitor| monitor.name.clone())
        .collect();
    for (index, monitor) in monitors.iter_mut().enumerate() {
        if labels
            .iter()
            .filter(|label| **label == monitor.name)
            .count()
            > 1
        {
            monitor.name = format!("{} - Display {}", monitor.name, index + 1);
        }
    }
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
            let rect = info.monitorInfo.rcMonitor;
            let result = &mut *(data.0 as *mut Vec<Monitor>);
            result.push(Monitor {
                id: stable.unwrap_or_else(|| name.clone()),
                name,
                bounds: [rect.left, rect.top, rect.right, rect.bottom],
                primary: info.monitorInfo.dwFlags & 1 != 0,
            });
        }
    }
    BOOL(1)
}

fn read_wide(units: &[u16]) -> String {
    String::from_utf16_lossy(
        &units[..units
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(units.len())],
    )
}

fn display_names() -> HashMap<String, String> {
    let mut names = HashMap::new();
    // Display topology can change between sizing and querying; retry a bounded number of times.
    for _ in 0..3 {
        let (mut path_count, mut mode_count) = (0, 0);
        unsafe {
            if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
                .is_err()
            {
                break;
            }
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
            let result = QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut path_count,
                paths.as_mut_ptr(),
                &mut mode_count,
                modes.as_mut_ptr(),
                None,
            );
            if result == ERROR_INSUFFICIENT_BUFFER {
                continue;
            }
            if result.is_err() {
                break;
            }
            for path in paths.into_iter().take(path_count as usize) {
                let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                    header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                        r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                        size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                        adapterId: path.sourceInfo.adapterId,
                        id: path.sourceInfo.id,
                    },
                    ..Default::default()
                };
                let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME {
                    header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                        r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                        size: size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                        adapterId: path.targetInfo.adapterId,
                        id: path.targetInfo.id,
                    },
                    ..Default::default()
                };
                if DisplayConfigGetDeviceInfo(&mut source.header) == 0
                    && DisplayConfigGetDeviceInfo(&mut target.header) == 0
                {
                    names.insert(
                        read_wide(&source.viewGdiDeviceName),
                        read_wide(&target.monitorFriendlyDeviceName),
                    );
                }
            }
        }
        break;
    }
    names
}

fn monitor_label(model: Option<&str>, index: usize, bounds: [i32; 4]) -> String {
    let model = model
        .map(str::trim)
        .filter(|name| !name.is_empty() && !name.to_ascii_lowercase().contains("generic"));
    match model {
        Some(model) => model.to_owned(),
        None => format!(
            "Display {index} ({} x {})",
            bounds[2] - bounds[0],
            bounds[3] - bounds[1]
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names_are_preferred_and_generic_or_missing_names_use_resolution() {
        let bounds = [-2560, 0, 0, 1440];
        assert_eq!(monitor_label(Some("  VG27AQ  "), 1, bounds), "VG27AQ");
        for model in [
            None,
            Some(""),
            Some(" "),
            Some("Generic PnP Monitor"),
            Some("Generic Non-PnP Monitor"),
        ] {
            assert_eq!(monitor_label(model, 2, bounds), "Display 2 (2560 x 1440)");
        }
    }
}

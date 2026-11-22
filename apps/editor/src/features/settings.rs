use crosshair_core::protocol::{UpdatePhase, UpdateStatus};

pub fn update_label(status: &UpdateStatus) -> String {
    if status.error.is_some() {
        return match status.phase {
            UpdatePhase::Ready => "Installation couldn't start",
            _ => "Update couldn't finish",
        }
        .into();
    }
    match status.phase {
        UpdatePhase::Idle => String::new(),
        UpdatePhase::Checking => "Checking for updates…".into(),
        UpdatePhase::Current => "You're up to date".into(),
        UpdatePhase::Available => format!(
            "{} is available",
            status.version.as_deref().unwrap_or("An update")
        ),
        UpdatePhase::Downloading => "Downloading update…".into(),
        UpdatePhase::Ready => status.version.as_ref().map_or_else(
            || "Ready to install".into(),
            |version| format!("Ready to install {version}"),
        ),
        UpdatePhase::Installing => "Installing…".into(),
        UpdatePhase::Failed => "Update couldn't finish".into(),
    }
}

pub fn download_progress(status: &UpdateStatus) -> String {
    if status.phase != UpdatePhase::Downloading {
        return String::new();
    }
    match status.total.filter(|total| *total > 0) {
        Some(total) => format!(
            "{} / {} - {}%",
            bytes(status.downloaded),
            bytes(total),
            (status.downloaded.saturating_mul(100) / total).min(100)
        ),
        None => format!("{} downloaded", bytes(status.downloaded)),
    }
}

fn bytes(value: u64) -> String {
    format!("{:.1} MB", value as f64 / 1_000_000.0)
}

pub const GAME_BAR_HELP: &str = "Choose Game Bar in Display mode, then Open widget if setup is needed. Press Win+G, pin the widget on your target display and enable click-through in the top toolbar. Dismiss Game Bar and click Use Game Bar when the widget is ready. Fullscreen support depends on the game and Game Bar.";
pub const INPUT_HELP: &str = "Shortcuts work globally, including while typing in other apps. They observe input without blocking it. Click a cell in the Keyboard / mouse or Controller column to record that shortcut. Both inputs can trigger the same action. For a controller, release held controls, then press a button, squeeze a trigger or move a stick. Face buttons use their position on the controller. Escape cancels recording. Backspace clears the selected binding. Wheel bindings are press-only.";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_label_preserves_the_complete_prerelease_version() {
        for version in ["0.1.0-alpha.6", "0.1.0-beta.2", "0.1.0-rc.1", "0.1.0"] {
            let status = UpdateStatus {
                phase: UpdatePhase::Available,
                version: Some(version.into()),
                ..Default::default()
            };
            assert_eq!(update_label(&status), format!("{version} is available"));
            assert_eq!(
                update_label(&UpdateStatus {
                    phase: UpdatePhase::Ready,
                    ..status
                }),
                format!("Ready to install {version}")
            );
        }
    }
    #[test]
    fn unknown_download_length_is_not_fake_percentage() {
        let mut status = UpdateStatus {
            phase: UpdatePhase::Downloading,
            downloaded: 4_000_000,
            ..Default::default()
        };
        assert_eq!(download_progress(&status), "4.0 MB downloaded");
        status.total = Some(0);
        assert_eq!(download_progress(&status), "4.0 MB downloaded");
        status.total = Some(8_000_000);
        assert_eq!(download_progress(&status), "4.0 MB / 8.0 MB - 50%");
        status.downloaded = 0;
        assert_eq!(download_progress(&status), "0.0 MB / 8.0 MB - 0%");
        status.downloaded = 9_000_000;
        assert_eq!(download_progress(&status), "9.0 MB / 8.0 MB - 100%");
        status.phase = UpdatePhase::Ready;
        status.error = Some("A deployment failure".into());
        assert_eq!(update_label(&status), "Installation couldn't start");
        assert!(download_progress(&status).is_empty());
    }
}

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
        UpdatePhase::Ready => "Ready to install".into(),
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
            "{} / {} · {}%",
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

pub const GAME_BAR_HELP: &str = "Open Xbox Game Bar (Win+G), select crosshair-rs in Widgets, then pin it and enable click-through. Position the widget over the target display and keep its window visible. Fullscreen support depends on Game Bar and the game; Vulkan and OpenGL can be restricted. The app never injects into games.";
pub const INPUT_HELP: &str = "Shortcuts work globally, including while typing in other apps. They observe input without blocking it. Windows and other applications keep their existing shortcuts. Wheel bindings are press-only.";

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_download_length_is_not_fake_percentage() {
        let mut status = UpdateStatus {
            phase: UpdatePhase::Downloading,
            downloaded: 4_000_000,
            ..Default::default()
        };
        assert_eq!(download_progress(&status), "4.0 MB downloaded");
        status.total = Some(8_000_000);
        assert!(download_progress(&status).ends_with("50%"));
        status.phase = UpdatePhase::Ready;
        status.error = Some("A deployment failure".into());
        assert_eq!(update_label(&status), "Installation couldn't start");
        assert!(download_progress(&status).is_empty());
    }
}

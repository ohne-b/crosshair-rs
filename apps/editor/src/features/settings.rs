use crosshair_core::protocol::{UpdatePhase, UpdateStatus};

pub fn update_label(status: &UpdateStatus) -> String {
    match status.phase {
        UpdatePhase::Idle => String::new(),
        UpdatePhase::Checking => "Checking for updates…".into(),
        UpdatePhase::Current => "You're up to date".into(),
        UpdatePhase::Available => format!(
            "{} is available",
            status.version.as_deref().unwrap_or("An update")
        ),
        UpdatePhase::Downloading => match status.total.filter(|total| *total > 0) {
            Some(total) => format!(
                "{} / {} · {}%",
                bytes(status.downloaded),
                bytes(total),
                (status.downloaded.saturating_mul(100) / total).min(100)
            ),
            None => format!("{} downloaded", bytes(status.downloaded)),
        },
        UpdatePhase::Ready => "Ready to install".into(),
        UpdatePhase::Installing => "Installing…".into(),
        UpdatePhase::Failed => status
            .error
            .clone()
            .unwrap_or_else(|| "Update failed".into()),
    }
}

fn bytes(value: u64) -> String {
    if value >= 1024 * 1024 {
        format!("{:.1} MB", value as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", value / 1024)
    }
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
            downloaded: 4096,
            ..Default::default()
        };
        assert_eq!(update_label(&status), "4 KB downloaded");
        status.total = Some(8192);
        assert!(update_label(&status).ends_with("50%"));
    }
}

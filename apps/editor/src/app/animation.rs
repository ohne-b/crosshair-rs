use super::*;

impl App {
    pub(super) fn stop_playback(&mut self) {
        self.playback_timer.stop();
        self.playback_next = None;
        self.playback.reset();
        self.playing = false;
        self.playback_generation = self.preview.cancel_bitmap();
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Store>().set_animation_playing(false);
        }
    }

    pub(super) fn show_animated_target(&mut self) -> Result<()> {
        if self.animate
            && !self.playing
            && !self.preview_pending
            && self.window()?.global::<Store>().get_page() == 1
            && let Some(base) = &self.document.draft
        {
            let animated =
                crate::features::designer::animated_document(base, &self.document.selected);
            self.request_bitmap(animated)?;
        }
        Ok(())
    }

    fn request_bitmap(&mut self, preset: Preset) -> Result<()> {
        let ui = self.window()?;
        self.playback_generation = self.preview.bitmap(
            preset,
            ui.global::<Store>().get_zoom() * ui.window().scale_factor(),
        );
        Ok(())
    }

    pub(super) fn preview_input(&mut self, value: &str) -> Result<()> {
        if !self.animate || self.window()?.global::<Store>().get_page() != 1 {
            return Ok(());
        }
        let Some(base) = &self.document.draft else {
            return Ok(());
        };
        if !self.playing && value.ends_with("-up") {
            return Ok(());
        }
        if !self.playing {
            self.playback_started = std::time::Instant::now();
            self.playback.select(base, false, false, 0);
            self.playing = true;
            self.window()?.global::<Store>().set_animation_playing(true);
        }
        let now = self.playback_started.elapsed().as_millis() as u64;
        let event = match value {
            "reset" => AnimationEvent::Reload,
            "aim-down" | "aim-up" => AnimationEvent::Trigger {
                trigger: AnimationTrigger::Aim,
                down: value == "aim-down",
            },
            "fire-down" | "fire-up" => AnimationEvent::Trigger {
                trigger: AnimationTrigger::Fire,
                down: value == "fire-down",
            },
            _ => return Ok(()),
        };
        self.playback.event(base, event, now);
        self.playback_tick()
    }

    pub(super) fn playback_tick(&mut self) -> Result<()> {
        self.playback_timer.stop();
        if !self.playing || !self.animate || self.window()?.global::<Store>().get_page() != 1 {
            return Ok(());
        }
        let Some(base) = &self.document.draft else {
            self.stop_playback();
            return Ok(());
        };
        let now = self.playback_started.elapsed().as_millis() as u64;
        let sample = self.playback.sample(base, now);
        self.request_bitmap(sample.preset)?;
        self.playback_next = sample.next_frame_ms;
        Ok(())
    }

    pub(super) fn schedule_playback(&mut self) {
        if self.playing
            && let Some(next) = self.playback_next.take()
        {
            let now = self.playback_started.elapsed().as_millis() as u64;
            self.playback_timer.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(next.saturating_sub(now).max(16)),
                || {
                    ui_event(UiEvent::Action("animation-tick".into(), String::new()));
                },
            );
        }
    }
}

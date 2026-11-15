use super::*;

#[derive(Default)]
pub(super) struct AnimationOutput {
    definition: Option<Preset>,
    key: Option<render::Key>,
    input_epoch: u64,
    generation: u64,
    playback: Playback,
    in_flight: Option<(u64, u64)>,
    last_submit: Option<u64>,
    next_frame: Option<u64>,
    pose: Option<Preset>,
    raster: Option<Arc<crosshair_renderer::Raster>>,
    failed: bool,
}

impl AnimationOutput {
    fn sync(
        &mut self,
        key: Option<render::Key>,
        definition: Option<&Preset>,
        input_epoch: u64,
        held: [bool; 2],
        now: u64,
    ) -> bool {
        let reset = self.input_epoch != input_epoch;
        if !reset && self.key == key && self.definition.as_ref() == definition {
            return false;
        }
        let changed = self.definition.as_ref().map(|p| (p.id, &p.layers))
            != definition.map(|p| (p.id, &p.layers));
        self.invalidate();
        if reset {
            self.playback.reset();
        }
        self.input_epoch = input_epoch;
        self.key = key;
        self.definition = definition.cloned();
        if (reset || changed)
            && let Some(preset) = &self.definition
        {
            self.playback.select(preset, held[0], held[1], now);
        }
        true
    }

    pub(super) fn active(&self) -> bool {
        self.definition.as_ref().is_some_and(|p| {
            p.layers.iter().any(|layer| {
                layer
                    .animation
                    .as_ref()
                    .is_some_and(|a| a.effective_for(layer))
            })
        })
    }

    pub(super) fn busy(&self) -> bool {
        self.in_flight.is_some()
    }

    pub(super) fn changing(&self) -> bool {
        self.busy() || self.next_frame.is_some()
    }

    fn invalidate(&mut self) {
        self.invalidate_frame();
        self.raster = None;
        self.failed = false;
    }

    fn invalidate_frame(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.in_flight = None;
        self.last_submit = None;
        self.next_frame = None;
        self.pose = None;
    }

    fn event(&mut self, event: AnimationEvent, now: u64) -> bool {
        let Some(preset) = &self.definition else {
            return false;
        };
        let affects_playback = preset.layers.iter().any(|layer| {
            layer.animation.as_ref().is_some_and(|animation| {
                animation.effective_for(layer)
                    && match event {
                        AnimationEvent::Reload => true,
                        AnimationEvent::Trigger { trigger, down } => {
                            animation.trigger == trigger
                                && (down || animation.activation == Activation::Hold)
                        }
                    }
            })
        });
        self.playback.event(preset, event, now);
        if affects_playback {
            self.invalidate_frame();
            if event == AnimationEvent::Reload {
                self.failed = false;
            }
        }
        affects_playback
    }

    fn accept(&mut self, frame: (u64, u64)) -> bool {
        if self.in_flight != Some(frame) || frame.0 != self.generation {
            return false;
        }
        self.in_flight = None;
        true
    }

    fn ready_at(&self, now: u64) -> u64 {
        self.last_submit
            .map_or(now, |previous| now.max(previous.saturating_add(17)))
    }

    fn begin_frame(&mut self, pose: &Preset, now: u64) -> Option<(u64, u64)> {
        if self.busy()
            || self.failed
            || self.ready_at(now) > now
            || self.pose.as_ref() == Some(pose)
        {
            return None;
        }
        let frame = (self.generation, now);
        self.in_flight = Some(frame);
        self.last_submit = Some(now);
        self.pose = Some(pose.clone());
        Some(frame)
    }
}

impl Runtime {
    pub(super) fn stop_animation_timer(&self) {
        unsafe {
            let _ = KillTimer(Some(self.window.0), ANIMATION_TIMER);
        }
    }

    fn animation_timer(&self, when: u64) {
        unsafe {
            SetTimer(
                Some(self.window.0),
                ANIMATION_TIMER,
                when.saturating_sub(self.app.elapsed_ms()).clamp(1, 10_000) as u32,
                None,
            );
        }
    }

    pub(super) fn sync_animation(&mut self) {
        let key = self
            .app
            .presentation_of(&self.app.state, true)
            .map(|(key, _)| key);
        let definition = self.app.selected_preset_of(&self.app.state).map(|preset| {
            self.preview
                .as_ref()
                .filter(|p| Some(p.key) == key)
                .map_or(preset, |p| &p.preset)
        });
        if self.animation.sync(
            key,
            definition,
            self.app.playback_epoch,
            [
                self.app.input.held(AnimationTrigger::Aim),
                self.app.input.held(AnimationTrigger::Fire),
            ],
            self.app.elapsed_ms(),
        ) {
            self.stop_animation_timer();
            self.render.cancel();
            self.rendering = None;
        }
        let now = self.app.elapsed_ms();
        let mut invalidated = false;
        for event in self.app.animation_events.drain(..) {
            if self.app.state.base_visible && self.animation.event(event, now) {
                self.render.cancel();
                self.rendering = None;
                invalidated = true;
            }
        }
        if invalidated {
            self.stop_animation_timer();
        }
    }

    pub(super) fn cancel_animation_frame(&mut self) {
        if self.animation.busy() {
            self.render.cancel();
            self.animation.invalidate_frame();
        }
    }

    pub(super) fn present_animation(&mut self, key: render::Key, center: [i32; 2]) {
        if self.app.state.settings.output_mode == OutputMode::GameBar
            && (!self.app.status.widget.connected || !self.app.status.widget.pinned)
        {
            self.cancel_animation_frame();
            return;
        }
        if let Some(raster) = self.animation.raster.clone() {
            let error = self.app.status.render_error.clone();
            self.present_raster(raster, center);
            if self.animation.failed {
                self.app.status.render_error = error;
            }
        }
        if self.animation.failed || self.animation.busy() {
            return;
        }
        let now = self.app.elapsed_ms();
        let Some(definition) = &self.animation.definition else {
            return;
        };
        let sample = self.animation.playback.sample(definition, now);
        self.animation.next_frame = sample.next_frame_ms;
        if self.animation.pose.is_none() {
            let base = Playback::default().sample(definition, now).preset;
            if base == sample.preset {
                let raster = self
                    .preview
                    .as_ref()
                    .filter(|p| p.key == key)
                    .map(|p| p.raster.clone())
                    .or_else(|| self.cache.get(key));
                if let Some(raster) = raster {
                    self.animation.raster = Some(raster.clone());
                    self.animation.pose = Some(base);
                    self.present_raster(raster, center);
                } else if sample.next_frame_ms.is_none() {
                    if self.rendering != Some(key) && !self.failed_render.contains(&key) {
                        self.render_key(key);
                    }
                    return;
                }
            }
        }
        if self.animation.pose.as_ref() != Some(&sample.preset) {
            let ready = self.animation.ready_at(now);
            if ready > now {
                self.animation_timer(ready);
                return;
            }
            let Some(frame) = self.animation.begin_frame(&sample.preset, now) else {
                return;
            };
            self.rendering = None;
            self.render.submit(render::Job {
                key,
                frame: Some(frame),
                preset: sample.preset,
                assets: self.app.state.assets.clone(),
            });
        } else if let Some(next) = sample.next_frame_ms {
            self.animation_timer(self.animation.ready_at(next.max(now.saturating_add(17))));
        }
    }

    pub(super) fn animation_completed(&mut self, result: render::Completed) {
        if !self.animation.accept(result.frame.unwrap()) {
            return;
        }
        // Throttle presentation as well as submission when a slow job just completed.
        self.animation.last_submit = Some(self.app.elapsed_ms());
        match result.result {
            Ok(raster) => self.animation.raster = Some(raster),
            Err(error) => {
                self.animation.failed = true;
                self.stop_animation_timer();
                self.app.status.render_error = Some(format!(
                    "Animation stopped: {error}. Reduce its scale, movement or rotation."
                ));
                self.app.changed(None, false);
            }
        }
        self.refresh_presentation();
    }

    pub(super) fn present_raster(
        &mut self,
        raster: Arc<crosshair_renderer::Raster>,
        center: [i32; 2],
    ) {
        let origin = [center[0] + raster.origin[0], center[1] + raster.origin[1]];
        if self.app.state.settings.output_mode == OutputMode::GameBar {
            self.overlay.hide();
            if let Some(widget) = &self.widget {
                if self.last_image.as_ref().is_none_or(|(previous, position)| {
                    !Arc::ptr_eq(previous, &raster) || *position != origin
                }) || self.widget_center
                {
                    widget.present(Some(raster.clone()), origin, self.widget_center);
                    self.widget_center = false;
                    self.last_image = Some((raster, origin));
                }
                self.app.status.render_error = None;
            } else {
                self.app.status.render_error =
                    Some("Install the Windows package to use Game Bar output".into());
            }
        } else {
            if !self.changing_output()
                && let Some(widget) = &self.widget
            {
                widget.present(None, [0, 0], false);
            }
            if self.last_image.as_ref().is_none_or(|(previous, position)| {
                !Arc::ptr_eq(previous, &raster) || *position != origin
            }) {
                match self
                    .overlay
                    .present_at([raster.width, raster.height], &raster.pixels, origin)
                {
                    Ok(()) => {
                        self.app.status.render_error = None;
                        self.last_image = Some((raster, origin));
                    }
                    Err(error) => {
                        self.hide();
                        self.app.status.render_error = Some(error.to_string());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_preview_or_metadata_preserves_playback_but_definition_edits_reset() {
        let mut preset = animated_rectangle(12.0);
        let animation = preset.layers[0].animation.as_mut().unwrap();
        animation.activation = Activation::Press;
        animation.repeat = true;
        let mut output = AnimationOutput::default();
        let key = |p: &Preset| Some(render::Key::new(p, 1.0));
        assert!(output.sync(key(&preset), Some(&preset), 0, [false; 2], 0));
        output.event(
            AnimationEvent::Trigger {
                trigger: AnimationTrigger::Fire,
                down: true,
            },
            0,
        );
        output.event(
            AnimationEvent::Trigger {
                trigger: AnimationTrigger::Fire,
                down: false,
            },
            1,
        );
        assert_eq!(
            output.playback.sample(&preset, 50).preset.layers[0]
                .transform
                .rotation,
            45.0
        );
        // Removing an identical or inactive preview resolves to this same definition.
        assert!(!output.sync(key(&preset), Some(&preset), 0, [false; 2], 50));
        preset.favorite = true;
        preset.name = "Renamed".into();
        preset.revision += 1;
        assert!(output.sync(key(&preset), Some(&preset), 0, [false; 2], 50));
        assert_eq!(
            output.playback.sample(&preset, 75).preset.layers[0]
                .transform
                .rotation,
            67.5
        );
        preset.layers[0].animation.as_mut().unwrap().duration_ms = 200;
        assert!(output.sync(key(&preset), Some(&preset), 0, [false; 2], 75));
        assert_eq!(
            output.playback.sample(&preset, 100).preset.layers[0]
                .transform
                .rotation,
            0.0
        );
    }

    fn animated_rectangle(size: f32) -> Preset {
        let mut preset = Preset::new("animation fixture");
        preset.layers = vec![Layer::new(
            "rectangle",
            Content::Rectangle(Rectangle {
                width: size,
                height: size,
                color: Color::rgba(200, 80, 40, 128),
                ..Default::default()
            }),
        )];
        preset.layers[0].animation = Some(Animation {
            easing: Easing::Linear,
            duration_ms: 100,
            target: AnimationTarget {
                rotation: Some(90.0),
                ..Default::default()
            },
            ..Default::default()
        });
        preset
    }

    fn dispatch(
        output: &mut AnimationOutput,
        input: &mut InputState,
        state: &mut AppState,
        key: InputKey,
        down: bool,
        now: u64,
    ) -> bool {
        let events = input
            .handle(
                InputEvent {
                    device: 1,
                    key,
                    down,
                },
                now,
                state,
            )
            .animation_events;
        assert_eq!(events.len(), 1);
        output.event(events[0], now)
    }

    #[test]
    fn fire_reset_rejects_pending_frame_while_aim_continues_and_reload_clears_both() -> Result<()> {
        let mut preset = animated_rectangle(12.0);
        preset.layers[0].animation.as_mut().unwrap().release = Release::Reset;
        let mut aiming = preset.layers[0].clone();
        aiming.id = Id::new_v4();
        aiming.animation.as_mut().unwrap().trigger = AnimationTrigger::Aim;
        preset.layers.push(aiming);
        let mut output = AnimationOutput {
            definition: Some(preset.clone()),
            ..Default::default()
        };
        let mut state = AppState::default();
        let mut input = InputState::default();
        let fire = InputKey::Mouse { button: 1 };
        let aim = InputKey::Mouse { button: 2 };
        assert!(dispatch(&mut output, &mut input, &mut state, aim, true, 0));
        assert!(dispatch(
            &mut output,
            &mut input,
            &mut state,
            fire,
            true,
            10
        ));
        let mut renderer = crosshair_renderer::Renderer::new()?;
        let valid = renderer.render(&output.playback.sample(&preset, 30).preset, 1.0, &|_| {
            anyhow::bail!("Unexpected asset")
        })?;
        output.raster = Some(valid.clone());
        let pending_pose = output.playback.sample(&preset, 60).preset;
        let pending = output.begin_frame(&pending_pose, 60).unwrap();
        assert!(dispatch(
            &mut output,
            &mut input,
            &mut state,
            fire,
            false,
            70
        ));
        assert!(output.pose.is_none());
        assert!(Arc::ptr_eq(output.raster.as_ref().unwrap(), &valid));
        assert!(!output.accept(pending));
        let reset = output.playback.sample(&preset, 70);
        assert_eq!(reset.preset.layers[0].transform.rotation, 0.0);
        assert_eq!(reset.preset.layers[1].transform.rotation, 63.0);
        assert!(reset.next_frame_ms.is_some());
        let next = output.begin_frame(&reset.preset, 70).unwrap();
        assert!(!output.accept(pending));
        assert!(output.busy());

        assert!(dispatch(
            &mut output,
            &mut input,
            &mut state,
            InputChord::key(0x13).trigger,
            true,
            80
        ));
        assert!(!output.accept(next));
        let reset = output.playback.sample(&preset, 80);
        assert!(
            reset
                .preset
                .layers
                .iter()
                .all(|layer| layer.transform.rotation == 0.0)
        );
        assert!(reset.next_frame_ms.is_none());
        assert!(Arc::ptr_eq(output.raster.as_ref().unwrap(), &valid));
        Ok(())
    }

    #[test]
    fn press_repeat_toggle_off_invalidates_pending_frame_and_returns_to_cached_base() {
        let mut preset = animated_rectangle(12.0);
        let animation = preset.layers[0].animation.as_mut().unwrap();
        animation.activation = Activation::Press;
        animation.repeat = true;
        let mut output = AnimationOutput {
            definition: Some(preset.clone()),
            ..Default::default()
        };
        let mut state = AppState::default();
        let mut input = InputState::default();
        let fire = InputKey::Mouse { button: 1 };
        assert!(dispatch(&mut output, &mut input, &mut state, fire, true, 0));
        assert!(!dispatch(
            &mut output,
            &mut input,
            &mut state,
            fire,
            false,
            1
        ));
        let pending = output
            .begin_frame(&output.playback.sample(&preset, 50).preset, 50)
            .unwrap();
        assert!(dispatch(
            &mut output,
            &mut input,
            &mut state,
            fire,
            true,
            60
        ));
        assert!(!output.accept(pending));
        assert!(output.pose.is_none());
        let sample = output.playback.sample(&preset, 60);
        assert_eq!(
            sample.preset,
            Playback::default().sample(&preset, 60).preset
        );
        assert!(sample.next_frame_ms.is_none());
    }

    #[test]
    fn irrelevant_and_no_change_animation_events_preserve_idle_pose() {
        let mut preset = animated_rectangle(12.0);
        preset.layers[0].animation.as_mut().unwrap().target.rotation = Some(0.0);
        let base = Playback::default().sample(&preset, 0).preset;
        let mut output = AnimationOutput {
            definition: Some(preset),
            pose: Some(base.clone()),
            ..Default::default()
        };
        for event in [
            AnimationEvent::Trigger {
                trigger: AnimationTrigger::Fire,
                down: true,
            },
            AnimationEvent::Trigger {
                trigger: AnimationTrigger::Aim,
                down: true,
            },
            AnimationEvent::Reload,
        ] {
            assert!(!output.event(event, 10));
            assert_eq!(output.pose.as_ref(), Some(&base));
            assert!(output.begin_frame(&base, 10).is_none());
        }
    }

    #[test]
    fn sampled_alpha_rotation_and_bounds_match_ordinary_output_pixels() -> Result<()> {
        let mut preset = animated_rectangle(12.0);
        let target = &mut preset.layers[0].animation.as_mut().unwrap().target;
        target.opacity = Some(0.0);
        target.x = Some(8.0);
        let mut playback = Playback::default();
        playback.select(&preset, false, true, 0);
        let sample = playback.sample(&preset, 50).preset;
        let mut expected = preset;
        expected.layers[0].animation = None;
        expected.layers[0].transform.rotation = 45.0;
        expected.layers[0].transform.x = 4.0;
        expected.layers[0].opacity = 0.5;
        let mut renderer = crosshair_renderer::Renderer::new()?;
        let no_assets = |_| anyhow::bail!("Unexpected asset request");
        let actual = renderer.render(&sample, 1.0, &no_assets)?;
        let expected = renderer.render(&expected, 1.0, &no_assets)?;
        assert_eq!(actual.origin, expected.origin);
        assert_eq!(
            (actual.width, actual.height),
            (expected.width, expected.height)
        );
        assert_eq!(actual.pixels, expected.pixels);
        assert_eq!(actual.to_bgra(), expected.to_bgra());
        assert!(actual.pixels.chunks_exact(4).all(|p| p[3] <= 64));
        Ok(())
    }

    #[test]
    fn intermediate_bounds_failure_keeps_last_raster_and_stops_new_jobs() -> Result<()> {
        let preset = animated_rectangle(800.0);
        let mut renderer = crosshair_renderer::Renderer::new()?;
        render::validate_animation_bounds(&mut renderer, &preset, 1.0)?;
        let no_assets = |_| anyhow::bail!("Unexpected asset request");
        let raster = renderer.render(&preset, 1.0, &no_assets)?;
        let mut output = AnimationOutput {
            raster: Some(raster.clone()),
            ..Default::default()
        };
        output.playback.select(&preset, false, true, 0);
        let pose = output.playback.sample(&preset, 50).preset;
        assert!(renderer.render(&pose, 1.0, &no_assets).is_err());
        output.failed = true;
        assert!(output.begin_frame(&pose, 50).is_none());
        assert!(Arc::ptr_eq(output.raster.as_ref().unwrap(), &raster));
        Ok(())
    }

    #[test]
    #[ignore = "synthetic animation raster throughput; run deliberately in release mode"]
    fn animation_raster_throughput() -> Result<()> {
        let mut preset = animated_rectangle(128.0);
        preset.layers[0].effects.blur = 8.0;
        preset.layers[0].animation.as_mut().unwrap().repeat = true;
        let mut playback = Playback::default();
        playback.select(&preset, false, true, 0);
        let mut renderer = crosshair_renderer::Renderer::new()?;
        let mut timings = Vec::new();
        for now in (0..5100).step_by(17) {
            let started = Instant::now();
            let pose = playback.sample(&preset, now).preset;
            let raster = renderer.render(&pose, 1.0, &|_| anyhow::bail!("Unexpected asset"))?;
            assert!(raster.width <= 1024 && raster.height <= 1024);
            renderer.clear_raster_cache();
            timings.push(started.elapsed().as_micros());
        }
        timings.sort_unstable();
        println!(
            "synthetic sample+raster (300 frames): median={}us p95={}us max={}us; native presentation and real input are not measured",
            timings[150], timings[285], timings[299]
        );
        Ok(())
    }

    #[test]
    fn slow_render_is_not_replaced_and_reset_rejects_its_completion() {
        let mut output = AnimationOutput::default();
        let mut pose = Preset::new("slow fixture");
        assert_eq!(output.begin_frame(&pose, 10), Some((0, 10)));
        for now in 11..500 {
            pose.layers[0].transform.x = now as f32;
            assert!(output.begin_frame(&pose, now).is_none());
            assert!(output.busy(), "tick {now} must wait for the valid render");
        }
        assert!(output.accept((0, 10)));
        assert!(!output.busy());
        assert_eq!(output.begin_frame(&pose, 500), Some((0, 500)));
        assert_eq!(output.pose.as_ref().unwrap().layers[0].transform.x, 499.0);
        output.invalidate();
        assert!(!output.busy());
        assert!(!output.accept((0, 500)));
    }

    #[test]
    fn idle_and_equal_targets_have_no_animation_work_and_frames_are_capped() {
        let mut output = AnimationOutput::default();
        let mut preset = Preset::new("idle");
        output.definition = Some(preset.clone());
        assert!(!output.active());
        preset.layers[0].animation = Some(Animation {
            repeat: true,
            target: AnimationTarget {
                x: Some(preset.layers[0].transform.x),
                ..Default::default()
            },
            ..Default::default()
        });
        output.definition = Some(preset);
        assert!(!output.active());
        output.last_submit = Some(100);
        assert_eq!(output.ready_at(101), 117);
        assert_eq!(output.ready_at(900), 900);
    }
}

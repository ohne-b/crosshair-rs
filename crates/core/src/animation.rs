use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Color, Content, Id, Layer, Preset};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationTrigger {
    Aim,
    #[default]
    Fire,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activation {
    #[default]
    Hold,
    Press,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Release {
    #[default]
    Return,
    Reset,
    Pause,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    Linear,
    #[default]
    Smooth,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    OneWay,
    BackAndForth,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnimationTarget {
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub scale: Option<f32>,
    pub rotation: Option<f32>,
    pub opacity: Option<f32>,
    pub length: Option<f32>,
    pub thickness: Option<f32>,
    pub gap: Option<f32>,
    pub dot_diameter: Option<f32>,
    pub color: Option<Color>,
    pub dot_color: Option<Color>,
}

impl AnimationTarget {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    /// Explicit values equal to Base are static, including when Repeat is enabled.
    pub fn changes(&self, layer: &Layer) -> bool {
        let differs = |value: Option<f32>, base: f32| value.is_some_and(|value| value != base);
        if differs(self.x, layer.transform.x)
            || differs(self.y, layer.transform.y)
            || differs(self.scale, layer.transform.scale)
            || differs(self.rotation, layer.transform.rotation)
            || differs(self.opacity, layer.opacity)
        {
            return true;
        }
        match &layer.content {
            Content::Crosshair(shape) => {
                differs(self.length, shape.length)
                    || differs(self.thickness, shape.thickness)
                    || differs(self.gap, shape.gap)
                    || differs(self.dot_diameter, shape.dot.diameter)
                    || self.color.is_some_and(|color| color != shape.color)
                    || self.dot_color.is_some_and(|color| color != shape.dot.color)
            }
            Content::Rectangle(shape) => self.color.is_some_and(|color| color != shape.color),
            Content::Ellipse(shape) | Content::Triangle(shape) => {
                self.color.is_some_and(|color| color != shape.color)
            }
            _ => false,
        }
    }

    /// Apply a fraction of the target to a fresh base layer; never sample a sampled layer.
    pub fn apply(&self, layer: &mut Layer, fraction: f32) {
        let t = fraction.clamp(0.0, 1.0);
        let mix = |base: &mut f32, target: Option<f32>| {
            if let Some(target) = target {
                *base += (target - *base) * t;
            }
        };
        mix(&mut layer.transform.x, self.x);
        mix(&mut layer.transform.y, self.y);
        mix(&mut layer.transform.scale, self.scale);
        mix(&mut layer.transform.rotation, self.rotation);
        mix(&mut layer.opacity, self.opacity);
        match &mut layer.content {
            Content::Crosshair(shape) => {
                mix(&mut shape.length, self.length);
                mix(&mut shape.thickness, self.thickness);
                mix(&mut shape.gap, self.gap);
                mix(&mut shape.dot.diameter, self.dot_diameter);
                if let Some(color) = self.color {
                    shape.color = mix_color(shape.color, color, t);
                }
                if let Some(color) = self.dot_color {
                    shape.dot.color = mix_color(shape.dot.color, color, t);
                }
            }
            Content::Rectangle(shape) => {
                if let Some(color) = self.color {
                    shape.color = mix_color(shape.color, color, t);
                }
            }
            Content::Ellipse(shape) | Content::Triangle(shape) => {
                if let Some(color) = self.color {
                    shape.color = mix_color(shape.color, color, t);
                }
            }
            _ => {}
        }
    }
}

fn mix_color(base: Color, target: Color, t: f32) -> Color {
    if t <= 0.0 {
        return base;
    }
    if t >= 1.0 {
        return target;
    }
    let a = f32::from(base.a) + (f32::from(target.a) - f32::from(base.a)) * t;
    let channel = |b: u8, v: u8| {
        if a <= 0.0 {
            0
        } else {
            ((f32::from(b) * f32::from(base.a) * (1.0 - t)
                + f32::from(v) * f32::from(target.a) * t)
                / a)
                .round()
                .clamp(0.0, 255.0) as u8
        }
    };
    Color::rgba(
        channel(base.r, target.r),
        channel(base.g, target.g),
        channel(base.b, target.b),
        a.round() as u8,
    )
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Animation {
    pub enabled: bool,
    pub trigger: AnimationTrigger,
    pub activation: Activation,
    pub release: Release,
    pub duration_ms: u32,
    pub delay_ms: u32,
    pub easing: Easing,
    pub repeat: bool,
    pub direction: Direction,
    pub target: AnimationTarget,
}

impl Default for Animation {
    fn default() -> Self {
        Self {
            enabled: true,
            trigger: AnimationTrigger::Fire,
            activation: Activation::Hold,
            release: Release::Return,
            duration_ms: 150,
            delay_ms: 0,
            easing: Easing::Smooth,
            repeat: false,
            direction: Direction::OneWay,
            target: AnimationTarget::default(),
        }
    }
}

impl Animation {
    pub fn effective(&self) -> bool {
        self.enabled && !self.target.is_empty()
    }

    pub fn effective_for(&self, layer: &Layer) -> bool {
        self.enabled && self.target.changes(layer)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationEvent {
    Trigger {
        trigger: AnimationTrigger,
        down: bool,
    },
    Reload,
}

#[derive(Clone, Copy, Debug)]
enum Track {
    Playing {
        start: u64,
        offset: u64,
        from: f32,
        delay: u32,
    },
    Paused {
        elapsed: u64,
        from: f32,
        delay: u32,
    },
    Returning {
        start: u64,
        from: f32,
    },
}

/// Runtime only. Call reset when the effective document/definition or output session changes.
#[derive(Default)]
pub struct Playback {
    tracks: BTreeMap<Id, Track>,
    held: [bool; 2],
    blocked: [bool; 2],
}

pub struct AnimationSample {
    /// Evaluated copy; animation definitions are removed to prevent double evaluation.
    pub preset: Preset,
    /// Next useful sampling time. None means the current pose is static or paused.
    pub next_frame_ms: Option<u64>,
}

impl Playback {
    pub fn reset(&mut self) {
        self.tracks.clear();
        self.held = [false; 2];
        self.blocked = [false; 2];
    }

    /// New effective crosshair inherits held inputs, but never synthesizes Press activations.
    pub fn select(&mut self, preset: &Preset, aim_held: bool, fire_held: bool, now_ms: u64) {
        self.tracks.clear();
        self.held = [aim_held, fire_held];
        for i in 0..2 {
            self.blocked[i] &= self.held[i];
        }
        for layer in &preset.layers {
            if let Some(a) = &layer.animation
                && a.effective_for(layer)
                && a.activation == Activation::Hold
                && self.held[index(a.trigger)]
                && !self.blocked[index(a.trigger)]
            {
                self.tracks.insert(
                    layer.id,
                    Track::Playing {
                        start: now_ms,
                        offset: 0,
                        from: 0.0,
                        delay: a.delay_ms,
                    },
                );
            }
        }
    }

    /// Accept logical edges (already filtered for physical key repeat by InputState).
    pub fn event(&mut self, preset: &Preset, event: AnimationEvent, now_ms: u64) {
        let AnimationEvent::Trigger { trigger, down } = event else {
            self.tracks.clear();
            self.blocked = self.held;
            return;
        };
        let i = index(trigger);
        self.held[i] = down;
        if !down {
            self.blocked[i] = false;
        }
        if self.blocked[i] {
            return;
        }
        for layer in &preset.layers {
            let Some(a) = layer
                .animation
                .as_ref()
                .filter(|a| a.effective_for(layer) && a.trigger == trigger)
            else {
                continue;
            };
            let previous = self.tracks.get(&layer.id).copied();
            if down {
                let next = match (a.activation, previous) {
                    (Activation::Press, Some(_)) if a.repeat => None,
                    (
                        Activation::Hold,
                        Some(Track::Paused {
                            elapsed,
                            from,
                            delay,
                        }),
                    ) => Some(Track::Playing {
                        start: now_ms,
                        offset: elapsed,
                        from,
                        delay,
                    }),
                    (Activation::Hold, Some(track @ Track::Returning { .. }))
                        if sample_track(track, a, now_ms).1.is_some() =>
                    {
                        Some(Track::Playing {
                            start: now_ms,
                            offset: 0,
                            from: sample_track(track, a, now_ms).0,
                            delay: 0,
                        })
                    }
                    _ => Some(Track::Playing {
                        start: now_ms,
                        offset: 0,
                        from: 0.0,
                        delay: a.delay_ms,
                    }),
                };
                if let Some(next) = next {
                    self.tracks.insert(layer.id, next);
                } else {
                    self.tracks.remove(&layer.id);
                }
            } else if a.activation == Activation::Hold
                && let Some(previous) = previous
            {
                let fraction = sample_track(previous, a, now_ms).0;
                match (a.release, previous) {
                    (
                        Release::Pause,
                        Track::Playing {
                            start,
                            offset,
                            from,
                            delay,
                        },
                    ) => {
                        let elapsed = now_ms.saturating_sub(start).saturating_add(offset);
                        // Releasing during the initial delay must not produce a delayed flash.
                        if elapsed < u64::from(delay) {
                            self.tracks.remove(&layer.id);
                        } else {
                            self.tracks.insert(
                                layer.id,
                                Track::Paused {
                                    elapsed,
                                    from,
                                    delay,
                                },
                            );
                        }
                    }
                    (Release::Return, _) if fraction > 0.0 => {
                        self.tracks.insert(
                            layer.id,
                            Track::Returning {
                                start: now_ms,
                                from: fraction,
                            },
                        );
                    }
                    _ => {
                        self.tracks.remove(&layer.id);
                    }
                }
            }
        }
    }

    pub fn sample(&self, base: &Preset, now_ms: u64) -> AnimationSample {
        let mut preset = base.clone();
        let mut next_frame_ms: Option<u64> = None;
        for layer in &mut preset.layers {
            let animation = layer.animation.take();
            if !layer.visible {
                continue;
            }
            if let Some(a) = animation.filter(|a| a.effective_for(layer))
                && let Some(track) = self.tracks.get(&layer.id)
            {
                let (fraction, next) = sample_track(*track, &a, now_ms);
                a.target.apply(layer, fraction);
                next_frame_ms = match (next_frame_ms, next) {
                    (Some(left), Some(right)) => Some(left.min(right)),
                    (left, right) => left.or(right),
                };
            }
        }
        AnimationSample {
            preset,
            next_frame_ms,
        }
    }
}

fn index(trigger: AnimationTrigger) -> usize {
    match trigger {
        AnimationTrigger::Aim => 0,
        AnimationTrigger::Fire => 1,
    }
}

fn eased(easing: Easing, t: f32) -> f32 {
    match easing {
        Easing::Linear => t,
        Easing::Smooth => t * t * (3.0 - 2.0 * t),
    }
}

fn sample_track(track: Track, a: &Animation, now: u64) -> (f32, Option<u64>) {
    let duration = u64::from(a.duration_ms);
    let next = || Some(now.saturating_add(17));
    if let Track::Returning { start, from } = track {
        let elapsed = now.saturating_sub(start);
        if elapsed >= duration {
            return (0.0, None);
        }
        return (
            from * (1.0 - eased(a.easing, elapsed as f32 / duration as f32)),
            next(),
        );
    }
    let (elapsed, from, delay, paused) = match track {
        Track::Playing {
            start,
            offset,
            from,
            delay,
        } => (
            now.saturating_sub(start).saturating_add(offset),
            from,
            delay,
            false,
        ),
        Track::Paused {
            elapsed,
            from,
            delay,
        } => (elapsed, from, delay, true),
        Track::Returning { .. } => unreachable!(),
    };
    if elapsed < u64::from(delay) {
        return (
            from,
            (!paused).then(|| now.saturating_add(u64::from(delay) - elapsed)),
        );
    }
    let elapsed = elapsed - u64::from(delay);
    if duration == 0 {
        let at_target =
            a.direction == Direction::OneWay && (a.activation == Activation::Hold || a.repeat);
        return (if at_target { 1.0 } else { 0.0 }, None);
    }
    let cycle = duration
        * if a.direction == Direction::BackAndForth {
            2
        } else {
            1
        };
    if !a.repeat && elapsed >= cycle {
        return (
            if a.activation == Activation::Hold && a.direction == Direction::OneWay {
                1.0
            } else {
                0.0
            },
            None,
        );
    }
    let position = elapsed % cycle;
    let fraction = if position >= duration {
        1.0 - eased(a.easing, (position - duration) as f32 / duration as f32)
    } else {
        let origin = if elapsed < cycle { from } else { 0.0 };
        origin + (1.0 - origin) * eased(a.easing, position as f32 / duration as f32)
    };
    (fraction, (!paused).then(next).flatten())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_equal_to_base_do_not_schedule_repeating_frames() {
        let mut p = design();
        let Content::Crosshair(shape) = &p.layers[0].content else {
            unreachable!()
        };
        let color = shape.color;
        p.layers[0].transform.x = 100.0;
        let a = p.layers[0].animation.as_mut().unwrap();
        a.repeat = true;
        a.target.color = Some(color);
        let mut playback = Playback::default();
        playback.event(&p, fire(true), 0);
        assert_eq!(playback.sample(&p, 10).next_frame_ms, None);
        close(x(&playback, &p, 10), 100.0);
        playback.select(&p, false, true, 20);
        assert_eq!(playback.sample(&p, 20).next_frame_ms, None);
    }

    fn design() -> Preset {
        let mut p = Preset::new("Test");
        p.layers[0].animation = Some(Animation {
            duration_ms: 100,
            easing: Easing::Linear,
            target: AnimationTarget {
                x: Some(100.0),
                ..Default::default()
            },
            ..Default::default()
        });
        p
    }
    fn fire(down: bool) -> AnimationEvent {
        AnimationEvent::Trigger {
            trigger: AnimationTrigger::Fire,
            down,
        }
    }
    fn x(playback: &Playback, p: &Preset, now: u64) -> f32 {
        playback.sample(p, now).preset.layers[0].transform.x
    }
    fn close(left: f32, right: f32) {
        assert!((left - right).abs() < 0.001, "{left} != {right}");
    }

    #[test]
    fn hold_return_and_interrupted_return_start_at_the_current_pose() {
        let p = design();
        let mut playback = Playback::default();
        playback.event(&p, fire(true), 0);
        close(x(&playback, &p, 50), 50.0);
        playback.event(&p, fire(false), 50);
        close(x(&playback, &p, 100), 25.0);
        playback.event(&p, fire(true), 100);
        close(x(&playback, &p, 100), 25.0);
        close(x(&playback, &p, 150), 62.5);
        close(x(&playback, &p, 200), 100.0);
        assert_eq!(playback.sample(&p, 200).next_frame_ms, None);
        playback.event(&p, fire(false), 200);
        close(x(&playback, &p, 300), 0.0);
        assert_eq!(playback.sample(&p, 300).next_frame_ms, None);
        assert_eq!(p.layers[0].transform.x, 0.0);
        assert!(playback.sample(&p, 50).preset.layers[0].animation.is_none());
    }

    #[test]
    fn instant_hold_honors_delay_release_and_repeated_presses_without_frames() {
        for release in [Release::Return, Release::Reset, Release::Pause] {
            let mut p = design();
            let a = p.layers[0].animation.as_mut().unwrap();
            a.duration_ms = 0;
            a.delay_ms = 20;
            a.release = release;
            let mut playback = Playback::default();
            playback.event(&p, fire(true), 0);
            close(x(&playback, &p, 19), 0.0);
            assert_eq!(playback.sample(&p, 19).next_frame_ms, Some(20));
            close(x(&playback, &p, 20), 100.0);
            assert_eq!(playback.sample(&p, 20).next_frame_ms, None);
            playback.event(&p, fire(false), 20);
            let paused = release == Release::Pause;
            close(x(&playback, &p, 20), if paused { 100.0 } else { 0.0 });
            assert_eq!(playback.sample(&p, 20).next_frame_ms, None);
            playback.event(&p, fire(true), 21);
            close(x(&playback, &p, 21), if paused { 100.0 } else { 0.0 });
            close(x(&playback, &p, 41), 100.0);
        }
    }

    #[test]
    fn instant_press_and_repeat_settle_without_zero_length_loops() {
        let mut p = design();
        let a = p.layers[0].animation.as_mut().unwrap();
        a.duration_ms = 0;
        a.activation = Activation::Press;
        let mut playback = Playback::default();
        playback.event(&p, fire(true), 0);
        close(x(&playback, &p, 0), 0.0);
        assert_eq!(playback.sample(&p, 0).next_frame_ms, None);
        p.layers[0].animation.as_mut().unwrap().repeat = true;
        playback.reset();
        playback.event(&p, fire(true), 1);
        close(x(&playback, &p, 1), 100.0);
        assert_eq!(playback.sample(&p, 1).next_frame_ms, None);
        playback.event(&p, fire(false), 2);
        playback.event(&p, fire(true), 3);
        close(x(&playback, &p, 3), 0.0);
        p.layers[0].animation.as_mut().unwrap().direction = Direction::BackAndForth;
        playback.event(&p, fire(true), 4);
        close(x(&playback, &p, 4), 0.0);
        assert_eq!(playback.sample(&p, 4).next_frame_ms, None);
    }

    #[test]
    fn a_completed_return_restarts_the_initial_delay() {
        let mut p = design();
        p.layers[0].animation.as_mut().unwrap().delay_ms = 20;
        let mut playback = Playback::default();
        playback.event(&p, fire(true), 0);
        playback.event(&p, fire(false), 120);
        playback.event(&p, fire(true), 220);
        close(x(&playback, &p, 239), 0.0);
        assert_eq!(playback.sample(&p, 239).next_frame_ms, Some(240));
        close(x(&playback, &p, 290), 50.0);
    }

    #[test]
    fn pause_resume_reload_and_lifecycle_reset_rearm_cleanly() {
        let mut p = design();
        p.layers[0].animation.as_mut().unwrap().release = Release::Pause;
        let mut playback = Playback::default();
        playback.event(&p, fire(true), 0);
        playback.event(&p, fire(false), 40);
        close(x(&playback, &p, 900), 40.0);
        assert_eq!(playback.sample(&p, 900).next_frame_ms, None);
        playback.event(&p, fire(true), 900);
        close(x(&playback, &p, 930), 70.0);
        playback.event(&p, AnimationEvent::Reload, 930);
        close(x(&playback, &p, 950), 0.0);
        playback.select(&p, false, true, 950);
        close(x(&playback, &p, 990), 0.0);
        playback.event(&p, fire(false), 1000);
        playback.event(&p, fire(true), 1001);
        close(x(&playback, &p, 1051), 50.0);
        playback.reset();
        playback.event(&p, fire(true), 2000);
        close(x(&playback, &p, 2050), 50.0);
    }

    #[test]
    fn delay_releases_do_not_flash_and_resume_does_not_repeat_delay() {
        let mut p = design();
        let a = p.layers[0].animation.as_mut().unwrap();
        a.delay_ms = 200;
        a.release = Release::Pause;
        let mut playback = Playback::default();
        playback.event(&p, fire(true), 0);
        assert_eq!(playback.sample(&p, 10).next_frame_ms, Some(200));
        playback.event(&p, fire(false), 100);
        close(x(&playback, &p, 500), 0.0);
        assert_eq!(playback.sample(&p, 500).next_frame_ms, None);
        playback.event(&p, fire(true), 1000);
        playback.event(&p, fire(false), 1250);
        playback.event(&p, fire(true), 2000);
        close(x(&playback, &p, 2025), 75.0);
    }

    #[test]
    fn press_pulse_finishes_and_repeated_press_loop_toggles() {
        let mut p = design();
        p.layers[0].animation.as_mut().unwrap().activation = Activation::Press;
        let mut playback = Playback::default();
        playback.event(&p, fire(true), 0);
        playback.event(&p, fire(false), 1);
        close(x(&playback, &p, 50), 50.0);
        close(x(&playback, &p, 100), 0.0);
        assert_eq!(playback.sample(&p, 100).next_frame_ms, None);
        p.layers[0].animation.as_mut().unwrap().repeat = true;
        playback.reset();
        playback.event(&p, fire(true), 0);
        playback.event(&p, fire(false), 1);
        close(x(&playback, &p, 250), 50.0);
        playback.event(&p, fire(true), 250);
        close(x(&playback, &p, 260), 0.0);
    }

    #[test]
    fn alternating_hold_finishes_at_base_and_repeat_uses_elapsed_time() {
        let mut p = design();
        p.layers[0].animation.as_mut().unwrap().direction = Direction::BackAndForth;
        let mut playback = Playback::default();
        playback.event(&p, fire(true), 0);
        close(x(&playback, &p, 100), 100.0);
        close(x(&playback, &p, 150), 50.0);
        close(x(&playback, &p, 200), 0.0);
        assert_eq!(playback.sample(&p, 200).next_frame_ms, None);
        p.layers[0].animation.as_mut().unwrap().repeat = true;
        close(x(&playback, &p, 10_150), 50.0);
        assert!(playback.sample(&p, 10_150).next_frame_ms.is_some());
    }

    #[test]
    fn selection_inherits_holds_but_not_presses_and_triggers_are_independent() {
        let mut p = design();
        let mut aim = p.layers[0].duplicate();
        aim.animation.as_mut().unwrap().trigger = AnimationTrigger::Aim;
        p.layers.push(aim);
        let mut playback = Playback::default();
        playback.select(&p, true, false, 0);
        let sample = playback.sample(&p, 50);
        close(sample.preset.layers[0].transform.x, 0.0);
        close(sample.preset.layers[1].transform.x, 50.0);
        p.layers[1].animation.as_mut().unwrap().activation = Activation::Press;
        playback.select(&p, true, false, 100);
        close(playback.sample(&p, 150).preset.layers[1].transform.x, 0.0);
        // The actual edge selecting this design may still trigger a Press animation.
        playback.event(
            &p,
            AnimationEvent::Trigger {
                trigger: AnimationTrigger::Aim,
                down: true,
            },
            150,
        );
        close(playback.sample(&p, 200).preset.layers[1].transform.x, 50.0);
    }

    #[test]
    fn empty_disabled_and_hidden_layers_never_request_frames() {
        let mut p = design();
        let mut playback = Playback::default();
        p.layers[0].animation.as_mut().unwrap().target = AnimationTarget::default();
        playback.event(&p, fire(true), 0);
        assert_eq!(playback.sample(&p, 20).next_frame_ms, None);
        p = design();
        p.layers[0].animation.as_mut().unwrap().enabled = false;
        playback.event(&p, fire(true), 0);
        assert_eq!(playback.sample(&p, 20).next_frame_ms, None);
        p = design();
        p.layers[0].visible = false;
        playback.event(&p, fire(true), 0);
        assert_eq!(playback.sample(&p, 20).next_frame_ms, None);
    }

    #[test]
    fn color_interpolation_is_premultiplied_and_keeps_exact_endpoints() {
        let transparent_red = Color::rgba(255, 0, 0, 0);
        let blue = Color::rgba(0, 0, 255, 255);
        assert_eq!(mix_color(transparent_red, blue, 0.0), transparent_red);
        assert_eq!(mix_color(transparent_red, blue, 1.0), blue);
        assert_eq!(
            mix_color(transparent_red, blue, 0.5),
            Color::rgba(0, 0, 255, 128)
        );
    }
}

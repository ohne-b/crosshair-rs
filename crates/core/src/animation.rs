use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Color, Content, Id, Layer, Preset};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationTrigger {
    Aim,
    #[default]
    Fire,
    Movement,
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

    /// Each effect contributes its delta from Base, independent of activation order.
    pub fn add_to(&self, layer: &mut Layer, base: &Layer, fraction: f32) {
        let t = fraction.clamp(0.0, 1.0);
        let add = |value: &mut f32, base: f32, target: Option<f32>| {
            if let Some(target) = target {
                *value += (target - base) * t;
            }
        };
        add(&mut layer.transform.x, base.transform.x, self.x);
        add(&mut layer.transform.y, base.transform.y, self.y);
        add(&mut layer.transform.scale, base.transform.scale, self.scale);
        add(
            &mut layer.transform.rotation,
            base.transform.rotation,
            self.rotation,
        );
        add(&mut layer.opacity, base.opacity, self.opacity);
        match (&mut layer.content, &base.content) {
            (Content::Crosshair(shape), Content::Crosshair(base)) => {
                add(&mut shape.length, base.length, self.length);
                add(&mut shape.thickness, base.thickness, self.thickness);
                add(&mut shape.gap, base.gap, self.gap);
                add(
                    &mut shape.dot.diameter,
                    base.dot.diameter,
                    self.dot_diameter,
                );
                if let Some(color) = self.color {
                    shape.color = mix_color(shape.color, color, t);
                }
                if let Some(color) = self.dot_color {
                    shape.dot.color = mix_color(shape.dot.color, color, t);
                }
            }
            (Content::Rectangle(shape), _) => {
                if let Some(color) = self.color {
                    shape.color = mix_color(shape.color, color, t);
                }
            }
            (Content::Ellipse(shape) | Content::Triangle(shape), _) => {
                if let Some(color) = self.color {
                    shape.color = mix_color(shape.color, color, t);
                }
            }
            _ => {}
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

pub fn clamp_animated_layer(layer: &mut Layer) {
    layer.transform.x = layer.transform.x.clamp(-4096.0, 4096.0);
    layer.transform.y = layer.transform.y.clamp(-4096.0, 4096.0);
    layer.transform.scale = layer.transform.scale.clamp(0.01, 100.0);
    layer.transform.rotation = layer.transform.rotation.clamp(-360.0, 360.0);
    layer.opacity = layer.opacity.clamp(0.0, 1.0);
    if let Content::Crosshair(shape) = &mut layer.content {
        shape.length = shape.length.clamp(0.0, 512.0);
        shape.thickness = shape.thickness.clamp(0.0, 512.0);
        shape.gap = shape.gap.clamp(0.0, 512.0);
        shape.dot.diameter = shape.dot.diameter.clamp(0.0, 1024.0);
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
    tracks: BTreeMap<(Id, AnimationTrigger), Track>,
    held: [bool; 3],
    blocked: [bool; 3],
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
        self.held = [false; 3];
        self.blocked = [false; 3];
    }

    /// New effective crosshair inherits held inputs, but never synthesizes Press activations.
    pub fn select(&mut self, preset: &Preset, held: [bool; 3], now_ms: u64) {
        self.tracks.clear();
        self.held = held;
        for i in 0..3 {
            self.blocked[i] &= self.held[i];
        }
        for layer in &preset.layers {
            for a in layer.animations.iter().filter(|a| {
                a.effective_for(layer)
                    && a.activation == Activation::Hold
                    && self.held[index(a.trigger)]
                    && !self.blocked[index(a.trigger)]
            }) {
                self.tracks.insert(
                    (layer.id, a.trigger),
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
                .animations
                .iter()
                .find(|a| a.effective_for(layer) && a.trigger == trigger)
            else {
                continue;
            };
            let previous = self.tracks.get(&(layer.id, a.trigger)).copied();
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
                    self.tracks.insert((layer.id, a.trigger), next);
                } else {
                    self.tracks.remove(&(layer.id, a.trigger));
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
                            self.tracks.remove(&(layer.id, a.trigger));
                        } else {
                            self.tracks.insert(
                                (layer.id, a.trigger),
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
                            (layer.id, a.trigger),
                            Track::Returning {
                                start: now_ms,
                                from: fraction,
                            },
                        );
                    }
                    _ => {
                        self.tracks.remove(&(layer.id, a.trigger));
                    }
                }
            }
        }
    }

    pub fn sample(&self, base: &Preset, now_ms: u64) -> AnimationSample {
        let mut preset = base.clone();
        let mut next_frame_ms: Option<u64> = None;
        for (layer, base_layer) in preset.layers.iter_mut().zip(&base.layers) {
            let mut animations = std::mem::take(&mut layer.animations);
            animations.sort_by_key(|a| match a.trigger {
                AnimationTrigger::Movement => 0,
                AnimationTrigger::Aim => 1,
                AnimationTrigger::Fire => 2,
            });
            if !layer.visible || animations.is_empty() {
                continue;
            }
            for a in animations.iter().filter(|a| a.effective_for(base_layer)) {
                if let Some(track) = self.tracks.get(&(layer.id, a.trigger)) {
                    let (fraction, next) = sample_track(*track, a, now_ms);
                    a.target.add_to(layer, base_layer, fraction);
                    next_frame_ms = match (next_frame_ms, next) {
                        (Some(left), Some(right)) => Some(left.min(right)),
                        (left, right) => left.or(right),
                    };
                }
            }
            clamp_animated_layer(layer);
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
        AnimationTrigger::Movement => 2,
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

pub(crate) fn read_animations<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Animation>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Definitions {
        Many(Vec<Animation>),
        Legacy(Option<Animation>),
    }
    Ok(match Definitions::deserialize(deserializer)? {
        Definitions::Many(value) => value,
        Definitions::Legacy(value) => value.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simultaneous_triggers_add_from_base_and_release_independently() {
        let mut p = design();
        p.layers[0].transform.x = 10.0;
        p.layers[0].animations = [
            (AnimationTrigger::Movement, 30.0),
            (AnimationTrigger::Aim, 5.0),
            (AnimationTrigger::Fire, 20.0),
        ]
        .into_iter()
        .map(|(trigger, x)| Animation {
            trigger,
            duration_ms: 100,
            easing: Easing::Linear,
            target: AnimationTarget {
                x: Some(x),
                ..Default::default()
            },
            ..Default::default()
        })
        .collect();
        let mut playback = Playback::default();
        for trigger in [
            AnimationTrigger::Movement,
            AnimationTrigger::Aim,
            AnimationTrigger::Fire,
        ] {
            playback.event(
                &p,
                AnimationEvent::Trigger {
                    trigger,
                    down: true,
                },
                0,
            );
        }
        close(x(&playback, &p, 50), 22.5);
        close(x(&playback, &p, 100), 35.0);
        playback.event(&p, fire(false), 100);
        close(x(&playback, &p, 150), 30.0);
        close(x(&playback, &p, 200), 25.0);
        assert!(playback.sample(&p, 200).next_frame_ms.is_none());
        playback.event(&p, AnimationEvent::Reload, 200);
        close(x(&playback, &p, 200), 10.0);
        playback.event(&p, fire(true), 200);
        close(x(&playback, &p, 300), 20.0);
        playback.event(
            &p,
            AnimationEvent::Trigger {
                trigger: AnimationTrigger::Aim,
                down: true,
            },
            200,
        );
        close(x(&playback, &p, 300), 20.0); // Aim was held during Reload and must rearm.
        p.layers[0].animations[2].enabled = false;
        playback.select(&p, [true, true, true], 300);
        close(x(&playback, &p, 400), 10.0);
    }

    #[test]
    fn combined_targets_clamp_once_and_color_priority_is_definition_order_independent() {
        let mut p = design();
        p.layers[0].opacity = 0.5;
        p.layers[0].animations = [
            AnimationTrigger::Movement,
            AnimationTrigger::Aim,
            AnimationTrigger::Fire,
        ]
        .into_iter()
        .map(|trigger| Animation {
            trigger,
            duration_ms: 0,
            target: AnimationTarget {
                opacity: Some(if trigger == AnimationTrigger::Aim {
                    0.0
                } else {
                    1.0
                }),
                color: Some(if trigger == AnimationTrigger::Fire {
                    Color::rgba(255, 0, 0, 255)
                } else {
                    Color::rgba(0, 255, 0, 255)
                }),
                ..Default::default()
            },
            ..Default::default()
        })
        .collect();
        let mut playback = Playback::default();
        playback.select(&p, [true; 3], 0);
        let sample = playback.sample(&p, 0);
        assert_eq!(sample.preset.layers[0].opacity, 1.0);
        let Content::Crosshair(shape) = &sample.preset.layers[0].content else {
            panic!()
        };
        assert_eq!(shape.color, Color::rgba(255, 0, 0, 255));
        p.layers[0].animations.reverse();
        assert_eq!(playback.sample(&p, 0).preset, sample.preset);
        assert!(sample.next_frame_ms.is_none());
    }

    #[test]
    fn legacy_definition_migrates_and_duplicate_triggers_are_rejected() {
        let p = design();
        let mut encoded = serde_json::to_value(&p.layers[0]).unwrap();
        let definitions = encoded
            .as_object_mut()
            .unwrap()
            .remove("animations")
            .unwrap();
        encoded["animation"] = definitions[0].clone();
        let migrated: Layer = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(migrated, p.layers[0]);
        encoded["animation"]["target"]["unknown"] = true.into();
        assert!(serde_json::from_value::<Layer>(encoded).is_err());
        let mut invalid = migrated.clone();
        invalid.animations.push(migrated.animations[0].clone());
        assert!(crate::validation::validate_layer_geometry(&invalid).is_err());
    }

    #[test]
    fn movement_hold_returns_and_reload_rearms_only_after_release() {
        let mut p = design();
        p.layers[0].animations.first_mut().unwrap().trigger = AnimationTrigger::Movement;
        let movement = |down| AnimationEvent::Trigger {
            trigger: AnimationTrigger::Movement,
            down,
        };
        let mut playback = Playback::default();
        playback.select(&p, [false, false, true], 0);
        close(x(&playback, &p, 100), 100.0);
        playback.event(&p, movement(false), 100);
        close(x(&playback, &p, 150), 50.0);
        close(x(&playback, &p, 200), 0.0);
        playback.event(&p, movement(true), 201);
        playback.event(&p, AnimationEvent::Reload, 250);
        playback.select(&p, [false, false, true], 251);
        close(x(&playback, &p, 400), 0.0);
        playback.event(&p, movement(false), 401);
        playback.event(&p, movement(true), 402);
        close(x(&playback, &p, 502), 100.0);
        p.layers[0].animations.first_mut().unwrap().activation = Activation::Press;
        playback.select(&p, [false, false, true], 503);
        close(x(&playback, &p, 550), 0.0);
    }

    #[test]
    fn targets_equal_to_base_do_not_schedule_repeating_frames() {
        let mut p = design();
        let Content::Crosshair(shape) = &p.layers[0].content else {
            unreachable!()
        };
        let color = shape.color;
        p.layers[0].transform.x = 100.0;
        let a = p.layers[0].animations.first_mut().unwrap();
        a.repeat = true;
        a.target.color = Some(color);
        let mut playback = Playback::default();
        playback.event(&p, fire(true), 0);
        assert_eq!(playback.sample(&p, 10).next_frame_ms, None);
        close(x(&playback, &p, 10), 100.0);
        playback.select(&p, [false, true, false], 20);
        assert_eq!(playback.sample(&p, 20).next_frame_ms, None);
    }

    fn design() -> Preset {
        let mut p = Preset::new("Test");
        p.layers[0].animations = vec![Animation {
            duration_ms: 100,
            easing: Easing::Linear,
            target: AnimationTarget {
                x: Some(100.0),
                ..Default::default()
            },
            ..Default::default()
        }];
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
        assert!(
            playback.sample(&p, 50).preset.layers[0]
                .animations
                .is_empty()
        );
    }

    #[test]
    fn instant_hold_honors_delay_release_and_repeated_presses_without_frames() {
        for release in [Release::Return, Release::Reset, Release::Pause] {
            let mut p = design();
            let a = p.layers[0].animations.first_mut().unwrap();
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
        let a = p.layers[0].animations.first_mut().unwrap();
        a.duration_ms = 0;
        a.activation = Activation::Press;
        let mut playback = Playback::default();
        playback.event(&p, fire(true), 0);
        close(x(&playback, &p, 0), 0.0);
        assert_eq!(playback.sample(&p, 0).next_frame_ms, None);
        p.layers[0].animations.first_mut().unwrap().repeat = true;
        playback.reset();
        playback.event(&p, fire(true), 1);
        close(x(&playback, &p, 1), 100.0);
        assert_eq!(playback.sample(&p, 1).next_frame_ms, None);
        playback.event(&p, fire(false), 2);
        playback.event(&p, fire(true), 3);
        close(x(&playback, &p, 3), 0.0);
        p.layers[0].animations.first_mut().unwrap().direction = Direction::BackAndForth;
        playback.event(&p, fire(true), 4);
        close(x(&playback, &p, 4), 0.0);
        assert_eq!(playback.sample(&p, 4).next_frame_ms, None);
    }

    #[test]
    fn a_completed_return_restarts_the_initial_delay() {
        let mut p = design();
        p.layers[0].animations.first_mut().unwrap().delay_ms = 20;
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
        p.layers[0].animations.first_mut().unwrap().release = Release::Pause;
        let mut playback = Playback::default();
        playback.event(&p, fire(true), 0);
        playback.event(&p, fire(false), 40);
        close(x(&playback, &p, 900), 40.0);
        assert_eq!(playback.sample(&p, 900).next_frame_ms, None);
        playback.event(&p, fire(true), 900);
        close(x(&playback, &p, 930), 70.0);
        playback.event(&p, AnimationEvent::Reload, 930);
        close(x(&playback, &p, 950), 0.0);
        playback.select(&p, [false, true, false], 950);
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
        let a = p.layers[0].animations.first_mut().unwrap();
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
        p.layers[0].animations.first_mut().unwrap().activation = Activation::Press;
        let mut playback = Playback::default();
        playback.event(&p, fire(true), 0);
        playback.event(&p, fire(false), 1);
        close(x(&playback, &p, 50), 50.0);
        close(x(&playback, &p, 100), 0.0);
        assert_eq!(playback.sample(&p, 100).next_frame_ms, None);
        p.layers[0].animations.first_mut().unwrap().repeat = true;
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
        p.layers[0].animations.first_mut().unwrap().direction = Direction::BackAndForth;
        let mut playback = Playback::default();
        playback.event(&p, fire(true), 0);
        close(x(&playback, &p, 100), 100.0);
        close(x(&playback, &p, 150), 50.0);
        close(x(&playback, &p, 200), 0.0);
        assert_eq!(playback.sample(&p, 200).next_frame_ms, None);
        p.layers[0].animations.first_mut().unwrap().repeat = true;
        close(x(&playback, &p, 10_150), 50.0);
        assert!(playback.sample(&p, 10_150).next_frame_ms.is_some());
    }

    #[test]
    fn selection_inherits_holds_but_not_presses_and_triggers_are_independent() {
        let mut p = design();
        let mut aim = p.layers[0].duplicate();
        aim.animations.first_mut().unwrap().trigger = AnimationTrigger::Aim;
        p.layers.push(aim);
        let mut playback = Playback::default();
        playback.select(&p, [true, false, false], 0);
        let sample = playback.sample(&p, 50);
        close(sample.preset.layers[0].transform.x, 0.0);
        close(sample.preset.layers[1].transform.x, 50.0);
        p.layers[1].animations.first_mut().unwrap().activation = Activation::Press;
        playback.select(&p, [true, false, false], 100);
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
        p.layers[0].animations.first_mut().unwrap().target = AnimationTarget::default();
        playback.event(&p, fire(true), 0);
        assert_eq!(playback.sample(&p, 20).next_frame_ms, None);
        p = design();
        p.layers[0].animations.first_mut().unwrap().enabled = false;
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

use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use anyhow::{Result, ensure};
use crosshair_core::{AppState, GamepadControl as Control, InputEvent, InputKey, InputSource};
use sdl3::{
    EventPump, GamepadSubsystem,
    event::Event,
    gamepad::{Axis, Button, Gamepad},
    hint::{Hint, set_with_priority},
    joystick::JoystickId,
};

const BUTTONS: &[(Button, Control)] = &[
    (Button::South, Control::South),
    (Button::East, Control::East),
    (Button::West, Control::West),
    (Button::North, Control::North),
    (Button::Back, Control::Back),
    (Button::Guide, Control::Guide),
    (Button::Start, Control::Start),
    (Button::LeftStick, Control::LeftStick),
    (Button::RightStick, Control::RightStick),
    (Button::LeftShoulder, Control::LeftShoulder),
    (Button::RightShoulder, Control::RightShoulder),
    (Button::DPadUp, Control::DPadUp),
    (Button::DPadDown, Control::DPadDown),
    (Button::DPadLeft, Control::DPadLeft),
    (Button::DPadRight, Control::DPadRight),
    (Button::Misc1, Control::Misc1),
    (Button::Misc2, Control::Misc2),
    (Button::Misc3, Control::Misc3),
    (Button::Misc4, Control::Misc4),
    (Button::Misc5, Control::Misc5),
    (Button::Misc6, Control::Misc6),
    (Button::RightPaddle1, Control::RightPaddle1),
    (Button::LeftPaddle1, Control::LeftPaddle1),
    (Button::RightPaddle2, Control::RightPaddle2),
    (Button::LeftPaddle2, Control::LeftPaddle2),
    (Button::Touchpad, Control::Touchpad),
];

const AXES: [Axis; 6] = [
    Axis::LeftX,
    Axis::LeftY,
    Axis::RightX,
    Axis::RightY,
    Axis::TriggerLeft,
    Axis::TriggerRight,
];

pub fn needed(state: &AppState, capture: Option<InputSource>) -> bool {
    capture == Some(InputSource::Controller)
        || state.settings.controller_toggle.is_some()
        || state.active_profile().is_some_and(|profile| {
            !profile.actions.controller.is_empty()
                || profile
                    .bindings
                    .iter()
                    .any(|b| b.enabled && b.controller.is_some())
        })
}

fn axis_control(axis: Axis, value: i16, held: Option<Control>) -> Option<Control> {
    let negative = value < 0;
    let control = match (axis, negative) {
        (Axis::LeftX, true) => Control::LeftStickLeft,
        (Axis::LeftX, false) => Control::LeftStickRight,
        (Axis::LeftY, true) => Control::LeftStickUp,
        (Axis::LeftY, false) => Control::LeftStickDown,
        (Axis::RightX, true) => Control::RightStickLeft,
        (Axis::RightX, false) => Control::RightStickRight,
        (Axis::RightY, true) => Control::RightStickUp,
        (Axis::RightY, false) => Control::RightStickDown,
        (Axis::TriggerLeft | Axis::TriggerRight, true) => return None,
        (Axis::TriggerLeft, false) => Control::LeftTrigger,
        (Axis::TriggerRight, false) => Control::RightTrigger,
    };
    let threshold = if held == Some(control) { 8_192 } else { 16_384 };
    (value.unsigned_abs() >= threshold).then_some(control)
}

pub enum Change {
    Edge(InputEvent),
    AlreadyHeld(InputEvent),
    Removed(u64),
}

fn event(id: JoystickId, control: Control, down: bool) -> InputEvent {
    InputEvent {
        device: id.raw().into(),
        key: InputKey::Gamepad { control },
        down,
    }
}

struct Pad {
    handle: Gamepad,
    axes: [Option<Control>; AXES.len()],
}

impl Pad {
    fn baseline(&mut self, id: JoystickId, emit: &mut impl FnMut(Change)) {
        for &(button, control) in BUTTONS {
            if self.handle.button(button) {
                emit(Change::AlreadyHeld(event(id, control, true)));
            }
        }
        for axis in AXES {
            let held = axis_control(axis, self.handle.axis(axis), None);
            self.axes[axis as usize] = held;
            if let Some(control) = held {
                emit(Change::AlreadyHeld(event(id, control, true)));
            }
        }
    }

    fn axis(&mut self, id: JoystickId, axis: Axis, value: i16, emit: &mut impl FnMut(Change)) {
        let held = &mut self.axes[axis as usize];
        let next = axis_control(axis, value, *held);
        if *held != next {
            if let Some(control) = *held {
                emit(Change::Edge(event(id, control, false)));
            }
            if let Some(control) = next {
                emit(Change::Edge(event(id, control, true)));
            }
            *held = next;
        }
    }
}

pub struct Gamepads {
    devices: BTreeMap<JoystickId, Pad>,
    subsystem: GamepadSubsystem,
    pump: EventPump,
    unopened: BTreeSet<JoystickId>,
    retry_at: Instant,
}

impl Gamepads {
    pub fn new() -> Result<Self> {
        // The engine owns Win32 dispatch and observes shortcuts while a game has focus.
        ensure!(
            set_with_priority("SDL_WINDOWS_ENABLE_MESSAGELOOP", "0", &Hint::Override)
                && set_with_priority("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1", &Hint::Override),
            "Could not configure background controller input"
        );
        let sdl = sdl3::init()?;
        Ok(Self {
            devices: BTreeMap::new(),
            subsystem: sdl.gamepad()?,
            pump: sdl.event_pump()?,
            unopened: BTreeSet::new(),
            retry_at: Instant::now() + Duration::from_secs(5),
        })
    }

    pub fn device_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.devices.keys().map(|id| u64::from(id.raw()))
    }

    pub fn interval(&self) -> Duration {
        Duration::from_millis(if self.devices.is_empty() { 500 } else { 10 })
    }

    pub fn has_open_error(&self) -> bool {
        !self.unopened.is_empty()
    }

    fn open(&mut self, id: JoystickId, emit: &mut impl FnMut(Change)) {
        if let std::collections::btree_map::Entry::Vacant(entry) = self.devices.entry(id) {
            match self.subsystem.open(id) {
                Ok(handle) => {
                    entry
                        .insert(Pad {
                            handle,
                            axes: [None; AXES.len()],
                        })
                        .baseline(id, emit);
                    self.unopened.remove(&id);
                }
                Err(_) => {
                    self.unopened.insert(id);
                }
            }
        }
    }

    pub fn poll(&mut self, mut emit: impl FnMut(Change)) {
        if !self.unopened.is_empty() && Instant::now() >= self.retry_at {
            for id in std::mem::take(&mut self.unopened) {
                self.open(id, &mut emit);
            }
            self.retry_at = Instant::now() + Duration::from_secs(5);
        }
        while let Some(input) = self.pump.poll_event() {
            match input {
                // SDL also sends Added for mapped gamepads present at startup.
                Event::GamepadAdded { which, .. } => self.open(which, &mut emit),
                Event::GamepadRemoved { which, .. } => {
                    self.unopened.remove(&which);
                    if self.devices.remove(&which).is_some() {
                        emit(Change::Removed(which.raw().into()));
                    }
                }
                Event::GamepadRemapped { which, .. } => {
                    if let Some(pad) = self.devices.get_mut(&which) {
                        emit(Change::Removed(which.raw().into()));
                        pad.baseline(which, &mut emit);
                    }
                }
                Event::GamepadButtonDown { which, button, .. }
                | Event::GamepadButtonUp { which, button, .. } => {
                    if self.devices.contains_key(&which)
                        && let Some(&(_, control)) = BUTTONS.iter().find(|(b, _)| *b == button)
                    {
                        emit(Change::Edge(event(
                            which,
                            control,
                            matches!(input, Event::GamepadButtonDown { .. }),
                        )));
                    }
                }
                Event::GamepadAxisMotion {
                    which, axis, value, ..
                } => {
                    if let Some(pad) = self.devices.get_mut(&which) {
                        pad.axis(which, axis, value, &mut emit);
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdl3::joystick::{JoystickType, VirtualJoystickDescription};

    #[test]
    fn axes_press_at_half_travel_and_release_below_one_quarter() {
        let right = Some(Control::LeftStickRight);
        assert_eq!(axis_control(Axis::LeftX, 16_383, None), None);
        assert_eq!(axis_control(Axis::LeftX, 16_384, None), right);
        assert_eq!(axis_control(Axis::LeftX, 8_192, right), right);
        assert_eq!(axis_control(Axis::LeftX, 8_191, right), None);
        assert_eq!(axis_control(Axis::LeftX, -9_000, right), None);
        assert_eq!(
            axis_control(Axis::LeftX, i16::MIN, right),
            Some(Control::LeftStickLeft)
        );
        assert_eq!(
            axis_control(Axis::RightY, i16::MAX, None),
            Some(Control::RightStickDown)
        );
        assert_eq!(axis_control(Axis::TriggerLeft, i16::MIN, None), None);
        assert_eq!(
            axis_control(Axis::TriggerRight, i16::MAX, None),
            Some(Control::RightTrigger)
        );
    }

    #[test]
    fn defaults_leave_gamepads_dormant_until_requested() {
        let mut state = AppState::default();
        assert!(!needed(&state, None));
        assert!(!needed(&state, Some(InputSource::KeyboardMouse)));
        assert!(needed(&state, Some(InputSource::Controller)));
        state.profiles[0].actions.controller.movement[0] = Some(Control::LeftStickUp);
        assert!(needed(&state, None));
        state.profiles[0].actions.controller.movement[0] = None;
        state.settings.controller_toggle = Some(Control::Back);
        assert!(needed(&state, None));
    }

    #[test]
    fn virtual_gamepad_opens_held_tracks_edges_and_disconnects() -> Result<()> {
        // Keep every Windows hardware backend out of this process-local fixture.
        for hint in [
            "SDL_JOYSTICK_HIDAPI",
            "SDL_JOYSTICK_RAWINPUT",
            "SDL_JOYSTICK_DIRECTINPUT",
            "SDL_XINPUT_ENABLED",
            "SDL_JOYSTICK_WGI",
            "SDL_JOYSTICK_GAMEINPUT",
            "SDL_JOYSTICK_GAMEINPUT_RAW",
        ] {
            assert!(set_with_priority(hint, "0", &Hint::Override));
        }
        let mut pads = Gamepads::new()?;
        let joysticks = sdl3::init()?.joystick()?;
        assert!(joysticks.joysticks()?.is_empty());
        let connection = joysticks.attach_virtual_joystick(
            VirtualJoystickDescription::new()
                .joystick_type(JoystickType::Gamepad)
                .with_button(Button::South)
                .with_axis(Axis::LeftX),
        )?;
        let id = connection.id();
        let stick = joysticks.open(id)?;
        stick.set_virtual_button(0, true)?;
        joysticks.update();
        let mut held = Vec::new();
        pads.poll(|change| match change {
            Change::AlreadyHeld(event) => held.push((event.device, event.key)),
            Change::Edge(_) | Change::Removed(_) => panic!("unexpected opening edge"),
        });
        let key = |control| InputKey::Gamepad { control };
        assert_eq!(held, [(id.raw().into(), key(Control::South))]);

        let mut edges = Vec::new();
        let mut collect = |change| {
            if let Change::Edge(event) = change {
                assert_eq!(event.device, u64::from(id.raw()));
                edges.push((event.key, event.down));
            }
        };
        stick.set_virtual_button(0, false)?;
        pads.poll(&mut collect);
        stick.set_virtual_button(0, true)?;
        pads.poll(&mut collect);
        for x in [17_000, 15_000, i16::MIN, 0] {
            stick.set_virtual_axis(0, x)?;
            pads.poll(&mut collect);
        }
        assert_eq!(
            edges,
            [
                (key(Control::South), false),
                (key(Control::South), true),
                (key(Control::LeftStickRight), true),
                (key(Control::LeftStickRight), false),
                (key(Control::LeftStickLeft), true),
                (key(Control::LeftStickLeft), false),
            ]
        );
        // A held button changes position in the new mapping without a fresh press.
        pads.subsystem
            .set_mapping(id, "*,Virtual pad,b:b0,leftx:a0,")?;
        let mut remapped = false;
        let mut baseline = Vec::new();
        pads.poll(|change| match change {
            Change::Removed(device) => remapped = device == u64::from(id.raw()),
            Change::AlreadyHeld(event) => baseline.push(event.key),
            Change::Edge(event) => assert!(!event.down),
        });
        assert!(remapped);
        assert_eq!(baseline, [key(Control::East)]);
        drop(connection);
        let mut removed = Vec::new();
        pads.poll(|change| {
            if let Change::Removed(device) = change {
                removed.push(device);
            }
        });
        assert_eq!(removed, [u64::from(id.raw())]);
        assert_eq!(pads.device_ids().count(), 0);
        Ok(())
    }
}

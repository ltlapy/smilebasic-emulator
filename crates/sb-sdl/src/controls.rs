//! What programs read from controllers, the mouse and the keyboard: SDL's
//! gamepads and keys turned into the runtime's input state each frame.

use sb_runtime::input::{Mouse, PADS, State};
use sdl3::event::Event;
use sdl3::gamepad::{Axis, Button, Gamepad};
use sdl3::keyboard::{KeyboardState, Mod, Scancode};
use sdl3::mouse::MouseState;
use sdl3::{GamepadSubsystem, Sdl};

/// SDL's buttons for BUTTON's ids, by position: the top face button is
/// #B_RUP whatever its label says.
const PAD_BUTTONS: [(Button, u32); 12] = [
    (Button::North, 0),
    (Button::South, 1),
    (Button::West, 2),
    (Button::East, 3),
    (Button::DPadUp, 4),
    (Button::DPadDown, 5),
    (Button::DPadLeft, 6),
    (Button::DPadRight, 7),
    (Button::LeftShoulder, 8),
    (Button::RightShoulder, 9),
    (Button::LeftStick, 12),
    (Button::RightStick, 13),
];

/// Keys for the first controller's buttons when they are allowed to press
/// them. The face buttons follow the Switch's layout: A on the right.
// TODO: editable with the key layout settings once they exist.
const KEY_BUTTONS: [(Scancode, u32); 12] = [
    (Scancode::S, 0),
    (Scancode::X, 1),
    (Scancode::A, 2),
    (Scancode::Z, 3),
    (Scancode::Up, 4),
    (Scancode::Down, 5),
    (Scancode::Left, 6),
    (Scancode::Right, 7),
    (Scancode::Q, 8),
    (Scancode::W, 9),
    (Scancode::_1, 10),
    (Scancode::_2, 11),
];

/// How far a trigger goes before it counts as ZL or ZR pressed.
const TRIGGER: i16 = i16::MAX / 2;
/// Sticks read as centered until tilted this far.
const DEAD_ZONE: f64 = 0.15;

pub struct Controls {
    subsystem: GamepadSubsystem,
    /// Controllers 1 to 4, in the order they were connected.
    pads: Vec<Gamepad>,
    pub mouse: Mouse,
}

fn stick(pad: &Gamepad, x: Axis, y: Axis) -> (f64, f64) {
    let read = |a| (f64::from(pad.axis(a)) / f64::from(i16::MAX)).clamp(-1.0, 1.0);
    let (x, y) = (read(x), read(y));
    if x.hypot(y) < DEAD_ZONE {
        (0.0, 0.0)
    } else {
        (x, y)
    }
}

impl Controls {
    pub fn new(sdl: &Sdl) -> Result<Controls, sdl3::Error> {
        Ok(Controls {
            subsystem: sdl.gamepad()?,
            pads: Vec::new(),
            mouse: Mouse::default(),
        })
    }

    /// Opens controllers as they are connected, and lets go of them.
    pub fn event(&mut self, event: &Event) {
        match *event {
            Event::GamepadAdded { which, .. } if self.pads.len() < PADS => {
                match self.subsystem.open(which) {
                    Ok(pad) => self.pads.push(pad),
                    Err(e) => eprintln!("could not open a controller: {e}"),
                }
            }
            Event::GamepadRemoved { which, .. } => {
                self.pads.retain(|p| p.id().is_ok_and(|id| id != which));
            }
            _ => {}
        }
    }

    /// What the program sees now. `keys_press_buttons` lets the keyboard
    /// press the first controller's buttons too.
    pub fn state(
        &self,
        keyboard: &KeyboardState,
        keymod: Mod,
        mouse: MouseState,
        keys_press_buttons: bool,
    ) -> State {
        let mut state = State::default();
        for (i, pad) in self.pads.iter().enumerate() {
            let p = &mut state.pads[i];
            p.connected = true;
            for (button, id) in PAD_BUTTONS {
                if pad.button(button) {
                    p.buttons |= 1 << id;
                }
            }
            for (axis, id) in [(Axis::TriggerLeft, 10), (Axis::TriggerRight, 11)] {
                if pad.axis(axis) > TRIGGER {
                    p.buttons |= 1 << id;
                }
            }
            p.sticks = [
                stick(pad, Axis::LeftX, Axis::LeftY),
                stick(pad, Axis::RightX, Axis::RightY),
            ];
        }
        for (scancode, down) in keyboard.scancodes() {
            if let Ok(usage) = u8::try_from(scancode as i32) {
                state.set_key(usage, down);
            }
        }
        if keys_press_buttons {
            for (scancode, id) in KEY_BUTTONS {
                if keyboard.is_scancode_pressed(scancode) {
                    state.pads[0].buttons |= 1 << id;
                }
            }
        }
        let held = |m: Mod| u32::from(keymod.intersects(m));
        state.modifiers = held(Mod::LGUIMOD | Mod::RGUIMOD)
            | held(Mod::LSHIFTMOD | Mod::RSHIFTMOD) << 1
            | held(Mod::LCTRLMOD | Mod::RCTRLMOD) << 2
            | held(Mod::LALTMOD | Mod::RALTMOD) << 3;
        state.mouse = Mouse {
            buttons: [
                mouse.left(),
                mouse.right(),
                mouse.middle(),
                mouse.x1(),
                mouse.x2(),
            ]
            .iter()
            .enumerate()
            .fold(0, |bits, (i, &down)| bits | u32::from(down) << i),
            ..self.mouse
        };
        state
    }
}

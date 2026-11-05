//! What programs read from controllers, the mouse and the keyboard: SDL's
//! gamepads and keys turned into the runtime's input state each frame.

use sb_runtime::input::{KeysRead, Mouse, PADS, State};
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
pub const KEY_BUTTONS: [(Scancode, u32); 12] = [
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

/// How long a key stays out of [`KEY_BUTTONS`] after the program last read
/// it, in frames. Long enough for programs that read the keyboard only now
/// and then, short enough that a game's name entry doesn't cost the rest of
/// the game its keys.
const KEY_RESERVED_FRAMES: u16 = 60;

/// Which of [`KEY_BUTTONS`] press their buttons now: not a key the program
/// has lately read for itself, or the same press would act twice. A typing
/// program that also takes the controller would have A delete the letter it
/// types.
#[derive(Debug, Default)]
pub struct KeyButtons {
    reserved: [u16; KEY_BUTTONS.len()],
}

impl KeyButtons {
    /// Goes once a frame, with what the program read in that frame.
    pub fn update(&mut self, read: KeysRead) {
        for (left, (key, _)) in self.reserved.iter_mut().zip(KEY_BUTTONS) {
            if read.covers(key as i32 as u8) {
                *left = KEY_RESERVED_FRAMES;
            } else {
                *left = left.saturating_sub(1);
            }
        }
    }

    /// Whether no key would press a button now.
    pub fn all_reserved(&self) -> bool {
        self.reserved.iter().all(|&left| left > 0)
    }

    /// The buttons pressed by the keys for which `down` says so.
    pub fn buttons(&self, down: impl Fn(Scancode) -> bool) -> u32 {
        (self.reserved.iter().zip(KEY_BUTTONS))
            .filter(|&(&left, (key, _))| left == 0 && down(key))
            .fold(0, |bits, (_, (_, id))| bits | 1 << id)
    }
}

/// How far a trigger goes before it counts as ZL or ZR pressed.
const TRIGGER: i16 = i16::MAX / 2;
/// Sticks read as centered until tilted this far.
const DEAD_ZONE: f64 = 0.15;

pub struct Controls {
    subsystem: GamepadSubsystem,
    /// Controllers 1 to 4, in the order they were connected.
    pads: Vec<Gamepad>,
    pub mouse: Mouse,
    pub key_buttons: KeyButtons,
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
            key_buttons: KeyButtons::default(),
        })
    }

    /// How many gamepads are connected.
    pub fn gamepads(&self) -> usize {
        self.pads.len()
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
            let pads = state.pads.iter().fold(0, |bits, p| bits | p.buttons);
            let keys = self
                .key_buttons
                .buttons(|key| keyboard.is_scancode_pressed(key));
            state.pads[0].buttons |= keys;
            state.key_pressed_buttons = keys & !pads;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The button the A key presses: #B_RLEFT, which reads as Y.
    const A_KEY_BUTTON: u32 = 1 << 2;
    const S_KEY_BUTTON: u32 = 1 << 0;

    fn read(source: &str) -> KeysRead {
        use sb_core::vm::{Status, Vm};
        let mut rt = sb_runtime::Runtime::new(400, 240, sb_runtime::font::Font::blank());
        let mut vm = Vm::new(sb_core::compile(source, &rt).unwrap());
        while !matches!(vm.run(&mut rt, 100_000).unwrap(), Status::Ended) {
            rt.begin_frame();
        }
        rt.input.take_keys_read()
    }

    fn held(key: Scancode) -> impl Fn(Scancode) -> bool {
        move |k| k == key
    }

    #[test]
    fn a_key_presses_its_button_for_a_program_that_only_reads_buttons() {
        let mut keys = KeyButtons::default();
        keys.update(read("B=BUTTON(0)"));
        assert_eq!(keys.buttons(held(Scancode::A)), A_KEY_BUTTON);
    }

    /// A romaji input program reads INKEY$ for the letter and #B_RLEFT for
    /// delete in the same frame, so that a press of A typed nothing.
    #[test]
    fn typing_keeps_the_a_key_from_its_button() {
        let mut keys = KeyButtons::default();
        keys.update(read("B=BUTTON(0):K$=INKEY$()"));
        assert_eq!(keys.buttons(held(Scancode::A)), 0);
        assert!(keys.all_reserved());
    }

    #[test]
    fn keyboard_reserves_only_the_key_it_asks_about() {
        let mut keys = KeyButtons::default();
        keys.update(read("B=BUTTON(0):K=KEYBOARD(4,2)"));
        assert_eq!(keys.buttons(held(Scancode::A)), 0);
        assert_eq!(keys.buttons(held(Scancode::S)), S_KEY_BUTTON);
        assert!(!keys.all_reserved());
    }

    /// A game with a name entry gets its keys back once that is over.
    #[test]
    fn keys_come_back_once_the_program_stops_reading_them() {
        let mut keys = KeyButtons::default();
        keys.update(read("K$=INKEY$()"));
        let nothing = KeysRead::default();
        for _ in 1..KEY_RESERVED_FRAMES {
            keys.update(nothing);
            assert_eq!(keys.buttons(held(Scancode::A)), 0);
        }
        keys.update(nothing);
        assert_eq!(keys.buttons(held(Scancode::A)), A_KEY_BUTTON);
    }
}

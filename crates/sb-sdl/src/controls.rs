//! What programs read from controllers, the mouse and the keyboard: SDL's
//! gamepads and keys turned into the runtime's input state each frame.

use sb_runtime::input::{KeysRead, Mouse, PADS, State};
use sdl3::event::Event;
use sdl3::gamepad::{Axis, Button, Gamepad};
use sdl3::keyboard::{KeyboardState, Scancode};
use sdl3::mouse::MouseState;
use sdl3::{GamepadSubsystem, Sdl};

/// The bit in BUTTON's value of one of SDL's buttons, for the bench.
pub fn button_bit(button: Button) -> u32 {
    (PAD_BUTTONS.iter())
        .find(|(b, _)| *b == button)
        .map_or(0, |(_, id)| 1 << id)
}

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

/// KEYBOARD's modifier bits, Windows key, Shift, Ctrl and Alt, from the keys
/// held rather than from SDL's modifier state. SDL takes that from what the
/// layout makes of a key, so a Korean layout that turns Right Alt into
/// Han/Eng would leave Alt out, while KEYBOARD sees the key itself.
fn modifiers(down: impl Fn(Scancode) -> bool) -> u32 {
    use Scancode::{LAlt, LCtrl, LGui, LShift, RAlt, RCtrl, RGui, RShift};
    [[LGui, RGui], [LShift, RShift], [LCtrl, RCtrl], [LAlt, RAlt]]
        .iter()
        .enumerate()
        .fold(0, |bits, (i, keys)| {
            bits | u32::from(keys.iter().any(|&k| down(k))) << i
        })
}

/// How far a stick has to be tilted to count as held through a screen: where
/// it starts to type keys for a program, past any drift.
const CARRIED_STICK: f64 = 0.3;

/// Keeps from the program what was pressed for one of the emulator's own
/// screens, such as the A that chose Run in Projects: a button or stick held
/// when the screen goes reads as let go until it really is, so that it
/// neither types Enter nor reads as a press beneath or after the screen.
#[derive(Debug, Default)]
pub struct Carried {
    buttons: [u32; PADS],
    sticks: [[bool; 2]; PADS],
    /// A screen had the controls at the end of the last frame.
    screen: bool,
}

impl Carried {
    /// What the program sees of `state`, once a frame. `screen`: a screen
    /// has the controls now, so that the program sees nothing at all. The
    /// frame it goes in has no buttons or sticks yet, as the press that closed
    /// it came in that frame, whichever order the events came in.
    pub fn program_sees(&mut self, mut state: State, screen: bool) -> State {
        let was_screen = std::mem::replace(&mut self.screen, screen);
        for (i, pad) in state.pads.iter_mut().enumerate() {
            let tilted = pad
                .sticks
                .map(|(x, y)| x.abs() > CARRIED_STICK || y.abs() > CARRIED_STICK);
            if screen || was_screen {
                self.buttons[i] = pad.buttons;
                self.sticks[i] = tilted;
                pad.buttons = 0;
                pad.sticks = Default::default();
                continue;
            }
            self.buttons[i] &= pad.buttons;
            pad.buttons &= !self.buttons[i];
            for (id, tilted) in tilted.into_iter().enumerate() {
                self.sticks[i][id] &= tilted;
                if self.sticks[i][id] {
                    pad.sticks[id] = (0.0, 0.0);
                }
            }
        }
        if screen { State::default() } else { state }
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
    pub carried: Carried,
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
            carried: Carried::default(),
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
        mouse: MouseState,
        keys_press_buttons: bool,
    ) -> State {
        let mut state = State::default();
        for (i, pad) in self.pads.iter().enumerate() {
            let p = &mut state.pads[i];
            p.connected = true;
            p.gamepad = true;
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
        state.modifiers = modifiers(|key| keyboard.is_scancode_pressed(key));
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
    /// #B_RRIGHT, which is A.
    const A_BUTTON: u32 = 1 << 3;

    fn holding(buttons: u32) -> State {
        let mut state = State::default();
        state.pads[0].buttons = buttons;
        state
    }

    /// Goes through frames as the frontend does, each with the buttons held
    /// and whether a screen has the controls, and gives what INKEY$ has then.
    fn typed_after(frames: &[(u32, bool)]) -> usize {
        use sb_core::vm::{Status, Vm};
        let mut rt = sb_runtime::Runtime::new(400, 240, sb_runtime::font::Font::blank());
        let mut carried = Carried::default();
        for &(buttons, screen) in frames {
            rt.input.live = carried.program_sees(holding(buttons), screen);
            rt.begin_frame();
        }
        let source = "N=0:WHILE INKEY$()!=\"\":INC N:WEND:?N";
        let mut vm = Vm::new(sb_core::compile(source, &rt).unwrap());
        while vm.run(&mut rt, 100_000).unwrap() != Status::Ended {}
        let cell = rt.text[4].cell(0, 0).code;
        usize::from(cell - u16::from(b'0'))
    }

    /// The press that runs a project comes while Projects is open, and is
    /// still down in the frame it closes and after it.
    #[test]
    fn the_a_that_runs_a_project_types_nothing() {
        // Down in the frame Projects closes in.
        let run = [(0, true), (A_BUTTON, false), (A_BUTTON, false)];
        assert_eq!(typed_after(&run), 0);
        // Held long past when a held key would repeat.
        let mut held = vec![(A_BUTTON, true)];
        held.resize(60, (A_BUTTON, false));
        assert_eq!(typed_after(&held), 0);
        // While a screen is open nothing goes into the queue beneath it.
        assert_eq!(typed_after(&[(A_BUTTON, true), (0, true)]), 0);
    }

    #[test]
    fn a_new_press_after_a_screen_types() {
        let again = [
            (A_BUTTON, true),
            (A_BUTTON, false),
            (0, false),
            (A_BUTTON, false),
        ];
        assert_eq!(typed_after(&again), 1);
        // Pressed with the screen long gone.
        assert_eq!(typed_after(&[(0, true), (0, false), (A_BUTTON, false)]), 1);
    }

    /// With no screen in sight, a program sees what is held as ever.
    #[test]
    fn carried_leaves_other_buttons_alone() {
        let mut carried = Carried::default();
        carried.program_sees(holding(A_BUTTON), true);
        let closed = carried.program_sees(holding(A_BUTTON | S_KEY_BUTTON), false);
        assert_eq!(closed.pads[0].buttons, 0, "the frame it closes in");
        let after = carried.program_sees(holding(A_BUTTON | 1 << 1), false);
        assert_eq!(after.pads[0].buttons, 1 << 1, "pressed after");
    }

    #[test]
    fn a_stick_pushed_for_a_screen_waits_until_it_is_let_go() {
        let tilted = |y| {
            let mut state = State::default();
            state.pads[0].sticks[0] = (0.0, y);
            state
        };
        let mut carried = Carried::default();
        carried.program_sees(tilted(-1.0), true);
        carried.program_sees(tilted(-1.0), false);
        let held = carried.program_sees(tilted(-1.0), false);
        assert_eq!(held.pads[0].sticks[0], (0.0, 0.0));
        carried.program_sees(tilted(0.0), false);
        let again = carried.program_sees(tilted(-1.0), false);
        assert_eq!(again.pads[0].sticks[0], (0.0, -1.0));
    }

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

    /// TEST68: Han/Eng reads as usage 144 and Hanja as 145, which SDL's
    /// scancodes are. Keyboards that send Right Alt and Right Ctrl for them
    /// read as those, with their modifier bits.
    #[test]
    fn korean_keys_read_as_smilebasic_4_reads_them() {
        assert_eq!(Scancode::Lang1 as i32, 144);
        assert_eq!(Scancode::Lang2 as i32, 145);
        assert_eq!(Scancode::RAlt as i32, 230);
        assert_eq!(Scancode::RCtrl as i32, 228);
        assert_eq!(modifiers(|k| k == Scancode::RAlt), 8);
        assert_eq!(modifiers(|k| k == Scancode::RCtrl), 4);
        assert_eq!(modifiers(|k| k == Scancode::LShift), 2);
        assert_eq!(modifiers(|k| k == Scancode::RGui), 1);
        assert_eq!(modifiers(|_| false), 0);
    }
}

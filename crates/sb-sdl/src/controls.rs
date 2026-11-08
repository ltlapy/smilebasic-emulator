//! What programs read from controllers, the mouse and the keyboard: SDL's
//! gamepads and keys turned into the runtime's input state each frame.

use sb_runtime::input::{KeysRead, Mouse, PADS, State};
use sdl3::event::Event;
use sdl3::gamepad::{Axis, Button, Gamepad};
use sdl3::keyboard::{KeyboardState, Mod, Scancode};
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

/// What last went down, for the text that follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Last {
    /// Nothing, or a key that typed its code itself: text after it is an
    /// input method's, or comes with no key.
    None,
    /// A key whose text comes next, and whether Shift was held.
    Typing { shift: bool },
    /// The computer's key repeat, or a key that types no text.
    Repeat,
}

/// How the keyboard types for programs, the prompt and the editor, as on
/// SmileBASIC 4: with its own Caps Lock, and with repeats counted in the
/// machine's frames rather than by the computer's key repeat.
#[derive(Debug)]
pub struct Typing {
    /// SmileBASIC 4's Caps Lock, which is on at start. The Caps Lock key
    /// toggles it wherever it is pressed, so it keeps step with the
    /// computer's own unless that was on when the emulator started or was
    /// toggled in another program's window.
    // UNVERIFIED: that the prompt and the editor share it with INKEY$.
    caps_lock: bool,
    last: Last,
    /// Keys type no text, as [`State::keys_type_nothing`] says of the
    /// last frame's controls.
    // UNVERIFIED: that the prompt and the editor go by it as programs do.
    blocked: bool,
}

impl Default for Typing {
    fn default() -> Self {
        Typing {
            caps_lock: true,
            last: Last::None,
            blocked: false,
        }
    }
}

impl Typing {
    /// Takes the controls held this frame, for the keys pressed next.
    pub fn hold(&mut self, held: &State) {
        self.blocked = held.keys_type_nothing();
    }

    /// Goes for every key that goes down on the emulator's windows, before
    /// anything else takes it. A key that does not type, such as a
    /// shortcut, stops the key held before from repeating.
    pub fn any_key_down(
        &mut self,
        rt: &mut sb_runtime::Runtime,
        scancode: Option<Scancode>,
        repeat: bool,
    ) {
        self.last = Last::None;
        if repeat {
            return;
        }
        if scancode == Some(Scancode::CapsLock) {
            self.caps_lock ^= true;
        }
        let usage = scancode.and_then(|s| u8::try_from(s as i32).ok());
        rt.input.key_down(usage.unwrap_or(0));
    }

    /// A key going down that types, after [`Typing::any_key_down`]; `code`
    /// is what it types itself, such as Enter or an arrow, if it types no
    /// text. The computer's repeats type nothing: the runtime repeats the
    /// press.
    pub fn key_down(
        &mut self,
        rt: &mut sb_runtime::Runtime,
        keymod: Mod,
        repeat: bool,
        code: Option<u16>,
    ) {
        // Ctrl as SDL has it too, for a key that goes down in the frame
        // Ctrl does.
        let blocked = self.blocked || keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD);
        if repeat || blocked && code.is_none() {
            self.last = Last::Repeat;
            return;
        }
        match code {
            Some(code) => rt.type_held(code),
            None => {
                self.last = Last::Typing {
                    shift: keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD),
                }
            }
        }
    }

    /// Text the computer typed. `upper_case`: letters type as SmileBASIC
    /// 4's do (the setting for capitals without Shift); otherwise they keep
    /// the case the computer gave them.
    ///
    /// Text outside ASCII, from an input method or a layout of another
    /// script, types once as the computer gives it, its repeats included,
    /// as which key it came from is not known.
    pub fn text(&mut self, rt: &mut sb_runtime::Runtime, text: &str, upper_case: bool) {
        for c in text.chars() {
            if !c.is_ascii() {
                // SmileBASIC strings hold only the Basic Multilingual Plane.
                if let Ok(code) = u16::try_from(u32::from(c)) {
                    rt.key_press(code);
                }
                continue;
            }
            match self.last {
                Last::Repeat => {}
                Last::Typing { shift } if upper_case => {
                    rt.type_held(self.letter_case(c, shift) as u16)
                }
                Last::Typing { .. } => rt.type_held(c as u16),
                Last::None if upper_case => rt.key_press(swap_case(c) as u16),
                Last::None => rt.key_press(c as u16),
            }
        }
    }

    /// A letter as SmileBASIC 4 types it: upper case with Caps Lock on,
    /// Shift turning it round, whatever case the computer typed it in.
    fn letter_case(&self, c: char, shift: bool) -> char {
        if self.caps_lock != shift {
            c.to_ascii_uppercase()
        } else {
            c.to_ascii_lowercase()
        }
    }
}

/// Swaps the case of ASCII letters, for text that comes with no key.
fn swap_case(c: char) -> char {
    if c.is_ascii_lowercase() {
        c.to_ascii_uppercase()
    } else {
        c.to_ascii_lowercase()
    }
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

    /// Everything INKEY$ has in the next frame, as hex codes.
    fn inkey(rt: &mut sb_runtime::Runtime) -> String {
        use sb_core::vm::{Status, Vm};
        rt.begin_frame();
        let source = "REPEAT:K$=INKEY$():IF K$!=\"\" THEN ?HEX$(ASC(K$));\" \";\nUNTIL K$==\"\"";
        let mut vm = Vm::new(sb_core::compile(source, rt).unwrap());
        while vm.run(rt, 100_000).unwrap() != Status::Ended {}
        let console = &rt.text[4];
        let codes: Vec<u16> = (0..console.size().0)
            .map(|x| console.cell(x, 0).code)
            .take_while(|&c| c != 0)
            .collect();
        String::from_utf16(&codes).unwrap().trim_end().to_string()
    }

    fn runtime() -> sb_runtime::Runtime {
        sb_runtime::Runtime::new(400, 240, sb_runtime::font::Font::blank())
    }

    /// Presses `key` with `keymod` held and types `text` for it, as the
    /// frontend does.
    fn type_key(
        keys: &mut Typing,
        rt: &mut sb_runtime::Runtime,
        key: Scancode,
        keymod: Mod,
        text: &str,
        upper: bool,
    ) {
        keys.any_key_down(rt, Some(key), false);
        keys.key_down(rt, keymod, false, None);
        keys.text(rt, text, upper);
    }

    /// What INKEY$ gets from [`type_key`].
    fn press(keys: &mut Typing, key: Scancode, keymod: Mod, text: &str, upper: bool) -> String {
        let mut rt = runtime();
        type_key(keys, &mut rt, key, keymod, text, upper);
        inkey(&mut rt)
    }

    fn caps_lock(keys: &mut Typing, repeat: bool) {
        keys.any_key_down(&mut runtime(), Some(Scancode::CapsLock), repeat);
    }

    /// TEST68: A types "A" and Shift+A "a", as SmileBASIC 4's Caps Lock
    /// starts on. Caps Lock turns both round, whatever case the computer's
    /// own Caps Lock typed them in.
    #[test]
    fn letters_follow_smilebasic_4s_own_caps_lock() {
        let mut keys = Typing::default();
        let shift = Mod::LSHIFTMOD;
        assert_eq!(press(&mut keys, Scancode::A, Mod::NOMOD, "a", true), "41");
        assert_eq!(press(&mut keys, Scancode::A, shift, "A", true), "61");
        // G, K and S, as TEST68 typed them.
        let gks: Vec<String> = [(Scancode::G, "g"), (Scancode::K, "k"), (Scancode::S, "s")]
            .map(|(key, text)| press(&mut keys, key, Mod::NOMOD, text, true))
            .into();
        assert_eq!(gks, ["47", "4B", "53"]);
        caps_lock(&mut keys, false);
        // The computer's Caps Lock went on with it, and typed "A".
        assert_eq!(press(&mut keys, Scancode::A, Mod::CAPSMOD, "A", true), "61");
        assert_eq!(press(&mut keys, Scancode::A, shift, "a", true), "41");
        // A repeat of Caps Lock toggles nothing.
        caps_lock(&mut keys, true);
        assert_eq!(press(&mut keys, Scancode::A, Mod::NOMOD, "a", true), "61");
    }

    /// TEST69K: 1, Shift+1 and / type the same with Caps Lock on or off.
    #[test]
    fn caps_lock_leaves_digits_and_symbols_alone() {
        let mut keys = Typing::default();
        for _ in 0..2 {
            assert_eq!(press(&mut keys, Scancode::_1, Mod::NOMOD, "1", true), "31");
            assert_eq!(
                press(&mut keys, Scancode::_1, Mod::LSHIFTMOD, "!", true),
                "21"
            );
            assert_eq!(
                press(&mut keys, Scancode::Slash, Mod::NOMOD, "/", true),
                "2F"
            );
            caps_lock(&mut keys, false);
        }
    }

    /// With "Type capitals without Shift" off, letters keep the computer's
    /// case.
    #[test]
    fn the_setting_off_types_as_the_computer_does() {
        let mut keys = Typing::default();
        assert_eq!(press(&mut keys, Scancode::A, Mod::NOMOD, "a", false), "61");
        assert_eq!(
            press(&mut keys, Scancode::A, Mod::LSHIFTMOD, "A", false),
            "41"
        );
    }

    /// The runtime repeats a held key in frames, so the computer's repeats
    /// type nothing, but text outside ASCII, which an input method may
    /// send, still goes.
    #[test]
    fn the_computers_repeats_type_nothing() {
        let mut keys = Typing::default();
        let mut rt = runtime();
        keys.any_key_down(&mut rt, Some(Scancode::A), true);
        keys.key_down(&mut rt, Mod::NOMOD, true, None);
        keys.text(&mut rt, "a", true);
        keys.text(&mut rt, "\u{AC00}", true);
        keys.any_key_down(&mut rt, Some(Scancode::Return), true);
        keys.key_down(&mut rt, Mod::NOMOD, true, Some(13));
        assert_eq!(inkey(&mut rt), "AC00");
    }

    #[test]
    fn keys_that_type_no_text_type_their_code() {
        let mut keys = Typing::default();
        let mut rt = runtime();
        keys.any_key_down(&mut rt, Some(Scancode::Return), false);
        let code = sb_runtime::runtime::keys::ENTER;
        keys.key_down(&mut rt, Mod::NOMOD, false, Some(code));
        assert_eq!(inkey(&mut rt), "D");
    }

    /// TEST69K: letters typed while the controller's L and R are held type
    /// nothing, nor do they with L or R alone or with Right Ctrl, but keys
    /// that type a code of their own still do.
    #[test]
    fn keys_type_nothing_with_l_or_r_or_ctrl_held() {
        let mut keys = Typing::default();
        keys.hold(&holding(0b11 << 8));
        assert_eq!(press(&mut keys, Scancode::A, Mod::NOMOD, "a", true), "");
        let mut rt = runtime();
        keys.any_key_down(&mut rt, Some(Scancode::Return), false);
        let code = sb_runtime::runtime::keys::ENTER;
        keys.key_down(&mut rt, Mod::NOMOD, false, Some(code));
        assert_eq!(inkey(&mut rt), "D");
        // L alone or R alone, which count as Ctrl.
        keys.hold(&holding(1 << 8));
        assert_eq!(press(&mut keys, Scancode::A, Mod::NOMOD, "a", true), "");
        keys.hold(&holding(1 << 9));
        assert_eq!(press(&mut keys, Scancode::A, Mod::NOMOD, "a", true), "");
        // ZL and ZR do not.
        keys.hold(&holding(0b11 << 10));
        assert_eq!(press(&mut keys, Scancode::A, Mod::NOMOD, "a", true), "41");
        // Right Ctrl, whether SDL has it as Ctrl or, on a Korean layout,
        // as Hanja.
        let ctrl = State {
            modifiers: 1 << 2,
            ..State::default()
        };
        keys.hold(&ctrl);
        assert_eq!(press(&mut keys, Scancode::A, Mod::NOMOD, "a", true), "");
        keys.hold(&State::default());
        assert_eq!(press(&mut keys, Scancode::A, Mod::RCTRLMOD, "a", true), "");
        assert_eq!(press(&mut keys, Scancode::A, Mod::NOMOD, "a", true), "41");
    }

    /// A machine at the prompt, or in the editor with `editing`.
    fn machine(editing: bool) -> sb_runtime::machine::Machine {
        let mut m = sb_runtime::machine::Machine::new(runtime());
        m.boot();
        if editing {
            m.toggle_editor();
        }
        m.frame();
        m
    }

    /// The console's last row with text.
    fn last_row(m: &sb_runtime::machine::Machine) -> Option<String> {
        let console = &m.rt.text[4];
        let (w, h) = console.size();
        (0..h).rev().find_map(|y| {
            let codes: Vec<u16> = (0..w)
                .map(|x| console.cell(x, y).code)
                .take_while(|&c| c != 0)
                .collect();
            (!codes.is_empty()).then(|| String::from_utf16(&codes).unwrap())
        })
    }

    /// The editor and the prompt type letters by the same Caps Lock as
    /// programs, which the Caps Lock key toggles wherever it is pressed.
    #[test]
    fn the_editor_and_the_prompt_type_by_smilebasic_4s_caps_lock() {
        let mut keys = Typing::default();
        let shift = Mod::LSHIFTMOD;
        let mut m = machine(true);
        type_key(&mut keys, &mut m.rt, Scancode::A, Mod::NOMOD, "a", true);
        type_key(&mut keys, &mut m.rt, Scancode::B, shift, "B", true);
        caps_lock(&mut keys, false);
        // The computer's Caps Lock went on with it.
        type_key(&mut keys, &mut m.rt, Scancode::C, Mod::CAPSMOD, "C", true);
        type_key(&mut keys, &mut m.rt, Scancode::D, shift, "d", true);
        m.frame();
        assert_eq!(m.editor().text(), "AbcD");

        // Caps Lock is off now at the prompt too.
        let mut m = machine(false);
        type_key(&mut keys, &mut m.rt, Scancode::A, Mod::CAPSMOD, "A", true);
        type_key(&mut keys, &mut m.rt, Scancode::B, shift, "b", true);
        m.frame();
        assert_eq!(last_row(&m).as_deref(), Some("aB"));
    }

    /// With "Type capitals without Shift" off, the editor and the prompt
    /// keep the computer's case, as programs do.
    #[test]
    fn the_editor_with_the_setting_off_types_as_the_computer_does() {
        let mut keys = Typing::default();
        for editing in [true, false] {
            let mut m = machine(editing);
            type_key(&mut keys, &mut m.rt, Scancode::A, Mod::NOMOD, "a", false);
            type_key(
                &mut keys,
                &mut m.rt,
                Scancode::B,
                Mod::LSHIFTMOD,
                "B",
                false,
            );
            m.frame();
            let typed = match editing {
                true => m.editor().text(),
                false => last_row(&m).unwrap_or_default(),
            };
            assert_eq!(typed, "aB");
        }
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

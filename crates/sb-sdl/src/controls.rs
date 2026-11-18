//! What programs read from controllers, the mouse and the keyboard: SDL's
//! gamepads and keys turned into the runtime's input state each frame.

use sb_runtime::input::{
    KeyText, KeysRead, Mouse, PADS, State, Tap, alt_key_code, types_when_blocked,
};
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

/// The button a press of SDL's `button` stands for: itself, or with
/// `swap` (the setting Swap A/B and X/Y) the face button across from it,
/// A for B and X for Y. Everything that reads a controller goes through
/// this or [`pad_buttons`], so that programs and the emulator's own
/// screens agree on which button is A. It is its own inverse.
pub fn face(button: Button, swap: bool) -> Button {
    match (swap, button) {
        (true, Button::East) => Button::South,
        (true, Button::South) => Button::East,
        (true, Button::North) => Button::West,
        (true, Button::West) => Button::North,
        (_, button) => button,
    }
}

/// A button event with its button as [`face`] has it. The first thing
/// done with every event the system gives.
pub fn swap_event(event: &mut Event, swap: bool) {
    if let Event::GamepadButtonDown { button, .. } | Event::GamepadButtonUp { button, .. } = event {
        *button = face(*button, swap);
    }
}

/// SDL's buttons to read for BUTTON's ids: [`PAD_BUTTONS`], or with
/// `swap` the same with A and B, and X and Y, traded as [`face`] trades
/// them.
fn pad_buttons(swap: bool) -> &'static [(Button, u32); 12] {
    if swap { &SWAPPED_BUTTONS } else { &PAD_BUTTONS }
}

/// BUTTON's bits for the buttons `down` says are held, as [`face`] reads
/// them.
fn held_buttons(down: impl Fn(Button) -> bool, swap: bool) -> u32 {
    (pad_buttons(swap).iter())
        .filter(|&&(button, _)| down(button))
        .fold(0, |bits, (_, id)| bits | 1 << id)
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

/// [`PAD_BUTTONS`] with the face buttons traded: #B_RUP is the left one,
/// #B_RRIGHT the bottom one.
const SWAPPED_BUTTONS: [(Button, u32); 12] = [
    (Button::West, 0),
    (Button::East, 1),
    (Button::North, 2),
    (Button::South, 3),
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
    /// A key that typed what Alt gives with it, or nothing: none of the
    /// computer's text for it goes, in any script.
    Alt,
}

/// The character a key gives on the computer's layout with these
/// modifiers, if it gives one.
type Layout = fn(Scancode, Mod) -> Option<char>;

/// The computer's layout, as SDL has it.
fn computer_layout(key: Scancode, keymod: Mod) -> Option<char> {
    use sdl3::sys::keycode::{SDL_Keymod, SDLK_SCANCODE_MASK};
    use sdl3::sys::scancode::SDL_Scancode;
    // SAFETY: SDL_GetKeyFromScancode only reads SDL's keymap, which events
    // change on this same thread, and before SDL is set up it uses its
    // default one.
    let code = unsafe {
        sdl3::sys::keyboard::SDL_GetKeyFromScancode(
            SDL_Scancode(key as i32),
            SDL_Keymod(keymod.bits()),
            false,
        )
    };
    // Key codes with the mask bit are keys without a character.
    (code.0 & SDLK_SCANCODE_MASK.0 == 0)
        .then(|| char::from_u32(code.0))
        .flatten()
        .filter(|c| !c.is_control())
}

/// Whether the key with this scancode is down among `keys`, by USB usage ID.
fn among(keys: &[u64; 4], key: Scancode) -> bool {
    u8::try_from(key as i32)
        .is_ok_and(|usage| keys[usize::from(usage / 64)] >> (usage % 64) & 1 != 0)
}

/// A character the runtime types itself as a key repeats: ASCII. What lies
/// outside it comes with the computer's own repeats.
fn ascii(c: Option<char>) -> u16 {
    c.filter(|c| c.is_ascii()).map_or(0, |c| c as u16)
}

/// How the keyboard types for programs, the prompt and the editor, as on
/// SmileBASIC 4: with its own Caps Lock, with repeats counted in the
/// machine's frames rather than by the computer's key repeat, and with its
/// own characters for Alt.
#[derive(Debug)]
pub struct Typing {
    /// SmileBASIC 4's Caps Lock, which is on at start, the same for
    /// programs, the prompt and the editor. The Caps Lock key toggles it
    /// wherever it is pressed, so it keeps step with the computer's own
    /// unless that was on when the emulator started or was toggled in
    /// another program's window.
    caps_lock: bool,
    last: Last,
    /// Keys type no text, as [`State::keys_type_nothing`] says of the
    /// last frame's controls.
    blocked: bool,
    /// The keyboard keys held on the last frame, by USB usage ID.
    held: [u64; 4],
    /// Letters type as SmileBASIC 4's do (the setting for capitals without
    /// Shift); otherwise they keep the case the computer gave them.
    pub upper_case: bool,
    /// The key that last went down.
    pressed: Option<Scancode>,
    /// What it types by the modifiers held, when the computer's text for it
    /// is still to come.
    text: Option<KeyText>,
    layout: Layout,
}

impl Default for Typing {
    fn default() -> Self {
        Typing::with_layout(computer_layout)
    }
}

impl Typing {
    /// Typing by the characters `layout` gives.
    pub fn with_layout(layout: Layout) -> Self {
        Typing {
            caps_lock: true,
            last: Last::None,
            blocked: false,
            held: [0; 4],
            upper_case: true,
            pressed: None,
            text: None,
            layout,
        }
    }

    /// Takes the controls held this frame, for the keys pressed next.
    pub fn hold(&mut self, held: &State) {
        self.blocked = held.keys_type_nothing();
        self.held = held.keys;
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
        self.pressed = scancode;
        self.text = None;
        if repeat {
            return;
        }
        if scancode == Some(Scancode::CapsLock) {
            self.caps_lock ^= true;
        }
        let usage = scancode.and_then(|s| u8::try_from(s as i32).ok());
        rt.input.key_down(usage.unwrap_or(0));
    }

    /// What `key` types by the modifiers held: Shift and SmileBASIC 4's
    /// Caps Lock, or Alt, which types SmileBASIC 4's own characters, or
    /// with Right Alt, on a layout where it is AltGr, the layout's.
    fn key_text(&self, key: Scancode, keymod: Mod) -> KeyText {
        let usage = u8::try_from(key as i32).unwrap_or(0);
        let alt = alt_key_code(usage).unwrap_or(0);
        let mut text = KeyText {
            alt,
            right_alt: alt,
            ..KeyText::default()
        };
        let layout = self.layout;
        let plain = layout(key, Mod::NOMOD);
        if let Some(c) = plain.filter(char::is_ascii_alphabetic)
            && self.upper_case
        {
            let (upper, lower) = (c.to_ascii_uppercase(), c.to_ascii_lowercase());
            let (plain, shift) = if self.caps_lock {
                (upper, lower)
            } else {
                (lower, upper)
            };
            (text.plain, text.shift) = (plain as u16, shift as u16);
        } else {
            let caps = keymod & Mod::CAPSMOD;
            text.plain = ascii(layout(key, caps));
            text.shift = ascii(layout(key, caps | Mod::LSHIFTMOD));
        }
        if let Some(c) = self.alt_gr(key) {
            text.right_alt = ascii(Some(c));
        }
        text
    }

    /// The character Right Alt gives with `key` on a layout where it is
    /// AltGr, if it gives one.
    fn alt_gr(&self, key: Scancode) -> Option<char> {
        let plain = (self.layout)(key, Mod::NOMOD);
        [Mod::MODEMOD, Mod::RALTMOD]
            .into_iter()
            .filter_map(|m| (self.layout)(key, m))
            .find(|&c| Some(c) != plain)
    }

    /// Whether Alt held now makes `key` type SmileBASIC 4's own character,
    /// or nothing: Left Alt does, and Right Alt unless it is AltGr for the
    /// key on the computer's layout.
    fn alt_table(&self, key: Option<Scancode>, keymod: Mod) -> bool {
        let left = keymod.contains(Mod::LALTMOD) || among(&self.held, Scancode::LAlt);
        let right =
            keymod.intersects(Mod::RALTMOD | Mod::MODEMOD) || among(&self.held, Scancode::RAlt);
        left || right && key.is_none_or(|key| self.alt_gr(key).is_none())
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
        let alt = code.is_none() && self.alt_table(self.pressed, keymod);
        if repeat {
            self.last = if alt { Last::Alt } else { Last::Repeat };
            return;
        }
        // Ctrl as SDL has it too, for a key that goes down in the frame
        // Ctrl does.
        let blocked = self.blocked || keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD);
        if let Some(code) = code {
            // Held on once Ctrl, L or R is released, it repeats as a
            // letter does. UNVERIFIED: that it does.
            match !blocked || types_when_blocked(code) {
                true => rt.type_held(code),
                false => rt.input.key_code(code),
            }
            return;
        }
        let text = self.pressed.map(|key| self.key_text(key, keymod));
        if let Some(text) = text {
            rt.input.key_text(text);
        }
        if alt {
            self.last = Last::Alt;
            if let Some(code) = text.map(|t| t.alt).filter(|&c| c != 0)
                && !blocked
            {
                rt.type_held(code);
            }
        } else if blocked {
            self.last = Last::Repeat;
        } else {
            self.last = Last::Typing {
                shift: keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD),
            };
            // A key the layout gives no character for, such as one on the
            // keypad, repeats what the computer types for it.
            self.text = text.filter(|t| t.plain == 0 && t.shift == 0);
        }
    }

    /// Text the computer typed.
    ///
    /// Text outside ASCII, from an input method or a layout of another
    /// script, types once as the computer gives it, its repeats included,
    /// as which key it came from is not known.
    pub fn text(&mut self, rt: &mut sb_runtime::Runtime, text: &str) {
        if self.last == Last::Alt {
            return;
        }
        for c in text.chars() {
            if !c.is_ascii() {
                // SmileBASIC strings hold only the Basic Multilingual Plane.
                if let Ok(code) = u16::try_from(u32::from(c)) {
                    rt.key_press(code);
                }
                continue;
            }
            let typed = match self.last {
                Last::Repeat | Last::Alt => continue,
                Last::Typing { shift } if self.upper_case => self.letter_case(c, shift),
                Last::Typing { .. } => c,
                Last::None if self.upper_case => {
                    rt.key_press(swap_case(c) as u16);
                    continue;
                }
                Last::None => {
                    rt.key_press(c as u16);
                    continue;
                }
            };
            rt.type_held(typed as u16);
            if let Some(mut text) = self.text.take() {
                (text.plain, text.shift) = (typed as u16, typed as u16);
                rt.input.key_text(text);
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

/// The bench's keys come from this keyboard, which is none, so that they
/// stay out of [`Controls::taps`] as they stay out of what is held.
pub const BENCH_KEYBOARD: u32 = u32::MAX;

/// What an event presses or releases for the program, for
/// [`sb_runtime::input::Input::tap`]: a key, and the button it presses as
/// a controller's; or a controller's button.
pub type Taps = [Option<(Tap, bool)>; 2];

/// ZL and ZR's triggers, and their bits in BUTTON's value.
const TRIGGERS: [(Axis, u32); 2] = [(Axis::TriggerLeft, 10), (Axis::TriggerRight, 11)];

/// What a controller's event presses or releases, as controller `pad`, 0
/// to 3: a button as [`face`] made it, or a trigger past its threshold
/// one way or the other, by `triggers`, which this keeps.
fn pad_tap(event: &Event, pad: usize, triggers: &mut [bool; 2]) -> Option<(Tap, bool)> {
    let (bits, down) = match *event {
        Event::GamepadButtonDown { button, .. } => (button_bit(button), true),
        Event::GamepadButtonUp { button, .. } => (button_bit(button), false),
        Event::GamepadAxisMotion { axis, value, .. } => {
            let i = TRIGGERS.iter().position(|&(a, _)| a == axis)?;
            let down = value > TRIGGER;
            if std::mem::replace(&mut triggers[i], down) == down {
                return None;
            }
            (1 << TRIGGERS[i].1, down)
        }
        _ => return None,
    };
    // Select, Start and the like have no bit.
    (bits != 0).then_some((Tap::Buttons(pad, bits), down))
}

/// What a key's event presses or releases: the key, by its USB usage ID,
/// and with `buttons`, when the keyboard is a controller too, the button
/// of controller 1 it presses. A key's repeat and the bench's keys press
/// nothing.
fn key_taps(event: &Event, buttons: Option<&KeyButtons>) -> Taps {
    let (key, down) = match *event {
        Event::KeyDown {
            scancode: Some(key),
            repeat: false,
            which,
            ..
        } if which != BENCH_KEYBOARD => (key, true),
        Event::KeyUp {
            scancode: Some(key),
            which,
            ..
        } if which != BENCH_KEYBOARD => (key, false),
        _ => return [None; 2],
    };
    let usage = u8::try_from(key as i32).ok().filter(|&usage| usage != 0);
    let bits = buttons.map_or(0, |b| b.buttons(|k| k == key));
    [
        usage.map(|usage| (Tap::Key(usage), down)),
        (bits != 0).then_some((Tap::Buttons(0, bits), down)),
    ]
}

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
    /// The program saw nothing pressed in the last frame.
    blanked: bool,
}

impl Carried {
    /// What the program sees of `state`, once a frame. `screen`: a screen
    /// has the controls now, so that the program sees nothing at all. The
    /// frame it goes in has no buttons or sticks yet, as the press that closed
    /// it came in that frame, whichever order the events came in.
    pub fn program_sees(&mut self, mut state: State, screen: bool) -> State {
        let was_screen = std::mem::replace(&mut self.screen, screen);
        self.blanked = screen || was_screen;
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

    /// Whether [`Carried::program_sees`] left out every button and stick
    /// in its last frame, as a screen had the controls in it or the frame
    /// before: so that a press or release in that frame is none of the
    /// program's either.
    pub fn blanked(&self) -> bool {
        self.blanked
    }
}

/// #B_RSTICK's bit in BUTTON's value: R3, the right stick pressed.
const R3: u32 = 1 << 13;

/// The width of SmileBASIC 4's display, in whose pixels the right stick
/// moves the pointer.
const SB4_DISPLAY_W: i32 = 1280;

/// How far the right stick tilted by `x`, `y` (Y down, as [`Pad::sticks`]
/// has it) moves the mouse pointer in a frame, in pixels of SmileBASIC
/// 4's 1280x720 display, as SmileBASIC 4 moves it: a speed by how far the
/// stick is tilted, nothing up to 0.3, and no speeding up while held.
///
/// [`Pad::sticks`]: sb_runtime::input::Pad::sticks
pub fn stick_step(x: f64, y: f64) -> (i32, i32) {
    let k = match x.hypot(y) {
        m if m > 0.85 => 16.0,
        m if m > 0.7 => 8.0,
        m if m > 0.4 => 4.0,
        m if m > 0.3 => 3.0,
        _ => return (0, 0),
    };
    let step = |a: f64| (a.abs() * k + 0.5) as i32;
    // An axis goes left or up only from a tenth of a tilt that way, and
    // right or down below that: a stick held right that drifts a little
    // up still goes down.
    let dx = if x <= -0.1 { -step(x) } else { step(x) };
    let dy = if -y >= 0.1 { -step(y) } else { step(y) };
    (dx, dy)
}

/// Puts the pointer at the screen's pixel `screen`, of a screen `size`
/// big, and the display's pixel `display`, of a display `display_size`
/// big. The program reads a pointer past the screen's edges as at them.
pub fn place_mouse(
    m: &mut Mouse,
    (sx, sy): (i32, i32),
    (w, h): (i32, i32),
    (dx, dy): (i32, i32),
    (display_w, display_h): (i32, i32),
) {
    (m.x, m.y) = (sx.clamp(0, w - 1), sy.clamp(0, h - 1));
    m.display = (dx.clamp(0, display_w - 1), dy.clamp(0, display_h - 1));
    m.on_screen = (0..w).contains(&sx) && (0..h).contains(&sy);
}

/// Select (SDL's Back, the − button) as the right-stick mouse and the
/// projects see it.
#[derive(Debug, Default)]
pub struct Select {
    held: bool,
    /// Another button went down while it was held, or the pointer moved.
    used: bool,
    /// Its press may open the projects at its release: see
    /// [`Select::arm`].
    armed: bool,
}

impl Select {
    fn event(&mut self, event: &Event) {
        match *event {
            Event::GamepadButtonDown {
                button: Button::Back,
                ..
            } => {
                *self = Select {
                    held: true,
                    ..Select::default()
                }
            }
            Event::GamepadButtonUp {
                button: Button::Back,
                ..
            } => self.held = false,
            Event::GamepadButtonDown { .. } => self.used |= self.held,
            _ => {}
        }
    }

    /// The press of Select now is one that opens the projects if it is
    /// released with nothing else done meanwhile.
    pub fn arm(&mut self) {
        self.armed = self.held;
    }

    /// At Select's release: whether it was pressed alone, after an
    /// [`Select::arm`] at its press. Only once for each press.
    pub fn alone(&mut self) -> bool {
        std::mem::take(&mut self.armed) && !self.used
    }
}

/// The right stick and R3 as the program does not see them: while Select
/// moves the pointer, and after that until they are released.
#[derive(Debug, Default)]
struct HeldBack {
    r3: [bool; PADS],
    stick: [bool; PADS],
}

impl HeldBack {
    /// Holds back what Select's pointer took this frame, with `select`
    /// held, from `state`, as the program will see it.
    fn apply(&mut self, state: &mut State, select: bool) {
        for (i, pad) in state.pads.iter_mut().enumerate() {
            let r3 = pad.buttons & R3 != 0;
            let (x, y) = pad.sticks[1];
            let tilted = x.abs() > CARRIED_STICK || y.abs() > CARRIED_STICK;
            self.r3[i] = r3 && (select || self.r3[i]);
            self.stick[i] = select || tilted && self.stick[i];
            if self.r3[i] {
                pad.buttons &= !R3;
            }
            if self.stick[i] {
                pad.sticks[1] = (0.0, 0.0);
            }
        }
    }
}

/// The pointer's place in SmileBASIC 4's display pixels, finer than the
/// display's own where that is smaller, so that the right stick moves it
/// as far across as on SmileBASIC 4.
#[derive(Debug, Default)]
struct StickPointer {
    at: (i32, i32),
    /// The display pixel `at` gave last; the pointer comes from the
    /// display again if it went elsewhere.
    gave: Option<(i32, i32)>,
}

impl StickPointer {
    /// The display pixel `display` moves to by `step`, in SmileBASIC 4's
    /// pixels, on a display `size` big. It stays on the display.
    fn moved(&mut self, display: (i32, i32), step: (i32, i32), size: (u32, u32)) -> (i32, i32) {
        let (w, h) = (size.0.max(1) as i32, size.1.max(1) as i32);
        if self.gave != Some(display) {
            let fine = |d: i32| (d * SB4_DISPLAY_W + w - 1) / w;
            self.at = (fine(display.0), fine(display.1));
        }
        let max = (SB4_DISPLAY_W - 1, h * SB4_DISPLAY_W / w - 1);
        self.at = (
            (self.at.0 + step.0).clamp(0, max.0),
            (self.at.1 + step.1).clamp(0, max.1),
        );
        let coarse = |v: i32, n: i32| (v * w / SB4_DISPLAY_W).clamp(0, n - 1);
        let to = (coarse(self.at.0, w), coarse(self.at.1, h));
        self.gave = Some(to);
        to
    }
}

/// The right stick as the right-stick mouse reads it: each axis from the
/// controller tilted furthest along it, and whether any has R3 pressed.
fn right_stick(state: &State) -> ((f64, f64), bool) {
    // UNVERIFIED: SmileBASIC 4 merges the controllers into one, by the
    // largest reading of each axis; taken here as the largest either way.
    let furthest = |a: f64, b: f64| if b.abs() > a.abs() { b } else { a };
    state
        .pads
        .iter()
        .fold(((0.0, 0.0), false), |((x, y), r3), pad| {
            let (px, py) = pad.sticks[1];
            (
                (furthest(x, px), furthest(y, py)),
                r3 || pad.buttons & R3 != 0,
            )
        })
}

/// SmileBASIC 4's right-stick mouse, and Select's use of it.
#[derive(Debug, Default)]
pub struct StickMouse {
    /// The setting Right stick moves the mouse, for the next event and
    /// frame.
    pub on: bool,
    pub select: Select,
    held_back: HeldBack,
    pointer: StickPointer,
}

impl StickMouse {
    /// Whether controller `pad`'s `tap` is R3 while the program does not
    /// see it, as [`StickMouse::frame`] keeps it.
    fn holds_back(&self, pad: usize, tap: Tap) -> bool {
        self.on && tap == Tap::Buttons(pad, R3) && (self.select.held || self.held_back.r3[pad])
    }

    /// The right-stick mouse in a frame, on `state` as the program is to
    /// see it: R3 is the left mouse button, and the right stick moves the
    /// pointer from the display pixel `from` to the one this returns, on
    /// a display `size` big. `shown`: no program runs, or the program
    /// shows the mouse cursor, as SmileBASIC 4 needs. With Select held it
    /// works whatever the program does, which then sees neither the right
    /// stick nor R3.
    pub fn frame(
        &mut self,
        state: &mut State,
        shown: bool,
        from: (i32, i32),
        size: (u32, u32),
    ) -> Option<(i32, i32)> {
        if !self.on {
            self.held_back = HeldBack::default();
            return None;
        }
        let select = self.select.held;
        let ((x, y), r3) = right_stick(state);
        self.held_back.apply(state, select);
        if !shown && !select {
            return None;
        }
        // It takes the place of the mouse's buttons, as on SmileBASIC 4.
        if r3 {
            state.mouse.buttons = 1;
        }
        let step = stick_step(x, y);
        if step == (0, 0) {
            return None;
        }
        self.select.used |= select;
        Some(self.pointer.moved(from, step, size))
    }

    /// Whether Select has the right stick for the pointer now, which
    /// shows the cursor whatever the program says.
    pub fn select_points(&self) -> bool {
        self.on && self.select.held
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
    /// The setting Swap A/B and X/Y, for the next event and state.
    pub swap: bool,
    /// Buttons held on the first controller, as SDL places them, for the
    /// bench.
    pub bench: u32,
    /// Each controller's triggers held past their threshold, as their
    /// events last said, for [`Controls::taps`].
    triggers: [[bool; 2]; PADS],
    pub stick_mouse: StickMouse,
}

fn stick(pad: &Gamepad, x: Axis, y: Axis) -> (f64, f64) {
    let read = |a| (f64::from(pad.axis(a)) / f64::from(i16::MAX)).clamp(-1.0, 1.0);
    let (x, y) = (read(x), read(y));
    // A Switch stick stays within a circle of radius 1, and programs index
    // arrays by its length; gamepads that reach the corners of a square
    // are brought into that circle.
    match x.hypot(y) {
        len if len < DEAD_ZONE => (0.0, 0.0),
        len if len > 1.0 => (x / len, y / len),
        _ => (x, y),
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
            swap: false,
            bench: 0,
            triggers: Default::default(),
            stick_mouse: StickMouse::default(),
        })
    }

    /// How many gamepads are connected.
    pub fn gamepads(&self) -> usize {
        self.pads.len()
    }

    /// Opens controllers as they are connected, and lets go of them; a
    /// button's event gets the button [`face`] makes of it. Goes first
    /// for every event the system gives.
    pub fn event(&mut self, event: &mut Event) {
        swap_event(event, self.swap);
        self.stick_mouse.select.event(event);
        match *event {
            Event::GamepadAdded { which, .. } if self.pads.len() < PADS => {
                match self.subsystem.open(which) {
                    Ok(pad) => self.pads.push(pad),
                    Err(e) => eprintln!("could not open a controller: {e}"),
                }
            }
            Event::GamepadRemoved { which, .. } => {
                self.pads.retain(|p| p.id().is_ok_and(|id| id != which));
                // The others may have moved up.
                self.triggers = Default::default();
            }
            _ => {}
        }
    }

    /// The presses and releases of `event` for the program, which
    /// [`sb_runtime::input::Input::tap`] keeps for the next frame in case
    /// what is held then misses them. Goes after [`Controls::event`], for
    /// events the system gave. `keys_press_buttons`: the keyboard presses
    /// controller 1's buttons too.
    pub fn taps(&mut self, event: &Event, keys_press_buttons: bool) -> Taps {
        let (Event::GamepadButtonDown { which, .. }
        | Event::GamepadButtonUp { which, .. }
        | Event::GamepadAxisMotion { which, .. }) = *event
        else {
            return key_taps(event, keys_press_buttons.then_some(&self.key_buttons));
        };
        // No controller has the id 0: the bench's buttons come so, and
        // are controller 1's.
        let pad = match which == 0.into() {
            true => Some(0),
            false => (self.pads.iter()).position(|p| p.id().is_ok_and(|id| id == which)),
        };
        match pad {
            Some(pad) => {
                let tap = pad_tap(event, pad, &mut self.triggers[pad]);
                [
                    tap.filter(|&(tap, _)| !self.stick_mouse.holds_back(pad, tap)),
                    None,
                ]
            }
            None => [None; 2],
        }
    }

    /// [`StickMouse::frame`] for `state` as [`Controls::state`] made it,
    /// from where the pointer is now.
    pub fn stick_pointer(
        &mut self,
        state: &mut State,
        shown: bool,
        size: (u32, u32),
    ) -> Option<(i32, i32)> {
        (self.stick_mouse).frame(state, shown, self.mouse.display, size)
    }

    /// Puts the pointer where [`Controls::stick_pointer`] moved it, in
    /// `state` too; the arguments are [`place_mouse`]'s.
    pub fn place_pointer(
        &mut self,
        state: &mut State,
        screen: (i32, i32),
        size: (i32, i32),
        display: (i32, i32),
        display_size: (i32, i32),
    ) {
        place_mouse(&mut self.mouse, screen, size, display, display_size);
        state.mouse = Mouse {
            buttons: state.mouse.buttons,
            ..self.mouse
        };
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
            p.buttons = held_buttons(|button| pad.button(button), self.swap);
            for (axis, id) in TRIGGERS {
                if pad.axis(axis) > TRIGGER {
                    p.buttons |= 1 << id;
                }
            }
            p.sticks = [
                stick(pad, Axis::LeftX, Axis::LeftY),
                stick(pad, Axis::RightX, Axis::RightY),
            ];
        }
        if self.bench != 0 {
            let bench = held_buttons(|b| self.bench & button_bit(b) != 0, self.swap);
            state.pads[0].buttons |= bench;
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
        let mut rt = sb_runtime::Runtime::for_tests(sb_runtime::font::Font::blank());
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

    /// #B_RDOWN, which is B.
    const B_BUTTON: u32 = 1 << 1;

    #[test]
    fn the_swap_trades_a_with_b_and_x_with_y_only() {
        let traded = [(Button::East, Button::South), (Button::North, Button::West)];
        for (a, b) in traded {
            assert_eq!((face(a, true), face(b, true)), (b, a));
            assert_eq!((face(a, false), face(b, false)), (a, b));
        }
        for button in [
            Button::DPadUp,
            Button::Back,
            Button::Start,
            Button::LeftShoulder,
        ] {
            assert_eq!(face(button, true), button);
        }
        // The table programs read by is the same trade.
        for ((button, id), (swapped, swapped_id)) in PAD_BUTTONS.iter().zip(&SWAPPED_BUTTONS) {
            assert_eq!((face(*button, true), *id), (*swapped, *swapped_id));
        }
    }

    #[test]
    fn events_and_held_buttons_swap_alike() {
        let down = |button| Event::GamepadButtonDown {
            timestamp: 0,
            which: 0.into(),
            button,
        };
        let up = |button| Event::GamepadButtonUp {
            timestamp: 0,
            which: 0.into(),
            button,
        };
        let button_of = |event: &Event| match *event {
            Event::GamepadButtonDown { button, .. } | Event::GamepadButtonUp { button, .. } => {
                Some(button)
            }
            _ => None,
        };
        for (swap, a) in [(false, Button::East), (true, Button::South)] {
            // What the emulator's screens take as A...
            for mut event in [down(a), up(a)] {
                swap_event(&mut event, swap);
                assert_eq!(button_of(&event), Some(Button::East), "swap {swap}");
            }
            // ...and what BUTTON reads as A.
            assert_eq!(held_buttons(|b| b == a, swap), A_BUTTON);
        }
        assert_eq!(held_buttons(|b| b == Button::East, true), B_BUTTON);
        assert_eq!(held_buttons(|b| b == Button::West, true), 1 << 0, "X");
        assert_eq!(held_buttons(|b| b == Button::DPadUp, true), 1 << 4);
        let mut quit = Event::Quit { timestamp: 0 };
        swap_event(&mut quit, true);
        assert!(matches!(quit, Event::Quit { .. }));
    }

    /// The A that turns the swap on, in the pause menu, is held on as the
    /// setting changes: as B now. The program sees neither, not even once
    /// the menu has closed, until it is released.
    #[test]
    fn a_button_held_through_the_swap_reads_as_nothing() {
        let east = |b| b == Button::East;
        let none = |_| false;
        let mut carried = Carried::default();
        let mut sees = |held: u32, screen| carried.program_sees(holding(held), screen);
        // Pressed for the menu, then the setting flips.
        assert_eq!(sees(held_buttons(east, false), true).pads[0].buttons, 0);
        assert_eq!(sees(held_buttons(east, true), true).pads[0].buttons, 0);
        // The menu closes with the button still down.
        for _ in 0..3 {
            assert_eq!(sees(held_buttons(east, true), false).pads[0].buttons, 0);
        }
        assert_eq!(sees(held_buttons(none, true), false).pads[0].buttons, 0);
        // Pressed again, it is B.
        let again = sees(held_buttons(east, true), false);
        assert_eq!(again.pads[0].buttons, B_BUTTON);
        // Nor does it type anything for INKEY$.
        let a = held_buttons(east, false);
        let b = held_buttons(east, true);
        let frames = [(a, true), (b, true), (b, false), (b, false), (0, false)];
        assert_eq!(typed_after(&frames), 0);
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

    fn pad_event(button: Button, down: bool) -> Event {
        let (timestamp, which) = (0, 7.into());
        match down {
            true => Event::GamepadButtonDown {
                timestamp,
                which,
                button,
            },
            false => Event::GamepadButtonUp {
                timestamp,
                which,
                button,
            },
        }
    }

    #[test]
    fn a_button_taps_its_bit_as_the_swap_makes_it() {
        let mut triggers = [false; 2];
        // The right button is A; swapped, the bottom one is.
        for (swap, pressed) in [(false, Button::East), (true, Button::South)] {
            for down in [true, false] {
                let mut event = pad_event(pressed, down);
                swap_event(&mut event, swap);
                let tap = pad_tap(&event, 2, &mut triggers);
                assert_eq!(tap, Some((Tap::Buttons(2, A_BUTTON), down)), "swap {swap}");
            }
        }
        // Select, Start and Home have no bit: the frontend has them.
        for button in [Button::Back, Button::Start, Button::Guide] {
            assert_eq!(pad_tap(&pad_event(button, true), 0, &mut triggers), None);
        }
    }

    #[test]
    fn a_trigger_taps_once_each_way_past_its_threshold() {
        let mut triggers = [false; 2];
        let mut tap = |axis, value| {
            let event = Event::GamepadAxisMotion {
                timestamp: 0,
                which: 7.into(),
                axis,
                value,
            };
            pad_tap(&event, 1, &mut triggers)
        };
        let zr = Tap::Buttons(1, 1 << 11);
        assert_eq!(tap(Axis::TriggerRight, TRIGGER), None, "at the threshold");
        assert_eq!(tap(Axis::TriggerRight, TRIGGER + 1), Some((zr, true)));
        assert_eq!(tap(Axis::TriggerRight, i16::MAX), None, "still down");
        assert_eq!(
            tap(Axis::TriggerLeft, i16::MAX).map(|t| t.0),
            Some(Tap::Buttons(1, 1 << 10))
        );
        assert_eq!(tap(Axis::TriggerRight, 0), Some((zr, false)));
        assert_eq!(tap(Axis::TriggerRight, 0), None);
        assert_eq!(tap(Axis::LeftX, i16::MAX), None, "a stick taps nothing");
    }

    fn key_event(key: Scancode, down: bool, repeat: bool, which: u32) -> Event {
        let (timestamp, window_id, keycode, keymod, raw) = (0, 1, None, Mod::NOMOD, 0);
        let scancode = Some(key);
        match down {
            true => Event::KeyDown {
                timestamp,
                window_id,
                keycode,
                scancode,
                keymod,
                repeat,
                which,
                raw,
            },
            false => Event::KeyUp {
                timestamp,
                window_id,
                keycode,
                scancode,
                keymod,
                repeat,
                which,
                raw,
            },
        }
    }

    #[test]
    fn a_key_taps_itself_and_its_button_unless_it_repeats() {
        let keys = KeyButtons::default();
        let z = Tap::Key(Scancode::Z as i32 as u8);
        let a = Tap::Buttons(0, A_BUTTON);
        for down in [true, false] {
            let event = key_event(Scancode::Z, down, false, 3);
            assert_eq!(
                key_taps(&event, Some(&keys)),
                [Some((z, down)), Some((a, down))]
            );
            // Without the keyboard as a controller, the key alone.
            assert_eq!(key_taps(&event, None), [Some((z, down)), None]);
        }
        // The system's repeat of a held key, and the bench's keys, press
        // nothing.
        let repeat = key_event(Scancode::Z, true, true, 3);
        assert_eq!(key_taps(&repeat, Some(&keys)), [None, None]);
        let bench = key_event(Scancode::Z, true, false, BENCH_KEYBOARD);
        assert_eq!(key_taps(&bench, Some(&keys)), [None, None]);
        // A key with no button, and one the program reads for itself.
        let enter = key_event(Scancode::Return, true, false, 3);
        assert_eq!(
            key_taps(&enter, Some(&keys)),
            [Some((Tap::Key(0x28), true)), None]
        );
        let mut reserved = KeyButtons::default();
        reserved.update(read("B=BUTTON(0):K=KEYBOARD(29,2)"));
        let event = key_event(Scancode::Z, true, false, 3);
        assert_eq!(key_taps(&event, Some(&reserved)), [Some((z, true)), None]);
    }

    #[test]
    fn the_frame_a_screen_closes_in_takes_no_taps_either() {
        let mut carried = Carried::default();
        carried.program_sees(holding(0), false);
        assert!(!carried.blanked());
        carried.program_sees(holding(A_BUTTON), true);
        assert!(carried.blanked());
        // The B that closed it came in the frame after.
        carried.program_sees(holding(0), false);
        assert!(carried.blanked());
        carried.program_sees(holding(0), false);
        assert!(!carried.blanked());
    }

    fn read(source: &str) -> KeysRead {
        use sb_core::vm::{Status, Vm};
        let mut rt = sb_runtime::Runtime::for_tests(sb_runtime::font::Font::blank());
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
        sb_runtime::Runtime::for_tests(sb_runtime::font::Font::blank())
    }

    /// Part of a US layout, the same on every computer the tests run on.
    fn us_layout(key: Scancode, keymod: Mod) -> Option<char> {
        let shift = keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
        let caps = keymod.contains(Mod::CAPSMOD);
        let usage = key as i32;
        let n = |first: i32| (usage - first) as u8;
        Some(match usage {
            4..=29 if shift != caps => (b'A' + n(4)) as char,
            4..=29 => (b'a' + n(4)) as char,
            30..=38 if shift => b"!@#$%^&*("[usize::from(n(30))] as char,
            30..=38 => (b'1' + n(30)) as char,
            39 if shift => ')',
            39 => '0',
            44 => ' ',
            56 if shift => '?',
            56 => '/',
            _ => return None,
        })
    }

    /// A layout where Right Alt is AltGr: with Q it gives "@", as German
    /// layouts do, and with E "€".
    fn alt_gr_layout(key: Scancode, keymod: Mod) -> Option<char> {
        match key {
            Scancode::Q if keymod.contains(Mod::MODEMOD) => Some('@'),
            Scancode::E if keymod.contains(Mod::MODEMOD) => Some('€'),
            _ => us_layout(key, keymod),
        }
    }

    fn typing() -> Typing {
        Typing::with_layout(us_layout)
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
        keys.upper_case = upper;
        keys.text(rt, text);
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
        let mut keys = typing();
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
        let mut keys = typing();
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
        let mut keys = typing();
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
        let mut keys = typing();
        let mut rt = runtime();
        keys.any_key_down(&mut rt, Some(Scancode::A), true);
        keys.key_down(&mut rt, Mod::NOMOD, true, None);
        keys.text(&mut rt, "a");
        keys.text(&mut rt, "\u{AC00}");
        keys.any_key_down(&mut rt, Some(Scancode::Return), true);
        keys.key_down(&mut rt, Mod::NOMOD, true, Some(13));
        assert_eq!(inkey(&mut rt), "AC00");
    }

    #[test]
    fn keys_that_type_no_text_type_their_code() {
        let mut keys = typing();
        let mut rt = runtime();
        keys.any_key_down(&mut rt, Some(Scancode::Return), false);
        let code = sb_runtime::runtime::keys::ENTER;
        keys.key_down(&mut rt, Mod::NOMOD, false, Some(code));
        assert_eq!(inkey(&mut rt), "D");
    }

    /// What a key that types `code` of its own types.
    fn press_code(keys: &mut Typing, key: Scancode, keymod: Mod, code: u16) -> String {
        let mut rt = runtime();
        keys.any_key_down(&mut rt, Some(key), false);
        keys.key_down(&mut rt, keymod, false, Some(code));
        inkey(&mut rt)
    }

    /// Letters typed while the controller's L and R are held type nothing,
    /// nor do they with L or R alone or with Right Ctrl. Of the keys that
    /// type a code of their own, Enter, Backspace, Tab, Delete and Esc type
    /// nothing then either, while the arrow keys, Home and End still type.
    #[test]
    fn keys_type_nothing_with_l_or_r_or_ctrl_held() {
        use sb_runtime::runtime::keys::{BACKSPACE, DELETE, END, ENTER, ESCAPE, HOME, LEFT, TAB};
        let mut keys = typing();
        keys.hold(&holding(0b11 << 8));
        assert_eq!(press(&mut keys, Scancode::A, Mod::NOMOD, "a", true), "");
        // L, R, and Left Ctrl.
        let held = [
            (holding(1 << 8), Mod::NOMOD),
            (holding(1 << 9), Mod::NOMOD),
            (State::default(), Mod::LCTRLMOD),
        ];
        for (held, keymod) in held {
            keys.hold(&held);
            assert_eq!(press_code(&mut keys, Scancode::Return, keymod, ENTER), "");
            assert_eq!(
                press_code(&mut keys, Scancode::Backspace, keymod, BACKSPACE),
                ""
            );
            assert_eq!(press_code(&mut keys, Scancode::Left, keymod, LEFT), "1D");
            let silent = [
                (Scancode::Tab, TAB),
                (Scancode::Delete, DELETE),
                (Scancode::Escape, ESCAPE),
            ];
            for (key, code) in silent {
                assert_eq!(press_code(&mut keys, key, keymod, code), "");
            }
            assert_eq!(press_code(&mut keys, Scancode::Home, keymod, HOME), "2");
            assert_eq!(press_code(&mut keys, Scancode::End, keymod, END), "3");
        }
        keys.hold(&State::default());
        assert_eq!(
            press_code(&mut keys, Scancode::Return, Mod::NOMOD, ENTER),
            "D"
        );
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
        let mut keys = typing();
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
        let mut keys = typing();
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
        // TEST69K: Caps Lock sets no bit.
        assert_eq!(modifiers(|k| k == Scancode::CapsLock), 0);
    }

    /// The prompt's line after `key` went down with `keymod` and typed
    /// `text`, and then `frames` frames with the keys `held` gives for
    /// each held.
    fn prompt_after(
        keys: &mut Typing,
        (key, keymod, text): (Scancode, Mod, &str),
        frames: usize,
        held: impl Fn(usize) -> Vec<Scancode>,
    ) -> String {
        let mut m = machine(false);
        type_key(keys, &mut m.rt, key, keymod, text, true);
        for i in 0..frames {
            let mut state = State::default();
            for k in held(i) {
                state.set_key(k as i32 as u8, true);
            }
            m.rt.input.live = state;
            m.frame();
        }
        last_row(&m).unwrap_or_default()
    }

    /// A held key's repeats type what the modifiers held give then: 1 with
    /// Shift gone down types "!" 19 frames later, and A "a".
    #[test]
    fn repeats_type_what_shift_gives_now() {
        let mut keys = typing();
        let with_shift = |key| {
            move |i| match i {
                0..5 => vec![key],
                _ => vec![key, Scancode::LShift],
            }
        };
        let one = (Scancode::_1, Mod::NOMOD, "1");
        assert_eq!(
            prompt_after(&mut keys, one, 30, with_shift(Scancode::_1)),
            "1!!"
        );
        let a = (Scancode::A, Mod::NOMOD, "a");
        assert_eq!(
            prompt_after(&mut keys, a, 30, with_shift(Scancode::A)),
            "Aaa"
        );
    }

    /// A key pressed with Ctrl types nothing, but held on once Ctrl is let
    /// go it repeats.
    #[test]
    fn a_key_pressed_with_ctrl_repeats_once_ctrl_is_let_go() {
        let mut keys = typing();
        let a = (Scancode::A, Mod::LCTRLMOD, "a");
        assert_eq!(prompt_after(&mut keys, a, 23, |_| vec![Scancode::A]), "AA");
    }

    /// A keypad key, which the layout gives no character for, repeats what
    /// the computer typed for it.
    #[test]
    fn a_keypad_key_repeats_what_it_typed() {
        let mut keys = typing();
        let one = (Scancode::Kp1, Mod::NOMOD, "1");
        assert_eq!(
            prompt_after(&mut keys, one, 21, |_| vec![Scancode::Kp1]),
            "11"
        );
    }

    /// Alt with a key types SmileBASIC 4's own character, whatever text the
    /// computer gave, with either Alt and Shift or not, or nothing for keys
    /// without one.
    #[test]
    fn alt_types_smilebasic_4s_own_characters() {
        let mut keys = typing();
        let left = Mod::LALTMOD;
        assert_eq!(press(&mut keys, Scancode::A, left, "\u{E5}", true), "E2EC");
        assert_eq!(
            press(&mut keys, Scancode::A, Mod::RALTMOD, "a", true),
            "E2EC"
        );
        let shift_alt = left | Mod::LSHIFTMOD;
        assert_eq!(press(&mut keys, Scancode::A, shift_alt, "A", true), "E2EC");
        assert_eq!(press(&mut keys, Scancode::V, left, "v", true), "2606");
        assert_eq!(press(&mut keys, Scancode::_3, left, "3", true), "");
        // With Ctrl too, nothing.
        let ctrl_alt = left | Mod::LCTRLMOD;
        assert_eq!(press(&mut keys, Scancode::A, ctrl_alt, "a", true), "");
        // Right Alt as the key held, on a layout that makes it Han/Eng.
        let mut held = State::default();
        held.set_key(Scancode::RAlt as i32 as u8, true);
        keys.hold(&held);
        assert_eq!(press(&mut keys, Scancode::A, Mod::NOMOD, "a", true), "E2EC");
        // Held on, it repeats.
        keys.hold(&State::default());
        let alt_a = (Scancode::A, left, "a");
        let alt_held = |_| vec![Scancode::A, Scancode::LAlt];
        let repeated = prompt_after(&mut keys, alt_a, 21, alt_held);
        assert_eq!(repeated, "\u{E2EC}\u{E2EC}");
    }

    /// On a layout where Right Alt is AltGr, it types the layout's
    /// character with keys that have one, and SmileBASIC 4's with the rest.
    /// Left Alt always types SmileBASIC 4's.
    #[test]
    fn right_alt_types_the_layouts_alt_gr_characters() {
        let mut keys = Typing::with_layout(alt_gr_layout);
        let alt_gr = Mod::MODEMOD;
        assert_eq!(press(&mut keys, Scancode::Q, alt_gr, "@", true), "40");
        assert_eq!(
            press(&mut keys, Scancode::E, alt_gr, "\u{20AC}", true),
            "20AC"
        );
        assert_eq!(press(&mut keys, Scancode::A, alt_gr, "a", true), "E2EC");
        let left = Mod::LALTMOD;
        assert_eq!(press(&mut keys, Scancode::Q, left, "q", true), "E2F0");
        // "@" repeats as the key is held.
        let q = (Scancode::Q, Mod::RALTMOD | alt_gr, "@");
        let held = |_| vec![Scancode::Q, Scancode::RAlt];
        assert_eq!(prompt_after(&mut keys, q, 21, held), "@@");
    }

    #[test]
    fn the_right_stick_moves_the_pointer_by_how_far_it_is_tilted() {
        assert_eq!(stick_step(1.0, 0.0), (16, 0));
        assert_eq!(stick_step(0.86, 0.0), (14, 0));
        assert_eq!(stick_step(0.85, 0.0), (7, 0));
        assert_eq!(stick_step(0.8, 0.0), (6, 0));
        assert_eq!(stick_step(0.7, 0.0), (3, 0));
        assert_eq!(stick_step(0.5, 0.0), (2, 0));
        assert_eq!(stick_step(0.4, 0.0), (1, 0));
        assert_eq!(stick_step(0.35, 0.0), (1, 0));
        assert_eq!(stick_step(-1.0, 0.0), (-16, 0));
        assert_eq!(stick_step(0.0, 1.0), (0, 16));
        assert_eq!(stick_step(0.0, -1.0), (0, -16));
        // Each axis goes by its share of a full tilt.
        let d = std::f64::consts::FRAC_1_SQRT_2;
        assert_eq!(stick_step(d, -d), (11, -11));
        assert_eq!(stick_step(-d, d), (-11, 11));
    }

    #[test]
    fn the_right_stick_moves_nothing_up_to_three_tenths() {
        for (x, y) in [
            (0.3, 0.0),
            (0.25, 0.0),
            (0.2, -0.2),
            (0.0, 0.0),
            (-0.1, 0.28),
        ] {
            assert_eq!(stick_step(x, y), (0, 0), "{x}, {y}");
        }
    }

    #[test]
    fn a_reading_short_of_a_tenth_left_or_up_goes_right_or_down() {
        // Held right, drifting a little either way: down by a pixel.
        assert_eq!(stick_step(1.0, -0.05), (16, 1));
        assert_eq!(stick_step(1.0, 0.05), (16, 1));
        // Held up, drifting a little left: right by a pixel.
        assert_eq!(stick_step(-0.05, -1.0), (1, -16));
        // From a tenth, that way.
        assert_eq!(stick_step(-0.1, -1.0), (-2, -16));
        assert_eq!(stick_step(1.0, -0.1), (16, -2));
    }

    #[test]
    fn the_pointer_stays_on_the_display() {
        let mut p = StickPointer::default();
        let size = (1280, 720);
        assert_eq!(p.moved((1275, 3), (16, -16), size), (1279, 0));
        assert_eq!(p.moved((1279, 0), (16, -16), size), (1279, 0));
        assert_eq!(p.moved((0, 715), (-16, 16), size), (0, 719));
        assert_eq!(p.moved((0, 719), (5, -1), size), (5, 718));
    }

    #[test]
    fn a_smaller_display_takes_steps_of_smilebasic_4s_pixels() {
        // Half as wide: two of SmileBASIC 4's pixels to one of its own.
        let mut p = StickPointer::default();
        let size = (640, 360);
        assert_eq!(p.moved((100, 100), (1, 1), size), (100, 100));
        assert_eq!(p.moved((100, 100), (1, 1), size), (101, 101));
        assert_eq!(p.moved((101, 101), (16, 0), size), (109, 101));
        // Moved elsewhere meanwhile, by the mouse: it goes on from there.
        assert_eq!(p.moved((10, 10), (2, 0), size), (11, 10));
        assert_eq!(p.moved((11, 10), (2000, 2000), size), (639, 359));
    }

    fn stick_mouse() -> StickMouse {
        StickMouse {
            on: true,
            ..StickMouse::default()
        }
    }

    const DISPLAY: (u32, u32) = (1280, 720);

    #[test]
    fn r3_is_the_left_mouse_button_while_the_cursor_may_show() {
        let pressed = || {
            let mut state = holding(0);
            state.pads[1].buttons = R3;
            state.mouse.buttons = 0b110;
            state
        };
        let mut stick = stick_mouse();
        let mut state = pressed();
        assert_eq!(stick.frame(&mut state, true, (10, 10), DISPLAY), None);
        // In place of the mouse's own buttons, and BUTTON still sees it.
        assert_eq!(state.mouse.buttons, 1);
        assert_eq!(state.pads[1].buttons, R3);
        assert!(!stick.holds_back(1, Tap::Buttons(1, R3)));
        let mut state = pressed();
        stick.frame(&mut state, false, (10, 10), DISPLAY);
        assert_eq!(state.mouse.buttons, 0b110);
        let mut state = pressed();
        stick.on = false;
        stick.frame(&mut state, true, (10, 10), DISPLAY);
        assert_eq!(state.mouse.buttons, 0b110);
    }

    #[test]
    fn the_program_sees_the_stick_that_moves_the_pointer() {
        let mut stick = stick_mouse();
        let mut state = holding(0);
        state.pads[2].sticks[1] = (1.0, 0.0);
        state.pads[0].sticks[1] = (0.2, -0.9);
        // Each axis from the controller tilted furthest along it.
        let moved = stick.frame(&mut state, true, (10, 100), DISPLAY);
        assert_eq!(moved, Some((26, 86)));
        assert_eq!(state.pads[2].sticks[1], (1.0, 0.0));
        assert_eq!(stick.frame(&mut state, false, (10, 100), DISPLAY), None);
        stick.on = false;
        assert_eq!(stick.frame(&mut state, true, (10, 100), DISPLAY), None);
    }

    #[test]
    fn select_points_in_any_program_and_keeps_the_stick_and_r3_from_it() {
        let mut stick = stick_mouse();
        let tilted = |buttons| {
            let mut state = holding(buttons | 1 << 1);
            state.pads[0].sticks = [(0.5, 0.0), (0.0, -1.0)];
            state
        };
        stick.select.event(&pad_event(Button::Back, true));
        assert!(stick.select_points());
        let mut state = tilted(R3);
        assert_eq!(
            stick.frame(&mut state, false, (10, 100), DISPLAY),
            Some((10, 84))
        );
        assert_eq!(state.pads[0].sticks, [(0.5, 0.0), (0.0, 0.0)]);
        assert_eq!(state.pads[0].buttons, 1 << 1);
        assert_eq!(state.mouse.buttons, 1);
        assert!(stick.holds_back(0, Tap::Buttons(0, R3)));
        assert!(!stick.holds_back(0, Tap::Buttons(0, 1 << 1)));
        // Let go of Select first: the stick and R3 stay the pointer's until
        // they are released.
        stick.select.event(&pad_event(Button::Back, false));
        assert!(!stick.select_points());
        let mut state = tilted(R3);
        assert_eq!(stick.frame(&mut state, false, (10, 84), DISPLAY), None);
        assert_eq!(state.pads[0].sticks[1], (0.0, 0.0));
        assert_eq!(state.pads[0].buttons, 1 << 1);
        assert!(stick.holds_back(0, Tap::Buttons(0, R3)));
        let mut state = holding(0);
        stick.frame(&mut state, false, (10, 84), DISPLAY);
        let mut state = tilted(R3);
        stick.frame(&mut state, false, (10, 84), DISPLAY);
        assert_eq!(state.pads[0].sticks[1], (0.0, -1.0));
        assert_eq!(state.pads[0].buttons, R3 | 1 << 1);
        assert!(!stick.holds_back(0, Tap::Buttons(0, R3)));
    }

    #[test]
    fn select_opens_the_projects_only_when_pressed_alone() {
        let mut stick = stick_mouse();
        let select =
            |stick: &mut StickMouse, down| stick.select.event(&pad_event(Button::Back, down));
        select(&mut stick, true);
        stick.select.arm();
        select(&mut stick, false);
        assert!(stick.select.alone());
        assert!(!stick.select.alone());
        // Not with another button pressed meanwhile...
        select(&mut stick, true);
        stick.select.arm();
        stick.select.event(&pad_event(Button::South, true));
        select(&mut stick, false);
        assert!(!stick.select.alone());
        // ...or the pointer moved...
        select(&mut stick, true);
        stick.select.arm();
        let mut state = holding(0);
        state.pads[0].sticks[1] = (1.0, 0.0);
        stick.frame(&mut state, false, (0, 0), DISPLAY);
        select(&mut stick, false);
        assert!(!stick.select.alone());
        // ...or a press that was not one to open them.
        select(&mut stick, true);
        select(&mut stick, false);
        assert!(!stick.select.alone());
        // A stick tilted too little to move the pointer does not count.
        select(&mut stick, true);
        stick.select.arm();
        let mut state = holding(0);
        state.pads[0].sticks[1] = (0.2, 0.0);
        stick.frame(&mut state, false, (0, 0), DISPLAY);
        select(&mut stick, false);
        assert!(stick.select.alone());
    }
}

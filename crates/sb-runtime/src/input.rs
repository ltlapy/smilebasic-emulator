//! Controllers, mouse, touch and keyboard: what the frontend reports, read
//! once a frame, and the commands that read it: BUTTON, BREPEAT, STICK,
//! CONTROLLER, XCTRLSTYLE, ACCEL, GYROA, GYROV, GYROSYNC, MOUSE, MBUTTON,
//! TOUCH, KEYBOARD, KEY and PUSHKEY.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{Value, new_str};

use crate::line::codes;
use crate::runtime::{Command, Runtime, checking, keys, no_form, opt_int};

/// Controllers 1 to 4; controller 0 combines them.
pub const PADS: usize = 4;
/// The buttons a controller reports, by BUTTON's ids: #B_RUP to #B_RSTICK.
pub const BUTTONS: usize = 14;
/// The A and B buttons' ids (#B_RRIGHT and #B_RDOWN).
pub const A: u32 = 3;
pub const B: u32 = 1;
/// #B_RANY, #B_LANY and #B_ANY come after the buttons themselves.
const BUTTON_IDS: usize = 17;
const RANY: u32 = 0b1111;
const LANY: u32 = 0b1111 << 4;
/// Every button but the stick pushes.
const ANY: u32 = (1 << 12) - 1;
/// Touches TOUCH can ask for; only the mouse's left button touches.
const TOUCHES: i32 = 10;
/// The most characters KEY keeps; KEY ignores a longer string, while
/// PUSHKEY's is String is too long (checked on hardware).
const KEY_LENGTH: usize = 256;
const PUSHKEY_LENGTH: usize = 63;
/// The highest code PUSHKEY takes.
const PUSHKEY_CODES: u32 = 0x10FFF;
/// BREPEAT's start and interval for buttons 0 to 7 and for the rest, as
/// SmileBASIC 4 starts.
const REPEAT_PADS: (u32, u32) = (20, 3);
const REPEAT_OTHERS: (u32, u32) = (15, 4);

/// How far a stick goes before it counts as a direction for [`PAD_KEYS`].
const PAD_KEY_STICK: f64 = 0.3;
/// A held button's key repeats after this many frames, then every so many.
const PAD_KEY_REPEAT: (u32, u32) = (20, 3);
/// A keyboard key held for a program, the prompt or the editor types again
/// this many frames after the press, or after any other keyboard key went
/// down or up, then every so many.
const KEY_REPEAT: (u32, u32) = (19, 3);
/// The keys that take over the repeat when they go down, by USB usage ID:
/// Shift, Ctrl, Alt and the Windows keys are not among them.
const REPEAT_KEYS: std::ops::RangeInclusive<u8> = 0x04..=0x94;
/// #B_L1 and #B_R1, either of which is Ctrl for [`PAD_KEYS`] and for typing.
const PAD_KEY_CTRL: u32 = 0b11 << 8;
/// The keyboard's Ctrl among [`State::modifiers`].
const MODIFIER_CTRL: u32 = 1 << 2;
/// Left and Right Shift's USB usage IDs.
const SHIFT_KEYS: [u8; 2] = [0xE1, 0xE5];
/// Left and Right Ctrl's.
const CTRL_KEYS: [u8; 2] = [0xE0, 0xE4];
/// Left and Right Alt's.
const LEFT_ALT: u8 = 0xE2;
const RIGHT_ALT: u8 = 0xE6;

/// Whether the key with this USB usage ID is down among `keys`, one bit each.
fn key_held(keys: &[u64; 4], usage: u8) -> bool {
    keys[usize::from(usage / 64)] >> (usage % 64) & 1 != 0
}

/// Whether either Shift is down among `keys`.
fn shift_held(keys: &[u64; 4]) -> bool {
    SHIFT_KEYS.iter().any(|&k| key_held(keys, k))
}

/// What Alt with a key types, by the key's USB usage ID: characters of
/// SmileBASIC 4's own, the same for either Alt and with Shift or without.
/// Keys not here type nothing with Alt.
const ALT_KEYS: [(u8, u16); 51] = [
    (0x04, 0xE2EC), // A
    (0x05, 0xE210),
    (0x06, 0xE20A),
    (0x07, 0xE2EE),
    (0x08, 0xE2F2),
    (0x09, 0xE2EF),
    (0x0A, 0xE2F4),
    (0x0B, 0xE205),
    (0x0C, 0xE2E3),
    (0x0D, 0xE20E),
    (0x0E, 0xE206),
    (0x0F, 0xE2E8),
    (0x10, 0xE214),
    (0x11, 0xE211),
    (0x12, 0xE2E9),
    (0x13, 0xE21B),
    (0x14, 0xE2F0),
    (0x15, 0xE2F3),
    (0x16, 0xE2ED),
    (0x17, 0xE2E0),
    (0x18, 0xE2E2),
    (0x19, 0x2606), // V
    (0x1A, 0xE2F1),
    (0x1B, 0xE208),
    (0x1C, 0xE2E1),
    (0x1D, 0xE207), // Z
    (0x1E, 0xE2EB), // 1
    (0x1F, 0xE2EA), // 2; 3 to 9 type nothing
    (0x27, 0xE29C), // 0
    (0x2D, 0xE29D), // -
    (0x2E, 0xE29E), // =
    (0x2F, 0xE2FF), // [
    (0x33, 0xE203), // ;
    (0x34, 0xE297), // '
    (0x54, 0xE215), // keypad /
    (0x55, 0xE216), // keypad *
    (0x56, 0xE217), // keypad -
    (0x57, 0xE28F), // keypad +
    (0x59, 0xE29A), // keypad 1
    (0x5A, 0xE290),
    (0x5B, 0xE29B),
    (0x5C, 0xE292),
    (0x5D, 0xE293),
    (0x5E, 0xE294),
    (0x5F, 0xE298),
    (0x60, 0xE291),
    (0x61, 0xE299), // keypad 9
    (0x62, 0xE295), // keypad 0
    (0x63, 0xE296), // keypad .
    (0x87, 0xE20B), // International 1 (Ro)
    (0x89, 0xE29F), // International 3 (Yen)
];

/// What Alt with the key of this USB usage ID types, if anything.
pub fn alt_key_code(usage: u8) -> Option<u16> {
    (ALT_KEYS.iter())
        .find(|&&(key, _)| key == usage)
        .map(|&(_, code)| code)
}

/// What a keyboard key types, by the modifiers held as it types: each
/// repeat of a held key types what the modifiers held then give, as its
/// press did. 0: nothing, or nothing of the runtime's own, as for text
/// outside ASCII, which comes with the computer's repeats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyText {
    /// With no modifier.
    pub plain: u16,
    pub shift: u16,
    /// With Left Alt: [`alt_key_code`]'s.
    pub alt: u16,
    /// With Right Alt alone: the layout's AltGr character, or else what
    /// Left Alt gives.
    pub right_alt: u16,
}

impl KeyText {
    /// What the key types with the keys of `keys` held.
    fn code(&self, keys: &[u64; 4]) -> u16 {
        if key_held(keys, LEFT_ALT) {
            self.alt
        } else if key_held(keys, RIGHT_ALT) {
            self.right_alt
        } else if shift_held(keys) {
            self.shift
        } else {
            self.plain
        }
    }
}

/// What a row of [`PAD_KEYS`] looks at.
#[derive(Clone, Copy)]
enum PadInput {
    /// A button's id.
    Button(u32),
    /// The left stick pushed that way.
    StickUp,
    StickDown,
    StickLeft,
    StickRight,
}

/// The keys the controllers type while a program runs: what SmileBASIC 4's
/// assignment table gives for any mode. The first row that holds gives the
/// frame's key, so Ctrl's rows come first. `true`: only with Ctrl.
const PAD_KEYS: [(PadInput, bool, u32); 17] = {
    use PadInput::{Button, StickDown, StickLeft, StickRight, StickUp};
    [
        (Button(1), true, codes::PASTE),
        (Button(6), true, codes::HOME),
        (Button(7), true, codes::END),
        (StickUp, true, codes::FILE_TOP),
        (StickDown, true, codes::FILE_END),
        (Button(2), true, codes::DELETE_LINE),
        (Button(1), false, codes::COPY),
        (StickUp, false, codes::PAGE_UP),
        (Button(2), false, codes::BACKSPACE),
        (StickDown, false, codes::PAGE_DOWN),
        (StickLeft, false, codes::HOME),
        (StickRight, false, codes::END),
        (Button(4), false, codes::UP),
        (Button(5), false, codes::DOWN),
        (Button(3), false, codes::ENTER),
        (Button(6), false, codes::LEFT),
        (Button(7), false, codes::RIGHT),
    ]
};
/// The buttons [`PAD_KEYS`] looks at, to go past it when none is down.
const PAD_KEY_BUTTONS: u32 = 0b1111_1110;

/// The key a held controller input repeats, and how long it has been held.
#[derive(Clone, Copy, Default)]
struct PadKeyRepeat {
    code: u32,
    frames: u32,
    repeating: bool,
    /// Nothing more is typed until all is let go.
    spent: bool,
}

/// The keyboard key that types again while it is held, and what it typed.
#[derive(Clone, Debug, Default)]
struct KeyRepeat {
    /// Its USB usage ID; 0 for none.
    usage: u8,
    /// What it typed at the press, which its repeats type again when
    /// [`KeyRepeat::text`] has nothing: keys such as Enter.
    codes: Vec<u16>,
    /// What it types by the modifiers held.
    text: Option<KeyText>,
    /// Frames since it went down, or since any keyboard key last went down
    /// or up, counting the frame it did.
    frames: u32,
    /// Whether it types into the editor, as known from its first frame.
    editor: Option<bool>,
    /// The keyboard keys held on the last frame.
    keys: Option<[u64; 4]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pad {
    pub connected: bool,
    /// A gamepad is there, not only the keyboard: the first controller
    /// is connected either way.
    pub gamepad: bool,
    /// One bit per button id below [`BUTTONS`].
    pub buttons: u32,
    /// The left and right sticks, each -1 to 1, Y down.
    pub sticks: [(f64, f64); 2],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mouse {
    /// On the screen, in its pixels.
    pub x: i32,
    pub y: i32,
    /// On the whole display (1280x720).
    pub display: (i32, i32),
    /// Whether the pointer is over the screen.
    pub on_screen: bool,
    /// The wheel's total movement, up positive.
    pub wheel: i32,
    /// Left, right, middle, button 4 and button 5, as MBUTTON's bits.
    pub buttons: u32,
}

/// Everything the program can read, as one frame sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub pads: [Pad; PADS],
    pub mouse: Mouse,
    /// Keys held, by USB usage id.
    pub keys: [u64; 4],
    /// Windows key, Shift, Ctrl and Alt, as KEYBOARD's bits.
    pub modifiers: u32,
    /// Buttons that are down only because keys press them. They type
    /// nothing: the keys themselves do.
    pub key_pressed_buttons: u32,
}

impl Default for State {
    fn default() -> Self {
        let mut pads = [Pad::default(); PADS];
        // The first controller is the keyboard's, always there.
        pads[0].connected = true;
        State {
            pads,
            mouse: Mouse::default(),
            keys: [0; 4],
            modifiers: 0,
            key_pressed_buttons: 0,
        }
    }
}

impl State {
    pub fn set_key(&mut self, usage: u8, down: bool) {
        let (word, bit) = (usize::from(usage / 64), usage % 64);
        if down {
            self.keys[word] |= 1 << bit;
        } else {
            self.keys[word] &= !(1 << bit);
        }
    }

    pub(crate) fn key(&self, usage: u8) -> bool {
        key_held(&self.keys, usage)
    }

    /// Whether keyboard keys type no text now: with either Ctrl held, or
    /// the controller's L or R, which count as Ctrl. Enter, Backspace, Tab,
    /// Delete and Esc type nothing then either, while the arrow keys, Home
    /// and End still type (see [`types_when_blocked`]). Buttons the keys
    /// press do not count.
    pub fn keys_type_nothing(&self) -> bool {
        let buttons = self.buttons(0) & !self.key_pressed_buttons;
        buttons & PAD_KEY_CTRL != 0
            || self.modifiers & MODIFIER_CTRL != 0
            || CTRL_KEYS.iter().any(|&k| self.key(k))
    }

    /// A controller's buttons; controller 0 has those of all of them.
    fn buttons(&self, pad: usize) -> u32 {
        match pad {
            0 => self.pads.iter().fold(0, |b, p| b | p.buttons),
            n => self.pads[n - 1].buttons,
        }
    }

    /// The default controller's stick: each axis on its own, the value
    /// furthest from 0 among the controllers, the earlier controller's on
    /// a tie. X and Y can come from different controllers.
    fn stick(&self, stick: usize) -> (f64, f64) {
        let further = |cur: f64, new: f64| if cur.abs() < new.abs() { new } else { cur };
        (self.pads.iter().map(|p| p.sticks[stick])).fold((0.0, 0.0), |(x, y), (nx, ny)| {
            (further(x, nx), further(y, ny))
        })
    }

    /// The first of [`PAD_KEYS`] that holds: the key the controllers ask
    /// for now, or 0.
    fn pad_key(&self) -> u32 {
        let buttons = self.buttons(0) & !self.key_pressed_buttons;
        let (x, y) = self.stick(0);
        let stick_moved = x.abs() > PAD_KEY_STICK || y.abs() > PAD_KEY_STICK;
        if buttons & PAD_KEY_BUTTONS == 0 && !stick_moved {
            return 0;
        }
        let ctrl = buttons & PAD_KEY_CTRL != 0 || self.modifiers & MODIFIER_CTRL != 0;
        for (input, needs_ctrl, code) in PAD_KEYS {
            let holds = match input {
                PadInput::Button(id) => buttons >> id & 1 != 0,
                PadInput::StickUp => y < -PAD_KEY_STICK,
                PadInput::StickDown => y > PAD_KEY_STICK,
                PadInput::StickLeft => x < -PAD_KEY_STICK,
                PadInput::StickRight => x > PAD_KEY_STICK,
            };
            if holds && (ctrl || !needs_ctrl) {
                return code;
            }
        }
        0
    }
}

pub struct Input {
    /// What the frontend reports now; [`Input::latch`] makes it the frame's.
    pub live: State,
    now: State,
    before: State,
    /// Frames each button of controllers 0 to 4 has been held, and
    /// #B_RANY, #B_LANY and #B_ANY have been on.
    held: [[u32; BUTTON_IDS]; PADS + 1],
    /// BREPEAT's start and interval for each button id.
    repeat: [(u32, u32); BUTTON_IDS],
    /// Frames the left button has been held, anywhere and on the screen.
    touch: (u32, u32),
    /// XCTRLSTYLE's maximum number of controllers and operation style.
    pub(crate) style: (i32, i32),
    /// XCTRLSTYLE's IR Motion Camera flag.
    pub(crate) ir_allowed: bool,
    /// XCTRLSTYLE's Toy-Con types under operation style 100: left and
    /// right of controller 1, then of controller 2.
    pub(crate) toycons: [i32; 4],
    /// IRSTATE of controllers 1 and 2.
    pub(crate) ir_state: [i32; 2],
    /// What VIBRATE last asked of each controller's left and right side,
    /// and the frames left before a pattern ends (`None`: until stopped).
    pub(crate) vibration: [[(i32, Option<u32>); 2]; PADS],
    /// KEY's contents for F1 to F5.
    pub function_keys: [Vec<u16>; 5],
    /// The key the controllers are holding down.
    pad_key_repeat: PadKeyRepeat,
    /// The keyboard key whose typing repeats.
    key_repeat: KeyRepeat,
    /// The keyboard and the controllers while the editor has the keys.
    /// [`Input::live`] has nothing held then, as programs see nothing,
    /// but a key held in the editor repeats too.
    pub editor_held: State,
    /// BUTTON or STICK has been called; see [`Input::take_buttons_read`].
    buttons_read: bool,
    /// What the program has read of the keyboard; see
    /// [`Input::take_keys_read`].
    pub(crate) keys_read: KeysRead,
    /// The program has read the controllers, the keyboard, the mouse or
    /// the touch screen since this was last cleared, for the debugger.
    pub(crate) polled: bool,
}

/// The keys a program has read for itself: those `KEYBOARD` asked about,
/// and with `INKEY$` every key, as any of them may type.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct KeysRead {
    typed: bool,
    keys: [u64; 4],
}

impl KeysRead {
    pub(crate) fn typed(&mut self) {
        self.typed = true;
    }

    pub(crate) fn key(&mut self, usage: u8) {
        self.keys[usize::from(usage >> 6)] |= 1 << (usage & 63);
    }

    /// Whether the program has read the key with this USB usage ID.
    pub fn covers(&self, usage: u8) -> bool {
        self.typed || self.keys[usize::from(usage >> 6)] >> (usage & 63) & 1 != 0
    }
}

impl Default for Input {
    fn default() -> Self {
        Input {
            live: State::default(),
            now: State::default(),
            before: State::default(),
            held: [[0; BUTTON_IDS]; PADS + 1],
            repeat: std::array::from_fn(|id| match id {
                0..=7 => REPEAT_PADS,
                _ => REPEAT_OTHERS,
            }),
            touch: (0, 0),
            style: (1, 0),
            ir_allowed: false,
            toycons: [0; 4],
            ir_state: [0; 2],
            vibration: Default::default(),
            function_keys: Default::default(),
            pad_key_repeat: PadKeyRepeat::default(),
            key_repeat: KeyRepeat::default(),
            editor_held: State::default(),
            buttons_read: false,
            keys_read: KeysRead::default(),
            polled: false,
        }
    }
}

impl Input {
    /// Starts a frame: what the frontend reports is what the program sees
    /// until the next frame.
    pub fn latch(&mut self) {
        self.before = self.now;
        self.now = self.live;
        for (pad, held) in self.held.iter_mut().enumerate() {
            let buttons = with_any(self.now.buttons(pad));
            for (id, frames) in held.iter_mut().enumerate() {
                *frames = if buttons >> id & 1 != 0 {
                    *frames + 1
                } else {
                    0
                };
            }
        }
        for (state, left) in self.vibration.iter_mut().flatten() {
            if let Some(n) = left {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    (*state, *left) = (0, None);
                }
            }
        }
        let m = &self.now.mouse;
        let down = m.buttons & 1 != 0;
        self.touch = (
            if down { self.touch.0 + 1 } else { 0 },
            if down && m.on_screen {
                self.touch.1 + 1
            } else {
                0
            },
        );
    }

    /// Forgets what was pressed and for how long, so that buttons held
    /// when a program ends give no presses or repeats to the next one. A
    /// keyboard key held goes on repeating: from a program into DIRECT
    /// mode, and across RUN.
    pub fn forget(&mut self) {
        self.now = self.live;
        self.before = self.live;
        self.held = [[0; BUTTON_IDS]; PADS + 1];
        self.touch = (0, 0);
        // What is held goes on counting as held, and types nothing more
        // until let go.
        self.spend_pad_key();
    }

    /// A keyboard key went down: what [`Runtime::type_held`] types for it
    /// next, for a program, the prompt or the editor, repeats while it
    /// stays down, counted in the machine's frames rather than by the
    /// computer's own key repeat, and the key held before stops repeating.
    /// Shift, Ctrl, Alt and the Windows keys leave the held key repeating;
    /// like any key going down or up, they start its count again.
    pub fn key_down(&mut self, usage: u8) {
        if REPEAT_KEYS.contains(&usage) {
            self.key_repeat = KeyRepeat {
                usage,
                ..KeyRepeat::default()
            };
        }
    }

    /// A code of its own, such as Enter, that the key that just went down
    /// types again as it repeats: what [`Runtime::type_held`] typed, or
    /// what it would have typed but for Ctrl, L or R held.
    pub fn key_code(&mut self, code: u16) {
        let r = &mut self.key_repeat;
        // Only what came with the press, not what a later frame brings.
        if r.usage != 0 && r.frames == 0 {
            r.codes.push(code);
        }
    }

    /// What the key that just went down types by the modifiers held, for
    /// its repeats, in place of what it typed at the press.
    pub fn key_text(&mut self, text: KeyText) {
        let r = &mut self.key_repeat;
        if r.usage != 0 && r.frames == 0 {
            r.text = Some(text);
        }
    }

    /// Whether the held keyboard key types again this frame, once a frame:
    /// never once it is let go. `held`: it is still down; `keys`: the
    /// keyboard keys held now.
    fn repeat_due(&mut self, held: bool, keys: &[u64; 4]) -> bool {
        let r = &mut self.key_repeat;
        if !held {
            *r = KeyRepeat::default();
            return false;
        }
        r.frames += 1;
        // Any key going down or up, a modifier or another key let go,
        // starts the count again.
        if r.keys.replace(*keys).is_some_and(|was| was != *keys) {
            r.frames = 1;
        }
        let (start, interval) = KEY_REPEAT;
        r.frames > start && (r.frames - 1 - start).is_multiple_of(interval)
    }

    /// What the controllers hold now types nothing more until let go.
    pub(crate) fn spend_pad_key(&mut self) {
        self.pad_key_repeat = PadKeyRepeat {
            code: self.now.pad_key(),
            spent: true,
            ..PadKeyRepeat::default()
        };
    }

    /// The key the controllers type this frame, if any, after
    /// [`Input::latch`]: when a button or the stick goes down, and then as
    /// it repeats. Another input going down meanwhile types nothing while
    /// both are held; once it is released, the first goes on repeating
    /// where it was.
    pub(crate) fn pad_key(&mut self) -> Option<u32> {
        let code = self.now.pad_key();
        let r = &mut self.pad_key_repeat;
        if code == 0 {
            *r = PadKeyRepeat::default();
            return None;
        }
        if r.code == 0 && !r.spent {
            r.code = code;
            r.frames = 0;
            return Some(code);
        }
        if r.spent || r.code != code {
            return None;
        }
        r.frames += 1;
        let (start, interval) = PAD_KEY_REPEAT;
        if r.frames < if r.repeating { interval } else { start } {
            return None;
        }
        r.frames = 0;
        r.repeating = true;
        Some(code)
    }

    /// Keyboard keys held now count as held before too, so that the key
    /// that closed a dialog is no press for KEYBOARD afterwards, and
    /// controller buttons are as they were, so that the button that closed
    /// it is neither a press nor a release for BUTTON.
    pub(crate) fn hide_presses(&mut self) {
        for (before, now) in self.before.keys.iter_mut().zip(self.now.keys) {
            *before |= now;
        }
        for (before, now) in self.before.pads.iter_mut().zip(&self.now.pads) {
            before.buttons = now.buttons;
        }
    }

    /// Lets a frame's key from the controllers go without typing it: its
    /// repeats come on as the button stays down.
    pub(crate) fn drop_pad_key(&mut self) {
        self.pad_key();
    }

    /// Whether the program has read buttons or sticks since the last call,
    /// for the frontend to show where they are on the keyboard.
    pub fn take_buttons_read(&mut self) -> bool {
        std::mem::take(&mut self.buttons_read)
    }

    /// What the program has read of the keyboard since the last call, for
    /// the frontend to leave those keys out of what presses buttons.
    pub fn take_keys_read(&mut self) -> KeysRead {
        std::mem::take(&mut self.keys_read)
    }

    /// Buttons of any controller that went down this frame, with
    /// #B_RANY, #B_LANY and #B_ANY.
    pub(crate) fn pressed(&self) -> u32 {
        self.buttons(0, 2)
    }

    pub(crate) fn released(&self) -> u32 {
        self.buttons(0, 3)
    }

    /// Whether the touch screen was touched this frame.
    pub(crate) fn touched(&self) -> bool {
        self.touch.1 == 1
    }

    /// The bits of controller `pad`'s buttons that meet BUTTON's
    /// condition `function`, with #B_RANY, #B_LANY and #B_ANY. A press or
    /// a release has those only when they go on or off themselves: with L
    /// held, pressing or releasing A gives A and #B_RANY, as #B_ANY stays
    /// on for L. A wait for buttons in DIALOG answers the same way. A
    /// repeat has them as they go on and then on their own clocks, by
    /// their own BREPEAT, counted from the frame each went on: with L
    /// held, A going down or repeating gives A and #B_RANY, and #B_ANY
    /// goes on repeating as it did for L.
    fn buttons(&self, pad: usize, function: i32) -> u32 {
        let (now, before) = (self.now.buttons(pad), self.before.buttons(pad));
        match function {
            0 => with_any(now),
            2 => with_any(now) & !with_any(before),
            3 => with_any(before) & !with_any(now),
            _ => {
                let mut bits = with_any(now) & !with_any(before);
                for (id, &held) in self.held[pad].iter().enumerate() {
                    let (start, interval) = self.repeat[id];
                    // The first repeat comes `start` frames after the press,
                    // the next ones `interval` frames apart, not counting
                    // the frame that repeats (checked on hardware: 20,3
                    // repeats at 20, 24, 28...).
                    if held > start && (held - 1 - start) % (interval + 1) == 0 {
                        bits |= 1 << id;
                    }
                }
                bits
            }
        }
    }
}

/// `bits` with #B_RANY, #B_LANY and #B_ANY for them.
fn with_any(bits: u32) -> u32 {
    let any = |mask: u32| u32::from(bits & mask != 0);
    bits | any(RANY) << 14 | any(LANY) << 15 | any(ANY) << 16
}

fn int(v: &Value) -> Result<i32> {
    v.to_int()
}

pub(crate) fn out_of_range(what: &str, n: i32) -> Error {
    Error::with_detail(ErrorKind::OutOfRange, format!("{what} {n}"))
}

pub(crate) fn in_range(what: &str, v: &Value, range: std::ops::RangeInclusive<i32>) -> Result<i32> {
    checking(v, || {
        let n = int(v)?;
        if range.contains(&n) {
            Ok(n)
        } else {
            Err(out_of_range(what, n))
        }
    })
}

/// An omittable argument in `range`, `default` when left out.
fn opt_in_range(
    what: &str,
    v: Option<&Value>,
    range: std::ops::RangeInclusive<i32>,
    default: i32,
) -> Result<i32> {
    match v {
        None | Some(Value::Default) => Ok(default),
        Some(v) => in_range(what, v, range),
    }
}

/// Whether a key that types `code` of its own, such as an arrow key,
/// still types it while [`State::keys_type_nothing`]: Enter, Backspace,
/// Tab, Delete and Esc do not, while Home and End do. UNVERIFIED: that
/// Insert, Page Up and Page Down still type.
pub fn types_when_blocked(code: u16) -> bool {
    !matches!(
        code,
        keys::ENTER | keys::BACKSPACE | keys::TAB | keys::DELETE | keys::ESCAPE
    )
}

fn pad_id(v: &Value) -> Result<usize> {
    in_range("controller", v, 0..=PADS as i32).map(|n| n as usize)
}

/// ACCEL and the gyroscope take controllers 1 to 4 and sensors 0 and 1.
fn sensor_args(args: &[Value]) -> Result<()> {
    in_range("controller", &args[0], 1..=PADS as i32)?;
    opt_in_range("sensor", args.get(1), 0..=1, 0)?;
    Ok(())
}

fn condition(v: Option<&Value>) -> Result<i32> {
    opt_in_range("function", v, 0..=3, 0)
}

pub(crate) fn reals(values: &[f64]) -> Vec<Value> {
    values.iter().map(|&x| Value::Real(x)).collect()
}

pub(crate) fn ints(values: &[i32]) -> Vec<Value> {
    values.iter().map(|&n| Value::Int(n)).collect()
}

fn flag(b: bool) -> Value {
    Value::Int(i32::from(b))
}

fn text(v: &Value) -> Result<Vec<u16>> {
    Ok(v.clone().into_str()?.borrow().clone())
}

impl Runtime {
    /// Types `code` for the keyboard key that last went down with
    /// [`Input::key_down`], which types it again while it is held.
    pub fn type_held(&mut self, code: u16) {
        self.key_press(code);
        self.input.key_code(code);
    }

    /// Types again what the held keyboard key typed, once a frame before
    /// the frame's keys go to the program, the prompt or the editor, made
    /// again from the modifiers held now. `beside` is the other
    /// interpreter's input, which has the keys while it has the focus;
    /// `editor`: the editor has the keys.
    pub(crate) fn repeat_held_key(&mut self, beside: Option<&State>, editor: bool) {
        let r = &mut self.input.key_repeat;
        let usage = r.usage;
        if usage == 0 {
            return;
        }
        // The key types only where it went down: the editor taking the
        // keys or giving them up stops it.
        let same_place = *r.editor.get_or_insert(editor) == editor;
        let (keys, blocked) = if editor {
            let held = &self.input.editor_held;
            (held.keys, held.keys_type_nothing())
        } else {
            let live = &self.input.live;
            let keys = match beside {
                Some(s) => std::array::from_fn(|i| live.keys[i] | s.keys[i]),
                None => live.keys,
            };
            let blocked = live.keys_type_nothing() || beside.is_some_and(State::keys_type_nothing);
            (keys, blocked)
        };
        // A DIALOG takes no repeats, and the key stops repeating.
        let held = key_held(&keys, usage) && same_place;
        let held = held && self.dialog.is_none() && !self.dialog_beside;
        if !self.input.repeat_due(held, &keys) {
            return;
        }
        let r = &self.input.key_repeat;
        match r.text {
            Some(text) => {
                let code = text.code(&keys);
                if code != 0 && !blocked {
                    self.key_press(code);
                }
            }
            // Allocated only on the frames that repeat.
            None => {
                for code in r.codes.clone() {
                    if !blocked || types_when_blocked(code) {
                        self.key_press(code);
                    }
                }
            }
        }
    }

    pub(crate) fn input_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        let input = &mut self.input;
        input.polled = true;
        if matches!(command, Command::Button | Command::Stick) {
            input.buttons_read = true;
        }
        match (command, want, args.len()) {
            (Command::Button, Want::Value, 1..=3) => {
                let pad = pad_id(&args[0])?;
                let button = opt_in_range("button", args.get(1), -1..=16, -1)?;
                let bits = input.buttons(pad, condition(args.get(2))?);
                Ok(vec![Value::Int(match button {
                    -1 => bits as i32,
                    id => (bits >> id & 1) as i32,
                })])
            }
            (Command::Brepeat, Want::Outs(0), 3) => {
                let id = in_range("button", &args[0], 0..=16)? as usize;
                // A start of 0 is taken as 1.
                let start = in_range("start", &args[1], 0..=i32::MAX)?.max(1);
                let interval = in_range("interval", &args[2], 0..=i32::MAX)?;
                input.repeat[id] = (start as u32, interval as u32);
                Ok(vec![])
            }
            (Command::Brepeat, Want::Outs(2), 1) => {
                let id = in_range("button", &args[0], 0..=16)? as usize;
                let (start, interval) = input.repeat[id];
                Ok(ints(&[start as i32, interval as i32]))
            }
            (Command::Stick, Want::Outs(2), 1..=2) => {
                let pad = pad_id(&args[0])?;
                let stick = opt_in_range("stick", args.get(1), 0..=1, 0)? as usize;
                let (x, y) = match pad {
                    0 => input.now.stick(stick),
                    n => input.now.pads[n - 1].sticks[stick],
                };
                Ok(reals(&[x, y]))
            }
            (Command::Controller, Want::Value | Want::Outs(1 | 3 | 5), 1) => {
                let pad = pad_id(&args[0])?;
                // The default controller is a Pro Controller without colors.
                // Gamepads count as Pro Controllers, and the keyboard alone
                // as handheld.
                let kind = match pad {
                    0 => 2,
                    1 if !input.now.pads[0].gamepad => 1,
                    n if input.now.pads[n - 1].connected => 2,
                    _ => 0,
                };
                // UNVERIFIED: the colors of handheld.
                let colors = match (pad, kind) {
                    (0, _) | (_, 0) => [0, 0, 0, 0],
                    (_, 1) => [0xFF82_8282_u32, 0xFF0F_0F0F, 0xFF00_0000, 0xFF00_0000],
                    _ => [0xFF23_2323_u32, 0xFFA0_A0A0, 0xFF00_0000, 0xFF00_0000],
                };
                let mut out = vec![Value::Int(kind)];
                out.extend(colors.map(|c| Value::Int(c as i32)));
                out.truncate(want.count());
                Ok(out)
            }
            (Command::Xctrlstyle, Want::Outs(0), 1) => {
                in_range("style", &args[0], 0..=0)?;
                input.style = (1, 0);
                input.ir_allowed = false;
                input.toycons = [0; 4];
                self.result = 1;
                Ok(vec![])
            }
            (Command::Xctrlstyle, Want::Outs(0), 2..=7) => {
                let style = int(&args[1])?;
                let most = match style {
                    0 | 1 | 100 => 1..=2,
                    2 | 3 => 2..=4,
                    n => return Err(out_of_range("style", n)),
                };
                let max = in_range("controllers", &args[0], most)?;
                let flags = (args[2..].iter())
                    .map(|a| Ok(opt_int(a)?.unwrap_or(0)))
                    .collect::<Result<Vec<_>>>()?;
                // The connection screen has nothing to ask: all controllers
                // there are are connected.
                input.style = (max, style);
                input.ir_allowed = style != 100 && flags.get(1).is_some_and(|&f| f != 0);
                input.toycons = [0; 4];
                if style == 100 {
                    // Two types for controller 1 or four for both, then
                    // the connection screen's flag.
                    let n = if flags.len() >= 4 {
                        4
                    } else {
                        flags.len().min(2)
                    };
                    input.toycons[..n].copy_from_slice(&flags[..n]);
                }
                self.result = 1;
                Ok(vec![])
            }
            (Command::Xctrlstyle, Want::Outs(2), 0) => Ok(ints(&[input.style.0, input.style.1])),
            // No motion sensors: everything reads as still.
            (Command::Accel | Command::Gyroa | Command::Gyrov, Want::Outs(3), 1..=2) => {
                sensor_args(args)?;
                Ok(reals(&[0.0, 0.0, 0.0]))
            }
            (Command::Gyrosync, Want::Outs(0), 1..=2) => {
                sensor_args(args)?;
                Ok(vec![])
            }
            (Command::Mouse, Want::Outs(2 | 3), 0..=1) => {
                let lcd = opt_in_range("flag", args.first(), 0..=1, 0)? == 1;
                let m = &input.now.mouse;
                let (x, y) = if lcd { m.display } else { (m.x, m.y) };
                let mut out = ints(&[x, y, m.wheel]);
                out.truncate(want.count());
                Ok(out)
            }
            (Command::Mouse, Want::Outs(0), 2..=3) => {
                let (x, y) = (int(&args[0])?, int(&args[1])?);
                let lcd = opt_in_range("flag", args.get(2), 0..=1, 0)? == 1;
                // The pointer stays where it is; the program sees it moved
                // until it moves again.
                for m in [&mut input.live.mouse, &mut input.now.mouse] {
                    if lcd {
                        m.display = (x, y);
                    } else {
                        (m.x, m.y) = (x, y);
                    }
                }
                Ok(vec![])
            }
            (Command::Mbutton, Want::Value, 0..=2) => {
                let (now, before) = (input.now.mouse.buttons, input.before.mouse.buttons);
                if args.is_empty() {
                    return Ok(vec![Value::Int(now as i32)]);
                }
                let id = in_range("button", &args[0], 0..=4)?;
                let bits = match condition(args.get(1))? {
                    0 => now,
                    1 => 0,
                    2 => now & !before,
                    _ => !now & before,
                };
                Ok(vec![Value::Int((bits >> id & 1) as i32)])
            }
            (Command::Touch, Want::Outs(3 | 4), 0..=2) => {
                let id = opt_in_range("touch", args.first(), 0..=TOUCHES - 1, 0)?;
                let lcd = opt_in_range("flag", args.get(1), 0..=1, 0)? == 1;
                let m = &input.now.mouse;
                let time = match (id, lcd) {
                    (0, true) => input.touch.0,
                    (0, false) => input.touch.1,
                    _ => 0,
                };
                let (x, y) = if lcd { m.display } else { (m.x, m.y) };
                // UNVERIFIED: what a mouse's touch area should be.
                let mut out = ints(&[time as i32, x, y, 1]);
                out.truncate(want.count());
                Ok(out)
            }
            (Command::Keyboard, Want::Value | Want::Outs(2), 1..=2) => {
                let usage = in_range("scan code", &args[0], 0..=255)? as u8;
                input.keys_read.key(usage);
                let (now, before) = (input.now.key(usage), input.before.key(usage));
                let result = match condition(args.get(1))? {
                    0 => now,
                    1 => false,
                    2 => now && !before,
                    _ => !now && before,
                };
                let mut out = vec![flag(result), Value::Int(input.now.modifiers as i32)];
                out.truncate(want.count());
                Ok(out)
            }
            (Command::Key, Want::Outs(0), 2) => {
                let n = in_range("key", &args[0], 1..=5)?;
                let s = text(&args[1])?;
                if s.len() <= KEY_LENGTH {
                    input.function_keys[n as usize - 1] = s;
                }
                Ok(vec![])
            }
            (Command::Key, Want::Value, 1) => {
                let n = in_range("key", &args[0], 1..=5)?;
                let s = input.function_keys[n as usize - 1].clone();
                Ok(vec![Value::Str(new_str(s))])
            }
            // It holds the program until the keys have gone on, a frame.
            (Command::Pushkey, Want::Outs(0), 1) if self.frame_wait_over() => Ok(vec![]),
            (Command::Pushkey, Want::Outs(0), 1) => {
                self.pushkey(&args[0])?;
                self.hold_for_a_frame();
                Ok(vec![])
            }
            _ => Err(no_form()),
        }
    }

    /// PUSHKEY's keys: a number is one key's code, a character or one of
    /// [`crate::line::codes`].
    pub(crate) fn pushkey(&mut self, keys: &Value) -> Result<()> {
        if !matches!(keys, Value::Str(_)) {
            let code = keys.to_int()? as u32;
            if code > PUSHKEY_CODES {
                return Err(out_of_range("key code", code as i32));
            }
            self.push_code(code);
            return Ok(());
        }
        let s = match keys {
            Value::Str(s) => s.borrow(),
            _ => unreachable!("checked above"),
        };
        if s.len() > PUSHKEY_LENGTH {
            return Err(Error::with_detail(ErrorKind::StringTooLong, "PUSHKEY"));
        }
        for &c in s.iter() {
            self.push_code(u32::from(c));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(input: &mut Input, buttons: u32) {
        input.live.pads[0].buttons = buttons;
        input.latch();
    }

    #[test]
    fn controller_0s_sticks_take_each_axis_furthest_from_0() {
        let mut input = Input::default();
        input.live.pads[0].sticks = [(0.5, 0.1), (-0.25, 0.0)];
        input.live.pads[1].sticks = [(-0.2, -0.9), (0.25, 0.75)];
        input.latch();
        assert_eq!(input.now.stick(0), (0.5, -0.9));
        // On a tie, the earlier controller's.
        assert_eq!(input.now.stick(1), (-0.25, 0.75));
    }

    #[test]
    fn edges_and_any() {
        let mut input = Input::default();
        press(&mut input, 1 << 3);
        assert_eq!(input.buttons(1, 0), 1 << 3 | 1 << 14 | 1 << 16);
        assert_eq!(input.buttons(0, 2), input.buttons(1, 0));
        press(&mut input, 1 << 3);
        assert_eq!(input.buttons(1, 2), 0);
        press(&mut input, 0);
        assert_eq!(input.buttons(1, 3), 1 << 3 | 1 << 14 | 1 << 16);
    }

    #[test]
    fn a_held_button_keeps_any_from_a_press_and_a_release() {
        let (l, a) = (1 << 8, 1 << 3);
        let mut input = Input::default();
        press(&mut input, l);
        assert_eq!(input.buttons(1, 2), l | 1 << 16);
        press(&mut input, l | a);
        assert_eq!(input.buttons(1, 0), l | a | 1 << 14 | 1 << 16);
        assert_eq!(input.buttons(1, 2), a | 1 << 14);
        press(&mut input, l);
        assert_eq!(input.buttons(1, 3), a | 1 << 14);
        press(&mut input, 0);
        assert_eq!(input.buttons(1, 3), l | 1 << 16);
    }

    #[test]
    fn repeats() {
        let mut input = Input::default();
        input.repeat[4] = (3, 2);
        let fired: Vec<bool> = (0..8)
            .map(|_| {
                press(&mut input, 1 << 4);
                input.buttons(1, 1) & 1 << 4 != 0
            })
            .collect();
        assert_eq!(fired, [true, false, false, true, false, false, true, false]);
        // SmileBASIC 4's own 20,3 for A: frames 0, 20, 24, 28 of the hold.
        let mut input = Input::default();
        let fired: Vec<usize> = (0..30)
            .filter(|_| {
                press(&mut input, 1 << 3);
                input.buttons(1, 1) & 1 << 3 != 0
            })
            .collect();
        assert_eq!(fired, [0, 20, 24, 28]);
    }

    /// The frames of a hold, counted from 0, on which BUTTON's repeat
    /// condition gives something, and what it gives.
    fn repeat_rows(input: &mut Input, frames: u32, held: impl Fn(u32) -> u32) -> Vec<(u32, u32)> {
        (0..frames)
            .filter_map(|f| {
                press(input, held(f));
                let bits = input.buttons(1, 1);
                (bits != 0).then_some((f, bits))
            })
            .collect()
    }

    #[test]
    fn any_bits_repeat_on_their_own_clocks() {
        // L held from frame 0, A from frame 67, with the default BREPEATs.
        let (l, a) = (1 << 8, 1 << 3);
        let mut input = Input::default();
        let rows = repeat_rows(&mut input, 136, |f| if f < 67 { l } else { l | a });
        #[rustfmt::skip]
        let expected = [
            (0, 0x10100), (15, 0x10100), (20, 0x10100), (25, 0x10100), (30, 0x10100),
            (35, 0x10100), (40, 0x10100), (45, 0x10100), (50, 0x10100), (55, 0x10100),
            (60, 0x10100), (65, 0x10100), (67, 0x04008), (70, 0x10100), (75, 0x10100),
            (80, 0x10100), (82, 0x04000), (85, 0x10100), (87, 0x04008), (90, 0x10100),
            (91, 0x00008), (92, 0x04000), (95, 0x10108), (97, 0x04000), (99, 0x00008),
            (100, 0x10100), (102, 0x04000), (103, 0x00008), (105, 0x10100), (107, 0x04008),
            (110, 0x10100), (111, 0x00008), (112, 0x04000), (115, 0x10108), (117, 0x04000),
            (119, 0x00008), (120, 0x10100), (122, 0x04000), (123, 0x00008), (125, 0x10100),
            (127, 0x04008), (130, 0x10100), (131, 0x00008), (132, 0x04000), (135, 0x10108),
        ];
        assert_eq!(rows, expected);

        // A alone, with #B_RANY at 40,10 and #B_ANY at 25,6.
        let mut input = Input::default();
        input.repeat[14] = (40, 10);
        input.repeat[16] = (25, 6);
        let rows = repeat_rows(&mut input, 64, |_| a);
        #[rustfmt::skip]
        let expected = [
            (0, 0x14008), (20, 0x00008), (24, 0x00008), (25, 0x10000), (28, 0x00008),
            (32, 0x10008), (36, 0x00008), (39, 0x10000), (40, 0x04008), (44, 0x00008),
            (46, 0x10000), (48, 0x00008), (51, 0x04000), (52, 0x00008), (53, 0x10000),
            (56, 0x00008), (60, 0x10008), (62, 0x04000),
        ];
        assert_eq!(rows, expected);

        // Up held 17 frames, with #B_RANY and #B_ANY at 30,6: only #B_LANY
        // repeats, at 15.
        let mut input = Input::default();
        input.repeat[14] = (30, 6);
        input.repeat[16] = (30, 6);
        let rows = repeat_rows(&mut input, 17, |_| 1 << 4);
        assert_eq!(rows, [(0, 0x18010), (15, 0x08000)]);
    }
}

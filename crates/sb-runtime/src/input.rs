//! Controllers, mouse, touch and keyboard: what the frontend reports, read
//! once a frame, and the commands that read it: BUTTON, BREPEAT, STICK,
//! CONTROLLER, XCTRLSTYLE, ACCEL, GYROA, GYROV, GYROSYNC, MOUSE, MBUTTON,
//! TOUCH, KEYBOARD, KEY and PUSHKEY.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{Value, new_str};

use crate::line::codes;
use crate::runtime::{Command, Runtime, checking, no_form, opt_int};

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
/// this many frames after the press, then every so many.
// UNVERIFIED: that the prompt and the editor repeat as INKEY$ does.
const KEY_REPEAT: (u32, u32) = (19, 3);
/// Shift, Ctrl, Alt and the Windows keys' USB usage IDs.
const MODIFIER_KEYS: std::ops::RangeInclusive<u8> = 0xE0..=0xE7;
/// #B_L1 and #B_R1, either of which is Ctrl for [`PAD_KEYS`] and for typing.
const PAD_KEY_CTRL: u32 = 0b11 << 8;
/// The keyboard's Ctrl among [`State::modifiers`].
const MODIFIER_CTRL: u32 = 1 << 2;
/// Left and Right Shift's USB usage IDs.
const SHIFT_KEYS: [u8; 2] = [0xE1, 0xE5];
/// Left and Right Ctrl's.
const CTRL_KEYS: [u8; 2] = [0xE0, 0xE4];

/// Whether the key with this USB usage ID is down among `keys`, one bit each.
fn key_held(keys: &[u64; 4], usage: u8) -> bool {
    keys[usize::from(usage / 64)] >> (usage % 64) & 1 != 0
}

/// Whether either Shift is down among `keys`.
fn shift_held(keys: &[u64; 4]) -> bool {
    SHIFT_KEYS.iter().any(|&k| key_held(keys, k))
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
    codes: Vec<u16>,
    /// Frames since it went down, or since Shift last went down or up.
    frames: u32,
    /// Whether it types into the editor, as known from its first frame.
    editor: Option<bool>,
    /// Whether Shift was down on the last frame.
    shift: Option<bool>,
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
    /// the controller's L or R, which count as Ctrl. Keys such as Enter
    /// still type. Buttons the keys press do not count.
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

    /// The default controller's stick: the one tilted furthest.
    // UNVERIFIED: that it is the one tilted furthest.
    fn stick(&self, stick: usize) -> (f64, f64) {
        (self.pads.iter().map(|p| p.sticks[stick]))
            .max_by(|a, b| (a.0.hypot(a.1)).total_cmp(&b.0.hypot(b.1)))
            .unwrap_or_default()
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
    /// Frames each button of controllers 0 to 4 has been held.
    held: [[u32; BUTTONS]; PADS + 1],
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
    /// Keyboard keys held while the editor has the keys, by USB usage ID
    /// as [`State::keys`]. [`Input::live`] has none of them then, as
    /// programs see nothing, but a key held in the editor repeats too.
    pub editor_keys: [u64; 4],
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
            held: [[0; BUTTONS]; PADS + 1],
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
            editor_keys: [0; 4],
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
            let buttons = self.now.buttons(pad);
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
    /// when a program ends give no presses or repeats to the next one.
    pub fn forget(&mut self) {
        self.now = self.live;
        self.before = self.live;
        self.held = [[0; BUTTONS]; PADS + 1];
        self.touch = (0, 0);
        // What is held goes on counting as held, and types nothing more
        // until let go.
        self.spend_pad_key();
        self.key_repeat = KeyRepeat::default();
    }

    /// A keyboard key went down: what [`Runtime::type_held`] types for it
    /// next, for a program, the prompt or the editor, repeats while it
    /// stays down, counted in the machine's frames rather than by the
    /// computer's own key repeat, and the key held before stops repeating.
    pub fn key_down(&mut self, usage: u8) {
        // UNVERIFIED: that Ctrl, Alt and the Windows keys going down leave
        // the held key repeating, as Shift does.
        if !MODIFIER_KEYS.contains(&usage) {
            self.key_repeat = KeyRepeat {
                usage,
                ..KeyRepeat::default()
            };
        }
    }

    /// The held keyboard key types nothing more.
    pub(crate) fn stop_key_repeat(&mut self) {
        self.key_repeat = KeyRepeat::default();
    }

    /// What the held keyboard key types again this frame, once a frame:
    /// none once it is let go. `held`: the key is still down; `shift`:
    /// Shift is down.
    fn repeated_key(&mut self, held: bool, shift: bool) -> &[u16] {
        let r = &mut self.key_repeat;
        if !held {
            *r = KeyRepeat::default();
            return &[];
        }
        r.frames += 1;
        // Shift going down while a letter is held starts the count again,
        // and the letter repeats in the other case.
        // UNVERIFIED: that Shift going up does the same, and that keys
        // other than letters go on repeating as they were.
        let letter = |c: &u16| u8::try_from(*c).is_ok_and(|c| c.is_ascii_alphabetic());
        if r.shift.replace(shift).is_some_and(|was| was != shift) && r.codes.iter().any(letter) {
            r.frames = 1;
            for c in r.codes.iter_mut().filter(|c| letter(c)) {
                *c ^= 0x20;
            }
        }
        let (start, interval) = KEY_REPEAT;
        if r.frames > start && (r.frames - 1 - start).is_multiple_of(interval) {
            &r.codes
        } else {
            &[]
        }
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
    /// it repeats. Another input going down meanwhile types nothing until
    /// all are let go.
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
    /// condition `function`, with #B_RANY, #B_LANY and #B_ANY.
    fn buttons(&self, pad: usize, function: i32) -> u32 {
        let (now, before) = (self.now.buttons(pad), self.before.buttons(pad));
        let bits = match function {
            0 => now,
            1 => {
                let mut bits = now & !before;
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
            2 => now & !before,
            _ => !now & before,
        };
        let any = |mask: u32| u32::from(bits & mask != 0);
        bits | any(RANY) << 14 | any(LANY) << 15 | any(ANY) << 16
    }
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
        let r = &mut self.input.key_repeat;
        // Only what came with the press, not what a later frame brings.
        if r.usage != 0 && r.frames == 0 {
            r.codes.push(code);
        }
    }

    /// Types again what the held keyboard key typed, once a frame before
    /// the frame's keys go to the program, the prompt or the editor.
    /// `beside` is the other interpreter's input, which has the keys while
    /// it has the focus; `editor`: the editor has the keys.
    pub(crate) fn repeat_held_key(&mut self, beside: Option<&State>, editor: bool) {
        let r = &mut self.input.key_repeat;
        let usage = r.usage;
        if usage == 0 {
            return;
        }
        // The key types only where it went down: the editor taking the
        // keys or giving them up stops it.
        let same_place = *r.editor.get_or_insert(editor) == editor;
        let down = if editor {
            key_held(&self.input.editor_keys, usage)
        } else {
            self.input.live.key(usage) || beside.is_some_and(|s| s.key(usage))
        };
        let shift = if editor {
            shift_held(&self.input.editor_keys)
        } else {
            shift_held(&self.input.live.keys) || beside.is_some_and(|s| shift_held(&s.keys))
        };
        // A DIALOG takes no repeats, and the key stops repeating.
        let held = down && same_place && self.dialog.is_none() && !self.dialog_beside;
        // Empty, and so not allocated, but on the frames that repeat.
        let codes = self.input.repeated_key(held, shift).to_vec();
        for code in codes {
            self.key_press(code);
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
                // UNVERIFIED: the colors of handheld, and the last two of a
                // Pro Controller.
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
}

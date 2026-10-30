//! Controllers, mouse, touch and keyboard: what the frontend reports, read
//! once a frame, and the commands that read it: BUTTON, BREPEAT, STICK,
//! CONTROLLER, XCTRLSTYLE, ACCEL, GYROA, GYROV, GYROSYNC, MOUSE, MBUTTON,
//! TOUCH, KEYBOARD, KEY and PUSHKEY.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{Value, new_str};

use crate::runtime::{Command, Runtime, no_form, opt_int};

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
/// The most characters KEY keeps.
// UNVERIFIED: longer strings are String too long, for KEY and PUSHKEY.
const KEY_LENGTH: usize = 256;
const PUSHKEY_LENGTH: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pad {
    pub connected: bool,
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

    fn key(&self, usage: u8) -> bool {
        self.keys[usize::from(usage / 64)] >> (usage % 64) & 1 != 0
    }

    /// A controller's buttons; controller 0 has those of all of them.
    fn buttons(&self, pad: usize) -> u32 {
        match pad {
            0 => self.pads.iter().fold(0, |b, p| b | p.buttons),
            n => self.pads[n - 1].buttons,
        }
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
    style: (i32, i32),
    /// KEY's contents for F1 to F5.
    pub function_keys: [Vec<u16>; 5],
    /// BUTTON or STICK has been called; see [`Input::take_buttons_read`].
    buttons_read: bool,
}

impl Default for Input {
    fn default() -> Self {
        Input {
            live: State::default(),
            now: State::default(),
            before: State::default(),
            held: [[0; BUTTONS]; PADS + 1],
            // UNVERIFIED: buttons do not repeat until BREPEAT says so.
            repeat: [(0, 0); BUTTON_IDS],
            touch: (0, 0),
            style: (1, 0),
            function_keys: Default::default(),
            buttons_read: false,
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

    /// Whether the program has read buttons or sticks since the last call,
    /// for the frontend to show where they are on the keyboard.
    pub fn take_buttons_read(&mut self) -> bool {
        std::mem::take(&mut self.buttons_read)
    }

    /// Buttons of any controller that went down this frame, with
    /// #B_RANY, #B_LANY and #B_ANY.
    pub(crate) fn pressed(&self) -> u32 {
        self.buttons(0, 2)
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
                    // UNVERIFIED: the first repeat comes `start` frames after
                    // the press, the next ones every `interval` frames.
                    if start > 0 && held > start && (held - 1 - start) % interval.max(1) == 0 {
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

fn out_of_range(what: &str, n: i32) -> Error {
    Error::with_detail(ErrorKind::OutOfRange, format!("{what} {n}"))
}

fn in_range(what: &str, v: &Value, range: std::ops::RangeInclusive<i32>) -> Result<i32> {
    let n = int(v)?;
    if range.contains(&n) {
        Ok(n)
    } else {
        Err(out_of_range(what, n))
    }
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

fn reals(values: &[f64]) -> Vec<Value> {
    values.iter().map(|&x| Value::Real(x)).collect()
}

fn ints(values: &[i32]) -> Vec<Value> {
    values.iter().map(|&n| Value::Int(n)).collect()
}

fn flag(b: bool) -> Value {
    Value::Int(i32::from(b))
}

fn text(v: &Value) -> Result<Vec<u16>> {
    Ok(v.clone().into_str()?.borrow().clone())
}

impl Runtime {
    pub(crate) fn input_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        let input = &mut self.input;
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
                let start = in_range("start", &args[1], 0..=i32::MAX)?;
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
                    // UNVERIFIED: the default controller's stick is the one
                    // tilted furthest.
                    0 => (input.now.pads.iter().map(|p| p.sticks[stick]))
                        .max_by(|a, b| (a.0.hypot(a.1)).total_cmp(&b.0.hypot(b.1)))
                        .unwrap_or_default(),
                    n => input.now.pads[n - 1].sticks[stick],
                };
                Ok(reals(&[x, y]))
            }
            (Command::Controller, Want::Value | Want::Outs(1 | 3 | 5), 1) => {
                let pad = pad_id(&args[0])?;
                // The keyboard's controller and the default one count as
                // handheld, others as Pro Controllers.
                let kind = match pad {
                    0 | 1 => 1,
                    n if input.now.pads[n - 1].connected => 2,
                    _ => 0,
                };
                // UNVERIFIED: the colors; black when not connected.
                let (main, sub) = if kind == 0 {
                    (0xFF00_0000_u32, 0xFF00_0000_u32)
                } else {
                    (0xFF82_8282, 0xFF0F_0F0F)
                };
                let mut out = vec![Value::Int(kind)];
                out.extend([main, sub, 0xFF00_0000, 0xFF00_0000].map(|c| Value::Int(c as i32)));
                out.truncate(want.count());
                Ok(out)
            }
            (Command::Xctrlstyle, Want::Outs(0), 1) => {
                in_range("style", &args[0], 0..=0)?;
                input.style = (1, 0);
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
                for a in &args[2..] {
                    opt_int(a)?;
                }
                // The connection screen has nothing to ask: all controllers
                // there are are connected.
                input.style = (max, style);
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
                if s.len() > KEY_LENGTH {
                    return Err(Error::with_detail(ErrorKind::StringTooLong, "KEY"));
                }
                input.function_keys[n as usize - 1] = s;
                Ok(vec![])
            }
            (Command::Key, Want::Value, 1) => {
                let n = in_range("key", &args[0], 1..=5)?;
                let s = input.function_keys[n as usize - 1].clone();
                Ok(vec![Value::Str(new_str(s))])
            }
            (Command::Pushkey, Want::Outs(0), 1) => {
                let s = text(&args[0])?;
                if s.len() > PUSHKEY_LENGTH {
                    return Err(Error::with_detail(ErrorKind::StringTooLong, "PUSHKEY"));
                }
                for c in s {
                    self.key_press(c);
                }
                Ok(vec![])
            }
            _ => Err(no_form()),
        }
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
        assert_eq!(fired, [true, false, false, true, false, true, false, true]);
    }
}

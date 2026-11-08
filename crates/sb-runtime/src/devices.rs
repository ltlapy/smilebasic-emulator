//! Controller hardware the emulator does not have, as stand-ins that check
//! their arguments and report nothing: the IR Motion Camera (IRSTART,
//! IRSTOP, IRSTATE, IRREAD, IRSPRITE), vibration (VIBRATE) and Toy-Con
//! (TCPIANO, TCHOUSE, TCFISHING, TCROBOT, TCVISOR, TCVEHICLE, TCBIKE,
//! TCCAR, TCPLANE, TCSUBM).

use std::cell::RefCell;
use std::rc::Rc;

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{Array, ElemType, Value};

use crate::input::{in_range, ints, out_of_range, reals};
use crate::runtime::{Command, Runtime, no_form};
use crate::sprites::SPRITES;

/// How many frames each of VIBRATE's patterns 1 to 62 lasts, measured on
/// SmileBASIC 4.4.9 (TTEST24) as frames until `VIBRATE(1,0)` read 0; 0
/// for those still going after two seconds (pattern 40 after five), which
/// last until stopped here.
#[rustfmt::skip]
const PATTERN_FRAMES: [u32; 62] = [
    22, 16, 16, 12, 35, 33, 66, 61, 57, 80,
    56, 47, 47, 10, 10, 35, 49, 72, 91, 81,
    16, 13, 115, 49, 16, 37, 48, 16, 35, 35,
    18, 89, 62, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 31, 31, 31, 0, 0, 61, 7,
    16, 16, 12, 13, 16, 5, 3, 8, 15, 9,
    9, 13,
];

/// XCTRLSTYLE's operation style for Toy-Con.
const TOYCON_STYLE: i32 = 100;
/// IRREAD's array: 48 rows of eight values, for up to 16 clusters or 48
/// blocks.
const IR_WIDTH: usize = 8;
const IR_ROWS: usize = 48;

fn zeros(elem: ElemType, dims: Vec<usize>) -> Value {
    Value::Array(Rc::new(RefCell::new(Array::new(elem, dims))))
}

/// The camera and Toy-Con take controller 1 or 2, as an index.
fn pad(v: &Value) -> Result<usize> {
    Ok(in_range("controller", v, 1..=2)? as usize - 1)
}

/// A Toy-Con's results, all zero, when `outs` is one of the counts its
/// command allows.
fn unrecognized(outs: usize, allowed: &[usize]) -> Result<Vec<Value>> {
    if !allowed.contains(&outs) {
        return Err(ErrorKind::WrongNumberOfOutArguments.into());
    }
    let mut out = ints(&[0]);
    out.extend(reals(&vec![0.0; outs - 1]));
    Ok(out)
}

impl Runtime {
    /// The Toy-Con that XCTRLSTYLE set up for a controller must be one of
    /// `types` on either side: Illegal control style otherwise (checked
    /// on hardware without any Toy-Con set up).
    fn toycon_set(&self, pad: usize, types: &[i32]) -> Result<()> {
        let input = &self.input;
        let sides = &input.toycons[pad * 2..pad * 2 + 2];
        if input.style.1 == TOYCON_STYLE && sides.iter().any(|t| types.contains(t)) {
            Ok(())
        } else {
            Err(ErrorKind::IllegalControlStyle.into())
        }
    }

    pub(crate) fn device_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        let outs = want.count();
        match (command, want, args.len()) {
            // The camera starts and stops, and never sees anything. Without
            // XCTRLSTYLE's IR flag it is not available (checked on
            // hardware).
            (Command::Irstart, Want::Outs(0), 2..=8) => {
                let pad = pad(&args[0])?;
                let mode = in_range("recognition type", &args[1], 0..=2)?;
                if mode == 1 && pad != 0 {
                    return Err(out_of_range("controller", pad as i32 + 1));
                }
                if !self.input.ir_allowed {
                    return Err(ErrorKind::IrCameraNotAvailable.into());
                }
                for a in &args[2..] {
                    crate::runtime::opt_int(a)?;
                }
                self.input.ir_state[pad] = 2 + mode;
                Ok(vec![])
            }
            (Command::Irstop, Want::Outs(0), 1) => {
                let pad = pad(&args[0])?;
                if self.input.ir_state[pad] != 0 {
                    self.input.ir_state[pad] = 1;
                }
                Ok(vec![])
            }
            (Command::Irstate, Want::Value, 1) => {
                let pad = pad(&args[0])?;
                Ok(ints(&[self.input.ir_state[pad]]))
            }
            // A camera never started is no camera, and mode 1 cannot be read.
            // Clustering and moment modes both give a 48-row array.
            // UNVERIFIED: the same after IRSTOP.
            (Command::Irread, Want::Outs(4), 1) => {
                let pad = pad(&args[0])?;
                let count = match self.input.ir_state[pad] {
                    0 => return Err(ErrorKind::IrCameraNotAvailable.into()),
                    3 => return Err(ErrorKind::IllegalIrCameraMode.into()),
                    4 => IR_ROWS,
                    _ => 0,
                };
                let mut out = ints(&[0, 0, count as i32]);
                out.push(zeros(ElemType::Int, vec![IR_ROWS, IR_WIDTH]));
                Ok(out)
            }
            (Command::Irsprite, Want::Outs(0), 2) => {
                in_range("controller", &args[0], 1..=1)?;
                in_range("sprite", &args[1], 0..=SPRITES as i32 - 1)?;
                Ok(vec![])
            }
            // Nothing vibrates, but VIBRATE() reports what was asked for:
            // the pattern until it would have ended, or 65535 for
            // frequencies until stopped, on each side. A 0 frequency or one
            // 0 amplitude in the frequency form does not stop it.
            // UNVERIFIED: that both amplitudes 0 stop it.
            (Command::Vibrate, Want::Outs(0), 0) => {
                self.input.vibration = Default::default();
                Ok(vec![])
            }
            (Command::Vibrate, Want::Outs(0), 2 | 3 | 5 | 6) => {
                let id = in_range("controller", &args[0], 1..=4)? as usize - 1;
                // Three or six arguments name the side.
                let (side, rest) = match args.len() {
                    3 | 6 => (
                        Some(in_range("side", &args[1], 0..=1)? as usize),
                        &args[2..],
                    ),
                    _ => (None, &args[1..]),
                };
                let (state, frames) = match rest {
                    [pattern] => {
                        let n = in_range("pattern", pattern, 0..=62)?;
                        let frames = (n as usize).checked_sub(1).map(|i| PATTERN_FRAMES[i]);
                        (n, frames.filter(|&f| f > 0))
                    }
                    [low_freq, low, high_freq, high] => {
                        low_freq.to_real()?;
                        high_freq.to_real()?;
                        let mut any = false;
                        for amp in [low, high] {
                            let a = amp.to_real()?;
                            if !(-128.0..=127.0).contains(&a) {
                                return Err(Error::with_detail(ErrorKind::OutOfRange, "amplitude"));
                            }
                            any |= a != 0.0;
                        }
                        (if any { 65535 } else { 0 }, None)
                    }
                    _ => return Err(no_form()),
                };
                // Held sideways or upright, a controller has one side.
                let one_side = matches!(self.input.style.1, 2 | 3);
                let v = &mut self.input.vibration[id];
                let set = (state, frames.filter(|_| state != 0));
                match (side, one_side) {
                    (Some(1), true) => {}
                    (Some(s), _) => v[s] = set,
                    (None, _) => *v = [set; 2],
                }
                Ok(vec![])
            }
            (Command::Vibrate, Want::Value, 1..=2) => {
                let id = in_range("controller", &args[0], 1..=4)? as usize - 1;
                let [(left, _), (right, _)] = self.input.vibration[id];
                let one_side = matches!(self.input.style.1, 2 | 3);
                let state = match args
                    .get(1)
                    .map(|s| in_range("side", s, 0..=1))
                    .transpose()?
                {
                    Some(1) if one_side => 0,
                    Some(1) => right,
                    Some(_) => left,
                    None if one_side => left,
                    // Checked on hardware: 40 on both sides reads 2621480,
                    // frequencies -1.
                    None => right.wrapping_mul(65536).wrapping_add(left),
                };
                Ok(ints(&[state]))
            }
            // Toy-Con are set up but never recognized.
            (Command::Tcpiano, Want::Outs(_), 1) => {
                self.toycon_set(pad(&args[0])?, &[1])?;
                unrecognized(outs, &[1, 2, 3, 4])
            }
            (Command::Tchouse, Want::Outs(2), 1) => {
                self.toycon_set(pad(&args[0])?, &[2])?;
                Ok(vec![Value::Int(0), zeros(ElemType::Real, vec![3, 4])])
            }
            (Command::Tcbike, Want::Outs(_), 1) => {
                self.toycon_set(pad(&args[0])?, &[3])?;
                unrecognized(outs, &[1, 5])
            }
            (Command::Tcfishing, Want::Outs(_), 1) => {
                self.toycon_set(pad(&args[0])?, &[4])?;
                unrecognized(outs, &[1, 6])
            }
            (Command::Tcrobot, Want::Outs(2 | 5), 1) => {
                self.toycon_set(pad(&args[0])?, &[5])?;
                let mut out = vec![Value::Int(0), zeros(ElemType::Real, vec![4, 5])];
                out.extend(reals(&vec![0.0; outs - 2]));
                Ok(out)
            }
            (Command::Tcvisor, Want::Outs(4), 1) => {
                self.toycon_set(pad(&args[0])?, &[6])?;
                unrecognized(outs, &[4])
            }
            (Command::Tccar, Want::Outs(_), 1) => {
                self.toycon_set(pad(&args[0])?, &[7, 10])?;
                unrecognized(outs, &[1, 3, 10])
            }
            (Command::Tcplane, Want::Outs(_), 1) => {
                self.toycon_set(pad(&args[0])?, &[8, 10])?;
                unrecognized(outs, &[1, 4])
            }
            (Command::Tcsubm, Want::Outs(_), 1) => {
                self.toycon_set(pad(&args[0])?, &[9, 10])?;
                unrecognized(outs, &[1, 4])
            }
            (Command::Tcvehicle, Want::Value, 1) => {
                let pad = pad(&args[0])?;
                if self.input.style.1 != TOYCON_STYLE {
                    self.toycon_set(pad, &[])?;
                }
                Ok(ints(&[0]))
            }
            _ => Err(no_form()),
        }
    }
}

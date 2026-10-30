//! Commands on what text screens hold: TPUT, TFILL, SCROLL, CHKCHR,
//! TCOORD, TCOPY, TSAVE, TLOAD, TARRAY and TUPDATE; and their animations,
//! variables and callbacks: TANIM, TSTART, TSTOP, TCHK, TVAR and TFUNC.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::Value;

use crate::anim::{Anim, Target, Track};
use crate::grp::{array_colors, numeric_array, store_ints};
use crate::runtime::{Command, Runtime, no_form, screen_id, with_screen};
use crate::sp::{anim_id, var_key};
use crate::text::{Cell, CellRect, TextScreen};

fn int(v: &Value) -> Result<i32> {
    v.to_int()
}

fn out_of_range(what: &str, n: i32) -> Error {
    Error::with_detail(ErrorKind::OutOfRange, format!("{what} {n}"))
}

/// A character code, or the characters of a string.
fn codes(v: &Value) -> Result<Vec<u16>> {
    match v {
        Value::Str(s) => Ok(s.borrow().clone()),
        v => match int(v)? {
            n @ 0..=0xFFFF => Ok(vec![n as u16]),
            n => Err(out_of_range("character code", n)),
        },
    }
}

/// An ATTR value, or the screen's own when omitted.
fn attr_arg(v: Option<&Value>, screen: &TextScreen) -> Result<u8> {
    match v {
        None | Some(Value::Default) => Ok(screen.attr),
        Some(v) => match int(v)? {
            n @ 0..16 => Ok(n as u8),
            n => Err(out_of_range("attribute", n)),
        },
    }
}

/// Corners given as X0,Y0,X1,Y1 in either order, as a rectangle.
fn corners(args: &[Value]) -> Result<CellRect> {
    let v: Vec<i32> = args[..4].iter().map(int).collect::<Result<_>>()?;
    let (x0, x1) = (v[0].min(v[2]), v[0].max(v[2]));
    let (y0, y1) = (v[1].min(v[3]), v[1].max(v[3]));
    Ok((x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

/// X,Y,width,height; the whole screen when `args` is empty. It may reach
/// off the screen (checked on hardware: TSAVE reads empty cells there).
// UNVERIFIED: the limits on width and height.
fn rect_on(args: &[Value], screen: &TextScreen) -> Result<CellRect> {
    let (w, h) = screen.size();
    if args.is_empty() {
        return Ok((0, 0, w as i32, h as i32));
    }
    let v: Vec<i32> = args[..4].iter().map(int).collect::<Result<_>>()?;
    let fits = |len: i32| (1..=4096).contains(&len);
    if fits(v[2]) && fits(v[3]) {
        Ok((v[0], v[1], v[2], v[3]))
    } else {
        Err(Error::with_detail(
            ErrorKind::OutOfRange,
            format!("{},{},{},{}", v[0], v[1], v[2], v[3]),
        ))
    }
}

/// Display coordinates to the screen's own pixels, which TOFS, THOME,
/// TROT and TSCALE place; `None` if the screen is scaled to nothing.
fn screen_dots(screen: &TextScreen, x: f64, y: f64) -> Option<(f64, f64)> {
    Some(screen.placement().inverse()?.apply(x, y))
}

/// The cell under the screen's pixel `(x, y)`.
fn cell_of(screen: &TextScreen, (x, y): (f64, f64)) -> (i32, i32) {
    let cs = f64::from(screen.font().1);
    ((x / cs).floor() as i32, (y / cs).floor() as i32)
}

impl Runtime {
    pub(crate) fn text_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        match (command, want, args.len()) {
            // Checked on hardware: off-screen cells are out of range, and
            // an empty string writes nothing.
            (Command::Tput, Want::Outs(0), 4 | 5) => {
                let t = &mut self.text[screen_id(&args[0])?];
                let (x, y) = (int(&args[1])?, int(&args[2])?);
                if t.cell_at(x, y).is_none() {
                    return Err(Error::with_detail(
                        ErrorKind::OutOfRange,
                        format!("cell {x},{y}"),
                    ));
                }
                let codes = codes(&args[3])?;
                let attr = attr_arg(args.get(4), t)?;
                if let Some(&code) = codes.first() {
                    let color = t.color();
                    t.set_cell(x, y, Cell { code, color, attr });
                }
                Ok(vec![])
            }
            // The characters repeat from the top left, row by row.
            (Command::Tfill, Want::Outs(0), 6 | 7) => {
                let t = &mut self.text[screen_id(&args[0])?];
                let (x, y, w, h) = corners(&args[1..5])?;
                let codes = codes(&args[5])?;
                let attr = attr_arg(args.get(6), t)?;
                let color = t.color();
                if !codes.is_empty() {
                    let cells: Vec<Cell> = (codes.iter().cycle().take((w * h) as usize))
                        .map(|&code| Cell { code, color, attr })
                        .collect();
                    t.load((x, y, w, h), &cells);
                }
                Ok(vec![])
            }
            (Command::Scroll, Want::Outs(0), 2 | 3) => {
                let (id, rest) = with_screen(args, 2)?;
                let (dx, dy) = (int(&rest[0])?, int(&rest[1])?);
                self.text[id].scroll(dx, dy);
                Ok(vec![])
            }
            // Off-screen positions give 0 (checked on hardware).
            (Command::Chkchr, Want::Value | Want::Outs(2), 2..=4) => {
                let (id, rest) = match args.len() {
                    2 => (crate::text::CONSOLE, args),
                    _ => (screen_id(&args[0])?, &args[1..]),
                };
                let t = &self.text[id];
                let (x, y) = (rest[0].to_real()?, rest[1].to_real()?);
                let display = match rest.get(2) {
                    None => false,
                    Some(v) => match int(v)? {
                        n @ (0 | 1) => n == 1,
                        n => return Err(out_of_range("coordinate mode", n)),
                    },
                };
                let cell = if display {
                    screen_dots(t, x, y).map(|dots| cell_of(t, dots))
                } else {
                    Some((int(&rest[0])?, int(&rest[1])?))
                };
                let cell = cell.and_then(|(x, y)| t.cell_at(x, y)).unwrap_or(Cell {
                    code: 0,
                    color: 0,
                    attr: 0,
                });
                let mut out = vec![Value::Int(i32::from(cell.code))];
                if want == Want::Outs(2) {
                    out.push(Value::Int(i32::from(cell.attr)));
                }
                Ok(out)
            }
            // UNVERIFIED: modes 0 and 2 give reals and mode 1 integers; a
            // screen scaled to nothing gives 0,0.
            (Command::Tcoord, Want::Outs(2), 3 | 4) => {
                let t = &self.text[screen_id(&args[0])?];
                let (x, y) = (args[1].to_real()?, args[2].to_real()?);
                let mode = args.get(3).map(int).transpose()?.unwrap_or(0);
                Ok(match mode {
                    0 => {
                        let (dx, dy) = t.placement().apply(x, y);
                        vec![Value::Real(dx), Value::Real(dy)]
                    }
                    1 => {
                        let (cx, cy) = screen_dots(t, x, y).map_or((0, 0), |d| cell_of(t, d));
                        vec![Value::Int(cx), Value::Int(cy)]
                    }
                    2 => {
                        let (sx, sy) = screen_dots(t, x, y).unwrap_or((0.0, 0.0));
                        vec![Value::Real(sx), Value::Real(sy)]
                    }
                    n => return Err(out_of_range("mode", n)),
                })
            }
            // UNVERIFIED: what reaches off either screen is left out.
            (Command::Tcopy, Want::Outs(0), 7 | 8) => {
                let dest = screen_id(&args[0])?;
                let (src, rest) = match args.len() {
                    8 => (screen_id(&args[1])?, &args[2..]),
                    _ => (dest, &args[1..]),
                };
                let (x, y, w, h) = corners(&rest[..4])?;
                let (dx, dy) = (int(&rest[4])?, int(&rest[5])?);
                let from = &self.text[src];
                let cells: Vec<(i32, i32, Cell)> = (0..h)
                    .flat_map(|cy| (0..w).map(move |cx| (cx, cy)))
                    .filter_map(|(cx, cy)| Some((cx, cy, from.cell_at(x + cx, y + cy)?)))
                    .collect();
                let t = &mut self.text[dest];
                for (cx, cy, cell) in cells {
                    t.set_cell(dx + cx, dy + cy, cell);
                }
                Ok(vec![])
            }
            (Command::Tsave, Want::Outs(0), 2 | 6) => {
                let t = &self.text[screen_id(&args[0])?];
                let rect = rect_on(&args[1..args.len() - 1], t)?;
                let array = numeric_array(args.last().expect("an array"))?;
                let values: Vec<i32> = t.save(rect).iter().flat_map(|c| c.to_ints()).collect();
                store_ints(&array, &values)?;
                Ok(vec![])
            }
            (Command::Tload, Want::Outs(0), 2 | 6) => {
                let id = screen_id(&args[0])?;
                let rect = rect_on(&args[1..args.len() - 1], &self.text[id])?;
                let array = numeric_array(args.last().expect("an array"))?;
                let values = array_colors(&array, (rect.2 * rect.3 * 2) as usize)?;
                let cells: Vec<Cell> = (values.chunks(2))
                    .map(|v| Cell::from_ints([v[0] as i32, v[1] as i32]))
                    .collect();
                self.text[id].load(rect, &cells);
                Ok(vec![])
            }
            (Command::Tarray, Want::Value, 1) => {
                let array = self.text[screen_id(&args[0])?].array();
                Ok(vec![Value::Array(array)])
            }
            (Command::Tupdate, Want::Outs(0), 1 | 5) => {
                let t = &mut self.text[screen_id(&args[0])?];
                let (w, h) = t.size();
                let rect = match args.len() {
                    5 => corners(&args[1..])?,
                    _ => (0, 0, w as i32, h as i32),
                };
                t.update(rect);
                Ok(vec![])
            }
            (Command::Tanim, Want::Outs(0), 0) => {
                for t in &mut self.text {
                    t.anims = Default::default();
                }
                Ok(vec![])
            }
            (Command::Tanim, Want::Outs(0), 1) => {
                self.text[screen_id(&args[0])?].anims = Default::default();
                Ok(vec![])
            }
            (Command::Tanim, Want::Outs(0), 2) => {
                let id = screen_id(&args[0])?;
                let n = anim_id(&args[1])?;
                let anim = self.sprites.anim_defs[n].clone().ok_or_else(|| {
                    Error::with_detail(ErrorKind::IllegalFunctionCall, format!("no animation {n}"))
                })?;
                self.start_text_anim(id, anim);
                Ok(vec![])
            }
            (Command::Tanim, Want::Outs(0), _) => {
                let id = screen_id(&args[0])?;
                let anim = self.anim_args(&args[1..])?;
                self.start_text_anim(id, anim);
                Ok(vec![])
            }
            (Command::Tstart | Command::Tstop, Want::Outs(0), 0 | 1) => {
                let stopped = command == Command::Tstop;
                match args.first() {
                    Some(id) => self.text[screen_id(id)?].anim_stopped = stopped,
                    None => {
                        for t in &mut self.text {
                            t.anim_stopped = stopped;
                        }
                    }
                }
                Ok(vec![])
            }
            // Checked on hardware for SPCHK: stopped animations report 0.
            (Command::Tchk, Want::Value, 1) => {
                let t = &self.text[screen_id(&args[0])?];
                if t.anim_stopped {
                    return Ok(vec![Value::Int(0)]);
                }
                let bits = (t.anims.iter().enumerate())
                    .filter(|(_, track)| track.is_some())
                    .fold(0, |bits, (i, _)| bits | 1 << i);
                Ok(vec![Value::Int(bits)])
            }
            (Command::Tvar, Want::Outs(0), 3) => {
                let key = var_key(&args[1])?;
                let value = match &args[2] {
                    Value::Int(_) | Value::Real(_) | Value::Str(_) => args[2].clone(),
                    _ => return Err(ErrorKind::TypeMismatch.into()),
                };
                self.text[screen_id(&args[0])?].vars.insert(key, value);
                Ok(vec![])
            }
            (Command::Tvar, Want::Value, 2) => {
                let key = var_key(&args[1])?;
                let t = &self.text[screen_id(&args[0])?];
                Ok(vec![t.vars.get(&key).cloned().unwrap_or(Value::Int(0))])
            }
            (Command::Tvar, Want::Outs(0), 1 | 2) => {
                let key = args.get(1).map(var_key).transpose()?;
                let t = &mut self.text[screen_id(&args[0])?];
                match key {
                    Some(key) => {
                        t.vars.remove(&key);
                    }
                    None => t.vars.clear(),
                }
                Ok(vec![])
            }
            (Command::Tfunc, Want::Outs(0), 1 | 2) => {
                let func = match args.get(1) {
                    Some(Value::Str(name)) => Some(name.borrow().clone()),
                    Some(_) => return Err(ErrorKind::TypeMismatch.into()),
                    None => None,
                };
                self.text[screen_id(&args[0])?].func = func;
                Ok(vec![])
            }
            _ => Err(no_form()),
        }
    }

    /// Text screens ignore "UV" and "I"; TCHK reports nothing for them
    /// (checked on hardware). Their animations start in step with sprites'.
    // UNVERIFIED: that such an animation is not set at all.
    fn start_text_anim(&mut self, id: usize, anim: Anim) {
        if matches!(anim.mode.target, Target::Uv | Target::I) {
            return;
        }
        let t = &mut self.text[id];
        let target = anim.mode.target.index();
        t.anims[target] = Some(Track::new(anim, t));
    }
}

//! Commands on what text screens hold: TPUT, TFILL, SCROLL, CHKCHR,
//! TCOORD, TCOPY, TSAVE, TLOAD, TARRAY and TUPDATE; and their animations,
//! variables and callbacks: TANIM, TSTART, TSTOP, TCHK, TVAR and TFUNC.

use std::ops::Range;

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::Value;

use crate::anim::{Anim, Target, Track};
use crate::grp::{array_colors, numeric_array, store_ints};
use crate::runtime::{Command, Runtime, checking, no_form, screen_id, with_screen};
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
    checking(v, || match v {
        Value::Str(s) => Ok(s.borrow().clone()),
        v => match int(v)? {
            n @ 0..=0xFFFF => Ok(vec![n as u16]),
            n => Err(out_of_range("character code", n)),
        },
    })
}

/// An ATTR value, or the screen's own when omitted.
fn attr_arg(v: Option<&Value>, screen: &TextScreen) -> Result<u16> {
    match v {
        None | Some(Value::Default) => Ok(screen.attr),
        Some(v) => match int(v)? {
            n @ 0..16 => Ok(n as u16),
            n => Err(out_of_range("attribute", n)),
        },
    }
}

/// Corners given as X0,Y0,X1,Y1 in either order, as X,Y,width,height. A
/// side can be up to 2^32 long, so this is wider than a [`CellRect`].
fn corners(args: &[Value]) -> Result<[i64; 4]> {
    let v: Vec<i64> = (args[..4].iter())
        .map(|v| int(v).map(i64::from))
        .collect::<Result<_>>()?;
    let (x0, x1) = (v[0].min(v[2]), v[0].max(v[2]));
    let (y0, y1) = (v[1].min(v[3]), v[1].max(v[3]));
    Ok([x0, y0, x1 - x0 + 1, y1 - y0 + 1])
}

/// The offsets in `0..len` that, added to `start`, land in `0..size`.
fn on_screen(start: i64, len: i64, size: u32) -> Range<i64> {
    (-start).max(0)..(i64::from(size) - start).min(len)
}

/// Both of [`on_screen`]'s ranges; a program can ask for far more cells
/// than any screen has, and only these can make a difference.
fn offsets_on(x: i64, y: i64, w: i64, h: i64, t: &TextScreen) -> (Range<i64>, Range<i64>) {
    let (sw, sh) = t.size();
    (on_screen(x, w, sw), on_screen(y, h, sh))
}

/// The most cells TSAVE and TLOAD take at once.
const MOST_CELLS: i64 = sb_core::memory::TOTAL as i64 / 8;

/// X,Y,width,height; the whole screen when `args` is empty. It may reach
/// off the screen (checked on hardware: TSAVE reads empty cells there).
// UNVERIFIED: expected of SmileBASIC 4, not yet seen: a width or height
// may be anything from 0 up. A rectangle past what memory holds is Out of
// range here, standing in for SmileBASIC 4's own error, as for GSAVE.
fn rect_on(args: &[Value], screen: &TextScreen) -> Result<CellRect> {
    let (w, h) = screen.size();
    if args.is_empty() {
        return Ok((0, 0, w as i32, h as i32));
    }
    let v: Vec<i32> = args[..4].iter().map(int).collect::<Result<_>>()?;
    if v[2] >= 0 && v[3] >= 0 && i64::from(v[2]) * i64::from(v[3]) <= MOST_CELLS {
        Ok((v[0], v[1], v[2], v[3]))
    } else {
        Err(Error::with_detail(
            ErrorKind::OutOfRange,
            format!("{},{},{},{}", v[0], v[1], v[2], v[3]),
        ))
    }
}

/// Display coordinates to the screen's own pixels, which TOFS, THOME,
/// TROT and TSCALE place. An axis scaled to nothing gives the home point.
// UNVERIFIED: that axis is expected of SmileBASIC 4, not yet seen.
fn screen_dots(screen: &TextScreen, x: f64, y: f64) -> (f64, f64) {
    let (dx, dy) = (x - screen.pos.0, y - screen.pos.1);
    let (sin, cos) = screen.rot.to_radians().sin_cos();
    let (rx, ry) = (dx * cos + dy * sin, dy * cos - dx * sin);
    let axis = |v: f64, scale: f64, home: f64| if scale == 0.0 { home } else { v / scale + home };
    (
        axis(rx, screen.scale.0, screen.home.0),
        axis(ry, screen.scale.1, screen.home.1),
    )
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
            // The rectangle is clipped to the screen, then the characters
            // repeat from its top left, row by row at its clipped width.
            (Command::Tfill, Want::Outs(0), 6 | 7) => {
                let t = &mut self.text[screen_id(&args[0])?];
                let [x, y, w, h] = corners(&args[1..5])?;
                let codes = codes(&args[5])?;
                let attr = attr_arg(args.get(6), t)?;
                let color = t.color();
                if !codes.is_empty() {
                    let (xs, ys) = offsets_on(x, y, w, h, t);
                    let width = (xs.end - xs.start).max(0) as usize;
                    for (row, cy) in ys.enumerate() {
                        let mut i = row * width % codes.len();
                        for cx in xs.clone() {
                            let code = codes[i];
                            t.set_cell(
                                (x + cx) as i32,
                                (y + cy) as i32,
                                Cell { code, color, attr },
                            );
                            i += 1;
                            if i == codes.len() {
                                i = 0;
                            }
                        }
                    }
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
                    Some(cell_of(t, screen_dots(t, x.trunc(), y.trunc())))
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
            // Modes 0 and 2 give reals and mode 1 integers. Mode 0 takes
            // whole coordinates, dropping the fractions.
            // UNVERIFIED: that dropping is expected of SmileBASIC 4, not
            // yet seen.
            (Command::Tcoord, Want::Outs(2), 3 | 4) => {
                let t = &self.text[screen_id(&args[0])?];
                let (x, y) = (args[1].to_real()?, args[2].to_real()?);
                let mode = args.get(3).map(int).transpose()?.unwrap_or(0);
                Ok(match mode {
                    0 => {
                        let (dx, dy) = t.placement().apply(x.trunc(), y.trunc());
                        vec![Value::Real(dx), Value::Real(dy)]
                    }
                    1 => {
                        let (cx, cy) = cell_of(t, screen_dots(t, x, y));
                        vec![Value::Int(cx), Value::Int(cy)]
                    }
                    2 => {
                        let (sx, sy) = screen_dots(t, x, y);
                        vec![Value::Real(sx), Value::Real(sy)]
                    }
                    n => return Err(out_of_range("mode", n)),
                })
            }
            // What reaches off either screen is left out.
            (Command::Tcopy, Want::Outs(0), 7 | 8) => {
                let dest = screen_id(&args[0])?;
                let (src, rest) = match args.len() {
                    8 => (screen_id(&args[1])?, &args[2..]),
                    _ => (dest, &args[1..]),
                };
                let [x, y, w, h] = corners(&rest[..4])?;
                let (dx, dy) = (i64::from(int(&rest[4])?), i64::from(int(&rest[5])?));
                // Only what is on both screens makes a difference.
                let from = &self.text[src];
                let (xs, ys) = offsets_on(x, y, w, h, from);
                let (to_xs, to_ys) = offsets_on(dx, dy, w, h, &self.text[dest]);
                let xs = xs.start.max(to_xs.start)..xs.end.min(to_xs.end);
                let ys = ys.start.max(to_ys.start)..ys.end.min(to_ys.end);
                let width = (xs.end - xs.start).max(1) as usize;
                let mut cells = Vec::with_capacity(width * (ys.end - ys.start).max(0) as usize);
                for cy in ys.clone() {
                    for cx in xs.clone() {
                        cells.push(from.cell((x + cx) as u32, (y + cy) as u32));
                    }
                }
                let t = &mut self.text[dest];
                for (cy, row) in ys.zip(cells.chunks(width)) {
                    for (cx, &cell) in xs.clone().zip(row) {
                        t.set_cell((dx + cx) as i32, (dy + cy) as i32, cell);
                    }
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
                let values = array_colors(&array, rect.2 as usize * rect.3 as usize * 2)?;
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
                    5 => {
                        let [x, y, rw, rh] = corners(&args[1..])?;
                        let (xs, ys) = offsets_on(x, y, rw, rh, t);
                        (
                            (x + xs.start) as i32,
                            (y + ys.start) as i32,
                            (xs.end - xs.start).max(0) as i32,
                            (ys.end - ys.start).max(0) as i32,
                        )
                    }
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
                // As with SPANIM, a number ANIMDEF never set starts nothing.
                if let Some(anim) = self.sprites.anim_defs[n].clone() {
                    self.start_text_anim(id, anim);
                }
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
                    // An array is kept as itself, not copied.
                    Value::Int(_) | Value::Real(_) | Value::Str(_) | Value::Array(_) => {
                        args[2].clone()
                    }
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
                    Some(Value::Str(name)) => Some(self.callback_name(&name.borrow())),
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
    // Such an animation is not set at all.
    fn start_text_anim(&mut self, id: usize, anim: Anim) {
        if matches!(anim.mode.target, Target::Uv | Target::I) {
            return;
        }
        let t = &mut self.text[id];
        let target = anim.mode.target.index();
        t.anims[target] = Some(Track::new(anim, t));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::Font;

    /// Gives two screens random sizes and cells.
    fn screens(rt: &mut Runtime, next: &mut impl FnMut(i64) -> i64) {
        for id in 0..2 {
            let (w, h) = (1 + next(9) as u32, 1 + next(9) as u32);
            rt.text[id].set_font(8, 8, w, h);
            for y in 0..h {
                for x in 0..w {
                    let cell = Cell {
                        code: 1 + next(500) as u16,
                        color: next(99) as u32,
                        attr: 0,
                    };
                    rt.text[id].set_cell(x as i32, y as i32, cell);
                }
            }
        }
    }

    fn rng() -> impl FnMut(i64) -> i64 {
        let mut seed = 0x0DDB_1A5E_5BAD_5EEDu64;
        move |n| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as i64
        }
    }

    fn cells(rt: &Runtime, id: usize) -> Vec<Cell> {
        let (w, h) = rt.text[id].size();
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| rt.text[id].cell(x, y))
            .collect()
    }

    fn ints(values: &[i32]) -> Vec<Value> {
        values.iter().map(|&v| Value::Int(v)).collect()
    }

    #[test]
    fn tcopy_matches_copying_every_cell() {
        let mut next = rng();
        let mut rt = Runtime::new(400, 240, Font::blank());
        for _ in 0..20_000 {
            screens(&mut rt, &mut next);
            let v: Vec<i32> = (0..6).map(|_| next(30) as i32 - 12).collect();
            let (dest, src) = (next(2) as usize, next(2) as usize);
            // The whole rectangle, cell by cell, into a buffer first.
            let (x, y) = (v[0].min(v[2]), v[1].min(v[3]));
            let (w, h) = (v[0].max(v[2]) - x + 1, v[1].max(v[3]) - y + 1);
            let copied: Vec<(i32, i32, Cell)> = (0..h)
                .flat_map(|cy| (0..w).map(move |cx| (cx, cy)))
                .filter_map(|(cx, cy)| Some((cx, cy, rt.text[src].cell_at(x + cx, y + cy)?)))
                .collect();
            let mut slow = cells(&rt, dest);
            let (dw, dh) = rt.text[dest].size();
            for (cx, cy, cell) in copied {
                let (to_x, to_y) = (v[4] + cx, v[5] + cy);
                if (0..dw as i32).contains(&to_x) && (0..dh as i32).contains(&to_y) {
                    slow[(to_y as u32 * dw + to_x as u32) as usize] = cell;
                }
            }
            let mut args = ints(&[dest as i32, src as i32]);
            args.extend(ints(&v));
            rt.text_command(Command::Tcopy, &args, Want::Outs(0))
                .unwrap();
            assert_eq!(cells(&rt, dest), slow, "{dest} {src} {v:?}");
        }
    }

    #[test]
    fn tfill_matches_filling_every_cell() {
        let mut next = rng();
        let mut rt = Runtime::new(400, 240, Font::blank());
        for _ in 0..20_000 {
            screens(&mut rt, &mut next);
            let id = next(2) as usize;
            let v: Vec<i32> = (0..4).map(|_| next(30) as i32 - 12).collect();
            let codes: Vec<u16> = (0..1 + next(7)).map(|i| 600 + i as u16).collect();
            let (x, y) = (v[0].min(v[2]), v[1].min(v[3]));
            let (w, h) = (v[0].max(v[2]) - x + 1, v[1].max(v[3]) - y + 1);
            let mut slow = cells(&rt, id);
            let (sw, sh) = rt.text[id].size();
            let (color, attr) = (rt.text[id].color(), 0);
            let (left, right) = (x.max(0), (x + w).min(sw as i32));
            let (top, bottom) = (y.max(0), (y + h).min(sh as i32));
            let mut code = codes.iter().cycle();
            for cy in top..bottom {
                for cx in left..right {
                    let code = *code.next().unwrap();
                    slow[(cy as u32 * sw + cx as u32) as usize] = Cell { code, color, attr };
                }
            }
            let mut args = ints(&[id as i32]);
            args.extend(ints(&v));
            args.push(Value::Str(sb_core::value::new_str(codes.clone())));
            rt.text_command(Command::Tfill, &args, Want::Outs(0))
                .unwrap();
            assert_eq!(cells(&rt, id), slow, "{id} {v:?} {codes:?}");
        }
    }

    #[test]
    fn rectangles_larger_than_any_screen_finish() {
        const FAR: i32 = 1_000_000_000;
        let mut next = rng();
        let mut rt = Runtime::new(400, 240, Font::blank());
        screens(&mut rt, &mut next);
        let (w, h) = rt.text[0].size();
        let before = cells(&rt, 0);
        // All of screen 0 onto itself, in as large a rectangle as can be
        // asked for.
        for far in [FAR, i32::MAX] {
            let args = ints(&[0, -far, -far, far, far, -far, -far]);
            rt.text_command(Command::Tcopy, &args, Want::Outs(0))
                .unwrap();
            assert_eq!(cells(&rt, 0), before);
        }
        // Off every screen.
        let args = ints(&[0, -FAR, -FAR, FAR, FAR, i32::MAX, i32::MAX]);
        rt.text_command(Command::Tcopy, &args, Want::Outs(0))
            .unwrap();
        assert_eq!(cells(&rt, 0), before);
        // The characters from the rectangle's corner, here the screen's.
        let codes = Value::Str(sb_core::value::new_str(vec![65, 66, 67]));
        let mut args = ints(&[0, 0, 0, i32::MAX, 0]);
        args.push(codes.clone());
        rt.text_command(Command::Tfill, &args, Want::Outs(0))
            .unwrap();
        for x in 0..w {
            assert_eq!(rt.text[0].cell(x, 0).code, 65 + (x % 3) as u16);
        }
        // The string starts at the clipped corner and goes on row by row.
        let mut args = ints(&[0, i32::MIN, i32::MIN, i32::MAX, i32::MAX]);
        args.push(codes);
        rt.text_command(Command::Tfill, &args, Want::Outs(0))
            .unwrap();
        for y in 0..h {
            for x in 0..w {
                let at = y * w + x;
                assert_eq!(u32::from(rt.text[0].cell(x, y).code), 65 + at % 3);
            }
        }
        let args = ints(&[0, i32::MIN, i32::MIN, i32::MAX, i32::MAX]);
        rt.text_command(Command::Tupdate, &args, Want::Outs(0))
            .unwrap();
    }
}

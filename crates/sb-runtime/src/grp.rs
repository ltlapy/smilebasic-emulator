//! Graphic commands: GTARGET, GCOLOR, GCLIP, GCLS, GPGET, GPSET, GLINE,
//! GBOX, GFILL, GCIRCLE, GTRI, GPAINT, GCOPY, GPUTCHR, GSAVE, GLOAD and
//! GSAMPLE.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{ArrayData, ArrayRef, Value};

use crate::font::FontSize;
use crate::graphics::{Blend, Clip, GRP_SIZE, PAGES};
use crate::runtime::{Command, Runtime, illegal, no_form, opt_int};

fn int(v: &Value) -> Result<i32> {
    v.to_int()
}

fn out_of_range(what: &str, n: i32) -> Error {
    Error::with_detail(ErrorKind::OutOfRange, format!("{what} {n}"))
}

fn page_arg(v: &Value) -> Result<usize> {
    let n = int(v)?;
    usize::try_from(n)
        .ok()
        .filter(|&p| p < PAGES)
        .ok_or_else(|| out_of_range("graphic page", n))
}

/// A color code as given (`&HAARRGGBB`).
fn color_arg(v: &Value) -> Result<u32> {
    Ok(int(v)? as u32)
}

fn blend_arg(v: &Value) -> Result<Blend> {
    let n = int(v)?;
    Blend::from_int(n).ok_or_else(|| out_of_range("drawing method", n))
}

pub(crate) fn numeric_array(v: &Value) -> Result<ArrayRef> {
    match v {
        Value::Array(a) if !matches!(a.borrow().data, ArrayData::Str(_)) => Ok(a.clone()),
        _ => Err(ErrorKind::TypeMismatch.into()),
    }
}

/// Writes `values` to the start of a numeric array. Only one-dimensional
/// arrays grow to fit.
pub(crate) fn store_ints(array: &ArrayRef, values: &[i32]) -> Result<()> {
    let mut array = array.borrow_mut();
    if array.len() < values.len() {
        if array.dims.len() != 1 {
            return Err(ErrorKind::SubscriptOutOfRange.into());
        }
        array.dims[0] = values.len();
        match &mut array.data {
            ArrayData::Int(v) => v.resize(values.len(), 0),
            ArrayData::Real(v) => v.resize(values.len(), 0.0),
            ArrayData::Str(_) => unreachable!("checked by numeric_array"),
        }
    }
    match &mut array.data {
        ArrayData::Int(v) => v[..values.len()].copy_from_slice(values),
        ArrayData::Real(v) => {
            for (d, &n) in v.iter_mut().zip(values) {
                *d = f64::from(n);
            }
        }
        ArrayData::Str(_) => unreachable!("checked by numeric_array"),
    }
    Ok(())
}

/// The first `n` elements of a numeric array as color codes.
pub(crate) fn array_colors(array: &ArrayRef, n: usize) -> Result<Vec<u32>> {
    let array = array.borrow();
    if array.len() < n {
        return Err(Error::with_detail(
            ErrorKind::SubscriptOutOfRange,
            format!("{n} elements needed"),
        ));
    }
    Ok(match &array.data {
        ArrayData::Int(v) => v[..n].iter().map(|&c| c as u32).collect(),
        ArrayData::Real(v) => v[..n].iter().map(|&c| c as i64 as u32).collect(),
        ArrayData::Str(_) => unreachable!("checked by numeric_array"),
    })
}

/// A rectangle given as X,Y,width,height. It may reach off the page
/// (checked on hardware: GSAVE reads 0 there), but not be empty or larger
/// than a page.
// UNVERIFIED: the limits on width and height.
fn rect_args(args: &[Value]) -> Result<(i32, i32, i32, i32)> {
    let v: Vec<i32> = args[..4].iter().map(int).collect::<Result<_>>()?;
    let (x, y, w, h) = (v[0], v[1], v[2], v[3]);
    let fits = |len: i32| (1..=GRP_SIZE).contains(&len);
    if fits(w) && fits(h) {
        Ok((x, y, w, h))
    } else {
        Err(Error::with_detail(
            ErrorKind::OutOfRange,
            format!("{x},{y},{w},{h}"),
        ))
    }
}

impl Runtime {
    /// The clipping rectangle as X,Y,width,height.
    fn clip_rect(&self) -> (i32, i32, i32, i32) {
        let c = self.grp.clip;
        (c.x0, c.y0, c.x1 - c.x0 + 1, c.y1 - c.y0 + 1)
    }

    /// The optional color and drawing method that end most commands.
    fn color_and_blend(&self, rest: &[Value]) -> Result<(u32, Blend)> {
        let color = match rest.first().map(opt_int).transpose()?.flatten() {
            Some(c) => c as u32,
            None => self.grp.color,
        };
        let blend = match rest.get(1) {
            None | Some(Value::Default) => Blend::Normal,
            Some(v) => blend_arg(v)?,
        };
        Ok((color, blend))
    }

    pub(crate) fn grp_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        let ints = |n: usize| -> Result<Vec<i32>> { args[..n].iter().map(int).collect() };
        match (command, want, args.len()) {
            (Command::Gtarget, Want::Value, 0) => Ok(vec![Value::Int(self.grp.target as i32)]),
            (Command::Gtarget, Want::Outs(0), 1) => {
                self.grp.target = page_arg(&args[0])?;
                Ok(vec![])
            }
            (Command::Gcolor, Want::Value, 0) => Ok(vec![Value::Int(self.grp.color as i32)]),
            (Command::Gcolor, Want::Outs(0), 1) => {
                self.grp.color = color_arg(&args[0])?;
                Ok(vec![])
            }
            (Command::Gclip, Want::Outs(0), 0) => {
                self.grp.clip = Clip::PAGE;
                Ok(vec![])
            }
            (Command::Gclip, Want::Outs(0), 4) => {
                let v = ints(4)?;
                self.grp.clip = Clip::new(v[0], v[1], v[2], v[3]);
                Ok(vec![])
            }
            (Command::Gclip, Want::Outs(4), 0) => {
                let c = self.grp.clip;
                Ok([c.x0, c.y0, c.x1, c.y1].map(Value::Int).to_vec())
            }
            (Command::Gcls, Want::Outs(0), 0 | 1) => {
                let color = args.first().map(opt_int).transpose()?.flatten();
                self.grp.cls(color.unwrap_or(0) as u32);
                Ok(vec![])
            }
            (Command::Gpget, Want::Value, 2) => {
                let v = ints(2)?;
                let p = self.grp.pixel(self.grp.target, v[0], v[1]);
                Ok(vec![Value::Int(p as i32)])
            }
            (Command::Gpset, Want::Outs(0), 2..=4) => {
                let v = ints(2)?;
                let (color, blend) = self.color_and_blend(&args[2..])?;
                self.grp.pset(v[0], v[1], color, blend);
                Ok(vec![])
            }
            (Command::Gline | Command::Gbox | Command::Gfill, Want::Outs(0), 4..=6) => {
                let v = ints(4)?;
                let (color, blend) = self.color_and_blend(&args[4..])?;
                let draw = match command {
                    Command::Gline => crate::graphics::Graphics::line,
                    Command::Gbox => crate::graphics::Graphics::rect,
                    _ => crate::graphics::Graphics::fill,
                };
                draw(&mut self.grp, v[0], v[1], v[2], v[3], color, blend);
                Ok(vec![])
            }
            (Command::Gcircle, Want::Outs(0), 3..=5) => {
                let v = ints(3)?;
                let (color, blend) = self.color_and_blend(&args[3..])?;
                self.grp.circle((v[0], v[1], v[2]), None, color, blend);
                Ok(vec![])
            }
            (Command::Gcircle, Want::Outs(0), 6..=8) => {
                let v = ints(6)?;
                let sector = match v[5] {
                    0 => false,
                    1 => true,
                    n => return Err(out_of_range("GCIRCLE flag", n)),
                };
                let (color, blend) = self.color_and_blend(&args[6..])?;
                let arc = Some((v[3], v[4], sector));
                self.grp.circle((v[0], v[1], v[2]), arc, color, blend);
                Ok(vec![])
            }
            (Command::Gtri, Want::Outs(0), 6..=8) => {
                let v = ints(6)?;
                let (color, blend) = self.color_and_blend(&args[6..])?;
                let points = [(v[0], v[1]), (v[2], v[3]), (v[4], v[5])];
                self.grp.triangle(points, color, blend);
                Ok(vec![])
            }
            (Command::Gpaint, Want::Outs(0), 2..=4) => {
                let v = ints(2)?;
                let color = match args.get(2).map(opt_int).transpose()?.flatten() {
                    Some(c) => c as u32,
                    None => self.grp.color,
                };
                let border = args.get(3).map(color_arg).transpose()?;
                self.grp.paint(v[0], v[1], color, border);
                Ok(vec![])
            }
            (Command::Gcopy, Want::Outs(0), 7..=9) => {
                let (src, rest) = match args.len() {
                    7 => (self.grp.target, args),
                    _ => (page_arg(&args[0])?, &args[1..]),
                };
                let v: Vec<i32> = rest[..6].iter().map(int).collect::<Result<_>>()?;
                let (tint, blend) = match &rest[6..] {
                    [blend] => (None, blend_arg(blend)?),
                    [tint, blend] => (Some(color_arg(tint)?), blend_arg(blend)?),
                    _ => unreachable!("7 to 9 arguments"),
                };
                let from = (v[0], v[1], v[2], v[3]);
                self.grp.copy(src, from, (v[4], v[5]), tint, blend);
                Ok(vec![])
            }
            (Command::Gputchr, Want::Outs(0), 3..=8) => {
                let v = ints(2)?;
                let text = match &args[2] {
                    Value::Str(s) => s.borrow().clone(),
                    // A number stands for one character code.
                    other => vec![int(other)? as u16],
                };
                let size = match args.get(3).map(opt_int).transpose()?.flatten() {
                    None | Some(16) => FontSize::Large,
                    Some(8) => FontSize::Small,
                    Some(n) => return Err(out_of_range("font size", n)),
                };
                let (scale, rest) = match args.len() {
                    7 | 8 => {
                        let (sx, sy) = (int(&args[4])?, int(&args[5])?);
                        for s in [sx, sy] {
                            if !(1..=32767).contains(&s) {
                                return Err(out_of_range("scale", s));
                            }
                        }
                        ((sx, sy), &args[6..])
                    }
                    _ => ((1, 1), args.get(4..).unwrap_or(&[])),
                };
                let (color, blend) = self.color_and_blend(rest)?;
                let font = &self.font;
                self.grp
                    .put_chars(font, (v[0], v[1]), &text, size, scale, color, blend);
                Ok(vec![])
            }
            (Command::Gsave, Want::Outs(0), 1 | 2 | 5 | 6) => {
                let (page, rest) = match args.len() {
                    2 | 6 => (page_arg(&args[0])?, &args[1..]),
                    _ => (self.grp.target, args),
                };
                let rect = match rest.len() {
                    5 => rect_args(rest)?,
                    _ => self.clip_rect(),
                };
                let array = numeric_array(rest.last().expect("an array"))?;
                let pixels: Vec<i32> = (self.grp.save_rect(page, rect).into_iter())
                    .map(|p| p as i32)
                    .collect();
                store_ints(&array, &pixels)?;
                Ok(vec![])
            }
            (Command::Gload, Want::Outs(0), 2..=4 | 6..=8) => {
                let (rect, rest) = match args.len() {
                    6..=8 => (rect_args(args)?, &args[4..]),
                    _ => (self.clip_rect(), args),
                };
                let palette = match rest {
                    [_, Value::Array(p), ..] => Some(numeric_array(&Value::Array(p.clone()))?),
                    _ => None,
                };
                let (blend, tint) = match (rest.len(), palette.is_some()) {
                    (2, false) | (3, true) => (blend_arg(rest.last().unwrap())?, None),
                    (3, false) => (blend_arg(&rest[2])?, Some(color_arg(&rest[1])?)),
                    (4, true) => (blend_arg(&rest[3])?, Some(color_arg(&rest[2])?)),
                    _ => return Err(illegal()),
                };
                let n = (rect.2 * rect.3) as usize;
                let mut pixels = array_colors(&numeric_array(&rest[0])?, n)?;
                if let Some(palette) = palette {
                    let len = palette.borrow().len();
                    let colors = array_colors(&palette, len)?;
                    // Indexes past the palette are no error (checked on
                    // hardware). UNVERIFIED: that they draw transparent.
                    for p in &mut pixels {
                        *p = colors.get(*p as usize).copied().unwrap_or(0);
                    }
                }
                self.grp.load_rect(rect, &pixels, tint, blend);
                Ok(vec![])
            }
            (Command::Gsample, Want::Value, 1) => {
                Ok(vec![Value::Int(self.grp.sample[page_arg(&args[0])?])])
            }
            (Command::Gsample, Want::Outs(0), 2) => {
                let page = page_arg(&args[0])?;
                self.grp.sample[page] = match int(&args[1])? {
                    n @ (0 | 1) => n,
                    n => return Err(out_of_range("sampling type", n)),
                };
                Ok(vec![])
            }
            _ => Err(no_form()),
        }
    }
}

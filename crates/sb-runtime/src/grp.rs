//! Graphic commands: GTARGET, GCOLOR, GCLIP, GCLS, GPGET, GPSET, GLINE,
//! GBOX, GFILL, GCIRCLE, GTRI, GPAINT, GCOPY, GPUTCHR, GSAVE, GLOAD,
//! GSAMPLE, GARRAY and GUPDATE.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{ArrayData, ArrayRef, Value};

use crate::font::FontSize;
use crate::graphics::{Blend, Clip, GRP_SIZE, PAGES};
use crate::runtime::{Command, Runtime, checking, illegal, many, no_form, one, opt_int};
use crate::text::CONSOLE;

/// The first `N` arguments as integers, in order; the first that is not
/// one fails. They come in an array because a list of a few numbers would
/// take one allocation per call.
fn ints<const N: usize>(args: &[Value]) -> Result<[i32; N]> {
    let mut v = [0; N];
    for (d, a) in v.iter_mut().zip(&args[..N]) {
        *d = int(a)?;
    }
    Ok(v)
}

fn int(v: &Value) -> Result<i32> {
    v.to_int()
}

fn out_of_range(what: &str, n: i32) -> Error {
    Error::with_detail(ErrorKind::OutOfRange, format!("{what} {n}"))
}

/// A point on a graphic page, 0 to 2047, as GCLIP and GUPDATE take it.
fn coordinate(v: &Value) -> Result<i32> {
    checking(v, || match int(v)? {
        n if (0..GRP_SIZE).contains(&n) => Ok(n),
        n => Err(out_of_range("coordinate", n)),
    })
}

fn page_arg(v: &Value) -> Result<usize> {
    checking(v, || {
        let n = int(v)?;
        usize::try_from(n)
            .ok()
            .filter(|&p| p < PAGES)
            .ok_or_else(|| out_of_range("graphic page", n))
    })
}

/// A color code as given (`&HAARRGGBB`).
fn color_arg(v: &Value) -> Result<u32> {
    Ok(int(v)? as u32)
}

fn blend_arg(v: &Value) -> Result<Blend> {
    checking(v, || {
        let n = int(v)?;
        Blend::from_int(n).ok_or_else(|| out_of_range("drawing method", n))
    })
}

pub(crate) fn numeric_array(v: &Value) -> Result<ArrayRef> {
    checking(v, || match v {
        Value::Array(a) if !matches!(a.borrow().data, ArrayData::Str(_)) => Ok(a.clone()),
        _ => Err(ErrorKind::TypeMismatch.into()),
    })
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

/// The most pixels a GSAVE or GLOAD rectangle may have: as many 4-byte
/// elements as SmileBASIC 4's memory holds.
const MOST_PIXELS: i64 = sb_core::memory::TOTAL as i64 / 4;

/// A rectangle given as X,Y,width,height. It may reach off the page, be
/// wider and taller than it, and be empty (GSAVE reads 0 off the page,
/// GLOAD drops the pixels that fall off it).
// A rectangle with more pixels than memory holds here is Out of range too,
// standing in for SmileBASIC 4's Out of memory.
fn rect_args(args: &[Value]) -> Result<(i32, i32, i32, i32)> {
    let v = ints::<4>(args)?;
    let (x, y, w, h) = (v[0], v[1], v[2], v[3]);
    if w >= 0 && h >= 0 && i64::from(w) * i64::from(h) <= MOST_PIXELS {
        return Ok((x, y, w, h));
    }
    let e = Error::with_detail(ErrorKind::OutOfRange, format!("{x},{y},{w},{h}"));
    // A negative width or height is named by its argument (GLOAD:3 for
    // the width); an error for too many pixels in total names none.
    // UNVERIFIED: GSAVE is taken to number them the same way, and a
    // negative height too.
    Err(match (w < 0, h < 0) {
        (true, _) => e.of_value(&args[2]),
        (_, true) => e.of_value(&args[3]),
        _ => e,
    })
}

impl Runtime {
    /// The clipping rectangle as X,Y,width,height.
    fn clip_rect(&self) -> (i32, i32, i32, i32) {
        let c = self.grp.clip;
        (c.x0, c.y0, c.x1 - c.x0 + 1, c.y1 - c.y0 + 1)
    }

    /// A color argument, or GCOLOR when it is omitted.
    fn color_or_gcolor(&self, arg: Option<&Value>) -> Result<u32> {
        let color = arg.map(opt_int).transpose()?.flatten();
        Ok(color.map_or(self.grp.color, |c| c as u32))
    }

    /// The optional color and drawing method that end most commands.
    fn color_and_blend(&self, rest: &[Value]) -> Result<(u32, Blend)> {
        let color = self.color_or_gcolor(rest.first())?;
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
        out: &mut Vec<Value>,
    ) -> Result<()> {
        match (command, want, args.len()) {
            (Command::Gtarget, Want::Value, 0) => one(out, Value::Int(self.grp.target as i32)),
            (Command::Gtarget, Want::Outs(0), 1) => {
                self.grp.target = page_arg(&args[0])?;
                Ok(())
            }
            (Command::Gcolor, Want::Value, 0) => one(out, Value::Int(self.grp.color as i32)),
            (Command::Gcolor, Want::Outs(0), 1) => {
                self.grp.color = color_arg(&args[0])?;
                Ok(())
            }
            (Command::Gclip, Want::Outs(0), 0) => {
                self.grp.clip = Clip::PAGE;
                Ok(())
            }
            (Command::Gclip, Want::Outs(0), 4) => {
                let v = args.iter().map(coordinate).collect::<Result<Vec<_>>>()?;
                self.grp.clip = Clip::new(v[0], v[1], v[2], v[3]);
                Ok(())
            }
            (Command::Gclip, Want::Outs(4), 0) => {
                let c = self.grp.clip;
                many(out, [c.x0, c.y0, c.x1, c.y1].map(Value::Int))
            }
            (Command::Gcls, Want::Outs(0), 0 | 1) => {
                let color = args.first().map(opt_int).transpose()?.flatten();
                self.grp.cls(color.unwrap_or(0) as u32);
                Ok(())
            }
            (Command::Gpget, Want::Value, 2) => {
                let v = ints::<2>(args)?;
                let p = self.grp.read(self.grp.target, v[0], v[1]);
                one(out, Value::Int(p as i32))
            }
            (Command::Gpset, Want::Outs(0), 2..=4) => {
                let v = ints::<2>(args)?;
                let (color, blend) = self.color_and_blend(&args[2..])?;
                self.grp.pset(v[0], v[1], color, blend);
                Ok(())
            }
            (Command::Gline | Command::Gbox | Command::Gfill, Want::Outs(0), 4..=6) => {
                let v = ints::<4>(args)?;
                let (color, blend) = self.color_and_blend(&args[4..])?;
                let draw = match command {
                    Command::Gline => crate::graphics::Graphics::line,
                    Command::Gbox => crate::graphics::Graphics::rect,
                    _ => crate::graphics::Graphics::fill,
                };
                draw(&mut self.grp, v[0], v[1], v[2], v[3], color, blend);
                Ok(())
            }
            (Command::Gcircle, Want::Outs(0), 3..=5) => {
                let v = ints::<3>(args)?;
                let (color, blend) = self.color_and_blend(&args[3..])?;
                self.grp.circle((v[0], v[1], v[2]), None, color, blend);
                Ok(())
            }
            (Command::Gcircle, Want::Outs(0), 6..=8) => {
                let v = ints::<6>(args)?;
                let sector = match v[5] {
                    0 => false,
                    1 => true,
                    n => return Err(out_of_range("GCIRCLE flag", n)),
                };
                let (color, blend) = self.color_and_blend(&args[6..])?;
                let arc = Some((v[3], v[4], sector));
                self.grp.circle((v[0], v[1], v[2]), arc, color, blend);
                Ok(())
            }
            (Command::Gtri, Want::Outs(0), 6..=8) => {
                let v = ints::<6>(args)?;
                let (color, blend) = self.color_and_blend(&args[6..])?;
                let points = [(v[0], v[1]), (v[2], v[3]), (v[4], v[5])];
                self.grp.triangle(points, color, blend);
                Ok(())
            }
            (Command::Gpaint, Want::Outs(0), 2..=4) => {
                let v = ints::<2>(args)?;
                let color = self.color_or_gcolor(args.get(2))?;
                let border = args.get(3).map(color_arg).transpose()?;
                let filled = self.grp.paint(v[0], v[1], color, border);
                if let Some(meter) = &mut self.meter {
                    meter.charge_paint(filled);
                }
                Ok(())
            }
            (Command::Gcopy, Want::Outs(0), 7..=9) => {
                let (src, rest) = match args.len() {
                    7 => (self.grp.target, args),
                    _ => (page_arg(&args[0])?, &args[1..]),
                };
                let v = ints::<6>(rest)?;
                let (tint, blend) = match &rest[6..] {
                    [blend] => (None, blend_arg(blend)?),
                    [tint, blend] => (Some(color_arg(tint)?), blend_arg(blend)?),
                    _ => unreachable!("7 to 9 arguments"),
                };
                let from = (v[0], v[1], v[2], v[3]);
                self.grp.copy(src, from, (v[4], v[5]), tint, blend);
                Ok(())
            }
            (Command::Gputchr, Want::Outs(0), 3..=8) => {
                let v = ints::<2>(args)?;
                let text = chars_arg(&args[2])?;
                // When omitted, it is the console's font type, 24 and up
                // too (checked on hardware after TSCREEN 24).
                let px = match args.get(3).map(opt_int).transpose()?.flatten() {
                    None => self.text[CONSOLE].font().0,
                    Some(_) => font_size_arg(args.get(3))? as u32,
                };
                let (scale, rest) = match args.len() {
                    7 | 8 => (scale_args(&args[4], &args[5])?, &args[6..]),
                    _ => ((1, 1), args.get(4..).unwrap_or(&[])),
                };
                let (color, blend) = self.color_and_blend(rest)?;
                let (font, user) = (&self.font, self.text[CONSOLE].user_page());
                self.grp
                    .put_chars(font, user, (v[0], v[1]), &text, px, scale, color, blend);
                Ok(())
            }
            // GPUTCHRP X,Y,text[,font[,color[,method]]] or
            // X,Y,text,font,scale X,scale Y,color[,method[,spacing]]:
            // proportional characters, drawn over transparent pixels by
            // default.
            (Command::Gputchrp, Want::Outs(0), 3..=9) => {
                let v = ints::<2>(args)?;
                let text = chars_arg(&args[2])?;
                let size = font_size_arg(args.get(3))?;
                let (scale, rest) = match args.len() {
                    7..=9 => (scale_args(&args[4], &args[5])?, &args[6..]),
                    3..=6 => ((1, 1), args.get(4..).unwrap_or(&[])),
                    _ => return Err(no_form()),
                };
                let color = self.color_or_gcolor(rest.first())?;
                let blend = match rest.get(1) {
                    None | Some(Value::Default) => Blend::Normal2,
                    Some(v) => blend_arg(v)?,
                };
                let spacing = spacing_arg(rest.get(2))?;
                let (font, user) = (&self.font, self.text[CONSOLE].user_page());
                self.grp.put_chars_proportional(
                    font,
                    user,
                    (v[0], v[1]),
                    &text,
                    size,
                    scale,
                    color,
                    blend,
                    spacing,
                );
                Ok(())
            }
            // GPUTCHRP(text[,font[,scale X[,spacing]]]): the width drawn.
            (Command::Gputchrp, Want::Value, 1..=4) => {
                let text = chars_arg(&args[0])?;
                let size = font_size_arg(args.get(1))?;
                let sx = match args.get(2).map(opt_int).transpose()?.flatten() {
                    None => 1,
                    Some(s) => scale_args(&Value::Int(s), &Value::Int(1))?.0,
                };
                let spacing = spacing_arg(args.get(3))?;
                let width = self
                    .grp
                    .proportional_width(&self.font, &text, size, sx, spacing);
                one(out, Value::Int(width))
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
                let pixels = self.grp.save_rect(page, rect);
                let as_ints = || pixels.iter().map(|&p| p as i32).collect::<Vec<_>>();
                store_ints(&array, &as_ints())?;
                // A real array takes the colors unsigned, &HFF000000 as
                // 4278190080.
                if let ArrayData::Real(v) = &mut array.borrow_mut().data {
                    for (d, &p) in v.iter_mut().zip(&pixels) {
                        *d = f64::from(p);
                    }
                }
                Ok(())
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
                let n = (i64::from(rect.2) * i64::from(rect.3)) as usize;
                let source = numeric_array(&rest[0])?;
                // An integer array can be read directly. The page's own
                // GARRAY array must be copied, because drawing writes to it.
                if palette.is_none() && self.grp.load_direct() {
                    let source = source.borrow();
                    if let ArrayData::Int(v) = &source.data
                        && let Some(v) = v.get(..n)
                    {
                        self.grp.load_ints(rect, v, tint, blend);
                        return Ok(());
                    }
                }
                let pixels = array_colors(&source, n)?;
                // Indexes past the palette are no error (checked on
                // hardware).
                let colors = match palette {
                    Some(palette) => {
                        let len = palette.borrow().len();
                        Some(array_colors(&palette, len)?)
                    }
                    None => None,
                };
                self.grp
                    .load_rect(rect, &pixels, colors.as_deref(), tint, blend);
                Ok(())
            }
            (Command::Gsample, Want::Value, 1) => {
                one(out, Value::Int(self.grp.sample[page_arg(&args[0])?]))
            }
            (Command::Gsample, Want::Outs(0), 2) => {
                let page = page_arg(&args[0])?;
                self.grp.sample[page] = match int(&args[1])? {
                    n @ (0 | 1) => n,
                    n => return Err(out_of_range("sampling type", n)),
                };
                Ok(())
            }
            (Command::Garray, Want::Value, 1) => {
                let page = page_arg(&args[0])?;
                one(out, Value::Array(self.grp.array(page)))
            }
            // The array's writes show on screen after GUPDATE: on
            // hardware, drawing near them shows them too.
            (Command::Gupdate, Want::Outs(0), 1 | 5) => {
                let page = page_arg(&args[0])?;
                let rect = match &args[1..] {
                    [] => Clip::PAGE,
                    corners => {
                        let c = corners.iter().map(coordinate).collect::<Result<Vec<_>>>()?;
                        Clip::new(c[0], c[1], c[2], c[3])
                    }
                };
                self.grp.update(page, rect);
                Ok(())
            }
            _ => Err(no_form()),
        }
    }
}

/// The text of GPUTCHR and GPUTCHRP: a string, or a number for one character.
fn chars_arg(v: &Value) -> Result<Vec<u16>> {
    checking(v, || {
        Ok(match v {
            Value::Str(s) => s.borrow().clone(),
            other => vec![int(other)? as u16],
        })
    })
}

/// A font type of 8 or 16, 16 when omitted.
fn font_size_arg(v: Option<&Value>) -> Result<FontSize> {
    match v.map(opt_int).transpose()?.flatten() {
        None | Some(16) => Ok(FontSize::Large),
        Some(8) => Ok(FontSize::Small),
        Some(n) => Err(Error::with_detail(
            ErrorKind::IllegalFontType,
            format!("{n}"),
        )),
    }
}

/// Scales of 1 to 32767.
fn scale_args(x: &Value, y: &Value) -> Result<(i32, i32)> {
    let scale = |v: &Value| {
        checking(v, || match int(v)? {
            s @ 1..=32767 => Ok(s),
            s => Err(out_of_range("scale", s)),
        })
    };
    Ok((scale(x)?, scale(y)?))
}

/// GPUTCHRP's space between characters: 0 to 64 pixels, 1 when omitted.
fn spacing_arg(v: Option<&Value>) -> Result<i32> {
    match v.map(opt_int).transpose()?.flatten() {
        None => Ok(1),
        Some(n @ 0..=64) => Ok(n),
        Some(n) => Err(out_of_range("spacing", n)),
    }
}

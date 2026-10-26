//! Graphic commands: GTARGET, GCOLOR, GCLIP, GCLS, GPGET, GPSET, GLINE,
//! GBOX, GFILL, GCIRCLE, GTRI, GPAINT, GCOPY and GPUTCHR.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::Value;

use crate::font::FontSize;
use crate::graphics::{Blend, Clip, PAGES};
use crate::runtime::{Command, Runtime, illegal, opt_int};

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

impl Runtime {
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
            _ => Err(illegal()),
        }
    }
}

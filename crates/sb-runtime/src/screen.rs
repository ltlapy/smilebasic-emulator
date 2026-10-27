//! Placing text screens (TOFS, THOME, TROT, TSCALE, TCOLOR, TBLEND, TSHOW,
//! THIDE, TLAYER), the layers they and sprites belong to (LAYER, LCLIP,
//! LMATRIX), BACKCOLOR, what text screens show (TSCREEN, TPAGE and
//! ATTR), the resolution (XSCREEN), ACLS, and FADE over everything.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::Value;

use crate::compose::{Affine, Compose, LAYERS};
use crate::graphics::Clip;
use crate::runtime::{Command, Runtime, illegal, opt_int, screen_id, with_screen};
use crate::text::CONSOLE;

/// XSCREEN: the screen's resolution and how it is shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenMode {
    pub width: u32,
    pub height: u32,
    /// Each pixel is drawn as `sample` x `sample` finer ones, so that
    /// turned and scaled elements look smoother.
    pub sample: u32,
    pub filter: Filter,
    /// Width over height on the display; `None` for square pixels.
    pub aspect: Option<f64>,
}

impl ScreenMode {
    /// `width` x `height` with square pixels, sampled once each and
    /// shown with bilinear filtering, as XSCREEN's defaults are.
    pub fn new(width: u32, height: u32) -> ScreenMode {
        ScreenMode {
            width,
            height,
            sample: 1,
            filter: Filter::Bilinear,
            aspect: None,
        }
    }

    /// The frame's size in its finer pixels.
    pub fn frame(&self) -> (u32, u32) {
        (self.width * self.sample, self.height * self.sample)
    }

    pub fn aspect(&self) -> f64 {
        self.aspect
            .unwrap_or(f64::from(self.width) / f64::from(self.height))
    }
}

/// How the frame is scaled to the display: XSCREEN's composite mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Bilinear,
    SmartNearest,
    Nearest,
}

impl Filter {
    fn from_int(n: i32) -> Option<Filter> {
        [Filter::Bilinear, Filter::SmartNearest, Filter::Nearest]
            .get(usize::try_from(n).ok()?)
            .copied()
    }
}

fn real(v: &Value) -> Result<f64> {
    v.to_real()
}

fn reals(values: &[f64]) -> Vec<Value> {
    values.iter().map(|&x| Value::Real(x)).collect()
}

fn int_value(n: i32) -> Vec<Value> {
    vec![Value::Int(n)]
}

fn layer_id(v: &Value) -> Result<usize> {
    let n = v.to_int()?;
    usize::try_from(n)
        .ok()
        .filter(|&n| n < LAYERS)
        .ok_or_else(|| Error::with_detail(ErrorKind::OutOfRange, format!("layer {n}")))
}

fn out_of_range(what: &str, n: i32) -> Error {
    Error::with_detail(ErrorKind::OutOfRange, format!("{what} {n}"))
}

/// TSCREEN's font type: 8 to 64 in steps of 8.
fn font_type(v: &Value) -> Result<u32> {
    let n = v.to_int()?;
    if (8..=64).contains(&n) && n % 8 == 0 {
        Ok(n as u32)
    } else {
        Err(out_of_range("font type", n))
    }
}

/// FADE: a color over the whole screen, and its way to another one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Fade {
    pub(crate) color: u32,
    from: u32,
    to: u32,
    /// Frames the change takes, and those gone; equal when it is done.
    frames: u32,
    elapsed: u32,
}

impl Fade {
    fn start(&mut self, to: u32, frames: u32) {
        *self = Fade {
            color: if frames == 0 { to } else { self.color },
            from: self.color,
            to,
            frames,
            elapsed: 0,
        };
    }

    pub(crate) fn fading(&self) -> bool {
        self.elapsed < self.frames
    }

    /// One frame of the change, each channel moving in a straight line.
    // UNVERIFIED: how the channels are rounded on the way.
    pub(crate) fn step(&mut self) {
        if !self.fading() {
            return;
        }
        self.elapsed += 1;
        let (t, n) = (i64::from(self.elapsed), i64::from(self.frames));
        self.color = (0..4).fold(0, |c, i| {
            let shift = i * 8;
            let a = i64::from(self.from >> shift & 0xFF);
            let b = i64::from(self.to >> shift & 0xFF);
            c | ((a + (b - a) * t / n) as u32) << shift
        });
    }

    /// Covers `pixels`, which are opaque, with the fade color.
    pub(crate) fn cover(&self, pixels: &mut [u32]) {
        let a = self.color >> 24;
        if a == 0 {
            return;
        }
        let mix = |p: u32, c: u32, shift: u32| {
            let (p, c) = (p >> shift & 0xFF, c >> shift & 0xFF);
            ((c * a + p * (255 - a) + 127) / 255) << shift
        };
        for p in pixels {
            *p = 0xFF00_0000
                | mix(*p, self.color, 16)
                | mix(*p, self.color, 8)
                | mix(*p, self.color, 0);
        }
    }
}

fn char_size(v: &Value) -> Result<u32> {
    let n = v.to_int()?;
    if (8..=64).contains(&n) {
        Ok(n as u32)
    } else {
        Err(out_of_range("character size", n))
    }
}

fn z(v: &Value) -> Result<i32> {
    let z = v.to_int()?;
    if (-4095..=4095).contains(&z) {
        Ok(z)
    } else {
        Err(Error::with_detail(ErrorKind::OutOfRange, format!("Z {z}")))
    }
}

impl Runtime {
    pub(crate) fn screen_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        match (command, want, args.len()) {
            (Command::Tofs, Want::Outs(0), 3 | 4) => {
                let (x, y) = (real(&args[1])?, real(&args[2])?);
                let z = args.get(3).map(opt_int).transpose()?.flatten();
                let z = z.map(|n| self::z(&Value::Int(n))).transpose()?;
                let t = &mut self.text[screen_id(&args[0])?];
                t.pos = (x, y);
                t.z = z.unwrap_or(t.z);
                Ok(vec![])
            }
            (Command::Tofs, Want::Outs(n @ (2 | 3)), 1) => {
                let t = &self.text[screen_id(&args[0])?];
                let mut out = reals(&[t.pos.0, t.pos.1]);
                if n == 3 {
                    out.push(Value::Int(t.z));
                }
                Ok(out)
            }
            (Command::Thome, Want::Outs(0), 3) => {
                let home = (real(&args[1])?, real(&args[2])?);
                self.text[screen_id(&args[0])?].home = home;
                Ok(vec![])
            }
            (Command::Thome, Want::Outs(2), 1) => {
                let (x, y) = self.text[screen_id(&args[0])?].home;
                Ok(reals(&[x, y]))
            }
            (Command::Trot, Want::Outs(0), 2) => {
                let angle = real(&args[1])?;
                self.text[screen_id(&args[0])?].rot = angle;
                Ok(vec![])
            }
            (Command::Trot, Want::Value, 1) => {
                Ok(vec![Value::Real(self.text[screen_id(&args[0])?].rot)])
            }
            (Command::Tscale, Want::Outs(0), 3) => {
                let scale = (real(&args[1])?, real(&args[2])?);
                self.text[screen_id(&args[0])?].scale = scale;
                Ok(vec![])
            }
            (Command::Tscale, Want::Outs(2), 1) => {
                let (x, y) = self.text[screen_id(&args[0])?].scale;
                Ok(reals(&[x, y]))
            }
            (Command::Tcolor, Want::Outs(0), 2) => {
                let color = args[1].to_int()? as u32;
                self.text[screen_id(&args[0])?].tint = color;
                Ok(vec![])
            }
            (Command::Tcolor, Want::Value, 1) => {
                Ok(int_value(self.text[screen_id(&args[0])?].tint as i32))
            }
            (Command::Tblend, Want::Outs(0), 2) => {
                let add = match args[1].to_int()? {
                    0 => false,
                    1 => true,
                    n => {
                        return Err(Error::with_detail(
                            ErrorKind::OutOfRange,
                            format!("blend {n}"),
                        ));
                    }
                };
                self.text[screen_id(&args[0])?].add = add;
                Ok(vec![])
            }
            (Command::Tblend, Want::Value, 1) => {
                Ok(int_value(i32::from(self.text[screen_id(&args[0])?].add)))
            }
            (Command::Tshow | Command::Thide, Want::Outs(0), 1) => {
                self.text[screen_id(&args[0])?].visible = command == Command::Tshow;
                Ok(vec![])
            }
            (Command::Tshow | Command::Thide, Want::Value, 1) => {
                let shown = self.text[screen_id(&args[0])?].visible;
                Ok(int_value(i32::from(shown == (command == Command::Tshow))))
            }
            (Command::Tlayer, Want::Outs(0), 2) => {
                let layer = layer_id(&args[1])? as i32;
                self.text[screen_id(&args[0])?].layer = layer;
                Ok(vec![])
            }
            (Command::Tlayer, Want::Value, 1) => {
                Ok(int_value(self.text[screen_id(&args[0])?].layer))
            }
            (Command::Layer, Want::Outs(0), 1..=3) => {
                let id = layer_id(&args[0])?;
                let mode = args.get(1).map(opt_int).transpose()?.flatten().unwrap_or(0);
                let compose = Compose::from_int(mode).ok_or_else(|| {
                    Error::with_detail(ErrorKind::OutOfRange, format!("composition {mode}"))
                })?;
                let color = args.get(2).map(opt_int).transpose()?.flatten();
                let l = &mut self.layers[id];
                l.compose = compose;
                l.color = color.map_or(0xFFFF_FFFF, |c| c as u32);
                Ok(vec![])
            }
            (Command::Lclip, Want::Outs(0), 1) => {
                self.layers[layer_id(&args[0])?].clip = None;
                Ok(vec![])
            }
            (Command::Lclip, Want::Outs(0), 5) => {
                let id = layer_id(&args[0])?;
                let n: Vec<i32> = args[1..].iter().map(Value::to_int).collect::<Result<_>>()?;
                self.layers[id].clip = Some(Clip {
                    x0: n[0].min(n[2]),
                    y0: n[1].min(n[3]),
                    x1: n[0].max(n[2]),
                    y1: n[1].max(n[3]),
                });
                Ok(vec![])
            }
            (Command::Lclip, Want::Outs(4), 1) => {
                let (w, h) = self.frame_size();
                let c = self.layers[layer_id(&args[0])?].clip.unwrap_or(Clip {
                    x0: 0,
                    y0: 0,
                    x1: w as i32 - 1,
                    y1: h as i32 - 1,
                });
                Ok([c.x0, c.y0, c.x1, c.y1].map(Value::Int).to_vec())
            }
            (Command::Lmatrix, Want::Outs(0), 1) => {
                self.layers[layer_id(&args[0])?].matrix = Affine::IDENTITY;
                Ok(vec![])
            }
            // TODO: 4x4 matrices with projection.
            (Command::Lmatrix, Want::Outs(0), 2) => Err(Error::with_detail(
                ErrorKind::IllegalFunctionCall,
                "4x4 not supported yet",
            )),
            (Command::Lmatrix, Want::Outs(0), 3 | 5 | 7 | 8) => {
                let id = layer_id(&args[0])?;
                let n = |i: usize, default: f64| args.get(i).map_or(Ok(default), real);
                self.layers[id].matrix = Affine::place(
                    (n(3, 0.0)?, n(4, 0.0)?),
                    (n(1, 0.0)?, n(2, 0.0)?),
                    n(7, 0.0)?,
                    (n(5, 1.0)?, n(6, 1.0)?),
                );
                Ok(vec![])
            }
            (Command::Tscreen, Want::Outs(0), 1..=5) => {
                let (id, ft, cs, size) = match args {
                    [ft] => (CONSOLE, font_type(ft)?, None, None),
                    [ft, cs] => (CONSOLE, font_type(ft)?, Some(char_size(cs)?), None),
                    [id, ft, cs] => (screen_id(id)?, font_type(ft)?, Some(char_size(cs)?), None),
                    [id, ft, w, h] => (screen_id(id)?, font_type(ft)?, None, Some((w, h))),
                    [id, ft, cs, w, h] => (
                        screen_id(id)?,
                        font_type(ft)?,
                        Some(char_size(cs)?),
                        Some((w, h)),
                    ),
                    _ => unreachable!("one to five"),
                };
                let cs = cs.unwrap_or(ft);
                let (fw, fh) = self.frame_size();
                // 0 or omitted: as many characters as fill the frame.
                let fill = |v: Option<&Value>, frame: u32| -> Result<u32> {
                    match v.map(Value::to_int).transpose()?.unwrap_or(0) {
                        0 => Ok(frame / cs),
                        n if n > 0 => Ok(n as u32),
                        n => Err(out_of_range("text screen size", n)),
                    }
                };
                let w = fill(size.map(|s| s.0), fw)?;
                let h = fill(size.map(|s| s.1), fh)?;
                if w * h > 32768 {
                    return Err(Error::with_detail(
                        ErrorKind::OutOfRange,
                        format!("{w}x{h} characters"),
                    ));
                }
                self.text[id].set_font(ft, cs, w, h);
                Ok(vec![])
            }
            (Command::Tscreen, Want::Outs(4), 0 | 1) => {
                let id = args.first().map(screen_id).transpose()?.unwrap_or(CONSOLE);
                let t = &self.text[id];
                let ((ft, cs), (w, h)) = (t.font(), t.size());
                Ok([ft, cs, w, h].map(|n| Value::Int(n as i32)).to_vec())
            }
            (Command::Tpage, Want::Outs(0), 3 | 4) => {
                let (id, rest) = with_screen(args, 3)?;
                let page = rest[0].to_int()?;
                if !(-1..=5).contains(&page) {
                    return Err(out_of_range("graphic page", page));
                }
                let origin = (rest[1].to_int()?, rest[2].to_int()?);
                let t = &mut self.text[id];
                (t.page, t.page_origin) = (page, origin);
                Ok(vec![])
            }
            (Command::Tpage, Want::Outs(3), 0 | 1) => {
                let (id, _) = with_screen(args, 0)?;
                let t = &self.text[id];
                Ok([t.page, t.page_origin.0, t.page_origin.1]
                    .map(Value::Int)
                    .to_vec())
            }
            (Command::Attr, Want::Outs(0), 1..=4) => {
                let (id, rest) = match args.len() {
                    1 | 3 => (CONSOLE, args),
                    _ => (screen_id(&args[0])?, &args[1..]),
                };
                let attr = match rest {
                    [a] => a.to_int()?,
                    [h, v, r] => {
                        let bit = |v: &Value| v.to_int().map(|n| i32::from(n != 0));
                        bit(h)? << 2 | bit(v)? << 3 | (r.to_int()? & 3)
                    }
                    _ => unreachable!("one or three"),
                };
                if !(0..16).contains(&attr) {
                    return Err(out_of_range("attribute", attr));
                }
                self.text[id].attr = attr as u8;
                Ok(vec![])
            }
            (Command::Attr, Want::Outs(3), 0 | 1) => {
                let (id, _) = with_screen(args, 0)?;
                let a = i32::from(self.text[id].attr);
                Ok([a >> 2 & 1, a >> 3 & 1, a & 3].map(Value::Int).to_vec())
            }
            (Command::Attr, Want::Value, 0 | 1) => {
                let (id, _) = with_screen(args, 0)?;
                Ok(int_value(i32::from(self.text[id].attr)))
            }
            (Command::Backcolor, Want::Outs(0), 1) => {
                self.back_color = args[0].to_int()? as u32;
                Ok(vec![])
            }
            (Command::Backcolor, Want::Value, 0) => Ok(int_value(self.back_color as i32)),
            (Command::Fade, Want::Outs(0), 1..=2) => {
                let color = args[0].to_int()? as u32;
                let frames = match args.get(1) {
                    None => 0,
                    Some(v) => match v.to_int()? {
                        n @ 0.. => n as u32,
                        n => return Err(out_of_range("time", n)),
                    },
                };
                self.fade.start(color, frames);
                Ok(vec![])
            }
            (Command::Fade, Want::Value, 0) => Ok(int_value(self.fade.color as i32)),
            (Command::Fadechk, Want::Value, 0) => Ok(int_value(i32::from(self.fade.fading()))),
            (Command::Xscreen, Want::Outs(0), 2..=5) => {
                let (w, h) = (args[0].to_int()?, args[1].to_int()?);
                if !(128..=1280).contains(&w) || w % 4 != 0 {
                    return Err(out_of_range("width", w));
                }
                if !(128..=720).contains(&h) || h % 4 != 0 {
                    return Err(out_of_range("height", h));
                }
                let opt = |i: usize| args.get(i).map(opt_int).transpose().map(Option::flatten);
                let sample = opt(2)?.unwrap_or(1);
                if sample < 1 || w * sample > 1280 || h * sample > 720 {
                    return Err(out_of_range("sample", sample));
                }
                let mode = opt(3)?.unwrap_or(0);
                let filter = Filter::from_int(mode).ok_or_else(|| out_of_range("mode", mode))?;
                let aspect = match args.get(4) {
                    None | Some(Value::Default) => None,
                    Some(v) => match real(v)? {
                        a if (0.1..=10.0).contains(&a) => Some(a),
                        a => {
                            return Err(Error::with_detail(
                                ErrorKind::OutOfRange,
                                format!("aspect {a}"),
                            ));
                        }
                    },
                };
                self.set_mode(ScreenMode {
                    width: w as u32,
                    height: h as u32,
                    sample: sample as u32,
                    filter,
                    aspect,
                });
                Ok(vec![])
            }
            (Command::Xscreen, Want::Outs(n @ 2..=5), 0) => {
                let m = self.mode;
                let mut out = [m.width, m.height, m.sample]
                    .map(|n| Value::Int(n as i32))
                    .to_vec();
                out.push(Value::Int(m.filter as i32));
                // SmileBASIC keeps it in single precision: 1.66666663 for
                // 400x240 (checked on hardware).
                out.push(Value::Real(f64::from(m.aspect() as f32)));
                out.truncate(n.into());
                Ok(out)
            }
            (Command::Acls, Want::Outs(0), 0 | 3 | 4) => {
                let flags: Vec<bool> = args
                    .iter()
                    .map(|v| v.to_int().map(|n| n != 0))
                    .collect::<Result<_>>()?;
                let keep = |i: usize| flags.get(i).copied().unwrap_or(false);
                self.acls(keep(0), keep(1), keep(2), keep(3));
                Ok(vec![])
            }
            _ => Err(illegal()),
        }
    }
}

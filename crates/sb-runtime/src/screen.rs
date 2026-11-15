//! Placing text screens (TOFS, THOME, TROT, TSCALE, TCOLOR, TBLEND, TSHOW,
//! THIDE, TLAYER), the layers they and sprites belong to (LAYER, LCLIP,
//! LMATRIX, LFILTER), BACKCOLOR, what text screens show (TSCREEN, TPAGE and
//! ATTR), the resolution (XSCREEN), ACLS, and FADE over everything.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::Value;

use crate::compose::{Affine, Compose, LAYERS};
use crate::filter::Filter as LayerFilter;
use crate::graphics::Clip;
use crate::runtime::{Command, Runtime, checking, no_form, opt_int, screen_id, with_screen};
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
    checking(v, || {
        let n = v.to_int()?;
        usize::try_from(n)
            .ok()
            .filter(|&n| n < LAYERS)
            .ok_or_else(|| Error::with_detail(ErrorKind::OutOfRange, format!("layer {n}")))
    })
}

fn out_of_range(what: &str, n: i32) -> Error {
    Error::with_detail(ErrorKind::OutOfRange, format!("{what} {n}"))
}

fn in_range(what: &str, v: &Value, range: std::ops::RangeInclusive<i32>) -> Result<i32> {
    checking(v, || match v.to_int()? {
        n if range.contains(&n) => Ok(n),
        n => Err(out_of_range(what, n)),
    })
}

/// TSCREEN's font type: 8 to 64 in steps of 8.
fn font_type(n: i32) -> Result<u32> {
    if (8..=64).contains(&n) && n % 8 == 0 {
        Ok(n as u32)
    } else {
        let detail = format!("font type {n}");
        Err(Error::with_detail(ErrorKind::IllegalFontType, detail))
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
        // A clear end takes the color of the other one, so only the alpha
        // moves (checked on hardware, fading from and to clear colors).
        let clear = |c: u32| c >> 24 == 0;
        let from = match self.color {
            c if clear(c) => to & 0x00FF_FFFF,
            c => c,
        };
        let to_reached = match to {
            c if clear(c) && frames > 0 => from & 0x00FF_FFFF,
            c => c,
        };
        *self = Fade {
            color: if frames == 0 { to } else { from },
            from,
            to: to_reached,
            frames,
            elapsed: 0,
        };
    }

    pub(crate) fn fading(&self) -> bool {
        self.elapsed < self.frames
    }

    /// One frame of the change: each channel is the two ends mixed in
    /// single precision and rounded down, which gives the hardware's
    /// values (&HFF to 0 over 3 frames passes &HA9 and &H54).
    pub(crate) fn step(&mut self) {
        if !self.fading() {
            return;
        }
        self.elapsed += 1;
        let f = self.elapsed as f32 / self.frames as f32;
        self.color = (0..4).fold(0, |c, i| {
            let shift = i * 8;
            let a = (self.from >> shift & 0xFF) as f32;
            let b = (self.to >> shift & 0xFF) as f32;
            c | ((a * (1.0 - f) + b * f).floor() as u32) << shift
        });
    }
}

/// Covers `pixels` with FADE's color, mixing each channel with it
/// by its alpha, and darkens them to 224/255 when `dim` is set, both
/// at once with one rounding. The alpha stays as it is.
pub(crate) fn cover(color: u32, pixels: &mut [u32], dim: bool) {
    let a = color >> 24;
    match (a, dim) {
        (0, false) => {}
        (0, true) => darken(pixels),
        (_, false) => {
            let mix = |p: u32, c: u32, shift: u32| {
                let (p, c) = (p >> shift & 0xFF, c >> shift & 0xFF);
                ((c * a + p * (255 - a) + 127) / 255) << shift
            };
            for p in pixels {
                *p = *p & 0xFF00_0000 | mix(*p, color, 16) | mix(*p, color, 8) | mix(*p, color, 0);
            }
        }
        (_, true) => {
            let mix = |p: u32, c: u32, shift: u32| {
                let (p, c) = (p >> shift & 0xFF, c >> shift & 0xFF);
                (((c * a + p * (255 - a)) * DIM + 255 * 255 / 2) / (255 * 255)) << shift
            };
            for p in pixels {
                *p = *p & 0xFF00_0000 | mix(*p, color, 16) | mix(*p, color, 8) | mix(*p, color, 0);
            }
        }
    }
}

/// What the side without the focus is darkened to beside a subprogram,
/// out of 255.
pub const DIM: u32 = 224;

/// Darkens each channel to [`DIM`]/255, rounded to the nearest: 255 to
/// 224, 248 to 218, 96 to 84.
pub(crate) fn darken(pixels: &mut [u32]) {
    for p in pixels {
        // Red and blue in one go, then green, 16 bits apiece: each
        // product is below 2^16, so (x + 128 + (x+128)/256) / 256 is
        // x/255 rounded, without carrying into the next lane.
        let lanes = |x: u32| {
            let t = x + 0x0080_0080;
            ((t + (t >> 8 & 0x00FF_00FF)) >> 8) & 0x00FF_00FF
        };
        let rb = lanes((*p & 0x00FF_00FF) * DIM);
        let g = lanes((*p >> 8 & 0xFF) * DIM) << 8;
        *p = *p & 0xFF00_0000 | rb | g;
    }
}

fn char_size(v: &Value) -> Result<u32> {
    checking(v, || {
        let n = v.to_int()?;
        if (8..=64).contains(&n) {
            Ok(n as u32)
        } else {
            Err(out_of_range("character size", n))
        }
    })
}

fn z(v: &Value) -> Result<i32> {
    checking(v, || {
        let z = v.to_int()?;
        if (-4095..=4095).contains(&z) {
            Ok(z)
        } else {
            Err(Error::with_detail(ErrorKind::OutOfRange, format!("Z {z}")))
        }
    })
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
                // X and Y may be left out (`TOFS 0,,,10`).
                let coord = |v: &Value| match v {
                    Value::Default => Ok(None),
                    v => real(v).map(Some),
                };
                let (x, y) = (coord(&args[1])?, coord(&args[2])?);
                let z = match args.get(3) {
                    None | Some(Value::Default) => None,
                    Some(v) => Some(self::z(v)?),
                };
                let t = &mut self.text[screen_id(&args[0])?];
                t.pos = (x.unwrap_or(t.pos.0), y.unwrap_or(t.pos.1));
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
                // Kept in single precision, as SPROT is.
                let angle = f64::from(real(&args[1])? as f32);
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
            (Command::Lfilter, Want::Value, 1) => {
                Ok(vec![Value::Int(self.filters[layer_id(&args[0])?].kind())])
            }
            (Command::Lfilter, Want::Outs(0), 1..=5) => {
                let id = layer_id(&args[0])?;
                let kind = args.get(1).map(opt_int).transpose()?.flatten().unwrap_or(0);
                let size =
                    |v: &Value| -> Result<u32> { Ok(in_range("filter size", v, 0..=1280)? as u32) };
                // Raster deformation needs the repeat flag: with the array
                // alone it is Out of range for the type. An array that is
                // not a real [n,2] or [n,4] is Type mismatch. The array is
                // copied here, into the rows kept from last time if there
                // are any, as a program may do this every frame.
                self.filters[id] = match (kind, &args[2.min(args.len())..]) {
                    (0, []) => LayerFilter::None,
                    (1, [n]) => LayerFilter::Mosaic(size(n)?),
                    (2, [n]) => LayerFilter::Blur(size(n)?),
                    (3 | 4, [info, repeat]) => {
                        let repeat = repeat.to_int()? != 0;
                        let wrong = || Error::new(ErrorKind::TypeMismatch).of_value(info);
                        let Value::Array(info) = info else {
                            return Err(wrong());
                        };
                        let a = info.borrow();
                        if a.elem_type() != sb_core::value::ElemType::Real
                            || !matches!(a.dims[..], [_, 2 | 4])
                        {
                            return Err(wrong());
                        }
                        let mut rows = match std::mem::take(&mut self.filters[id]) {
                            LayerFilter::Raster { rows, .. } => rows,
                            _ => Vec::new(),
                        };
                        rows.clear();
                        crate::filter::raster_rows(&a, kind == 4, &mut rows);
                        LayerFilter::Raster {
                            vertical: kind == 4,
                            rows,
                            repeat,
                        }
                    }
                    (5, [h, s, v]) => LayerFilter::Color {
                        hue: real(h)?,
                        saturation: real(s)?,
                        value: real(v)?,
                    },
                    // A type that does not go with this many arguments.
                    (n, _) => return Err(out_of_range("filter type", n).of_arg(1)),
                };
                Ok(vec![])
            }
            (Command::Lmatrix, Want::Outs(0), 1) => {
                let layer = &mut self.layers[layer_id(&args[0])?];
                (layer.matrix, layer.projection) = (Affine::IDENTITY, None);
                Ok(vec![])
            }
            // Past the first 16 elements nothing is looked at.
            (Command::Lmatrix, Want::Outs(0), 2) => {
                let id = layer_id(&args[0])?;
                let wrong = || Error::new(ErrorKind::TypeMismatch).of_arg(1);
                let Value::Array(a) = &args[1] else {
                    return Err(wrong());
                };
                let a = a.borrow();
                if a.elem_type() != sb_core::value::ElemType::Real || a.len() < 16 {
                    return Err(wrong());
                }
                let mut m = [0.0; 16];
                for (i, v) in m.iter_mut().enumerate() {
                    let x = a.get(i).to_real()?;
                    if x.is_finite() && x.abs() > f64::from(f32::MAX) {
                        return Err(out_of_range("matrix", i as i32).of_arg(1));
                    }
                    // Kept as the GPU takes it.
                    *v = f64::from(x as f32);
                }
                let layer = &mut self.layers[id];
                (layer.matrix, layer.projection) = (Affine::IDENTITY, Some(m));
                Ok(vec![])
            }
            (Command::Lmatrix, Want::Outs(0), 3 | 5 | 7 | 8) => {
                let id = layer_id(&args[0])?;
                let n = |i: usize, default: f64| args.get(i).map_or(Ok(default), real);
                self.layers[id].projection = None;
                self.layers[id].matrix = Affine::place(
                    (n(3, 0.0)?, n(4, 0.0)?),
                    (n(1, 0.0)?, n(2, 0.0)?),
                    n(7, 0.0)?,
                    (n(5, 1.0)?, n(6, 1.0)?),
                );
                Ok(vec![])
            }
            (Command::Tscreen, Want::Outs(0), 1..=5) => {
                // The font type is checked after the other arguments.
                let ft_at = usize::from(args.len() >= 3);
                let id = match ft_at {
                    0 => CONSOLE,
                    _ => screen_id(&args[0])?,
                };
                let ft = checking(&args[ft_at], || args[ft_at].to_int())?;
                let cells = |v: &Value| -> Result<u32> {
                    Ok(in_range("text screen size", v, 1..=32768)? as u32)
                };
                let (cs, size) = match args {
                    [_] => (None, None),
                    [_, cs] | [_, _, cs] => (Some(char_size(cs)?), None),
                    [_, _, w, h] => (None, Some((cells(w)?, cells(h)?))),
                    [_, _, cs, w, h] => (Some(char_size(cs)?), Some((cells(w)?, cells(h)?))),
                    _ => unreachable!("one to five"),
                };
                let ft = font_type(ft).map_err(|e| e.of_arg(ft_at))?;
                let cs = cs.unwrap_or(ft);
                let (fw, fh) = self.frame_size();
                // Omitted: as many characters as fill the frame.
                let (w, h) = size.unwrap_or((fw / cs, fh / cs));
                if u64::from(w) * u64::from(h) > 32768 {
                    return Err(Error::with_detail(
                        ErrorKind::OutOfRange,
                        format!("{w}x{h} characters"),
                    ));
                }
                self.text[id].set_font(ft, cs, w, h);
                self.text[id].restart_look();
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
                let page = in_range("graphic page", &rest[0], -1..=5)?;
                let origin = |v: &Value| in_range("origin", v, -32768..=32767);
                let origin = (origin(&rest[1])?, origin(&rest[2])?);
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
                // A value out of range is named by its own argument:
                // `ATTR 4,128` is (ATTR:2), the flips and turn of
                // `ATTR 4,0,0,4` (ATTR:2) to (ATTR:4).
                let ranged = |v: &Value, what: &'static str, max: i32| {
                    checking(v, || match v.to_int()? {
                        n if (0..=max).contains(&n) => Ok(n),
                        n => Err(out_of_range(what, n)),
                    })
                };
                let attr = match rest {
                    [a] => ranged(a, "attribute", 15)?,
                    [h, v, r] => {
                        let h = ranged(h, "horizontal flip", 1)?;
                        let v = ranged(v, "vertical flip", 1)?;
                        h << 2 | v << 3 | ranged(r, "turn", 3)?
                    }
                    _ => unreachable!("one or three"),
                };
                self.text[id].attr = attr as u16;
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
            // It holds the program until the frame job has taken the new mode.
            (Command::Xscreen, Want::Outs(0), 2..=5) if self.mode_taken() => Ok(vec![]),
            (Command::Xscreen, Want::Outs(0), 2..=5) => {
                let w = in_range("width", &args[0], 128..=1280)?;
                let h = in_range("height", &args[1], 128..=720)?;
                let opt = |i: usize, what, range, default| match args.get(i) {
                    None | Some(Value::Default) => Ok(default),
                    Some(v) => in_range(what, v, range),
                };
                let sample = opt(2, "sample", 1..=10, 1)?;
                let mode = opt(3, "mode", 0..=2, 0)?;
                let filter = Filter::from_int(mode).expect("0 to 2");
                let aspect = match args.get(4) {
                    None | Some(Value::Default) => None,
                    Some(v) => Some(checking(v, || match real(v)? {
                        a if (0.1..=10.0).contains(&a) => Ok(a),
                        a => Err(Error::with_detail(
                            ErrorKind::OutOfRange,
                            format!("aspect {a}"),
                        )),
                    })?),
                };
                // After every range.
                for (i, n) in [w, h].into_iter().enumerate() {
                    if n % 4 != 0 {
                        return Err(Error::new(ErrorKind::UseMultiplesOf4).of_arg(i));
                    }
                }
                if w * sample > 1280 || h * sample > 720 {
                    // SmileBASIC 4 names the fourth argument when all five
                    // are given.
                    let arg = if args.len() == 5 { 3 } else { 2 };
                    return Err(out_of_range("sample", sample).of_arg(arg));
                }
                self.set_mode(ScreenMode {
                    width: w as u32,
                    height: h as u32,
                    sample: sample as u32,
                    filter,
                    aspect,
                });
                self.text.iter_mut().for_each(|t| t.stop_anims());
                self.hold_for_a_frame();
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
            (Command::Acls, Want::Outs(0), 0 | 3 | 4) if self.mode_taken() => Ok(vec![]),
            (Command::Acls, Want::Outs(0), 0 | 3 | 4) => {
                let flags: Vec<bool> = args
                    .iter()
                    .map(|v| v.to_int().map(|n| n != 0))
                    .collect::<Result<_>>()?;
                let keep = |i: usize| flags.get(i).copied().unwrap_or(false);
                self.acls(keep(0), keep(1), keep(2), keep(3));
                self.hold_for_a_frame();
                Ok(vec![])
            }
            _ => Err(no_form()),
        }
    }
}

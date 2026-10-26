//! Placing text screens (TOFS, THOME, TROT, TSCALE, TCOLOR, TBLEND, TSHOW,
//! THIDE, TLAYER), the layers they and sprites belong to (LAYER, LCLIP,
//! LMATRIX), and BACKCOLOR.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::Value;

use crate::compose::{Affine, Compose, LAYERS};
use crate::graphics::Clip;
use crate::runtime::{Command, Runtime, illegal, opt_int, screen_id};

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
            (Command::Backcolor, Want::Outs(0), 1) => {
                self.back_color = args[0].to_int()? as u32;
                Ok(vec![])
            }
            (Command::Backcolor, Want::Value, 0) => Ok(int_value(self.back_color as i32)),
            _ => Err(illegal()),
        }
    }
}

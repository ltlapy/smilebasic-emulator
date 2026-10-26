//! Sprite commands: SPSET, SPCLR, SPSHOW, SPHIDE, SPHOME, SPOFS, SPROT,
//! SPSCALE, SPCOLOR, SPCHR, SPPAGE, SPLAYER, SPDEF, SPVAR and SPUSED.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{ArrayData, Value};

use crate::runtime::{Command, Runtime, illegal, opt_int};
use crate::sprites::{DEFS, SPRITES, Sprite, SpriteDef, VarKey};

fn int(v: &Value) -> Result<i32> {
    v.to_int()
}

fn real(v: &Value) -> Result<f64> {
    v.to_real()
}

fn out_of_range(what: &str, n: i32) -> Error {
    Error::with_detail(ErrorKind::OutOfRange, format!("{what} {n}"))
}

fn in_range(v: &Value, what: &str, range: std::ops::RangeInclusive<i32>) -> Result<i32> {
    let n = int(v)?;
    if range.contains(&n) {
        Ok(n)
    } else {
        Err(out_of_range(what, n))
    }
}

fn sprite_id(v: &Value) -> Result<usize> {
    Ok(in_range(v, "sprite", 0..=SPRITES as i32 - 1)? as usize)
}

fn def_id(v: &Value) -> Result<usize> {
    Ok(in_range(v, "sprite definition", 0..=DEFS as i32 - 1)? as usize)
}

fn page(v: &Value) -> Result<i32> {
    in_range(v, "graphic page", -1..=5)
}

fn layer(v: &Value) -> Result<i32> {
    in_range(v, "layer", 0..=7)
}

/// A show flag, 1 when omitted.
fn flag(v: Option<&Value>) -> Result<bool> {
    Ok(v.map(opt_int).transpose()?.flatten().unwrap_or(1) != 0)
}

fn var_key(v: &Value) -> Result<VarKey> {
    match v {
        Value::Str(s) => Ok(VarKey::Str(s.borrow().clone())),
        v => Ok(VarKey::Int(int(v)?)),
    }
}

fn reals(values: &[f64]) -> Vec<Value> {
    values.iter().map(|&x| Value::Real(x)).collect()
}

fn ints(values: &[i32]) -> Vec<Value> {
    values.iter().map(|&n| Value::Int(n)).collect()
}

impl Runtime {
    /// A sprite that SPSET has created.
    // UNVERIFIED: the error for using a sprite before SPSET.
    fn sprite(&mut self, v: &Value) -> Result<&mut Sprite> {
        let id = sprite_id(v)?;
        let s = &mut self.sprites.list[id];
        if s.used {
            Ok(s)
        } else {
            Err(Error::with_detail(
                ErrorKind::IllegalFunctionCall,
                format!("sprite {id} is not set"),
            ))
        }
    }

    /// The image part of SPSET and SPCHR: U,V,W,H and attributes.
    fn image_args(args: &[Value]) -> Result<(i32, i32, i32, i32, i32)> {
        let u = int(&args[0])?;
        let v = int(&args[1])?;
        let w = in_range(&args[2], "width", 0..=2048)?;
        let h = in_range(&args[3], "height", 0..=2048)?;
        let attr = args.get(4).map(opt_int).transpose()?.flatten().unwrap_or(0);
        Ok((u, v, w, h, attr))
    }

    /// A free sprite number, searching from `start` towards `end`.
    fn free_sprite(&self, start: usize, end: usize) -> Option<usize> {
        let free = |&i: &usize| !self.sprites.list[i].used;
        if start <= end {
            (start..=end).find(free)
        } else {
            (end..=start).rev().find(free)
        }
    }

    /// Creates sprite `id` from a template or from image arguments.
    fn spset(&mut self, id: usize, image: &[Value]) -> Result<()> {
        let shown;
        let def = match image.len() {
            1 | 2 => {
                let n = def_id(&image[0])?;
                shown = flag(image.get(1))?;
                Some((n, self.sprites.defs[n]))
            }
            _ => {
                shown = flag(image.get(5))?;
                None
            }
        };
        let fields = match def {
            Some(_) => None,
            None => Some(Runtime::image_args(&image[..image.len().min(5)])?),
        };
        let s = self.sprites.create(id, shown);
        match (def, fields) {
            (Some((n, d)), _) => s.set_def(n as i32, &d),
            (None, Some((u, v, w, h, attr))) => (s.u, s.v, s.w, s.h, s.attr) = (u, v, w, h, attr),
            (None, None) => unreachable!("one of the two forms"),
        }
        Ok(())
    }

    pub(crate) fn sp_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        match (command, want, args.len()) {
            (Command::Spset, Want::Outs(0), 2 | 3 | 5..=7) => {
                let id = sprite_id(&args[0])?;
                self.spset(id, &args[1..])?;
                Ok(vec![])
            }
            (Command::Spset, Want::Value, 1..=8) => {
                let (range, image) = match args.len() {
                    3 | 4 | 7 | 8 => {
                        let (a, b) = (sprite_id(&args[0])?, sprite_id(&args[1])?);
                        ((a, b), &args[2..])
                    }
                    _ => ((0, SPRITES - 1), args),
                };
                match self.free_sprite(range.0, range.1) {
                    Some(id) => {
                        self.spset(id, image)?;
                        Ok(vec![Value::Int(id as i32)])
                    }
                    None => Ok(vec![Value::Int(-1)]),
                }
            }
            (Command::Spclr, Want::Outs(0), 0) => {
                self.sprites.clear_all();
                Ok(vec![])
            }
            (Command::Spclr, Want::Outs(0), 1 | 2) => {
                let a = sprite_id(&args[0])?;
                let b = args.get(1).map(sprite_id).transpose()?.unwrap_or(a);
                for id in a.min(b)..=a.max(b) {
                    self.sprites.clear(id);
                }
                Ok(vec![])
            }
            (Command::Spshow | Command::Sphide, Want::Outs(0), 1 | 2) => {
                let on = flag(args.get(1))?;
                let shown = if command == Command::Spshow { on } else { !on };
                self.sprite(&args[0])?.shown = shown;
                Ok(vec![])
            }
            (Command::Spshow | Command::Sphide, Want::Value, 1) => {
                let shown = self.sprite(&args[0])?.shown;
                let answer = if command == Command::Spshow {
                    shown
                } else {
                    !shown
                };
                Ok(vec![Value::Int(i32::from(answer))])
            }
            (Command::Sphome, Want::Outs(0), 3) => {
                let home = (real(&args[1])?, real(&args[2])?);
                self.sprite(&args[0])?.home = home;
                Ok(vec![])
            }
            (Command::Sphome, Want::Outs(2), 1) => {
                let (x, y) = self.sprite(&args[0])?.home;
                Ok(reals(&[x, y]))
            }
            (Command::Spofs, Want::Outs(0), 2..=4) => {
                let x = args.get(1).map(optional_real).transpose()?.flatten();
                let y = args.get(2).map(optional_real).transpose()?.flatten();
                let z = match args.get(3).map(opt_int).transpose()?.flatten() {
                    Some(z) if !(-4095..=4095).contains(&z) => {
                        return Err(out_of_range("Z", z));
                    }
                    z => z,
                };
                let s = self.sprite(&args[0])?;
                s.pos = (x.unwrap_or(s.pos.0), y.unwrap_or(s.pos.1));
                s.z = z.unwrap_or(s.z);
                Ok(vec![])
            }
            // The coordinate flag matters only once SPLINK exists.
            (Command::Spofs, Want::Outs(n @ (2 | 3)), 1 | 2) => {
                let s = self.sprite(&args[0])?;
                let mut out = reals(&[s.pos.0, s.pos.1]);
                if n == 3 {
                    out.push(Value::Int(s.z));
                }
                Ok(out)
            }
            (Command::Sprot, Want::Outs(0), 2) => {
                let angle = real(&args[1])?;
                self.sprite(&args[0])?.rot = angle;
                Ok(vec![])
            }
            (Command::Sprot, Want::Value, 1) => Ok(vec![Value::Real(self.sprite(&args[0])?.rot)]),
            (Command::Spscale, Want::Outs(0), 3) => {
                let scale = (real(&args[1])?, real(&args[2])?);
                self.sprite(&args[0])?.scale = scale;
                Ok(vec![])
            }
            (Command::Spscale, Want::Outs(2), 1) => {
                let (x, y) = self.sprite(&args[0])?.scale;
                Ok(reals(&[x, y]))
            }
            (Command::Spcolor, Want::Outs(0), 2) => {
                let color = int(&args[1])? as u32;
                self.sprite(&args[0])?.color = color;
                Ok(vec![])
            }
            (Command::Spcolor, Want::Value, 1) => {
                Ok(vec![Value::Int(self.sprite(&args[0])?.color as i32)])
            }
            (Command::Spchr, Want::Outs(0), 2) => {
                let n = def_id(&args[1])?;
                let def = self.sprites.defs[n];
                self.sprite(&args[0])?.set_def(n as i32, &def);
                Ok(vec![])
            }
            (Command::Spchr, Want::Outs(0), 3..=6) => {
                let field = |i: usize| args.get(i).map(opt_int).transpose().map(Option::flatten);
                let (u, v, w, h, attr) = (field(1)?, field(2)?, field(3)?, field(4)?, field(5)?);
                for (size, what) in [(w, "width"), (h, "height")] {
                    if let Some(n) = size.filter(|n| !(0..=2048).contains(n)) {
                        return Err(out_of_range(what, n));
                    }
                }
                let s = self.sprite(&args[0])?;
                // Omitted U and V stay; W and H become 16 and the
                // attributes 0, as the reference describes.
                s.u = u.unwrap_or(s.u);
                s.v = v.unwrap_or(s.v);
                s.w = w.unwrap_or(16);
                s.h = h.unwrap_or(16);
                s.attr = attr.unwrap_or(0);
                Ok(vec![])
            }
            (Command::Spchr, Want::Outs(n @ (2 | 4 | 5)), 1) => {
                let s = self.sprite(&args[0])?;
                Ok(ints(&[s.u, s.v, s.w, s.h, s.attr][..n as usize]))
            }
            (Command::Spchr, Want::Value, 1) => Ok(vec![Value::Int(self.sprite(&args[0])?.def)]),
            (Command::Sppage, Want::Outs(0), 2) => {
                let p = page(&args[1])?;
                self.sprite(&args[0])?.page = p;
                Ok(vec![])
            }
            (Command::Sppage, Want::Value, 1) => Ok(vec![Value::Int(self.sprite(&args[0])?.page)]),
            (Command::Sppage, Want::Outs(0), 1) => {
                self.sprites.new_page = page(&args[0])?;
                Ok(vec![])
            }
            (Command::Sppage, Want::Value, 0) => Ok(vec![Value::Int(self.sprites.new_page)]),
            (Command::Splayer, Want::Outs(0), 2) => {
                let l = layer(&args[1])?;
                self.sprite(&args[0])?.layer = l;
                Ok(vec![])
            }
            (Command::Splayer, Want::Value, 1) => {
                Ok(vec![Value::Int(self.sprite(&args[0])?.layer)])
            }
            (Command::Splayer, Want::Outs(0), 1) => {
                self.sprites.new_layer = layer(&args[0])?;
                Ok(vec![])
            }
            (Command::Splayer, Want::Value, 0) => Ok(vec![Value::Int(self.sprites.new_layer)]),
            (Command::Spdef, _, _) => self.spdef(args, want),
            (Command::Spvar, Want::Outs(0), 3) => {
                let key = var_key(&args[1])?;
                let value = match &args[2] {
                    Value::Int(_) | Value::Real(_) | Value::Str(_) => args[2].clone(),
                    _ => return Err(ErrorKind::TypeMismatch.into()),
                };
                self.sprite(&args[0])?.vars.insert(key, value);
                Ok(vec![])
            }
            (Command::Spvar, Want::Value, 2) => {
                let key = var_key(&args[1])?;
                let s = self.sprite(&args[0])?;
                Ok(vec![s.vars.get(&key).cloned().unwrap_or(Value::Int(0))])
            }
            (Command::Spvar, Want::Outs(0), 1 | 2) => {
                let key = args.get(1).map(var_key).transpose()?;
                let s = self.sprite(&args[0])?;
                match key {
                    Some(key) => {
                        s.vars.remove(&key);
                    }
                    None => s.vars.clear(),
                }
                Ok(vec![])
            }
            (Command::Spused, Want::Value, 1) => {
                let id = sprite_id(&args[0])?;
                Ok(vec![Value::Int(i32::from(self.sprites.list[id].used))])
            }
            _ => Err(illegal()),
        }
    }

    /// SPDEF in all its forms.
    fn spdef(&mut self, args: &[Value], want: Want) -> Result<Vec<Value>> {
        match (want, args) {
            (Want::Outs(0), []) => {
                self.sprites.reset_defs();
                Ok(vec![])
            }
            (Want::Outs(n), [id]) if n > 0 => {
                let d = self.sprites.defs[def_id(id)?];
                let out = match n {
                    2 => vec![d.u, d.v],
                    3 => vec![d.u, d.v, d.attr],
                    4 => vec![d.u, d.v, d.w, d.h],
                    5 => vec![d.u, d.v, d.w, d.h, d.attr],
                    6 => vec![d.u, d.v, d.w, d.h, d.hx, d.hy],
                    7 => vec![d.u, d.v, d.w, d.h, d.hx, d.hy, d.attr],
                    _ => return Err(illegal()),
                };
                Ok(ints(&out))
            }
            (Want::Outs(0), [Value::Array(array), rest @ ..]) if rest.len() <= 3 => {
                let offset = rest.first().map(def_id).transpose()?.unwrap_or(0);
                let (du, dv) = match rest {
                    [_, u, v] => (int(u)?, int(v)?),
                    [] | [_] => (0, 0),
                    _ => return Err(illegal()),
                };
                let values: Vec<i32> = match &array.borrow().data {
                    ArrayData::Int(v) => v.clone(),
                    ArrayData::Real(v) => v.iter().map(|&x| x as i32).collect(),
                    ArrayData::Str(_) => return Err(ErrorKind::TypeMismatch.into()),
                };
                if !values.len().is_multiple_of(7) {
                    return Err(Error::with_detail(
                        ErrorKind::IllegalFunctionCall,
                        "needs 7 values each",
                    ));
                }
                if offset + values.len() / 7 > DEFS {
                    return Err(ErrorKind::OutOfRange.into());
                }
                for (i, d) in values.chunks_exact(7).enumerate() {
                    self.sprites.defs[offset + i] = SpriteDef {
                        u: d[0] + du,
                        v: d[1] + dv,
                        w: d[2],
                        h: d[3],
                        hx: d[4],
                        hy: d[5],
                        attr: d[6],
                    };
                }
                Ok(vec![])
            }
            // TODO: SPDEF "@label" reads DATA, which the host cannot reach yet.
            (Want::Outs(0), [Value::Str(_), ..]) => Err(Error::with_detail(
                ErrorKind::IllegalFunctionCall,
                "SPDEF from DATA is not supported yet",
            )),
            // SPDEF id,source,[U],[V],[W],[H],[HX],[HY],[attributes]
            (Want::Outs(0), [id, source, fields @ ..]) if fields.len() == 7 => {
                let id = def_id(id)?;
                let mut d = self.sprites.defs[def_id(source)?];
                let mut slots = [
                    &mut d.u,
                    &mut d.v,
                    &mut d.w,
                    &mut d.h,
                    &mut d.hx,
                    &mut d.hy,
                    &mut d.attr,
                ];
                for (slot, field) in slots.iter_mut().zip(fields) {
                    if let Some(n) = opt_int(field)? {
                        **slot = n;
                    }
                }
                self.sprites.defs[id] = d;
                Ok(vec![])
            }
            // SPDEF id,U,V[,W,H[,HX,HY]][,attributes]
            (Want::Outs(0), [id, u, v, rest @ ..]) if rest.len() <= 5 => {
                let id = def_id(id)?;
                let n = |v: &Value| int(v);
                let (size, home, attr) = match rest {
                    [] => (None, None, None),
                    [a] => (None, None, Some(a)),
                    [w, h] => (Some((w, h)), None, None),
                    [w, h, a] => (Some((w, h)), None, Some(a)),
                    [w, h, x, y] => (Some((w, h)), Some((x, y)), None),
                    [w, h, x, y, a] => (Some((w, h)), Some((x, y)), Some(a)),
                    _ => unreachable!("at most five"),
                };
                let (w, h) = match size {
                    Some((w, h)) => (n(w)?, n(h)?),
                    None => (16, 16),
                };
                let (hx, hy) = match home {
                    Some((x, y)) => (n(x)?, n(y)?),
                    None => (0, 0),
                };
                self.sprites.defs[id] = SpriteDef {
                    u: n(u)?,
                    v: n(v)?,
                    w,
                    h,
                    hx,
                    hy,
                    attr: attr.map(n).transpose()?.unwrap_or(0),
                };
                Ok(vec![])
            }
            _ => Err(illegal()),
        }
    }
}

/// An omittable real argument.
fn optional_real(v: &Value) -> Result<Option<f64>> {
    match v {
        Value::Default => Ok(None),
        v => v.to_real().map(Some),
    }
}

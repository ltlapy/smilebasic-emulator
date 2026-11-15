//! Sprite commands: SPSET, SPCLR, SPSHOW, SPHIDE, SPHOME, SPOFS, SPROT,
//! SPSCALE, SPCOLOR, SPCHR, SPPAGE, SPLAYER, SPDEF, SPVAR, SPUSED, SPLINK,
//! SPUNLINK, SPFUNC, and the
//! animation commands SPANIM, ANIMDEF, SPSTART, SPSTOP and SPCHK, and the
//! collision commands SPCOL, SPCOLVEC, SPHITSP, SPHITRC and SPHITINFO.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{ArrayData, Value};

use crate::anim::{self, Anim, MAX_KEYS, Mode, Target, Track};
use crate::collision::{Body, Collision, Search};
use crate::runtime::{Command, Runtime, checking, illegal, no_form, opt_int};
use crate::sprites::{DEFS, SPRITES, Sprite, SpriteDef, VarKey, WithDefs};

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
    checking(v, || {
        let n = int(v)?;
        if range.contains(&n) {
            Ok(n)
        } else {
            Err(out_of_range(what, n))
        }
    })
}

pub(crate) fn sprite_id(v: &Value) -> Result<usize> {
    Ok(in_range(v, "sprite", 0..=SPRITES as i32 - 1)? as usize)
}

fn def_id(v: &Value) -> Result<usize> {
    Ok(in_range(v, "sprite definition", 0..=DEFS as i32 - 1)? as usize)
}

pub(crate) fn anim_id(v: &Value) -> Result<usize> {
    Ok(in_range(v, "animation definition", 0..=anim::DEFINITIONS as i32 - 1)? as usize)
}

/// A loop count, 1 when omitted.
fn loops(v: Option<&Value>) -> Result<i32> {
    Ok(v.map(opt_int).transpose()?.flatten().unwrap_or(1))
}

fn numbers(array: &ArrayData) -> Result<Vec<f64>> {
    match array {
        ArrayData::Int(v) => Ok(v.iter().map(|&n| f64::from(n)).collect()),
        ArrayData::Real(v) => Ok(v.clone()),
        ArrayData::Str(_) => Err(ErrorKind::TypeMismatch.into()),
    }
}

/// A collision mask, every bit when omitted.
fn mask(v: Option<&Value>) -> Result<u32> {
    Ok(v.map(opt_int)
        .transpose()?
        .flatten()
        .map_or(u32::MAX, |m| m as u32))
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

pub(crate) fn var_key(v: &Value) -> Result<VarKey> {
    checking(v, || match v {
        Value::Str(s) => Ok(VarKey::Str(s.borrow().clone())),
        v => Ok(VarKey::Int(int(v)?)),
    })
}

fn reals(values: &[f64]) -> Vec<Value> {
    values.iter().map(|&x| Value::Real(x)).collect()
}

fn ints(values: &[i32]) -> Vec<Value> {
    values.iter().map(|&n| Value::Int(n)).collect()
}

impl Runtime {
    /// A sprite that SPSET has created; others are "Sprite not set"
    /// (checked on hardware).
    fn sprite(&mut self, v: &Value) -> Result<&mut Sprite> {
        let id = sprite_id(v)?;
        let s = self.sprites.sprite_mut(id);
        if s.used {
            Ok(s)
        } else {
            Err(Error::with_detail(
                ErrorKind::SpriteNotSet,
                format!("sprite {id}"),
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
    pub(crate) fn free_sprite(&self, start: usize, end: usize) -> Option<usize> {
        let free = |&i: &usize| !self.sprites.list()[i].used;
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

    /// The animation SPANIM and ANIMDEF take after their number: the
    /// target, then an array, a `"@label"` or the keys themselves, then
    /// the loop count.
    pub(crate) fn anim_args(&self, args: &[Value]) -> Result<Anim> {
        let mode = Mode::parse(&args[0])?;
        let per = 1 + mode.target.items();
        match &args[1..] {
            [Value::Array(array), rest @ ..] if rest.len() <= 1 => {
                let data = numbers(&array.borrow().data)?;
                Anim::new(mode, &data, loops(rest.first())?)
            }
            [label @ Value::Str(_), rest @ ..] if rest.len() <= 1 => {
                let mut data = self.label_data(label)?;
                let keys = match data.number()? as i32 {
                    n if n < 1 => {
                        let e = Error::with_detail(ErrorKind::IllegalAnimationData, "no keys");
                        return Err(e);
                    }
                    n => n as usize,
                };
                if keys > MAX_KEYS {
                    let detail = format!("{keys} keys (32 at most)");
                    return Err(Error::with_detail(ErrorKind::AnimationTooLong, detail));
                }
                Anim::new(mode, &data.numbers(keys * per)?, loops(rest.first())?)
            }
            keys => {
                let (keys, rest) = keys.split_at(keys.len() - keys.len() % per);
                if rest.len() > 1 {
                    return Err(Error::with_detail(
                        ErrorKind::IllegalFunctionCall,
                        format!("{per} values per key"),
                    ));
                }
                let data = keys.iter().map(real).collect::<Result<Vec<_>>>()?;
                Anim::new(mode, &data, loops(rest.first())?)
            }
        }
    }

    fn start_anim(&mut self, id: &Value, anim: Anim) -> Result<Vec<Value>> {
        self.sprite(id)?;
        let id = sprite_id(id)?;
        let (s, defs) = self.sprites.sprite_and_defs(id);
        let target = anim.mode.target.index();
        let track = Track::new(anim, &WithDefs::new(s, defs));
        s.anims[target] = Some(Box::new(track));
        Ok(vec![])
    }

    /// SPCOL's setting forms.
    fn spcol(&mut self, args: &[Value]) -> Result<()> {
        let flag = |v: Option<&Value>| -> Result<bool> {
            Ok(v.map(opt_int).transpose()?.flatten().unwrap_or(0) != 0)
        };
        let rect = match args.len() {
            5..=7 => Some((
                in_range(&args[1], "collision X", -32768..=32767)?,
                in_range(&args[2], "collision Y", -32768..=32767)?,
                in_range(&args[3], "collision width", 1..=65535)?,
                in_range(&args[4], "collision height", 1..=65535)?,
            )),
            _ => None,
        };
        let (scale, mask) = match args.len() {
            1 | 2 => (flag(args.get(1))?, u32::MAX),
            3 => (flag(args.get(1))?, mask(args.get(2))?),
            5 | 6 => (flag(args.get(5))?, u32::MAX),
            7 => (flag(args.get(5))?, mask(args.get(6))?),
            // 4 and 8 arguments are no form.
            _ => return Err(no_form()),
        };
        let id = sprite_id(&args[0])?;
        let s = self.sprite(&args[0])?;
        // Without a rectangle, the sprite's own at the time, from its home.
        let (x, y, w, h) = rect.unwrap_or((-s.home.0 as i32, -s.home.1 as i32, s.w, s.h));
        self.sprites.set_collision(
            id,
            Collision {
                x,
                y,
                w,
                h,
                scale,
                mask,
            },
        );
        Ok(())
    }

    /// Runs a search, or goes on with the last one, and returns the next
    /// sprite hit or -1.
    fn hit(&mut self, rect: bool, start: Option<Search>) -> i32 {
        let slot = if rect {
            &mut self.sprites.hit_rc
        } else {
            &mut self.sprites.hit_sp
        };
        let Some(mut search) = start.or_else(|| slot.take()) else {
            return -1;
        };
        let found = self.sprites.next_hit(&mut search);
        if rect {
            self.sprites.hit_rc = Some(search);
        } else {
            self.sprites.hit_sp = Some(search);
        }
        found.map_or(-1, |id| id as i32)
    }

    /// SPHITRC's rectangle: X,Y,W,H, then optionally [mask],VX,VY.
    fn hit_rect(args: &[Value]) -> Result<Body> {
        let (vel, mask) = match args {
            [_, _, _, _] => ((0.0, 0.0), u32::MAX),
            [_, _, _, _, m, vx, vy] => ((real(vx)?, real(vy)?), mask(Some(m))?),
            _ => return Err(illegal()),
        };
        Ok(Body::rect(
            real(&args[0])?,
            real(&args[1])?,
            real(&args[2])?,
            real(&args[3])?,
            vel,
            mask,
        ))
    }

    pub(crate) fn sp_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        // These check the sprite's number before how many arguments and
        // OUT variables there are.
        if let (
            Command::Spofs
            | Command::Spcol
            | Command::Spcolor
            | Command::Sphome
            | Command::Sprot
            | Command::Spscale,
            Some(id),
        ) = (command, args.first())
        {
            sprite_id(id)?;
        }
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
                let z = match args.get(3) {
                    None | Some(Value::Default) => None,
                    Some(z) => Some(in_range(z, "Z", -4095..=4095)?),
                };
                let s = self.sprite(&args[0])?;
                if x.is_some() || y.is_some() {
                    stop(s, Target::Xy);
                }
                if z.is_some() {
                    stop(s, Target::Z);
                }
                s.pos = (x.unwrap_or(s.pos.0), y.unwrap_or(s.pos.1));
                s.z = z.unwrap_or(s.z);
                Ok(vec![])
            }
            // With the flag set, where the sprite shows once its links are
            // followed.
            (Command::Spofs, Want::Outs(n @ (2 | 3)), 1 | 2) => {
                let absolute = args.get(1).map(opt_int).transpose()?.flatten().unwrap_or(0) != 0;
                self.sprite(&args[0])?;
                let id = sprite_id(&args[0])?;
                let s = &self.sprites.list()[id];
                let (pos, z) = if absolute {
                    let pose = self.sprites.pose(id);
                    (pose.pos, pose.z)
                } else {
                    (s.pos, s.z)
                };
                let mut out = reals(&[pos.0, pos.1]);
                if n == 3 {
                    out.push(Value::Int(z));
                }
                Ok(out)
            }
            (Command::Sprot, Want::Outs(0), 2) => {
                let angle = real(&args[1])?;
                let s = self.sprite(&args[0])?;
                stop(s, Target::R);
                s.rot = crate::sprites::single(angle);
                Ok(vec![])
            }
            (Command::Sprot, Want::Value, 1) => Ok(vec![Value::Real(self.sprite(&args[0])?.rot)]),
            (Command::Spscale, Want::Outs(0), 3) => {
                let scale = (real(&args[1])?, real(&args[2])?);
                let s = self.sprite(&args[0])?;
                stop(s, Target::S);
                s.scale = (
                    crate::sprites::single(scale.0),
                    crate::sprites::single(scale.1),
                );
                Ok(vec![])
            }
            (Command::Spscale, Want::Outs(2), 1) => {
                let (x, y) = self.sprite(&args[0])?.scale;
                Ok(reals(&[x, y]))
            }
            (Command::Spcolor, Want::Outs(0), 2) => {
                let color = int(&args[1])? as u32;
                let s = self.sprite(&args[0])?;
                stop(s, Target::C);
                s.color = color;
                Ok(vec![])
            }
            (Command::Spcolor, Want::Value, 1) => {
                Ok(vec![Value::Int(self.sprite(&args[0])?.color as i32)])
            }
            (Command::Spchr, Want::Outs(0), 2) => {
                let n = def_id(&args[1])?;
                let def = self.sprites.defs[n];
                let s = self.sprite(&args[0])?;
                stop(s, Target::I);
                stop(s, Target::Uv);
                s.set_def(n as i32, &def);
                Ok(vec![])
            }
            (Command::Spchr, Want::Outs(0), n @ 3..=6) => {
                let field = |i: usize| args.get(i).map(opt_int).transpose().map(Option::flatten);
                let (u, v) = (field(1)?, field(2)?);
                // SPCHR ID,U,V,ATTR takes its fourth argument as the
                // attributes. W and H are the size as shown: with a
                // rotation of 90 or 270 degrees an empty W takes the
                // image's H, and a rotated result keeps W as the image's H.
                let (w, h, attr) = match n {
                    3 => (None, None, None),
                    4 => (None, None, field(3)?),
                    5 => (field(3)?, field(4)?, None),
                    _ => (field(3)?, field(4)?, field(5)?),
                };
                for (size, what, at) in [(w, "width", 3), (h, "height", 4)] {
                    if let Some(n) = size.filter(|n| !(0..=2048).contains(n)) {
                        return Err(out_of_range(what, n).of_arg(at));
                    }
                }
                if let Some(a) = attr.filter(|a| !(0..=0x1F).contains(a)) {
                    return Err(out_of_range("attribute", a).of_arg(n - 1));
                }
                let s = self.sprite(&args[0])?;
                stop(s, Target::Uv);
                s.u = u.unwrap_or(s.u);
                s.v = v.unwrap_or(s.v);
                let turned = |a: i32| a & 1 != 0;
                let (cw, ch) = if turned(s.attr) {
                    (s.h, s.w)
                } else {
                    (s.w, s.h)
                };
                (s.w, s.h, s.attr) = match n {
                    3 | 4 => (16, 16, attr.unwrap_or(0)),
                    5 => (w.unwrap_or(cw), h.unwrap_or(ch), 0),
                    _ => {
                        let (w, h, a) = (w.unwrap_or(cw), h.unwrap_or(ch), attr.unwrap_or(s.attr));
                        if turned(a) { (h, w, a) } else { (w, h, a) }
                    }
                };
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
            // Every sprite has its variables, set or not: BASICDEMO keeps
            // its menu positions in sprite 0, which it never sets.
            (Command::Spvar, Want::Outs(0), 3) => {
                let key = var_key(&args[1])?;
                let value = match &args[2] {
                    // An array is kept as itself, not copied.
                    Value::Int(_) | Value::Real(_) | Value::Str(_) | Value::Array(_) => {
                        args[2].clone()
                    }
                    _ => return Err(ErrorKind::TypeMismatch.into()),
                };
                self.sprites
                    .sprite_mut(sprite_id(&args[0])?)
                    .vars
                    .insert(key, value);
                Ok(vec![])
            }
            (Command::Spvar, Want::Value, 2) => {
                let key = var_key(&args[1])?;
                let s = &self.sprites.list()[sprite_id(&args[0])?];
                Ok(vec![s.vars.get(&key).cloned().unwrap_or(Value::Int(0))])
            }
            (Command::Spvar, Want::Outs(0), 1 | 2) => {
                let key = args.get(1).map(var_key).transpose()?;
                let s = self.sprites.sprite_mut(sprite_id(&args[0])?);
                match key {
                    Some(key) => {
                        s.vars.remove(&key);
                    }
                    None => s.vars.clear(),
                }
                Ok(vec![])
            }
            (Command::Spanim, Want::Outs(0), 0) => {
                for s in self.sprites.list_mut() {
                    s.anims = Default::default();
                }
                Ok(vec![])
            }
            (Command::Spanim, Want::Outs(0), 1) => {
                self.sprite(&args[0])?.anims = Default::default();
                Ok(vec![])
            }
            // The sprite is checked first, and named.
            (Command::Spanim, Want::Outs(0), 2) => {
                self.sprite(&args[0]).map_err(|e| e.of_arg(0))?;
                let n = anim_id(&args[1])?;
                // A number ANIMDEF never set starts nothing.
                match self.sprites.anim_defs[n].clone() {
                    Some(anim) => self.start_anim(&args[0], anim),
                    None => Ok(vec![]),
                }
            }
            (Command::Spanim, Want::Outs(0), _) => {
                self.sprite(&args[0]).map_err(|e| e.of_arg(0))?;
                let anim = self.anim_args(&args[1..])?;
                self.start_anim(&args[0], anim)
            }
            (Command::Animdef, Want::Outs(0), 0) => {
                self.sprites.anim_defs.fill(None);
                Ok(vec![])
            }
            (Command::Animdef, Want::Outs(0), 1) => {
                self.sprites.anim_defs[anim_id(&args[0])?] = None;
                Ok(vec![])
            }
            (Command::Animdef, Want::Outs(0), 3..) => {
                let n = anim_id(&args[0])?;
                self.sprites.anim_defs[n] = Some(self.anim_args(&args[1..])?);
                Ok(vec![])
            }
            (Command::Spstart | Command::Spstop, Want::Outs(0), 0 | 1) => {
                let stopped = command == Command::Spstop;
                match args.first() {
                    Some(id) => self.sprite(id)?.anim_stopped = stopped,
                    None => {
                        for s in self.sprites.list_mut() {
                            s.anim_stopped = stopped;
                        }
                    }
                }
                Ok(vec![])
            }
            // Checked on hardware: stopped animations report 0.
            (Command::Spchk, Want::Value, 1) => {
                let s = self.sprite(&args[0])?;
                if s.anim_stopped {
                    return Ok(vec![Value::Int(0)]);
                }
                let bits = (s.anims.iter().enumerate())
                    .filter(|(_, track)| track.is_some())
                    .fold(0, |bits, (i, _)| bits | 1 << i);
                Ok(vec![Value::Int(bits)])
            }
            (Command::Spcol, Want::Outs(0), _) => {
                self.spcol(args)?;
                Ok(vec![])
            }
            (Command::Spcol, Want::Outs(n @ (1 | 2 | 4..=6)), 1) => {
                // A sprite without SPCOL reports zeros.
                let c = self.sprite(&args[0])?.col.unwrap_or(Collision {
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 0,
                    scale: false,
                    mask: u32::MAX,
                });
                let all = [c.x, c.y, c.w, c.h, i32::from(c.scale), c.mask as i32];
                let n = n as usize;
                Ok(ints(if n <= 2 { &all[4..4 + n] } else { &all[..n] }))
            }
            (Command::Spcolvec, Want::Outs(0), 1 | 3) => {
                let vel = match args {
                    [_, x, y] => Some((real(x)?, real(y)?)),
                    _ => None,
                };
                self.sprite(&args[0])?.col_vel = vel;
                Ok(vec![])
            }
            (Command::Sphitsp, Want::Value, 0) => Ok(vec![Value::Int(self.hit(false, None))]),
            // A sprite that is not set hits nothing.
            (Command::Sphitsp, Want::Value, 1 | 3) => {
                let id = sprite_id(&args[0])?;
                let (first, last) = match args {
                    [_, a, b] => (sprite_id(a)?, sprite_id(b)?),
                    _ => (0, SPRITES - 1),
                };
                let found = match self.sprites.body(id) {
                    Some(source) => {
                        self.hit(false, Some(Search::new(source, first, last, Some(id))))
                    }
                    None => {
                        self.sprites.hit_sp = None;
                        -1
                    }
                };
                Ok(vec![Value::Int(found)])
            }
            (Command::Sphitsp, Want::Value, 2) => {
                let (id, other) = (sprite_id(&args[0])?, sprite_id(&args[1])?);
                let hit = match self.sprites.body(id) {
                    Some(source) if id != other => {
                        let mut search = Search::new(source, other, other, None);
                        self.sprites.next_hit(&mut search).is_some()
                    }
                    _ => false,
                };
                Ok(vec![Value::Int(i32::from(hit))])
            }
            (Command::Sphitrc, Want::Value, 0) => Ok(vec![Value::Int(self.hit(true, None))]),
            (Command::Sphitrc, Want::Value, 4 | 7) => {
                let source = Runtime::hit_rect(args)?;
                let found = self.hit(true, Some(Search::new(source, 0, SPRITES - 1, None)));
                Ok(vec![Value::Int(found)])
            }
            (Command::Sphitrc, Want::Value, 5 | 8) => {
                let id = sprite_id(&args[0])?;
                self.sprite(&args[0])?;
                let mut search = Search::new(Runtime::hit_rect(&args[1..])?, id, id, None);
                let hit = self.sprites.next_hit(&mut search).is_some();
                Ok(vec![Value::Int(i32::from(hit))])
            }
            (Command::Sphitrc, Want::Value, 6 | 9) => {
                let (first, last) = (sprite_id(&args[0])?, sprite_id(&args[1])?);
                let source = Runtime::hit_rect(&args[2..])?;
                let found = self.hit(true, Some(Search::new(source, first, last, None)));
                Ok(vec![Value::Int(found)])
            }
            (Command::Sphitinfo, Want::Value, 0) => {
                Ok(vec![Value::Real(self.sprites.hit_info.time)])
            }
            (Command::Sphitinfo, Want::Outs(n @ (5 | 9)), 0) => {
                let i = self.sprites.hit_info;
                let [(x1, y1), (x2, y2)] = i.pos;
                let [(vx1, vy1), (vx2, vy2)] = i.vel;
                Ok(if n == 5 {
                    reals(&[i.time, x1, y1, x2, y2])
                } else {
                    reals(&[i.time, x1, y1, vx1, vy1, x2, y2, vx2, vy2])
                })
            }
            (Command::Splink, Want::Outs(0), 2 | 3) => {
                let (child, parent) = (sprite_id(&args[0])?, sprite_id(&args[1])?);
                let flags = match args.get(2) {
                    None | Some(Value::Default) => 0,
                    Some(f) => in_range(f, "link flags", 0..=31)?,
                };
                if child == parent {
                    let e = Error::with_detail(ErrorKind::IllegalFunctionCall, "link to itself");
                    return Err(e.of_arg(1));
                }
                // Named here, unlike elsewhere.
                self.sprite(&args[0]).map_err(|e| e.of_arg(0))?;
                self.sprite(&args[1]).map_err(|e| e.of_arg(1))?;
                if self.sprites.descends_from(parent, child) {
                    let e = Error::with_detail(ErrorKind::CantLinkToDescendant, "link loop");
                    return Err(e.of_arg(1));
                }
                self.sprites.linked = true;
                let s = self.sprites.sprite_mut(child);
                (s.parent, s.link_flags) = (Some(parent), flags);
                Ok(vec![])
            }
            (Command::Splink, Want::Value, 1 | 2) => {
                let kind = args.get(1).map(opt_int).transpose()?.flatten().unwrap_or(0);
                self.sprite(&args[0])?;
                let id = sprite_id(&args[0])?;
                let s = &self.sprites.list()[id];
                let found = match kind {
                    0 => s.parent,
                    1 => self.sprites.children(id).next(),
                    2 => s
                        .parent
                        .and_then(|p| self.sprites.children(p).find(|&i| i > id)),
                    3 => return Ok(vec![Value::Int(s.link_flags)]),
                    _ => return Err(out_of_range("link type", kind)),
                };
                Ok(vec![Value::Int(found.map_or(-1, |i| i as i32))])
            }
            (Command::Spunlink, Want::Outs(0), 1) => {
                self.sprite(&args[0])?.parent = None;
                Ok(vec![])
            }
            (Command::Spfunc, Want::Outs(0), 1 | 2) => {
                let id = sprite_id(&args[0])?;
                let func = match args.get(1) {
                    Some(Value::Str(name)) => {
                        self.check_callback(&name.borrow())?;
                        Some(self.callback_name(&name.borrow()))
                    }
                    Some(_) => return Err(Error::new(ErrorKind::TypeMismatch).of_arg(1)),
                    None => None,
                };
                // A sprite that is not set takes it too.
                self.sprites.sprite_mut(id).func = func;
                Ok(vec![])
            }
            (Command::Spused, Want::Value, 1) => {
                let id = sprite_id(&args[0])?;
                Ok(vec![Value::Int(i32::from(self.sprites.list()[id].used))])
            }
            _ => Err(no_form()),
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
                    _ => return Err(no_form()),
                };
                Ok(ints(&out))
            }
            (Want::Outs(0), [source @ (Value::Array(_) | Value::Str(_)), rest @ ..])
                if rest.len() <= 3 =>
            {
                // An array's length is checked before anything else.
                if let Value::Array(a) = source
                    && !a.borrow().len().is_multiple_of(7)
                {
                    let e = Error::with_detail(ErrorKind::IllegalArraySize, "needs 7 values each");
                    return Err(e.of_arg(0));
                }
                // A label's count of templates comes first, then seven
                // values for each. A negative count sets none.
                let mut data = match source {
                    Value::Str(_) => {
                        let mut data = self.label_data(source)?;
                        let n = data.number()? as i32;
                        if n > DEFS as i32 {
                            return Err(out_of_range("data count", n));
                        }
                        Some((data, n.max(0) as usize))
                    }
                    _ => None,
                };
                let offset = rest.first().map(def_id).transpose()?.unwrap_or(0);
                let (du, dv) = match rest {
                    [_, u, v] => (int(u)?, int(v)?),
                    [] | [_] => (0, 0),
                    _ => return Err(no_form()),
                };
                // Templates past the last are left out.
                let room = self.sprites.defs.len().saturating_sub(offset);
                // DATA is read up to the first value missing, whose error
                // comes after the templates before it are set.
                let (values, missing) = match &mut data {
                    Some((data, n)) => {
                        let mut values = Vec::new();
                        let mut missing = None;
                        for _ in 0..(*n).min(room) {
                            match data.numbers(7) {
                                Ok(d) => values.extend(d.iter().map(|&x| f64::from(x as i32))),
                                Err(e) => {
                                    missing = Some(e);
                                    break;
                                }
                            }
                        }
                        (values, missing)
                    }
                    None => match source {
                        Value::Array(array) => (numbers(&array.borrow().data)?, None),
                        _ => unreachable!("an array or a label"),
                    },
                };
                drop(data);
                let count = (values.len() / 7).min(room);
                // Each is checked as it is set.
                for i in 0..count {
                    let d = &values[i * 7..i * 7 + 7];
                    let size = 0.0..=2048.0;
                    let home = -32768.0..=32767.0;
                    if !(size.contains(&d[2])
                        && size.contains(&d[3])
                        && home.contains(&d[4])
                        && home.contains(&d[5])
                        && (0.0..=31.0).contains(&d[6]))
                    {
                        let what = format!("template {}", offset + i);
                        return Err(Error::with_detail(ErrorKind::OutOfRange, what));
                    }
                    self.sprites.defs[offset + i] = SpriteDef {
                        u: (d[0] as i32).wrapping_add(du),
                        v: (d[1] as i32).wrapping_add(dv),
                        w: d[2] as i32,
                        h: d[3] as i32,
                        hx: d[4] as i32,
                        hy: d[5] as i32,
                        attr: d[6] as i32,
                    };
                }
                match missing {
                    Some(e) => Err(e),
                    None => Ok(vec![]),
                }
            }
            // SPDEF id,source,[U],[V],[W],[H],[HX],[HY],[attributes]
            // Each value given is checked, and then that the image stays
            // on the page.
            (Want::Outs(0), [id, source, fields @ ..]) if fields.len() == 7 => {
                let id = def_id(id)?;
                let mut d = self.sprites.defs[def_id(source)?];
                let mut slots = [
                    (&mut d.u, "U", 0..=2048),
                    (&mut d.v, "V", 0..=2048),
                    (&mut d.w, "width", 0..=2048),
                    (&mut d.h, "height", 0..=2048),
                    (&mut d.hx, "home X", -32768..=32767),
                    (&mut d.hy, "home Y", -32768..=32767),
                    (&mut d.attr, "attribute", 0..=31),
                ];
                for ((slot, what, range), field) in slots.iter_mut().zip(fields) {
                    if !matches!(field, Value::Default) {
                        **slot = in_range(field, what, range.clone())?;
                    }
                }
                if d.u + d.w > 2048 || d.v + d.h > 2048 {
                    return Err(out_of_range("image corner", d.u + d.w));
                }
                self.sprites.defs[id] = d;
                Ok(vec![])
            }
            // SPDEF id,U,V[,W,H[,HX,HY]][,attributes]
            (Want::Outs(0), [id, u, v, rest @ ..]) if rest.len() <= 5 => {
                let id = def_id(id)?;
                let (u, v) = (
                    in_range(u, "U", 0..=i32::MAX)?,
                    in_range(v, "V", 0..=i32::MAX)?,
                );
                let (size_args, home_args, attr) = match rest {
                    [] => (None, None, None),
                    [a] => (None, None, Some(a)),
                    [w, h] => (Some((w, h)), None, None),
                    [w, h, a] => (Some((w, h)), None, Some(a)),
                    [w, h, x, y] => (Some((w, h)), Some((x, y)), None),
                    [w, h, x, y, a] => (Some((w, h)), Some((x, y)), Some(a)),
                    _ => unreachable!("at most five"),
                };
                let size = |v| in_range(v, "size", 0..=2048);
                let (w, h) = match size_args {
                    Some((w, h)) => (size(w)?, size(h)?),
                    None => (16, 16),
                };
                let home = |v| in_range(v, "home", -32768..=32767);
                let (hx, hy) = match home_args {
                    Some((x, y)) => (home(x)?, home(y)?),
                    None => (0, 0),
                };
                let attr = attr.map(|a| in_range(a, "attribute", 0..=31));
                self.sprites.defs[id] = SpriteDef {
                    u,
                    v,
                    w,
                    h,
                    hx,
                    hy,
                    attr: attr.transpose()?.unwrap_or(0),
                };
                Ok(vec![])
            }
            _ => Err(no_form()),
        }
    }
}

/// An omittable real argument.
fn optional_real(v: &Value) -> Result<Option<f64>> {
    checking(v, || match v {
        Value::Default => Ok(None),
        v => v.to_real().map(Some),
    })
}

/// Setting what an animation changes stops that animation, so that SPCHK
/// no longer shows it.
fn stop(s: &mut Sprite, target: Target) {
    s.anims[target.index()] = None;
}

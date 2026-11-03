//! Sprites 0-4095 and their definition templates (SPDEF). Sprite
//! `#GSPRITE` (4095) shows the graphic page GRP0 from start-up.

use std::collections::HashMap;

use sb_core::value::Value;

use crate::anim::{self, ANIM_VAR, Anim, Animated, Target, Track};
use crate::collision::{Body, Collision, HitInfo, Search};
use crate::compose::{self, Affine, Paint, Placement, draw_image};
use crate::framebuffer::Framebuffer;
use crate::graphics::{Clip, GRP_SIZE, Graphics, PAGES, PIECE, Tint, TintJob, multiply};

pub const SPRITES: usize = 4096;
pub const DEFS: usize = 8192;
/// Templates in the first release's table.
pub const FIRST_RELEASE_DEFS: usize = 4096;
/// `#GSPRITE`: the sprite that shows the graphic screen.
pub const GSPRITE: usize = 4095;
/// SPPAGE at start-up: sprites take their images from GRP4.
const DEFAULT_PAGE: i32 = 4;

/// Attribute bits (`#A_ROT90` and so on).
pub mod attr {
    pub const ROTATION: i32 = 0b11;
    pub const REVH: i32 = 0b100;
    pub const REVV: i32 = 0b1000;
    pub const ADD: i32 = 0b1_0000;
}

/// SPLINK flags: what a child takes from its parent besides position and Z.
pub mod link {
    /// Shown only while the parent is, and cleared a frame after it.
    pub const SHOWN: i32 = 1;
    pub const ROTATION: i32 = 2;
    pub const SCALE: i32 = 4;
    pub const COLOR: i32 = 8;
    pub const LAYER: i32 = 16;
}

/// An SPDEF template: image rectangle, home point and attributes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpriteDef {
    pub u: i32,
    pub v: i32,
    pub w: i32,
    pub h: i32,
    pub hx: i32,
    pub hy: i32,
    pub attr: i32,
}

/// Bytes per template as SmileBASIC 4 stores them: seven u16.
pub const DEF_BYTES: usize = 14;

impl SpriteDef {
    fn from_bytes(b: &[u8]) -> SpriteDef {
        let f = |i: usize| i32::from(u16::from_le_bytes([b[2 * i], b[2 * i + 1]]));
        SpriteDef {
            u: f(0),
            v: f(1),
            w: f(2),
            h: f(3),
            hx: f(4),
            hy: f(5),
            attr: f(6),
        }
    }

    fn fits_page(&self) -> bool {
        self.u + self.w <= GRP_SIZE && self.v + self.h <= GRP_SIZE && self.attr < 32
    }
}

/// Reads a table of [`DEFS`] templates.
pub fn defs_from_bytes(bytes: &[u8]) -> Option<Vec<SpriteDef>> {
    if bytes.len() != DEFS * DEF_BYTES {
        return None;
    }
    let defs: Vec<SpriteDef> = bytes
        .chunks_exact(DEF_BYTES)
        .map(SpriteDef::from_bytes)
        .collect();
    defs.iter().all(SpriteDef::fits_page).then_some(defs)
}

/// Templates for when none were imported: a grid of 16x16 cells.
// UNVERIFIED stand-in; SmileBASIC 4's own table is imported from the
// user's copy.
fn grid_defs() -> Vec<SpriteDef> {
    (0..DEFS as i32)
        .map(|n| SpriteDef {
            u: n % 128 * 16,
            v: n / 128 * 16,
            w: 16,
            h: 16,
            ..SpriteDef::default()
        })
        .collect()
}

/// A sprite with the templates that animating its definition number
/// (target "I") takes.
pub struct WithDefs<'a> {
    sprite: &'a mut Sprite,
    defs: &'a [SpriteDef],
}

impl<'a> WithDefs<'a> {
    pub fn new(sprite: &'a mut Sprite, defs: &'a [SpriteDef]) -> WithDefs<'a> {
        WithDefs { sprite, defs }
    }
}

impl Animated for WithDefs<'_> {
    fn value(&self, target: Target) -> [f64; 2] {
        let s = &self.sprite;
        match target {
            Target::Xy => [s.pos.0, s.pos.1],
            Target::Z => [f64::from(s.z), 0.0],
            Target::R => [s.rot, 0.0],
            Target::S => [s.scale.0, s.scale.1],
            Target::C => [f64::from(s.color), 0.0],
            Target::V => [var_number(&s.vars), 0.0],
            Target::Uv => [f64::from(s.u), f64::from(s.v)],
            Target::I => [f64::from(s.def), 0.0],
        }
    }

    // Integer properties take the value rounded towards zero.
    fn set_value(&mut self, target: Target, v: [f64; 2]) {
        let s = &mut self.sprite;
        match target {
            Target::Xy => s.pos = (v[0], v[1]),
            Target::Z => s.z = (v[0] as i32).clamp(-4095, 4095),
            Target::R => s.rot = single(v[0]),
            Target::S => s.scale = (single(v[0]), single(v[1])),
            Target::C => s.color = v[0] as i64 as u32,
            Target::V => {
                s.vars.insert(VarKey::Int(ANIM_VAR), Value::Real(v[0]));
            }
            Target::Uv => (s.u, s.v) = (v[0] as i32, v[1] as i32),
            Target::I => {
                let n = (v[0] as i32).clamp(0, DEFS as i32 - 1);
                if let Some(def) = self.defs.get(n as usize) {
                    s.set_def(n, def);
                }
            }
        }
    }
}

/// Variable 7, which target "V" animates, as a number.
pub fn var_number(vars: &HashMap<VarKey, Value>) -> f64 {
    let v = vars.get(&VarKey::Int(ANIM_VAR));
    v.and_then(|v| v.to_real().ok()).unwrap_or(0.0)
}

/// A key of a sprite variable (SPVAR): a number or a string.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum VarKey {
    Int(i32),
    Str(Vec<u16>),
}

#[derive(Clone, Debug)]
pub struct Sprite {
    pub used: bool,
    pub shown: bool,
    /// The template number SPCHR() reports, if set from one.
    pub def: i32,
    pub page: i32,
    pub u: i32,
    pub v: i32,
    pub w: i32,
    pub h: i32,
    pub attr: i32,
    pub home: (f64, f64),
    pub pos: (f64, f64),
    /// Smaller is nearer the front: -4095 to 4095.
    pub z: i32,
    /// Degrees, clockwise. Rotation and scale are kept in single
    /// precision; see [`single`].
    pub rot: f64,
    pub scale: (f64, f64),
    pub color: u32,
    pub layer: i32,
    pub vars: HashMap<VarKey, Value>,
    /// Animations playing, by [`anim::Target`].
    pub anims: [Option<Track>; anim::TARGETS],
    /// SPSTOP pauses the animations until SPSTART.
    pub anim_stopped: bool,
    /// SPCOL's settings; sprites without them never collide.
    // UNVERIFIED: whether SPSET gives sprites a rectangle already.
    pub col: Option<Collision>,
    /// SPCOLVEC's movement, or `None` to use `anim_move`.
    pub col_vel: Option<(f64, f64)>,
    /// How far an XY slide moved the sprite in the last frame.
    pub anim_move: (f64, f64),
    /// SPLINK's parent; position and Z are then relative to it.
    pub parent: Option<usize>,
    pub link_flags: i32,
    /// Cleared on the next frame, as its parent was.
    pub clear_pending: bool,
    /// SPFUNC's callback for CALL SPRITE: a `"@label"` or a DEF name.
    pub func: Option<Vec<u16>>,
}

/// A number as SmileBASIC 4 keeps a sprite's rotation and scale: in
/// single precision, so that `SPROT 0,-3.3` reads back -3.29999995.
pub fn single(v: f64) -> f64 {
    f64::from(v as f32)
}

/// Where and how a sprite shows once its links are followed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub pos: (f64, f64),
    pub z: i32,
    pub rot: f64,
    pub scale: (f64, f64),
    pub color: u32,
    pub shown: bool,
    pub layer: i32,
}

impl Pose {
    /// A sprite's own settings, as if it had no parent.
    pub fn of(s: &Sprite) -> Pose {
        Pose {
            pos: s.pos,
            z: s.z,
            rot: s.rot,
            scale: s.scale,
            color: s.color,
            shown: s.shown,
            layer: s.layer,
        }
    }
}

/// Everything of a sprite that its drawing depends on, to tell whether a
/// command changed how it shows.
#[derive(Clone, Copy, PartialEq)]
pub struct Appearance {
    used: bool,
    shown: bool,
    image: (i32, i32, i32, i32, i32, i32),
    home: (f64, f64),
    pos: (f64, f64),
    z: i32,
    rot: f64,
    scale: (f64, f64),
    color: u32,
    layer: i32,
    link: (Option<usize>, i32),
}

impl Sprite {
    pub fn appearance(&self) -> Appearance {
        Appearance {
            used: self.used,
            shown: self.shown,
            image: (self.page, self.u, self.v, self.w, self.h, self.attr),
            home: self.home,
            pos: self.pos,
            z: self.z,
            rot: self.rot,
            scale: self.scale,
            color: self.color,
            layer: self.layer,
            link: (self.parent, self.link_flags),
        }
    }

    fn unused() -> Sprite {
        Sprite {
            used: false,
            shown: false,
            def: 0,
            page: DEFAULT_PAGE,
            u: 0,
            v: 0,
            w: 0,
            h: 0,
            attr: 0,
            home: (0.0, 0.0),
            pos: (0.0, 0.0),
            z: 0,
            rot: 0.0,
            scale: (1.0, 1.0),
            color: 0xFFFF_FFFF,
            layer: 0,
            vars: HashMap::new(),
            anims: Default::default(),
            anim_stopped: false,
            col: None,
            col_vel: None,
            anim_move: (0.0, 0.0),
            parent: None,
            link_flags: 0,
            clear_pending: false,
            func: None,
        }
    }

    /// Takes the image, home point and attributes of a template.
    pub fn set_def(&mut self, n: i32, def: &SpriteDef) {
        self.def = n;
        (self.u, self.v, self.w, self.h) = (def.u, def.v, def.w, def.h);
        self.home = (f64::from(def.hx), f64::from(def.hy));
        self.attr = def.attr;
    }
}

pub struct Sprites {
    pub list: Vec<Sprite>,
    /// Whether SPLINK has been used, as most programs never do and looking
    /// for a sprite's children goes through them all.
    pub linked: bool,
    pub defs: Vec<SpriteDef>,
    /// What SPDEF without arguments restores.
    default_defs: Vec<SpriteDef>,
    /// SPPAGE and SPLAYER for sprites created from now on.
    pub new_page: i32,
    pub new_layer: i32,
    /// ANIMDEF definitions.
    pub anim_defs: Vec<Option<Anim>>,
    /// The program has not been woken by a frame: it has not waited since
    /// it last ran out of frame or started, or its VSYNC found the frame
    /// gone by. An SPANIM set now starts a frame later than one set after a
    /// VSYNC that waited.
    // UNVERIFIED: the rule fits TEST48 and TEST56 (2 late, 3 not); the
    // cause is likely when SmileBASIC 4's thread runs against its frame.
    pub behind: bool,
    /// Where SPHITSP() and SPHITRC() without arguments go on searching.
    pub hit_sp: Option<Search>,
    pub hit_rc: Option<Search>,
    /// What SPHITINFO reports.
    pub hit_info: HitInfo,
    /// A bit for each sprite that may have SPCOL's settings, as most have
    /// none and a search would otherwise go through them all.
    may_collide: [u64; SPRITES / 64],
}

impl Default for Sprites {
    fn default() -> Self {
        Sprites::new(grid_defs())
    }
}

impl Sprites {
    /// The definitions that ACLS and SPDEF with no arguments go back to.
    pub(crate) fn default_defs(&self) -> &[SpriteDef] {
        &self.default_defs
    }

    pub fn new(defs: Vec<SpriteDef>) -> Sprites {
        let mut s = Sprites {
            list: vec![Sprite::unused(); SPRITES],
            linked: false,
            defs: defs.clone(),
            default_defs: defs,
            new_page: DEFAULT_PAGE,
            new_layer: 0,
            anim_defs: vec![None; anim::DEFINITIONS],
            behind: true,
            hit_sp: None,
            hit_rc: None,
            hit_info: HitInfo::default(),
            may_collide: [0; SPRITES / 64],
        };
        s.reset_gsprite((400, 240));
        s
    }

    /// Sprite 4095 shows the top-left `w` x `h` of GRP0 behind everything
    /// else, on the layer `SPLAYER` gives new sprites.
    pub fn reset_gsprite(&mut self, (w, h): (u32, u32)) {
        let layer = self.new_layer;
        self.no_collision(GSPRITE);
        let g = &mut self.list[GSPRITE];
        *g = Sprite::unused();
        (g.used, g.shown, g.page, g.layer) = (true, true, 0, layer);
        (g.w, g.h) = (w as i32, h as i32);
        g.z = 4095;
    }

    /// ACLS: every sprite cleared but `#GSPRITE`, which the caller resets,
    /// and the templates and animations unless kept.
    pub fn reset(&mut self, keep_defs: bool, keep_anims: bool) {
        self.clear_all();
        (self.new_page, self.new_layer) = (DEFAULT_PAGE, 0);
        (self.hit_sp, self.hit_rc) = (None, None);
        if !keep_defs {
            self.reset_defs();
        }
        if !keep_anims {
            self.anim_defs = vec![None; anim::DEFINITIONS];
        }
    }

    /// Clears every sprite's variables, as starting a program does.
    pub fn clear_vars(&mut self) {
        for s in &mut self.list {
            s.vars.clear();
        }
    }

    /// SPDEF without arguments.
    pub fn reset_defs(&mut self) {
        self.defs.clone_from(&self.default_defs);
    }

    /// SPSET: a fresh sprite with the current SPPAGE and SPLAYER.
    pub fn create(&mut self, id: usize, shown: bool) -> &mut Sprite {
        self.no_collision(id);
        let s = &mut self.list[id];
        *s = Sprite::unused();
        (s.used, s.shown) = (true, shown);
        (s.page, s.layer) = (self.new_page, self.new_layer);
        s
    }

    /// SPCLR of one sprite. Children that follow its display go a frame
    /// later; the others are unlinked, staying where they show (checked
    /// on hardware for the position).
    // UNVERIFIED: that Z also becomes absolute.
    pub fn clear(&mut self, id: usize) {
        if self.linked {
            for child in self.children(id).collect::<Vec<_>>() {
                self.unlink(child);
            }
        }
        self.no_collision(id);
        self.list[id] = Sprite::unused();
    }

    /// SPCOL's settings of a sprite.
    pub fn set_collision(&mut self, id: usize, col: Collision) {
        self.list[id].col = Some(col);
        self.may_collide[id / 64] |= 1 << (id % 64);
    }

    fn no_collision(&mut self, id: usize) {
        self.may_collide[id / 64] &= !(1 << (id % 64));
    }

    /// Takes a sprite off its parent, keeping where it shows.
    fn unlink(&mut self, id: usize) {
        let pose = self.pose(id);
        let s = &mut self.list[id];
        if s.parent.take().is_some() {
            (s.pos, s.z) = (pose.pos, pose.z);
            s.clear_pending |= s.link_flags & link::SHOWN != 0;
        }
    }

    /// SPCLR without arguments: every sprite but `#GSPRITE`.
    pub fn clear_all(&mut self) {
        for s in &mut self.list[..GSPRITE] {
            *s = Sprite::unused();
        }
        self.may_collide = [0; SPRITES / 64];
        if self.list[GSPRITE].col.is_some() {
            self.may_collide[GSPRITE / 64] |= 1 << (GSPRITE % 64);
        }
        self.unlink(GSPRITE);
    }

    /// Whether `ancestor` is `id` or one of its parents.
    pub fn descends_from(&self, id: usize, ancestor: usize) -> bool {
        let mut at = Some(id);
        while let Some(i) = at {
            if i == ancestor {
                return true;
            }
            at = self.list[i].parent;
        }
        false
    }

    /// The children of `id`, in id order.
    // UNVERIFIED: SPLINK(n,1) and SPLINK(n,2) go through children by id.
    pub fn children(&self, id: usize) -> impl Iterator<Item = usize> + '_ {
        (0..SPRITES).filter(move |&i| self.list[i].used && self.list[i].parent == Some(id))
    }

    /// A sprite's pose with its parents' applied.
    // UNVERIFIED: an inherited rotation or scale also turns or scales the
    // child's offset from its parent.
    pub fn pose(&self, id: usize) -> Pose {
        let s = &self.list[id];
        let own = Pose::of(s);
        let Some(parent) = s.parent else { return own };
        let p = self.pose(parent);
        let f = s.link_flags;
        let mut pose = own;
        // The offset is in the parent's turned and scaled space whatever
        // the flags (checked on hardware); they decide whether the child's
        // own image turns and scales too.
        let (dx, dy) = (s.pos.0 * p.scale.0, s.pos.1 * p.scale.1);
        let (sin, cos) = p.rot.to_radians().sin_cos();
        let (dx, dy) = (dx * cos - dy * sin, dx * sin + dy * cos);
        if f & link::SCALE != 0 {
            pose.scale = (own.scale.0 * p.scale.0, own.scale.1 * p.scale.1);
        }
        if f & link::ROTATION != 0 {
            pose.rot += p.rot;
        }
        pose.pos = (p.pos.0 + dx, p.pos.1 + dy);
        pose.z = p.z + own.z;
        if f & link::SHOWN != 0 {
            pose.shown &= p.shown;
        }
        if f & link::COLOR != 0 {
            pose.color = multiply(own.color, p.color);
        }
        if f & link::LAYER != 0 {
            pose.layer = p.layer;
        }
        pose
    }

    /// Plays one frame of every sprite's animations, and gives whether
    /// any sprite shows differently. Sprites whose animation asks for it
    /// are cleared when it ends.
    pub fn animate(&mut self) -> bool {
        let mut moved = false;
        for id in 0..SPRITES {
            if self.list[id].clear_pending {
                self.clear(id);
                moved = true;
            }
        }
        for id in 0..SPRITES {
            let s = &mut self.list[id];
            if !s.used || s.anim_stopped {
                s.anim_move = (0.0, 0.0);
                continue;
            }
            if s.anims.iter().all(Option::is_none) {
                s.anim_move = (0.0, 0.0);
                continue;
            }
            let shown_as = s.appearance();
            let mut tracks = std::mem::take(&mut s.anims);
            let mut delete = false;
            let before = s.pos;
            s.anim_move = (0.0, 0.0);
            for (i, slot) in tracks.iter_mut().enumerate() {
                let Some(track) = slot else { continue };
                if std::mem::take(&mut track.late) {
                    continue;
                }
                let going = track.step(&mut WithDefs::new(s, &self.defs));
                if i == Target::Xy.index() && track.slid() {
                    s.anim_move = (s.pos.0 - before.0, s.pos.1 - before.1);
                }
                if !going {
                    delete |= track.mode().delete;
                    *slot = None;
                }
            }
            s.anims = tracks;
            moved |= s.appearance() != shown_as;
            if delete {
                // UNVERIFIED: #GSPRITE is cleared like any other.
                self.clear(id);
                moved = true;
            }
        }
        moved
    }

    /// The body a sprite collides with, if SPCOL has been set.
    /// Hidden sprites do not collide (checked on hardware).
    // UNVERIFIED: whether a sprite hidden through its link (SPLINK bit0)
    // counts as hidden.
    pub fn body(&self, id: usize) -> Option<Body> {
        let s = &self.list[id];
        let c = s.col.as_ref().filter(|_| s.used)?;
        let pose = self.pose(id);
        if !pose.shown {
            return None;
        }
        Some(Body::sprite(
            c,
            pose.pos,
            pose.scale,
            s.col_vel.unwrap_or(s.anim_move),
        ))
    }

    /// The next sprite `search` hits, noting the details for SPHITINFO.
    pub fn next_hit(&mut self, search: &mut Search) -> Option<usize> {
        while search.rest.start < search.rest.end {
            let from = search.rest.start;
            let bits = self.may_collide[from / 64] >> (from % 64);
            if bits == 0 {
                search.rest.start = (from / 64 + 1) * 64;
                continue;
            }
            let id = from + bits.trailing_zeros() as usize;
            if id >= search.rest.end {
                search.rest.start = search.rest.end;
                break;
            }
            search.rest.start = id + 1;
            if Some(id) == search.skip {
                continue;
            }
            // What body() gives, without a pose for a sprite with no parent.
            let s = &self.list[id];
            let Some(c) = s
                .col
                .as_ref()
                .filter(|c| s.used && c.mask & search.source.mask != 0)
            else {
                continue;
            };
            let body = if s.parent.is_some() {
                self.body(id)
            } else if s.shown {
                let vel = s.col_vel.unwrap_or(s.anim_move);
                Some(Body::sprite(c, s.pos, s.scale, vel))
            } else {
                None
            };
            if let Some(body) = body
                && let Some(t) = crate::collision::sweep(&search.source, &body)
            {
                self.hit_info = HitInfo::new(t, &search.source, &body);
                return Some(id);
            }
        }
        None
    }

    /// Shown sprites with their poses, back to front: among equal Z,
    /// lower ids are in front.
    pub fn draw_order(&self) -> Vec<(usize, Pose)> {
        let mut shown: Vec<(usize, Pose)> = (0..SPRITES)
            .filter(|&i| self.list[i].used)
            .map(|i| (i, self.pose(i)))
            .filter(|(_, pose)| pose.shown)
            .collect();
        shown.sort_by_key(|&(i, pose)| (std::cmp::Reverse(pose.z), std::cmp::Reverse(i)));
        shown
    }
}

/// The page pixel a sprite shows at `(x, y)` of its own rectangle (before
/// rotation and flipping). GRP-1 is white everywhere; off a page is
/// transparent.
pub fn sample(grp: &Graphics, page: i32, x: i32, y: i32) -> u32 {
    let Ok(p) = usize::try_from(page) else {
        return 0xFFFF_FFFF;
    };
    if p >= PAGES {
        return 0;
    }
    if grp.sample[p] == 1 {
        grp.pixel(p, x.rem_euclid(GRP_SIZE), y.rem_euclid(GRP_SIZE))
    } else {
        grp.pixel(p, x, y)
    }
}

/// Draws one sprite over the frame, as if it had no parent or layer.
pub fn draw(s: &Sprite, grp: &Graphics, fb: &mut Framebuffer) {
    draw_posed(s, &Pose::of(s), grp, fb, &Affine::IDENTITY, None, None);
}

/// Draws one sprite over the frame in the given pose, then through its
/// layer's matrix and clipping, and through LMATRIX's 4x4 matrix with the
/// frame's pixels to a layer pixel when the layer has one.
pub fn draw_posed(
    s: &Sprite,
    pose: &Pose,
    grp: &Graphics,
    fb: &mut Framebuffer,
    layer: &Affine,
    clip: Option<&Clip>,
    projection: Option<(&[f64; 16], u32)>,
) {
    // A blank page shows nothing, as GRP0 does through #GSPRITE until
    // something is drawn there.
    if usize::try_from(s.page).is_ok_and(|p| p >= PAGES || grp.is_blank(p)) {
        return;
    }
    // A clear color makes every pixel clear, which draws nothing.
    if pose.color >> 24 == 0 {
        return;
    }
    // The shown rectangle swaps sides for quarter turns.
    let quarter = s.attr & attr::ROTATION;
    let (bw, bh) = if quarter % 2 == 1 {
        (s.h, s.w)
    } else {
        (s.w, s.h)
    };
    let place = Placement {
        matrix: Affine::place(pose.pos, s.home, pose.rot, pose.scale).then(layer),
        clip,
        add: s.attr & attr::ADD != 0,
    };
    let source = Source::new(s, grp);
    // Roughly how many pixels the sprite covers.
    let area = (f64::from(bw * bh) * (pose.scale.0 * pose.scale.1).abs()).min(1e7) as usize;
    let tint = Tint::new(pose.color, area);
    if let Some((m, sample)) = projection {
        struct Projected<'a, 'b> {
            fb: &'a mut Framebuffer,
            place: &'a Placement<'b>,
            m: &'a [f64; 16],
            sample: u32,
            size: (i32, i32),
            source: &'a Source<'a>,
        }
        impl TintJob for Projected<'_, '_> {
            type Out = ();
            fn run<P: Paint>(self, paint: impl Fn(u32) -> P + Copy) {
                let (bw, bh) = self.size;
                let source = self.source;
                // The usual image: every point is on the page as it is.
                if let Some((page, u, v)) = source.plain() {
                    let (fb, m, sample, place) = (self.fb, self.m, self.sample, self.place);
                    return compose::project_image(fb, m, sample, place, bw, bh, |x, y| {
                        let p = page[(v + y as usize) * GRP_SIZE as usize + u + x as usize];
                        (p >> 24 != 0).then(|| paint(p))
                    });
                }
                compose::project_image(self.fb, self.m, self.sample, self.place, bw, bh, |x, y| {
                    let (x, y) = source.page_xy(x, y);
                    let p = source.pixel(x, y);
                    (p >> 24 != 0).then(|| paint(p))
                });
            }
        }
        return tint.with(
            place.add,
            Projected {
                fb,
                place: &place,
                m,
                sample,
                size: (bw, bh),
                source: &source,
            },
        );
    }
    let tie = match place.is_aligned() {
        true => (0.0, 0.0),
        false => place.texel_tie((bw, bh), source.reversed()),
    };
    if place.is_upright() {
        return draw_upright(&source, &tint, fb, &place, (bw, bh), tie, grp);
    }
    struct Turned<'a, 'b> {
        fb: &'a mut Framebuffer,
        place: &'a Placement<'b>,
        size: (i32, i32),
        tie: (f64, f64),
        source: &'a Source<'a>,
    }
    impl TintJob for Turned<'_, '_> {
        type Out = ();
        fn run<P: Paint>(self, paint: impl Fn(u32) -> P + Copy) {
            let (bw, bh) = self.size;
            let source = self.source;
            let (tx, ty) = self.tie;
            // At most angles nothing ties, and this is for each pixel.
            if (tx, ty) == (0.0, 0.0) {
                return draw_image(self.fb, self.place, bw, bh, |x, y| {
                    let (x, y) = source.page_xy(x as i32, y as i32);
                    let p = source.pixel(x, y);
                    (p >> 24 != 0).then(|| paint(p))
                });
            }
            draw_image(self.fb, self.place, bw, bh, |x, y| {
                let (x, y) = source.page_xy(texel(x + tx, bw), texel(y + ty, bh));
                let p = source.pixel(x, y);
                (p >> 24 != 0).then(|| paint(p))
            });
        }
    }
    tint.with(
        place.add,
        Turned {
            fb,
            place: &place,
            size: (bw, bh),
            tie,
            source: &source,
        },
    );
}

/// The texel of an image `size` long that a point lies in. A point moved
/// to settle a tie can land just off the image.
#[inline]
fn texel(v: f64, size: i32) -> i32 {
    (v as i32).min(size - 1)
}

/// Where a sprite's pixels come from.
struct Source<'a> {
    page: &'a [u32],
    /// Which page it is; `None` for GRP-1.
    page_no: Option<usize>,
    /// GSAMPLE 1: the page repeats in every direction.
    repeat: bool,
    /// GRP-1, white everywhere.
    white: bool,
    u: i32,
    v: i32,
    w: i32,
    h: i32,
    attr: i32,
}

impl Source<'_> {
    /// For a sprite whose page is GRP-1 or one that is not blank.
    fn new<'a>(s: &Sprite, grp: &'a Graphics) -> Source<'a> {
        let (page, repeat, white) = match usize::try_from(s.page) {
            Ok(p) => (grp.page(p), grp.sample[p] == 1, false),
            Err(_) => (&[][..], true, true),
        };
        Source {
            page,
            page_no: usize::try_from(s.page).ok(),
            repeat,
            white,
            u: s.u,
            v: s.v,
            w: s.w,
            h: s.h,
            attr: s.attr,
        }
    }

    /// The page and where the image starts on it, for an image neither
    /// turned nor flipped that lies wholly on a page.
    fn plain(&self) -> Option<(&[u32], usize, usize)> {
        let on_page = |at: i32, size: i32| at >= 0 && at + size <= GRP_SIZE;
        let plain = !self.white
            && self.attr & (attr::ROTATION | attr::REVH | attr::REVV) == 0
            && on_page(self.u, self.w)
            && on_page(self.v, self.h)
            && self.page.len() == (GRP_SIZE * GRP_SIZE) as usize;
        plain.then_some((self.page, self.u as usize, self.v as usize))
    }

    /// The page point under the point (lx, ly) of the shown rectangle.
    /// The image is flipped, then turned (checked on hardware): undo the
    /// turn, then the flips.
    #[inline]
    fn page_xy(&self, lx: i32, ly: i32) -> (i32, i32) {
        let (w, h) = (self.w, self.h);
        let (mut ix, mut iy) = match self.attr & attr::ROTATION {
            0 => (lx, ly),
            1 => (ly, h - 1 - lx),
            2 => (w - 1 - lx, h - 1 - ly),
            _ => (w - 1 - ly, lx),
        };
        if self.attr & attr::REVH != 0 {
            ix = w - 1 - ix;
        }
        if self.attr & attr::REVV != 0 {
            iy = h - 1 - iy;
        }
        (self.u + ix, self.v + iy)
    }

    /// Whether the page's coordinates go down along the shown rectangle's
    /// x and y.
    fn reversed(&self) -> (bool, bool) {
        let (x, y) = self.page_xy(0, 0);
        let (x1, y1) = self.page_xy(1, 0);
        let (x2, y2) = self.page_xy(0, 1);
        (x1 + y1 < x + y, x2 + y2 < x + y)
    }

    /// A page coordinate as it is read: wrapped around when the page
    /// repeats, or `None` off the page.
    #[inline]
    fn wrap(&self, v: i32) -> Option<usize> {
        match self.repeat {
            true => Some(v.rem_euclid(GRP_SIZE) as usize),
            false => (0..GRP_SIZE).contains(&v).then_some(v as usize),
        }
    }

    /// The page pixel at a page point, as [`sample`] gives it.
    #[inline]
    fn pixel(&self, x: i32, y: i32) -> u32 {
        if self.white {
            return 0xFFFF_FFFF;
        }
        match (self.wrap(x), self.wrap(y)) {
            (Some(x), Some(y)) => self.page[y * GRP_SIZE as usize + x],
            _ => 0,
        }
    }
}

/// Off the page, as an offset into it.
const NONE: usize = usize::MAX;
/// Frame pixels from which a sprite is drawn piece by piece.
const LARGE: usize = 4096;

/// [`draw_posed`] for a sprite neither turned nor sheared on the frame,
/// the usual case: which page column and row each frame column and row
/// shows is worked out once, so each pixel is a lookup and a blend.
fn draw_upright(
    source: &Source,
    tint: &Tint,
    fb: &mut Framebuffer,
    place: &Placement,
    (bw, bh): (i32, i32),
    (tx, ty): (f64, f64),
    grp: &Graphics,
) {
    let Some(grid) = compose::grid(fb, place, bw, bh) else {
        return;
    };
    let stride = GRP_SIZE as usize;
    let at = |v: i32, step: usize| source.wrap(v).map_or(NONE, |v| v * step);
    // A quarter turn makes frame columns go down the page, and rows
    // across it.
    let turned = (source.attr & attr::ROTATION) % 2 == 1;
    let cols: Vec<usize> = (grid.xs.iter())
        .map(|&x| {
            let (px, py) = source.page_xy(texel(x + tx, bw), 0);
            match turned {
                false => at(px, 1),
                true => at(py, stride),
            }
        })
        .collect();
    let rows = grid.ys.iter().map(|&y| {
        let (px, py) = source.page_xy(0, texel(y + ty, bh));
        match turned {
            false => at(py, stride),
            true => at(px, 1),
        }
    });
    // Without a flip or a turn, the frame columns show page columns
    // from left to right, so the pieces of each row with nothing in them
    // (see Graphics::pieces) can be passed over.
    // Small sprites, the many, are not worth the setting up.
    let large = cols.len() * grid.ys.len() >= LARGE;
    let run = match (turned, source.white, source.page_no) {
        (false, false, Some(page)) if large => steady(&cols).map(|run| (run, page)),
        _ => None,
    };
    struct Rows<'a, R> {
        fb: &'a mut Framebuffer,
        grid: &'a compose::Grid,
        cols: &'a [usize],
        rows: R,
        source: &'a Source<'a>,
        run: Option<(Run, usize)>,
        grp: &'a Graphics,
        /// Neither tinted nor added: what is opaque is put as it is.
        plain: bool,
    }
    impl<R: Iterator<Item = usize>> TintJob for Rows<'_, R> {
        type Out = ();
        fn run<P: Paint>(self, paint: impl Fn(u32) -> P + Copy) {
            let width = self.fb.width() as usize;
            let pixels = self.fb.pixels_mut();
            let page = self.source.page;
            // GRP-1 is one color all over, as large boxes and fades use:
            // its blend is worked out once.
            if self.source.white {
                let white = paint(0xFFFF_FFFF);
                for (fy, row) in (self.grid.y0 as usize..).zip(self.rows) {
                    if row == NONE {
                        continue;
                    }
                    let at = fy * width + self.grid.x0 as usize;
                    let line = &mut pixels[at..at + self.cols.len()];
                    for (dst, &col) in line.iter_mut().zip(self.cols) {
                        if col != NONE {
                            *dst = white.put(*dst);
                        }
                    }
                }
                return;
            }
            if let Some((run, page_no)) = self.run {
                let stride = GRP_SIZE as usize;
                let shown = &self.cols[run.at..][..run.len];
                let (first, last) = (shown[0], shown[run.len - 1]);
                let (p0, p1) = (first / PIECE, last / PIECE);
                // Where among the shown columns each piece's start.
                let mut starts = [0; GRP_SIZE as usize / PIECE + 1];
                for (b, start) in starts.iter_mut().enumerate().take(p1 + 2).skip(p0) {
                    *start = shown.partition_point(|&c| c < b * PIECE);
                }
                // At a scale of 1, the usual way to put a large image up,
                // a row of the frame is a run of a row of the page.
                let side_by_side = last - first + 1 == run.len;
                // The page pixel `paint` blends, which a clear one never
                // is.
                let (mut held, mut held_paint) = (0, paint(0));
                for (fy, row) in (self.grid.y0 as usize..).zip(self.rows) {
                    if row == NONE {
                        continue;
                    }
                    let mut pieces = (self.grp).pieces(page_no, row / stride, p0, p1);
                    let at = fy * width + self.grid.x0 as usize + run.at;
                    let line = &mut pixels[at..at + run.len];
                    let put = |dst: &mut u32, p: u32| {
                        if p >> 24 != 0 {
                            *dst = paint(p).put(*dst);
                        }
                    };
                    while pieces != 0 {
                        let b = pieces.trailing_zeros() as usize;
                        pieces &= pieces - 1;
                        let (start, end) = (starts[b], starts[b + 1]);
                        let line = &mut line[start..end];
                        if side_by_side {
                            let from = &page[row + first + start..][..end - start];
                            // Eight at a time, so that clear stretches
                            // inside a piece cost one test.
                            for (dst, from) in line.chunks_mut(8).zip(from.chunks(8)) {
                                if from.iter().fold(0, |a, &p| a | p) >> 24 == 0 {
                                    continue;
                                }
                                // Opaque pixels as they are cover what
                                // is under them.
                                if self.plain && from.iter().fold(!0, |a, &p| a & p) >> 24 == 0xFF {
                                    dst.copy_from_slice(from);
                                    continue;
                                }
                                for (dst, &p) in dst.iter_mut().zip(from) {
                                    put(dst, p);
                                }
                            }
                        } else {
                            // Scaled up, neighbors show the same page
                            // pixel, whose blend is worked out once.
                            for (dst, &col) in line.iter_mut().zip(&shown[start..end]) {
                                let p = page[row + col];
                                match p >> 24 {
                                    0 => continue,
                                    0xFF if self.plain => {
                                        *dst = p;
                                        continue;
                                    }
                                    _ => {}
                                }
                                if p != held {
                                    held = p;
                                    held_paint = paint(p);
                                }
                                *dst = held_paint.put(*dst);
                            }
                        }
                    }
                }
                return;
            }
            for (fy, row) in (self.grid.y0 as usize..).zip(self.rows) {
                if row == NONE {
                    continue;
                }
                let at = fy * width + self.grid.x0 as usize;
                let line = &mut pixels[at..at + self.cols.len()];
                // Scaled up, neighbors show the same page pixel, whose
                // paint is worked out once.
                let (mut held, mut held_paint) = (0, paint(0));
                for (dst, &col) in line.iter_mut().zip(self.cols) {
                    if col == NONE {
                        continue;
                    }
                    let p = page[col + row];
                    if p >> 24 == 0 {
                        continue;
                    }
                    if p != held {
                        held = p;
                        held_paint = paint(p);
                    }
                    *dst = held_paint.put(*dst);
                }
            }
        }
    }
    tint.with(
        place.add,
        Rows {
            fb,
            grid: &grid,
            cols: &cols,
            rows,
            source,
            run,
            grp,
            plain: matches!(tint, Tint::White) && !place.add,
        },
    );
}

/// Frame columns of a sprite that show page columns from left to
/// right: from the `at`th of the sprite's frame columns on, `len` of
/// them, the rest showing none.
#[derive(Clone, Copy)]
struct Run {
    at: usize,
    len: usize,
}

/// The [`Run`] that `cols` (as [`draw_upright`] works them out) are, if
/// they are one.
fn steady(cols: &[usize]) -> Option<Run> {
    let at = cols.iter().position(|&c| c != NONE)?;
    let len = cols[at..].iter().take_while(|&&c| c != NONE).count();
    let rightwards = cols[at..at + len].windows(2).all(|w| w[0] <= w[1]);
    let rest_off = cols[at + len..].iter().all(|&c| c == NONE);
    (rightwards && rest_off).then_some(Run { at, len })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::Blend;

    const RED: u32 = 0xFFFF_0000;
    const BLUE: u32 = 0xFF00_00FF;
    /// What a new frame starts as.
    const BLACK: u32 = 0xFF00_0000;

    /// GRP4 with a 2x2 image at (0,0): red on the left column, blue on the
    /// right.
    fn page() -> Graphics {
        let mut g = Graphics::default();
        g.target = 4;
        g.fill(0, 0, 0, 1, RED, Blend::Normal);
        g.fill(1, 0, 1, 1, BLUE, Blend::Normal);
        g
    }

    fn sprite(attr: i32) -> Sprite {
        let mut s = Sprite::unused();
        (s.used, s.shown, s.w, s.h, s.attr) = (true, true, 2, 2, attr);
        s.pos = (4.0, 4.0);
        s
    }

    fn row(fb: &Framebuffer, y: u32, x: std::ops::Range<u32>) -> Vec<u32> {
        x.map(|x| fb.get(x, y).unwrap()).collect()
    }

    /// Drawing a large sprite piece by piece gives the same frame as
    /// drawing its image as small sprites, which go pixel by pixel.
    #[test]
    fn large_sprites_draw_as_small_ones() {
        let mut g = Graphics::default();
        g.target = 0;
        let mut seed = 0x1357_9BDF_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        // Mostly clear, with some opaque, some partly clear and some
        // clear but colored pixels, and an opaque block.
        for _ in 0..300 {
            let (x, y) = ((next() % 120) as i32, (next() % 80) as i32);
            let color = match next() % 3 {
                0 => next() | 0xFF00_0000,
                1 => next(),
                _ => next() & 0x00FF_FFFF,
            };
            g.pset(x, y, color, Blend::Normal);
        }
        g.fill(70, 20, 109, 39, 0xFF33_6699, Blend::Normal);
        for (scale, color, attr) in [
            (1.0, 0xFFFF_FFFF, 0),
            (1.0, 0x80FF_FFFF, attr::ADD),
            (1.0, 0xFF80_C040, 0),
            (2.0, 0xFFFF_FFFF, 0),
            (2.0, 0xC0FF_80FF, attr::ADD),
        ] {
            let at = |u: i32, v: i32, w: i32, h: i32| {
                let mut s = Sprite::unused();
                (s.used, s.shown, s.page, s.attr, s.color) = (true, true, 0, attr, color);
                (s.u, s.v, s.w, s.h) = (u, v, w, h);
                s.scale = (scale, scale);
                s.pos = (3.0 + f64::from(u) * scale, 5.0 + f64::from(v) * scale);
                s
            };
            let mut whole = Framebuffer::new(320, 200);
            draw(&at(0, 0, 120, 80), &g, &mut whole);
            let mut tiles = Framebuffer::new(320, 200);
            for v in (0..80).step_by(20) {
                for u in (0..120).step_by(20) {
                    draw(&at(u, v, 20, 20), &g, &mut tiles);
                }
            }
            assert!(
                whole.pixels() == tiles.pixels(),
                "scale {scale} color {color:08X}"
            );
        }
    }

    #[test]
    fn draws_at_its_position() {
        let mut fb = Framebuffer::new(16, 16);
        draw(&sprite(0), &page(), &mut fb);
        assert_eq!(row(&fb, 4, 3..7), [BLACK, RED, BLUE, BLACK]);
        assert_eq!(fb.get(4, 6), Some(BLACK), "two rows only");
    }

    #[test]
    fn flips_turns_and_scale() {
        let mut fb = Framebuffer::new(16, 16);
        draw(&sprite(attr::REVH), &page(), &mut fb);
        assert_eq!(row(&fb, 4, 4..6), [BLUE, RED]);
        // A quarter turn clockwise puts the left column on top.
        let mut fb = Framebuffer::new(16, 16);
        draw(&sprite(1), &page(), &mut fb);
        assert_eq!(row(&fb, 4, 4..6), [RED, RED]);
        assert_eq!(row(&fb, 5, 4..6), [BLUE, BLUE]);
        let mut s = sprite(0);
        s.scale = (2.0, 1.0);
        let mut fb = Framebuffer::new(16, 16);
        draw(&s, &page(), &mut fb);
        assert_eq!(row(&fb, 4, 4..8), [RED, RED, BLUE, BLUE]);
    }

    #[test]
    fn rotation_turns_around_home() {
        let mut s = sprite(0);
        s.home = (1.0, 1.0);
        s.rot = 180.0;
        let mut fb = Framebuffer::new(16, 16);
        draw(&s, &page(), &mut fb);
        // Turned half around the point (4,4): blue now left of red.
        assert_eq!(row(&fb, 3, 3..5), [BLUE, RED]);
    }

    #[test]
    fn every_way_of_drawing_paints_each_pixel_once() {
        use crate::compose::{Lit, Over3};
        let mut g = Graphics::default();
        g.target = 0;
        let mut next = {
            let mut seed = 0x0BAD_F00D_u32;
            move || {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed
            }
        };
        for y in 0..80 {
            for x in 0..120 {
                let c = match next() % 4 {
                    0 => 0,
                    1 => next() | 0xFF00_0000,
                    _ => next(),
                };
                g.pset(x, y, c, Blend::Normal);
            }
        }
        // Repeats, so that scaled up neighbors show the same pixel.
        g.fill(10, 10, 40, 30, 0x80C0_4020, Blend::Normal);
        let mut under = Framebuffer::new(320, 200);
        for p in under.pixels_mut() {
            *p = next() | 0xFF00_0000;
        }
        for (w, h) in [(120, 80), (20, 20)] {
            for scale in [1, 2] {
                for color in [0xFFFF_FFFF, 0x80FF_FFFF, 0xFF80_C040, 0x7F12_C0FE] {
                    for add in [0, attr::ADD] {
                        let mut s = Sprite::unused();
                        (s.used, s.shown, s.page, s.attr, s.color) = (true, true, 0, add, color);
                        (s.w, s.h, s.pos) = (w, h, (3.0, 5.0));
                        s.scale = (f64::from(scale), f64::from(scale));
                        let mut fb = under.clone();
                        draw(&s, &g, &mut fb);
                        for (i, (&got, &d)) in fb.pixels().iter().zip(under.pixels()).enumerate() {
                            let (x, y) = ((i % 320) as i32 - 3, (i / 320) as i32 - 5);
                            let inside = (0..w * scale).contains(&x) && (0..h * scale).contains(&y);
                            let p = match inside {
                                true => g.pixel(0, x / scale, y / scale),
                                false => 0,
                            };
                            let expected = match (p >> 24, add) {
                                (0, _) => d,
                                (_, 0) => Over3::new(p, color).put(d),
                                _ => Lit::tinted(p, color).put(d),
                            };
                            assert_eq!(
                                got, expected,
                                "{w}x{h} x{scale} {color:08X} {add} at {x},{y}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn color_and_additive_blending() {
        let mut s = sprite(attr::ADD);
        s.color = 0xFF00_FFFF;
        let mut fb = Framebuffer::new(16, 16);
        fb.clear(0xFF10_1010);
        draw(&s, &page(), &mut fb);
        // Red times cyan is black, which adds nothing.
        assert_eq!(fb.get(4, 4), Some(0xFF10_1010));
        assert_eq!(fb.get(5, 4), Some(0xFF10_10FF));
    }

    #[test]
    fn default_table_is_checked() {
        let table: Vec<u8> = (0..DEFS as u16)
            .flat_map(|n| [n % 128 * 16, n / 128 * 16, 16, 16, 0, 0, 0])
            .flat_map(|v| v.to_le_bytes())
            .collect();
        assert_eq!(defs_from_bytes(&table), Some(grid_defs()));
        let mut bad = table.clone();
        bad[2] = 0xFF; // U far off the page
        bad[3] = 0xFF;
        assert_eq!(defs_from_bytes(&bad), None);
    }

    #[test]
    fn spclr_keeps_the_graphic_sprite() {
        let mut s = Sprites::default();
        s.create(3, true);
        s.clear_all();
        assert!(!s.list[3].used);
        assert!(s.list[GSPRITE].used);
        assert_eq!(s.draw_order()[0].0, GSPRITE);
    }

    fn colliding(s: &mut Sprites, id: usize) {
        s.create(id, true);
        s.set_collision(
            id,
            Collision {
                x: 0,
                y: 0,
                w: 16,
                h: 16,
                scale: false,
                mask: u32::MAX,
            },
        );
    }

    fn hits(s: &mut Sprites, first: usize, last: usize, skip: Option<usize>) -> Vec<usize> {
        let source = Body::rect(0.0, 0.0, 16.0, 16.0, (0.0, 0.0), u32::MAX);
        let mut search = Search::new(source, first, last, skip);
        std::iter::from_fn(|| s.next_hit(&mut search)).collect()
    }

    #[test]
    fn search_finds_hits_upwards_across_the_bit_words() {
        let mut s = Sprites::default();
        for id in [0, 1, 63, 64, 65, 127, 128, 1000, 4094, GSPRITE] {
            colliding(&mut s, id);
        }
        let all = vec![0, 1, 63, 64, 65, 127, 128, 1000, 4094, GSPRITE];
        assert_eq!(hits(&mut s, 0, GSPRITE, None), all);
        assert_eq!(hits(&mut s, GSPRITE, 0, None), all);
        assert_eq!(hits(&mut s, 63, 64, None), [63, 64]);
        assert_eq!(hits(&mut s, 2, 62, None), [0usize; 0]);
        assert_eq!(hits(&mut s, 64, 64, None), [64]);
        assert_eq!(hits(&mut s, 64, 127, Some(65)), [64, 127]);
        // The range ends inside a word with more hits after it.
        assert_eq!(hits(&mut s, 0, 1, None), [0, 1]);
        assert_eq!(hits(&mut s, 1000, 4094, None), [1000, 4094]);
    }

    #[test]
    fn search_sees_what_changed_while_it_went_on() {
        let mut s = Sprites::default();
        for id in [1, 2, 3] {
            colliding(&mut s, id);
        }
        let source = Body::rect(0.0, 0.0, 16.0, 16.0, (0.0, 0.0), u32::MAX);
        let mut search = Search::new(source, 0, 100, None);
        assert_eq!(s.next_hit(&mut search), Some(1));
        s.clear(2);
        colliding(&mut s, 70);
        assert_eq!(s.next_hit(&mut search), Some(3));
        assert_eq!(s.next_hit(&mut search), Some(70));
        assert_eq!(s.next_hit(&mut search), None);
    }

    #[test]
    fn sprites_stop_colliding_when_cleared_or_set_again() {
        let mut s = Sprites::default();
        for id in [1, 2, 3, GSPRITE] {
            colliding(&mut s, id);
        }
        s.clear(1);
        s.create(2, true);
        assert_eq!(hits(&mut s, 0, GSPRITE, None), [3, GSPRITE]);
        s.clear_all();
        assert_eq!(hits(&mut s, 0, GSPRITE, None), [GSPRITE]);
        s.reset_gsprite((16, 16));
        assert_eq!(hits(&mut s, 0, GSPRITE, None), [0usize; 0]);
    }

    /// A search skips making a pose where there is no parent to follow.
    #[test]
    fn search_gives_the_same_body_as_a_sprites_pose() {
        let mut s = Sprites::default();
        colliding(&mut s, 5);
        s.list[5].pos = (2.5, 3.0);
        s.list[5].scale = (2.0, 2.0);
        s.list[5].col_vel = Some((1.0, 0.0));
        let source = Body::rect(0.0, 0.0, 4.0, 4.0, (-1.0, 0.0), u32::MAX);
        let mut search = Search::new(source, 0, 10, None);
        assert_eq!(s.next_hit(&mut search), Some(5));
        let body = s.body(5).unwrap();
        assert_eq!(s.hit_info, HitInfo::new(0.0, &source, &body));

        s.list[5].shown = false;
        assert_eq!(hits(&mut s, 0, 10, None), [0usize; 0]);
        s.list[5].shown = true;
        let col = s.list[5].col.unwrap();
        s.list[5].col = Some(Collision { mask: 0, ..col });
        assert_eq!(hits(&mut s, 0, 10, None), [0usize; 0]);
    }
}

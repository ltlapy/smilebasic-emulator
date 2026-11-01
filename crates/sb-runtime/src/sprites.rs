//! Sprites 0-4095 and their definition templates (SPDEF). Sprite
//! `#GSPRITE` (4095) shows the graphic page GRP0 from start-up.

use std::collections::HashMap;

use sb_core::value::Value;

use crate::anim::{self, ANIM_VAR, Anim, Animated, Target, Track};
use crate::collision::{Body, Collision, HitInfo, Search};
use crate::compose::{self, Affine, Placement, draw_image};
use crate::framebuffer::Framebuffer;
use crate::graphics::{Clip, GRP_SIZE, Graphics, PAGES, Tint, TintJob, multiply};

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

    // UNVERIFIED: integer properties take the value rounded towards zero.
    fn set_value(&mut self, target: Target, v: [f64; 2]) {
        let s = &mut self.sprite;
        match target {
            Target::Xy => s.pos = (v[0], v[1]),
            Target::Z => s.z = (v[0] as i32).clamp(-4095, 4095),
            Target::R => s.rot = v[0],
            Target::S => s.scale = (v[0], v[1]),
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
    /// Degrees, clockwise.
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

impl Sprite {
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
    pub defs: Vec<SpriteDef>,
    /// What SPDEF without arguments restores.
    default_defs: Vec<SpriteDef>,
    /// SPPAGE and SPLAYER for sprites created from now on.
    pub new_page: i32,
    pub new_layer: i32,
    /// ANIMDEF definitions.
    pub anim_defs: Vec<Option<Anim>>,
    /// Where SPHITSP() and SPHITRC() without arguments go on searching.
    pub hit_sp: Option<Search>,
    pub hit_rc: Option<Search>,
    /// What SPHITINFO reports.
    pub hit_info: HitInfo,
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
            defs: defs.clone(),
            default_defs: defs,
            new_page: DEFAULT_PAGE,
            new_layer: 0,
            anim_defs: vec![None; anim::DEFINITIONS],
            hit_sp: None,
            hit_rc: None,
            hit_info: HitInfo::default(),
        };
        s.reset_gsprite((400, 240));
        s
    }

    /// Sprite 4095 shows the top-left `w` x `h` of GRP0 behind everything
    /// else, on the layer `SPLAYER` gives new sprites.
    pub fn reset_gsprite(&mut self, (w, h): (u32, u32)) {
        let layer = self.new_layer;
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
        for child in self.children(id).collect::<Vec<_>>() {
            self.unlink(child);
        }
        self.list[id] = Sprite::unused();
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

    /// Plays one frame of every sprite's animations. Sprites whose
    /// animation asks for it are cleared when it ends.
    pub fn animate(&mut self) {
        for id in 0..SPRITES {
            if self.list[id].clear_pending {
                self.clear(id);
            }
        }
        for id in 0..SPRITES {
            let s = &mut self.list[id];
            if !s.used || s.anim_stopped {
                s.anim_move = (0.0, 0.0);
                continue;
            }
            let mut tracks = std::mem::take(&mut s.anims);
            let mut delete = false;
            let before = s.pos;
            s.anim_move = (0.0, 0.0);
            for (i, slot) in tracks.iter_mut().enumerate() {
                let Some(track) = slot else { continue };
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
            if delete {
                // UNVERIFIED: #GSPRITE is cleared like any other.
                self.clear(id);
            }
        }
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
        while let Some(id) = search.rest.pop() {
            if Some(id) == search.skip {
                continue;
            }
            if let Some(body) = self.body(id)
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

/// How far left of a column boundary a sprite's sample point is taken.
const TIE: f64 = 1e-9;

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
            fn run(self, tint: impl Fn(u32) -> u32 + Copy) {
                let (bw, bh) = self.size;
                let source = self.source;
                compose::project_image(self.fb, self.m, self.sample, self.place, bw, bh, |x, y| {
                    let (x, y) = source.page_xy(x, y);
                    tint(source.pixel(x, y))
                });
            }
        }
        return tint.with(Projected {
            fb,
            place: &place,
            m,
            sample,
            size: (bw, bh),
            source: &source,
        });
    }
    if place.is_upright() {
        return draw_upright(&source, &tint, fb, &place, (bw, bh));
    }
    struct Turned<'a, 'b> {
        fb: &'a mut Framebuffer,
        place: &'a Placement<'b>,
        size: (i32, i32),
        source: &'a Source<'a>,
    }
    impl TintJob for Turned<'_, '_> {
        type Out = ();
        fn run(self, tint: impl Fn(u32) -> u32 + Copy) {
            let (bw, bh) = self.size;
            let source = self.source;
            draw_image(self.fb, self.place, bw, bh, |x, y| {
                // A point exactly on a column boundary takes the column to
                // its left, while rows take the one below as usual: at a
                // scale of 1.5, the second pixel across shows the first
                // column (checked on hardware with lossless screenshots at
                // 1.5 and 2.5).
                let (lx, ly) = ((x - TIE).max(0.0) as i32, y as i32);
                let (x, y) = source.page_xy(lx, ly);
                tint(source.pixel(x, y))
            });
        }
    }
    tint.with(Turned {
        fb,
        place: &place,
        size: (bw, bh),
        source: &source,
    });
}

/// Where a sprite's pixels come from.
struct Source<'a> {
    page: &'a [u32],
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
            repeat,
            white,
            u: s.u,
            v: s.v,
            w: s.w,
            h: s.h,
            attr: s.attr,
        }
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

/// [`draw_posed`] for a sprite neither turned nor sheared on the frame,
/// the usual case: which page column and row each frame column and row
/// shows is worked out once, so each pixel is a lookup and a blend.
fn draw_upright(
    source: &Source,
    tint: &Tint,
    fb: &mut Framebuffer,
    place: &Placement,
    (bw, bh): (i32, i32),
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
            let (px, py) = source.page_xy((x - TIE).max(0.0) as i32, 0);
            match turned {
                false => at(px, 1),
                true => at(py, stride),
            }
        })
        .collect();
    let rows = grid.ys.iter().map(|&y| {
        let (px, py) = source.page_xy(0, y as i32);
        match turned {
            false => at(py, stride),
            true => at(px, 1),
        }
    });
    struct Rows<'a, R> {
        fb: &'a mut Framebuffer,
        grid: &'a compose::Grid,
        cols: &'a [usize],
        rows: R,
        source: &'a Source<'a>,
        add: bool,
    }
    impl<R: Iterator<Item = usize>> TintJob for Rows<'_, R> {
        type Out = ();
        fn run(self, tint: impl Fn(u32) -> u32 + Copy) {
            let width = self.fb.width() as usize;
            let pixels = self.fb.pixels_mut();
            let (page, white) = (self.source.page, tint(0xFFFF_FFFF));
            for (fy, row) in (self.grid.y0 as usize..).zip(self.rows) {
                if row == NONE {
                    continue;
                }
                let at = fy * width + self.grid.x0 as usize;
                let line = &mut pixels[at..at + self.cols.len()];
                for (dst, &col) in line.iter_mut().zip(self.cols) {
                    if col == NONE {
                        continue;
                    }
                    let src = match self.source.white {
                        true => white,
                        false => tint(page[col + row]),
                    };
                    if src >> 24 == 0 {
                        continue;
                    }
                    *dst = compose::blend(*dst, src, self.add);
                }
            }
        }
    }
    tint.with(Rows {
        fb,
        grid: &grid,
        cols: &cols,
        rows,
        source,
        add: place.add,
    });
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
}

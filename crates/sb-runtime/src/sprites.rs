//! Sprites 0-4095 and their definition templates (SPDEF). Sprite
//! `#GSPRITE` (4095) shows the graphic page GRP0 from start-up.

use std::collections::HashMap;

use sb_core::value::Value;

use crate::anim::{self, Anim, Target, Track};
use crate::collision::{Body, Collision, HitInfo, Search};
use crate::framebuffer::Framebuffer;
use crate::graphics::{GRP_SIZE, Graphics, PAGES, multiply};

pub const SPRITES: usize = 4096;
pub const DEFS: usize = 8192;
/// Templates in the first release's table.
const FIRST_RELEASE_DEFS: usize = 4096;
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

/// Finds SmileBASIC 4's built-in template table in its executable's
/// read-only data: it starts with 16x16 cells at (0,0) and (16,0). Returns
/// the table's bytes. The first release has only the first
/// [`FIRST_RELEASE_DEFS`] templates; the rest are left empty, as unused
/// templates are in later releases.
pub fn find_default_defs(ro: &[u8]) -> Option<Vec<u8>> {
    let start: Vec<u8> = [0u16, 0, 16, 16, 0, 0, 0, 16, 0, 16, 16, 0, 0, 0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let starts: Vec<usize> = (0..ro.len())
        .filter(|&at| ro[at..].starts_with(&start))
        .collect();
    for count in [DEFS, FIRST_RELEASE_DEFS] {
        for &at in &starts {
            let Some(table) = ro.get(at..at + count * DEF_BYTES) else {
                continue;
            };
            let mut table = table.to_vec();
            table.resize(DEFS * DEF_BYTES, 0);
            if defs_from_bytes(&table).is_some() {
                return Some(table);
            }
        }
    }
    None
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
        s.reset_gsprite();
        s
    }

    /// Sprite 4095 shows the top-left of GRP0 behind everything else.
    // UNVERIFIED: its Z and size at start-up.
    fn reset_gsprite(&mut self) {
        let g = &mut self.list[GSPRITE];
        *g = Sprite::unused();
        (g.used, g.shown, g.page) = (true, true, 0);
        (g.w, g.h) = (400, 240);
        g.z = 4095;
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

    /// SPCLR of one sprite.
    pub fn clear(&mut self, id: usize) {
        self.list[id] = Sprite::unused();
    }

    /// SPCLR without arguments: every sprite but `#GSPRITE`.
    pub fn clear_all(&mut self) {
        for s in &mut self.list[..GSPRITE] {
            *s = Sprite::unused();
        }
    }

    /// Plays one frame of every sprite's animations. Sprites whose
    /// animation asks for it are cleared when it ends.
    pub fn animate(&mut self) {
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
                let going = track.step(s, &self.defs);
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
    // UNVERIFIED: hidden sprites collide too.
    pub fn body(&self, id: usize) -> Option<Body> {
        let s = &self.list[id];
        let c = s.col.as_ref().filter(|_| s.used)?;
        Some(Body::sprite(
            c,
            s.pos,
            s.scale,
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

    /// Ids of shown sprites, back to front.
    // UNVERIFIED: among equal Z, lower ids are drawn in front.
    pub fn draw_order(&self) -> Vec<usize> {
        let mut ids: Vec<usize> = (0..SPRITES)
            .filter(|&i| self.list[i].used && self.list[i].shown)
            .collect();
        ids.sort_by_key(|&i| (std::cmp::Reverse(self.list[i].z), std::cmp::Reverse(i)));
        ids
    }
}

/// The page pixel a sprite shows at `(x, y)` of its own rectangle (before
/// rotation and flipping), or 0 off the page.
fn sample(grp: &Graphics, page: i32, x: i32, y: i32) -> u32 {
    let Ok(p) = usize::try_from(page) else {
        // UNVERIFIED: what page -1 shows.
        return 0;
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

/// Draws one sprite over the frame.
pub fn draw(s: &Sprite, grp: &Graphics, fb: &mut Framebuffer) {
    if s.w <= 0 || s.h <= 0 || s.scale.0 == 0.0 || s.scale.1 == 0.0 {
        return;
    }
    // The shown rectangle swaps sides for quarter turns.
    let quarter = s.attr & attr::ROTATION;
    let (bw, bh) = if quarter % 2 == 1 {
        (s.h, s.w)
    } else {
        (s.w, s.h)
    };
    let (sin, cos) = s.rot.to_radians().sin_cos();
    let (sx, sy) = s.scale;
    // Screen position of a point of the shown rectangle.
    let to_screen = |lx: f64, ly: f64| {
        let (dx, dy) = ((lx - s.home.0) * sx, (ly - s.home.1) * sy);
        (s.pos.0 + dx * cos - dy * sin, s.pos.1 + dx * sin + dy * cos)
    };
    let corners = [
        to_screen(0.0, 0.0),
        to_screen(f64::from(bw), 0.0),
        to_screen(0.0, f64::from(bh)),
        to_screen(f64::from(bw), f64::from(bh)),
    ];
    let fw = f64::from(fb.width());
    let fh = f64::from(fb.height());
    let x0 = corners
        .iter()
        .map(|c| c.0)
        .fold(f64::MAX, f64::min)
        .max(0.0)
        .floor();
    let x1 = corners
        .iter()
        .map(|c| c.0)
        .fold(f64::MIN, f64::max)
        .min(fw)
        .ceil();
    let y0 = corners
        .iter()
        .map(|c| c.1)
        .fold(f64::MAX, f64::min)
        .max(0.0)
        .floor();
    let y1 = corners
        .iter()
        .map(|c| c.1)
        .fold(f64::MIN, f64::max)
        .min(fh)
        .ceil();
    let add = s.attr & attr::ADD != 0;
    for py in y0 as u32..y1 as u32 {
        for px in x0 as u32..x1 as u32 {
            // Back from the pixel's center to the shown rectangle.
            let (dx, dy) = (f64::from(px) + 0.5 - s.pos.0, f64::from(py) + 0.5 - s.pos.1);
            let lx = (dx * cos + dy * sin) / sx + s.home.0;
            let ly = (-dx * sin + dy * cos) / sy + s.home.1;
            if lx < 0.0 || ly < 0.0 || lx >= f64::from(bw) || ly >= f64::from(bh) {
                continue;
            }
            let (mut lx, mut ly) = (lx as i32, ly as i32);
            // UNVERIFIED: flips apply to the shown rectangle, then the
            // quarter turn picks the image pixel.
            if s.attr & attr::REVH != 0 {
                lx = bw - 1 - lx;
            }
            if s.attr & attr::REVV != 0 {
                ly = bh - 1 - ly;
            }
            let (ix, iy) = match quarter {
                0 => (lx, ly),
                1 => (ly, s.h - 1 - lx),
                2 => (s.w - 1 - lx, s.h - 1 - ly),
                _ => (s.w - 1 - ly, lx),
            };
            let src = multiply(sample(grp, s.page, s.u + ix, s.v + iy), s.color);
            let a = src >> 24;
            if a == 0 {
                continue;
            }
            let Some(dst) = fb.get(px, py) else { continue };
            let [_, sr, sg, sb] = src.to_be_bytes().map(u32::from);
            let [da, dr, dg, db] = dst.to_be_bytes().map(u32::from);
            let mix = |s: u32, d: u32| {
                if add {
                    (d + s * a / 255).min(255)
                } else {
                    (s * a + d * (255 - a)) / 255
                }
            };
            fb.set(
                px,
                py,
                u32::from_be_bytes([
                    da as u8,
                    mix(sr, dr) as u8,
                    mix(sg, dg) as u8,
                    mix(sb, db) as u8,
                ]),
            );
        }
    }
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
    fn default_table_is_found_and_checked() {
        let mut bytes = vec![0xAB; 10];
        let table: Vec<u8> = (0..DEFS as u16)
            .flat_map(|n| [n % 128 * 16, n / 128 * 16, 16, 16, 0, 0, 0])
            .flat_map(|v| v.to_le_bytes())
            .collect();
        bytes.extend_from_slice(&table);
        let found = find_default_defs(&bytes).expect("table");
        assert_eq!(defs_from_bytes(&found), Some(grid_defs()));
        // A first-release table, followed by other data.
        let mut first = bytes[..10 + FIRST_RELEASE_DEFS * DEF_BYTES].to_vec();
        first.extend_from_slice(&[0xFF; 1000]);
        let defs = defs_from_bytes(&find_default_defs(&first).expect("table")).unwrap();
        assert_eq!(
            defs[..FIRST_RELEASE_DEFS],
            grid_defs()[..FIRST_RELEASE_DEFS]
        );
        assert_eq!(defs[FIRST_RELEASE_DEFS], SpriteDef::default());
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
        assert_eq!(s.draw_order(), [GSPRITE]);
    }
}

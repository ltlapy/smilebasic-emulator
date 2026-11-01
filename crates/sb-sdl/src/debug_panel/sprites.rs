//! The debugger panel's Sprites tabs: the sprites SPSET has set on the
//! main interpreter's screen, or on a subprogram's in a tab of their
//! own, gathered only while the tab shows, and the outline of the one
//! chosen on its screen. SmileBASIC 4 has no such list; this is the
//! emulator's own.

use sb_core::value::{Value, format_number};
use sb_runtime::Runtime;
use sb_runtime::collision::Collision;
use sb_runtime::framebuffer::Framebuffer;
use sb_runtime::graphics::PAGES;
use sb_runtime::sprites::{self, Sprite, VarKey, attr, link};

use crate::ui::Rect;

use super::value_text;

/// Pixels across and down a row's thumbnail of the sprite's image.
pub const THUMB: usize = 16;
/// SPANIM's targets in SPCHK's bit order.
const TARGETS: [&str; 8] = ["XY", "Z", "R", "S", "C", "V", "UV", "I"];
/// The outline of the sprite chosen, and its dark edge; dashed while it
/// does not show.
const OUTLINE: u32 = 0xFFFF_DC00;
const OUTLINE_EDGE: u32 = 0xFF00_0000;

/// One sprite in use, as the list shows it.
#[derive(Clone, Debug)]
pub struct Row {
    pub id: usize,
    pub def: i32,
    pub page: i32,
    /// SPCHR's image rectangle: U, V, W and H.
    pub image: [i32; 4],
    pub attr: i32,
    pub home: (f64, f64),
    /// Its own SPOFS, relative to its parent when it has one.
    pub pos: (f64, f64),
    pub z: i32,
    pub rot: f64,
    pub scale: (f64, f64),
    pub color: u32,
    pub layer: i32,
    /// SPSHOW or SPHIDE, and whether it shows once its parents' are
    /// followed.
    pub shown: bool,
    pub shows: bool,
    pub parent: Option<usize>,
    pub link_flags: i32,
    /// The targets SPANIM is playing, by [`TARGETS`], and whether SPSTOP
    /// holds them.
    pub anims: u8,
    pub anim_stopped: bool,
    pub col: Option<Collision>,
    /// SPVAR's variables that are not 0 or empty: numbers first.
    pub vars: Vec<(String, Value)>,
    /// The image as SPCHR cuts it from the page, fitted into
    /// [`THUMB`] x [`THUMB`]; `None` for a blank page or no image.
    pub thumb: Option<Vec<u32>>,
}

/// Where the chosen sprite shows on the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outline {
    pub corners: [(f64, f64); 4],
    pub shows: bool,
}

/// The subprogram's screen, for the outline of one of its sprites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubScreen {
    /// Where XSUBSCREEN puts it on the 1280x720 display: x, y, width
    /// and height, while it shows.
    pub place: Option<[i32; 4]>,
    /// The size of its frame, which the outline's corners are on.
    pub frame: (u32, u32),
}

/// The sprites in use, of which only the rows shown are gathered.
#[derive(Clone, Debug, Default)]
pub struct List {
    pub total: usize,
    /// From the `first` in use on.
    pub first: usize,
    pub rows: Vec<Row>,
    pub outline: Option<Outline>,
    /// For a subprogram's sprites, its screen; `None` for the main
    /// interpreter's, outlined on the main screens.
    pub sub: Option<SubScreen>,
}

/// The sprites in use on `rt`'s screen, with rows for `count` of them
/// from the `first`, and the outline of `chosen`.
pub fn list(rt: &Runtime, first: usize, count: usize, chosen: Option<usize>) -> List {
    let all = &rt.sprites.list;
    let used = (0..all.len()).filter(|&i| all[i].used);
    let mut list = List {
        first,
        ..List::default()
    };
    for (n, id) in used.enumerate() {
        list.total += 1;
        if n >= first && n - first < count {
            list.rows.push(row(rt, id));
        }
    }
    list.outline = chosen.and_then(|id| {
        let corners = rt.sprite_corners(id)?;
        let shows = rt.sprites.pose(id).shown;
        Some(Outline { corners, shows })
    });
    list
}

fn row(rt: &Runtime, id: usize) -> Row {
    let s: &Sprite = &rt.sprites.list[id];
    let mut vars: Vec<(String, Value)> = (s.vars.iter())
        .filter(|(_, v)| !is_empty(v))
        .map(|(k, v)| {
            let key = match k {
                VarKey::Int(n) => n.to_string(),
                VarKey::Str(s) => format!("\"{}\"", String::from_utf16_lossy(s)),
            };
            (key, v.clone())
        })
        .collect();
    // Numbers in order, then names.
    vars.sort_by_key(|(k, _)| (k.starts_with('"'), k.parse::<i64>().ok(), k.clone()));
    let anims = (s.anims.iter().enumerate())
        .filter(|(_, t)| t.is_some())
        .fold(0u8, |bits, (i, _)| bits | 1 << i);
    Row {
        id,
        def: s.def,
        page: s.page,
        image: [s.u, s.v, s.w, s.h],
        attr: s.attr,
        home: s.home,
        pos: s.pos,
        z: s.z,
        rot: s.rot,
        scale: s.scale,
        color: s.color,
        layer: s.layer,
        shown: s.shown,
        shows: rt.sprites.pose(id).shown,
        parent: s.parent,
        link_flags: s.link_flags,
        anims,
        anim_stopped: s.anim_stopped,
        col: s.col,
        vars,
        thumb: thumb(rt, s),
    }
}

fn is_empty(v: &Value) -> bool {
    match v {
        Value::Default => true,
        Value::Int(n) => *n == 0,
        Value::Real(x) => *x == 0.0,
        Value::Str(s) => s.borrow().is_empty(),
        Value::Array(_) => false,
    }
}

/// The sprite's image fitted into the thumbnail at its shape, its top
/// left at the thumbnail's, nearest pixels.
fn thumb(rt: &Runtime, s: &Sprite) -> Option<Vec<u32>> {
    let [u, v, w, h] = [s.u, s.v, s.w, s.h];
    let blank = usize::try_from(s.page).is_ok_and(|p| p >= PAGES || rt.grp.is_blank(p));
    if blank || w <= 0 || h <= 0 {
        return None;
    }
    let side = w.max(h) as i64;
    let mut pixels = vec![0; THUMB * THUMB];
    for ty in 0..THUMB {
        let y = (ty as i64 * side / THUMB as i64) as i32;
        if y >= h {
            break;
        }
        for tx in 0..THUMB {
            let x = (tx as i64 * side / THUMB as i64) as i32;
            if x >= w {
                break;
            }
            pixels[ty * THUMB + tx] = sprites::sample(&rt.grp, s.page, u + x, v + y);
        }
    }
    Some(pixels)
}

/// A number as PRINT shows it.
fn num(x: f64) -> String {
    String::from_utf16_lossy(&format_number(&Value::Real(x)))
}

fn pair((x, y): (f64, f64)) -> String {
    format!("{},{}", num(x), num(y))
}

/// The attributes in short: S for shown or `-`, the quarter turn, H and V
/// for flips and `+` for additive blending.
pub fn attr_text(row: &Row) -> String {
    let mut text = String::from(if row.shown { "S" } else { "-" });
    let quarter = row.attr & attr::ROTATION;
    if quarter != 0 {
        text.push_str(&format!(" {}", quarter * 90));
    }
    let flips: String = [(attr::REVH, 'H'), (attr::REVV, 'V'), (attr::ADD, '+')]
        .iter()
        .filter(|(bit, _)| row.attr & bit != 0)
        .map(|&(_, c)| c)
        .collect();
    if !flips.is_empty() {
        text.push(' ');
        text.push_str(&flips);
    }
    text
}

/// The SPANIM targets playing, or `-`.
pub fn anim_text(row: &Row) -> String {
    let names: Vec<&str> = (TARGETS.iter().enumerate())
        .filter(|(i, _)| row.anims & 1 << i != 0)
        .map(|(_, &n)| n)
        .collect();
    match names.is_empty() {
        true => "-".to_string(),
        false => names.join(","),
    }
}

/// The list's columns: heading, characters taken, and whether it can be
/// left out for want of room. The colour is a swatch.
pub const COLUMNS: [(&str, usize, bool); 11] = [
    ("#", 5, false),
    ("DEF", 5, false),
    ("X", 8, false),
    ("Y", 8, false),
    ("Z", 6, false),
    ("ROT", 7, true),
    ("SCALE", 10, true),
    ("C", 2, true),
    ("ATTR", 9, true),
    ("LINK", 5, true),
    ("ANIM", 12, true),
];

/// The columns that fit in `room` characters, after `taken` for the
/// thumbnail: those that cannot be left out, and then the others in turn
/// as long as there is room.
pub fn fitting(room: usize, taken: usize) -> usize {
    let mut used = taken;
    let mut n = 0;
    for (_, chars, optional) in COLUMNS {
        if optional && used + chars > room {
            break;
        }
        used += chars;
        n += 1;
    }
    n
}

/// A row's text in each column; the colour's is empty.
pub fn cells(row: &Row) -> [String; 11] {
    let link = row.parent.map_or("-".to_string(), |p| p.to_string());
    [
        row.id.to_string(),
        row.def.to_string(),
        num(row.pos.0),
        num(row.pos.1),
        row.z.to_string(),
        num(row.rot),
        pair(row.scale),
        String::new(),
        attr_text(row),
        link,
        anim_text(row),
    ]
}

/// Everything about a sprite, for its tip and for copying.
pub fn details(row: &Row, hex: bool) -> Vec<String> {
    let mut lines = Vec::new();
    let state = match (row.shown, row.shows) {
        (true, true) => "",
        (true, false) => " (hidden by its parent)",
        (false, _) => " (hidden)",
    };
    lines.push(format!("SPRITE {}{state}", row.id));
    let [u, v, w, h] = row.image;
    lines.push(format!(
        "DEF {}  U,V {u},{v}  W,H {w},{h}  PAGE {}",
        row.def, row.page
    ));
    let own = if row.parent.is_some() {
        " (on parent)"
    } else {
        ""
    };
    lines.push(format!(
        "X,Y {}  Z {}{own}  HOME {}",
        pair(row.pos),
        row.z,
        pair(row.home)
    ));
    lines.push(format!(
        "ROT {}  SCALE {}  LAYER {}",
        num(row.rot),
        pair(row.scale),
        row.layer
    ));
    lines.push(format!("COLOR &H{:08X}", row.color));
    let mut a = vec![if row.shown { "shown" } else { "hidden" }.to_string()];
    let quarter = row.attr & attr::ROTATION;
    if quarter != 0 {
        a.push(format!("turned {}", quarter * 90));
    }
    for (bit, name) in [
        (attr::REVH, "flipped H"),
        (attr::REVV, "flipped V"),
        (attr::ADD, "additive"),
    ] {
        if row.attr & bit != 0 {
            a.push(name.to_string());
        }
    }
    lines.push(format!("ATTR &H{:02X}: {}", row.attr, a.join(", ")));
    if let Some(parent) = row.parent {
        let follows: Vec<&str> = [
            (link::SHOWN, "shown"),
            (link::ROTATION, "rotation"),
            (link::SCALE, "scale"),
            (link::COLOR, "color"),
            (link::LAYER, "layer"),
        ]
        .iter()
        .filter(|(bit, _)| row.link_flags & bit != 0)
        .map(|&(_, n)| n)
        .collect();
        let follows = match follows.is_empty() {
            true => String::new(),
            false => format!(", follows its {}", follows.join(", ")),
        };
        lines.push(format!("LINK to {parent}{follows}"));
    }
    if row.anims != 0 {
        let stopped = if row.anim_stopped { " (SPSTOP)" } else { "" };
        lines.push(format!("ANIM {}{stopped}", anim_text(row)));
    }
    if let Some(c) = row.col {
        let scale = if c.scale { " scaled" } else { "" };
        lines.push(format!(
            "COL {},{} {}x{}{scale}  MASK &H{:X}",
            c.x, c.y, c.w, c.h, c.mask
        ));
    }
    let vars: Vec<String> = (row.vars.iter())
        .map(|(k, v)| format!("{k}={}", value_text(v, 20, hex).0))
        .collect();
    // A few to a line.
    for chunk in vars.chunks(4) {
        lines.push(format!("VAR {}", chunk.join("  ")));
    }
    lines
}

/// Draws the outline of a sprite at `corners` of a `frame` shown at
/// `place` (x, y, width and height, in `fb`'s pixels), lines `width`
/// pixels thick, kept within `clip`.
pub fn draw_outline(
    fb: &mut Framebuffer,
    outline: &Outline,
    frame: (u32, u32),
    place: (f64, f64, f64, f64),
    clip: Rect,
    width: u32,
) {
    let (sx, sy) = (
        place.2 / f64::from(frame.0.max(1)),
        place.3 / f64::from(frame.1.max(1)),
    );
    let at = outline
        .corners
        .map(|(x, y)| (place.0 + x * sx, place.1 + y * sy));
    let width = width.max(1);
    for (thick, color) in [(width + 2, OUTLINE_EDGE), (width, OUTLINE)] {
        for i in 0..4 {
            let dashed = !outline.shows;
            line(fb, at[i], at[(i + 1) % 4], clip, thick, color, dashed);
        }
    }
}

/// Where the outline of a subprogram's sprite goes on the display, whose
/// pixels are those of the 1280x720 display: the place of its screen,
/// and that within the `display`'s `(w, h)` and above `top`, where what
/// is below covers the screen. `None` once nothing of it is left.
pub fn sub_place(
    [x, y, w, h]: [i32; 4],
    (dw, dh): (u32, u32),
    top: u32,
) -> Option<((f64, f64, f64, f64), Rect)> {
    let place = (f64::from(x), f64::from(y), f64::from(w), f64::from(h));
    let span = |from: i32, len: i32, end: u32| {
        let a = i64::from(from).clamp(0, i64::from(end));
        let b = (i64::from(from) + i64::from(len)).clamp(a, i64::from(end));
        (a as u32, (b - a) as u32)
    };
    let (cx, cw) = span(x, w, dw);
    let (cy, ch) = span(y, h, dh.min(top));
    let clip = Rect {
        x: cx,
        y: cy,
        w: cw,
        h: ch,
    };
    (cw > 0 && ch > 0).then_some((place, clip))
}

/// A line from `a` to `b` of squares `thick` pixels across, within
/// `clip`, every other few pixels left out if `dashed`.
fn line(
    fb: &mut Framebuffer,
    a: (f64, f64),
    b: (f64, f64),
    clip: Rect,
    thick: u32,
    color: u32,
    dashed: bool,
) {
    let Some((a, b)) = clip_line(a, b, clip) else {
        return;
    };
    // The squares' corners can stray past the ends by their size.
    let (x0, y0) = (a.0.min(b.0), a.1.min(b.1));
    let reach = f64::from(thick) + 1.0;
    let around = Rect {
        x: (x0 - reach).max(0.0) as u32,
        y: (y0 - reach).max(0.0) as u32,
        w: ((a.0 - b.0).abs() + 2.0 * reach) as u32,
        h: ((a.1 - b.1).abs() + 2.0 * reach) as u32,
    };
    let ends = [a.0, a.1, b.0, b.1].map(f64::to_bits);
    let what = ("line", ends, clip, thick, color, dashed);
    if crate::ui::dry(fb, around, what) {
        return;
    }
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let steps = dx.abs().max(dy.abs()).ceil().max(1.0) as u32;
    let half = f64::from(thick) / 2.0;
    let (x1, y1) = (clip.x + clip.w, clip.y + clip.h);
    for i in 0..=steps {
        if dashed && (i / (4 * thick)) % 2 == 1 {
            continue;
        }
        let t = f64::from(i) / f64::from(steps);
        let (x, y) = (a.0 + dx * t - half, a.1 + dy * t - half);
        let (x, y) = (x.round().max(0.0) as u32, y.round().max(0.0) as u32);
        for py in y.max(clip.y)..(y + thick).min(y1) {
            for px in x.max(clip.x)..(x + thick).min(x1) {
                fb.set(px, py, color);
            }
        }
    }
}

/// The part of the line from `a` to `b` inside `clip`, if any.
fn clip_line(a: (f64, f64), b: (f64, f64), clip: Rect) -> Option<((f64, f64), (f64, f64))> {
    let (x0, y0) = (f64::from(clip.x), f64::from(clip.y));
    let (x1, y1) = (x0 + f64::from(clip.w), y0 + f64::from(clip.h));
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-dx, a.0 - x0),
        (dx, x1 - a.0),
        (-dy, a.1 - y0),
        (dy, y1 - a.1),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            t0 = t0.max(r);
        } else {
            t1 = t1.min(r);
        }
        if t0 > t1 {
            return None;
        }
    }
    let at = |t: f64| (a.0 + dx * t, a.1 + dy * t);
    Some((at(t0), at(t1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_runtime::font::Font;
    use sb_runtime::machine::Machine;

    fn run(source: &str) -> Machine {
        let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
        m.boot();
        m.set_slot(0, source.to_string());
        m.run_slot(0);
        for _ in 0..10 {
            m.frame();
            if !m.is_running() {
                break;
            }
        }
        assert!(!m.is_running(), "program did not end");
        m
    }

    #[test]
    fn lists_sprites_in_use_and_gathers_the_rows_shown() {
        let m = run("GTARGET 4:GFILL 0,0,99,99,#C_RED\n\
             SPSET 3,0:SPSET 10,5:SPSET 20,0:SPSET 7,0:SPCLR 7\n\
             SPOFS 10,1.5,2,-3:SPROT 10,45:SPSCALE 10,2,1:SPCOLOR 10,&HFF00FF00\n\
             SPLINK 20,10,1:SPHIDE 10:SPVAR 10,0,5:SPVAR 10,2,0:SPVAR 10,\"HP\",3\n\
             SPCOL 10,0,0,8,8,1,&HF:SPANIM 10,\"XY\",60,10,10:SPANIM 10,\"UV\",60,0,0\n\
             SPSTOP 10");
        let all = list(&m.rt, 0, 100, Some(10));
        // #GSPRITE is in use from the start.
        assert_eq!(all.total, 4);
        let ids: Vec<usize> = all.rows.iter().map(|r| r.id).collect();
        assert_eq!(ids, [3, 10, 20, 4095]);
        let part = list(&m.rt, 1, 2, None);
        let ids: Vec<usize> = part.rows.iter().map(|r| r.id).collect();
        assert_eq!((part.total, ids), (4, vec![10, 20]));
        assert_eq!(part.outline, None);

        let r = &all.rows[1];
        assert_eq!((r.def, r.image[2], r.z, r.color), (5, 16, -3, 0xFF00_FF00));
        assert!(!r.shown && !all.rows[2].shows && all.rows[2].shown);
        assert_eq!(all.rows[2].parent, Some(10));
        assert!(r.thumb.is_some(), "GRP4 holds the sprite image");
        assert_eq!(all.rows[3].thumb, None, "GRP0 is blank");
        let vars: Vec<&str> = r.vars.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(vars, ["0", "\"HP\""]);
        let cells = cells(r);
        assert_eq!(&cells[..7], ["10", "5", "1.5", "2", "-3", "45", "2,1"]);
        assert_eq!((cells[8].as_str(), cells[10].as_str()), ("-", "XY,UV"));
        assert_eq!(super::cells(&all.rows[2])[9], "10");

        let outline = all.outline.unwrap();
        assert!(!outline.shows);
        assert_eq!(Some(outline.corners), m.rt.sprite_corners(10));

        let tip = details(r, false);
        assert_eq!(tip[0], "SPRITE 10 (hidden)");
        assert!(tip.contains(&"COL 0,0 8x8 scaled  MASK &HF".to_string()));
        assert!(tip.contains(&"ANIM XY,UV (SPSTOP)".to_string()));
        assert_eq!(tip.last().unwrap(), "VAR 0=5  \"HP\"=3");
        let child = details(&all.rows[2], false);
        assert_eq!(child[0], "SPRITE 20 (hidden by its parent)");
        assert!(child.contains(&"LINK to 10, follows its shown".to_string()));
    }

    #[test]
    fn attributes_in_short() {
        let m = run("SPSET 0,0,0,16,16,1+4+16:SPSET 1,0");
        let all = list(&m.rt, 0, 2, None);
        assert_eq!(attr_text(&all.rows[0]), "S 90 H+");
        assert_eq!(attr_text(&all.rows[1]), "S");
        assert_eq!(anim_text(&all.rows[1]), "-");
        let tip = details(&all.rows[0], false);
        assert!(tip.contains(&"ATTR &H15: shown, turned 90, flipped H, additive".to_string()));
    }

    #[test]
    fn columns_left_out_for_room() {
        let needed: usize = COLUMNS.iter().filter(|c| !c.2).map(|c| c.1).sum();
        assert_eq!(fitting(0, 2), 5);
        assert_eq!(fitting(needed + 2 + 7, 2), 6);
        assert_eq!(fitting(1000, 2), COLUMNS.len());
    }

    #[test]
    fn outlines_stay_in_their_place() {
        let mut fb = Framebuffer::new(40, 40);
        fb.clear(0);
        let outline = Outline {
            corners: [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
            shows: true,
        };
        let clip = Rect {
            x: 10,
            y: 10,
            w: 20,
            h: 20,
        };
        // A 10x10 frame shown twice its size at (10, 10).
        draw_outline(
            &mut fb,
            &outline,
            (10, 10),
            (10.0, 10.0, 20.0, 20.0),
            clip,
            1,
        );
        assert_eq!(fb.get(20, 10), Some(OUTLINE));
        assert_eq!(fb.get(20, 20), Some(0), "inside stays clear");
        assert_eq!(fb.get(5, 5), Some(0), "nothing outside the place");
        assert_eq!(fb.get(9, 20), Some(0));
        let far = Outline {
            corners: [(-1e9, -1e9), (1e9, -1e9), (1e9, 1e9), (-1e9, 1e9)],
            shows: false,
        };
        let mut fb = Framebuffer::new(40, 40);
        fb.clear(0);
        draw_outline(&mut fb, &far, (10, 10), (10.0, 10.0, 20.0, 20.0), clip, 1);
        assert!(fb.pixels().iter().all(|&p| p == 0), "all off the place");
        assert_eq!(
            clip_line((0.0, 15.0), (40.0, 15.0), clip),
            Some(((10.0, 15.0), (30.0, 15.0)))
        );
    }

    #[test]
    fn a_subprograms_outline_keeps_to_its_screen() {
        let place = [10, 20, 300, 200];
        let (at, clip) = sub_place(place, (1280, 720), 720).unwrap();
        assert_eq!(at, (10.0, 20.0, 300.0, 200.0));
        assert_eq!(
            clip,
            Rect {
                x: 10,
                y: 20,
                w: 300,
                h: 200
            }
        );
        // Off the display's left edge, and under the panel from 650 down.
        let (_, clip) = sub_place([-50, 600, 100, 200], (1280, 720), 650).unwrap();
        assert_eq!(
            clip,
            Rect {
                x: 0,
                y: 600,
                w: 50,
                h: 50
            }
        );
        assert_eq!(sub_place(place, (1280, 720), 20), None);
        assert_eq!(sub_place([0, 0, 0, 10], (1280, 720), 720), None);
    }
}

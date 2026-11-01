//! Text screens 0-4. Screen 4 is the console that PRINT writes to.

use std::collections::HashMap;

use sb_core::value::{Array, ArrayData, ArrayRef, ElemType, Value};

use crate::anim::{self, ANIM_VAR, Animated, Target, Track};
use crate::compose::{self, Affine, Placement, draw_image_part, over};
use crate::font::{Font, FontSize};
use crate::framebuffer::Framebuffer;
use crate::graphics::{Clip, Graphics};
use crate::sprites::{VarKey, sample, var_number};

pub const SCREENS: usize = 5;
pub const CONSOLE: usize = 4;
/// Characters from here to `0xF7FF` are drawn from a graphic page (TPAGE).
pub const USER_CHARS: std::ops::RangeInclusive<u16> = 0xE800..=0xF7FF;
/// UNVERIFIED: SB3's default TABSTEP; SB4's console has not been measured.
pub const TAB_WIDTH: u32 = 4;
const WHITE: u32 = 0xFFFF_FFFF;
/// GRP5 (`#GRPF`) holds the font that text screens and GPUTCHR draw.
pub const FONT_PAGE: usize = 5;
/// GRP4 holds the default sprite image and user-defined characters.
pub const SPRITE_PAGE: usize = 4;

/// Pixel `(x, y)` of a character as GRP5 now shows it; fallback glyphs
/// that have no place there come from the font itself.
pub fn glyph_pixel(font: &Font, grp: &Graphics, code: u16, size: FontSize, x: u32, y: u32) -> u32 {
    match font.origin(code, size) {
        Some((gx, gy)) => grp.pixel(FONT_PAGE, (gx + x) as i32, (gy + y) as i32),
        None => font.pixel(code, size, x, y),
    }
}

/// Like [`glyph_pixel`] for any font type: 24 to 64 take glyph number i
/// from GRP5 at (i % 64, i / 64) cells of that size, as
/// `#GUIDE/DEFFONT24` lays them out (GPUTCHR drew it on hardware after
/// `TSCREEN 24`).
pub fn glyph_pixel_px(font: &Font, grp: &Graphics, code: u16, px: u32, x: u32, y: u32) -> u32 {
    match px {
        8 => glyph_pixel(font, grp, code, FontSize::Small, x, y),
        16 => glyph_pixel(font, grp, code, FontSize::Large, x, y),
        _ => font.glyph_index(code).map_or(0, |i| {
            let (gx, gy) = (i % 64 * px + x, i / 64 * px + y);
            grp.pixel(FONT_PAGE, gx as i32, gy as i32)
        }),
    }
}

/// User-defined characters sit in a grid this many characters wide.
const USER_GRID: i32 = 64;

/// ATTR bits of a character.
pub mod attr {
    pub const ROTATION: u8 = 0b11;
    pub const REVH: u8 = 0b100;
    pub const REVV: u8 = 0b1000;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// 0 is an empty cell.
    pub code: u16,
    pub color: u32,
    /// ATTR: quarter turns and flips.
    pub attr: u8,
}

const EMPTY: Cell = Cell {
    code: 0,
    color: 0,
    attr: 0,
};

impl Cell {
    /// TARRAY's two elements: the attribute over the character code, and
    /// the color.
    pub fn to_ints(self) -> [i32; 2] {
        [
            i32::from(self.attr) << 16 | i32::from(self.code),
            self.color as i32,
        ]
    }

    pub fn from_ints([code, color]: [i32; 2]) -> Cell {
        Cell {
            code: code as u16,
            color: color as u32,
            attr: (code >> 16) as u8 & 0xF,
        }
    }
}

/// A rectangle of cells: `x, y, width, height`.
pub type CellRect = (i32, i32, i32, i32);

pub struct TextScreen {
    /// TSCREEN's font type: 8 or 16 for the normal font at that size, or
    /// up to 64 for user-defined characters only.
    font_type: u32,
    /// Size each character is drawn at, in pixels.
    char_size: u32,
    width: u32,
    height: u32,
    cells: Vec<Cell>,
    cursor: (u32, u32),
    color: u32,
    /// ATTR for characters written from now on.
    pub attr: u8,
    /// TPAGE: where user-defined characters come from.
    pub page: i32,
    pub page_origin: (i32, i32),
    pub visible: bool,
    /// TOFS, THOME, TROT and TSCALE place the screen like a sprite.
    pub pos: (f64, f64),
    /// Smaller is nearer the front: -4095 to 4095.
    pub z: i32,
    pub home: (f64, f64),
    pub rot: f64,
    pub scale: (f64, f64),
    /// TCOLOR: multiplies the whole screen.
    pub tint: u32,
    /// TBLEND 1: additive.
    pub add: bool,
    pub layer: i32,
    /// TARRAY's array, kept up to date with every cell written.
    array: Option<ArrayRef>,
    /// TANIM's animations, by [`anim::Target`].
    pub anims: [Option<Track>; anim::TARGETS],
    /// TSTOP pauses the animations until TSTART.
    pub anim_stopped: bool,
    /// TVAR's variables.
    pub vars: HashMap<VarKey, Value>,
    /// TFUNC: the label or DEF that CALL TEXT calls.
    pub func: Option<Vec<u16>>,
}

impl TextScreen {
    /// A screen filling `screen_w` x `screen_h` pixels with the 8-pixel font.
    pub fn new(screen_w: u32, screen_h: u32) -> TextScreen {
        let (width, height) = (screen_w / 8, screen_h / 8);
        TextScreen {
            font_type: 8,
            char_size: 8,
            width,
            height,
            cells: vec![EMPTY; (width * height) as usize],
            cursor: (0, 0),
            color: WHITE,
            attr: 0,
            page: 4,
            page_origin: (1024, 0),
            visible: true,
            pos: (0.0, 0.0),
            z: 0,
            home: (0.0, 0.0),
            rot: 0.0,
            scale: (1.0, 1.0),
            tint: WHITE,
            add: false,
            layer: 0,
            array: None,
            anims: Default::default(),
            anim_stopped: false,
            vars: HashMap::new(),
            func: None,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// TSCREEN's font type and character size.
    pub fn font(&self) -> (u32, u32) {
        (self.font_type, self.char_size)
    }

    /// TSCREEN: a new font and size, which clears the screen.
    // UNVERIFIED: whether TSCREEN keeps the contents.
    pub fn set_font(&mut self, font_type: u32, char_size: u32, width: u32, height: u32) {
        (self.font_type, self.char_size) = (font_type, char_size);
        (self.width, self.height) = (width, height);
        self.cells = vec![EMPTY; (width * height) as usize];
        self.cursor = (0, 0);
        // TARRAY's array takes the new size and goes on following the
        // screen (checked on hardware).
        if let Some(array) = &self.array {
            let mut array = array.borrow_mut();
            array.dims = vec![height as usize, width as usize * 2];
            array.data = ArrayData::Int(vec![0; (width * height * 2) as usize]);
        }
    }

    pub fn cursor(&self) -> (u32, u32) {
        self.cursor
    }

    pub fn color(&self) -> u32 {
        self.color
    }

    pub fn set_color(&mut self, color: u32) {
        self.color = color;
    }

    pub fn cell(&self, x: u32, y: u32) -> Cell {
        self.cells[(y * self.width + x) as usize]
    }

    /// The cell at `(x, y)`, or `None` off the screen.
    pub fn cell_at(&self, x: i32, y: i32) -> Option<Cell> {
        self.index(x, y).map(|i| self.cells[i])
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let x = u32::try_from(x).ok().filter(|&x| x < self.width)?;
        let y = u32::try_from(y).ok().filter(|&y| y < self.height)?;
        Some((y * self.width + x) as usize)
    }

    fn set(&mut self, i: usize, cell: Cell) {
        self.cells[i] = cell;
        if let Some(array) = &self.array
            && let ArrayData::Int(v) = &mut array.borrow_mut().data
            && v.len() == self.cells.len() * 2
        {
            v[i * 2..i * 2 + 2].copy_from_slice(&cell.to_ints());
        }
    }

    /// Writes one cell; off-screen cells are ignored.
    pub fn set_cell(&mut self, x: i32, y: i32, cell: Cell) {
        if let Some(i) = self.index(x, y) {
            self.set(i, cell);
        }
    }

    /// Writes one cell with the current ATTR without moving the cursor;
    /// off-screen cells are ignored.
    pub fn put_at(&mut self, x: u32, y: u32, code: u16, color: u32) {
        let attr = self.attr;
        self.set_cell(x as i32, y as i32, Cell { code, color, attr });
    }

    /// The cells of `rect` row by row, empty where it leaves the screen.
    pub fn save(&self, (x, y, w, h): CellRect) -> Vec<Cell> {
        let mut cells = Vec::with_capacity((w.max(0) * h.max(0)) as usize);
        for cy in y..y + h {
            for cx in x..x + w {
                cells.push(self.cell_at(cx, cy).unwrap_or(EMPTY));
            }
        }
        cells
    }

    /// Writes row-by-row `cells` into `rect`, skipping what leaves the
    /// screen.
    pub fn load(&mut self, (x, y, w, _): CellRect, cells: &[Cell]) {
        for (i, &cell) in cells.iter().enumerate() {
            let i = i as i32;
            self.set_cell(x + i % w, y + i / w, cell);
        }
    }

    /// TARRAY: an array of the cells that follows every write to them;
    /// TUPDATE brings writes to the array back.
    pub fn array(&mut self) -> ArrayRef {
        if let Some(array) = &self.array {
            return array.clone();
        }
        let mut array = Array::new(
            ElemType::Int,
            vec![self.height as usize, self.width as usize * 2],
        );
        array.data = ArrayData::Int(self.cells.iter().flat_map(|c| c.to_ints()).collect());
        let array = ArrayRef::new(array.into());
        self.array = Some(array.clone());
        array
    }

    /// TARRAY's array, if a program has asked for it.
    pub fn array_if_made(&self) -> Option<ArrayRef> {
        self.array.clone()
    }

    /// TUPDATE: the cells of `rect` from TARRAY's array.
    pub fn update(&mut self, (x, y, w, h): CellRect) {
        let Some(array) = &self.array else { return };
        let array = array.borrow();
        let ArrayData::Int(v) = &array.data else {
            return;
        };
        if v.len() != self.cells.len() * 2 {
            return;
        }
        for cy in y.max(0)..(y + h).min(self.height as i32) {
            for cx in x.max(0)..(x + w).min(self.width as i32) {
                let i = (cy as u32 * self.width + cx as u32) as usize;
                self.cells[i] = Cell::from_ints([v[i * 2], v[i * 2 + 1]]);
            }
        }
    }

    /// Clears the screen and homes the cursor.
    pub fn cls(&mut self) {
        for i in 0..self.cells.len() {
            self.set(i, EMPTY);
        }
        self.cursor = (0, 0);
    }

    /// SCROLL: moves the contents `dx` cells left and `dy` up, emptying
    /// what is uncovered. The cursor stays.
    // UNVERIFIED: that the cursor stays.
    pub fn scroll(&mut self, dx: i32, dy: i32) {
        let (w, h) = (self.width as i32, self.height as i32);
        let moved = self.save((dx, dy, w, h));
        self.load((0, 0, w, h), &moved);
    }

    /// Moves the cursor; `None` keeps that coordinate. Returns false if the
    /// position is off the screen.
    pub fn locate(&mut self, x: Option<i32>, y: Option<i32>) -> bool {
        let x = x.unwrap_or(self.cursor.0 as i32);
        let y = y.unwrap_or(self.cursor.1 as i32);
        let fits = |v: i32, max: u32| u32::try_from(v).is_ok_and(|v| v < max);
        // The column just past the right edge is where a full line leaves
        // the cursor.
        if !fits(x, self.width + 1) || !fits(y, self.height) {
            return false;
        }
        self.cursor = (x as u32, y as u32);
        true
    }

    pub fn print(&mut self, text: &[u16]) {
        for &c in text {
            match c {
                // UNVERIFIED: whether CHR$(10) also starts a new line.
                10 | 13 => self.newline(),
                _ => self.put(c),
            }
        }
    }

    /// PRINT's `,`: pads with spaces to the next tab stop.
    pub fn tab(&mut self) {
        let pad = TAB_WIDTH - self.cursor.0 % TAB_WIDTH;
        for _ in 0..pad {
            self.put(u16::from(b' '));
        }
    }

    /// Steps the cursor back one cell, to the previous line if needed, and
    /// clears that cell.
    pub fn backspace(&mut self) {
        let (x, y) = self.cursor;
        self.cursor = match (x, y) {
            (0, 0) => return,
            (0, y) => (self.width - 1, y - 1),
            (x, y) => (x - 1, y),
        };
        let (x, y) = self.cursor;
        self.set((y * self.width + x) as usize, EMPTY);
    }

    pub fn newline(&mut self) {
        self.cursor.0 = 0;
        if self.cursor.1 + 1 < self.height {
            self.cursor.1 += 1;
        } else {
            self.scroll_up();
        }
    }

    /// Writing the last column leaves the cursor just past it; the wrap
    /// happens with the next character, so a full line followed by a
    /// newline moves down only once.
    fn put(&mut self, code: u16) {
        if self.width == 0 || self.height == 0 {
            return;
        }
        if self.cursor.0 >= self.width {
            self.newline();
        }
        let (x, y) = self.cursor;
        let cell = Cell {
            code,
            color: self.color,
            attr: self.attr,
        };
        self.set((y * self.width + x) as usize, cell);
        self.cursor.0 += 1;
    }

    fn scroll_up(&mut self) {
        self.scroll(0, 1);
    }

    /// Plays one frame of the animations.
    pub fn animate(&mut self) {
        if self.anim_stopped {
            return;
        }
        let mut tracks = std::mem::take(&mut self.anims);
        for slot in &mut tracks {
            if let Some(track) = slot
                && !track.step(self)
            {
                *slot = None;
            }
        }
        self.anims = tracks;
    }

    /// Where TOFS, THOME, TROT and TSCALE put the screen's pixels.
    pub fn placement(&self) -> Affine {
        Affine::place(self.pos, self.home, self.rot, self.scale)
    }

    /// The smallest and largest x and y the screen covers through its
    /// layer's matrix `layer`.
    pub fn extent(&self, layer: &Affine) -> (f64, f64, f64, f64) {
        let cs = f64::from(self.char_size);
        let (w, h) = (f64::from(self.width) * cs, f64::from(self.height) * cs);
        compose::bounds(&self.placement().then(layer), (0.0, 0.0, w, h))
    }

    /// Draws the screen over `fb` where TOFS and its companions put it.
    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        let mut grp = Graphics::default();
        grp.set_page(FONT_PAGE, font.page_image());
        self.draw_placed(fb, font, &grp, &Affine::IDENTITY, None, false);
    }

    /// The pixel at `(x, y)` of a character's `font_type`-sized image.
    fn glyph_pixel(&self, font: &Font, grp: &Graphics, code: u16, x: u32, y: u32) -> u32 {
        let ft = self.font_type;
        if USER_CHARS.contains(&code) {
            let i = i32::from(code - USER_CHARS.start());
            let (ox, oy) = self.page_origin;
            let size = ft as i32;
            let gx = ox + i % USER_GRID * size + x as i32;
            let gy = oy + i / USER_GRID * size + y as i32;
            return sample(grp, self.page, gx, gy);
        }
        match ft {
            8 => glyph_pixel(font, grp, code, FontSize::Small, x, y),
            16 => glyph_pixel(font, grp, code, FontSize::Large, x, y),
            // Larger font types show only user-defined characters.
            _ => 0,
        }
    }

    /// Draws the screen through its layer's matrix and clipping, with the
    /// INPUT cursor if `cursor` is set.
    // UNVERIFIED: the shape of SmileBASIC's INPUT cursor (a cell filled
    // with the text color).
    pub fn draw_placed(
        &self,
        fb: &mut Framebuffer,
        font: &Font,
        grp: &Graphics,
        layer: &Affine,
        clip: Option<&Clip>,
        cursor: bool,
    ) {
        if !self.visible {
            return;
        }
        let cs = self.char_size;
        let ft = self.font_type;
        let cursor = cursor.then_some(self.cursor);
        if cursor.is_none() && self.cells.iter().all(|c| c.code == 0) {
            return;
        }
        let place = Placement {
            matrix: self.placement().then(layer),
            clip,
            add: self.add,
        };
        if place.is_upright() {
            return self.draw_upright(fb, font, grp, &place, cursor);
        }
        let (w, h) = ((self.width * cs) as i32, (self.height * cs) as i32);
        let size = f64::from(cs);
        // The cell's character in its colors, looked up once rather than
        // for every pixel it covers.
        let mut glyph = vec![0; (ft * ft) as usize];
        for cy in 0..self.height {
            for cx in 0..self.width {
                let cell = self.cell(cx, cy);
                let at_cursor = cursor == Some((cx, cy));
                // Empty cells show nothing, so need no drawing.
                if cell.code == 0 && !at_cursor {
                    continue;
                }
                self.glyph(font, grp, cell, at_cursor, &mut glyph);
                let (x0, y0) = ((cx * cs) as i32, (cy * cs) as i32);
                let part = (x0, y0, x0 + cs as i32, y0 + cs as i32);
                draw_image_part(fb, &place, w, h, part, |x, y| {
                    // Pixels of the cells around are theirs to draw.
                    if ((x / size) as u32, (y / size) as u32) != (cx, cy) {
                        return 0;
                    }
                    // Where in the cell, which a fine frame splits further.
                    // As for sprites, the character is flipped, then turned
                    // (checked on hardware): undo the turn, then the flips.
                    let (px, py) = (x - f64::from(cx * cs), y - f64::from(cy * cs));
                    let (mut px, mut py) = match cell.attr & attr::ROTATION {
                        0 => (px, py),
                        1 => (py, size - px),
                        2 => (size - px, size - py),
                        _ => (size - py, px),
                    };
                    if cell.attr & attr::REVH != 0 {
                        px = size - px;
                    }
                    if cell.attr & attr::REVV != 0 {
                        py = size - py;
                    }
                    let at = |p: f64| ((p * f64::from(ft) / size) as u32).min(ft - 1);
                    glyph[(at(py) * ft + at(px)) as usize]
                });
            }
        }
    }
}

impl TextScreen {
    /// A cell's character in its colors, `ft` x `ft`, as draw_placed
    /// works it out.
    fn glyph(&self, font: &Font, grp: &Graphics, cell: Cell, at_cursor: bool, out: &mut [u32]) {
        let ft = self.font_type;
        let color = |p: u32| {
            let p = if cell.code == 0 {
                0
            } else {
                multiply(p, cell.color)
            };
            let p = if at_cursor { over(p, self.color) } else { p };
            multiply(p, self.tint)
        };
        // Where the character comes from is looked up once, not for
        // every pixel; the pixels are those glyph_pixel gives.
        let mut fill = |pixel: &dyn Fn(u32, u32) -> u32| {
            for (i, p) in out.iter_mut().enumerate() {
                *p = color(pixel(i as u32 % ft, i as u32 / ft));
            }
        };
        let size = match ft {
            8 => Some(FontSize::Small),
            16 => Some(FontSize::Large),
            _ => None,
        };
        match (cell.code, size) {
            (0, _) => fill(&|_, _| 0),
            (code, _) if USER_CHARS.contains(&code) => {
                fill(&|x, y| self.glyph_pixel(font, grp, code, x, y))
            }
            (code, Some(size)) => match font.origin(code, size) {
                Some((gx, gy)) => {
                    fill(&|x, y| grp.pixel(FONT_PAGE, (gx + x) as i32, (gy + y) as i32))
                }
                None => fill(&|x, y| font.pixel(code, size, x, y)),
            },
            // Larger font types show only user-defined characters.
            (_, None) => fill(&|_, _| 0),
        }
    }

    /// draw_placed for a screen neither turned nor sheared on the frame:
    /// which cell and which point of it each frame column and row shows
    /// is worked out once, and each row of cells' characters once.
    fn draw_upright(
        &self,
        fb: &mut Framebuffer,
        font: &Font,
        grp: &Graphics,
        place: &Placement,
        cursor: Option<(u32, u32)>,
    ) {
        let (cs, ft) = (self.char_size, self.font_type);
        let (w, h) = ((self.width * cs) as i32, (self.height * cs) as i32);
        let Some(grid) = crate::compose::grid(fb, place, w, h) else {
            return;
        };
        let size = f64::from(cs);
        let at = |p: f64| ((p * f64::from(ft) / size) as u32).min(ft - 1);
        // For each column (or row): its cell, and the glyph column (or
        // row) for the point in the cell, for the point flipped, and for
        // it flipped twice, as draw_placed's arithmetic gives them.
        let axis = |v: f64| {
            let c = (v / size) as u32;
            let p = v - f64::from(c * cs);
            (c, [at(p), at(size - p), at(size - (size - p))])
        };
        let cols: Vec<(u32, [u32; 3])> = grid.xs.iter().map(|&x| axis(x)).collect();
        // Which of those each glyph coordinate comes from, by ATTR: the
        // column's (false) or the row's (true) table, and which entry.
        let pick = |a: u8| -> ((bool, usize), (bool, usize)) {
            let (mut p, mut q) = match a & attr::ROTATION {
                0 => ((false, 0), (true, 0)),
                1 => ((true, 0), (false, 1)),
                2 => ((false, 1), (true, 1)),
                _ => ((true, 1), (false, 0)),
            };
            if a & attr::REVH != 0 {
                p.1 += 1;
            }
            if a & attr::REVV != 0 {
                q.1 += 1;
            }
            (p, q)
        };
        let glyph_len = (ft * ft) as usize;
        // Each character in each color once, however many cells show it.
        let mut glyphs: Vec<u32> = Vec::new();
        let mut made: HashMap<(u16, u32, bool), usize> = HashMap::new();
        // Per cell of the row, if it shows anything: its picks and where
        // its character is in `glyphs`.
        let mut shown = vec![None; self.width as usize];
        let mut row_of = None;
        let width = fb.width() as usize;
        let pixels = fb.pixels_mut();
        for (fy, &y) in (grid.y0 as usize..).zip(&grid.ys) {
            let (cy, rat) = axis(y);
            if row_of != Some(cy) {
                row_of = Some(cy);
                for cx in 0..self.width {
                    let cell = self.cell(cx, cy);
                    let at_cursor = cursor == Some((cx, cy));
                    shown[cx as usize] = (cell.code != 0 || at_cursor).then(|| {
                        let key = (cell.code, cell.color, at_cursor);
                        let at = *made.entry(key).or_insert_with(|| {
                            let at = glyphs.len();
                            glyphs.resize(at + glyph_len, 0);
                            self.glyph(font, grp, cell, at_cursor, &mut glyphs[at..]);
                            at
                        });
                        (pick(cell.attr), at)
                    });
                }
            }
            let line = &mut pixels[fy * width + grid.x0 as usize..][..cols.len()];
            for (dst, &(cx, cat)) in line.iter_mut().zip(&cols) {
                let Some(((p, q), at)) = shown[cx as usize] else {
                    continue;
                };
                let coord = |(row, k): (bool, usize)| if row { rat[k] } else { cat[k] };
                let src = glyphs[at + (coord(q) * ft + coord(p)) as usize];
                if src >> 24 == 0 {
                    continue;
                }
                *dst = crate::compose::blend(*dst, src, place.add);
            }
        }
    }
}

/// Text screens have no image to animate: "UV" and "I" do nothing.
// UNVERIFIED: Z and the color take the value rounded towards zero, as
// for sprites.
impl Animated for TextScreen {
    fn value(&self, target: Target) -> [f64; 2] {
        match target {
            Target::Xy => [self.pos.0, self.pos.1],
            Target::Z => [f64::from(self.z), 0.0],
            Target::R => [self.rot, 0.0],
            Target::S => [self.scale.0, self.scale.1],
            Target::C => [f64::from(self.tint), 0.0],
            Target::V => [var_number(&self.vars), 0.0],
            Target::Uv | Target::I => [0.0, 0.0],
        }
    }

    fn set_value(&mut self, target: Target, v: [f64; 2]) {
        match target {
            Target::Xy => self.pos = (v[0], v[1]),
            Target::Z => self.z = (v[0] as i32).clamp(-4095, 4095),
            Target::R => self.rot = v[0],
            Target::S => self.scale = (v[0], v[1]),
            Target::C => self.tint = v[0] as i64 as u32,
            Target::V => {
                self.vars.insert(VarKey::Int(ANIM_VAR), Value::Real(v[0]));
            }
            Target::Uv | Target::I => {}
        }
    }
}

fn channels(c: u32) -> [u32; 4] {
    c.to_be_bytes().map(u32::from)
}

/// Per-channel product, as used for text colors.
fn multiply(a: u32, b: u32) -> u32 {
    if b == WHITE {
        return a;
    }
    let (a, b) = (channels(a), channels(b));
    let c: [u8; 4] = std::array::from_fn(|i| (a[i] * b[i] / 255) as u8);
    u32::from_be_bytes(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn row(screen: &TextScreen, y: u32) -> String {
        let (w, _) = screen.size();
        let codes: Vec<u16> = (0..w)
            .map(|x| screen.cell(x, y).code)
            .take_while(|&c| c != 0)
            .collect();
        String::from_utf16(&codes).unwrap()
    }

    #[test]
    fn console_size() {
        assert_eq!(TextScreen::new(400, 240).size(), (50, 30));
    }

    #[test]
    fn print_wraps_and_scrolls() {
        let mut s = TextScreen::new(32, 24); // 4 x 3 characters
        s.print(&text("ABCDE"));
        assert_eq!((row(&s, 0).as_str(), row(&s, 1).as_str()), ("ABCD", "E"));
        assert_eq!(s.cursor(), (1, 1));
        s.newline();
        s.print(&text("X\rY"));
        assert_eq!(row(&s, 0), "E");
        assert_eq!(row(&s, 1), "X");
        assert_eq!(row(&s, 2), "Y");
    }

    #[test]
    fn full_line_then_newline_moves_down_once() {
        let mut s = TextScreen::new(32, 24);
        s.print(&text("ABCD"));
        assert_eq!(s.cursor(), (4, 0));
        s.newline();
        assert_eq!(s.cursor(), (0, 1));
        s.print(&text("EFGHI"));
        assert_eq!((row(&s, 1).as_str(), row(&s, 2).as_str()), ("EFGH", "I"));
        s.backspace();
        s.backspace();
        assert_eq!((row(&s, 1).as_str(), s.cursor()), ("EFG", (3, 1)));
    }

    #[test]
    fn backspace_crosses_lines() {
        let mut s = TextScreen::new(32, 24);
        s.print(&text("ABCDE"));
        s.backspace();
        s.backspace();
        assert_eq!((row(&s, 0).as_str(), row(&s, 1).as_str()), ("ABC", ""));
        assert_eq!(s.cursor(), (3, 0));
        s.locate(Some(0), Some(0));
        s.backspace();
        assert_eq!(s.cursor(), (0, 0));
    }

    #[test]
    fn tab_and_locate() {
        let mut s = TextScreen::new(80, 16);
        s.print(&text("A"));
        s.tab();
        s.print(&text("B"));
        assert_eq!(row(&s, 0), "A   B");
        assert!(s.locate(Some(9), None));
        assert_eq!(s.cursor(), (9, 0));
        // Just past the right edge is allowed; beyond it is not.
        assert!(s.locate(Some(10), Some(0)));
        assert!(!s.locate(Some(11), Some(0)));
        assert!(!s.locate(None, Some(-1)));
        s.cls();
        assert_eq!((s.cursor(), row(&s, 0).as_str()), ((0, 0), ""));
    }

    #[test]
    fn draw_blends_glyph_with_color() {
        let mut font = Font::blank();
        let (gx, gy) = font.origin(0x41, FontSize::Small).unwrap();
        font.page_mut()[(gy * crate::font::PAGE_SIZE + gx) as usize] = 0xFFFF_FFFF;
        font.page_mut()[(gy * crate::font::PAGE_SIZE + gx + 1) as usize] = 0x80FF_FFFF;
        let mut s = TextScreen::new(16, 8);
        s.set_color(0xFFFF_0000);
        s.print(&text("A"));
        let mut fb = Framebuffer::new(16, 8);
        s.draw(&mut fb, &font);
        assert_eq!(fb.get(0, 0), Some(0xFFFF_0000));
        assert_eq!(fb.get(1, 0), Some(0xFF80_0000));
        assert_eq!(fb.get(2, 0), Some(0xFF00_0000));
    }
}

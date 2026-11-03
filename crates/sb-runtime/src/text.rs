//! Text screens 0-4. Screen 4 is the console that PRINT writes to.

use std::collections::HashMap;

use sb_core::value::{Array, ArrayData, ArrayRef, ElemType, Value};

use crate::anim::{self, ANIM_VAR, Animated, Target, Track};
use crate::compose::{self, Affine, Mixed, Paint, Placement, draw_image_part};
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

/// From this many characters PRINT works out the scrolling first. Fewer
/// take too little time for it to matter, and they are most of what is
/// printed.
const LONG_PRINT: usize = 256;

/// User-defined characters sit in a grid this many characters wide.
const USER_GRID: i32 = 64;

/// ATTR bits of a character.
pub mod attr {
    pub const ROTATION: u16 = 0b11;
    pub const REVH: u16 = 0b100;
    pub const REVV: u16 = 0b1000;
    /// Not ATTR's: on the console, the cell is filled with its color, less
    /// the glyph. Other text screens keep the bit and show nothing of it.
    pub const INVERT: u16 = 0x80;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// 0 is an empty cell.
    pub code: u16,
    pub color: u32,
    /// ATTR, quarter turns and flips, and [`attr::INVERT`]. TLOAD and
    /// TUPDATE store all 16 bits, which is what TSAVE and CHKCHR read;
    /// only these do anything.
    pub attr: u16,
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
            attr: (code >> 16) as u16,
        }
    }
}

/// How [`TextScreen::draw_placed`] draws the cursor of a line being typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineCursor {
    /// A block over the whole cell rather than a bar at its left.
    pub overwrite: bool,
    /// Its layer is composed. The cursor replaces what its layer holds,
    /// and then shows through by its own alpha, so it adds its light to
    /// the frame without covering it.
    pub composed: bool,
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
    /// A cell has been written since [`TextScreen::take_changed`] last
    /// asked.
    cells_changed: bool,
    /// The cells as [`TextScreen::take_changed`] last saw them.
    cells_seen: Vec<Cell>,
    cursor: (u32, u32),
    color: u32,
    /// ATTR for characters written from now on.
    pub attr: u16,
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
    /// [`attr::INVERT`] does something here: only the console's program
    /// has the flag.
    pub inverts: bool,
    /// A cell may be inverted, which even an empty one shows. Few programs
    /// ever have one, so the others are spared looking at each cell's
    /// attribute to see what to skip.
    may_invert: bool,
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
            cells_changed: true,
            cells_seen: Vec::new(),
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
            inverts: false,
            may_invert: false,
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

    /// TSCREEN also takes the screen's look back to the start: its
    /// animations stop, THOME, TROT, TSCALE, TCOLOR and TBLEND are undone,
    /// and TOFS goes back to 0,0 at the same depth. TLAYER, TPAGE, TVAR
    /// and TFUNC stay.
    pub fn restart_look(&mut self) {
        self.home = (0.0, 0.0);
        self.rot = 0.0;
        self.scale = (1.0, 1.0);
        self.tint = WHITE;
        self.pos = (0.0, 0.0);
        self.add = false;
        self.stop_anims();
    }

    /// XSCREEN stops the screen's animations where they are.
    pub fn stop_anims(&mut self) {
        self.anims = Default::default();
        self.anim_stopped = false;
    }

    /// TSCREEN: a new font and size, which clears the screen.
    // UNVERIFIED: whether TSCREEN keeps the contents.
    pub fn set_font(&mut self, font_type: u32, char_size: u32, width: u32, height: u32) {
        (self.font_type, self.char_size) = (font_type, char_size);
        (self.width, self.height) = (width, height);
        self.cells = vec![EMPTY; (width * height) as usize];
        self.may_invert = false;
        self.cells_changed = true;
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
        self.may_invert |= self.inverts && cell.attr & attr::INVERT != 0;
        self.cells_changed |= self.cells[i] != cell;
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
        // A rectangle past the largest coordinate wraps to an empty one.
        for cy in y..y.wrapping_add(h) {
            for cx in x..x.wrapping_add(w) {
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
            self.set_cell(x.wrapping_add(i % w), y.wrapping_add(i / w), cell);
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
                let cell = Cell::from_ints([v[i * 2], v[i * 2 + 1]]);
                self.may_invert |= self.inverts && cell.attr & attr::INVERT != 0;
                self.cells[i] = cell;
            }
        }
        self.cells_changed = true;
    }

    /// Whether the cells differ from the last time this was asked. Cells
    /// written over with what they held, as when a program clears the
    /// screen and prints the same again every frame, do not count.
    pub fn take_changed(&mut self) -> bool {
        if !std::mem::take(&mut self.cells_changed) || self.cells == self.cells_seen {
            return false;
        }
        self.cells_seen.clone_from(&self.cells);
        true
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
        // A whole screen's shift or more leaves nothing; held to that, the
        // rectangle below cannot pass the largest coordinate.
        let (dx, dy) = (dx.clamp(-w, w), dy.clamp(-h, h));
        // Without TARRAY's array to keep in step, the cells can move in
        // place. An inverted cell has set may_invert when it was written.
        if self.array.is_none() {
            self.shift(dx, dy);
            return;
        }
        let moved = self.save((dx, dy, w, h));
        self.load((0, 0, w, h), &moved);
    }

    /// Each cell takes the one `(dx, dy)` away, or empty off the screen.
    /// The shift is at most the screen's size.
    fn shift(&mut self, dx: i32, dy: i32) {
        if (dx, dy) == (0, 0) || self.cells.is_empty() {
            return;
        }
        self.cells_changed = true;
        let (w, h) = (self.width as usize, self.height as usize);
        let keep = w - dx.unsigned_abs() as usize;
        let (from, to) = (dx.max(0) as usize, (-dx).max(0) as usize);
        let mut copy_row = |row: usize| {
            let start = row * w;
            match (row as i32)
                .checked_add(dy)
                .filter(|&r| (0..h as i32).contains(&r))
            {
                Some(source) => {
                    let source = source as usize * w;
                    self.cells
                        .copy_within(source + from..source + from + keep, start + to);
                    self.cells[start..start + to].fill(EMPTY);
                    self.cells[start + to + keep..start + w].fill(EMPTY);
                }
                None => self.cells[start..start + w].fill(EMPTY),
            }
        };
        // Rows are taken before they are written over.
        if dy >= 0 {
            (0..h).for_each(&mut copy_row);
        } else {
            (0..h).rev().for_each(&mut copy_row);
        }
    }

    /// Moves the cursor; `None` keeps that coordinate. Returns false if the
    /// position is off the screen.
    /// Moves the cursor, or gives which of X (0) and Y (1) is off the
    /// screen.
    pub fn locate(&mut self, x: Option<i32>, y: Option<i32>) -> std::result::Result<(), usize> {
        let x = x.unwrap_or(self.cursor.0 as i32);
        let y = y.unwrap_or(self.cursor.1 as i32);
        let fits = |v: i32, max: u32| u32::try_from(v).is_ok_and(|v| v < max);
        // The column just past the right edge is where a full line leaves
        // the cursor.
        if !fits(x, self.width + 1) {
            return Err(0);
        }
        if !fits(y, self.height) {
            return Err(1);
        }
        self.cursor = (x as u32, y as u32);
        Ok(())
    }

    pub fn print(&mut self, text: &[u16]) {
        if text.len() >= LONG_PRINT && self.width > 0 && self.height > 0 {
            self.print_long(text);
            return;
        }
        self.print_each(text);
    }

    fn print_each(&mut self, text: &[u16]) {
        for &c in text {
            match c {
                // UNVERIFIED: whether CHR$(10) also starts a new line.
                10 | 13 => self.newline(),
                _ => self.put(c),
            }
        }
    }

    /// The same as [`Self::print_each`], but scrolling only once: each
    /// line can move the whole screen, which for long text would take as
    /// long as the text times the screen.
    fn print_long(&mut self, text: &[u16]) {
        let width = self.width;
        // A line counts up from the cursor's row without scrolling.
        let step = |(x, line): &mut (u32, u64), c: u16| -> Option<(u32, u64)> {
            if matches!(c, 10 | 13) {
                (*x, *line) = (0, *line + 1);
                return None;
            }
            if *x >= width {
                (*x, *line) = (0, *line + 1);
            }
            *x += 1;
            Some((*x - 1, *line))
        };
        let start = (self.cursor.0, u64::from(self.cursor.1));
        let mut end = start;
        for &c in text {
            step(&mut end, c);
        }
        let scrolled = end.1.saturating_sub(u64::from(self.height - 1));
        // Past the height it all leaves the screen, however far it goes.
        if scrolled > 0 {
            self.scroll(0, scrolled.min(u64::from(self.height)) as i32);
        }
        let (color, attr) = (self.color, self.attr);
        let mut pen = start;
        for &code in text {
            if let Some((x, line)) = step(&mut pen, code)
                && line >= scrolled
            {
                let y = (line - scrolled) as u32;
                self.set((y * width + x) as usize, Cell { code, color, attr });
            }
        }
        self.cursor = (end.0, (end.1 - scrolled) as u32);
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

    /// Plays one frame of the animations, and gives whether the screen
    /// shows differently.
    pub fn animate(&mut self) -> bool {
        if self.anim_stopped || self.anims.iter().all(Option::is_none) {
            return false;
        }
        // What TANIM can change besides TVAR's variable.
        let place = |t: &TextScreen| (t.pos, t.z, t.rot, t.scale, t.tint);
        let before = place(self);
        let mut tracks = std::mem::take(&mut self.anims);
        for slot in &mut tracks {
            if let Some(track) = slot
                && !track.step(self)
            {
                *slot = None;
            }
        }
        self.anims = tracks;
        place(self) != before
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
        self.draw_placed(fb, font, font.pages(), &Affine::IDENTITY, None, None);
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
    /// INPUT cursor on top if `cursor` is given.
    pub fn draw_placed(
        &self,
        fb: &mut Framebuffer,
        font: &Font,
        grp: &Graphics,
        layer: &Affine,
        clip: Option<&Clip>,
        cursor: Option<LineCursor>,
    ) {
        if !self.visible {
            return;
        }
        let cs = self.char_size;
        if cursor.is_none() && !self.may_invert && self.cells.iter().all(|c| c.code == 0) {
            return;
        }
        let place = Placement {
            matrix: self.placement().then(layer),
            clip,
            add: self.add,
        };
        let (w, h) = ((self.width * cs) as i32, (self.height * cs) as i32);
        if place.is_upright() {
            self.draw_upright(fb, font, grp, &place);
        } else {
            self.draw_turned(fb, font, grp, &place);
        }
        let (cx, cy) = self.cursor;
        if let Some(line) = cursor.filter(|_| cx < self.width && cy < self.height) {
            let cell = (cx * cs) as i32;
            let part = (
                cell,
                (cy * cs) as i32,
                cell + cs as i32,
                ((cy + 1) * cs) as i32,
            );
            // UNVERIFIED: for the bar on other than 8-pixel cells, expected of SmileBASIC 4.
            let width = if line.overwrite { cs } else { cs >> 2 };
            let (x0, x1) = (f64::from(part.0), f64::from(part.0) + f64::from(width));
            let paint = Mixed::Solid(if line.composed { 0x00FF_FFFF } else { WHITE });
            let (y0, y1) = (f64::from(part.1), f64::from(part.3));
            draw_image_part(fb, &place, w, h, part, |x, y| {
                (x >= x0 && x < x1 && y >= y0 && y < y1).then_some(paint)
            });
        }
    }

    /// draw_placed for a screen turned or sheared on the frame, cell by
    /// cell.
    fn draw_turned(&self, fb: &mut Framebuffer, font: &Font, grp: &Graphics, place: &Placement) {
        let (cs, ft) = (self.char_size, self.font_type);
        let (w, h) = ((self.width * cs) as i32, (self.height * cs) as i32);
        let size = f64::from(cs);
        let (tx, ty) = place.texel_tie((cs as i32, cs as i32), (false, false));
        // The cell's character in its colors, looked up once rather than
        // for every pixel it covers.
        let mut glyph = vec![0; (ft * ft) as usize];
        let mut paints = Vec::new();
        for cy in 0..self.height {
            for cx in 0..self.width {
                let cell = self.cell(cx, cy);
                // Empty cells show nothing, so need no drawing.
                if cell.code == 0 && (!self.may_invert || cell.attr & attr::INVERT == 0) {
                    continue;
                }
                paints.clear();
                self.glyph(font, grp, cell, &mut glyph, &mut paints);
                let (x0, y0) = ((cx * cs) as i32, (cy * cs) as i32);
                let part = (x0, y0, x0 + cs as i32, y0 + cs as i32);
                draw_image_part(fb, place, w, h, part, |x, y| {
                    // Pixels of the cells around are theirs to draw.
                    if ((x / size) as u32, (y / size) as u32) != (cx, cy) {
                        return None;
                    }
                    // Where in the cell, which a fine frame splits further.
                    // As for sprites, the character is flipped, then turned
                    // (checked on hardware): undo the turn, then the flips.
                    let (px, py) = (
                        (x + tx - f64::from(cx * cs)).max(0.0),
                        (y + ty - f64::from(cy * cs)).max(0.0),
                    );
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
                    paint_at(glyph[(at(py) * ft + at(px)) as usize], &paints)
                });
            }
        }
    }
}

impl TextScreen {
    /// A cell's character in its colors, `ft` x `ft`, as draw_placed
    /// works it out: what each pixel puts on the frame, as [`glyph_paint`]
    /// gives it, with the paints that are not opaque pushed onto `paints`.
    fn glyph(
        &self,
        font: &Font,
        grp: &Graphics,
        cell: Cell,
        out: &mut [u32],
        paints: &mut Vec<Mixed>,
    ) {
        let ft = self.font_type;
        let inverted = self.inverts && cell.attr & attr::INVERT != 0;
        let mut color = |p: u32| {
            let p = if inverted { inverted_texel(p) } else { p };
            let paint = Mixed::new(p, cell.color, self.tint, self.add);
            glyph_paint(paint, paints)
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
            // An inverted one shows SmileBASIC 4's font.map entry for it,
            // which the bundled font lacks.
            (0, _) if !inverted || !font.is_smilebasic() => fill(&|_, _| 0),
            (code, _) if USER_CHARS.contains(&code) => {
                fill(&|x, y| self.glyph_pixel(font, grp, code, x, y))
            }
            (code, Some(size)) => match font.origin(code, size) {
                Some((gx, gy)) => {
                    fill(&|x, y| grp.pixel(FONT_PAGE, (gx + x) as i32, (gy + y) as i32))
                }
                None => fill(&|x, y| font.pixel(code, size, x, y)),
            },
            // Larger font types show only user-defined characters, and
            // the font page's corner to what is inverted.
            // UNVERIFIED: that corner, read from the text drawing.
            (_, None) if inverted => fill(&|x, y| grp.pixel(FONT_PAGE, x as i32, y as i32)),
            (_, None) => fill(&|_, _| 0),
        }
    }

    /// draw_placed for a screen neither turned nor sheared on the frame:
    /// which cell and which point of it each frame column and row shows
    /// is worked out once, and each row of cells' characters once.
    fn draw_upright(&self, fb: &mut Framebuffer, font: &Font, grp: &Graphics, place: &Placement) {
        let (cs, ft) = (self.char_size, self.font_type);
        let (w, h) = ((self.width * cs) as i32, (self.height * cs) as i32);
        let Some(grid) = crate::compose::grid(fb, place, w, h) else {
            return;
        };
        let size = f64::from(cs);
        let at = |p: f64| ((p * f64::from(ft) / size) as u32).min(ft - 1);
        let (tx, ty) = place.texel_tie((cs as i32, cs as i32), (false, false));
        // For each column (or row): its cell, and the glyph column (or
        // row) for the point in the cell, for the point flipped, and for
        // it flipped twice, as draw_placed's arithmetic gives them.
        let axis = |v: f64, tie: f64| {
            let c = (v / size) as u32;
            let p = (v + tie - f64::from(c * cs)).max(0.0);
            (c, [at(p), at(size - p), at(size - (size - p))])
        };
        let cols: Vec<(u32, [u32; 3])> = grid.xs.iter().map(|&x| axis(x, tx)).collect();
        // Which of those each glyph coordinate comes from, by ATTR: the
        // column's (false) or the row's (true) table, and which entry.
        let pick = |a: u16| -> ((bool, usize), (bool, usize)) {
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
        let mut paints: Vec<Mixed> = Vec::new();
        let mut made: HashMap<(u16, u32), usize> = HashMap::new();
        let mut made_inverted: HashMap<(u16, u32), usize> = HashMap::new();
        let may_invert = self.may_invert;
        // Per cell of the row, if it shows anything: its picks and where
        // its character is in `glyphs`.
        let mut shown = vec![None; self.width as usize];
        let mut row_of = None;
        let width = fb.width() as usize;
        let pixels = fb.pixels_mut();
        for (fy, &y) in (grid.y0 as usize..).zip(&grid.ys) {
            let (cy, rat) = axis(y, ty);
            if row_of != Some(cy) {
                row_of = Some(cy);
                for cx in 0..self.width {
                    let cell = self.cell(cx, cy);
                    let shows = cell.code != 0 || (may_invert && cell.attr & attr::INVERT != 0);
                    shown[cx as usize] = shows.then(|| {
                        let key = (cell.code, cell.color);
                        let made = match cell.attr & attr::INVERT {
                            0 => &mut made,
                            _ => &mut made_inverted,
                        };
                        let at = *made.entry(key).or_insert_with(|| {
                            let at = glyphs.len();
                            glyphs.resize(at + glyph_len, 0);
                            let out = &mut glyphs[at..];
                            self.glyph(font, grp, cell, out, &mut paints);
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
                match glyphs[at + (coord(q) * ft + coord(p)) as usize] {
                    0 => {}
                    solid if solid >> 24 == 0xFF => *dst = solid,
                    n => *dst = paints[n as usize - 1].put(*dst),
                }
            }
        }
    }
}

/// Text screens have no image to animate: "UV" and "I" do nothing.
/// Z and the color take the value rounded towards zero, as for sprites.
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

/// The texel an inverted cell draws with for a glyph's `p`, multiplied by
/// the cell's color and TCOLOR as any other: SmileBASIC 4 takes the
/// colors times `1 - alpha` in place of the glyph's, and then multiplies
/// the colour by alpha as always, which is why the light is left squared.
// UNVERIFIED: the sum is read from the console's program, not yet seen.
fn inverted_texel(p: u32) -> u32 {
    (255 - (p >> 24)) * 0x0101_0101
}

/// What a glyph pixel puts on the frame, as one `u32`: 0 for nothing, an
/// opaque color for itself, or else the place of its paint in `paints`
/// plus one, which is never opaque.
fn glyph_paint(paint: Option<Mixed>, paints: &mut Vec<Mixed>) -> u32 {
    match paint {
        None => 0,
        Some(Mixed::Solid(c)) => c,
        Some(paint) => {
            paints.push(paint);
            // Memory runs out long before this reaches &HFF000000.
            paints.len() as u32
        }
    }
}

/// Back from [`glyph_paint`].
fn paint_at(v: u32, paints: &[Mixed]) -> Option<Mixed> {
    match v {
        0 => None,
        solid if solid >> 24 == 0xFF => Some(Mixed::Solid(solid)),
        n => Some(paints[n as usize - 1]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    /// Moving cells in place against the save and load that TARRAY's array
    /// still needs.
    #[test]
    fn scroll_in_place_moves_as_save_and_load_do() {
        let (w, h) = (7, 5);
        let filled = || {
            let mut s = TextScreen::new(8 * w, 8 * h);
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    let code = (0x41 + y * w as i32 + x) as u16;
                    s.put_at(x as u32, y as u32, code, 0xFF00_0000 | code as u32);
                }
            }
            s
        };
        for dy in [i32::MIN, -6, -5, -4, -1, 0, 1, 4, 5, 6, i32::MAX] {
            for dx in [i32::MIN, -8, -7, -6, -1, 0, 1, 6, 7, 8, i32::MAX] {
                let mut slow = filled();
                slow.array();
                slow.scroll(dx, dy);
                let mut fast = filled();
                fast.scroll(dx, dy);
                assert!(slow.cells == fast.cells, "{dx},{dy}");
            }
        }
    }

    #[test]
    fn scrolling_counts_as_a_change_only_if_cells_differ() {
        let mut s = TextScreen::new(80, 40);
        s.take_changed();
        s.scroll(1, 1);
        assert!(!s.take_changed());
        s.put_at(0, 0, 0x41, !0);
        s.take_changed();
        s.scroll(0, 0);
        assert!(!s.take_changed());
        s.scroll(-1, 0);
        assert!(s.take_changed());
        assert_eq!(s.cell(1, 0).code, 0x41);
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
    fn cells_keep_all_of_their_attribute() {
        let cell = Cell::from_ints([0xFFFF_0041_u32 as i32, 0x1234]);
        assert_eq!((cell.code, cell.attr, cell.color), (0x41, 0xFFFF, 0x1234));
        assert_eq!(cell.to_ints(), [0xFFFF_0041_u32 as i32, 0x1234]);
    }

    #[test]
    fn inverted_cells_fill_around_the_glyph() {
        let font = Font::fallback();
        let mut screen = TextScreen::new(32, 8);
        screen.inverts = true;
        let code = u16::from(b'A');
        let (color, invert) = (0xFF40_80C0, attr::INVERT);
        screen.set_cell(
            0,
            0,
            Cell {
                code,
                color,
                attr: 0,
            },
        );
        screen.set_cell(
            1,
            0,
            Cell {
                code,
                color,
                attr: invert,
            },
        );
        // An empty cell and a space are filled whole.
        screen.set_cell(
            2,
            0,
            Cell {
                code: 0,
                color,
                attr: invert,
            },
        );
        screen.set_cell(
            3,
            0,
            Cell {
                code: 32,
                color,
                attr: invert,
            },
        );
        const BACK: u32 = 0xFF10_2030;
        let mut fb = Framebuffer::new(32, 8);
        fb.clear(BACK);
        screen.draw(&mut fb, &font);
        for y in 0..8u32 {
            for x in 0..8u32 {
                let at = |cx: u32| fb.pixels()[(y * 32 + x + cx * 8) as usize];
                let ink = font.pixel(code, FontSize::Small, x, y) >> 24 != 0;
                assert_eq!(at(0), if ink { color } else { BACK });
                assert_eq!(at(1), if ink { BACK } else { color });
                assert_eq!((at(2), at(3)), (color, color));
            }
        }
    }

    /// TEST56: on screens 0 to 3 the bit stays in the cell and the glyph is
    /// drawn as if it were not there.
    #[test]
    fn other_screens_keep_the_inverting_bit_but_show_a_plain_glyph() {
        let font = Font::fallback();
        let (code, color) = (u16::from(b'A'), 0xFF40_80C0);
        let drawn = |attr: u16| {
            let mut screen = TextScreen::new(16, 8);
            let cell = |code| Cell { code, color, attr };
            screen.set_cell(0, 0, cell(code));
            screen.set_cell(1, 0, cell(0));
            assert_eq!(screen.cell(0, 0).attr, attr);
            let mut fb = Framebuffer::new(16, 8);
            fb.clear(0xFF10_2030);
            screen.draw(&mut fb, &font);
            fb.pixels().to_vec()
        };
        assert_eq!(drawn(attr::INVERT), drawn(0));
    }

    /// The exact sum, one rounding: `(c*n)*(a*n) + d*(1 - a*n)` with n the
    /// glyph's 1 - alpha, tints unrounded.
    #[test]
    fn inverted_texels_keep_the_exact_sum() {
        let n = 0x80_u32;
        let texel = inverted_texel(((255 - n) << 24) | 0x12_3456);
        assert_eq!(texel, 0x8080_8080);
        let (c, ca, tc, d) = (0xC0_u32, 0x90_u32, 0xE0_u32, 0x40_u32);
        let paint = Mixed::new(
            texel,
            (ca << 24) | (c * 0x1_0101),
            0xFF00_0000 | (tc * 0x1_0101),
            false,
        );
        let got = paint.unwrap().put(0xFF00_0000 | (d * 0x1_0101)) & 0xFF;
        let f = |v: u32| f64::from(v) / 255.0;
        let a = f(ca) * f(n);
        let want = f(c) * f(n) * f(tc) * a + f(d) * (1.0 - a);
        assert_eq!(got, (want * 255.0).round() as u32);
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
        let _ = s.locate(Some(0), Some(0));
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
        assert!(s.locate(Some(9), None).is_ok());
        assert_eq!(s.cursor(), (9, 0));
        // Just past the right edge is allowed; beyond it is not.
        assert!(s.locate(Some(10), Some(0)).is_ok());
        assert_eq!(s.locate(Some(11), Some(0)), Err(0));
        assert_eq!(s.locate(None, Some(-1)), Err(1));
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

    #[test]
    fn long_prints_match_printing_each_character() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let array_of = |s: &TextScreen| match s.array.as_ref().map(|a| a.borrow().data.clone()) {
            Some(ArrayData::Int(v)) => v,
            _ => Vec::new(),
        };
        for _ in 0..4000 {
            let (w, h) = (1 + next(7) as u32, 1 + next(6) as u32);
            // Something to scroll away.
            let cells: Vec<(u32, u32, u16)> = (0..next(12))
                .map(|_| (next(8) as u32, next(7) as u32, 33 + next(9) as u16))
                .collect();
            // At the end of a line too, past the last column.
            let cursor = (next(u64::from(w) + 1) as u32, next(u64::from(h)) as u32);
            let screen = || {
                let mut s = TextScreen::new(8, 8);
                s.set_font(8, 8, w, h);
                s.set_color(0xFF00_FF00);
                for &(x, y, code) in &cells {
                    s.put_at(x, y, code, 7);
                }
                s.cursor = cursor;
                s
            };
            let (mut a, mut b) = (screen(), screen());
            if next(2) == 0 {
                // Written to the array alone: only a scroll brings the
                // cells back over it.
                let (ta, tb) = (a.array(), b.array());
                for t in [ta, tb] {
                    if let ArrayData::Int(v) = &mut t.borrow_mut().data {
                        v[0] = 1234;
                    }
                }
            }
            a.take_changed();
            b.take_changed();
            let newlines = 1 + next(5);
            let text: Vec<u16> = (0..next(60))
                .map(|_| match next(newlines + 5) {
                    0 => 10,
                    1 => 13,
                    _ => 65 + next(20) as u16,
                })
                .collect();
            a.print_each(&text);
            b.print_long(&text);
            assert_eq!(a.cells, b.cells, "{w}x{h} {text:?}");
            assert_eq!(a.cursor, b.cursor, "{w}x{h} {text:?}");
            assert_eq!(array_of(&a), array_of(&b), "{w}x{h} {text:?}");
            assert_eq!(a.take_changed(), b.take_changed(), "{w}x{h} {text:?}");
        }
    }

    #[test]
    fn a_very_long_print_finishes() {
        let mut s = TextScreen::new(8, 8);
        s.set_font(8, 8, 256, 128);
        // One scroll for each of these would take a few minutes in all.
        s.print(&vec![10; 10_000_000]);
        s.print(&text("A"));
        assert_eq!(s.cursor(), (1, 127));
        assert_eq!(row(&s, 127), "A");
        s.set_font(8, 8, 1, 32768);
        let mut long = vec![65; 10_000_000];
        long.push(66);
        s.print(&long);
        assert_eq!(s.cursor(), (1, 32767));
        assert_eq!(row(&s, 32767), "B");
        assert_eq!(row(&s, 32766), "A");
    }

    #[test]
    fn scrolling_by_the_largest_shifts_clears() {
        for (dx, dy) in [
            (0, i32::MAX),
            (i32::MAX, 0),
            (i32::MAX, i32::MAX),
            (0, i32::MIN),
            (i32::MIN, 0),
            (32, 0),
            (0, 24),
        ] {
            let mut s = TextScreen::new(32, 24);
            s.print(&text("ABCD"));
            s.scroll(dx, dy);
            assert_eq!(row(&s, 0), "", "{dx},{dy}");
        }
    }
}

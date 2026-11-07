//! The program editor that LIST and F4 open. It follows SmileBASIC 4's
//! editor loosely: line numbers on the left, a column ruler and the slot's
//! name at the bottom, and long lines either scrolled or wrapped. Text is
//! selected with Shift and the cursor keys or by dragging the mouse.
//!
//! The code is colored by [`crate::syntax`] in the settings' palette, in
//! the 8- or 16-pixel font at one of three sizes.

use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::complete::{self, Completion, Item, Library, Options};
use crate::extra_glyphs::{ExtraGlyphs, FontSource, Fonts, Glyph, Shown};
use crate::font::{Font, FontSize};
use crate::framebuffer::Framebuffer;
use crate::half_width::{HalfGlyphs, is_half_width};
use crate::hint;
use crate::palette::{Palette, Part};
use crate::runtime::{SLOTS, keys};
use crate::syntax::{definition, highlight, is_ident_char};
use crate::text::TextScreen;

/// Six digits on three number tiles (U+E100-E16E) and a thin cap.
const TILE_GUTTER: usize = 4;
/// Rows below the code: the ruler and the status line.
const FOOTER: usize = 2;
/// Closes the number tiles on the right.
const TILE_CAP: u16 = 0xE17F;

// The ruler and the status line keep their colors whatever the palette.
const TEXT: u32 = 0xFFFF_FFFF;
const BLACK: u32 = 0xFF00_0000;
const BAR: u32 = 0xFF30_3040;
const DIM: u32 = 0xFF70_7080;
const LINE_NUMBER: u32 = 0xFF80_8080;
const RETURN_MARK: u32 = 0xFF50_5868;
/// A breakpoint's line number, and the cursor's.
const BREAKPOINT: u32 = 0xFFDC_2828;
const BREAKPOINT_CURSOR: u32 = 0xFFFF_7070;
/// A breakpoint with a condition, and the cursor's.
const CONDITIONAL: u32 = 0xFFE0_7818;
const CONDITIONAL_CURSOR: u32 = 0xFFFF_A860;
/// The stopped line's background, or a grey one when edits moved the
/// lines since, taking this much out of 256.
const STOPPED: u32 = 0xFFFF_DC00;
const STOPPED_LINE: u32 = 100;

/// What the debugger shows of the slot shown, by 0-based line.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Marks {
    pub breakpoints: Vec<usize>,
    /// Those of the breakpoints that have a condition.
    pub conditional: Vec<usize>,
    /// The line the stopped program goes on from.
    pub stopped: Option<usize>,
    /// The program runs code from before the text was edited, so the
    /// stopped line is where it was then.
    pub stale: bool,
}

/// The mark drawn after each line's last character.
const RETURN_GLYPH: [u8; 8] = [
    0b0000_0000,
    0b0000_0010,
    0b0000_0010,
    0b0010_0010,
    0b0110_0010,
    0b1111_1110,
    0b0110_0000,
    0b0010_0000,
];

/// How large the editor's characters are. SmileBASIC 4 steps through its
/// sizes with F2 and Ctrl+B; Tiny is the emulator's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorSize {
    Large,
    Medium,
    Small,
    Tiny,
}

impl EditorSize {
    /// The size after this one, back to Large after Tiny.
    pub fn next(self) -> EditorSize {
        match self {
            EditorSize::Large => EditorSize::Medium,
            EditorSize::Medium => EditorSize::Small,
            EditorSize::Small => EditorSize::Tiny,
            EditorSize::Tiny => EditorSize::Large,
        }
    }

    /// Lines of code shown. SmileBASIC 4 shows 21, 28 and 43 whatever the
    /// font (checked on hardware); Tiny fills about 60 rows in all.
    fn code_rows(self) -> u32 {
        match self {
            EditorSize::Large => 21,
            EditorSize::Medium => 28,
            EditorSize::Small => 43,
            EditorSize::Tiny => 57,
        }
    }
}

/// The part of the display's height the menu bar takes: 24 of 720 pixels.
const MENU_SHARE: u32 = 30;

/// The pixels at the top of a frame `height` pixels high that the menu bar
/// covers once the frame fills the display's height.
pub fn menu_height(height: u32) -> u32 {
    height.div_ceil(MENU_SHARE)
}

/// The size of the frame the editor draws for a screen of `width` x
/// `height` pixels: room for the menu bar, then the code, the ruler and
/// the status line in whole cells of the font, at the screen's shape. It
/// is shown where the screen is, scaled to fit.
pub fn frame_size((width, height): (u32, u32), font: FontSize, size: EditorSize) -> (u32, u32) {
    let rows = (size.code_rows() + FOOTER as u32) * font as u32;
    // The menu bar's share of the whole frame, rows and all.
    let h = (rows * MENU_SHARE).div_ceil(MENU_SHARE - 1);
    (
        (f64::from(h) * f64::from(width) / f64::from(height)).round() as u32,
        h,
    )
}

/// How the editor is drawn, from the settings.
#[derive(Clone, Copy, Debug)]
pub struct Look<'a> {
    /// Long lines wrap instead of scrolling sideways.
    pub wrap: bool,
    /// Line numbers are drawn on number tiles.
    pub tiles: bool,
    /// An overview of the program is drawn behind the code's right side.
    pub map: bool,
    /// Pixels left at the top for the menu bar; `None` for its share of
    /// the frame.
    pub top: Option<u32>,
    /// Pixels left at the bottom, for the debugger's panel.
    pub bottom: u32,
    pub font: FontSize,
    pub colors: &'a Palette,
    /// Glyphs for the characters `font` lacks.
    pub extra: &'a ExtraGlyphs,
    /// The source characters are looked for in first.
    pub first: FontSource,
    /// Half-width characters are drawn half a cell wide. Each character
    /// still takes one column.
    pub half_width: bool,
    /// The half-width glyphs, if not the font's own.
    pub halves: Option<&'a HalfGlyphs>,
}

impl<'a> Look<'a> {
    /// The sources the editor draws characters with.
    fn fonts(&self, font: &'a Font) -> Fonts<'a> {
        Fonts {
            font,
            extra: Some(self.extra),
            first: self.first,
            size: self.font,
        }
    }
}

/// Where the editor's character cells are on its frame: `cell` pixels
/// square, starting `top` pixels down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Grid {
    cell: usize,
    top: usize,
}

impl Grid {
    /// The cell under a pixel, as a column and a row that may be off the
    /// grid.
    fn at(self, x: i32, y: i32) -> (i32, i32) {
        let cell = self.cell as i32;
        (x.div_euclid(cell), (y - self.top as i32).div_euclid(cell))
    }

    /// Fills `w` cells of a row from column `x`.
    fn fill(self, fb: &mut Framebuffer, row: usize, x: usize, w: usize, color: u32) {
        let (cell, top) = (self.cell, self.top);
        for py in top + row * cell..top + (row + 1) * cell {
            for px in x * cell..(x + w) * cell {
                fb.set(px as u32, py as u32, color);
            }
        }
    }

    /// Fills `w` pixels of a row from pixel `x`.
    fn fill_px(self, fb: &mut Framebuffer, row: usize, x: usize, w: usize, color: u32) {
        let (cell, top) = (self.cell, self.top);
        for py in top + row * cell..top + (row + 1) * cell {
            for px in x..x + w {
                fb.set(px as u32, py as u32, color);
            }
        }
    }

    /// Draws an 8x8 one-bit glyph over a cell from pixel `x` of row `y`,
    /// scaled to fill it.
    fn glyph(self, fb: &mut Framebuffer, x: usize, y: usize, glyph: &[u8; 8], color: u32) {
        let scale = self.cell / 8;
        for py in 0..self.cell {
            for px in 0..self.cell {
                if glyph[py / scale] & (0x80 >> (px / scale)) != 0 {
                    let (fx, fy) = (x + px, self.top + y * self.cell + py);
                    fb.set(fx as u32, fy as u32, color);
                }
            }
        }
    }
}

/// How wide the editor draws `line[col]` with half-width characters at
/// half a cell: columns past the end are as wide as a space.
#[inline]
fn half_cell_width(line: &[u16], col: usize, cell: usize) -> usize {
    match line.get(col) {
        Some(&c) if !is_half_width(c) => cell,
        _ => cell / 2,
    }
}

/// Pixels from the left of a row that starts at column `start` of `line`
/// to column `col`, with half-width characters at half a cell.
fn half_x(line: &[u16], start: usize, col: usize, cell: usize) -> usize {
    (start..col).map(|c| half_cell_width(line, c, cell)).sum()
}

/// The column of `line` drawn under pixel `x` of a row that starts at
/// column `start`, with half-width characters at half a cell.
fn half_col_at(line: &[u16], start: usize, x: i32, cell: usize) -> usize {
    let mut left = 0;
    let mut col = start;
    loop {
        let w = half_cell_width(line, col, cell) as i32;
        if x < left + w {
            return col;
        }
        left += w;
        col += 1;
    }
}

/// What the editor draws over its screen when half-width characters are
/// half a cell wide: those characters are not on the screen's grid.
#[derive(Clone, Copy, Debug)]
enum Over {
    /// A half-width glyph.
    Half(u16),
    /// The font's glyph.
    Font(u16),
    Extra(Glyph),
}

/// Draws the font's glyph for `code` in `color` with its top left at
/// `(x, y)`, as a text screen colors it.
fn draw_font_glyph(
    fb: &mut Framebuffer,
    font: &Font,
    code: u16,
    size: FontSize,
    at: (u32, u32),
    color: u32,
) {
    font.each_pixel(code, size, |px, py, p| {
        let a = p >> 24;
        let mul = |shift: u32| ((p >> shift & 0xFF) * (color >> shift & 0xFF) / 255) << shift;
        let tinted = mul(16) | mul(8) | mul(0);
        if let Some(under) = fb.get(at.0 + px, at.1 + py) {
            fb.set(at.0 + px, at.1 + py, blend(under, tinted, a + (a >> 7)));
        }
    });
}

/// Draws a half-width glyph, `cell / 2` by `cell` alpha bytes, in
/// `color` with its top left at `(x, y)`.
fn draw_half_glyph(fb: &mut Framebuffer, alpha: &[u8], cell: u32, at: (u32, u32), color: u32) {
    let w = cell / 2;
    for (i, &a) in alpha.iter().enumerate() {
        if a == 0 {
            continue;
        }
        let (px, py) = (at.0 + i as u32 % w, at.1 + i as u32 / w);
        if let Some(under) = fb.get(px, py) {
            let a = u32::from(a);
            fb.set(px, py, blend(under, color, a + (a >> 7)));
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct View {
    line: usize,
    col: usize,
    top: usize,
    left: usize,
}

/// Where the editor was, to go back to: see [`Editor::place`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place(View);

/// A line and a column in it.
pub type Pos = (usize, usize);

/// The overview's pixels per line, of which the characters fill the
/// first two, one pixel wide each.
const MAP_PITCH: usize = 3;
const MAP_BLOCK: usize = 2;
/// The overview is at most this wide, and a fifth of the frame.
const MAP_WIDTH: usize = 120;
/// How strongly the overview shows over the background, out of 256.
const MAP_ALPHA: u32 = 120;
/// How strongly the lines on screen are filled and outlined in the
/// overview, out of 256, in the text's color.
const VIEW_FILL: u32 = 40;
const VIEW_EDGE: u32 = 170;

/// Where the overview was drawn: from pixel `x` rightwards and from
/// pixel `top` down for `height`, starting with line `first`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Map {
    x: usize,
    top: usize,
    height: usize,
    first: usize,
}

/// Text revisions, taken by each change to an editor's text so that no
/// two texts, in any editor, share one.
static REVISIONS: AtomicU64 = AtomicU64::new(1);

/// How far, out of 256, the margins of a split display's side without the
/// keys fade toward the background.
const MARGIN_DIM: u32 = 160;

/// `over` mixed into `under` by `alpha` out of 256.
pub(crate) fn blend(under: u32, over: u32, alpha: u32) -> u32 {
    let mix = |shift: u32| {
        let (a, b) = ((under >> shift) & 0xFF, (over >> shift) & 0xFF);
        ((a * (256 - alpha) + b * alpha) / 256) << shift
    };
    0xFF00_0000 | mix(16) | mix(8) | mix(0)
}

/// Steps kept for Ctrl+Z.
const UNDO_STEPS: usize = 1000;

/// Text as the pieces between its line breaks; never empty.
type Lines = Vec<Vec<u16>>;

/// One change to the text: `removed` taken out at `at` and `inserted`
/// put there instead.
#[derive(Clone, Debug)]
struct Change {
    at: Pos,
    removed: Lines,
    inserted: Lines,
}

/// Changes that Ctrl+Z undoes together, and where the cursor was before
/// and after them.
#[derive(Clone, Debug)]
struct Step {
    changes: Vec<Change>,
    before: Pos,
    after: Pos,
}

/// Which edits join the last step rather than starting one: typing and
/// erasing go back a run at a time, until the cursor is moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Merge {
    Alone,
    Type,
    Erase,
}

/// What the editor's keys do: edit the program, or type into the status
/// line to go to a line, find or replace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Edit,
    GoTo,
    Find,
    Replace,
}

/// A character as searches compare it: ASCII letters in either case.
fn fold(c: u16) -> u16 {
    match c {
        0x61..=0x7A => c - 0x20,
        _ => c,
    }
}

/// The columns where `find` starts in `line`, not overlapping.
fn matches_in(line: &[u16], find: &[u16]) -> Vec<usize> {
    let mut at = Vec::new();
    if find.is_empty() {
        return at;
    }
    let mut col = 0;
    while col + find.len() <= line.len() {
        if line[col..col + find.len()]
            .iter()
            .zip(find)
            .all(|(&a, &b)| fold(a) == fold(b))
        {
            at.push(col);
            col += find.len();
        } else {
            col += 1;
        }
    }
    at
}

/// Where text ends that is put at `at`.
fn end_of(at: Pos, text: &Lines) -> Pos {
    let last = text.len() - 1;
    let col = text[last].len() + if last == 0 { at.1 } else { 0 };
    (at.0 + last, col)
}

pub struct Editor {
    slot: usize,
    lines: Vec<Vec<u16>>,
    /// Changes with the text, and with the other slots' names offered,
    /// for [`Editor::draw_key`].
    revision: u64,
    view: View,
    /// The column Up and Down return to after passing shorter lines.
    want_col: usize,
    /// Typing replaces the character under the cursor (Insert toggles it).
    pub overwrite: bool,
    /// Tab pads with spaces to the next multiple of this column.
    pub tab_step: usize,
    /// The other end of the selection; the cursor is one end.
    anchor: Option<Pos>,
    /// Line numbers are drawn on SmileBASIC 4's number tiles rather than
    /// in digits, which fonts other than its own may lack.
    number_tiles: bool,
    /// The line and first column shown on each row of the last frame.
    layout: Vec<Pos>,
    grid: Grid,
    modified: bool,
    /// The text changed since [`Editor::take_changed`] last looked.
    changed: bool,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// How the last edit may be joined by the next one.
    merge: Merge,
    mode: Mode,
    /// The line number typed after Ctrl+G.
    goto: Vec<u16>,
    /// What to find, and what to replace it with.
    find: Vec<u16>,
    with: Vec<u16>,
    /// Where typing what to find searches from.
    search_from: Pos,
    /// Shown in the status line until the next key, as "Not found".
    note: Option<String>,
    /// The mouse wheel scrolled away from the cursor, which stays where it
    /// is until it moves.
    scrolled: bool,
    /// The cursor when the view last followed it.
    followed: Pos,
    /// Completion's list for the word being typed.
    completion: Option<Completion>,
    /// Ctrl+Space hid the list and the hints until it is pressed again.
    quiet: bool,
    /// The overview in the last frame, and whether the mouse is dragging
    /// on it.
    map: Option<Map>,
    map_drag: bool,
    /// SmileBASIC's words and their forms, for completion and hints.
    pub library: Rc<Library>,
    /// The program's names, until the text changes.
    names: Option<Rc<Vec<Item>>>,
    /// The other slots' names, offered after the program's own.
    other_names: Option<Rc<Vec<Item>>>,
    /// What completion offers; `None` when it is off.
    pub complete: Option<Options>,
    /// Where each slot was left, restored when it is opened again.
    saved: [View; SLOTS],
    screen: TextScreen,
    /// Characters drawn from [`ExtraGlyphs`] over the screen, by column
    /// and row, kept to save allocating each frame.
    extra_cells: Vec<(usize, usize, Glyph, u32)>,
    /// With half-width characters at half a cell: what is drawn over the
    /// screen, by pixel x and row, kept to save allocating each frame.
    over: Vec<(usize, usize, Over, u32)>,
    /// The last frame drew half-width characters at half a cell.
    half: bool,
    /// How edits since they were last taken moved the lines, by slot.
    shifts: Vec<(usize, LineShift)>,
    /// Breakpoints and the stopped line, which the machine sets.
    pub marks: Marks,
    /// The next frame puts the cursor's line in the middle of the view
    /// if it is out of view.
    center: bool,
}

/// How an edit moved a program's lines, for what is kept by line, such
/// as breakpoints. Lines and columns count from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineShift {
    /// The text from `start` to `end`, on a later line, was taken out.
    Joined { start: Pos, end: Pos },
    /// `count` line breaks were put in at `at`.
    Broken { at: Pos, count: usize },
}

impl LineShift {
    /// Where a line is after the edit, or `None` once it is gone. Text
    /// taken out from a line's start leaves the line to the one whose
    /// rest moves there; text put in at a line's start pushes the line
    /// down.
    pub fn map(self, line: usize) -> Option<usize> {
        match self {
            LineShift::Joined { start, end } => {
                let from_start = start.1 == 0;
                if line < start.0 || (line == start.0 && !from_start) {
                    Some(line)
                } else if line == end.0 && from_start {
                    Some(start.0)
                } else if line > end.0 {
                    Some(line - (end.0 - start.0))
                } else {
                    None
                }
            }
            LineShift::Broken { at, count } => {
                if line < at.0 || (line == at.0 && at.1 > 0) {
                    Some(line)
                } else {
                    Some(line + count)
                }
            }
        }
    }
}

/// Splits a program into lines the way the lexer counts them.
pub(crate) fn split_lines(source: &str) -> Vec<Vec<u16>> {
    let text: Vec<u16> = source.encode_utf16().collect();
    let mut lines = vec![Vec::new()];
    let mut i = 0;
    while i < text.len() {
        match text[i] {
            13 if text.get(i + 1) == Some(&10) => {
                lines.push(Vec::new());
                i += 1;
            }
            10 | 13 => lines.push(Vec::new()),
            c => lines.last_mut().expect("never empty").push(c),
        }
        i += 1;
    }
    lines
}

impl Editor {
    /// An editor for a frame of `screen_w` x `screen_h` pixels in the
    /// 8-pixel font. [`Editor::draw`] takes on the frame and font it is
    /// given.
    pub fn new(screen_w: u32, screen_h: u32) -> Editor {
        let mut editor = Editor {
            slot: 0,
            lines: vec![Vec::new()],
            revision: 0,
            view: View::default(),
            want_col: 0,
            overwrite: false,
            tab_step: 1,
            anchor: None,
            number_tiles: true,
            layout: Vec::new(),
            grid: Grid { cell: 8, top: 0 },
            modified: false,
            changed: false,
            undo: Vec::new(),
            redo: Vec::new(),
            merge: Merge::Alone,
            mode: Mode::Edit,
            goto: Vec::new(),
            find: Vec::new(),
            with: Vec::new(),
            search_from: (0, 0),
            note: None,
            scrolled: false,
            followed: (0, 0),
            completion: None,
            quiet: false,
            library: Rc::default(),
            names: None,
            other_names: None,
            complete: None,
            map: None,
            map_drag: false,
            saved: [View::default(); SLOTS],
            screen: TextScreen::new(screen_w, screen_h),
            extra_cells: Vec::new(),
            over: Vec::new(),
            half: false,
            shifts: Vec::new(),
            marks: Marks::default(),
            center: false,
        };
        editor.fit(screen_w, screen_h, FontSize::Small, None);
        editor
    }

    /// Marks the text as changed for [`Editor::draw_key`].
    fn revise(&mut self) {
        self.revision = REVISIONS.fetch_add(1, Ordering::Relaxed);
    }

    /// Feeds `state` what [`Editor::draw`] draws from, besides what it is
    /// given: equal keys draw the same frame.
    pub fn draw_key(&self, state: &mut impl Hasher) {
        (self.revision, self.slot, self.modified).hash(state);
        (self.view, self.followed, self.scrolled, self.center).hash(state);
        (self.anchor, self.mode, self.overwrite).hash(state);
        (&self.goto, &self.find, &self.with, &self.note).hash(state);
        (&self.marks, &self.completion, self.quiet, self.complete).hash(state);
        Rc::as_ptr(&self.library).hash(state);
    }

    /// Lays the character cells out on a frame of `width` x `height`
    /// pixels, below the menu bar.
    fn fit(&mut self, width: u32, height: u32, font: FontSize, top: Option<u32>) {
        let cell = font as u32;
        let top = top.unwrap_or_else(|| menu_height(height));
        let size = (width / cell, (height - top) / cell);
        if self.grid.cell != cell as usize || self.screen.size() != size {
            self.screen.set_font(cell, cell, size.0, size.1);
        }
        self.screen.pos = (0.0, f64::from(top));
        self.grid = Grid {
            cell: cell as usize,
            top: top as usize,
        };
    }

    /// The slot being edited, or last edited.
    pub fn slot(&self) -> usize {
        self.slot
    }

    /// Whether the text changed since it was opened.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Whether the text changed since the last call, or since it was
    /// opened.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// The cursor as a 1-based line and 0-based column.
    pub fn cursor(&self) -> (usize, usize) {
        (self.view.line + 1, self.view.col)
    }

    /// The line the cursor is on, and the cursor's column in it.
    pub fn cursor_line(&self) -> (&[u16], usize) {
        (&self.lines[self.view.line], self.view.col)
    }

    pub fn text(&self) -> String {
        let lines: Vec<String> = self
            .lines
            .iter()
            .map(|l| String::from_utf16_lossy(l))
            .collect();
        lines.join("\n")
    }

    /// Starts editing `source` from `slot`, at a 1-based line and column
    /// (0 for the start of the line), or where the slot was left.
    pub fn open(&mut self, slot: usize, source: &str, at: Option<(usize, usize)>) {
        self.names = None;
        self.slot = slot;
        self.lines = split_lines(source);
        self.revise();
        self.modified = false;
        self.changed = false;
        self.view = self.saved[slot];
        if let Some((line, col)) = at {
            self.view.line = line.saturating_sub(1);
            self.view.col = col.saturating_sub(1);
        }
        self.view.line = self.view.line.min(self.lines.len() - 1);
        self.view.col = self.view.col.min(self.lines[self.view.line].len());
        self.want_col = self.view.col;
        self.anchor = None;
        self.layout.clear();
        self.undo.clear();
        self.redo.clear();
        self.merge = Merge::Alone;
        self.mode = Mode::Edit;
        self.note = None;
    }

    /// How the edits since the last call moved the lines, by slot, in
    /// order.
    pub fn take_shifts(&mut self) -> Vec<(usize, LineShift)> {
        std::mem::take(&mut self.shifts)
    }

    /// Puts the cursor at the start of a 1-based line, in the middle of
    /// the view if it has to scroll there.
    pub fn go_to(&mut self, line: usize) {
        self.anchor = None;
        self.go(line.saturating_sub(1), 0);
        self.want_col = 0;
        self.center = true;
    }

    /// Takes over the program and its undo history from the other side of
    /// a split display, which showed the same slot.
    pub fn take_document(&mut self, from: &mut Editor) {
        self.names = None;
        self.lines = from.lines.clone();
        self.revise();
        self.undo = std::mem::take(&mut from.undo);
        self.redo = std::mem::take(&mut from.redo);
        self.modified = from.modified;
        self.changed |= std::mem::take(&mut from.changed);
        self.merge = Merge::Alone;
        self.fit_cursor();
    }

    /// Shows the program that the other side of a split display edits.
    pub fn mirror(&mut self, from: &Editor) {
        if self.lines != from.lines {
            self.names = None;
            self.lines.clone_from(&from.lines);
            self.revise();
            self.fit_cursor();
        }
        self.modified = from.modified;
    }

    /// Keeps the cursor in the text after it changed under it.
    fn fit_cursor(&mut self) {
        let (line, col) = self.pos();
        self.go(line, col);
        self.anchor = None;
        self.view.top = self.view.top.min(self.lines.len() - 1);
    }

    /// Stops editing. Returns the new source if it was changed, so that an
    /// untouched program keeps its original line endings.
    pub fn close(&mut self) -> Option<String> {
        self.saved[self.slot] = self.view;
        std::mem::take(&mut self.modified).then(|| self.text())
    }

    /// Columns before the code: the line numbers and a space. Digits take
    /// as many as the last line's number needs.
    fn gutter(&self) -> usize {
        if self.number_tiles {
            TILE_GUTTER
        } else {
            self.lines.len().to_string().len() + 1
        }
    }

    fn size(&self) -> (usize, usize) {
        let (w, h) = self.screen.size();
        (
            (w as usize).saturating_sub(self.gutter()).max(1),
            (h as usize).saturating_sub(FOOTER).max(1),
        )
    }

    fn line(&self) -> &[u16] {
        &self.lines[self.view.line]
    }

    fn go(&mut self, line: usize, col: usize) {
        self.view.line = line.min(self.lines.len() - 1);
        self.view.col = col.min(self.line().len());
    }

    /// Moves to another line, keeping the wanted column.
    fn go_line(&mut self, line: usize) {
        self.go(line, self.want_col);
    }

    fn pos(&self) -> Pos {
        (self.view.line, self.view.col)
    }

    /// The selected text's start and end, if any is selected.
    fn selection(&self) -> Option<(Pos, Pos)> {
        let anchor = self.anchor?;
        let cursor = self.pos();
        (anchor != cursor).then(|| (anchor.min(cursor), anchor.max(cursor)))
    }

    /// Takes the text between two places out.
    fn take(&mut self, start: Pos, end: Pos) -> Lines {
        self.revise();
        if start.0 == end.0 {
            return vec![self.lines[start.0].drain(start.1..end.1).collect()];
        }
        (self.shifts).push((self.slot, LineShift::Joined { start, end }));
        let rest = self.lines[end.0].split_off(end.1);
        let mut taken = vec![self.lines[start.0].split_off(start.1)];
        taken.extend(self.lines.drain(start.0 + 1..=end.0));
        self.lines[start.0].extend(rest);
        taken
    }

    /// Puts text in at a place; returns where it ends.
    fn put(&mut self, at: Pos, text: &Lines) -> Pos {
        self.revise();
        if text.len() > 1 {
            let count = text.len() - 1;
            (self.shifts).push((self.slot, LineShift::Broken { at, count }));
        }
        let rest = self.lines[at.0].split_off(at.1);
        self.lines[at.0].extend_from_slice(&text[0]);
        self.lines
            .splice(at.0 + 1..at.0 + 1, text[1..].iter().cloned());
        let end = end_of(at, text);
        self.lines[end.0].extend(rest);
        end
    }

    /// Starts a step of edits for Ctrl+Z, or goes on with the last one if
    /// `merge` allows it.
    fn begin(&mut self, merge: Merge) {
        if merge == Merge::Alone || merge != self.merge || self.undo.is_empty() {
            let at = self.pos();
            self.undo.push(Step {
                changes: Vec::new(),
                before: at,
                after: at,
            });
            if self.undo.len() > UNDO_STEPS {
                self.undo.remove(0);
            }
        }
        self.merge = merge;
        self.redo.clear();
    }

    /// Replaces the text between two places, as part of the step that
    /// [`Editor::begin`] started. Returns where the new text ends.
    fn replace(&mut self, start: Pos, end: Pos, text: Lines) -> Pos {
        self.names = None;
        if self.undo.is_empty() {
            self.begin(Merge::Alone);
        }
        let removed = self.take(start, end);
        let after = self.put(start, &text);
        let step = self.undo.last_mut().expect("begun above");
        step.changes.push(Change {
            at: start,
            removed,
            inserted: text,
        });
        step.after = after;
        self.modified = true;
        self.changed = true;
        after
    }

    /// Takes back the last step of edits (Ctrl+Z).
    pub fn undo(&mut self) {
        self.names = None;
        while let Some(step) = self.undo.pop() {
            if step.changes.is_empty() {
                continue;
            }
            for c in step.changes.iter().rev() {
                self.take(c.at, end_of(c.at, &c.inserted));
                self.put(c.at, &c.removed);
            }
            self.moved_to(step.before);
            self.redo.push(step);
            self.modified = true;
            self.changed = true;
            break;
        }
    }

    /// Makes the last step undone again (Ctrl+Shift+Z).
    pub fn redo(&mut self) {
        self.names = None;
        if let Some(step) = self.redo.pop() {
            for c in &step.changes {
                self.take(c.at, end_of(c.at, &c.removed));
                self.put(c.at, &c.inserted);
            }
            self.moved_to(step.after);
            self.undo.push(step);
            self.modified = true;
            self.changed = true;
        }
    }

    /// Puts the cursor somewhere, dropping the selection; the next edit
    /// starts a step of its own.
    fn moved_to(&mut self, (line, col): Pos) {
        self.completion = None;
        self.go(line, col);
        self.anchor = None;
        self.want_col = self.view.col;
        self.merge = Merge::Alone;
    }

    /// Removes the selected text, if any, leaving the cursor where it was.
    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            return false;
        };
        self.replace(start, end, vec![Vec::new()]);
        self.go(start.0, start.1);
        self.anchor = None;
        true
    }

    /// The selected text, with lines ending in LF.
    pub fn copy(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        let mut text = Vec::new();
        for line in start.0..=end.0 {
            let l = &self.lines[line];
            let from = if line == start.0 { start.1 } else { 0 };
            let to = if line == end.0 { end.1 } else { l.len() };
            text.extend_from_slice(&l[from..to]);
            if line < end.0 {
                text.push(10);
            }
        }
        Some(String::from_utf16_lossy(&text))
    }

    /// Takes the selected text out, for the clipboard.
    pub fn cut(&mut self) -> Option<String> {
        let text = self.copy()?;
        self.begin(Merge::Alone);
        self.delete_selection();
        self.want_col = self.view.col;
        Some(text)
    }

    /// Puts text at the cursor in place of the selection. Insert mode
    /// does not apply.
    pub fn paste(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.begin(Merge::Alone);
        self.delete_selection();
        let at = self.pos();
        let (line, col) = self.replace(at, at, split_lines(text));
        self.moved_to((line, col));
    }

    /// Selects the whole program.
    pub fn select_all(&mut self) {
        self.file_end();
        self.anchor = Some((0, 0));
    }

    fn insert(&mut self, code: u16) {
        let (line, col) = self.pos();
        let end = if self.overwrite && col < self.line().len() {
            col + 1
        } else {
            col
        };
        self.replace((line, col), (line, end), vec![vec![code]]);
        self.view.col += 1;
    }

    fn split(&mut self) {
        let at = self.pos();
        self.begin(Merge::Alone);
        self.replace(at, at, vec![Vec::new(), Vec::new()]);
        self.go(at.0 + 1, 0);
    }

    /// The lines that line commands work on: those with selected text, or
    /// the cursor's.
    fn chosen_lines(&self) -> (usize, usize) {
        match self.selection() {
            // A selection ending at a line's start leaves that line out.
            Some((start, end)) if end.1 == 0 && end.0 > start.0 => (start.0, end.0 - 1),
            Some((start, end)) => (start.0, end.0),
            None => (self.view.line, self.view.line),
        }
    }

    /// Changes each chosen line in one step: `change` gives the columns
    /// to replace and what with, or `None` to leave the line alone. Whole
    /// lines stay selected if any were; the cursor otherwise stays on the
    /// same text.
    fn change_lines(&mut self, change: impl Fn(&[u16]) -> Option<(usize, usize, Vec<u16>)>) {
        let (first, last) = self.chosen_lines();
        let selected = self.selection().is_some();
        self.begin(Merge::Alone);
        let mut col = self.view.col;
        for line in first..=last {
            let Some((from, to, text)) = change(&self.lines[line]) else {
                continue;
            };
            if line == self.view.line && col > from {
                col = if col >= to {
                    col - (to - from) + text.len()
                } else {
                    from + text.len()
                };
            }
            self.replace((line, from), (line, to), vec![text]);
        }
        if selected {
            self.go(last, usize::MAX);
            self.anchor = Some((first, 0));
        } else {
            self.go(first, col);
        }
        self.want_col = self.view.col;
        self.merge = Merge::Alone;
    }

    /// Indents the chosen lines by TABSTEP's spaces (Tab over lines).
    // UNVERIFIED: SmileBASIC 4 indents by TABSTEP's spaces.
    pub fn indent(&mut self) {
        let spaces = vec![u16::from(b' '); self.tab_step.max(1)];
        self.change_lines(|_| Some((0, 0, spaces.clone())));
    }

    /// Takes up to TABSTEP's spaces from the start of the chosen lines
    /// (Shift+Tab).
    pub fn unindent(&mut self) {
        let step = self.tab_step.max(1);
        self.change_lines(|text| {
            let n = text.iter().take(step).take_while(|&&c| c == 32).count();
            (n > 0).then(|| (0, n, Vec::new()))
        });
    }

    /// Turns the chosen lines into comments (Ctrl+K): a quote at column
    /// 0, also before lines that are comments already.
    pub fn comment(&mut self) {
        self.change_lines(|_| Some((0, 0, vec![u16::from(b'\'')])));
    }

    /// Takes the first quote after the indentation off the chosen lines
    /// (Ctrl+Shift+K).
    pub fn uncomment(&mut self) {
        self.change_lines(|text| {
            let at = text.iter().position(|&c| c != 32)?;
            (text[at] == u16::from(b'\'')).then(|| (at, at + 1, Vec::new()))
        });
    }

    /// Puts an empty line below the cursor's and moves to it (Ctrl+Enter).
    pub fn insert_line(&mut self) {
        let line = self.view.line;
        let end = self.lines[line].len();
        self.anchor = None;
        self.begin(Merge::Alone);
        self.replace((line, end), (line, end), vec![Vec::new(), Vec::new()]);
        self.moved_to((line + 1, 0));
    }

    /// Removes the cursor's line, or the lines with selected text
    /// (Ctrl+Backspace).
    pub fn delete_line(&mut self) {
        let (first, last) = self.chosen_lines();
        self.anchor = None;
        self.begin(Merge::Alone);
        if last + 1 < self.lines.len() {
            self.replace((first, 0), (last + 1, 0), vec![Vec::new()]);
        } else if first > 0 {
            let len = self.lines[first - 1].len();
            self.replace(
                (first - 1, len),
                (last, self.lines[last].len()),
                vec![Vec::new()],
            );
        } else {
            self.replace((0, 0), (last, self.lines[last].len()), vec![Vec::new()]);
        }
        self.go_line(first);
        self.merge = Merge::Alone;
    }

    /// Removes the rest of the line from the cursor (Ctrl+Delete).
    pub fn delete_to_end(&mut self) {
        self.anchor = None;
        let (line, col) = self.pos();
        if col < self.line().len() {
            self.begin(Merge::Alone);
            self.replace((line, col), (line, self.line().len()), vec![Vec::new()]);
            self.merge = Merge::Alone;
        }
    }

    /// What the keys do now.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Starts typing a line number to go to, or stops (Ctrl+G).
    pub fn go_to_line(&mut self) {
        self.note = None;
        self.mode = if self.mode == Mode::GoTo {
            Mode::Edit
        } else {
            self.goto.clear();
            Mode::GoTo
        };
    }

    /// The text was saved as it is.
    pub fn mark_saved(&mut self) {
        self.modified = false;
    }

    /// Shows a word in the status line until the next key.
    pub fn set_note(&mut self, note: String) {
        self.note = Some(note);
    }

    /// Starts typing what to find (F3, Ctrl+F) or, with `replace`, what
    /// to replace it with (Ctrl+H). Text selected on one line becomes
    /// what to find.
    pub fn start_find(&mut self, replace: bool) {
        self.note = None;
        if let Some((start, end)) = self.selection()
            && start.0 == end.0
        {
            self.find = self.lines[start.0][start.1..end.1].to_vec();
        }
        self.search_from = self.selection().map_or(self.pos(), |(start, _)| start);
        self.mode = if replace { Mode::Replace } else { Mode::Find };
    }

    /// Selects the next place that holds what to find, after the cursor
    /// or, going back, before the selection, going round the program's
    /// ends. Returns whether there was one.
    pub fn find_next(&mut self, forward: bool) -> bool {
        let from = match (forward, self.selection()) {
            (false, Some((start, _))) => start,
            _ => self.pos(),
        };
        let found = self.search(from, forward, false);
        self.show_found(found)
    }

    /// The nearest match from `from`, going forward or back, `at` it too
    /// if `here`, around the ends.
    fn search(&self, from: Pos, forward: bool, here: bool) -> Option<Pos> {
        let n = self.lines.len();
        // One more line than there are, to come back to the first's start.
        for i in 0..=n {
            let line = if forward {
                (from.0 + i) % n
            } else {
                (from.0 + n * 2 - i) % n
            };
            let cols = matches_in(&self.lines[line], &self.find);
            let hit = if forward {
                cols.into_iter().find(|&c| {
                    i > 0 && i < n
                        || if i == 0 {
                            c > from.1 || here && c == from.1
                        } else {
                            c < from.1
                        }
                })
            } else {
                cols.into_iter()
                    .rev()
                    .find(|&c| i > 0 && i < n || if i == 0 { c < from.1 } else { c > from.1 })
            };
            if let Some(col) = hit {
                return Some((line, col));
            }
        }
        None
    }

    /// Selects a match, or notes that there was none.
    fn show_found(&mut self, found: Option<Pos>) -> bool {
        match found {
            Some((line, col)) => {
                self.moved_to((line, col + self.find.len()));
                self.anchor = Some((line, col));
                self.note = None;
                true
            }
            None => {
                self.anchor = None;
                self.note = (!self.find.is_empty()).then(|| "Not found".to_string());
                false
            }
        }
    }

    /// Whether the selection is a match.
    fn on_match(&self) -> bool {
        self.selection().is_some_and(|(start, end)| {
            start.0 == end.0
                && end.1 - start.1 == self.find.len()
                && matches_in(&self.lines[start.0][start.1..end.1], &self.find) == [0]
        })
    }

    /// Replaces the selected match and finds the next (Enter in replace
    /// mode), or finds one first.
    pub fn replace_one(&mut self) {
        if self.on_match() {
            let (start, end) = self.selection().expect("on a match");
            self.begin(Merge::Alone);
            let after = self.replace(start, end, vec![self.with.clone()]);
            self.moved_to(after);
        }
        self.find_next(true);
    }

    /// Replaces every match in one step (Ctrl+Enter in replace mode).
    pub fn replace_all(&mut self) {
        let mut count = 0;
        self.begin(Merge::Alone);
        for line in 0..self.lines.len() {
            // From the right, so that the columns found stay right.
            for col in matches_in(&self.lines[line], &self.find).into_iter().rev() {
                let end = (line, col + self.find.len());
                self.replace((line, col), end, vec![self.with.clone()]);
                count += 1;
            }
        }
        let at = self.pos();
        self.moved_to(at);
        self.note = Some(match count {
            0 => "Not found".to_string(),
            1 => "Replaced 1".to_string(),
            n => format!("Replaced {n}"),
        });
    }

    /// A key while typing in the status line.
    fn prompt_key(&mut self, code: u16) {
        self.note = None;
        let printable = !matches!(code, 0..0x20 | 0x7F);
        match (self.mode, code) {
            (_, keys::ESCAPE) => self.mode = Mode::Edit,
            (Mode::GoTo, keys::ENTER) => {
                let digits = String::from_utf16_lossy(&self.goto);
                if let Ok(line) = digits.parse::<usize>() {
                    self.moved_to((line.max(1) - 1, 0));
                }
                self.mode = Mode::Edit;
            }
            (Mode::GoTo, keys::BACKSPACE) => {
                self.goto.pop();
            }
            (Mode::GoTo, 0x30..=0x39) if self.goto.len() < 7 => self.goto.push(code),
            (Mode::GoTo, _) => {}
            (_, keys::DOWN) | (Mode::Find, keys::ENTER) => {
                self.find_next(true);
            }
            (_, keys::UP) => {
                self.find_next(false);
            }
            (Mode::Replace, keys::ENTER) => self.replace_one(),
            (Mode::Find, keys::TAB) => self.mode = Mode::Replace,
            (Mode::Replace, keys::TAB) => self.mode = Mode::Find,
            (Mode::Find, keys::BACKSPACE) => {
                self.find.pop();
                self.find_from_start();
            }
            (Mode::Replace, keys::BACKSPACE) => {
                self.with.pop();
            }
            (Mode::Find, c) if printable => {
                self.find.push(c);
                self.find_from_start();
            }
            (Mode::Replace, c) if printable => self.with.push(c),
            _ => {}
        }
    }

    /// Finds what is being typed from where typing began.
    fn find_from_start(&mut self) {
        let found = self.search(self.search_from, true, true);
        if self.show_found(found) {
            return;
        }
        let from = self.search_from;
        self.moved_to(from);
    }

    /// The matches in the program, and which of them is selected (from
    /// 1), for the status line.
    fn match_count(&self) -> (usize, Option<usize>) {
        let at = self
            .on_match()
            .then(|| self.selection().expect("on a match").0);
        let mut count = 0;
        let mut index = None;
        for (line, text) in self.lines.iter().enumerate() {
            for col in matches_in(text, &self.find) {
                count += 1;
                if at == Some((line, col)) {
                    index = Some(count);
                }
            }
        }
        (count, index)
    }

    /// Moves the cursor with a [`keys`] code while keeping the other end
    /// of the selection, as Shift does.
    pub fn key_select(&mut self, code: u16) {
        if self.mode != Mode::Edit {
            self.key(code);
            return;
        }
        let anchor = self.anchor.unwrap_or(self.pos());
        self.anchor = None;
        self.key(code);
        if is_movement(code) {
            self.anchor = Some(anchor);
        }
    }

    /// Edits with a typed character or one of the [`keys`] codes.
    pub fn key(&mut self, code: u16) {
        // Any key brings the cursor back into view.
        self.scrolled = false;
        if self.mode != Mode::Edit {
            self.prompt_key(code);
            return;
        }
        self.note = None;
        if let Some(list) = &mut self.completion {
            let enter = self.complete.is_some_and(|o| o.enter);
            match list.key(code, enter) {
                complete::Outcome::Used => return,
                complete::Outcome::Close => {
                    self.completion = None;
                    return;
                }
                complete::Outcome::Accept => {
                    self.accept_completion();
                    return;
                }
                complete::Outcome::Pass => {}
            }
        }
        let listing = self.completion.is_some();
        self.edit_key(code);
        // Typing a name, or erasing in it, looks for words again.
        let looks = match code {
            keys::BACKSPACE => listing,
            c => is_ident_char(c) || c == 0x40 || c == 0x23,
        };
        self.completion = None;
        if let (true, Some(options), false) = (looks, self.complete, self.quiet) {
            self.look_for_words(options);
        }
    }

    fn look_for_words(&mut self, options: Options) {
        if self.selection().is_some() {
            return;
        }
        let names = self.program_names();
        let (line, col) = self.pos();
        let text = &self.lines[line];
        self.completion = Completion::find(text, col, &options, &self.library, &names);
    }

    /// Ctrl+Space: hides completion's list and the hints, or shows them
    /// again with the list for the word at the cursor, from its first
    /// letter.
    pub fn toggle_popups(&mut self) {
        let Some(options) = self.complete else {
            return;
        };
        self.quiet = !self.quiet;
        self.completion = None;
        if self.quiet {
            self.note = Some("Hints hidden: Ctrl+Space".into());
        } else {
            self.note = None;
            self.look_for_words(Options {
                after: 1,
                ..options
            });
        }
    }

    /// The names the program uses, worked out again after a change, then
    /// those of the other slots if given.
    fn program_names(&mut self) -> Rc<Vec<Item>> {
        let (lines, others) = (&self.lines, &self.other_names);
        (self.names.get_or_insert_with(|| {
            let own = complete::names(lines.iter().map(Vec::as_slice));
            Rc::new(match others {
                Some(others) => complete::merge(own, others),
                None => own,
            })
        }))
        .clone()
    }

    /// Sets the other slots' names to offer too, or none.
    pub fn set_other_names(&mut self, names: Option<Rc<Vec<Item>>>) {
        let same = match (&self.other_names, &names) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (a, b) => a.is_none() && b.is_none(),
        };
        if !same {
            self.other_names = names;
            self.names = None;
            // The hints may name them.
            self.revise();
        }
    }

    /// Puts the chosen word of completion's list in place of the word
    /// being typed.
    fn accept_completion(&mut self) {
        let Some(list) = self.completion.take() else {
            return;
        };
        let (line, col) = self.pos();
        let word: Vec<u16> = list.chosen().encode_utf16().collect();
        self.begin(Merge::Alone);
        let end = self.replace((line, list.start), (line, col), vec![word]);
        self.moved_to(end);
    }

    fn edit_key(&mut self, code: u16) {
        // A click leaves the anchor at the cursor; once the text changes
        // it would no longer be a place in it.
        if self.selection().is_none() {
            self.anchor = None;
        }
        // Control codes other than these type nothing.
        let types = !matches!(code, 0..0x20 | 0x7F) || matches!(code, keys::ENTER | keys::TAB);
        if code == keys::TAB && self.selection().is_some_and(|(a, b)| a.0 != b.0) {
            self.indent();
            return;
        }
        if is_movement(code) {
            self.anchor = None;
            self.merge = Merge::Alone;
        } else if self.selection().is_some() {
            match code {
                keys::BACKSPACE | keys::DELETE => {
                    self.begin(Merge::Alone);
                    self.delete_selection();
                    self.want_col = self.view.col;
                    return;
                }
                keys::ENTER => {
                    self.begin(Merge::Alone);
                    self.delete_selection();
                }
                _ if types => {
                    self.begin(Merge::Type);
                    self.delete_selection();
                }
                _ => {}
            }
        }
        let (line, col) = self.pos();
        let (_, rows) = self.size();
        let keeps_column = matches!(
            code,
            keys::UP | keys::DOWN | keys::PAGE_UP | keys::PAGE_DOWN
        );
        match code {
            keys::ENTER => self.split(),
            keys::BACKSPACE if col > 0 => {
                self.begin(Merge::Erase);
                self.replace((line, col - 1), (line, col), vec![Vec::new()]);
                self.view.col -= 1;
            }
            keys::BACKSPACE if line > 0 => {
                self.begin(Merge::Erase);
                let len = self.lines[line - 1].len();
                self.replace((line - 1, len), (line, 0), vec![Vec::new()]);
                self.go(line - 1, len);
            }
            keys::DELETE if col < self.line().len() => {
                self.begin(Merge::Erase);
                self.replace((line, col), (line, col + 1), vec![Vec::new()]);
            }
            keys::DELETE if line + 1 < self.lines.len() => {
                self.begin(Merge::Erase);
                self.replace((line, col), (line + 1, 0), vec![Vec::new()]);
            }
            keys::LEFT if col > 0 => self.view.col -= 1,
            keys::LEFT if line > 0 => self.go(line - 1, usize::MAX),
            keys::RIGHT if col < self.line().len() => self.view.col += 1,
            keys::RIGHT if line + 1 < self.lines.len() => self.go(line + 1, 0),
            keys::UP => self.go_line(line.saturating_sub(1)),
            keys::DOWN => self.go_line(line + 1),
            keys::PAGE_UP => self.go_line(line.saturating_sub(rows)),
            keys::PAGE_DOWN => self.go_line(line + rows),
            keys::HOME => self.view.col = 0,
            keys::END => self.view.col = self.line().len(),
            // UNVERIFIED: Tab pads with spaces to the next TABSTEP column.
            keys::TAB => {
                self.begin(Merge::Type);
                let tab = self.tab_step.max(1);
                for _ in 0..tab - col % tab {
                    self.insert(u16::from(b' '));
                }
            }
            keys::INSERT => self.overwrite = !self.overwrite,
            _ if !types => {}
            c => {
                self.begin(Merge::Type);
                self.insert(c);
            }
        }
        if !keeps_column {
            self.want_col = self.view.col;
        }
    }

    /// The text position under a pixel of the editor's screen, from the
    /// last frame drawn. Rows above or below the code reach one line
    /// further, so that dragging there scrolls.
    fn hit(&self, x: i32, y: i32) -> Option<Pos> {
        let (first, last) = (*self.layout.first()?, *self.layout.last()?);
        let (col, row) = self.grid.at(x, y);
        let col = (col - self.gutter() as i32).max(0) as usize;
        // Wrapped lines start every row at column 0, where `left` is 0.
        let left = self.view.left;
        // The column under `x` on a row that starts at `start` of `line`.
        let col_in = |line: usize, start: usize| match self.half {
            false => start + col,
            true => {
                let text = self.lines.get(line).map_or(&[][..], Vec::as_slice);
                let x = (x - (self.gutter() * self.grid.cell) as i32).max(0);
                half_col_at(text, start, x, self.grid.cell)
            }
        };
        Some(if row < 0 {
            let line = first.0.saturating_sub(1);
            (line, col_in(line, left))
        } else if let Some(&(line, start)) = self.layout.get(row as usize) {
            (line, col_in(line, start))
        } else if self.layout.len() < self.size().1 {
            // Below the program's end.
            (last.0, usize::MAX)
        } else {
            (last.0 + 1, col_in(last.0 + 1, left))
        })
    }

    /// The 1-based line whose number is under a pixel of the editor's
    /// screen, from the last frame drawn.
    pub fn margin_line(&self, x: i32, y: i32) -> Option<usize> {
        let (col, row) = self.grid.at(x, y);
        if !(0..self.gutter() as i32).contains(&col) || row < 0 {
            return None;
        }
        let row = row as usize;
        let &(line, _) = self.layout.get(row)?;
        // A wrapped line has its number on its first row only.
        let first = row == 0 || self.layout[row - 1].0 != line;
        first.then_some(line + 1)
    }

    /// A press of the mouse at a pixel of the editor's screen: moves the
    /// cursor there and starts a selection, or with `extend` moves the
    /// selection's end there. Presses outside the code do nothing.
    pub fn mouse_down(&mut self, x: i32, y: i32, extend: bool) {
        self.map_drag = self.map.is_some_and(|m| {
            let (x, y) = (x as i64, y as i64);
            x >= m.x as i64 && y >= m.top as i64 && y < (m.top + m.height) as i64
        });
        if self.map_drag {
            self.map_scroll(y);
            return;
        }
        let row = self.grid.at(x, y).1;
        if row < 0 || row >= self.size().1 as i32 {
            return;
        }
        let Some((line, col)) = self.hit(x, y) else {
            return;
        };
        let from = self.pos();
        self.completion = None;
        self.go(line, col);
        self.want_col = self.view.col;
        self.merge = Merge::Alone;
        self.anchor = Some(if extend {
            self.anchor.unwrap_or(from)
        } else {
            self.pos()
        });
    }

    /// The mouse moved with its button held since [`Editor::mouse_down`].
    pub fn mouse_drag(&mut self, x: i32, y: i32) {
        if self.map_drag {
            self.map_scroll(y);
            return;
        }
        if let Some((line, col)) = self.hit(x, y) {
            self.go(line, col);
            self.want_col = self.view.col;
        }
    }

    /// Moves to the first line (Ctrl+Home, FILETOP).
    pub fn file_top(&mut self) {
        self.moved_to((0, 0));
    }

    /// Moves to the end of the last line (Ctrl+End, FILEEND).
    pub fn file_end(&mut self) {
        self.moved_to((usize::MAX, usize::MAX));
    }

    /// The lines that start with a label or a DEF, for jumping to.
    pub fn index(&self) -> Vec<(usize, String)> {
        (self.lines.iter().enumerate())
            .filter_map(|(i, line)| Some((i, definition(line)?)))
            .collect()
    }

    pub fn has_selection(&self) -> bool {
        self.selection().is_some()
    }

    /// The selected text if it is within a line, or else the name at the
    /// cursor with its `@` or suffix, for a watch or a definition.
    pub fn word_or_selection(&self) -> Option<String> {
        if let Some((start, end)) = self.selection() {
            return (start.0 == end.0)
                .then(|| String::from_utf16_lossy(&self.lines[start.0][start.1..end.1]));
        }
        let (line, col) = self.cursor_line();
        let part = |c: u16| is_ident_char(c) || c == u16::from(b'@');
        let suffix = |c: u16| matches!(c, 0x23..=0x25);
        let mut start = col.min(line.len());
        while start > 0 && part(line[start - 1]) {
            start -= 1;
        }
        let mut end = col.min(line.len());
        while end < line.len() && part(line[end]) {
            end += 1;
        }
        if end < line.len() && suffix(line[end]) {
            end += 1;
        }
        (start < end).then(|| String::from_utf16_lossy(&line[start..end]))
    }

    /// Goes to the line with a label or the DEF of a name; false when the
    /// program has none.
    pub fn go_to_definition(&mut self, name: &str) -> bool {
        let found = (self.index().into_iter()).find(|(_, d)| d.eq_ignore_ascii_case(name));
        let Some((line, _)) = found else {
            return false;
        };
        self.go_to(line + 1);
        true
    }

    /// Where the editor is now, for [`Editor::go_back`].
    pub fn place(&self) -> Place {
        Place(self.view)
    }

    pub fn go_back(&mut self, Place(view): Place) {
        self.moved_to((view.line, view.col));
        self.view.top = view.top.min(self.lines.len() - 1);
        self.view.left = view.left;
    }

    /// Moves to the start of a line (from 0), shown near the top.
    pub fn jump_to(&mut self, line: usize) {
        self.moved_to((line, 0));
        self.view.top = self.view.line.saturating_sub(2);
    }

    /// Scrolls to show the line under a pixel row of the overview in the
    /// middle, leaving the cursor where it is.
    fn map_scroll(&mut self, y: i32) {
        let Some(m) = self.map else {
            return;
        };
        let row = (y - m.top as i32).max(0) as usize / MAP_PITCH;
        let line = (m.first + row).min(self.lines.len() - 1);
        self.view.top = line.saturating_sub(self.size().1 / 2);
        self.scrolled = true;
    }

    /// Draws the overview of the program behind the right of the code:
    /// each character a pixel in its color, the lines shown lighter.
    fn draw_map(&mut self, fb: &mut Framebuffer, colors: &Palette, rows: usize) {
        let width = (fb.width() as usize / 5).min(MAP_WIDTH);
        let top = self.grid.top;
        let height = rows * self.grid.cell;
        let x0 = fb.width() as usize - width;
        let fit = height / MAP_PITCH;
        let total = self.lines.len();
        // A long program's overview moves with the view, in proportion.
        let first = match total > fit {
            true => self.view.top * (total - fit) / total.saturating_sub(rows).max(1),
            false => 0,
        }
        .min(total.saturating_sub(fit));
        self.map = Some(Map {
            x: x0,
            top,
            height,
            first,
        });
        let mut paint = |x: usize, y: usize, h: usize, color: u32, alpha: u32| {
            for py in y..(y + h).min(top + height) {
                let (px, py) = (x as u32, py as u32);
                if let Some(under) = fb.get(px, py) {
                    fb.set(px, py, blend(under, color, alpha));
                }
            }
        };
        // The lines on screen, filled and outlined so that the view
        // stands out of the code around it.
        let shown =
            self.view.top.saturating_sub(first)..(self.view.top + rows).saturating_sub(first);
        let y = top + shown.start.min(fit) * MAP_PITCH;
        let h = (shown.end.min(fit) - shown.start.min(fit)) * MAP_PITCH;
        let edge = colors[Part::Character];
        if h > 0 {
            for x in x0..x0 + width {
                paint(x, y, h, edge, VIEW_FILL);
                paint(x, y, 1, edge, VIEW_EDGE);
                paint(x, y + h - 1, 1, edge, VIEW_EDGE);
            }
            paint(x0, y + 1, h.saturating_sub(2), edge, VIEW_EDGE);
        }
        for (i, text) in self.lines[first..].iter().take(fit).enumerate() {
            let y = top + i * MAP_PITCH;
            let (roles, kind) = highlight(text);
            let back = colors.line_background(kind);
            if back != colors[Part::Background] {
                for x in x0..x0 + width {
                    paint(x, y, MAP_PITCH, back, MAP_ALPHA);
                }
            }
            for (col, &c) in text.iter().enumerate().take(width) {
                if c != 0x20 {
                    let color = colors[Part::from(roles[col])];
                    paint(x0 + col, y, MAP_BLOCK, color, MAP_ALPHA);
                }
            }
        }
    }

    /// Scrolls by lines without moving the cursor, as the mouse wheel does.
    pub fn wheel(&mut self, lines: i32) {
        let top = self.view.top as i64 + i64::from(lines);
        self.view.top = top.clamp(0, self.lines.len() as i64 - 1) as usize;
        self.scrolled = true;
    }

    /// Scrolls so that the cursor is on screen, unless the wheel scrolled
    /// away and the cursor has not moved since.
    fn scroll(&mut self, wrap: bool) {
        if self.scrolled && self.followed == self.pos() {
            self.view.top = self.view.top.min(self.lines.len() - 1);
            return;
        }
        self.scrolled = false;
        self.followed = self.pos();
        let (cols, rows) = self.size();
        let v = &mut self.view;
        v.top = v.top.min(v.line);
        if wrap {
            v.left = 0;
            let cursor_row = v.col / cols;
            loop {
                let above: usize = (v.top..v.line).map(|l| rows_of(&self.lines[l], cols)).sum();
                if above + cursor_row < rows || v.top == v.line {
                    break;
                }
                v.top += 1;
            }
        } else {
            if v.line >= v.top + rows {
                v.top = v.line + 1 - rows;
            }
            v.left = v.left.min(v.col);
            if v.col >= v.left + cols {
                v.left = v.col + 1 - cols;
            }
        }
    }

    /// Draws the editor over the whole frame. `name` is the slot's file
    /// name; `blink` shows the cursor in this frame.
    pub fn draw(&mut self, fb: &mut Framebuffer, font: &Font, name: &str, look: Look, blink: bool) {
        let Look {
            wrap,
            tiles,
            map,
            colors,
            ..
        } = look;
        self.number_tiles = tiles;
        // The menu bar's share is of the whole frame, whatever is left
        // below.
        let top = look.top.unwrap_or_else(|| menu_height(fb.height()));
        let height = fb.height().saturating_sub(look.bottom);
        self.fit(fb.width(), height, look.font, Some(top));
        let rows = self.size().1;
        let v = &mut self.view;
        if std::mem::take(&mut self.center) && !(v.top..v.top + rows).contains(&v.line) {
            v.top = v.line.saturating_sub(rows / 2);
        }
        self.scroll(wrap);
        let (cols, rows) = self.size();
        let gutter = self.gutter();
        let grid = self.grid;
        let v = self.view;
        let selection = self.selection();
        self.layout.clear();
        self.extra_cells.clear();
        self.over.clear();
        let half = look.half_width;
        self.half = half;
        let fonts = look.fonts(font);
        let cell = grid.cell;
        let (status_text, ruler_text) = self.prompt();
        let note = self.note.clone().or_else(|| self.mode_note());
        fb.clear(colors[Part::Background]);
        let screen = &mut self.screen;
        screen.cls();
        let width = screen.size().0 as usize;
        let mut marks = Vec::new();
        let mut cursor = None;

        let mut row = 0;
        let mut line = v.top;
        while row < rows && line < self.lines.len() {
            let text = &self.lines[line];
            let (roles, kind) = highlight(text);
            let hits = match self.mode {
                Mode::Find | Mode::Replace => matches_in(text, &self.find),
                _ => Vec::new(),
            };
            let current = line == v.line;
            let back = if current {
                colors[Part::CursorLine]
            } else {
                colors.line_background(kind)
            };
            let back = match self.marks.stopped == Some(line) {
                true if self.marks.stale => blend(back, DIM, STOPPED_LINE),
                true => blend(back, STOPPED, STOPPED_LINE),
                false => back,
            };
            let chunks = if wrap { rows_of(text, cols) } else { 1 };
            for chunk in 0..chunks {
                if row >= rows {
                    break;
                }
                if back != colors[Part::Background] {
                    grid.fill(fb, row, 0, width, back);
                }
                if chunk == 0 {
                    // A breakpoint turns the number's tiles red, or the
                    // digits' background; orange with a condition.
                    let breakpoint = self.marks.breakpoints.contains(&line);
                    let conditional = breakpoint && self.marks.conditional.contains(&line);
                    let color = match (breakpoint, current) {
                        (true, true) if conditional => CONDITIONAL_CURSOR,
                        (true, false) if conditional => CONDITIONAL,
                        (true, true) => BREAKPOINT_CURSOR,
                        (true, false) => BREAKPOINT,
                        (false, true) => colors[Part::Character],
                        (false, false) => LINE_NUMBER,
                    };
                    if tiles {
                        for (x, &c) in number_tiles(line + 1).iter().enumerate() {
                            screen.put_at(x as u32, row as u32, c, color);
                        }
                        screen.put_at(3, row as u32, TILE_CAP, color);
                    } else {
                        let number = format!("{:>1$}", line + 1, gutter - 1);
                        if breakpoint {
                            grid.fill(fb, row, 0, gutter - 1, color);
                        }
                        let digits = if breakpoint { TEXT } else { color };
                        let extras = &mut self.extra_cells;
                        put_str(screen, extras, &fonts, (0, row), &number, digits);
                    }
                }
                let start = if wrap { chunk * cols } else { v.left };
                self.layout.push((line, start));
                if half {
                    // Columns start where the characters before them end.
                    let mut px = gutter * cell;
                    for x in 0..cols {
                        let col = start + x;
                        let w = half_cell_width(text, col, cell);
                        if hits.iter().any(|&h| h <= col && col < h + self.find.len()) {
                            grid.fill_px(fb, row, px, w, colors[Part::Search]);
                        }
                        let at = (line, col);
                        if selection.is_some_and(|(a, b)| a <= at && at < b) && col <= text.len() {
                            grid.fill_px(fb, row, px, w, colors[Part::Selection]);
                        }
                        let Some(&c) = text.get(col) else {
                            if col == text.len() {
                                marks.push((px, row));
                            }
                            break;
                        };
                        let color = colors[Part::from(roles[col])];
                        let over = if is_half_width(c) {
                            Some(Over::Half(c))
                        } else {
                            match fonts.choose(text, col) {
                                Shown::Font => Some(Over::Font(c)),
                                Shown::Extra(glyph) => Some(Over::Extra(glyph)),
                                Shown::Blank => None,
                            }
                        };
                        if let Some(over) = over {
                            self.over.push((px, row, over, color));
                        }
                        px += w;
                    }
                    if current && v.col >= start && v.col - start < cols {
                        let x = gutter * cell + half_x(text, start, v.col, cell);
                        cursor = Some((x, row));
                    }
                    row += 1;
                    continue;
                }
                for x in 0..cols {
                    let at = (line, start + x);
                    let col = start + x;
                    if hits.iter().any(|&h| h <= col && col < h + self.find.len()) {
                        grid.fill(fb, row, gutter + x, 1, colors[Part::Search]);
                    }
                    if selection.is_some_and(|(a, b)| a <= at && at < b) && at.1 <= text.len() {
                        grid.fill(fb, row, gutter + x, 1, colors[Part::Selection]);
                    }
                    match text.get(start + x) {
                        Some(&c) => {
                            let color = colors[Part::from(roles[start + x])];
                            let shown = match fonts.choose(text, start + x) {
                                Shown::Font => c,
                                Shown::Extra(glyph) => {
                                    self.extra_cells.push((gutter + x, row, glyph, color));
                                    0
                                }
                                Shown::Blank => 0,
                            };
                            screen.put_at((gutter + x) as u32, row as u32, shown, color);
                        }
                        None => {
                            if start + x == text.len() {
                                marks.push(((gutter + x) * cell, row));
                            }
                            break;
                        }
                    }
                }
                if current && v.col >= start && v.col - start < cols {
                    cursor = Some(((gutter + v.col - start) * cell, row));
                }
                row += 1;
            }
            line += 1;
        }
        self.map = None;
        if map {
            self.draw_map(fb, colors, rows);
        }
        let screen = &mut self.screen;

        // The ruler numbers the visible columns, every other one, on tiles
        // or in digits. Tiles other than the cursor's take the bar's color,
        // with their digits recolored after drawing.
        let ruler = rows;
        let mut ruler_digits = Vec::new();
        grid.fill(fb, ruler, 0, width, BAR);
        let first = if wrap { 0 } else { v.left };
        let cursor_col = if wrap { v.col % cols } else { v.col };
        if half && ruler_text.is_none() {
            // The columns sit over the cursor's row, as wide as its
            // characters.
            let line = &self.lines[v.line];
            let row_start = if wrap { v.col - cursor_col } else { v.left };
            let left = gutter * cell;
            let cursor_x = left + half_x(line, row_start, v.col, cell);
            let mut px = left;
            for x in 0..cols {
                let col = first + x;
                let w = half_cell_width(line, row_start + x, cell);
                let digit = Over::Half(u16::from(b'0' + (col % 10) as u8));
                // A digit in the middle of its column.
                let digit_x = px + (w - cell / 2) / 2;
                let color = if col.is_multiple_of(10) { TEXT } else { DIM };
                if tiles {
                    // A tile is a cell wide, so it covers the next column
                    // of half-width characters: tiles every other column
                    // would run together ("10121416"). Every tenth column
                    // takes a tile, and columns 4, 6 and 8 after it a
                    // digit, leaving a gap after the tile; the cursor's
                    // tile comes first.
                    let clear = |x: usize, w: usize| x + w <= cursor_x || x >= cursor_x + cell;
                    if col == cursor_col {
                        self.over
                            .push((px, ruler, Over::Font(ruler_tile(col)), TEXT));
                    } else if col.is_multiple_of(10) && clear(px, cell) {
                        self.over
                            .push((px, ruler, Over::Font(ruler_tile(col)), BAR));
                        ruler_digits.push((px, color));
                    } else if matches!(col % 10, 4 | 6 | 8) && clear(digit_x, cell / 2) {
                        self.over.push((digit_x, ruler, digit, DIM));
                    }
                } else if col == cursor_col {
                    grid.fill_px(fb, ruler, px, w, TEXT);
                    self.over.push((digit_x, ruler, digit, BLACK));
                } else if col.is_multiple_of(2) {
                    self.over.push((digit_x, ruler, digit, color));
                }
                px += w;
            }
        }
        for x in 0..cols {
            if ruler_text.is_some() || half {
                break;
            }
            let col = first + x;
            let at = (gutter + x) as u32;
            if tiles {
                if col == cursor_col {
                    // On a white tile, as the current line's number.
                    screen.put_at(at, ruler as u32, ruler_tile(col), TEXT);
                } else if col.is_multiple_of(2) {
                    let color = if col.is_multiple_of(10) { TEXT } else { DIM };
                    screen.put_at(at, ruler as u32, ruler_tile(col), BAR);
                    ruler_digits.push(((gutter + x) * cell, color));
                }
            } else if col == cursor_col {
                grid.fill(fb, ruler, gutter + x, 1, TEXT);
                let digit = b'0' + (col % 10) as u8;
                screen.put_at(at, ruler as u32, u16::from(digit), BLACK);
            } else if col.is_multiple_of(2) {
                let digit = b'0' + (col % 10) as u8;
                let color = if col.is_multiple_of(10) { TEXT } else { DIM };
                screen.put_at(at, ruler as u32, u16::from(digit), color);
            }
        }

        // The status line, and the pixels below the last whole row.
        let status = ruler + 1;
        grid.fill(fb, status, 0, width, BAR);
        let below = grid.top + (status + 1) * grid.cell;
        for py in below as u32..fb.height() {
            for px in 0..fb.width() {
                fb.set(px, py, BAR);
            }
        }
        let mut prompt_cursor = None;
        let extras = &mut self.extra_cells;
        match &status_text {
            Some((text, field)) => {
                put_str(screen, extras, &fonts, (0, status), text, TEXT);
                prompt_cursor = Some(*field);
            }
            None => {
                let text = format!("PRG{} {name}", self.slot);
                put_str(screen, extras, &fonts, (0, status), &text, TEXT);
            }
        }
        if let Some(text) = &ruler_text {
            put_str(screen, extras, &fonts, (0, ruler), text, TEXT);
        }
        if let Some(note) = &note {
            let x = width.saturating_sub(note.encode_utf16().count());
            put_str(screen, extras, &fonts, (x, status), note, DIM);
        }

        screen.draw(fb, font);
        for &(x, y, glyph, color) in &self.extra_cells {
            let at = ((x * cell) as u32, (grid.top + y * cell) as u32);
            look.extra.draw(fb, glyph, at, color);
        }
        let halves = look.halves.unwrap_or_else(|| font.half_glyphs());
        for &(x, y, over, color) in &self.over {
            let at = (x as u32, (grid.top + y * cell) as u32);
            match over {
                Over::Half(c) => {
                    if let Some(alpha) = halves.get(c, look.font) {
                        draw_half_glyph(fb, alpha, cell as u32, at, color);
                    }
                }
                Over::Font(c) => draw_font_glyph(fb, font, c, look.font, at, color),
                Over::Extra(glyph) => look.extra.draw(fb, glyph, at, color),
            }
        }
        for (x, color) in ruler_digits {
            let y0 = grid.top + ruler * cell;
            for py in y0..y0 + cell {
                for px in x..x + cell {
                    let (px, py) = (px as u32, py as u32);
                    if fb.get(px, py).is_some_and(|c| c != BAR) {
                        fb.set(px, py, color);
                    }
                }
            }
        }
        for (x, y) in marks {
            grid.glyph(fb, x, y, &RETURN_GLYPH, RETURN_MARK);
        }
        if let (Some(x), true) = (prompt_cursor, blink) {
            for py in 0..cell {
                for px in 0..cell / 4 {
                    let (fx, fy) = (x * cell + px, grid.top + status * cell + py);
                    fb.set(fx as u32, fy as u32, TEXT);
                }
            }
        }
        // How the command being typed is used, above the cursor's line
        // or at the bottom.
        let options = self
            .complete
            .filter(|_| !self.quiet && self.mode == Mode::Edit);
        if let (Some((x, y)), Some(options)) = (cursor, options) {
            let names = self.program_names();
            let hint = (options.hints && self.selection().is_none())
                .then(|| hint::find(&self.lines[v.line], v.col, &self.library, &names))
                .flatten();
            let top = (grid.top + y * cell) as i64;
            let left = (gutter * cell) as i64;
            if options.bottom {
                let band = hint::Band {
                    left,
                    top: grid.top as i64,
                    bottom: (grid.top + ruler * cell) as i64,
                    cursor: top,
                };
                let syntax = self.completion.as_ref().and_then(Completion::syntax);
                hint::draw_band(fb, fonts, band, (hint.as_ref(), syntax), cell as u32);
            } else if let Some(hint) = &hint {
                hint.draw(fb, fonts, left, (top, top + cell as i64), cell as u32);
            }
            if let Some(list) = &self.completion {
                let start = list.start.min(v.col);
                let back = match half {
                    true => half_x(&self.lines[v.line], start, v.col, cell),
                    false => (v.col - start) * cell,
                };
                let at = (x.saturating_sub(back) as i64, top + cell as i64);
                list.draw(fb, fonts, at, top, cell as u32, colors);
            }
        }
        if let (Some((x, y)), true) = (cursor, blink) {
            // A bar for inserting, a block for overwriting.
            let w = match (self.overwrite, half) {
                (false, _) => cell / 4,
                (true, false) => cell,
                (true, true) => half_cell_width(&self.lines[v.line], v.col, cell),
            };
            for py in 0..cell {
                for px in 0..w {
                    let (fx, fy) = (x + px, grid.top + y * cell + py);
                    fb.set(fx as u32, fy as u32, colors[Part::Character]);
                }
            }
        }
    }

    /// Fades the line numbers, the ruler and the status line of the last
    /// [`Editor::draw`] toward `background`, on the side of a split
    /// display without the keys.
    pub fn dim_margins(&self, fb: &mut Framebuffer, background: u32) {
        let grid = self.grid;
        let gutter = (self.gutter() * grid.cell) as u32;
        let footer = (grid.top + self.size().1 * grid.cell) as u32;
        for y in grid.top as u32..fb.height() {
            let right = if y >= footer { fb.width() } else { gutter };
            for x in 0..right.min(fb.width()) {
                if let Some(c) = fb.get(x, y) {
                    fb.set(x, y, blend(c, background, MARGIN_DIM));
                }
            }
        }
    }
}

impl Editor {
    /// The status line while typing in it, with the column after what is
    /// typed, and what the ruler's row shows instead of the ruler.
    fn prompt(&self) -> (Option<(String, usize)>, Option<String>) {
        let field = |label: &str, text: &[u16]| {
            let text = String::from_utf16_lossy(text);
            let at = label.len() + text.encode_utf16().count();
            Some((format!("{label}{text}"), at))
        };
        match self.mode {
            Mode::Edit => (None, None),
            Mode::GoTo => (field("Go to line: ", &self.goto), None),
            Mode::Find => (field("Find: ", &self.find), None),
            Mode::Replace => (
                field("Replace with: ", &self.with),
                Some(format!("Find: {}", String::from_utf16_lossy(&self.find))),
            ),
        }
    }

    /// The count of matches shown while finding: "2/5".
    fn mode_note(&self) -> Option<String> {
        if !matches!(self.mode, Mode::Find | Mode::Replace) || self.find.is_empty() {
            return None;
        }
        Some(match self.match_count() {
            (0, _) => "Not found".to_string(),
            (n, Some(i)) => format!("{i}/{n}"),
            (n, None) => format!("{n}"),
        })
    }
}

/// Whether a [`keys`] code moves the cursor without editing.
pub fn is_movement(code: u16) -> bool {
    matches!(
        code,
        keys::LEFT
            | keys::RIGHT
            | keys::UP
            | keys::DOWN
            | keys::HOME
            | keys::END
            | keys::PAGE_UP
            | keys::PAGE_DOWN
    )
}

/// A line number's six digits on three tiles, two digits each, with the
/// leading zeros dimmed as SmileBASIC 4 does.
fn number_tiles(number: usize) -> [u16; 3] {
    let n = number % 1_000_000;
    let pairs = [n / 10_000, n / 100 % 100, n % 100];
    let mut leading = true;
    pairs.map(|pair| {
        let pair = pair as u16;
        if leading && pair == 0 {
            0xE16E
        } else if leading {
            // 00-09 here have their first zero dimmed; 10-99 are plain.
            leading = false;
            0xE100 + pair
        } else if pair < 10 {
            0xE164 + pair
        } else {
            0xE100 + pair
        }
    })
}

/// A column number on one tile: 0-9 on one-digit tiles, then its last
/// two digits.
fn ruler_tile(col: usize) -> u16 {
    match (col, (col % 100) as u16) {
        (0..10, n) => 0xE170 + n,
        (_, n @ 0..10) => 0xE164 + n,
        (_, n) => 0xE100 + n,
    }
}

/// Screen rows taken by a line when wrapping at `cols`; the last row
/// keeps room for the cursor and the return mark.
fn rows_of(line: &[u16], cols: usize) -> usize {
    line.len() / cols + 1
}

/// Puts `text` on the editor's screen from column `x` of row `y`, with
/// the characters `fonts` draws from other sources than the font left
/// empty there and added to `extras`.
fn put_str(
    screen: &mut TextScreen,
    extras: &mut Vec<(usize, usize, Glyph, u32)>,
    fonts: &Fonts,
    (x, y): (usize, usize),
    text: &str,
    color: u32,
) {
    let mut units = text.encode_utf16().peekable();
    let (mut prev, mut col) = (None, x);
    while let Some(c) = units.next() {
        let shown = match fonts.choose_unit(c, prev, units.peek().copied()) {
            Shown::Font => c,
            Shown::Extra(glyph) => {
                extras.push((col, y, glyph, color));
                0
            }
            Shown::Blank => 0,
        };
        screen.put_at(col as u32, y as u32, shown, color);
        (prev, col) = (Some(c), col + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::SMILEBASIC;

    #[test]
    fn room_below_leaves_the_menu_bar_its_share() {
        let mut e = editor("A=1");
        let font = Font::blank();
        let mut fb = Framebuffer::new(400, 300);
        e.draw(&mut fb, &font, "", look(true), false);
        let (top, rows) = (e.grid.top, e.size().1);
        let below = Look {
            bottom: 100,
            ..look(true)
        };
        e.draw(&mut fb, &font, "", below, false);
        assert_eq!(e.grid.top, top);
        assert!(e.size().1 < rows);
    }

    #[test]
    fn names_at_the_cursor() {
        let mut e = editor("GOSUB @SUB\nA$=NAME$(1)\n@SUB\nDEF NAME$(N)\nEND");
        e.go(0, 8);
        assert_eq!(e.word_or_selection().as_deref(), Some("@SUB"));
        e.go(1, 5);
        assert_eq!(e.word_or_selection().as_deref(), Some("NAME$"));
        assert!(e.go_to_definition("@sub"));
        assert_eq!(e.cursor().0, 3);
        assert!(!e.go_to_definition("NOPE"));
        e.go(1, 0);
        e.anchor = Some((1, 2));
        assert_eq!(e.word_or_selection().as_deref(), Some("A$"));
    }

    #[test]
    fn line_shifts() {
        let joined = |start, end| LineShift::Joined { start, end };
        let broken = |at, count| LineShift::Broken { at, count };
        // Whole lines 2 and 3 taken out: 4 takes 2's place.
        let whole = joined((2, 0), (4, 0));
        let mapped: Vec<_> = (0..6).map(|l| whole.map(l)).collect();
        assert_eq!(mapped, [Some(0), Some(1), None, None, Some(2), Some(3)]);
        // Text up to within line 3 taken from line 2's start, as undoing
        // a paste there: 3 takes 2's place.
        let undo = joined((2, 0), (3, 3));
        assert_eq!(
            (undo.map(2), undo.map(3), undo.map(4)),
            (None, Some(2), Some(3))
        );
        // Line 3 joined onto the end of 2: 2 stays, 3 is gone.
        let join = joined((2, 5), (3, 0));
        let mapped: Vec<_> = (1..5).map(|l| join.map(l)).collect();
        assert_eq!(mapped, [Some(1), Some(2), None, Some(3)]);
        // Text within a line moves nothing.
        assert_eq!(joined((2, 1), (2, 4)).map(2), Some(2));
        // Lines put in at a line's start push it down; within it, not.
        assert_eq!(broken((2, 0), 2).map(2), Some(4));
        assert_eq!(broken((2, 3), 2).map(2), Some(2));
        assert_eq!(broken((2, 3), 2).map(3), Some(5));
        assert_eq!(broken((2, 3), 2).map(1), Some(1));
    }

    /// The bundled extra glyphs, one set per test thread.
    fn extra() -> &'static ExtraGlyphs {
        thread_local! {
            static EXTRA: &'static ExtraGlyphs = Box::leak(Box::new(ExtraGlyphs::bundled()));
        }
        EXTRA.with(|e| *e)
    }

    fn look(tiles: bool) -> Look<'static> {
        Look {
            wrap: false,
            tiles,
            map: false,
            top: None,
            bottom: 0,
            font: FontSize::Small,
            colors: &SMILEBASIC,
            extra: extra(),
            first: FontSource::SmileBasic,
            half_width: false,
            halves: None,
        }
    }

    fn editor(source: &str) -> Editor {
        let mut e = Editor::new(400, 240);
        e.open(0, source, None);
        e
    }

    fn typing(e: &mut Editor, text: &str) {
        for c in text.encode_utf16() {
            e.key(c);
        }
    }

    #[test]
    fn splits_lines_like_the_lexer() {
        let e = editor("A\r\nB\rC\nD");
        assert_eq!(e.text(), "A\nB\nC\nD");
        assert_eq!(editor("").lines.len(), 1);
        assert_eq!(editor("A\n").lines.len(), 2);
    }

    #[test]
    fn type_split_and_join() {
        let mut e = editor("PRINT 1");
        e.key(keys::END);
        typing(&mut e, "23");
        e.key(keys::LEFT);
        e.key(keys::ENTER);
        assert_eq!(e.text(), "PRINT 12\n3");
        assert_eq!(e.cursor(), (2, 0));
        e.key(keys::BACKSPACE);
        assert_eq!((e.text().as_str(), e.cursor()), ("PRINT 123", (1, 8)));
        e.key(keys::END);
        e.key(keys::DELETE);
        assert_eq!(e.text(), "PRINT 123");
        assert_eq!(e.close().as_deref(), Some("PRINT 123"));
    }

    #[test]
    fn untouched_source_is_not_returned() {
        let mut e = editor("A\r\nB");
        e.key(keys::DOWN);
        e.key(keys::RIGHT);
        assert_eq!(e.close(), None);
    }

    #[test]
    fn up_and_down_keep_the_column() {
        let mut e = editor("ABCDEF\nAB\nABCDEF");
        e.key(keys::END);
        e.key(keys::DOWN);
        assert_eq!(e.cursor(), (2, 2));
        e.key(keys::DOWN);
        assert_eq!(e.cursor(), (3, 6));
        e.key(keys::LEFT);
        e.key(keys::UP);
        assert_eq!(e.cursor(), (2, 2));
        e.key(keys::RIGHT);
        assert_eq!(e.cursor(), (3, 0));
        e.key(keys::LEFT);
        assert_eq!(e.cursor(), (2, 2));
    }

    #[test]
    fn overwrite_and_tab() {
        let mut e = editor("ABC");
        e.key(keys::INSERT);
        typing(&mut e, "XY");
        assert_eq!(e.text(), "XYC");
        e.key(keys::INSERT);
        e.key(keys::HOME);
        e.tab_step = 4;
        e.key(keys::TAB);
        assert_eq!((e.text().as_str(), e.cursor()), ("    XYC", (1, 4)));
        // Control codes other than the editing keys type nothing.
        e.key(keys::ESCAPE);
        assert_eq!(e.text(), "    XYC");
        e.tab_step = 3;
        e.key(keys::TAB);
        assert_eq!(e.text(), "      XYC");
    }

    #[test]
    fn open_restores_where_a_slot_was_left() {
        let mut e = editor("A\nB\nC");
        e.key(keys::DOWN);
        e.key(keys::DOWN);
        e.close();
        e.open(1, "X", None);
        assert_eq!(e.cursor(), (1, 0));
        e.close();
        e.open(0, "A\nB\nC", None);
        assert_eq!(e.cursor(), (3, 0));
        e.open(0, "A", None);
        assert_eq!(e.cursor(), (1, 0), "clamped to a shorter program");
        e.open(0, "A\nB\nC", Some((2, 0)));
        assert_eq!(e.cursor(), (2, 0));
    }

    #[test]
    fn scrolls_to_the_cursor() {
        let mut e = editor(&"X\n".repeat(100));
        e.key(keys::PAGE_DOWN);
        e.scroll(false);
        assert_eq!((e.cursor().0, e.view.top), (28, 1));
        e.file_end();
        e.scroll(false);
        assert_eq!(e.view.top, 101 - 27);
        e.file_top();
        e.scroll(false);
        assert_eq!(e.view.top, 0);

        let mut e = editor(&"A".repeat(100));
        e.key(keys::END);
        e.scroll(false);
        assert_eq!(e.view.left, 100 + 1 - 46);
        e.scroll(true);
        assert_eq!(e.view.left, 0);
    }

    #[test]
    fn wrapped_lines_take_more_rows() {
        let long = "A".repeat(46 * 3);
        let mut e = editor(&format!("{long}\n").repeat(10));
        e.go(9, 0);
        e.scroll(true);
        // Each line takes four rows (the last for its return mark).
        assert_eq!(e.view.top, 3);
        e.file_end();
        e.scroll(true);
        assert_eq!(e.view.top, 4);
    }

    fn row(e: &Editor, y: u32) -> String {
        (0..e.screen.size().0)
            .map(|x| e.screen.cell(x, y).code)
            .map(|c| {
                if c == 0 {
                    ' '
                } else {
                    char::from_u32(c.into()).unwrap()
                }
            })
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    #[test]
    fn extra_glyphs_fill_one_cell_each() {
        // The font lacks everything from U+AC00 on, as SmileBASIC 4's lacks
        // Hangul and surrogates, and draws nothing for the rest.
        let font = Font::blank_from(0xAC00);
        let mut e = editor("A가😀\u{10FFFD}B");
        let mut fb = Framebuffer::new(400, 240);
        e.draw(&mut fb, &font, "", look(true), false);
        // Characters the font has stay on the screen; those drawn from the
        // extra glyphs leave their cell empty there; a character nothing
        // has keeps its two units, which the font draws as boxes.
        let codes: Vec<u16> = (4..11).map(|x| e.screen.cell(x, 0).code).collect();
        assert_eq!(codes, [0x41, 0, 0, 0, 0xDBFF, 0xDFFD, 0x42]);
        // One cell per UTF-16 unit, as the cursor counts: 가 in column 1,
        // 😀 in column 2 with column 3 empty.
        let top = e.grid.top as u32;
        let back = fb.get(4 * 8, top);
        let inked = |col: u32| {
            let x0 = (4 + col) * 8;
            (0..8).any(|y| (x0..x0 + 8).any(|x| fb.get(x, top + y) != back))
        };
        assert!(!inked(0), "the blank font draws A as nothing");
        assert!(inked(1), "가");
        assert!(inked(2), "😀");
        assert!(!inked(3), "😀's second unit");
    }

    #[test]
    fn the_font_comes_before_a_chosen_file() {
        // A file with A and 가 as full squares.
        let square = |code: u32| {
            format!(
                "STARTCHAR x\nENCODING {code}\nDWIDTH 8 0\nBBX 8 8 0 0\nBITMAP\n{}ENDCHAR\n",
                "FF\n".repeat(8)
            )
        };
        let bdf = format!(
            "STARTFONT 2.1\nFONT_ASCENT 8\nFONT_DESCENT 0\n{}{}ENDFONT\n",
            square(0x41),
            square(0xAC00)
        );
        let file = ExtraGlyphs::from_bytes(bdf.into_bytes()).unwrap();
        let font = Font::blank_from(0xAC00);
        let mut e = editor("A가");
        let mut fb = Framebuffer::new(400, 240);
        e.draw(
            &mut fb,
            &font,
            "",
            Look {
                extra: &file,
                ..look(true)
            },
            false,
        );
        // A comes from the font, which draws it blank; 가 from the file.
        assert_eq!(e.screen.cell(4, 0).code, 0x41);
        let top = e.grid.top as u32;
        let back = fb.get(4 * 8, top);
        assert!((0..8).all(|i| fb.get(5 * 8 + i, top + i) != back));
    }

    #[test]
    fn the_first_source_is_the_settings_choice() {
        // A file with A, P and 가 as full squares.
        let square = |code: char| {
            format!(
                "STARTCHAR x\nENCODING {}\nDWIDTH 8 0\nBBX 8 8 0 0\nBITMAP\n{}ENDCHAR\n",
                code as u32,
                "FF\n".repeat(8)
            )
        };
        let bdf = format!(
            "STARTFONT 2.1\nFONT_ASCENT 8\nFONT_DESCENT 0\n{}{}{}ENDFONT\n",
            square('A'),
            square('P'),
            square('가')
        );
        let file = ExtraGlyphs::from_bytes(bdf.into_bytes()).unwrap();
        let font = Font::blank_from(0xAC00);
        let draw = |first| {
            let mut e = editor("A가B");
            let mut fb = Framebuffer::new(400, 240);
            let look = Look {
                extra: &file,
                first,
                ..look(true)
            };
            e.draw(&mut fb, &font, "", look, false);
            let top = e.grid.top as u32;
            let square = |x: u32, y: u32| {
                let back = fb.get(390, y);
                (0..8).all(|i| fb.get(x + i, y + i) != back)
            };
            // A, 가 and B in the code; P of "PRG0" in the status line.
            let codes = [4, 5, 6].map(|x| e.screen.cell(x, 0).code);
            let squares = [square(4 * 8, top), square(5 * 8, top)];
            (
                codes,
                squares,
                e.screen.cell(0, 28).code,
                square(0, top + 28 * 8),
            )
        };
        // SmileBASIC's font first: A and B from it, 가 from the file.
        let (codes, squares, p, p_square) = draw(FontSource::SmileBasic);
        assert_eq!(codes, [0x41, 0, 0x42]);
        assert_eq!(squares, [false, true]);
        assert_eq!((p, p_square), (0x50, false));
        // The file first: A, 가 and the status line's P from it.
        let (codes, squares, p, p_square) = draw(FontSource::File);
        assert_eq!(codes, [0, 0, 0x42]);
        assert_eq!(squares, [true, true]);
        assert_eq!((p, p_square), (0, true));
        // The bundled fonts first: A, B and 가 from them, not squares.
        let (codes, squares, p, p_square) = draw(FontSource::Bundled);
        assert_eq!(codes, [0, 0, 0]);
        assert_eq!(squares, [false, false]);
        assert_eq!((p, p_square), (0, false));
    }

    #[test]
    fn draws_numbers_ruler_and_status() {
        let mut e = editor("PRINT 1");
        let mut fb = Framebuffer::new(400, 240);
        e.draw(&mut fb, &Font::blank(), "GAME", look(true), true);
        assert_eq!(row(&e, 0), "\u{E16E}\u{E16E}\u{E101}\u{E17F}PRINT 1");
        let ruler: Vec<u16> = (4..15).map(|x| e.screen.cell(x, 27).code).collect();
        assert_eq!(
            ruler,
            [
                0xE170, 0, 0xE172, 0, 0xE174, 0, 0xE176, 0, 0xE178, 0, 0xE10A
            ]
        );
        assert_eq!(row(&e, 28), "PRG0 GAME");
        // The top row is left for the menu bar.
        assert_eq!(fb.get(0, 7), Some(SMILEBASIC[Part::Background]));
        // The cursor bar at column 0 of the code.
        assert_eq!(fb.get(4 * 8, 8), Some(TEXT));
        assert_eq!(fb.get(4 * 8 + 2, 8), Some(SMILEBASIC[Part::CursorLine]));
    }

    #[test]
    fn ruler_tiles_take_the_bar_color() {
        let mut e = editor("PRINT 1");
        let mut fb = Framebuffer::new(400, 240);
        e.draw(&mut fb, &Font::fallback(), "GAME", look(true), true);
        // Column 2's tile: dim digits on the bar.
        let cell: Vec<u32> = (0..64)
            .map(|i| fb.get((4 + 2) * 8 + i % 8, 28 * 8 + i / 8).unwrap())
            .collect();
        assert!(cell.iter().all(|&c| c == BAR || c == DIM));
        assert!(cell.contains(&DIM));
    }

    #[test]
    fn plain_numbers_take_the_digits_they_need() {
        let mut e = editor(&"PRINT\n".repeat(119));
        let mut fb = Framebuffer::new(400, 240);
        e.draw(&mut fb, &Font::blank(), "GAME", look(false), true);
        assert_eq!(row(&e, 0), "  1 PRINT");
        assert!(row(&e, 27).starts_with("    0 2 4"));
    }

    #[test]
    fn number_tiles_dim_leading_zeros() {
        assert_eq!(number_tiles(1), [0xE16E, 0xE16E, 0xE101]);
        assert_eq!(number_tiles(123), [0xE16E, 0xE101, 0xE117]);
        assert_eq!(number_tiles(1005), [0xE16E, 0xE10A, 0xE169]);
        assert_eq!(number_tiles(120_000), [0xE10C, 0xE164, 0xE164]);
        assert_eq!(ruler_tile(7), 0xE177);
        assert_eq!(ruler_tile(42), 0xE12A);
        assert_eq!(ruler_tile(105), 0xE169);
    }

    #[test]
    fn select_copy_cut_and_paste() {
        let mut e = editor("ABC\nDEF\nGHI");
        e.key(keys::RIGHT);
        e.key_select(keys::DOWN);
        e.key_select(keys::RIGHT);
        assert_eq!(e.copy().as_deref(), Some("BC\nDE"));
        assert_eq!(e.cut().as_deref(), Some("BC\nDE"));
        assert_eq!((e.text().as_str(), e.cursor()), ("AF\nGHI", (1, 1)));
        assert_eq!(e.copy(), None);
        e.paste("12\r\n34\n5");
        assert_eq!(
            (e.text().as_str(), e.cursor()),
            ("A12\n34\n5F\nGHI", (3, 1))
        );
        // Typing replaces a selection; moving drops it.
        e.select_all();
        typing(&mut e, "X");
        assert_eq!(e.text(), "X");
        e.key_select(keys::LEFT);
        e.key(keys::RIGHT);
        e.key(keys::BACKSPACE);
        assert_eq!(e.text(), "");
    }

    #[test]
    fn typing_after_a_click_selects_nothing() {
        let mut e = editor("ABCDEF");
        let mut fb = Framebuffer::new(400, 240);
        e.draw(&mut fb, &Font::blank(), "GAME", look(true), true);
        e.mouse_down((4 + 6) * 8, 8 + 4, false);
        typing(&mut e, "XY");
        assert_eq!(e.text(), "ABCDEFXY");
        e.key(keys::BACKSPACE);
        e.key(keys::BACKSPACE);
        e.key(keys::BACKSPACE);
        e.key_select(keys::LEFT);
        e.key(keys::DELETE);
        assert_eq!(e.text(), "ABCD");
    }

    #[test]
    fn undo_and_redo() {
        let mut e = editor("PRINT 1\nEND");
        e.key(keys::END);
        typing(&mut e, " 23");
        e.key(keys::ENTER);
        typing(&mut e, "X");
        e.key(keys::BACKSPACE);
        e.key(keys::BACKSPACE);
        assert_eq!(e.text(), "PRINT 1 23\nEND");
        // The run of erasing, then Enter, then the run of typing.
        e.undo();
        assert_eq!(
            (e.text().as_str(), e.cursor()),
            ("PRINT 1 23\nX\nEND", (2, 1))
        );
        e.undo();
        assert_eq!(e.text(), "PRINT 1 23\n\nEND");
        e.undo();
        assert_eq!(
            (e.text().as_str(), e.cursor()),
            ("PRINT 1 23\nEND", (1, 10))
        );
        e.undo();
        assert_eq!((e.text().as_str(), e.cursor()), ("PRINT 1\nEND", (1, 7)));
        e.undo();
        assert_eq!(e.text(), "PRINT 1\nEND");
        e.redo();
        e.redo();
        assert_eq!(
            (e.text().as_str(), e.cursor()),
            ("PRINT 1 23\n\nEND", (2, 0))
        );
        // A new edit drops what was undone.
        typing(&mut e, "Y");
        e.redo();
        assert_eq!(e.text(), "PRINT 1 23\nY\nEND");

        // Moving ends a run; a selection typed over goes back in one step.
        let mut e = editor("ABC\nDEF");
        typing(&mut e, "1");
        e.key(keys::RIGHT);
        typing(&mut e, "2");
        e.select_all();
        e.paste("Z\nZ");
        typing(&mut e, "Q");
        e.undo();
        assert_eq!(e.text(), "Z\nZ");
        e.undo();
        assert_eq!(e.text(), "1A2BC\nDEF");
        e.undo();
        assert_eq!(e.text(), "1ABC\nDEF");
        e.key(keys::INSERT);
        typing(&mut e, "XY");
        assert_eq!(e.text(), "1AXY\nDEF", "at the cursor from before the 2");
        e.undo();
        assert_eq!(e.text(), "1ABC\nDEF");
    }

    #[test]
    fn line_commands() {
        let mut e = editor("A\n  B\nC");
        e.tab_step = 2;
        e.key(keys::DOWN);
        e.key(keys::END);
        e.unindent();
        assert_eq!((e.text().as_str(), e.cursor()), ("A\nB\nC", (2, 1)));
        e.key(keys::UP);
        e.key(keys::HOME);
        e.key_select(keys::DOWN);
        e.key_select(keys::DOWN);
        // The selection ends at line 3's start, which is left out.
        e.key(keys::TAB);
        assert_eq!(e.text(), "  A\n  B\nC");
        assert_eq!(e.copy().as_deref(), Some("  A\n  B"));
        e.comment();
        assert_eq!(e.text(), "'  A\n'  B\nC");
        e.undo();
        e.undo();
        assert_eq!(e.text(), "A\nB\nC");
        e.redo();
        e.select_all();
        e.comment();
        e.key(keys::DOWN);
        e.uncomment();
        e.uncomment();
        assert_eq!(e.text(), "'  A\n'  B\nC");

        let mut e = editor("A\nB\nC");
        e.key(keys::DOWN);
        e.insert_line();
        assert_eq!((e.text().as_str(), e.cursor()), ("A\nB\n\nC", (3, 0)));
        e.key(keys::UP);
        e.delete_line();
        e.delete_line();
        assert_eq!((e.text().as_str(), e.cursor()), ("A\nC", (2, 0)));
        e.delete_line();
        assert_eq!((e.text().as_str(), e.cursor()), ("A", (1, 0)));
        e.delete_line();
        assert_eq!(e.text(), "");
        e.undo();
        typing(&mut e, "XYZ");
        e.key(keys::LEFT);
        e.key(keys::LEFT);
        e.delete_to_end();
        assert_eq!(e.text(), "X", "typed before the A");
    }

    #[test]
    fn go_to_a_line() {
        let mut e = editor("A\nB\nC\nD");
        e.go_to_line();
        typing(&mut e, "3X");
        assert_eq!(e.cursor(), (1, 0), "nothing moves until Enter");
        e.key(keys::ENTER);
        assert_eq!((e.cursor(), e.mode()), ((3, 0), Mode::Edit));
        e.go_to_line();
        typing(&mut e, "99");
        e.key(keys::ENTER);
        assert_eq!(e.cursor(), (4, 0));
        e.go_to_line();
        typing(&mut e, "1");
        e.go_to_line();
        assert_eq!((e.cursor(), e.mode()), ((4, 0), Mode::Edit));
        assert_eq!(e.text(), "A\nB\nC\nD");
    }

    #[test]
    fn find_and_replace() {
        let mut e = editor("PRINT A\nprint b\nEND\nPRINT C");
        e.key(keys::DOWN);
        e.start_find(false);
        typing(&mut e, "Pri");
        assert_eq!(
            e.copy().as_deref(),
            Some("pri"),
            "from the cursor, in any case"
        );
        assert_eq!(e.mode_note().as_deref(), Some("2/3"));
        e.key(keys::DOWN);
        assert_eq!(e.cursor(), (4, 3));
        e.key(keys::DOWN);
        assert_eq!(e.cursor(), (1, 3), "around the end");
        e.key(keys::UP);
        assert_eq!(e.cursor(), (4, 3));
        typing(&mut e, "Q");
        assert_eq!((e.note.as_deref(), e.copy()), (Some("Not found"), None));
        // Typing finds again from where finding began.
        e.key(keys::BACKSPACE);
        assert_eq!(e.cursor(), (2, 3));
        e.key(keys::TAB);
        assert_eq!(e.mode(), Mode::Replace);
        typing(&mut e, "?");
        e.key(keys::ENTER);
        assert_eq!(e.text(), "PRINT A\n?nt b\nEND\nPRINT C");
        assert_eq!(e.cursor(), (4, 3), "on to the next");
        e.replace_all();
        assert_eq!(e.text(), "?NT A\n?nt b\nEND\n?NT C");
        assert_eq!(e.note.as_deref(), Some("Replaced 2"));
        e.key(keys::ESCAPE);
        e.undo();
        assert_eq!(e.text(), "PRINT A\n?nt b\nEND\nPRINT C");

        // A selection on one line is what to find.
        e.file_top();
        e.key_select(keys::RIGHT);
        e.start_find(false);
        e.key(keys::DOWN);
        assert_eq!(e.cursor(), (4, 1));
    }

    #[test]
    fn matches_do_not_overlap() {
        let aa: Vec<u16> = "AAAAA".encode_utf16().collect();
        assert_eq!(matches_in(&aa, &aa[..2]), [0, 2]);
        assert_eq!(matches_in(&aa, &[]), Vec::<usize>::new());
    }

    #[test]
    fn index_and_jumping() {
        let mut e = editor(&format!("@MAIN\n{}DEF F X\nEND", "PRINT\n".repeat(50)));
        assert_eq!(
            e.index(),
            [(0, "@MAIN".to_string()), (51, "DEF F".to_string())]
        );
        let back = e.place();
        e.jump_to(51);
        e.scroll(false);
        assert_eq!((e.cursor(), e.view.top), ((52, 0), 49));
        e.go_back(back);
        e.scroll(false);
        assert_eq!((e.cursor(), e.view.top), ((1, 0), 0));
        // The wheel leaves the cursor until it moves.
        e.wheel(10);
        e.scroll(false);
        assert_eq!((e.cursor(), e.view.top), ((1, 0), 10));
        e.key(keys::DOWN);
        e.scroll(false);
        assert_eq!(e.view.top, 1);
    }

    #[test]
    fn two_sides_share_a_program() {
        let mut a = editor("ABC\nDEF");
        let mut b = editor("ABC\nDEF");
        b.file_end();
        a.key(keys::DELETE);
        a.key(keys::DOWN);
        a.key(keys::DELETE);
        b.mirror(&a);
        assert_eq!((b.text().as_str(), b.cursor()), ("BC\nEF", (2, 2)));
        assert!(b.is_modified());
        b.take_document(&mut a);
        b.undo();
        assert_eq!(b.text(), "BC\nDEF");
        a.undo();
        assert_eq!(a.text(), "BC\nEF", "the history went along");
    }

    #[test]
    fn completes_words() {
        let mut e = editor("SCORE=0\n");
        e.file_end();
        e.complete = Some(Options {
            after: 2,
            commands: true,
            names: true,
            enter: true,
            hints: true,
            bottom: false,
        });
        typing(&mut e, "S");
        assert!(e.completion.is_none(), "one letter");
        typing(&mut e, "C");
        let words = |e: &Editor| -> Vec<String> {
            let list = e.completion.as_ref().unwrap();
            list.items.iter().map(|i| i.word.clone()).collect()
        };
        assert_eq!(words(&e), ["SCORE", "SCROLL"]);
        e.key(keys::DOWN);
        e.key(keys::ENTER);
        assert_eq!((e.text().as_str(), e.cursor()), ("SCORE=0\nSCORE", (2, 5)));
        e.undo();
        assert_eq!(e.text(), "SCORE=0\nSC");
        // Erasing looks again; moving or Esc closes.
        typing(&mut e, "O");
        e.key(keys::BACKSPACE);
        assert!(e.completion.is_some());
        e.key(keys::ESCAPE);
        assert!(e.completion.is_none());
        typing(&mut e, "O");
        e.key(keys::LEFT);
        assert!(e.completion.is_none());
        // Without a list, Enter breaks the line.
        e.key(keys::END);
        e.key(keys::ENTER);
        assert_eq!(e.text(), "SCORE=0\nSCO\n");
        // Ctrl+Space hides the list until pressed again, which shows it
        // from the first letter.
        typing(&mut e, "SC");
        e.toggle_popups();
        assert!(e.completion.is_none());
        typing(&mut e, "O");
        assert!(e.completion.is_none(), "hidden");
        e.key(keys::BACKSPACE);
        e.key(keys::BACKSPACE);
        e.toggle_popups();
        assert!(words(&e).contains(&"SCORE".to_string()), "after one letter");
    }

    #[test]
    fn overview_scrolls_the_view() {
        let mut e = editor(&"PRINT 1\n".repeat(300));
        let mut fb = Framebuffer::new(400, 240);
        let with_map = Look {
            map: true,
            ..look(true)
        };
        e.draw(&mut fb, &Font::blank(), "GAME", with_map, true);
        let m = e.map.unwrap();
        assert_eq!((m.x, m.top, m.first), (320, 8, 0));
        // The P of line 1 in the overview, over the background.
        let p = fb.get(320, 8).unwrap();
        assert_ne!(p, SMILEBASIC[Part::CursorLine]);
        // A press there scrolls; the cursor stays.
        e.mouse_down(330, (8 + 200 * MAP_PITCH / 3) as i32, false);
        e.mouse_drag(330, (8 + 60 * MAP_PITCH) as i32);
        e.draw(&mut fb, &Font::blank(), "GAME", with_map, true);
        assert_eq!((e.cursor(), e.view.top), ((1, 0), 60 - 27 / 2));
        assert!(e.map.unwrap().first > 0, "the overview moved along");
        // Off, nothing is drawn and presses edit again.
        e.draw(&mut fb, &Font::blank(), "GAME", look(true), true);
        e.mouse_down(330, 8 + 4, false);
        assert_eq!(e.cursor(), (48, 7), "the top line shown, at its end");
    }

    fn half(e: &mut Editor, fb: &mut Framebuffer) {
        let look = Look {
            half_width: true,
            ..look(true)
        };
        e.draw(fb, &Font::blank(), "GAME", look, true);
    }

    #[test]
    fn half_width_clicks_land_on_the_character_drawn() {
        // At 8 pixels, after the 4 columns of number tiles: A from 32,
        // 가 from 36, B from 44, 漢 from 48, C from 56, the end at 60.
        let mut e = editor("A가B漢C\nXYZ");
        let mut fb = Framebuffer::new(400, 240);
        half(&mut e, &mut fb);
        for (x, col) in [
            (32, 0),
            (35, 0),
            (36, 1),
            (43, 1),
            (44, 2),
            (50, 3),
            (56, 4),
            (60, 5),
            (90, 5),
        ] {
            e.mouse_down(x, 8 + 4, false);
            assert_eq!(e.pos(), (0, col), "pixel {x}");
        }
        // Dragging selects by the same columns: from B to C, B and 漢.
        e.mouse_down(45, 12, false);
        e.mouse_drag(57, 12);
        assert_eq!(e.copy().as_deref(), Some("B漢"));
        half(&mut e, &mut fb);
        let selection = SMILEBASIC[Part::Selection];
        assert_eq!(fb.get(44, 8 + 7), Some(selection));
        assert_eq!(fb.get(55, 8 + 7), Some(selection));
        assert_ne!(fb.get(43, 8 + 7), Some(selection));
        assert_ne!(fb.get(56, 8 + 7), Some(selection));
        // The second line is all ASCII: column 2 starts 8 pixels in.
        e.mouse_down(32 + 8, 16 + 4, false);
        assert_eq!(e.pos(), (1, 2));
        // Off, every character takes a whole cell again.
        e.draw(&mut fb, &Font::blank(), "GAME", look(true), true);
        e.mouse_down(36, 8 + 4, false);
        assert_eq!(e.pos(), (0, 0));
        e.mouse_down(48, 8 + 4, false);
        assert_eq!(e.pos(), (0, 2));
    }

    #[test]
    fn half_width_keeps_columns_and_moves_the_cursor_bar() {
        let mut e = editor("A가B漢C");
        let mut fb = Framebuffer::new(400, 240);
        for _ in 0..3 {
            e.key(keys::RIGHT);
        }
        // One column per character, as the status and HELPINFO count.
        assert_eq!(e.pos(), (0, 3));
        half(&mut e, &mut fb);
        let bar = SMILEBASIC[Part::Character];
        // The bar sits at the left of 漢, at 48, which the blank font
        // draws as nothing, not at column 3's cell.
        assert!((8..16).all(|y| fb.get(48, y) == Some(bar)));
        assert!((8..16).all(|y| fb.get(50, y) != Some(bar)));
        assert_eq!(e.pos(), (0, 3));
        // Typing goes in at that column.
        typing(&mut e, "x");
        assert_eq!(e.text(), "A가Bx漢C");
        // Wrapping still breaks lines by character count.
        let long = "가".repeat(30) + &"a".repeat(30);
        let mut e = editor(&long);
        let wrapped = Look {
            wrap: true,
            half_width: true,
            ..look(true)
        };
        e.draw(&mut fb, &Font::blank(), "GAME", wrapped, true);
        let cols = e.size().0;
        let starts: Vec<usize> = e.layout.iter().map(|&(_, start)| start).collect();
        assert_eq!(starts, [0, cols]);
    }

    #[test]
    fn half_width_ruler_labels_stay_apart() {
        let mut e = editor(&"A".repeat(80));
        let mut fb = Framebuffer::new(400, 240);
        let look = Look {
            half_width: true,
            ..look(true)
        };
        e.draw(&mut fb, &Font::blank(), "GAME", look, false);
        let ruler = e.size().1;
        let cell = e.grid.cell;
        let mut labels: Vec<(usize, usize, Over)> = (e.over.iter())
            .filter(|&&(_, row, _, _)| row == ruler)
            .map(|&(x, _, over, _)| match over {
                Over::Font(_) => (x, x + cell, over),
                _ => (x, x + cell / 2, over),
            })
            .collect();
        labels.sort_by_key(|&(x, ..)| x);
        // Tiles on the cursor's column and every tenth, digits on 4, 6 and 8.
        let tiles: Vec<u16> = (labels.iter())
            .filter_map(|&(.., over)| match over {
                Over::Font(c) => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(tiles[..4], [0, 10, 20, 30].map(ruler_tile));
        for &(.., over) in &labels {
            if let Over::Half(c) = over {
                assert!(b"468".contains(&(c as u8)), "{}", c as u8 as char);
            }
        }
        // None touches the next.
        for pair in labels.windows(2) {
            assert!(pair[0].1 < pair[1].0, "{:?}", (pair[0].0, pair[1].0));
        }
    }

    #[test]
    fn half_width_draws_ascii_in_half_cells() {
        // A page font whose glyphs are all full squares.
        let mut font = Font::blank();
        font.page_mut().fill(0xFFFF_FFFF);
        let mut e = editor("AB가");
        let mut fb = Framebuffer::new(400, 240);
        let look = Look {
            half_width: true,
            font: FontSize::Large,
            ..look(true)
        };
        e.draw(&mut fb, &font, "GAME", look, false);
        let top = e.grid.top as u32;
        let back = fb.get(390, top + 3);
        let inked: Vec<bool> = (64..112)
            .map(|x| (top..top + 16).any(|y| fb.get(x, y) != back))
            .collect();
        // A and B in 8 pixels each, from the half-width font rather than
        // the square font, 가 in 16, then nothing.
        assert!(inked[..8].iter().any(|&i| i) && !inked[..8].iter().all(|&i| i));
        assert!(inked[8..16].iter().any(|&i| i), "{inked:?}");
        assert!(inked[16..32].iter().all(|&i| i), "{inked:?}");
        assert_eq!(fb.get(96, top + 3), back, "the line ends after 32 pixels");
    }

    #[test]
    fn mouse_selects() {
        let mut e = editor("PRINT 1\nPRINT 2\nEND");
        let mut fb = Framebuffer::new(400, 240);
        e.draw(&mut fb, &Font::blank(), "GAME", look(true), true);
        // Column 2 of line 1, then the end of line 2.
        e.mouse_down((4 + 2) * 8 + 3, 8 + 4, false);
        e.mouse_drag(40 * 8, 16 + 7);
        assert_eq!(e.copy().as_deref(), Some("INT 1\nPRINT 2"));
        // Shift keeps the start; below the end goes to the last line's end.
        e.mouse_down(0, 20 * 8, true);
        assert_eq!(e.copy().as_deref(), Some("INT 1\nPRINT 2\nEND"));
        e.draw(&mut fb, &Font::blank(), "GAME", look(true), true);
        assert_eq!(fb.get(6 * 8, 8), Some(SMILEBASIC[Part::Selection]));
        // The ruler and the menu bar's row do nothing.
        e.mouse_down(0, 28 * 8, false);
        e.mouse_down(0, 4, false);
        assert!(e.copy().is_some());
    }

    #[test]
    fn colors_code_and_line_backgrounds() {
        let mut e = editor("@MAIN\nPRINT 1 'X\nDEF F\nEND");
        e.go(3, 0);
        let mut fb = Framebuffer::new(400, 240);
        e.draw(&mut fb, &Font::blank(), "GAME", look(true), true);
        let color = |x: u32, y: u32| e.screen.cell(x, y).color;
        let p = SMILEBASIC;
        assert_eq!(color(4, 0), p[Part::Label]);
        assert_eq!(color(4, 1), p[Part::Reserved]);
        assert_eq!(color(10, 1), p[Part::Numeric]);
        assert_eq!(color(12, 1), p[Part::Comment]);
        assert_eq!(fb.get(0, 8), Some(p[Part::LabelLine]));
        assert_eq!(fb.get(0, 16), Some(p[Part::Background]));
        assert_eq!(fb.get(0, 24), Some(p[Part::DefLine]));
        assert_eq!(fb.get(0, 32), Some(p[Part::CursorLine]));
    }

    #[test]
    fn sizes_and_the_large_font() {
        let base = (400, 240);
        for font in [FontSize::Small, FontSize::Large] {
            let mut size = EditorSize::Large;
            for code in [21, 28, 43, 57] {
                let (w, h) = frame_size(base, font, size);
                let mut fb = Framebuffer::new(w, h);
                let look = Look { font, ..look(true) };
                let mut e = editor("");
                e.draw(&mut fb, &Font::blank(), "GAME", look, true);
                assert_eq!(e.size().1, code, "{font:?} {size:?}");
                // The menu bar covers none of the first line.
                assert!(e.grid.top as u32 * 30 >= h);
                size = size.next();
            }
        }
        assert_eq!(
            frame_size(base, FontSize::Small, EditorSize::Tiny),
            (815, 489)
        );
        assert_eq!(EditorSize::Tiny.next(), EditorSize::Large);

        // 600x360: 12 pixels for the menu bar, then 21 rows of 16 pixels,
        // the last two for the ruler and the status line, and 12 left.
        let mut e = editor("PRINT 1\nEND");
        let mut fb = Framebuffer::new(600, 360);
        let large = Look {
            font: FontSize::Large,
            ..look(true)
        };
        e.draw(&mut fb, &Font::blank(), "GAME", large, true);
        assert_eq!(e.screen.size(), (37, 21));
        assert_eq!(row(&e, 1), "\u{E16E}\u{E16E}\u{E102}\u{E17F}END");
        assert_eq!(row(&e, 20), "PRG0 GAME");
        assert_eq!(fb.get(0, 359), Some(BAR), "below the last row");
        // The cursor is a quarter of a cell wide; clicks find 16-pixel cells.
        assert_eq!(fb.get(4 * 16 + 3, 12), Some(TEXT));
        assert_eq!(fb.get(4 * 16 + 4, 12), Some(SMILEBASIC[Part::CursorLine]));
        e.mouse_down(6 * 16 + 1, 12 + 16 + 1, false);
        assert_eq!(e.cursor(), (2, 2));
    }
}

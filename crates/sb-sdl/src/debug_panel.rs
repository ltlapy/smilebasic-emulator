//! The debugger's panel below the editor while a program is stopped: room
//! for the screens as the program left them, which the frontend draws at
//! the window's resolution, its calls under way, the watch expressions,
//! and the variables of the call chosen or of a slot, what PERFEND has
//! timed, the sprites in use, a graphic page, or what BGM and BEEP play.

pub mod bgm;
pub mod sprites;

use sb_core::value::{ArrayData, ElemType, Value, format_number};
use sb_core::vm::{Call, CallKind};
use sb_runtime::font::Font;
use sb_runtime::{Framebuffer, PerfStats};
use sdl3::keyboard::Keycode;

use crate::grp_view::{GrpView, Probe};
use crate::ui::{BORDER, DISABLED, HIGHLIGHT, PAD, Rect, TEXT, draw_text_scaled, fill};

pub(crate) const HEADING: u32 = 0xFF80_8080;
const NUMBER: u32 = 0xFFF0_4090;
const STRING: u32 = 0xFF60_80F0;
const FACE: u32 = 0xFF18_1818;
pub(crate) const TAB: u32 = 0xFF40_4040;
const ERROR: u32 = 0xFFF0_6060;
const SELECTED: u32 = 0xFF30_3850;
/// The mark of a watch that stops the program when it changes, red as
/// the editor's breakpoints.
const BREAKS: u32 = 0xFFDC_2828;
/// A breakpoint's condition, orange as in the editor.
const CONDITION: u32 = 0xFFE0_7818;
/// The value of the watch whose change stopped the program.
const CHANGED: u32 = 0xFFFF_DC00;
/// Every other row of the lists.
const STRIPE: u32 = 0xFF24_2428;
/// Characters of the calls' and watches' column, and of a watch's
/// expression, in large text; small text keeps the column's width and
/// fits twice as many.
const CALL_CHARS: u32 = 20;
const WATCH_CHARS: usize = 9;
/// The most characters a variable's name takes before it is cut.
const NAME_CHARS: usize = 10;
/// A value shown whole when the pointer rests on a row that cuts it:
/// characters to a line, and lines.
const TIP_CHARS: usize = 60;
const TIP_LINES: usize = 14;

/// Which variables the panel lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// Those of the chosen call's DEF.
    Locals,
    /// A slot's globals.
    Globals(usize),
    /// Not variables: PERFBEGIN's and PERFEND's measurements, which
    /// SmileBASIC 4 shows nowhere.
    Perf,
    /// The sprites in use, which only this tab gathers.
    Sprites,
    /// The running subprogram's, kept apart from the main interpreter's.
    SubSprites,
    /// A graphic page, which SmileBASIC 4 shows only as the program
    /// draws it.
    Grp,
    /// What the BGM tracks and BEEP play, which only this tab gathers.
    Bgm,
}

/// What the panel shows, gathered from the machine each frame.
pub struct View {
    pub calls: Vec<Call>,
    /// The tabs there are, and the variables of the one shown.
    pub tabs: Vec<Tab>,
    pub variables: Vec<(String, Value)>,
    /// Where the variables shown are: a DEF call's, by
    /// [`Call::frame`], or a slot's globals.
    pub frame: Option<usize>,
    pub slot: usize,
    /// The value of each watch, or why it has none.
    pub watches: Vec<Result<Value, String>>,
    /// A line at the bottom: why and where the program stopped, or that
    /// a step runs, red for a failure; then the keys, which go first when
    /// there is no room.
    pub status: String,
    pub status_failed: bool,
    pub keys: String,
    /// The watch whose change stopped the program.
    pub changed: Option<String>,
    /// Every breakpoint: slot, line and condition.
    pub breakpoints: Vec<(usize, u32, Option<String>)>,
    /// What PERFEND has timed since RUN.
    pub perf: [PerfStats; 8],
    /// The sprites in use, while the Sprites or Sub sprites tab shows.
    pub sprites: Option<sprites::List>,
    /// The GRP tab's page.
    pub grp: Probe,
    /// What plays, while the BGM tab shows.
    pub bgm: Option<bgm::Bgm>,
}

impl View {
    /// How many rows the tab shown has to scroll through.
    pub fn scrolled(&self) -> usize {
        match (&self.sprites, &self.bgm) {
            (Some(list), _) => list.total,
            (None, Some(bgm)) => bgm.len(),
            (None, None) => self.variables.len(),
        }
    }
}

/// What a click on the panel chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A call, by its place in the list from the outermost.
    Call(usize),
    Tab(Tab),
    /// A variable shown, by its place in [`View::variables`].
    Variable(usize),
    Watch(usize),
    AddWatch,
    /// A breakpoint, by its place in [`View::breakpoints`].
    Breakpoint(usize),
    /// The Perf tab's measurements.
    Perf,
    /// A sprite in the Sprites or Sub sprites tab shown, by its number.
    Sprite(usize),
    /// The rest of that tab.
    Sprites,
    /// The GRP tab's buttons for the pages, and its view of one.
    GrpPage(usize),
    Grp,
    /// The BGM tab's lines for a track and for a channel of it, a BEEP
    /// handle's, and the rest of the tab.
    BgmTrack(usize),
    BgmChannel(usize, usize),
    Beep(usize),
    Bgm,
    /// The button that puts the panel away.
    Close,
}

/// What is being typed into the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    NewWatch,
    Watch(usize),
    /// A new value for the variable of this name.
    Variable(String),
}

/// What a key typed into the panel did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Typed {
    Stay,
    /// Enter on a new value for a variable, which the machine takes.
    Value(String, String),
}

#[derive(Default)]
pub struct DebugPanel {
    /// The call chosen, from the outermost; `None` for the innermost.
    pub chosen: Option<usize>,
    pub tab: Option<Tab>,
    /// Variables scrolled past.
    scroll: usize,
    /// Where the panel and its clickable parts were last drawn.
    frame: Option<Rect>,
    hits: Vec<(Rect, Hit)>,
    /// Rows of variables the last frame had room for.
    rows: usize,
    /// The watch expressions, kept while the emulator runs.
    pub watches: Vec<String>,
    /// Those of them that stop the program when their values change.
    breaks: Vec<String>,
    /// The row last clicked.
    selected: Option<Hit>,
    typing: Option<(Target, String)>,
    /// Integers show in hexadecimal.
    pub hex: bool,
    /// Where the pointer is, and the rows of the last frame that cut
    /// their values, with the whole value's lines.
    pointer: Option<(i32, i32)>,
    tips: Vec<(Rect, Vec<String>)>,
    /// How many times its size the 8-pixel font is drawn, as last drawn.
    scale: u32,
    /// The sprite chosen in the Sprites tab, outlined on the screens,
    /// and in the Sub sprites tab, outlined on the subprogram's.
    pub sprite: Option<usize>,
    pub sub_sprite: Option<usize>,
    pub grp: GrpView,
    /// The BGM tab's level bars.
    meters: bgm::Meters,
}

/// The panel's character width, row height, padding and text offset in a
/// row, for the font drawn `scale` times its size.
pub(crate) struct Metrics {
    pub char: u32,
    pub line: u32,
    pub pad: u32,
    pub text: u32,
    scale: u32,
}

impl Metrics {
    fn new(scale: u32) -> Metrics {
        let scale = scale.max(1);
        Metrics {
            char: 8 * scale,
            line: 12 * scale,
            pad: 4 * scale,
            text: 2 * scale,
            scale,
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, x: u32, y: u32, text: &str, color: u32) {
        draw_text_scaled(fb, font, x, y, text, color, self.scale);
    }
}

/// A value whole, for a tip: an array's elements as INSPECT lists them,
/// and other values broken into lines.
fn tip_lines(value: &Value, hex: bool) -> Vec<String> {
    let lines: Vec<String> = match value {
        Value::Array(_) => (sb_core::inspect::lines(value).iter())
            .map(|l| String::from_utf16_lossy(l))
            .filter(|l| !l.is_empty())
            .collect(),
        _ => vec![value_text(value, usize::MAX, hex).0],
    };
    let mut wrapped: Vec<String> = Vec::new();
    for line in lines {
        let chars: Vec<char> = line.chars().collect();
        wrapped.extend(
            chars
                .chunks(TIP_CHARS)
                .map(|c| c.iter().collect::<String>()),
        );
    }
    if wrapped.len() > TIP_LINES {
        let more = wrapped.len() - (TIP_LINES - 1);
        wrapped.truncate(TIP_LINES - 1);
        wrapped.push(format!("... {more} more lines"));
    }
    wrapped
}

/// A value on one line: numbers as PRINT shows them, or integers in
/// hexadecimal, strings in quotes, arrays with their first elements, cut
/// to `chars`.
pub fn value_text(value: &Value, chars: usize, hex: bool) -> (String, u32) {
    let quoted = |s: &[u16]| format!("\"{}\"", String::from_utf16_lossy(s));
    let int = |n: i32| match hex {
        true => format!("&H{:X}", n as u32),
        false => n.to_string(),
    };
    let (text, color) = match value {
        Value::Default => ("(none)".to_string(), DISABLED),
        Value::Int(n) => (int(*n), NUMBER),
        Value::Real(_) => (String::from_utf16_lossy(&format_number(value)), NUMBER),
        Value::Str(s) => (quoted(&s.borrow()), STRING),
        Value::Array(a) => {
            let a = a.borrow();
            let items: Vec<String> = match &a.data {
                ArrayData::Int(v) => v.iter().take(chars).map(|&n| int(n)).collect(),
                ArrayData::Real(v) => (v.iter().take(chars))
                    .map(|&x| String::from_utf16_lossy(&format_number(&Value::Real(x))))
                    .collect(),
                ArrayData::Str(v) => v.iter().take(chars).map(|s| quoted(&s.borrow())).collect(),
            };
            let more = if a.len() > items.len() { ",…" } else { "" };
            let color = if a.elem_type() == ElemType::Str {
                STRING
            } else {
                NUMBER
            };
            (format!("[{}{more}]", items.join(",")), color)
        }
    };
    (cut(&text, chars), color)
}

/// A value's type as TYPEOF tells them apart, with an array's sizes;
/// STR for an array of strings, to leave the value room.
fn type_text(value: &Value) -> String {
    let elem = |e: ElemType| match e {
        ElemType::Int => "INT",
        ElemType::Real => "REAL",
        ElemType::Str => "STR",
    };
    match value {
        Value::Default => "DEFAULT".to_string(),
        Value::Int(_) => "INT".to_string(),
        Value::Real(_) => "REAL".to_string(),
        Value::Str(_) => "STRING".to_string(),
        Value::Array(a) => {
            let a = a.borrow();
            let dims: Vec<String> = a.dims.iter().map(usize::to_string).collect();
            format!("{}[{}]", elem(a.elem_type()), dims.join(","))
        }
    }
}

/// Text cut to `chars` characters, with an ellipsis if it was longer.
pub(crate) fn cut(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(chars.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// A PERFEND time in microseconds, and in milliseconds too with `ms`.
fn perf_time(micros: u64, ms: bool) -> String {
    match ms {
        true => format!("{micros}us {}.{:03}ms", micros / 1000, micros % 1000),
        false => format!("{micros}us"),
    }
}

/// A measurement's last, average and longest times, which are dashes
/// before its first PERFEND.
fn perf_times(stats: &PerfStats, ms: bool) -> [String; 3] {
    match stats.count {
        0 => std::array::from_fn(|_| "-".to_string()),
        _ => [stats.last, stats.average(), stats.max].map(|t| perf_time(t, ms)),
    }
}

fn call_text(call: &Call, chars: usize) -> String {
    let name = match &call.kind {
        CallKind::Top => "(top level)",
        CallKind::Def(name) => name,
        CallKind::Gosub => "GOSUB",
    };
    let at = format!(" {}:{}", call.slot, call.line);
    let room = chars.saturating_sub(at.chars().count());
    format!("{:<room$}{at}", cut(name, room))
}

impl DebugPanel {
    /// The call chosen, from the outermost, among `calls`.
    pub fn chosen_call(&self, calls: usize) -> usize {
        let last = calls.saturating_sub(1);
        self.chosen.map_or(last, |c| c.min(last))
    }

    /// Goes back to the innermost call and its variables, as after a new
    /// stop. The tabs that have nothing to do with the call stay, so that
    /// a sprite can be followed step by step.
    pub fn reset(&mut self) {
        self.chosen = None;
        if !matches!(
            self.tab,
            Some(Tab::Perf | Tab::Sprites | Tab::SubSprites | Tab::Grp | Tab::Bgm)
        ) {
            self.tab = None;
            self.scroll = 0;
        }
    }

    /// Whether the tab shown lists the subprogram's sprites rather than
    /// the main interpreter's.
    pub fn sub_tab(&self) -> bool {
        self.tab == Some(Tab::SubSprites)
    }

    /// The sprite chosen among the subprogram's sprites, or the main
    /// interpreter's.
    pub fn sprite_in(&self, sub: bool) -> Option<usize> {
        match sub {
            true => self.sub_sprite,
            false => self.sprite,
        }
    }

    /// Chooses a sprite, or none, among the subprogram's sprites or the
    /// main interpreter's.
    pub fn choose_sprite(&mut self, sub: bool, sprite: Option<usize>) {
        match sub {
            true => self.sub_sprite = sprite,
            false => self.sprite = sprite,
        }
    }

    /// The rows the Sprites tab has room for, from the first shown.
    pub fn sprite_rows(&self) -> (usize, usize) {
        // Before the tab is first drawn, enough for a tall panel.
        let rows = if self.rows == 0 { 64 } else { self.rows };
        (self.scroll, rows)
    }

    /// Forgets where it was drawn and what was typed, once it is not
    /// shown.
    pub fn hide(&mut self) {
        self.frame = None;
        self.hits.clear();
        self.tips.clear();
        self.typing = None;
        self.grp.set_place(None);
        self.grp.release();
    }

    /// The pointer moved to `at`, with the left button `held` or not as
    /// the motion tells; it drags the GRP tab's page only while the
    /// button stays down since it went down on the page. A motion with
    /// the button up ends a drag whose button-up never came, so that the
    /// page does not follow a pointer it was let go of.
    pub fn motion(&mut self, at: (i32, i32), held: bool) {
        self.pointer = Some(at);
        if !held {
            self.grp.release();
        }
        self.grp.drag_to(at);
    }

    /// The pointer left the window.
    pub fn leave(&mut self) {
        self.pointer = None;
    }

    /// The left button went up.
    pub fn release(&mut self) {
        self.grp.release();
    }

    /// Whether the GRP tab's page is blank, and its pixel under the
    /// pointer.
    pub fn grp_probe(&self, grp: &sb_runtime::graphics::Graphics) -> Probe {
        self.grp.probe(self.pointer, grp)
    }

    pub fn contains(&self, at: (i32, i32)) -> bool {
        self.frame.is_some_and(|f| f.contains(at))
    }

    /// The clickable part under a point, from the last frame drawn.
    pub fn hit_at(&self, at: (i32, i32)) -> Option<Hit> {
        (self.hits.iter())
            .find(|(r, _)| r.contains(at))
            .map(|&(_, hit)| hit)
    }

    /// A click: chooses a call or a tab, selects a row, or starts a new
    /// watch; a double click edits a watch.
    pub fn click(&mut self, at: (i32, i32), double: bool) -> Option<Hit> {
        let hit = self.hit_at(at);
        self.typing = None;
        self.selected = hit.filter(|h| matches!(h, Hit::Variable(_) | Hit::Watch(_)));
        match hit? {
            Hit::Call(i) => {
                self.chosen = Some(i);
                self.tab = None;
                self.scroll = 0;
            }
            Hit::Tab(tab) => {
                self.tab = Some(tab);
                self.scroll = 0;
            }
            Hit::AddWatch => self.add_watch(String::new()),
            Hit::GrpPage(page) => self.grp.page = page,
            Hit::Grp => self.grp.press(at),
            Hit::Close => {}
            Hit::Watch(i) if double => self.edit_watch(i),
            // A second click lets go of the sprite.
            Hit::Sprite(id) => {
                let sub = self.sub_tab();
                self.choose_sprite(sub, (self.sprite_in(sub) != Some(id)).then_some(id));
            }
            Hit::Watch(_)
            | Hit::Variable(_)
            | Hit::Breakpoint(_)
            | Hit::Perf
            | Hit::Sprites
            | Hit::BgmTrack(_)
            | Hit::BgmChannel(..)
            | Hit::Beep(_)
            | Hit::Bgm => {}
        }
        hit
    }

    /// Starts typing a new watch, beginning with `text`.
    pub fn add_watch(&mut self, text: String) {
        self.typing = Some((Target::NewWatch, text));
    }

    pub fn edit_watch(&mut self, i: usize) {
        if let Some(text) = self.watches.get(i) {
            self.typing = Some((Target::Watch(i), text.clone()));
        }
    }

    pub fn remove_watch(&mut self, i: usize) {
        if i < self.watches.len() {
            self.watches.remove(i);
            self.selected = None;
            self.forget_breaks();
        }
    }

    /// Whether a watch stops the program when its value changes.
    pub fn breaks(&self, i: usize) -> bool {
        self.watches.get(i).is_some_and(|w| self.breaks.contains(w))
    }

    /// Makes a watch stop the program when its value changes, or not.
    pub fn toggle_break(&mut self, i: usize) {
        let Some(watch) = self.watches.get(i) else {
            return;
        };
        match self.breaks.iter().position(|b| b == watch) {
            Some(at) => _ = self.breaks.remove(at),
            None => self.breaks.push(watch.clone()),
        }
    }

    /// The watches that stop the program when their values change.
    pub fn change_watches(&self) -> Vec<&str> {
        self.breaks.iter().map(String::as_str).collect()
    }

    /// Lets go of the breaks of watches there are no more.
    fn forget_breaks(&mut self) {
        let watches = &self.watches;
        self.breaks.retain(|b| watches.contains(b));
    }

    /// Starts typing a new value for a variable, beginning with its
    /// current one.
    pub fn edit_variable(&mut self, name: String, text: String) {
        self.typing = Some((Target::Variable(name), text));
    }

    /// Whether keys go to the panel's typing.
    pub fn typing(&self) -> bool {
        self.typing.is_some()
    }

    pub fn text(&mut self, text: &str) {
        if let Some((_, typed)) = &mut self.typing {
            typed.push_str(text);
        }
    }

    /// A key while typing: Enter keeps what was typed, Esc forgets it.
    pub fn key(&mut self, key: Keycode) -> Typed {
        match key {
            Keycode::Return | Keycode::KpEnter => {
                let Some((target, typed)) = self.typing.take() else {
                    return Typed::Stay;
                };
                let typed = typed.trim().to_string();
                match target {
                    Target::NewWatch if !typed.is_empty() => self.watches.push(typed),
                    Target::Watch(i) if typed.is_empty() => self.remove_watch(i),
                    // A watch that breaks goes on breaking as edited.
                    Target::Watch(i) => {
                        let old = std::mem::replace(&mut self.watches[i], typed.clone());
                        if self.breaks.contains(&old) && !self.breaks.contains(&typed) {
                            self.breaks.push(typed);
                        }
                        self.forget_breaks();
                    }
                    Target::Variable(name) => return Typed::Value(name, typed),
                    Target::NewWatch => {}
                }
            }
            Keycode::Escape => self.typing = None,
            Keycode::Backspace => {
                if let Some((_, typed)) = &mut self.typing {
                    typed.pop();
                }
            }
            _ => {}
        }
        Typed::Stay
    }

    /// The wheel scrolls the variables, or zooms the GRP tab's page.
    pub fn wheel(&mut self, rows: i32, variables: usize) {
        if self.grp.wheel(rows, self.pointer) {
            return;
        }
        let most = variables.saturating_sub(self.rows);
        self.scroll = (self.scroll as i64 + i64::from(rows)).clamp(0, most as i64) as usize;
    }

    /// Draws the panel into `f`, with the screens scaled down to fit on
    /// its left.
    /// Where the screens go in the panel at `f`, for screens of `size`:
    /// its height, at their shape.
    pub fn screen_place(f: Rect, (sw, sh): (u32, u32)) -> Rect {
        let h = f.h.saturating_sub(2 + PAD + 4);
        let w = (u64::from(h) * u64::from(sw.max(1)) / u64::from(sh.max(1))) as u32;
        Rect {
            x: f.x + PAD + 2,
            y: f.y + 2 + PAD / 2 + 2,
            w,
            h,
        }
    }

    /// A frame on, for what the panel animates: the BGM tab's level bars.
    pub fn advance(&mut self, view: &View) {
        if let Some(bgm) = &view.bgm {
            self.meters.update(bgm);
        }
    }

    /// Draws the panel at `f` on the display into `fb`, a frame of `f`'s
    /// size, leaving the place of screens of `size` clear for the
    /// frontend to draw them. Without a size, the screens show elsewhere
    /// and the panel has no place for them. The whole value of a row cut
    /// short goes elsewhere too: see [`DebugPanel::draw_tip`].
    pub fn draw(
        &mut self,
        fb: &mut Framebuffer,
        font: &Font,
        f: Rect,
        size: Option<(u32, u32)>,
        view: &View,
        scale: u32,
    ) {
        self.draw_at(fb, font, Rect { x: 0, y: 0, ..f }, size, view, scale);
        // Clicks and the pointer come in display pixels.
        let shift = |r: &mut Rect| (r.x, r.y) = (r.x + f.x, r.y + f.y);
        self.frame = Some(f);
        self.hits.iter_mut().for_each(|(r, _)| shift(r));
        self.tips.iter_mut().for_each(|(r, _)| shift(r));
        self.grp.shift(f.x, f.y);
    }

    /// Draws the panel at `f` on `fb`, a frame the size of the display,
    /// as part of what else is drawn there.
    pub fn draw_on_display(
        &mut self,
        fb: &mut Framebuffer,
        font: &Font,
        f: Rect,
        size: Option<(u32, u32)>,
        view: &View,
        scale: u32,
    ) {
        self.draw_at(fb, font, f, size, view, scale);
    }

    fn draw_at(
        &mut self,
        fb: &mut Framebuffer,
        font: &Font,
        f: Rect,
        size: Option<(u32, u32)>,
        view: &View,
        scale: u32,
    ) {
        self.scale = scale;
        let m = Metrics::new(scale);
        self.frame = Some(f);
        self.hits.clear();
        self.tips.clear();
        self.grp.set_place(None);
        fill(fb, f, BORDER);
        let inner = Rect {
            x: f.x,
            y: f.y + 2,
            w: f.w,
            h: f.h - 2,
        };
        fill(fb, inner, FACE | 0xFF00_0000);

        let (x0, w) = match size {
            Some(size) => {
                let screen = Self::screen_place(f, size);
                fill(fb, screen, 0);
                let frame = Rect {
                    x: screen.x - 2,
                    y: screen.y - 2,
                    w: screen.w + 4,
                    h: screen.h + 4,
                };
                crate::ui::outline(fb, frame, BORDER);
                (screen.x, screen.w)
            }
            None => (f.x, 0),
        };

        let close = Rect {
            x: inner.x + inner.w - m.line - m.pad / 2,
            y: inner.y + m.pad / 2,
            w: m.line,
            h: m.line,
        };
        fill(fb, close, TAB);
        m.draw(
            fb,
            font,
            close.x + (m.line - m.char) / 2,
            close.y + m.text,
            "x",
            TEXT,
        );
        self.hits.push((close, Hit::Close));

        // The calls, innermost last as BACKTRACE lists them reversed.
        let cx = x0 + w + 2 * m.pad;
        let mut y = inner.y + m.pad;
        m.draw(fb, font, cx, y + m.text, "CALLS", HEADING);
        y += m.line;
        let chosen = self.chosen_call(view.calls.len());
        let call_chars = (CALL_CHARS * 2 / m.scale) as usize;
        let watch_chars = WATCH_CHARS * 2 / m.scale as usize;
        let call_w = call_chars as u32 * m.char + m.pad;
        let bottom = inner.y + inner.h - m.line;
        for (n, (i, call)) in view.calls.iter().enumerate().rev().enumerate() {
            if y + m.line > bottom {
                break;
            }
            let row = Rect {
                x: cx - m.pad / 2,
                y,
                w: call_w,
                h: m.line,
            };
            if i == chosen {
                fill(fb, row, HIGHLIGHT);
            } else if n % 2 == 1 {
                fill(fb, row, STRIPE);
            }
            m.draw(fb, font, cx, y + m.text, &call_text(call, call_chars), TEXT);
            self.hits.push((row, Hit::Call(i)));
            y += m.line;
        }

        // The watches, and a row to add one.
        y += m.pad;
        if y + 2 * m.line <= bottom {
            m.draw(fb, font, cx, y + m.text, "WATCH", HEADING);
            y += m.line;
        }
        let typing = self.typing.clone();
        let cursor = |t: &str| format!("{t}_");
        for (i, expr) in self.watches.iter().enumerate() {
            if y + 2 * m.line > bottom {
                break;
            }
            let row = Rect {
                x: cx - m.pad / 2,
                y,
                w: call_w,
                h: m.line,
            };
            if self.selected == Some(Hit::Watch(i)) {
                fill(fb, row, SELECTED);
            } else if i % 2 == 1 {
                fill(fb, row, STRIPE);
            }
            if self.breaks.contains(expr) {
                fill(
                    fb,
                    Rect {
                        w: m.pad / 2,
                        ..row
                    },
                    BREAKS,
                );
            }
            if let Some((Target::Watch(t), typed)) = &typing
                && *t == i
            {
                m.draw(fb, font, cx, y + m.text, &cursor(typed), TEXT);
            } else {
                m.draw(fb, font, cx, y + m.text, &cut(expr, watch_chars), TEXT);
                let x = cx + (watch_chars as u32 + 1) * m.char;
                let room = call_chars - watch_chars - 1;
                let (text, color, whole) = match view.watches.get(i) {
                    Some(Ok(v)) => {
                        let (text, color) = value_text(v, room, self.hex);
                        (text, color, tip_lines(v, self.hex))
                    }
                    Some(Err(e)) => (cut(e, room), ERROR, vec![e.clone()]),
                    None => (String::new(), TEXT, Vec::new()),
                };
                let color = match view.changed.as_ref() == Some(expr) {
                    true => CHANGED,
                    false => color,
                };
                m.draw(fb, font, x, y + m.text, &text, color);
                if expr.chars().count() > watch_chars || text.ends_with('…') {
                    let mut lines = vec![format!("{expr} =")];
                    lines.extend(whole);
                    self.tips.push((row, lines));
                }
            }
            self.hits.push((row, Hit::Watch(i)));
            y += m.line;
        }
        if y + m.line <= bottom {
            let row = Rect {
                x: cx - m.pad / 2,
                y,
                w: call_w,
                h: m.line,
            };
            match &typing {
                Some((Target::NewWatch, typed)) => {
                    m.draw(fb, font, cx, y + m.text, &cursor(typed), TEXT)
                }
                _ => m.draw(fb, font, cx, y + m.text, "+ add", DISABLED),
            }
            self.hits.push((row, Hit::AddWatch));
            y += m.line;
        }

        // The breakpoints, with their conditions.
        y += m.pad;
        if !view.breakpoints.is_empty() && y + 2 * m.line <= bottom {
            m.draw(fb, font, cx, y + m.text, "BREAKPOINTS", HEADING);
            y += m.line;
        }
        for (i, (slot, line, condition)) in view.breakpoints.iter().enumerate() {
            if y + m.line > bottom {
                break;
            }
            let row = Rect {
                x: cx - m.pad / 2,
                y,
                w: call_w,
                h: m.line,
            };
            if i % 2 == 1 {
                fill(fb, row, STRIPE);
            }
            fill(
                fb,
                Rect {
                    w: m.pad / 2,
                    ..row
                },
                BREAKS,
            );
            let at = format!("{slot}:{line}");
            m.draw(fb, font, cx, y + m.text, &at, TEXT);
            if let Some(condition) = condition {
                let x = cx + (at.len() as u32 + 1) * m.char;
                let room = call_chars.saturating_sub(at.len() + 1);
                m.draw(fb, font, x, y + m.text, &cut(condition, room), CONDITION);
                if condition.chars().count() > room {
                    self.tips
                        .push((row, vec![format!("{at} when {condition}")]));
                }
            }
            self.hits.push((row, Hit::Breakpoint(i)));
            y += m.line;
        }

        // The variables, under their tabs.
        let vx = cx + call_w + 2 * m.pad;
        let mut tx = vx;
        let ty = inner.y + m.pad;
        let tab = self
            .tab
            .filter(|t| view.tabs.contains(t))
            .or(view.tabs.first().copied());
        for &t in &view.tabs {
            let label = match t {
                Tab::Locals => "Locals".to_string(),
                Tab::Globals(slot) => format!("PRG{slot}"),
                Tab::Perf => "Perf".to_string(),
                Tab::Sprites => "Sprites".to_string(),
                Tab::SubSprites => "Sub sprites".to_string(),
                Tab::Grp => "GRP".to_string(),
                Tab::Bgm => "BGM".to_string(),
            };
            let r = Rect {
                x: tx,
                y: ty,
                w: label.chars().count() as u32 * m.char + m.pad,
                h: m.line - 2,
            };
            fill(fb, r, if Some(t) == tab { HIGHLIGHT } else { TAB });
            m.draw(fb, font, tx + m.pad / 2, ty + m.text - 1, &label, TEXT);
            self.hits.push((r, Hit::Tab(t)));
            tx += r.w + m.pad / 2;
        }
        let mut y = ty + m.line + 2;
        self.rows = ((bottom.saturating_sub(y)) / m.line) as usize;
        let right = inner.x + inner.w - m.pad;
        if tab == Some(Tab::Perf) {
            self.draw_perf(fb, font, &m, (vx, y), (right, bottom), &view.perf);
        } else if let (Some(Tab::Sprites | Tab::SubSprites), Some(list)) = (tab, &view.sprites) {
            // The headings take a row.
            self.rows = self.rows.saturating_sub(1);
            // Scrolled past sprites cleared since, it comes back next frame.
            self.scroll = self.scroll.min(list.total.saturating_sub(self.rows));
            self.draw_sprites(fb, font, &m, (vx, y), (right, bottom), list);
            // The frontend outlines a subprogram's sprite on its screen.
            if let (Some(size), Some(outline), None) = (size, &list.outline, list.sub) {
                let s = Self::screen_place(f, size);
                let place = (s.x.into(), s.y.into(), s.w.into(), s.h.into());
                sprites::draw_outline(fb, outline, size, place, s, m.scale);
            }
        } else if let (Some(Tab::Bgm), Some(bgm)) = (tab, &view.bgm) {
            // The headings take a row.
            self.rows = self.rows.saturating_sub(1);
            // Scrolled past what ended since, it comes back.
            self.scroll = self.scroll.min(bgm.len().saturating_sub(self.rows));
            self.draw_bgm(fb, font, &m, (vx, y), (right, bottom), bgm);
        } else if tab == Some(Tab::Grp) {
            let right = inner.x + inner.w - m.pad;
            let (hits, tips) = (&mut self.hits, &mut self.tips);
            let at = ((vx, y), (right, bottom));
            self.grp
                .draw(fb, font, &m, at.0, at.1, &view.grp, hits, tips);
        } else if view.variables.is_empty() {
            m.draw(fb, font, vx, y + m.text, "No variables", DISABLED);
        }
        // The names and types take what they need, the values the rest.
        let longest = |f: &dyn Fn(&(String, Value)) -> usize| {
            view.variables.iter().map(f).max().unwrap_or(0) as u32
        };
        let name_chars = longest(&|(n, _)| n.chars().count()).clamp(4, NAME_CHARS as u32);
        let type_chars = longest(&|(_, v)| type_text(v).chars().count());
        let value_x = vx + (name_chars + 1) * m.char;
        let type_x = (inner.x + inner.w - m.pad).saturating_sub(type_chars * m.char);
        let value_chars = (type_x.saturating_sub(value_x) / m.char)
            .saturating_sub(1)
            .max(4) as usize;
        let shown = view.variables.iter().enumerate().skip(self.scroll);
        for (i, (name, value)) in shown.take(self.rows) {
            let row = Rect {
                x: vx - m.pad / 2,
                y,
                w: inner.x + inner.w - vx,
                h: m.line,
            };
            if self.selected == Some(Hit::Variable(i)) {
                fill(fb, row, SELECTED);
            } else if i % 2 == 1 {
                fill(fb, row, STRIPE);
            }
            m.draw(
                fb,
                font,
                vx,
                y + m.text,
                &cut(name, name_chars as usize),
                TEXT,
            );
            match &typing {
                Some((Target::Variable(n), typed)) if n == name => {
                    m.draw(fb, font, value_x, y + m.text, &cursor(typed), TEXT)
                }
                _ => {
                    let (text, color) = value_text(value, value_chars, self.hex);
                    m.draw(fb, font, value_x, y + m.text, &text, color);
                    m.draw(fb, font, type_x, y + m.text, &type_text(value), HEADING);
                    let cut_name = name.chars().count() > name_chars as usize;
                    if cut_name || text.ends_with('…') || matches!(value, Value::Array(_)) {
                        let mut lines = vec![format!("{name} =")];
                        lines.extend(tip_lines(value, self.hex));
                        self.tips.push((row, lines));
                    }
                }
            }
            self.hits.push((row, Hit::Variable(i)));
            y += m.line;
        }

        let status = Rect {
            x: cx - m.pad / 2,
            y: bottom,
            w: inner.x + inner.w - cx,
            h: m.line,
        };
        let room = (status.w.saturating_sub(m.pad) / m.char) as usize;
        let reason = view.status.chars().count();
        let color = if view.status_failed { ERROR } else { DISABLED };
        let (x, y) = (status.x + m.pad / 2, status.y + m.text);
        m.draw(fb, font, x, y, &cut(&view.status, room), color);
        if reason > room {
            self.tips.push((status, vec![view.status.clone()]));
        } else if !view.keys.is_empty() && reason + 3 + view.keys.chars().count() <= room {
            let x = x + reason as u32 * m.char;
            m.draw(fb, font, x, y, &format!(" / {}", view.keys), DISABLED);
        }
    }

    /// The measurements PERFBEGIN or PERFEND used since RUN, from `(x,
    /// y)` to `(right, bottom)`: PERFBEGIN's colour, the last, average
    /// and longest times, and how many PERFENDs there were. Times show in
    /// milliseconds too where there is room, else in a tip.
    fn draw_perf(
        &mut self,
        fb: &mut Framebuffer,
        font: &Font,
        m: &Metrics,
        (x, mut y): (u32, u32),
        (right, bottom): (u32, u32),
        perf: &[PerfStats; 8],
    ) {
        let area = Rect {
            x: x - m.pad / 2,
            y,
            w: (right + m.pad / 2).saturating_sub(x),
            h: bottom.saturating_sub(y),
        };
        self.hits.push((area, Hit::Perf));
        let used: Vec<(usize, &PerfStats)> = (perf.iter().enumerate())
            .filter(|(_, s)| s.used())
            .collect();
        if used.is_empty() {
            let text = "No PERFBEGIN or PERFEND since RUN";
            m.draw(fb, font, x, y + m.text, text, DISABLED);
            return;
        }
        let count = |s: &PerfStats| s.count.to_string();
        let count_chars = (used.iter().map(|(_, s)| count(s).len()).max())
            .unwrap_or(0)
            .max("COUNT".len());
        // A swatch and the number take four characters.
        let room = (right.saturating_sub(x) / m.char) as usize;
        let time_chars = room.saturating_sub(4 + count_chars) / 3;
        let widest = |ms| {
            (used.iter())
                .flat_map(|(_, s)| perf_times(s, ms))
                .map(|t| t.len() + 1)
                .max()
                .unwrap_or(0)
        };
        let ms = widest(true) <= time_chars;
        let time_chars = time_chars.max(widest(false));
        let column = |i: usize| x + (4 + i * time_chars) as u32 * m.char;
        let headings = ["LAST", "AVERAGE", "MAX", "COUNT"];
        if y + m.line <= bottom {
            m.draw(fb, font, x + 2 * m.char, y + m.text, "#", HEADING);
            for (i, heading) in headings.iter().enumerate() {
                m.draw(fb, font, column(i), y + m.text, heading, HEADING);
            }
            y += m.line;
        }
        for (n, (id, stats)) in used.into_iter().enumerate() {
            if y + m.line > bottom {
                break;
            }
            let row = Rect {
                y,
                h: m.line,
                ..area
            };
            if n % 2 == 1 {
                fill(fb, row, STRIPE);
            }
            if let Some(color) = stats.color {
                let swatch = Rect {
                    x,
                    y: y + m.text,
                    w: m.char,
                    h: m.line - 2 * m.text,
                };
                // Opaque, so that a colour code without alpha still shows.
                fill(fb, swatch, color | 0xFF00_0000);
            }
            m.draw(fb, font, x + 2 * m.char, y + m.text, &id.to_string(), TEXT);
            let times = perf_times(stats, ms);
            for (i, time) in times.iter().enumerate() {
                m.draw(fb, font, column(i), y + m.text, time, NUMBER);
            }
            m.draw(fb, font, column(3), y + m.text, &count(stats), TEXT);
            if !ms && stats.count > 0 {
                let mut lines = vec![format!("PERFEND {id}")];
                let whole = perf_times(stats, true);
                lines.extend((headings.iter().zip(whole)).map(|(h, t)| format!("{h:<8}{t}")));
                lines.push(format!("{:<8}{}", headings[3], stats.count));
                self.tips.push((row, lines));
            }
            y += m.line;
        }
    }

    /// The sprites in use from `(x, y)` to `(right, bottom)`, a row each
    /// with a thumbnail of its image; the columns that do not fit, and
    /// everything else, are in its tip. A sprite that does not show is
    /// greyed.
    fn draw_sprites(
        &mut self,
        fb: &mut Framebuffer,
        font: &Font,
        m: &Metrics,
        (x, mut y): (u32, u32),
        (right, bottom): (u32, u32),
        list: &sprites::List,
    ) {
        let area = Rect {
            x: x - m.pad / 2,
            y,
            w: (right + m.pad / 2).saturating_sub(x),
            h: bottom.saturating_sub(y),
        };
        if list.total == 0 {
            m.draw(fb, font, x, y + m.text, "No sprites in use", DISABLED);
            self.hits.push((area, Hit::Sprites));
            return;
        }
        // The thumbnail takes two characters after the number.
        let room = (right.saturating_sub(x) / m.char) as usize;
        let shown = sprites::fitting(room, 2);
        let mut columns = Vec::with_capacity(shown);
        let mut at = 0;
        for (i, &(heading, chars, _)) in sprites::COLUMNS[..shown].iter().enumerate() {
            columns.push((x + at as u32 * m.char, heading, chars));
            at += chars + if i == 0 { 2 } else { 0 };
        }
        if y + m.line <= bottom {
            for &(cx, heading, _) in &columns {
                m.draw(fb, font, cx, y + m.text, heading, HEADING);
            }
            y += m.line;
        }
        for (n, row) in list.rows.iter().enumerate() {
            if y + m.line > bottom {
                break;
            }
            let r = Rect {
                y,
                h: m.line,
                ..area
            };
            if self.sprite_in(list.sub.is_some()) == Some(row.id) {
                fill(fb, r, SELECTED);
            } else if (list.first + n) % 2 == 1 {
                fill(fb, r, STRIPE);
            }
            let cells = sprites::cells(row);
            for (i, &(cx, heading, chars)) in columns.iter().enumerate() {
                let color = match (row.shows, i) {
                    (false, _) => DISABLED,
                    (true, 2..=6) => NUMBER,
                    _ => TEXT,
                };
                if heading == "C" {
                    let swatch = Rect {
                        x: cx,
                        y: y + m.text,
                        w: m.char,
                        h: m.line - 2 * m.text,
                    };
                    fill(fb, swatch, row.color | 0xFF00_0000);
                    continue;
                }
                let text = cut(&cells[i], chars - 1);
                m.draw(fb, font, cx, y + m.text, &text, color);
            }
            if let Some(thumb) = &row.thumb {
                let side = m.line - m.scale;
                let (tx, ty) = (x + 5 * m.char, y + m.scale / 2);
                draw_thumb(fb, thumb, (tx, ty), side);
            }
            self.tips.push((r, sprites::details(row, self.hex)));
            self.hits.push((r, Hit::Sprite(row.id)));
            y += m.line;
        }
        // After the rows, which go first.
        self.hits.push((area, Hit::Sprites));
    }

    /// The whole value of the row under the pointer, above it, if the row
    /// cuts it, on `fb` the size of the display.
    pub fn draw_tip(&self, fb: &mut Framebuffer, font: &Font) {
        let m = Metrics::new(self.scale);
        let Some(at) = self.pointer else {
            return;
        };
        let Some((row, lines)) = self.tips.iter().find(|(r, _)| r.contains(at)) else {
            return;
        };
        let chars = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u32;
        let (w, h) = (
            chars * m.char + 2 * m.pad,
            lines.len() as u32 * m.line + m.pad,
        );
        let x = (at.0.max(0) as u32).min(fb.width().saturating_sub(w));
        let y = row.y.saturating_sub(h + 2);
        let tip = Rect { x, y, w, h };
        fill(fb, tip, FACE | 0xFF00_0000);
        crate::ui::outline(fb, tip, BORDER);
        for (i, line) in lines.iter().enumerate() {
            let color = if i == 0 { HEADING } else { TEXT };
            m.draw(
                fb,
                font,
                x + m.pad,
                y + m.pad / 2 + i as u32 * m.line + m.text,
                line,
                color,
            );
        }
    }
}

/// A sprite's thumbnail drawn `side` pixels across at `(x, y)`.
fn draw_thumb(fb: &mut Framebuffer, thumb: &[u32], (x, y): (u32, u32), side: u32) {
    let area = Rect {
        x,
        y,
        w: side,
        h: side,
    };
    if crate::ui::dry(fb, area, ("thumb", area, thumb)) {
        return;
    }
    let n = sprites::THUMB as u32;
    for dy in 0..side {
        let ty = (dy * n / side) as usize;
        for dx in 0..side {
            let p = thumb[ty * sprites::THUMB + (dx * n / side) as usize];
            if p >> 24 != 0
                && let Some(d) = fb.get(x + dx, y + dy)
            {
                fb.set(x + dx, y + dy, sb_runtime::compose::over(d, p));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use sb_core::value::{Array, new_str};

    /// What the panel shows with nothing in it but `tabs`.
    fn view(tabs: Vec<Tab>) -> View {
        View {
            calls: Vec::new(),
            tabs,
            variables: Vec::new(),
            frame: None,
            slot: 0,
            watches: Vec::new(),
            status: String::new(),
            status_failed: false,
            keys: String::new(),
            changed: None,
            breakpoints: Vec::new(),
            perf: [PerfStats::default(); 8],
            sprites: None,
            grp: Probe::default(),
            bgm: None,
        }
    }

    #[test]
    fn the_grp_page_moves_only_while_dragged() {
        let mut p = DebugPanel {
            tab: Some(Tab::Grp),
            ..DebugPanel::default()
        };
        p.grp.zoom = crate::grp_view::Zoom::Times(2);
        let mut fb = Framebuffer::new(1280, 300);
        let f = Rect {
            x: 0,
            y: 0,
            w: 1280,
            h: 300,
        };
        let view = view(vec![Tab::Globals(0), Tab::Grp]);
        p.draw(&mut fb, &Font::blank(), f, Some((400, 240)), &view, 1);
        let (page, _) = *(p.hits.iter())
            .find(|(_, h)| *h == Hit::Grp)
            .expect("the page's view");
        let corner = (page.x as i32, page.y as i32);
        let at = (corner.0 + 100, corner.1 + 50);
        assert_eq!(p.grp.page_at(corner), Some((0, 0)));
        // Hovering moves nothing.
        p.motion(at, false);
        p.motion((at.0 - 40, at.1 - 20), false);
        assert_eq!(p.grp.page_at(corner), Some((0, 0)));
        // Nor does a press elsewhere, as on the tab's own button.
        let (tab, _) = *(p.hits.iter())
            .find(|(_, h)| *h == Hit::Tab(Tab::Grp))
            .expect("the tab");
        let on_tab = (tab.x as i32 + 2, tab.y as i32 + 2);
        assert_eq!(p.click(on_tab, false), Some(Hit::Tab(Tab::Grp)));
        p.motion((on_tab.0 + 40, on_tab.1 + 40), true);
        p.motion(at, true);
        assert_eq!(p.grp.page_at(corner), Some((0, 0)));
        p.release();
        // A press on the page drags it while the button stays down.
        assert_eq!(p.click(at, false), Some(Hit::Grp));
        p.motion((at.0 - 40, at.1 - 20), true);
        assert_eq!(p.grp.page_at(corner), Some((20, 10)));
        p.release();
        p.motion((at.0 - 80, at.1 - 40), false);
        assert_eq!(p.grp.page_at(corner), Some((20, 10)));
        // A button-up that never came, taken by a dialog or lost on the
        // way, ends with the first motion that has the button up.
        p.click(at, false);
        p.motion((at.0 - 40, at.1 - 20), false);
        p.motion((at.0 - 80, at.1 - 40), false);
        assert_eq!(p.grp.page_at(corner), Some((20, 10)));
        // Hidden, the panel forgets the drag too.
        p.click(at, false);
        p.hide();
        p.draw(&mut fb, &Font::blank(), f, Some((400, 240)), &view, 1);
        p.motion((at.0 - 40, at.1 - 20), true);
        assert_eq!(p.grp.page_at(corner), Some((20, 10)));
    }

    #[test]
    fn values_on_one_line() {
        assert_eq!(value_text(&Value::Int(-3), 8, false).0, "-3");
        assert_eq!(value_text(&Value::Real(0.5), 8, false).0, "0.5");
        let s = Value::Str(new_str("HELLO".encode_utf16().collect()));
        assert_eq!(value_text(&s, 16, false).0, "\"HELLO\"");
        assert_eq!(value_text(&s, 5, false).0, "\"HEL…");
        let mut a = Array::new(ElemType::Int, vec![2, 3]);
        a.data = ArrayData::Int(vec![1, 2, 3, 4, 5, 6]);
        let a = Value::Array(Rc::new(RefCell::new(a)));
        assert_eq!(value_text(&a, 16, false).0, "[1,2,3,4,5,6]");
        assert_eq!(value_text(&a, 8, false).0, "[1,2,3,…");
        assert_eq!(type_text(&a), "INT[2,3]");
        assert_eq!(value_text(&Value::Int(-1), 12, true).0, "&HFFFFFFFF");
    }

    #[test]
    fn perf_times_in_both_units() {
        assert_eq!(perf_time(16_667, true), "16667us 16.667ms");
        assert_eq!(perf_time(5, true), "5us 0.005ms");
        assert_eq!(perf_time(5, false), "5us");
        let begun = PerfStats {
            begun: true,
            ..PerfStats::default()
        };
        assert_eq!(perf_times(&begun, true), ["-", "-", "-"]);
        let timed = PerfStats {
            count: 2,
            last: 1000,
            max: 3000,
            total: 4000,
            ..begun
        };
        assert_eq!(perf_times(&timed, false), ["1000us", "2000us", "3000us"]);
    }

    #[test]
    fn sprites_tab_chooses_and_outlines_a_sprite() {
        use sb_runtime::machine::Machine;
        let mut m = Machine::new(sb_runtime::Runtime::new(400, 240, Font::blank()));
        m.boot();
        m.set_slot(0, "SPSET 2,0:SPOFS 2,100,100:SPSET 9,0".to_string());
        m.run_slot(0);
        m.frame();
        let mut p = DebugPanel {
            tab: Some(Tab::Sprites),
            sprite: Some(2),
            ..DebugPanel::default()
        };
        let (first, rows) = p.sprite_rows();
        let view = View {
            sprites: Some(sprites::list(&m.rt, first, rows, p.sprite)),
            ..view(vec![Tab::Globals(0), Tab::Perf, Tab::Sprites])
        };
        assert_eq!(view.scrolled(), 3);
        let mut fb = Framebuffer::new(1280, 300);
        let f = Rect {
            x: 0,
            y: 0,
            w: 1280,
            h: 300,
        };
        p.draw(&mut fb, &Font::blank(), f, Some((400, 240)), &view, 2);
        // The outline is on the screens' place, around (100, 100) of 400.
        let s = DebugPanel::screen_place(f, (400, 240));
        let (x, y) = (s.x + 100 * s.w / 400, s.y + 100 * s.h / 240);
        let lit = |x: u32, y: u32| fb.get(x, y).is_some_and(|c| c >> 24 != 0);
        assert!(lit(x + 2, y) && !lit(x + 8, y + 8));
        // Rows under the headings, and the tab's own menu below them.
        let rows: Vec<(Rect, Hit)> = (p.hits.iter())
            .filter(|(_, h)| matches!(h, Hit::Sprite(_)))
            .copied()
            .collect();
        assert_eq!(rows.len(), 3);
        let (r, hit) = rows[1];
        assert_eq!(hit, Hit::Sprite(9));
        let at = ((r.x + 4) as i32, (r.y + 4) as i32);
        assert_eq!(p.click(at, false), Some(Hit::Sprite(9)));
        assert_eq!(p.sprite, Some(9));
        p.click(at, false);
        assert_eq!(p.sprite, None, "a second click lets go");
        let below = ((r.x + 4) as i32, (rows[2].0.y + r.h + 4) as i32);
        assert_eq!(p.hit_at(below), Some(Hit::Sprites));
        // A new stop keeps the tab.
        p.reset();
        assert_eq!(p.tab, Some(Tab::Sprites));
        p.tab = Some(Tab::Locals);
        p.reset();
        assert_eq!(p.tab, None);
    }

    #[test]
    fn sub_sprites_keep_their_own_choice_and_screen() {
        use sb_runtime::machine::Machine;
        let mut m = Machine::new(sb_runtime::Runtime::new(400, 240, Font::blank()));
        m.boot();
        m.set_slot(0, "SPSET 2,0:SPOFS 2,100,100:SPSET 9,0".to_string());
        m.run_slot(0);
        m.frame();
        let mut p = DebugPanel {
            tab: Some(Tab::SubSprites),
            sprite: Some(9),
            sub_sprite: Some(2),
            ..DebugPanel::default()
        };
        assert!(p.sub_tab());
        let (first, rows) = p.sprite_rows();
        let list = sprites::list(&m.rt, first, rows, p.sub_sprite);
        assert!(list.outline.is_some());
        let sub = Some(sprites::SubScreen {
            place: Some([10, 20, 300, 200]),
            frame: (400, 240),
        });
        let view = View {
            sprites: Some(sprites::List { sub, ..list }),
            ..view(vec![Tab::Globals(0), Tab::Sprites, Tab::SubSprites])
        };
        let mut fb = Framebuffer::new(1280, 300);
        let f = Rect {
            x: 0,
            y: 0,
            w: 1280,
            h: 300,
        };
        p.draw(&mut fb, &Font::blank(), f, Some((400, 240)), &view, 2);
        // Not outlined on the main screens: the frontend does it on the
        // subprogram's.
        let s = DebugPanel::screen_place(f, (400, 240));
        let (x, y) = (s.x + 100 * s.w / 400, s.y + 100 * s.h / 240);
        assert_eq!(fb.get(x + 2, y), Some(0));
        let rows: Vec<(Rect, Hit)> = (p.hits.iter())
            .filter(|(_, h)| matches!(h, Hit::Sprite(_)))
            .copied()
            .collect();
        assert_eq!(rows.len(), 3);
        let (r, hit) = rows[1];
        assert_eq!(hit, Hit::Sprite(9));
        p.click(((r.x + 4) as i32, (r.y + 4) as i32), false);
        assert_eq!((p.sub_sprite, p.sprite), (Some(9), Some(9)));
        p.click(((r.x + 4) as i32, (r.y + 4) as i32), false);
        assert_eq!((p.sub_sprite, p.sprite), (None, Some(9)));
        // The main tab's choice is its own.
        p.tab = Some(Tab::Sprites);
        assert!(!p.sub_tab());
        p.choose_sprite(false, None);
        p.choose_sprite(true, Some(2));
        assert_eq!((p.sub_sprite, p.sprite), (Some(2), None));
        // A new stop keeps the tab.
        p.tab = Some(Tab::SubSprites);
        p.reset();
        assert_eq!(p.tab, Some(Tab::SubSprites));
    }

    #[test]
    fn typing_watches_and_values() {
        let mut p = DebugPanel::default();
        p.add_watch(String::new());
        p.text("A*2");
        assert_eq!(p.key(Keycode::Return), Typed::Stay);
        assert_eq!(p.watches, ["A*2"]);
        p.edit_watch(0);
        p.key(Keycode::Backspace);
        p.text("3");
        p.key(Keycode::Return);
        assert_eq!(p.watches, ["A*3"]);
        // A watch that breaks goes on breaking once edited.
        p.toggle_break(0);
        assert!(p.breaks(0));
        p.edit_watch(0);
        p.text("+1");
        p.key(Keycode::Return);
        assert_eq!(p.change_watches(), ["A*3+1"]);
        p.toggle_break(0);
        assert!(!p.breaks(0));
        p.edit_watch(0);
        p.key(Keycode::Backspace);
        p.key(Keycode::Backspace);
        p.key(Keycode::Return);
        // Emptied, a watch goes.
        p.edit_watch(0);
        for _ in 0..3 {
            p.key(Keycode::Backspace);
        }
        p.key(Keycode::Return);
        assert!(p.watches.is_empty());
        p.edit_variable("X".to_string(), "1".to_string());
        p.text("0");
        let typed = p.key(Keycode::Return);
        assert_eq!(typed, Typed::Value("X".to_string(), "10".to_string()));
        p.add_watch("B".to_string());
        p.key(Keycode::Escape);
        assert!(!p.typing() && p.watches.is_empty());
    }

    /// A panel with variables, one of them too long for its row.
    fn busy_view(last: i32) -> View {
        let long = Value::Str(new_str(
            "A LONG STRING ".repeat(20).encode_utf16().collect(),
        ));
        View {
            variables: vec![
                ("COUNT".to_string(), Value::Int(last)),
                ("NAME$".to_string(), long),
                ("SPEED#".to_string(), Value::Real(1.5)),
            ],
            status: "Running".to_string(),
            ..view(vec![Tab::Globals(0), Tab::Perf])
        }
    }

    #[test]
    fn drawn_on_its_own_the_panel_looks_as_on_the_display() {
        let font = Font::fallback();
        let f = Rect {
            x: 0,
            y: 420,
            w: 1280,
            h: 300,
        };
        let view = busy_view(7);
        // As it was drawn before: on the display's overlay, the tip last.
        let mut before = DebugPanel::default();
        let mut display = Framebuffer::new(1280, 720);
        display.clear(0);
        before.draw_at(&mut display, &font, f, Some((400, 240)), &view, 2);
        let (row, _) = before.tips[0].clone();
        let at = (row.x as i32 + 4, row.y as i32 + 4);
        before.motion(at, false);
        before.draw_at(&mut display, &font, f, Some((400, 240)), &view, 2);
        before.draw_tip(&mut display, &font);

        let mut after = DebugPanel::default();
        after.motion(at, false);
        let mut panel = Framebuffer::new(f.w, f.h);
        let mut overlay = Framebuffer::new(1280, 720);
        overlay.clear(0);
        after.draw(&mut panel, &font, f, Some((400, 240)), &view, 2);
        after.draw_tip(&mut overlay, &font);
        assert_eq!(after.hits, before.hits);
        assert_eq!(after.frame, before.frame);
        // The GRP tab's page goes where it went.
        let grp = View {
            tabs: vec![Tab::Globals(0), Tab::Grp],
            ..busy_view(7)
        };
        (before.tab, after.tab) = (Some(Tab::Grp), Some(Tab::Grp));
        before.draw_at(&mut display, &font, f, Some((400, 240)), &grp, 2);
        after.draw(&mut panel, &font, f, Some((400, 240)), &grp, 2);
        assert!(before.grp.shown().is_some());
        assert_eq!(after.grp.shown(), before.grp.shown());
        assert_eq!(after.hits, before.hits);
        // Back to the variables, whose tip is checked below.
        let (mut display, mut panel) = (Framebuffer::new(1280, 720), Framebuffer::new(f.w, f.h));
        display.clear(0);
        (before.tab, after.tab) = (None, None);
        before.draw_at(&mut display, &font, f, Some((400, 240)), &view, 2);
        before.draw_tip(&mut display, &font);
        after.draw(&mut panel, &font, f, Some((400, 240)), &view, 2);
        // The overlay goes over the panel, opaque where it is drawn.
        for y in 0..720 {
            for x in 0..1280 {
                let over = overlay.get(x, y).unwrap();
                let under = match y.checked_sub(f.y) {
                    Some(py) => panel.get(x, py).unwrap(),
                    None => 0,
                };
                let shown = if over >> 24 == 0 { under } else { over };
                assert_eq!(shown, display.get(x, y).unwrap(), "at {x},{y}");
            }
        }
    }

    #[test]
    fn a_dry_run_draws_nothing_and_tells_what_changed() {
        let font = Font::fallback();
        let f = Rect {
            x: 0,
            y: 0,
            w: 1280,
            h: 300,
        };
        let mut p = DebugPanel::default();
        let mut fb = Framebuffer::new(f.w, f.h);
        fb.clear(0);
        let key = |p: &mut DebugPanel, view: &View, fb: &mut Framebuffer| {
            crate::ui::dry_run(|| p.draw(fb, &font, f, None, view, 2))
        };
        let first = key(&mut p, &busy_view(7), &mut fb);
        assert!(fb.pixels().iter().all(|&c| c == 0));
        assert_eq!((first.bounds, first.translucent), (Some(f), None));
        assert_eq!(key(&mut p, &busy_view(7), &mut fb).key, first.key);
        assert_ne!(key(&mut p, &busy_view(8), &mut fb).key, first.key);
        // The pointer changes nothing while it shows no tip.
        p.motion((600, 10), false);
        assert_eq!(key(&mut p, &busy_view(7), &mut fb).key, first.key);
    }
}

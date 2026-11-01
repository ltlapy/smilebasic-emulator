//! The debugger's panel below the editor while a program is stopped: room
//! for the screens as the program left them, which the frontend draws at
//! the window's resolution, its calls under way, the watch expressions,
//! and the variables of the call chosen or of a slot.

use sb_core::value::{ArrayData, ElemType, Value, format_number};
use sb_core::vm::{Call, CallKind};
use sb_runtime::Framebuffer;
use sb_runtime::font::Font;
use sdl3::keyboard::Keycode;

use crate::ui::{BORDER, DISABLED, HIGHLIGHT, PAD, Rect, TEXT, draw_text_scaled, fill};

const HEADING: u32 = 0xFF80_8080;
const NUMBER: u32 = 0xFFF0_4090;
const STRING: u32 = 0xFF60_80F0;
const FACE: u32 = 0xFF18_1818;
const TAB: u32 = 0xFF40_4040;
const ERROR: u32 = 0xFFF0_6060;
const SELECTED: u32 = 0xFF30_3850;
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
    /// A line at the bottom: where the program stopped, or that a step
    /// runs.
    pub status: String,
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
}

/// The panel's character width, row height, padding and text offset in a
/// row, for the font drawn `scale` times its size.
struct Metrics {
    char: u32,
    line: u32,
    pad: u32,
    text: u32,
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

    fn draw(&self, fb: &mut Framebuffer, font: &Font, x: u32, y: u32, text: &str, color: u32) {
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
fn cut(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(chars.saturating_sub(1)).collect();
    cut.push('…');
    cut
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
    /// stop.
    pub fn reset(&mut self) {
        self.chosen = None;
        self.tab = None;
        self.scroll = 0;
    }

    /// Forgets where it was drawn and what was typed, once it is not
    /// shown.
    pub fn hide(&mut self) {
        self.frame = None;
        self.hits.clear();
        self.tips.clear();
        self.typing = None;
    }

    /// The pointer moved, or left the window with `None`.
    pub fn pointer(&mut self, at: Option<(i32, i32)>) {
        self.pointer = at;
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
            Hit::Close => {}
            Hit::Watch(i) if double => self.edit_watch(i),
            Hit::Watch(_) | Hit::Variable(_) => {}
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
        }
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
                    Target::Watch(i) => self.watches[i] = typed,
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

    pub fn wheel(&mut self, rows: i32, variables: usize) {
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

    /// Draws the panel into `f`, leaving the place of screens of `size`
    /// clear for the frontend to draw them.
    pub fn draw(
        &mut self,
        fb: &mut Framebuffer,
        font: &Font,
        f: Rect,
        size: (u32, u32),
        view: &View,
        scale: u32,
    ) {
        self.scale = scale;
        let m = Metrics::new(scale);
        self.frame = Some(f);
        self.hits.clear();
        self.tips.clear();
        fill(fb, f, BORDER);
        let inner = Rect {
            x: f.x,
            y: f.y + 2,
            w: f.w,
            h: f.h - 2,
        };
        fill(fb, inner, FACE | 0xFF00_0000);

        let screen = Self::screen_place(f, size);
        fill(fb, screen, 0);
        let frame = Rect {
            x: screen.x - 2,
            y: screen.y - 2,
            w: screen.w + 4,
            h: screen.h + 4,
        };
        crate::ui::outline(fb, frame, BORDER);
        let (x0, w) = (screen.x, screen.w);

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
        if view.variables.is_empty() {
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
        m.draw(
            fb,
            font,
            status.x + m.pad / 2,
            status.y + m.text,
            &view.status,
            DISABLED,
        );
        self.draw_tip(fb, font);
    }

    /// The whole value of the row under the pointer, above it, if the row
    /// cuts it.
    fn draw_tip(&self, fb: &mut Framebuffer, font: &Font) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use sb_core::value::{Array, new_str};

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
}

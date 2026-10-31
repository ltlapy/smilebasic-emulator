//! The debugger's panel below the editor while a program is stopped: the
//! screens as the program left them, its calls under way and the
//! variables of the one chosen.

use sb_core::value::{ArrayData, ElemType, Value, format_number};
use sb_core::vm::{Call, CallKind};
use sb_runtime::Framebuffer;
use sb_runtime::font::Font;

use crate::ui::{BORDER, CHAR, DISABLED, HIGHLIGHT, LINE, PAD, Rect, TEXT, draw_text, fill};

const HEADING: u32 = 0xFF80_8080;
const NUMBER: u32 = 0xFFF0_4090;
const STRING: u32 = 0xFF60_80F0;
const FACE: u32 = 0xFF18_1818;
const TAB: u32 = 0xFF40_4040;
/// Characters of the call stack's column, and of the variables' names
/// and values.
const CALL_CHARS: u32 = 18;
const NAME_CHARS: usize = 8;
const VALUE_CHARS: usize = 16;

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
}

/// A value on one line: numbers as PRINT shows them, strings in quotes,
/// arrays with their first elements, cut to `chars`.
fn value_text(value: &Value, chars: usize) -> (String, u32) {
    let quoted = |s: &[u16]| format!("\"{}\"", String::from_utf16_lossy(s));
    let (text, color) = match value {
        Value::Default => ("(none)".to_string(), DISABLED),
        Value::Int(_) | Value::Real(_) => (String::from_utf16_lossy(&format_number(value)), NUMBER),
        Value::Str(s) => (quoted(&s.borrow()), STRING),
        Value::Array(a) => {
            let a = a.borrow();
            let items: Vec<String> = match &a.data {
                ArrayData::Int(v) => v.iter().take(chars).map(i32::to_string).collect(),
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

/// A value's type as TYPEOF tells them apart, with an array's sizes.
fn type_text(value: &Value) -> String {
    let elem = |e: ElemType| match e {
        ElemType::Int => "INT",
        ElemType::Real => "REAL",
        ElemType::Str => "STRING",
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

fn call_text(call: &Call) -> String {
    let name = match &call.kind {
        CallKind::Top => "(top level)",
        CallKind::Def(name) => name,
        CallKind::Gosub => "GOSUB",
    };
    let at = format!(" {}:{}", call.slot, call.line);
    let room = (CALL_CHARS as usize).saturating_sub(at.chars().count());
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

    /// Forgets where it was drawn, once it is not shown.
    pub fn hide(&mut self) {
        self.frame = None;
        self.hits.clear();
    }

    pub fn contains(&self, at: (i32, i32)) -> bool {
        self.frame.is_some_and(|f| f.contains(at))
    }

    pub fn click(&mut self, at: (i32, i32)) -> Option<Hit> {
        let hit = (self.hits.iter())
            .find(|(r, _)| r.contains(at))
            .map(|&(_, hit)| hit)?;
        match hit {
            Hit::Call(i) => {
                self.chosen = Some(i);
                self.tab = None;
            }
            Hit::Tab(tab) => self.tab = Some(tab),
        }
        self.scroll = 0;
        Some(hit)
    }

    pub fn wheel(&mut self, rows: i32, variables: usize) {
        let most = variables.saturating_sub(self.rows);
        self.scroll = (self.scroll as i64 + i64::from(rows)).clamp(0, most as i64) as usize;
    }

    /// Draws the panel into `f`, with the screens scaled down to fit on
    /// its left.
    pub fn draw(
        &mut self,
        fb: &mut Framebuffer,
        font: &Font,
        f: Rect,
        screen: &Framebuffer,
        view: &View,
    ) {
        self.frame = Some(f);
        self.hits.clear();
        fill(fb, f, BORDER);
        let inner = Rect {
            x: f.x,
            y: f.y + 2,
            w: f.w,
            h: f.h - 2,
        };
        fill(fb, inner, FACE | 0xFF00_0000);

        // The screens, at a whole fraction of their size if they are
        // taller than the room.
        let room = inner.h - PAD;
        let (sw, sh) = (screen.width().max(1), screen.height().max(1));
        let down = sh.div_ceil(room).max(1);
        let (w, h) = (sw / down, sh / down);
        let (x0, y0) = (inner.x + PAD, inner.y + PAD / 2 + (room - h) / 2);
        for y in 0..h {
            for x in 0..w {
                let c = screen.get(x * down, y * down).unwrap_or(0xFF00_0000);
                fb.set(x0 + x, y0 + y, c | 0xFF00_0000);
            }
        }
        let frame = Rect {
            x: x0 - 1,
            y: y0 - 1,
            w: w + 2,
            h: h + 2,
        };
        crate::ui::outline(fb, frame, BORDER);

        // The calls, innermost last as BACKTRACE lists them reversed.
        let cx = x0 + w + 2 * PAD;
        let mut y = inner.y + PAD;
        draw_text(fb, font, cx, y + 4, "CALLS", HEADING);
        y += LINE;
        let chosen = self.chosen_call(view.calls.len());
        let call_w = CALL_CHARS * CHAR + PAD;
        let bottom = inner.y + inner.h - LINE;
        for (i, call) in view.calls.iter().enumerate().rev() {
            if y + LINE > bottom {
                break;
            }
            let row = Rect {
                x: cx - PAD / 2,
                y,
                w: call_w,
                h: LINE,
            };
            if i == chosen {
                fill(fb, row, HIGHLIGHT);
            }
            draw_text(fb, font, cx, y + 4, &call_text(call), TEXT);
            self.hits.push((row, Hit::Call(i)));
            y += LINE;
        }

        // The variables, under their tabs.
        let vx = cx + call_w + 2 * PAD;
        let mut tx = vx;
        let ty = inner.y + PAD;
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
                w: crate::ui::text_width(&label) + PAD,
                h: LINE - 2,
            };
            fill(fb, r, if Some(t) == tab { HIGHLIGHT } else { TAB });
            draw_text(fb, font, tx + PAD / 2, ty + 3, &label, TEXT);
            self.hits.push((r, Hit::Tab(t)));
            tx += r.w + PAD / 2;
        }
        let mut y = ty + LINE + 2;
        self.rows = ((bottom.saturating_sub(y)) / LINE) as usize;
        if view.variables.is_empty() {
            draw_text(fb, font, vx, y + 4, "No variables", DISABLED);
        }
        let value_x = vx + (NAME_CHARS as u32 + 1) * CHAR;
        let type_x = value_x + (VALUE_CHARS as u32 + 1) * CHAR;
        for (name, value) in view.variables.iter().skip(self.scroll).take(self.rows) {
            draw_text(fb, font, vx, y + 4, &cut(name, NAME_CHARS), TEXT);
            let (text, color) = value_text(value, VALUE_CHARS);
            draw_text(fb, font, value_x, y + 4, &text, color);
            draw_text(fb, font, type_x, y + 4, &type_text(value), HEADING);
            y += LINE;
        }

        let status = Rect {
            x: cx - PAD / 2,
            y: bottom,
            w: inner.x + inner.w - cx,
            h: LINE,
        };
        draw_text(
            fb,
            font,
            status.x + PAD / 2,
            status.y + 4,
            &view.status,
            DISABLED,
        );
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
        assert_eq!(value_text(&Value::Int(-3), 8).0, "-3");
        assert_eq!(value_text(&Value::Real(0.5), 8).0, "0.5");
        let s = Value::Str(new_str("HELLO".encode_utf16().collect()));
        assert_eq!(value_text(&s, 16).0, "\"HELLO\"");
        assert_eq!(value_text(&s, 5).0, "\"HEL…");
        let mut a = Array::new(ElemType::Int, vec![2, 3]);
        a.data = ArrayData::Int(vec![1, 2, 3, 4, 5, 6]);
        let a = Value::Array(Rc::new(RefCell::new(a)));
        assert_eq!(value_text(&a, 16).0, "[1,2,3,4,5,6]");
        assert_eq!(value_text(&a, 8).0, "[1,2,3,…");
        assert_eq!(type_text(&a), "INT[2,3]");
    }
}

//! The projects window's viewer, over the whole window: a GRP or JPG
//! file as large as fits, a TXT file's text, or a DAT file's values as a
//! table, all read-only.

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;
use sb_runtime::sbfile::Data;
use sb_runtime::storage::{FileKind, Storage};
use sdl3::gamepad::Button;
use sdl3::keyboard::Keycode;

use crate::preview::{self, Content, Picture};
use crate::ui::{
    BORDER, CHAR, DISABLED, HIGHLIGHT, LINE, Mark, PAD, Rect, TEXT, draw_text, fill, image, mark,
    outline, window,
};

/// The color around a picture and under its transparent pixels.
const BACKDROP: u32 = 0xFF10_1010;
/// The maximum number of values in a table. A larger array shows its
/// first values.
const MOST_VALUES: usize = 100_000;
/// The maximum width of a table cell, in characters.
const MOST_CELL: usize = 16;
const ERROR: u32 = 0xFFFF_8080;

enum Body {
    Picture(Picture),
    Text(Vec<String>),
    Table(Table),
    Failed(String),
}

struct Table {
    /// The columns' headings.
    head: Vec<String>,
    /// Each row's label, then its cells.
    rows: Vec<(String, Vec<String>)>,
    /// The width of a cell, in characters.
    cell: usize,
    /// The width of the labels, in characters.
    label: usize,
}

pub struct Viewer {
    frame: Rect,
    title: String,
    body: Body,
    /// The first row shown, and the first column (a character of text,
    /// or a table's column).
    top: usize,
    left: usize,
    mouse: Option<(i32, i32)>,
}

/// Returns the table of the values of `data`. A 2D array gives its rows
/// and columns. With more dimensions, each row holds the values along the
/// last index. With one dimension, a row has as many columns as fit in
/// `chars`, and each row is labeled by the index it starts at.
fn table(data: &Data, chars: usize) -> Table {
    let count = preview::value_count(data).min(MOST_VALUES);
    let cells: Vec<String> = (0..count)
        .map(|i| {
            let text = preview::value_text(data, i);
            match text.chars().count() > MOST_CELL {
                true => text.chars().take(MOST_CELL - 1).chain(['~']).collect(),
                false => text,
            }
        })
        .collect();
    let mut cell = cells
        .iter()
        .map(|c| c.chars().count())
        .max()
        .unwrap_or(1)
        .max(2)
        + 1;
    let digits = |n: usize| n.max(1).to_string().len();
    let (columns, label_of): (usize, Box<dyn Fn(usize) -> String>) = match &data.dims[..] {
        [_] => {
            let label = digits(count.saturating_sub(1)) + 1;
            // A round number of values per row, so that the labels count up
            // evenly.
            let fit = (chars.saturating_sub(label) / cell).max(1);
            let columns = [64, 32, 16, 10, 8, 4, 2, 1]
                .into_iter()
                .find(|&n| n <= fit)
                .unwrap_or(1);
            (columns, Box::new(move |row| (row * columns).to_string()))
        }
        dims => {
            let columns = dims.last().copied().unwrap_or(1).max(1);
            cell = cell.max(digits(columns - 1) + 1);
            let outer = dims[..dims.len() - 1].to_vec();
            (
                columns,
                Box::new(move |row| {
                    // The row's indexes, outermost first.
                    let mut rest = row;
                    let mut at = vec![0; outer.len()];
                    for (i, &d) in outer.iter().enumerate().rev() {
                        at[i] = rest % d.max(1);
                        rest /= d.max(1);
                    }
                    let at: Vec<String> = at.iter().map(usize::to_string).collect();
                    at.join(",")
                }),
            )
        }
    };
    let rows: Vec<(String, Vec<String>)> = cells
        .chunks(columns)
        .enumerate()
        .map(|(row, cells)| (label_of(row), cells.to_vec()))
        .collect();
    let label = rows.iter().map(|r| r.0.chars().count()).max().unwrap_or(1) + 1;
    let head = (0..columns)
        .map(|c| match data.dims.len() {
            1 => format!("+{c}"),
            _ => c.to_string(),
        })
        .collect();
    Table {
        head,
        rows,
        cell,
        label,
    }
}

impl Viewer {
    /// Reads a file to show it over `frame`.
    pub fn open(
        frame: Rect,
        storage: &Storage,
        project: &str,
        kind: FileKind,
        name: &str,
    ) -> Viewer {
        let mut viewer = Viewer {
            frame,
            title: format!("{}:{name}", kind.name()),
            body: Body::Failed(String::new()),
            top: 0,
            left: 0,
            mouse: None,
        };
        let room = viewer.body_rect();
        viewer.body = match preview::read(storage, project, kind, name) {
            Content::Image { w, h, pixels } => {
                viewer.title.push_str(&format!(" ({w}x{h})"));
                Body::Picture(Picture::fitted(&pixels, (w, h), (room.w, room.h)))
            }
            Content::Text { lines, count } => {
                viewer.title.push_str(&format!(" ({count} lines)"));
                Body::Text(lines)
            }
            Content::Data(data) => {
                let count = preview::value_count(&data);
                let mut about = format!(
                    " ({} {})",
                    preview::element_type(&data),
                    preview::dims_text(&data)
                );
                if count > MOST_VALUES {
                    about.push_str(&format!(", the first {MOST_VALUES} values"));
                }
                viewer.title.push_str(&about);
                let chars = room.w.saturating_sub(2 * PAD) / CHAR;
                Body::Table(table(&data, chars as usize))
            }
            Content::Failed(e) => Body::Failed(e),
        };
        viewer
    }

    fn body_rect(&self) -> Rect {
        let f = self.frame;
        Rect {
            x: f.x + 2 * PAD,
            y: f.y + LINE + 2 * PAD,
            w: f.w - 4 * PAD,
            h: f.h - LINE - 4 * PAD,
        }
    }

    fn close_rect(&self) -> Rect {
        let f = self.frame;
        Rect {
            x: f.x + f.w - 2 - LINE,
            y: f.y + 2,
            w: LINE,
            h: LINE,
        }
    }

    /// The number of rows that fit, under the headings for a table.
    fn rows_shown(&self) -> usize {
        let rows = (self.body_rect().h / LINE) as usize;
        match self.body {
            Body::Table(_) => rows.saturating_sub(1),
            _ => rows,
        }
    }

    fn row_count(&self) -> usize {
        match &self.body {
            Body::Text(lines) => lines.len(),
            Body::Table(t) => t.rows.len(),
            _ => 0,
        }
    }

    /// The maximum number of columns to scroll past.
    fn column_count(&self) -> usize {
        let chars = (self.body_rect().w.saturating_sub(2 * PAD) / CHAR) as usize;
        match &self.body {
            Body::Text(lines) => {
                let widest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
                (widest + self.number_width() + 1).saturating_sub(chars)
            }
            Body::Table(t) => {
                let fit = (chars.saturating_sub(t.label) / t.cell).max(1);
                t.head.len().saturating_sub(fit)
            }
            _ => 0,
        }
    }

    fn number_width(&self) -> usize {
        self.row_count().max(1).to_string().len()
    }

    fn scroll(&mut self, rows: isize, columns: isize) {
        let last = self.row_count().saturating_sub(self.rows_shown());
        self.top = self.top.saturating_add_signed(rows).min(last);
        self.left = (self.left.saturating_add_signed(columns)).min(self.column_count());
    }

    /// Returns true when the key closes the viewer.
    pub fn key(&mut self, key: Keycode) -> bool {
        let page = self.rows_shown().saturating_sub(1).max(1) as isize;
        let step = match self.body {
            Body::Text(_) => 8,
            _ => 1,
        };
        match key {
            Keycode::Escape | Keycode::Return | Keycode::KpEnter | Keycode::Space => return true,
            Keycode::Up => self.scroll(-1, 0),
            Keycode::Down => self.scroll(1, 0),
            Keycode::PageUp => self.scroll(-page, 0),
            Keycode::PageDown => self.scroll(page, 0),
            Keycode::Home => self.scroll(isize::MIN / 2, isize::MIN / 2),
            Keycode::End => self.scroll(isize::MAX / 2, 0),
            Keycode::Left => self.scroll(0, -step),
            Keycode::Right => self.scroll(0, step),
            _ => {}
        }
        false
    }

    /// Handles a controller button: the D-pad scrolls, the shoulder buttons
    /// scroll by pages, and A or B closes. Returns true when it closes.
    pub fn pad(&mut self, button: Button) -> bool {
        let key = match button {
            Button::DPadUp => Keycode::Up,
            Button::DPadDown => Keycode::Down,
            Button::DPadLeft => Keycode::Left,
            Button::DPadRight => Keycode::Right,
            Button::LeftShoulder => Keycode::PageUp,
            Button::RightShoulder => Keycode::PageDown,
            Button::East | Button::South => return true,
            _ => return false,
        };
        self.key(key)
    }

    pub fn wheel(&mut self, lines: i32) {
        self.scroll(lines as isize, 0);
    }

    pub fn mouse_move(&mut self, at: (i32, i32)) {
        self.mouse = Some(at);
    }

    /// Returns true when the click closes the viewer.
    pub fn click(&mut self, at: (i32, i32)) -> bool {
        self.close_rect().contains(at)
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        let f = self.frame;
        window(fb, font, f, &self.title);
        let close = self.close_rect();
        if self.mouse.is_some_and(|m| close.contains(m)) {
            fill(fb, close, BORDER);
        }
        mark(fb, Mark::Close, close.x + 4, close.y + 4, TEXT);
        let body = self.body_rect();
        // Text keeps clear of the outline.
        let left = body.x + PAD;
        let chars = (body.w.saturating_sub(2 * PAD) / CHAR) as usize;
        let line_y = |row: usize| body.y + row as u32 * LINE + 4;
        match &self.body {
            Body::Picture(p) => {
                fill(fb, body, BACKDROP);
                let x = body.x + (body.w - p.w) / 2;
                let y = body.y + (body.h - p.h) / 2;
                image(fb, x, y, p.w, &p.pixels, p.key);
            }
            Body::Text(lines) => {
                let numbers = self.number_width();
                let room = chars.saturating_sub(numbers + 1);
                let shown = lines
                    .iter()
                    .enumerate()
                    .skip(self.top)
                    .take(self.rows_shown());
                for (row, (i, line)) in shown.enumerate() {
                    let y = line_y(row);
                    let number = format!("{:>numbers$}", i + 1);
                    draw_text(fb, font, left, y, &number, DISABLED);
                    let text: String = line.chars().skip(self.left).take(room).collect();
                    let x = left + (numbers as u32 + 1) * CHAR;
                    draw_text(fb, font, x, y, &text, TEXT);
                }
            }
            Body::Table(t) => {
                let fit = (chars.saturating_sub(t.label) / t.cell).max(1);
                let columns = self.left..t.head.len().min(self.left + fit);
                let x_of = |c: usize| left + (t.label + (c - self.left) * t.cell) as u32 * CHAR;
                fill(fb, Rect { h: LINE, ..body }, HIGHLIGHT);
                for c in columns.clone() {
                    draw_text(fb, font, x_of(c), line_y(0), &t.head[c], TEXT);
                }
                let shown = t.rows.iter().skip(self.top).take(self.rows_shown());
                for (row, (label, cells)) in shown.enumerate() {
                    let y = line_y(row + 1);
                    draw_text(fb, font, left, y, label, DISABLED);
                    for c in columns.clone() {
                        if let Some(cell) = cells.get(c) {
                            draw_text(fb, font, x_of(c), y, cell, TEXT);
                        }
                    }
                }
            }
            Body::Failed(e) => draw_text(fb, font, left, line_y(0), e, ERROR),
        }
        outline(fb, body, BORDER);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_runtime::sbfile::DataValues;

    #[test]
    fn tables_follow_the_dimensions() {
        let data = Data {
            dims: vec![2, 3],
            values: DataValues::Int(vec![1, 2, 3, 4, 5, 6]),
        };
        let t = table(&data, 40);
        assert_eq!(t.head, ["0", "1", "2"]);
        assert_eq!(
            t.rows[1],
            ("1".to_string(), vec!["4".into(), "5".into(), "6".into()])
        );
        let data = Data {
            dims: vec![2, 2, 2],
            values: DataValues::Real(vec![0.5; 8]),
        };
        let t = table(&data, 40);
        let labels: Vec<&str> = t.rows.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(labels, ["0,0", "0,1", "1,0", "1,1"]);
        assert_eq!(t.rows[0].1, ["0.5", "0.5"]);
        // One dimension wraps into as many columns as fit.
        let data = Data {
            dims: vec![100],
            values: DataValues::Int((0..100).collect()),
        };
        let t = table(&data, 3 + 11 * 3);
        assert_eq!(t.head.len(), 10, "a round number");
        assert_eq!(t.rows.len(), 10);
        assert_eq!(t.rows[3].0, "30");
        assert_eq!(t.rows[3].1[2], "32");
    }

    #[test]
    fn views_scroll_and_close() {
        let dir = std::env::temp_dir().join(format!("sb-view-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let s = Storage::new(dir.clone());
        let text: String = (0..100).map(|i| format!("?{i}\n")).collect();
        s.write(FileKind::Txt, "A.PRG", text.as_bytes(), None)
            .unwrap();
        let frame = Rect {
            x: 0,
            y: 0,
            w: 1200,
            h: 672,
        };
        let mut v = Viewer::open(frame, &s, "", FileKind::Txt, "A.PRG");
        assert_eq!(v.title, "TXT:A.PRG (100 lines)");
        assert!(!v.key(Keycode::PageDown));
        assert!(v.top > 0);
        v.key(Keycode::End);
        assert_eq!(v.top, 100 - v.rows_shown());
        v.wheel(-1000);
        assert_eq!(v.top, 0);
        assert!(v.pad(Button::South));
        assert!(v.key(Keycode::Escape));
        assert!(v.click((1200 - 10, 10)), "the close mark");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

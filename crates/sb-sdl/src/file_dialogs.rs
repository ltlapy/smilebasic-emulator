//! The windows of Ctrl+S and Ctrl+L, after SmileBASIC 4's: a name to save
//! the edited slot as, starting with its own, and a list of the programs
//! in the projects to load into it.

use sb_runtime::Framebuffer;
use sb_runtime::confirm::project_label;
use sb_runtime::font::Font;
use sb_runtime::storage::{FileKind, Storage};
use sdl3::keyboard::Keycode;

use crate::ui::{
    BORDER, CHAR, DISABLED, HIGHLIGHT, LINE, PAD, Rect, TEXT, draw_text, fill, outline, text_width,
    window,
};

/// The longest name typed, a little over the limit so that a name that
/// is too long gets an error rather than being cut short.
const MAX_TYPED: usize = 40;
const PICKER_WIDTH: u32 = 1000;
const PICKER_HEIGHT: u32 = 600;
const UP: &str = "Projects";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
    /// Save the slot under this name.
    Save(String),
    /// Load this file (`PROJECT/NAME`) into the slot.
    Load(String),
}

fn centered((w, h): (u32, u32), width: u32, height: u32) -> Rect {
    let (width, height) = (width.min(w), height.min(h));
    Rect {
        x: (w - width) / 2,
        y: (h - height) / 2,
        w: width,
        h: height,
    }
}

/// Ctrl+S: the name to save the slot as.
pub struct SaveDialog {
    size: (u32, u32),
    title: String,
    project: String,
    name: String,
}

impl SaveDialog {
    pub fn new(size: (u32, u32), slot: usize, name: &str, project: &str) -> SaveDialog {
        SaveDialog {
            size,
            title: format!("Slot {slot}: Save file"),
            project: project_label(project).to_string(),
            name: name.to_string(),
        }
    }

    fn frame(&self) -> Rect {
        // Title bar, the project, the name box, the hint.
        centered(self.size, 40 * CHAR + 4 * PAD, 4 * LINE + 6 * PAD)
    }

    pub fn text(&mut self, text: &str) {
        for c in text.chars().filter(char::is_ascii_graphic) {
            if self.name.len() < MAX_TYPED {
                self.name.push(c.to_ascii_uppercase());
            }
        }
    }

    pub fn key(&mut self, key: Keycode) -> Outcome {
        match key {
            Keycode::Escape => Outcome::Close,
            Keycode::Backspace => {
                self.name.pop();
                Outcome::Stay
            }
            Keycode::Return | Keycode::KpEnter if !self.name.is_empty() => {
                Outcome::Save(self.name.clone())
            }
            _ => Outcome::Stay,
        }
    }

    /// A click outside the window closes it.
    pub fn click(&self, at: (i32, i32)) -> Outcome {
        match self.frame().contains(at) {
            true => Outcome::Stay,
            false => Outcome::Close,
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, blink: bool) {
        let f = self.frame();
        window(fb, font, f, &self.title);
        let left = f.x + 2 * PAD;
        let project = format!("Project: {}", self.project);
        draw_text(fb, font, left, f.y + LINE + PAD + 4, &project, DISABLED);
        let field = Rect {
            x: left,
            y: f.y + 2 * LINE + 2 * PAD,
            w: f.w - 4 * PAD,
            h: LINE,
        };
        fill(fb, field, 0xFF00_0000);
        outline(fb, field, BORDER);
        draw_text(fb, font, field.x + PAD, field.y + 4, &self.name, TEXT);
        if blink {
            let x = field.x + PAD + text_width(&self.name);
            fill(
                fb,
                Rect {
                    x,
                    y: field.y + 4,
                    w: 2,
                    h: CHAR,
                },
                TEXT,
            );
        }
        let hint = "Enter: save  Esc: cancel";
        draw_text(fb, font, left, f.y + f.h - LINE - PAD + 4, hint, DISABLED);
    }
}

/// A row of the file list.
struct Entry {
    name: String,
    /// A file's size and date.
    info: Option<(u64, String)>,
}

/// Ctrl+L: the programs of a project, or the projects, to load from.
pub struct FilePicker {
    size: (u32, u32),
    slot: usize,
    /// The project whose files are listed; `None` while listing the
    /// projects.
    project: Option<String>,
    entries: Vec<Entry>,
    chosen: usize,
    scroll: usize,
    error: Option<String>,
}

impl FilePicker {
    /// The current project's programs.
    pub fn new(size: (u32, u32), slot: usize, storage: &Storage) -> FilePicker {
        let mut picker = FilePicker {
            size,
            slot,
            project: None,
            entries: Vec::new(),
            chosen: 0,
            scroll: 0,
            error: None,
        };
        picker.open(Some(storage.project().to_string()), storage);
        // The first program rather than the way up.
        picker.choose(1);
        picker
    }

    fn open(&mut self, project: Option<String>, storage: &Storage) {
        self.error = None;
        self.entries = match &project {
            Some(project) => {
                let mut entries = vec![Entry {
                    name: UP.to_string(),
                    info: None,
                }];
                match storage.list(Some(FileKind::Txt), Some(project)) {
                    Ok(files) => entries.extend(files.into_iter().map(|(kind, name)| Entry {
                        info: storage.file_info(project, kind, &name),
                        name,
                    })),
                    Err(e) => self.error = Some(e.to_string()),
                }
                entries
            }
            None => {
                let mut names = vec![String::new()];
                match storage.projects() {
                    Ok(projects) => names.extend(projects),
                    Err(e) => self.error = Some(e.to_string()),
                }
                (names.into_iter())
                    .map(|name| Entry { name, info: None })
                    .collect()
            }
        };
        self.project = project;
        self.chosen = 0;
        self.scroll = 0;
    }

    fn frame(&self) -> Rect {
        centered(self.size, PICKER_WIDTH, PICKER_HEIGHT)
    }

    fn list(&self) -> Rect {
        let f = self.frame();
        Rect {
            x: f.x + 2 * PAD,
            y: f.y + LINE + 2 * PAD,
            w: f.w - 4 * PAD,
            h: f.h - 2 * LINE - 4 * PAD,
        }
    }

    fn rows(&self) -> usize {
        (self.list().h / LINE).max(1) as usize
    }

    fn choose(&mut self, i: usize) {
        self.chosen = i.min(self.entries.len().saturating_sub(1));
        let rows = self.rows();
        self.scroll = self.scroll.min(self.chosen);
        if self.chosen >= self.scroll + rows {
            self.scroll = self.chosen + 1 - rows;
        }
    }

    /// Enter on the chosen row: up to the projects, into a project, or
    /// load a file.
    fn act(&mut self, storage: &Storage) -> Outcome {
        let Some(entry) = self.entries.get(self.chosen) else {
            return Outcome::Stay;
        };
        match &self.project {
            Some(_) if self.chosen == 0 => self.up(storage),
            Some(project) => return Outcome::Load(format!("{project}/{}", entry.name)),
            None => {
                let project = entry.name.clone();
                self.open(Some(project), storage);
                self.choose(1);
            }
        }
        Outcome::Stay
    }

    /// To the projects, with the one left chosen.
    fn up(&mut self, storage: &Storage) {
        let left = self.project.take();
        self.open(None, storage);
        if let Some(i) = self
            .entries
            .iter()
            .position(|e| Some(&e.name) == left.as_ref())
        {
            self.choose(i);
        }
    }

    pub fn key(&mut self, key: Keycode, storage: &Storage) -> Outcome {
        let page = self.rows().saturating_sub(1).max(1);
        match key {
            Keycode::Escape => return Outcome::Close,
            Keycode::Up => self.choose(self.chosen.saturating_sub(1)),
            Keycode::Down => self.choose(self.chosen + 1),
            Keycode::PageUp => self.choose(self.chosen.saturating_sub(page)),
            Keycode::PageDown => self.choose(self.chosen + page),
            Keycode::Home => self.choose(0),
            Keycode::End => self.choose(usize::MAX),
            Keycode::Backspace if self.project.is_some() => self.up(storage),
            Keycode::Return | Keycode::KpEnter => return self.act(storage),
            _ => {}
        }
        Outcome::Stay
    }

    fn row_at(&self, at: (i32, i32)) -> Option<usize> {
        let list = self.list();
        if !list.contains(at) {
            return None;
        }
        let i = self.scroll + ((at.1 - list.y as i32) / LINE as i32) as usize;
        (i < self.entries.len()).then_some(i)
    }

    pub fn mouse_move(&mut self, at: (i32, i32)) {
        if let Some(i) = self.row_at(at) {
            self.chosen = i;
        }
    }

    /// A click on a row acts on it; one outside the window closes it.
    pub fn click(&mut self, at: (i32, i32), storage: &Storage) -> Outcome {
        if !self.frame().contains(at) {
            return Outcome::Close;
        }
        match self.row_at(at) {
            Some(i) => {
                self.chosen = i;
                self.act(storage)
            }
            None => Outcome::Stay,
        }
    }

    pub fn wheel(&mut self, rows: i32) {
        let most = self.entries.len().saturating_sub(self.rows());
        self.scroll = self
            .scroll
            .saturating_add_signed(3 * rows as isize)
            .min(most);
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        let f = self.frame();
        let title = match &self.project {
            Some(project) => format!("Slot {}: Load file - {}", self.slot, project_label(project)),
            None => format!("Slot {}: Load file - Projects", self.slot),
        };
        window(fb, font, f, &title);
        let list = self.list();
        for (row, entry) in self
            .entries
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(self.rows())
        {
            let y = list.y + (row - self.scroll) as u32 * LINE;
            if row == self.chosen {
                fill(fb, Rect { y, h: LINE, ..list }, HIGHLIGHT);
            }
            let up = self.project.is_some() && row == 0;
            let name = match (&self.project, up) {
                (_, true) => format!("^ {UP}"),
                (None, _) => project_label(&entry.name).to_string(),
                (Some(_), _) => entry.name.clone(),
            };
            let color = if up { DISABLED } else { TEXT };
            draw_text(fb, font, list.x + PAD, y + 4, &name, color);
            if let Some((size, date)) = &entry.info {
                let info = format!("{size:>8}  {date}");
                let x = list.x + list.w - PAD - text_width(&info);
                draw_text(fb, font, x, y + 4, &info, TEXT);
            }
        }
        let hint = match &self.error {
            Some(e) => e.clone(),
            None => "Enter: load  Backspace: projects  Esc: cancel".to_string(),
        };
        draw_text(
            fb,
            font,
            list.x,
            f.y + f.h - LINE - PAD + 4,
            &hint,
            DISABLED,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_types_a_name() {
        let mut d = SaveDialog::new((1280, 720), 0, "GAME", "");
        d.key(Keycode::Backspace);
        d.text("x");
        assert_eq!(d.key(Keycode::Return), Outcome::Save("GAMX".into()));
        for _ in 0..4 {
            d.key(Keycode::Backspace);
        }
        assert_eq!(d.key(Keycode::Return), Outcome::Stay, "no name");
        assert_eq!(d.key(Keycode::Escape), Outcome::Close);
    }

    #[test]
    fn picks_files_across_projects() {
        let dir = std::env::temp_dir().join(format!("sb-picker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut storage = Storage::new(dir.clone());
        storage.create_project("OTHER").unwrap();
        storage.write(FileKind::Txt, "A", b"?1", None).unwrap();
        storage.set_project("OTHER").unwrap();
        storage.write(FileKind::Txt, "B", b"?2", None).unwrap();
        storage.set_project("").unwrap();

        let mut p = FilePicker::new((1280, 720), 2, &storage);
        assert_eq!(p.entries[1].info.as_ref().map(|i| i.0), Some(2));
        assert_eq!(p.key(Keycode::Return, &storage), Outcome::Load("/A".into()));
        p.key(Keycode::Backspace, &storage);
        assert_eq!(p.project, None);
        assert_eq!(p.chosen, 0, "the workspace, left behind");
        p.key(Keycode::Down, &storage);
        p.key(Keycode::Return, &storage);
        assert_eq!(
            p.key(Keycode::Return, &storage),
            Outcome::Load("OTHER/B".into())
        );
        p.key(Keycode::Up, &storage);
        p.key(Keycode::Return, &storage);
        assert_eq!(p.project, None, "the way up");
        assert_eq!(p.key(Keycode::Escape, &storage), Outcome::Close);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

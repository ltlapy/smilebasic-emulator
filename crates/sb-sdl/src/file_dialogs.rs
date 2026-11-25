//! The windows of Ctrl+S and Ctrl+L, modeled on those of SmileBASIC 4: a
//! name to save the edited slot as, starting with the slot's own name, and
//! a list of the programs in the projects to load into it. The settings
//! pick the tools of F9 to F11 from the same list.

use sb_runtime::Framebuffer;
use sb_runtime::confirm::project_label;
use sb_runtime::font::Font;
use sb_runtime::storage::{FileKind, Storage};
use sdl3::keyboard::Keycode;
use std::time::SystemTime;

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
/// The row of the first file in a project's list. Row 0 is the way up.
const FIRST_FILE_ROW: usize = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
    /// Save the slot under this name.
    Save(String),
    /// Load this file (`PROJECT/NAME`) into the slot.
    Load(String),
    /// Copy the file into `project` as `name`.
    Copy {
        project: String,
        name: String,
    },
    /// Make this file (`PROJECT/NAME`) tool `n`, 0 to 2.
    Tool(usize, String),
}

/// Returns a file name in the form that SUBRUN, SYSPARAM "TOOL1" and the
/// picker's outcomes use: `PROJECT/NAME`, with an empty project for the
/// workspace.
pub fn file_spec(project: &str, name: &str) -> String {
    format!("{project}/{name}")
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

/// The purpose of the file list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// Ctrl+L: a program to load into this slot. The typed text narrows
    /// the list.
    Load(usize),
    /// A file to copy into the project chosen here, under the name typed.
    Copy { kind: FileKind, name: String },
    /// The settings: a program for F9, F10 or F11 (0 to 2) to start.
    /// The typed text narrows the list, the same as in loading.
    Tool(usize),
}

/// The list of Ctrl+L: the programs of a project, or the projects, to load
/// from. The projects window also uses it to choose where to copy a file.
pub struct FilePicker {
    size: (u32, u32),
    purpose: Purpose,
    /// The project whose files are listed; `None` while listing the
    /// projects.
    project: Option<String>,
    /// The project's files, or the projects, before narrowing.
    all: Vec<Entry>,
    /// The rows shown. In a project, the first row is the way up to the
    /// projects.
    entries: Vec<Entry>,
    chosen: usize,
    scroll: usize,
    error: Option<String>,
    /// The name box: the text that narrows the list, or the name to copy to.
    field: String,
    /// The modification time of the listed folder, taken when the list was
    /// read.
    read_at: Option<SystemTime>,
}

impl FilePicker {
    /// Opens on the programs of the current project.
    pub fn new(size: (u32, u32), slot: usize, storage: &Storage) -> FilePicker {
        FilePicker::open_at(size, Purpose::Load(slot), storage.project(), storage)
    }

    /// For copying a file: the files of its kind in `project`, with its
    /// name in the name box.
    pub fn copy(
        size: (u32, u32),
        (kind, name): (FileKind, &str),
        project: &str,
        storage: &Storage,
    ) -> FilePicker {
        let purpose = Purpose::Copy {
            kind,
            name: name.to_string(),
        };
        FilePicker::open_at(size, purpose, project, storage)
    }

    /// For tool `n`, 0 to 2: the programs of the project the tool is in
    /// now, with the tool chosen, or of the current project if that project
    /// is gone.
    pub fn tool(size: (u32, u32), n: usize, now: &str, storage: &Storage) -> FilePicker {
        let (project, name) = now.split_once('/').unwrap_or(("", ""));
        let project = match storage.project_exists(project) {
            true if !now.is_empty() => project,
            _ => storage.project(),
        };
        let mut picker = FilePicker::open_at(size, Purpose::Tool(n), project, storage);
        let mut files = picker.entries.iter().skip(FIRST_FILE_ROW);
        if let Some(i) = files.position(|e| e.name == name) {
            picker.choose(FIRST_FILE_ROW + i);
        }
        picker
    }

    fn open_at(size: (u32, u32), purpose: Purpose, project: &str, storage: &Storage) -> FilePicker {
        let field = match &purpose {
            Purpose::Load(_) | Purpose::Tool(_) => String::new(),
            Purpose::Copy { name, .. } => name.clone(),
        };
        let mut picker = FilePicker {
            size,
            purpose,
            project: None,
            all: Vec::new(),
            entries: Vec::new(),
            chosen: 0,
            scroll: 0,
            error: None,
            field,
            read_at: None,
        };
        picker.open(Some(project.to_string()), storage);
        picker.choose(FIRST_FILE_ROW);
        picker
    }

    fn kind(&self) -> FileKind {
        match &self.purpose {
            Purpose::Load(_) | Purpose::Tool(_) => FileKind::Txt,
            Purpose::Copy { kind, .. } => *kind,
        }
    }

    fn narrows(&self) -> bool {
        matches!(self.purpose, Purpose::Load(_) | Purpose::Tool(_))
    }

    fn open(&mut self, project: Option<String>, storage: &Storage) {
        self.error = None;
        self.read_at = storage.changed_at(project.as_deref());
        self.all = match &project {
            Some(project) => match storage.list(Some(self.kind()), Some(project)) {
                Ok(files) => (files.into_iter())
                    .map(|(kind, name)| Entry {
                        info: storage.file_info(project, kind, &name),
                        name,
                    })
                    .collect(),
                Err(e) => {
                    self.error = Some(e.to_string());
                    Vec::new()
                }
            },
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
        self.narrow();
    }

    /// Reads the list again for files that other programs added, removed
    /// or renamed, keeping the chosen row. Unless `always` is set, it reads
    /// only when the folder's modification time shows a change in it.
    pub fn reread(&mut self, storage: &Storage, always: bool) {
        if !always && storage.changed_at(self.project.as_deref()) == self.read_at {
            return;
        }
        let (chosen, scroll) = (self.chosen, self.scroll);
        let name = self.entries.get(chosen).map(|e| e.name.clone());
        self.open(self.project.clone(), storage);
        let found = name.and_then(|name| self.entries.iter().position(|e| e.name == name));
        self.scroll = scroll;
        self.choose(found.unwrap_or(chosen));
    }

    /// Shows the rows that contain the typed text, when the text narrows
    /// the list.
    fn narrow(&mut self) {
        let typed = self.field.to_ascii_uppercase();
        let fits = |e: &Entry| {
            let shown = match self.project {
                Some(_) => e.name.as_str(),
                None => project_label(&e.name),
            };
            !self.narrows() || shown.to_ascii_uppercase().contains(&typed)
        };
        let mut entries = Vec::new();
        if self.project.is_some() {
            entries.push(Entry {
                name: UP.to_string(),
                info: None,
            });
        }
        entries.extend(self.all.iter().filter(|e| fits(e)).map(|e| Entry {
            name: e.name.clone(),
            info: e.info.clone(),
        }));
        self.entries = entries;
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
            // Under the list: the name box and the hint.
            h: f.h - 3 * LINE - 6 * PAD,
        }
    }

    fn field_rect(&self) -> Rect {
        let f = self.frame();
        let list = self.list();
        let label = text_width("Copy as: ");
        Rect {
            x: list.x + label,
            y: f.y + f.h - 2 * LINE - 3 * PAD,
            w: list.w - label,
            h: LINE,
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

    /// Handles Enter on the chosen row: it goes up to the projects, opens a
    /// project, or loads a file. When choosing a tool, it makes the file
    /// that tool instead of loading it. When copying, Enter in a project
    /// copies the file there under the name in the name box, and does
    /// nothing while the box is empty.
    fn act(&mut self, storage: &Storage) -> Outcome {
        let Some(entry) = self.entries.get(self.chosen) else {
            return Outcome::Stay;
        };
        match (&self.project, &self.purpose) {
            (Some(project), Purpose::Copy { .. }) if !self.field.is_empty() => {
                return Outcome::Copy {
                    project: project.clone(),
                    name: self.field.clone(),
                };
            }
            (Some(_), Purpose::Copy { .. }) => {}
            (Some(_), _) if self.chosen == 0 => self.up(storage),
            (Some(project), Purpose::Tool(n)) => {
                return Outcome::Tool(*n, file_spec(project, &entry.name));
            }
            (Some(project), _) => return Outcome::Load(file_spec(project, &entry.name)),
            (None, _) => {
                let project = entry.name.clone();
                if self.narrows() {
                    self.field.clear();
                }
                self.open(Some(project), storage);
                self.choose(FIRST_FILE_ROW);
            }
        }
        Outcome::Stay
    }

    /// Goes up to the projects, with the project just left chosen.
    fn up(&mut self, storage: &Storage) {
        let left = self.project.take();
        if self.narrows() {
            self.field.clear();
        }
        self.open(None, storage);
        if let Some(i) = self
            .entries
            .iter()
            .position(|e| Some(&e.name) == left.as_ref())
        {
            self.choose(i);
        }
    }

    /// Typed text goes into the name box.
    pub fn text(&mut self, text: &str) {
        for c in text.chars().filter(char::is_ascii_graphic) {
            if self.field.len() < MAX_TYPED {
                self.field.push(c.to_ascii_uppercase());
            }
        }
        if self.narrows() {
            self.narrow();
            self.choose(self.first_match_row());
        }
    }

    /// The row of the first entry that the typed text leaves: below the way
    /// up in a project, the top row among the projects.
    fn first_match_row(&self) -> usize {
        match self.project {
            Some(_) => FIRST_FILE_ROW,
            None => 0,
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
            // Backspace erases the typed text, then goes up.
            Keycode::Backspace if !self.field.is_empty() => {
                self.field.pop();
                if self.narrows() {
                    self.narrow();
                    self.choose(self.first_match_row());
                }
            }
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

    /// A click on a row acts on it, but when copying, a click on a file's
    /// row puts its name in the name box, to copy over that file. A click
    /// outside the window closes it.
    pub fn click(&mut self, at: (i32, i32), storage: &Storage) -> Outcome {
        if !self.frame().contains(at) {
            return Outcome::Close;
        }
        let Some(i) = self.row_at(at) else {
            return Outcome::Stay;
        };
        self.chosen = i;
        match (&self.project, &self.purpose) {
            (Some(_), Purpose::Copy { .. }) if i == 0 => self.up(storage),
            (Some(_), Purpose::Copy { .. }) => self.field = self.entries[i].name.clone(),
            _ => return self.act(storage),
        }
        Outcome::Stay
    }

    pub fn wheel(&mut self, rows: i32) {
        let most = self.entries.len().saturating_sub(self.rows());
        self.scroll = self
            .scroll
            .saturating_add_signed(3 * rows as isize)
            .min(most);
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, blink: bool) {
        let f = self.frame();
        let place = match &self.project {
            Some(project) => project_label(project),
            None => "Projects",
        };
        let title = match &self.purpose {
            Purpose::Load(slot) => format!("Slot {slot}: Load file - {place}"),
            Purpose::Tool(n) => format!("F{} tool: choose a program - {place}", n + 9),
            Purpose::Copy { kind, name } => match &self.project {
                Some(_) => format!("Copy {}:{name} into {place}", kind.name()),
                None => format!("Copy {}:{name}: choose a project", kind.name()),
            },
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
        outline(fb, list, BORDER);
        let field = self.field_rect();
        let label = if self.narrows() { "Find:" } else { "Copy as:" };
        draw_text(fb, font, list.x, field.y + 4, label, TEXT);
        fill(fb, field, 0xFF00_0000);
        outline(fb, field, BORDER);
        draw_text(fb, font, field.x + PAD, field.y + 4, &self.field, TEXT);
        if blink {
            let x = field.x + PAD + text_width(&self.field);
            let cursor = Rect {
                x,
                y: field.y + 4,
                w: 2,
                h: CHAR,
            };
            fill(fb, cursor, TEXT);
        }
        let hint = match (&self.error, &self.purpose, &self.project) {
            (Some(e), _, _) => e.clone(),
            (None, Purpose::Load(_), _) => {
                "Type to narrow  Enter: load  Backspace: projects  Esc: cancel".to_string()
            }
            (None, Purpose::Tool(_), _) => {
                "Type to narrow  Enter: choose  Backspace: projects  Esc: cancel".to_string()
            }
            (None, Purpose::Copy { .. }, None) => "Enter: open  Esc: cancel".to_string(),
            (None, Purpose::Copy { .. }, Some(_)) => "Enter: copy  Esc: cancel".to_string(),
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

    #[test]
    fn rereads_what_other_programs_changed() {
        let dir = std::env::temp_dir().join(format!("sb-reread-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let storage = Storage::new(dir.clone());
        for name in ["B", "D"] {
            storage.write(FileKind::Txt, name, b"", None).unwrap();
        }
        let workspace = dir.join("@WORKSPACE");
        let mut p = FilePicker::new((1280, 720), 0, &storage);
        p.choose(2);
        let names = |p: &FilePicker| -> Vec<String> {
            (p.entries.iter().skip(1)).map(|e| e.name.clone()).collect()
        };
        std::fs::write(workspace.join("TA"), "").unwrap();
        std::fs::write(workspace.join("TC"), "?1").unwrap();
        // The folder's modification time when the list was read: nothing to
        // read again.
        p.read_at = storage.changed_at(Some(""));
        p.reread(&storage, false);
        assert_eq!(names(&p), ["B", "D"]);
        // Read before the change: the new files show, D stays chosen.
        p.read_at = Some(std::time::SystemTime::UNIX_EPOCH);
        p.reread(&storage, false);
        assert_eq!(names(&p), ["A", "B", "C", "D"]);
        assert_eq!(p.entries[p.chosen].name, "D");
        assert_eq!(p.entries[3].info.as_ref().map(|i| i.0), Some(2));
        // When the chosen file is gone, its row stays chosen.
        std::fs::remove_file(workspace.join("TD")).unwrap();
        p.reread(&storage, true);
        assert_eq!(names(&p), ["A", "B", "C"]);
        assert_eq!(p.entries[p.chosen].name, "C");
        // The projects, too.
        p.key(Keycode::Backspace, &storage);
        storage.create_project("NEW").unwrap();
        p.reread(&storage, true);
        let projects: Vec<&str> = p.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(projects, ["", "NEW"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn picks_a_tool_where_it_is() {
        let dir = std::env::temp_dir().join(format!("sb-tool-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut storage = Storage::new(dir.clone());
        storage.create_project("#TOOL").unwrap();
        storage.set_project("#TOOL").unwrap();
        for name in ["GAHAKU.PRG", "SBSMILE.PRG"] {
            storage.write(FileKind::Txt, name, b"", None).unwrap();
        }
        storage.set_project("").unwrap();
        storage.write(FileKind::Txt, "MINE.PRG", b"", None).unwrap();

        // In the tool's project, with it chosen.
        let mut p = FilePicker::tool((1280, 720), 0, "#TOOL/SBSMILE.PRG", &storage);
        assert_eq!(p.project.as_deref(), Some("#TOOL"));
        assert_eq!(
            p.key(Keycode::Return, &storage),
            Outcome::Tool(0, "#TOOL/SBSMILE.PRG".into())
        );
        // None set, or its project gone: the current project.
        for now in ["", "GONE/X.PRG"] {
            let mut p = FilePicker::tool((1280, 720), 2, now, &storage);
            assert_eq!(p.project.as_deref(), Some(""), "{now:?}");
            assert_eq!(
                p.key(Keycode::Return, &storage),
                Outcome::Tool(2, "/MINE.PRG".into())
            );
        }
        // Other projects, as in loading.
        let mut p = FilePicker::tool((1280, 720), 1, "", &storage);
        p.key(Keycode::Backspace, &storage);
        p.text("#t");
        p.key(Keycode::Return, &storage);
        assert_eq!(
            p.key(Keycode::Return, &storage),
            Outcome::Tool(1, "#TOOL/GAHAKU.PRG".into())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn typing_among_the_projects_chooses_the_first_match() {
        let dir = std::env::temp_dir().join(format!("sb-first-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let storage = Storage::new(dir.clone());
        for name in ["GAME1", "GAME2", "OTHER"] {
            storage.create_project(name).unwrap();
        }
        let mut p = FilePicker::new((1280, 720), 0, &storage);
        p.key(Keycode::Backspace, &storage);
        assert_eq!(p.project, None);
        p.text("game");
        let names: Vec<&str> = p.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["GAME1", "GAME2"]);
        assert_eq!(p.entries[p.chosen].name, "GAME1");
        // Erasing a letter chooses the first match again.
        p.key(Keycode::Down, &storage);
        p.key(Keycode::Backspace, &storage);
        assert_eq!(p.entries[p.chosen].name, "GAME1");
        p.key(Keycode::Return, &storage);
        assert_eq!(p.project.as_deref(), Some("GAME1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn typing_narrows_the_list() {
        let dir = std::env::temp_dir().join(format!("sb-narrow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let storage = Storage::new(dir.clone());
        for name in ["GAME.PRG", "TOOL.PRG", "MYGAME.PRG"] {
            storage.write(FileKind::Txt, name, b"", None).unwrap();
        }
        let mut p = FilePicker::new((1280, 720), 0, &storage);
        p.text("game");
        let names: Vec<&str> = p.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, [UP, "GAME.PRG", "MYGAME.PRG"]);
        assert_eq!(
            p.key(Keycode::Return, &storage),
            Outcome::Load("/GAME.PRG".into())
        );
        // Backspace erases, then goes up.
        for _ in 0..4 {
            p.key(Keycode::Backspace, &storage);
        }
        assert_eq!(p.entries.len(), 4);
        p.key(Keycode::Backspace, &storage);
        assert_eq!(p.project, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

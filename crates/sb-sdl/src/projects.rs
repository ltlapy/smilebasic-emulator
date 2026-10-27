//! The project manager: lists the projects and their files, and switches,
//! creates, renames and deletes projects. Drawn over the screen like the
//! menu bar, it takes all input while it is open.

use sb_runtime::Framebuffer;
use sb_runtime::confirm::{project_label, switch_lines};
use sb_runtime::font::Font;
use sb_runtime::storage::{Storage, check_project_name, suggest_project_name};
use sdl3::keyboard::Keycode;

use crate::dialog::{Answer, Dialog};
use crate::ui::{
    BORDER, CHAR, HIGHLIGHT, LINE, PAD, Rect, TEXT, button, draw_text, fill, outline, text_width,
    window,
};

const WIDTH: u32 = 1120;
const HEIGHT: u32 = 600;
/// The width of the project list; the files take the rest.
const LIST_WIDTH: u32 = 600;
const ERROR: u32 = 0xFFFF_8080;
/// The longest name the name box takes, a little over the limit so that
/// a name that is too long gets an error rather than being cut short.
const MAX_TYPED: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Button {
    Switch,
    New,
    Rename,
    Delete,
    OpenFolder,
    Close,
}

const BUTTONS: [Button; 6] = [
    Button::Switch,
    Button::New,
    Button::Rename,
    Button::Delete,
    Button::OpenFolder,
    Button::Close,
];

impl Button {
    fn label(self) -> &'static str {
        match self {
            Button::Switch => "Switch",
            Button::New => "New",
            Button::Rename => "Rename",
            Button::Delete => "Delete",
            Button::OpenFolder => "Open folder",
            Button::Close => "Close",
        }
    }
}

/// What the frontend should do after an event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
    /// The current project changed to the one named; the manager closes.
    Switched(String),
    /// Open the folder that holds the projects.
    OpenFolder,
}

enum Naming {
    New,
    /// `then_switch` when renaming a folder whose name breaks the rules,
    /// so that it can be switched to.
    Rename {
        from: String,
        then_switch: bool,
    },
}

struct NameBox {
    naming: Naming,
    text: String,
    message: Option<String>,
}

enum Pending {
    Switch(String),
    Delete(String),
}

enum Mode {
    Browse,
    Name(NameBox),
    Ask(Dialog, Pending),
}

pub struct ProjectManager {
    /// Display size the window is centered in.
    size: (u32, u32),
    /// The workspace (`""`) first, then the folders as named on disk.
    projects: Vec<String>,
    current: String,
    selected: usize,
    /// The first project row shown.
    scroll: usize,
    /// Files of the selected project, as `TXT:NAME`.
    files: Vec<String>,
    status: Option<(String, bool)>,
    mode: Mode,
    mouse: Option<(i32, i32)>,
}

/// Whether a folder's name can be used as a project name as it is.
fn usable(name: &str) -> bool {
    name.is_empty() || check_project_name(name).is_ok_and(|n| n == name)
}

/// `text` cut to `width` pixels, with `~` where it was cut.
fn fit(text: &str, width: u32) -> String {
    let max = (width / CHAR) as usize;
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
    cut.push('~');
    cut
}

impl ProjectManager {
    pub fn new(storage: &Storage, size: (u32, u32)) -> ProjectManager {
        let mut manager = ProjectManager {
            size,
            projects: Vec::new(),
            current: String::new(),
            selected: 0,
            scroll: 0,
            files: Vec::new(),
            status: None,
            mode: Mode::Browse,
            mouse: None,
        };
        manager.refresh(storage, None);
        let current = manager.current.clone();
        manager.select_name(&current, storage);
        manager
    }

    /// Reads the projects again, selecting `select` if given.
    fn refresh(&mut self, storage: &Storage, select: Option<&str>) {
        self.current = storage.project().to_string();
        self.projects = vec![String::new()];
        match storage.projects() {
            Ok(names) => self.projects.extend(names),
            Err(e) => self.status = Some((format!("Could not list the projects: {e}"), true)),
        }
        match select {
            Some(name) => self.select_name(name, storage),
            None => self.select(self.selected, storage),
        }
    }

    fn select_name(&mut self, name: &str, storage: &Storage) {
        let i = self.projects.iter().position(|p| p == name).unwrap_or(0);
        self.select(i, storage);
    }

    fn select(&mut self, i: usize, storage: &Storage) {
        self.selected = i.min(self.projects.len() - 1);
        let rows = self.rows();
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + rows {
            self.scroll = self.selected + 1 - rows;
        }
        self.files = match storage.list(None, Some(self.selected_name())) {
            Ok(files) => files
                .into_iter()
                .map(|(kind, name)| format!("{}:{name}", kind.name()))
                .collect(),
            Err(e) => {
                self.status = Some((format!("Could not list the files: {e}"), true));
                Vec::new()
            }
        };
    }

    fn selected_name(&self) -> &str {
        &self.projects[self.selected]
    }

    fn enabled(&self, button: Button) -> bool {
        let name = self.selected_name();
        match button {
            Button::Switch => name != self.current,
            Button::Rename | Button::Delete => !name.is_empty(),
            Button::New | Button::OpenFolder | Button::Close => true,
        }
    }

    fn frame(&self) -> Rect {
        let (w, h) = (WIDTH.min(self.size.0), HEIGHT.min(self.size.1));
        Rect {
            x: (self.size.0 - w) / 2,
            y: (self.size.1 - h) / 2,
            w,
            h,
        }
    }

    /// The project list; the files sit to its right.
    fn list(&self) -> Rect {
        let f = self.frame();
        Rect {
            x: f.x + 2 * PAD,
            y: f.y + 2 * LINE + 2 * PAD,
            w: LIST_WIDTH,
            // Under the list: the status line and the buttons.
            h: f.h - 4 * LINE - 6 * PAD,
        }
    }

    fn rows(&self) -> usize {
        (self.list().h / LINE) as usize
    }

    fn files_rect(&self) -> Rect {
        let f = self.frame();
        let list = self.list();
        let x = list.x + list.w + 2 * PAD;
        Rect {
            x,
            w: f.x + f.w - 2 * PAD - x,
            ..list
        }
    }

    fn row(&self, i: usize) -> Rect {
        let list = self.list();
        Rect {
            y: list.y + (i - self.scroll) as u32 * LINE,
            h: LINE,
            ..list
        }
    }

    fn button(&self, button: Button) -> Rect {
        let f = self.frame();
        let width = |b: Button| text_width(b.label()) + 4 * PAD;
        let y = f.y + f.h - 2 * PAD - LINE;
        if button == Button::Close {
            let w = width(button);
            return Rect {
                x: f.x + f.w - 2 * PAD - w,
                y,
                w,
                h: LINE,
            };
        }
        let before: u32 = BUTTONS
            .iter()
            .take_while(|&&b| b != button)
            .map(|&b| width(b) + PAD)
            .sum();
        Rect {
            x: f.x + 2 * PAD + before,
            y,
            w: width(button),
            h: LINE,
        }
    }

    pub fn mouse_move(&mut self, at: (i32, i32)) {
        self.mouse = Some(at);
        if let Mode::Ask(dialog, _) = &mut self.mode {
            dialog.mouse_move(at);
        }
    }

    /// Scrolls the project list by `lines`, down for positive.
    pub fn wheel(&mut self, lines: i32) {
        if !matches!(self.mode, Mode::Browse) {
            return;
        }
        let last = self.projects.len().saturating_sub(self.rows());
        self.scroll = self.scroll.saturating_add_signed(lines as isize).min(last);
    }

    /// Typed text, for the name box.
    pub fn text(&mut self, text: &str) {
        if let Mode::Name(name) = &mut self.mode {
            for c in text.chars().filter(char::is_ascii_graphic) {
                if name.text.len() < MAX_TYPED {
                    name.text.push(c);
                }
            }
        }
    }

    /// `unsaved` lists the slots with unsaved edits, for the warning
    /// before switching.
    pub fn click(&mut self, at: (i32, i32), storage: &mut Storage, unsaved: &[usize]) -> Outcome {
        match &mut self.mode {
            Mode::Ask(dialog, _) => match dialog.click(at) {
                Some(answer) => self.answer(answer, storage, unsaved),
                None => Outcome::Stay,
            },
            Mode::Name(_) => {
                if self.name_button(0).contains(at) {
                    self.submit(storage, unsaved)
                } else if self.name_button(1).contains(at) {
                    self.mode = Mode::Browse;
                    Outcome::Stay
                } else {
                    Outcome::Stay
                }
            }
            Mode::Browse => {
                if let Some(&b) = BUTTONS.iter().find(|&&b| self.button(b).contains(at)) {
                    return self.press(b, storage, unsaved);
                }
                let shown = self.scroll..self.projects.len().min(self.scroll + self.rows());
                if let Some(i) = shown.into_iter().find(|&i| self.row(i).contains(at)) {
                    self.select(i, storage);
                }
                Outcome::Stay
            }
        }
    }

    /// Up and Down choose a project, Enter switches to it, Esc closes.
    pub fn key(&mut self, key: Keycode, storage: &mut Storage, unsaved: &[usize]) -> Outcome {
        match &mut self.mode {
            Mode::Ask(dialog, _) => match dialog.key(key) {
                Some(answer) => self.answer(answer, storage, unsaved),
                None => Outcome::Stay,
            },
            Mode::Name(name) => match key {
                Keycode::Return | Keycode::KpEnter => self.submit(storage, unsaved),
                Keycode::Escape => {
                    self.mode = Mode::Browse;
                    Outcome::Stay
                }
                Keycode::Backspace => {
                    name.text.pop();
                    Outcome::Stay
                }
                _ => Outcome::Stay,
            },
            Mode::Browse => {
                let last = self.projects.len() - 1;
                let page = self.rows().saturating_sub(1).max(1);
                let to = match key {
                    Keycode::Up => self.selected.saturating_sub(1),
                    Keycode::Down => (self.selected + 1).min(last),
                    Keycode::PageUp => self.selected.saturating_sub(page),
                    Keycode::PageDown => (self.selected + page).min(last),
                    Keycode::Home => 0,
                    Keycode::End => last,
                    Keycode::Return | Keycode::KpEnter => {
                        return self.press(Button::Switch, storage, unsaved);
                    }
                    Keycode::Delete => return self.press(Button::Delete, storage, unsaved),
                    Keycode::F2 => return self.press(Button::Rename, storage, unsaved),
                    Keycode::Escape => return Outcome::Close,
                    _ => return Outcome::Stay,
                };
                self.select(to, storage);
                Outcome::Stay
            }
        }
    }

    fn press(&mut self, button: Button, storage: &mut Storage, unsaved: &[usize]) -> Outcome {
        if !self.enabled(button) {
            return Outcome::Stay;
        }
        let name = self.selected_name().to_string();
        match button {
            Button::Switch => return self.switch(name, storage, unsaved, false),
            Button::New => self.ask_name(Naming::New, String::new(), None),
            Button::Rename => {
                let text = suggest_project_name(&name);
                let naming = Naming::Rename {
                    from: name,
                    then_switch: false,
                };
                self.ask_name(naming, text, None);
            }
            Button::Delete => {
                let count = self.files.len();
                let files = match count {
                    0 => "It has no files.".to_string(),
                    1 => "Its 1 file will be deleted too.".to_string(),
                    n => format!("Its {n} files will be deleted too."),
                };
                let lines = vec![
                    format!("Delete project {name}."),
                    String::new(),
                    files,
                    "This cannot be undone.".to_string(),
                ];
                let dialog = Dialog::new("Delete project", lines, "Delete", self.size);
                self.mode = Mode::Ask(dialog, Pending::Delete(name));
            }
            Button::OpenFolder => return Outcome::OpenFolder,
            Button::Close => return Outcome::Close,
        }
        Outcome::Stay
    }

    fn ask_name(&mut self, naming: Naming, text: String, message: Option<String>) {
        self.mode = Mode::Name(NameBox {
            naming,
            text,
            message,
        });
    }

    /// Switches to `name`, first asking if slots have unsaved edits
    /// (unless `asked`), or for a new name if the folder's breaks the rules.
    fn switch(
        &mut self,
        name: String,
        storage: &mut Storage,
        unsaved: &[usize],
        asked: bool,
    ) -> Outcome {
        if !usable(&name) {
            let text = suggest_project_name(&name);
            let naming = Naming::Rename {
                from: name,
                then_switch: true,
            };
            let message = "Rename this folder to use it as a project.".to_string();
            self.ask_name(naming, text, Some(message));
            return Outcome::Stay;
        }
        if !asked && !unsaved.is_empty() {
            let old = project_label(&self.current);
            let lines = switch_lines(project_label(&name), old, unsaved);
            let dialog = Dialog::new("Switch project", lines, "Switch", self.size);
            self.mode = Mode::Ask(dialog, Pending::Switch(name));
            return Outcome::Stay;
        }
        self.mode = Mode::Browse;
        match storage.set_project(&name) {
            Ok(()) => Outcome::Switched(project_label(storage.project()).to_string()),
            Err(e) => {
                self.status = Some((format!("Could not switch: {e}"), true));
                Outcome::Stay
            }
        }
    }

    fn answer(&mut self, answer: Answer, storage: &mut Storage, unsaved: &[usize]) -> Outcome {
        let Mode::Ask(_, pending) = std::mem::replace(&mut self.mode, Mode::Browse) else {
            unreachable!("only questions are answered")
        };
        if answer == Answer::Cancel {
            return Outcome::Stay;
        }
        match pending {
            Pending::Switch(name) => return self.switch(name, storage, unsaved, true),
            Pending::Delete(name) => {
                let was_current = name == self.current;
                self.status = Some(match storage.delete_project(&name) {
                    Ok(()) if was_current => (
                        format!("Deleted project {name}; now in {}.", project_label("")),
                        false,
                    ),
                    Ok(()) => (format!("Deleted project {name}."), false),
                    Err(e) => (format!("Could not delete {name}: {e}"), true),
                });
                self.refresh(storage, None);
            }
        }
        Outcome::Stay
    }

    fn submit(&mut self, storage: &mut Storage, unsaved: &[usize]) -> Outcome {
        let Mode::Name(name) = &mut self.mode else {
            unreachable!("only the name box submits")
        };
        let done = match &name.naming {
            Naming::New => storage.create_project(&name.text),
            Naming::Rename { from, .. } => storage.rename_project(from, &name.text),
        };
        let new = match done {
            Ok(new) => new,
            Err(e) => {
                name.message = Some(e.to_string());
                return Outcome::Stay;
            }
        };
        let Mode::Name(name) = std::mem::replace(&mut self.mode, Mode::Browse) else {
            unreachable!()
        };
        self.refresh(storage, Some(&new));
        match name.naming {
            Naming::New => self.status = Some((format!("Created project {new}."), false)),
            Naming::Rename {
                then_switch: true, ..
            } => return self.switch(new, storage, unsaved, false),
            Naming::Rename { from, .. } => {
                self.status = Some((format!("Renamed project {from} to {new}."), false));
            }
        }
        Outcome::Stay
    }

    fn name_frame(&self) -> Rect {
        let (w, h) = (760, 6 * LINE + 6 * PAD);
        Rect {
            x: self.size.0.saturating_sub(w) / 2,
            y: self.size.1.saturating_sub(h) / 2,
            w,
            h,
        }
    }

    /// OK (0) and Cancel (1) of the name box.
    fn name_button(&self, i: u32) -> Rect {
        let f = self.name_frame();
        let w = text_width("Cancel") + 4 * PAD;
        Rect {
            x: f.x + f.w - 2 * PAD - (2 - i) * w - (1 - i) * PAD,
            y: f.y + f.h - 2 * PAD - LINE,
            w,
            h: LINE,
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        let f = self.frame();
        window(fb, font, f, "Projects");
        let current = format!("Current: {}", project_label(&self.current));
        let x = f.x + f.w - 2 * PAD - text_width(&current);
        draw_text(fb, font, x, f.y + 6, &current, TEXT);

        let list = self.list();
        let files = self.files_rect();
        let heading_y = list.y - LINE + 4;
        draw_text(fb, font, list.x, heading_y, "Project", TEXT);
        let heading = format!("Files ({})", self.files.len());
        draw_text(fb, font, files.x, heading_y, &heading, TEXT);

        let shown = self.scroll..self.projects.len().min(self.scroll + self.rows());
        for i in shown {
            let r = self.row(i);
            let name = &self.projects[i];
            if i == self.selected {
                fill(fb, r, HIGHLIGHT);
            }
            let mark = if *name == self.current { "*" } else { " " };
            let mut label = format!("{mark} {}", project_label(name));
            if !usable(name) {
                label.push_str(" (rename to use)");
            }
            let color = if usable(name) { TEXT } else { ERROR };
            draw_text(
                fb,
                font,
                r.x + PAD,
                r.y + 4,
                &fit(&label, r.w - 2 * PAD),
                color,
            );
        }
        outline(fb, list, BORDER);

        let rows = (files.h / LINE) as usize;
        for (i, file) in self.files.iter().enumerate().take(rows) {
            let text = if i + 1 == rows && self.files.len() > rows {
                format!("... and {} more", self.files.len() - i)
            } else {
                fit(file, files.w - 2 * PAD)
            };
            let y = files.y + i as u32 * LINE + 4;
            draw_text(fb, font, files.x + PAD, y, &text, TEXT);
        }
        outline(fb, files, BORDER);

        if let Some((status, error)) = &self.status {
            let y = list.y + list.h + PAD + 4;
            let color = if *error { ERROR } else { TEXT };
            draw_text(fb, font, list.x, y, &fit(status, f.w - 4 * PAD), color);
        }
        let browsing = matches!(self.mode, Mode::Browse);
        for b in BUTTONS {
            let hovered = browsing && self.mouse.is_some_and(|m| self.button(b).contains(m));
            let enabled = self.enabled(b);
            button(fb, font, self.button(b), b.label(), hovered, enabled);
        }

        match &self.mode {
            Mode::Browse => {}
            Mode::Ask(dialog, _) => dialog.draw(fb, font),
            Mode::Name(name) => self.draw_name(fb, font, name),
        }
    }

    fn draw_name(&self, fb: &mut Framebuffer, font: &Font, name: &NameBox) {
        let f = self.name_frame();
        let title = match &name.naming {
            Naming::New => "New project".to_string(),
            Naming::Rename { from, .. } => format!("Rename project {from}"),
        };
        window(fb, font, f, &fit(&title, f.w - 4 * PAD));
        let x = f.x + 2 * PAD;
        let line = |i: u32| f.y + LINE + 2 * PAD + i * LINE + 4;
        draw_text(
            fb,
            font,
            x,
            line(0),
            "Letters, digits, _ - and . (up to 32);",
            TEXT,
        );
        draw_text(fb, font, x, line(1), "# first for a system project.", TEXT);
        let field = Rect {
            x,
            y: line(2) - 4 + PAD / 2,
            w: f.w - 4 * PAD,
            h: LINE,
        };
        fill(fb, field, 0xFF00_0000);
        outline(fb, field, BORDER);
        let text = format!("{}_", name.text.to_ascii_uppercase());
        draw_text(fb, font, field.x + PAD, field.y + 4, &text, TEXT);
        if let Some(message) = &name.message {
            let text = fit(message, f.w - 4 * PAD);
            draw_text(fb, font, x, line(3) + PAD, &text, ERROR);
        }
        for (i, label) in ["OK", "Cancel"].into_iter().enumerate() {
            let r = self.name_button(i as u32);
            let hovered = self.mouse.is_some_and(|m| r.contains(m));
            button(fb, font, r, label, hovered, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A fresh folder under the system temp directory, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let dir =
                std::env::temp_dir().join(format!("sb-projects-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn centre(r: Rect) -> (i32, i32) {
        ((r.x + r.w / 2) as i32, (r.y + r.h / 2) as i32)
    }

    #[test]
    fn create_and_switch() {
        let tmp = TempDir::new("create");
        let mut s = Storage::new(tmp.0.clone());
        let mut m = ProjectManager::new(&s, (1280, 720));
        assert_eq!(m.projects, [""]);
        assert!(!m.enabled(Button::Switch), "already in the workspace");
        assert_eq!(
            m.click(centre(m.button(Button::New)), &mut s, &[]),
            Outcome::Stay
        );
        m.text("game");
        assert_eq!(m.key(Keycode::Return, &mut s, &[]), Outcome::Stay);
        assert_eq!(m.projects, ["", "GAME"]);
        assert_eq!(m.selected_name(), "GAME");
        assert_eq!(
            m.key(Keycode::Return, &mut s, &[]),
            Outcome::Switched("GAME".to_string())
        );
        assert_eq!(s.project(), "GAME");
    }

    #[test]
    fn bad_names_are_refused() {
        let tmp = TempDir::new("bad");
        let mut s = Storage::new(tmp.0.clone());
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.press(Button::New, &mut s, &[]);
        m.text("a/b");
        m.key(Keycode::Return, &mut s, &[]);
        let Mode::Name(name) = &m.mode else {
            panic!("the name box stays open")
        };
        assert!(name.message.is_some());
        m.key(Keycode::Escape, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Browse));
        assert_eq!(m.projects, [""]);
    }

    #[test]
    fn unsaved_slots_ask_before_switching() {
        let tmp = TempDir::new("unsaved");
        let mut s = Storage::new(tmp.0.clone());
        s.create_project("GAME").unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.key(Keycode::Down, &mut s, &[1]);
        assert_eq!(m.key(Keycode::Return, &mut s, &[1]), Outcome::Stay);
        assert!(matches!(m.mode, Mode::Ask(_, Pending::Switch(_))));
        // The question starts on Cancel.
        m.key(Keycode::Return, &mut s, &[1]);
        assert_eq!(s.project(), "");
        m.key(Keycode::Return, &mut s, &[1]);
        m.key(Keycode::Left, &mut s, &[1]);
        assert_eq!(
            m.key(Keycode::Return, &mut s, &[1]),
            Outcome::Switched("GAME".to_string())
        );
    }

    #[test]
    fn hand_copied_folders_are_renamed_to_switch() {
        let tmp = TempDir::new("copied");
        let mut s = Storage::new(tmp.0.clone());
        std::fs::create_dir_all(tmp.0.join("my game")).unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.key(Keycode::Down, &mut s, &[]);
        assert_eq!(m.key(Keycode::Return, &mut s, &[]), Outcome::Stay);
        let Mode::Name(name) = &m.mode else {
            panic!("asks for a new name")
        };
        assert_eq!(name.text, "MY_GAME");
        assert_eq!(
            m.key(Keycode::Return, &mut s, &[]),
            Outcome::Switched("MY_GAME".to_string())
        );
    }

    #[test]
    fn delete_asks_first() {
        let tmp = TempDir::new("delete");
        let mut s = Storage::new(tmp.0.clone());
        s.create_project("A").unwrap();
        s.create_project("B").unwrap();
        s.set_project("B").unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        assert_eq!(m.selected_name(), "B");
        m.key(Keycode::Up, &mut s, &[]);
        m.key(Keycode::Delete, &mut s, &[]);
        m.key(Keycode::Return, &mut s, &[]);
        assert_eq!(m.projects, ["", "A", "B"], "Cancel is the default");
        m.key(Keycode::Delete, &mut s, &[]);
        m.key(Keycode::Left, &mut s, &[]);
        m.key(Keycode::Return, &mut s, &[]);
        assert_eq!(m.projects, ["", "B"]);
        assert!(!tmp.0.join("A").exists());

        // Deleting the current project goes back to the workspace.
        m.key(Keycode::End, &mut s, &[]);
        m.key(Keycode::Delete, &mut s, &[]);
        m.key(Keycode::Left, &mut s, &[]);
        m.key(Keycode::Return, &mut s, &[]);
        assert_eq!(s.project(), "");
        assert_eq!(m.current, "");
    }

    #[test]
    fn buttons_fit_and_do_not_overlap() {
        let tmp = TempDir::new("layout");
        let s = Storage::new(tmp.0.clone());
        let m = ProjectManager::new(&s, (1280, 720));
        let f = m.frame();
        for pair in BUTTONS.windows(2) {
            let (a, b) = (m.button(pair[0]), m.button(pair[1]));
            assert!(a.x + a.w < b.x, "{pair:?}");
        }
        let close = m.button(Button::Close);
        assert!(close.x + close.w <= f.x + f.w);
        assert!(m.rows() >= 10);
    }
}

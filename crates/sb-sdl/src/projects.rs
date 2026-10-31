//! The project manager: lists the projects and their files, and switches,
//! creates, renames, deletes, imports and exports projects. It also loads
//! a program into a slot, runs one, and copies and deletes files. Drawn
//! over the screen like the menu bar, it takes all input while it is open.

use sb_runtime::Framebuffer;
use sb_runtime::confirm::{project_label, switch_lines};
use sb_runtime::font::Font;
use sb_runtime::runtime::SLOTS;
use sb_runtime::storage::{FileKind, Storage, check_project_name, suggest_project_name};
use sb_runtime::transfer;
use sdl3::keyboard::Keycode;
use std::path::{Path, PathBuf};

use crate::dialog::{Answer, Dialog};
use crate::file_dialogs::{self, FilePicker};
use crate::ui::{
    BACKGROUND, BORDER, CHAR, DISABLED, HIGHLIGHT, LINE, PAD, Rect, TEXT, button, draw_text, fill,
    outline, text_width, window,
};

const WIDTH: u32 = 1200;
const HEIGHT: u32 = 600;
/// The width of the project list; the files take the rest.
const LIST_WIDTH: u32 = 460;
/// How far the mouse moves with a file before it is being dragged.
const DRAG_START: i32 = 6;
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
    /// Opens a menu of the rest: Import and Export.
    More,
    Import,
    Export,
    Close,
    /// Load the selected program into a slot.
    LoadTo,
    /// Run the selected program, or else the project's MAIN.PRG.
    Run,
    CopyTo,
    DeleteFile,
}

/// The buttons under the projects.
const PROJECT_BUTTONS: [Button; 4] = [Button::New, Button::Rename, Button::Delete, Button::More];
/// The buttons under the files.
const FILE_BUTTONS: [Button; 4] = [
    Button::LoadTo,
    Button::Run,
    Button::CopyTo,
    Button::DeleteFile,
];
/// The buttons at the bottom right.
const WINDOW_BUTTONS: [Button; 2] = [Button::Switch, Button::Close];
/// The menus of a right click on a project and on a file.
const PROJECT_MENU: [Button; 4] = [
    Button::Switch,
    Button::Rename,
    Button::Delete,
    Button::Export,
];
/// The program that Run runs when no program is selected.
const MAIN: &str = "MAIN.PRG";

impl Button {
    fn label(self) -> &'static str {
        match self {
            Button::Switch => "Switch",
            Button::New => "New",
            Button::Rename => "Rename",
            Button::Delete | Button::DeleteFile => "Delete",
            Button::More => "...",
            Button::Import => "Import...",
            Button::Export => "Export...",
            Button::Close => "Close",
            Button::LoadTo => "Load to...",
            Button::Run => "Run",
            Button::CopyTo => "Copy to...",
        }
    }
}

/// A file of the selected project.
#[derive(Clone, Debug, PartialEq, Eq)]
struct File {
    kind: FileKind,
    name: String,
    /// Its size and date.
    info: Option<(u64, String)>,
}

impl File {
    /// `TXT:NAME`.
    fn label(&self) -> String {
        format!("{}:{}", self.kind.name(), self.name)
    }
}

/// A file to copy or delete, wherever it is.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Source {
    project: String,
    kind: FileKind,
    name: String,
}

/// A right click's menu, or the one of the `...` button.
struct Popup {
    at: (u32, u32),
    items: Vec<Button>,
    chosen: Option<usize>,
}

/// A file pressed on in the list, which moving the mouse drags onto a
/// project to copy it there.
#[derive(Clone, Copy, Debug)]
struct Drag {
    file: usize,
    from: (i32, i32),
    moving: bool,
}

/// What the frontend should do after an event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
    /// The current project changed to the one named; the manager closes.
    Switched(String),
    /// Ask for a folder, then call [`ProjectManager::folder_chosen`].
    ChooseFolder(Transfer),
    /// Load a TXT file of the current project into a slot, or run it;
    /// the manager closes. `switched` names the project switched to first.
    Open {
        switched: Option<String>,
        open: Open,
    },
}

/// A program to load from the current project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    pub file: String,
    pub slot: usize,
    /// Run it after loading (into PRG0).
    pub run: bool,
}

/// What a chosen folder is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transfer {
    /// Export the named project into it, in SmileBASIC 4's files.
    Export(String),
    /// Import SmileBASIC 4's project in it as a new project.
    Import,
}

enum Naming {
    New,
    /// `then_switch` when renaming a folder whose name breaks the rules,
    /// so that it can be switched to.
    Rename {
        from: String,
        then_switch: bool,
    },
    /// A new project for the SmileBASIC 4 project in `from`.
    Import {
        from: PathBuf,
    },
}

struct NameBox {
    naming: Naming,
    text: String,
    message: Option<String>,
}

enum Pending {
    /// Switch, then open a program if given.
    Switch(String, Option<Open>),
    /// Open a program over a slot's unsaved changes.
    Open(Open),
    Delete(String),
    /// Export a project into a folder that already holds one.
    Export(String, PathBuf),
    DeleteFile(Source),
    /// Copy a file over one of the same name, in a project and as a name.
    Copy(Source, String, String),
}

enum Mode {
    Browse,
    Name(NameBox),
    Ask(Dialog, Pending),
    /// Choosing the slot to load the selected file into.
    Slot,
    Menu(Popup),
    /// Choosing where to copy a file to.
    Copy(Box<FilePicker>, Source),
}

/// Which list Up and Down move in; Tab switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Projects,
    Files,
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
    /// Files of the selected project.
    files: Vec<File>,
    file_selected: Option<usize>,
    /// The first file row shown.
    file_scroll: usize,
    focus: Focus,
    status: Option<(String, bool)>,
    mode: Mode,
    mouse: Option<(i32, i32)>,
    drag: Option<Drag>,
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
            file_selected: None,
            file_scroll: 0,
            focus: Focus::Projects,
            status: None,
            mode: Mode::Browse,
            mouse: None,
            drag: None,
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
        self.file_selected = None;
        self.file_scroll = 0;
        let project = self.selected_name().to_string();
        self.files = match storage.list(None, Some(&project)) {
            Ok(files) => files
                .into_iter()
                .map(|(kind, name)| File {
                    info: storage.file_info(&project, kind, &name),
                    kind,
                    name,
                })
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

    /// The selected file's name, if it is a TXT file.
    fn selected_program(&self) -> Option<&str> {
        let file = self.files.get(self.file_selected?)?;
        (file.kind == FileKind::Txt).then_some(file.name.as_str())
    }

    /// The selected file, in the selected project.
    fn selected_file(&self) -> Option<Source> {
        let file = self.files.get(self.file_selected?)?;
        Some(Source {
            project: self.selected_name().to_string(),
            kind: file.kind,
            name: file.name.clone(),
        })
    }

    fn select_file(&mut self, i: usize) {
        let Some(last) = self.files.len().checked_sub(1) else {
            return;
        };
        let i = i.min(last);
        self.file_selected = Some(i);
        let rows = self.file_rows();
        if i < self.file_scroll {
            self.file_scroll = i;
        } else if i >= self.file_scroll + rows {
            self.file_scroll = i + 1 - rows;
        }
    }

    fn enabled(&self, button: Button) -> bool {
        let name = self.selected_name();
        match button {
            Button::Switch => name != self.current,
            Button::Rename | Button::Delete => !name.is_empty(),
            Button::LoadTo => self.selected_program().is_some(),
            Button::Run => {
                self.selected_program().is_some()
                    || (self.files.iter()).any(|f| f.kind == FileKind::Txt && f.name == MAIN)
            }
            Button::CopyTo | Button::DeleteFile => self.selected_file().is_some(),
            Button::New | Button::More | Button::Import | Button::Export | Button::Close => true,
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
        let y = f.y + 2 * LINE + 2 * PAD;
        Rect {
            x: f.x + 2 * PAD,
            y,
            w: LIST_WIDTH,
            // Under the list: its buttons, then the status line and the
            // window's buttons.
            h: self.group_y() - PAD - y,
        }
    }

    /// The row of the buttons under the lists.
    fn group_y(&self) -> u32 {
        self.bottom_y() - PAD - LINE
    }

    /// The bottom row: the status line, Switch and Close.
    fn bottom_y(&self) -> u32 {
        let f = self.frame();
        f.y + f.h - 2 * PAD - LINE
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

    fn file_rows(&self) -> usize {
        (self.files_rect().h / LINE) as usize
    }

    fn file_row(&self, i: usize) -> Rect {
        let files = self.files_rect();
        Rect {
            y: files.y + (i - self.file_scroll) as u32 * LINE,
            h: LINE,
            ..files
        }
    }

    fn shown_files(&self) -> std::ops::Range<usize> {
        self.file_scroll..self.files.len().min(self.file_scroll + self.file_rows())
    }

    fn row(&self, i: usize) -> Rect {
        let list = self.list();
        Rect {
            y: list.y + (i - self.scroll) as u32 * LINE,
            h: LINE,
            ..list
        }
    }

    /// A button of the rows under the lists, or of the bottom right.
    /// Import and Export, which are in the `...` menu, have no place.
    fn button(&self, button: Button) -> Rect {
        let f = self.frame();
        let width = |b: Button| text_width(b.label()) + 4 * PAD;
        let before = |row: &[Button]| -> u32 {
            (row.iter())
                .take_while(|&&b| b != button)
                .map(|&b| width(b) + PAD)
                .sum()
        };
        let (x, y) = if PROJECT_BUTTONS.contains(&button) {
            (self.list().x + before(&PROJECT_BUTTONS), self.group_y())
        } else if FILE_BUTTONS.contains(&button) {
            (self.files_rect().x + before(&FILE_BUTTONS), self.group_y())
        } else if WINDOW_BUTTONS.contains(&button) {
            let all: u32 = WINDOW_BUTTONS.iter().map(|&b| width(b) + PAD).sum();
            let left = f.x + f.w - 2 * PAD + PAD - all;
            (left + before(&WINDOW_BUTTONS), self.bottom_y())
        } else {
            return Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            };
        };
        Rect {
            x,
            y,
            w: width(button),
            h: LINE,
        }
    }

    fn buttons() -> impl Iterator<Item = Button> {
        PROJECT_BUTTONS
            .into_iter()
            .chain(FILE_BUTTONS)
            .chain(WINDOW_BUTTONS)
    }

    fn project_at(&self, at: (i32, i32)) -> Option<usize> {
        let shown = self.scroll..self.projects.len().min(self.scroll + self.rows());
        shown.into_iter().find(|&i| self.row(i).contains(at))
    }

    fn file_at(&self, at: (i32, i32)) -> Option<usize> {
        self.shown_files().find(|&i| self.file_row(i).contains(at))
    }

    fn popup_width(items: &[Button]) -> u32 {
        items
            .iter()
            .map(|b| text_width(b.label()))
            .max()
            .unwrap_or(0)
            + 4 * PAD
    }

    /// A menu item's place, kept inside the window.
    fn popup_item(&self, popup: &Popup, i: usize) -> Rect {
        let f = self.frame();
        let w = Self::popup_width(&popup.items);
        let h = popup.items.len() as u32 * LINE;
        let x = popup.at.0.min(f.x + f.w - w - 2);
        let y = popup.at.1.min(f.y + f.h - h - 2);
        Rect {
            x,
            y: y + i as u32 * LINE,
            w,
            h: LINE,
        }
    }

    fn open_menu(&mut self, at: (u32, u32), items: &[Button]) {
        self.mode = Mode::Menu(Popup {
            at,
            items: items.to_vec(),
            chosen: None,
        });
    }

    pub fn mouse_move(&mut self, at: (i32, i32)) {
        self.mouse = Some(at);
        match &mut self.mode {
            Mode::Ask(dialog, _) => dialog.mouse_move(at),
            Mode::Copy(picker, _) => picker.mouse_move(at),
            Mode::Menu(popup) => {
                let items = popup.items.len();
                let hit = (0..items).find(|&i| self.popup_item_at(i).contains(at));
                if let Mode::Menu(popup) = &mut self.mode {
                    popup.chosen = hit;
                }
            }
            _ => {}
        }
        if let Some(drag) = &mut self.drag {
            let (dx, dy) = (at.0 - drag.from.0, at.1 - drag.from.1);
            drag.moving |= dx.abs().max(dy.abs()) >= DRAG_START;
        }
    }

    fn popup_item_at(&self, i: usize) -> Rect {
        match &self.mode {
            Mode::Menu(popup) => self.popup_item(popup, i),
            _ => unreachable!("only a menu has items"),
        }
    }

    /// Scrolls the list under the mouse by `lines`, down for positive.
    pub fn wheel(&mut self, lines: i32) {
        if let Mode::Copy(picker, _) = &mut self.mode {
            picker.wheel(lines);
            return;
        }
        if !matches!(self.mode, Mode::Browse) {
            return;
        }
        if self.mouse.is_some_and(|m| self.files_rect().contains(m)) {
            let last = self.files.len().saturating_sub(self.file_rows());
            self.file_scroll = (self.file_scroll.saturating_add_signed(lines as isize)).min(last);
            return;
        }
        let last = self.projects.len().saturating_sub(self.rows());
        self.scroll = self.scroll.saturating_add_signed(lines as isize).min(last);
    }

    /// Typed text, for the name box.
    pub fn text(&mut self, text: &str) {
        match &mut self.mode {
            Mode::Name(name) => {
                for c in text.chars().filter(char::is_ascii_graphic) {
                    if name.text.len() < MAX_TYPED {
                        name.text.push(c);
                    }
                }
            }
            Mode::Copy(picker, _) => picker.text(text),
            _ => {}
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
            Mode::Slot => {
                let chosen = (0..=SLOTS).find(|&i| self.slot_button(i).contains(at));
                match chosen {
                    Some(SLOTS) => self.mode = Mode::Browse,
                    Some(slot) => return self.load_to(slot, storage, unsaved),
                    None => {}
                }
                Outcome::Stay
            }
            Mode::Menu(popup) => {
                let items = popup.items.clone();
                let hit = (0..items.len()).find(|&i| self.popup_item_at(i).contains(at));
                self.mode = Mode::Browse;
                match hit {
                    Some(i) => self.press(items[i], storage, unsaved),
                    None => Outcome::Stay,
                }
            }
            Mode::Copy(picker, _) => {
                let outcome = picker.click(at, storage);
                self.picked(outcome, storage)
            }
            Mode::Browse => {
                if let Some(b) = Self::buttons().find(|&b| self.button(b).contains(at)) {
                    if b == Button::More {
                        let r = self.button(b);
                        self.open_menu((r.x, r.y + r.h), &[Button::Import, Button::Export]);
                        return Outcome::Stay;
                    }
                    return self.press(b, storage, unsaved);
                }
                if let Some(i) = self.project_at(at) {
                    self.select(i, storage);
                    self.focus = Focus::Projects;
                }
                if let Some(i) = self.file_at(at) {
                    self.select_file(i);
                    self.focus = Focus::Files;
                    self.drag = Some(Drag {
                        file: i,
                        from: at,
                        moving: false,
                    });
                }
                Outcome::Stay
            }
        }
    }

    /// A double click on a project switches to it and closes.
    pub fn double_click(
        &mut self,
        at: (i32, i32),
        storage: &mut Storage,
        unsaved: &[usize],
    ) -> Outcome {
        if !matches!(self.mode, Mode::Browse) {
            return Outcome::Stay;
        }
        match self.project_at(at) {
            Some(i) if self.projects[i] == self.current => Outcome::Close,
            Some(i) => {
                self.select(i, storage);
                self.press(Button::Switch, storage, unsaved)
            }
            None => Outcome::Stay,
        }
    }

    /// A right click on a project or a file opens a menu of what can be
    /// done with it.
    pub fn right_click(&mut self, at: (i32, i32), storage: &Storage) {
        if !matches!(self.mode, Mode::Browse | Mode::Menu(_)) {
            return;
        }
        self.mode = Mode::Browse;
        let place = (at.0.max(0) as u32, at.1.max(0) as u32);
        if let Some(i) = self.project_at(at) {
            self.select(i, storage);
            self.focus = Focus::Projects;
            self.open_menu(place, &PROJECT_MENU);
        } else if let Some(i) = self.file_at(at) {
            self.select_file(i);
            self.focus = Focus::Files;
            self.open_menu(place, &FILE_BUTTONS);
        }
    }

    /// Letting go of the left button: a file dragged onto a project is
    /// copied there under its own name.
    pub fn release(&mut self, at: (i32, i32), storage: &Storage) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if !drag.moving || !matches!(self.mode, Mode::Browse) {
            return;
        }
        let (Some(file), Some(to)) = (self.files.get(drag.file), self.project_at(at)) else {
            return;
        };
        if to == self.selected {
            return;
        }
        let source = Source {
            project: self.selected_name().to_string(),
            kind: file.kind,
            name: file.name.clone(),
        };
        let to = self.projects[to].clone();
        let name = source.name.clone();
        self.copy(source, to, name, storage, false);
    }

    /// Up and Down choose a project, or a file after Tab. Enter switches to
    /// the project, or loads the file into a slot. Esc and F12 close.
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
            Mode::Menu(popup) => {
                let n = popup.items.len();
                match key {
                    Keycode::Escape => self.mode = Mode::Browse,
                    Keycode::Up => {
                        popup.chosen = Some(popup.chosen.map_or(n - 1, |i| (i + n - 1) % n))
                    }
                    Keycode::Down => popup.chosen = Some(popup.chosen.map_or(0, |i| (i + 1) % n)),
                    Keycode::Return | Keycode::KpEnter => {
                        if let Some(i) = popup.chosen {
                            let chosen = popup.items[i];
                            self.mode = Mode::Browse;
                            return self.press(chosen, storage, unsaved);
                        }
                    }
                    _ => {}
                }
                Outcome::Stay
            }
            Mode::Copy(picker, _) => {
                let outcome = picker.key(key, storage);
                self.picked(outcome, storage)
            }
            Mode::Slot => {
                let digit = (key as i32).checked_sub(Keycode::_0 as i32);
                match digit {
                    Some(d @ 0..) if (d as usize) < SLOTS => {
                        return self.load_to(d as usize, storage, unsaved);
                    }
                    _ if key == Keycode::Escape => self.mode = Mode::Browse,
                    _ => {}
                }
                Outcome::Stay
            }
            Mode::Browse if key == Keycode::Tab => {
                self.focus = match self.focus {
                    Focus::Projects if !self.files.is_empty() => {
                        if self.file_selected.is_none() {
                            self.select_file(0);
                        }
                        Focus::Files
                    }
                    _ => Focus::Projects,
                };
                Outcome::Stay
            }
            Mode::Browse if key == Keycode::F12 => Outcome::Close,
            Mode::Browse if self.focus == Focus::Files => {
                let last = self.files.len().saturating_sub(1);
                let page = self.file_rows().saturating_sub(1).max(1);
                let at = self.file_selected.unwrap_or(0);
                let to = match key {
                    Keycode::Up => at.saturating_sub(1),
                    Keycode::Down => (at + 1).min(last),
                    Keycode::PageUp => at.saturating_sub(page),
                    Keycode::PageDown => (at + page).min(last),
                    Keycode::Home => 0,
                    Keycode::End => last,
                    Keycode::Return | Keycode::KpEnter => {
                        return self.press(Button::LoadTo, storage, unsaved);
                    }
                    Keycode::Delete => return self.press(Button::DeleteFile, storage, unsaved),
                    Keycode::Escape => return Outcome::Close,
                    _ => return Outcome::Stay,
                };
                self.select_file(to);
                Outcome::Stay
            }
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
            Button::Switch => return self.switch(name, storage, unsaved, false, None),
            Button::LoadTo => self.mode = Mode::Slot,
            Button::Run => {
                let open = Open {
                    file: self.selected_program().unwrap_or(MAIN).to_string(),
                    slot: 0,
                    run: true,
                };
                return self.open(open, storage, unsaved, false);
            }
            Button::CopyTo => {
                let source = self.selected_file().expect("enabled");
                let picker = FilePicker::copy(
                    self.size,
                    (source.kind, &source.name),
                    &source.project,
                    storage,
                );
                self.mode = Mode::Copy(Box::new(picker), source);
            }
            Button::DeleteFile => {
                let source = self.selected_file().expect("enabled");
                let lines = vec![
                    format!(
                        "Delete {}:{} from {}.",
                        source.kind.name(),
                        source.name,
                        project_label(&source.project)
                    ),
                    String::new(),
                    "This cannot be undone.".to_string(),
                ];
                let dialog = Dialog::new("Delete file", lines, "Delete", self.size);
                self.mode = Mode::Ask(dialog, Pending::DeleteFile(source));
            }
            Button::More => {
                let r = self.button(Button::More);
                self.open_menu((r.x, r.y + r.h), &[Button::Import, Button::Export]);
            }
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
            Button::Import => return Outcome::ChooseFolder(Transfer::Import),
            Button::Export => return Outcome::ChooseFolder(Transfer::Export(name)),
            Button::Close => return Outcome::Close,
        }
        Outcome::Stay
    }

    /// Goes on with `transfer` once the frontend's folder dialog has
    /// given `folder`.
    pub fn folder_chosen(&mut self, transfer: Transfer, folder: PathBuf, storage: &Storage) {
        if !matches!(self.mode, Mode::Browse) {
            return;
        }
        match transfer {
            Transfer::Import => {
                let text = transfer::import_name(&folder);
                let naming = Naming::Import { from: folder };
                self.ask_name(naming, text, None);
            }
            Transfer::Export(name) => {
                let target = transfer::export_target(&name, &folder);
                let used = std::fs::read_dir(&target).is_ok_and(|mut d| d.next().is_some());
                if !used {
                    return self.export(&name, &folder, storage);
                }
                let shown = target.file_name().unwrap_or_default().to_string_lossy();
                let lines = vec![
                    format!("The folder {shown} is already there."),
                    String::new(),
                    "Its files of the same names will be replaced.".to_string(),
                ];
                let dialog = Dialog::new("Export project", lines, "Replace", self.size);
                self.mode = Mode::Ask(dialog, Pending::Export(name, folder));
            }
        }
    }

    /// What the window choosing where to copy to gave.
    fn picked(&mut self, outcome: file_dialogs::Outcome, storage: &Storage) -> Outcome {
        match outcome {
            file_dialogs::Outcome::Close => self.mode = Mode::Browse,
            file_dialogs::Outcome::Copy { project, name } => {
                let Mode::Copy(_, source) = std::mem::replace(&mut self.mode, Mode::Browse) else {
                    unreachable!("only copying picks")
                };
                self.copy(source, project, name, storage, false);
            }
            _ => {}
        }
        Outcome::Stay
    }

    /// Copies a file to `to` as `name`, first asking before replacing a
    /// file there (unless `asked`).
    fn copy(&mut self, source: Source, to: String, name: String, storage: &Storage, asked: bool) {
        let name = name.to_ascii_uppercase();
        let kind = source.kind.name();
        if to == source.project && name == source.name {
            self.status = Some((format!("{kind}:{name} is already there."), true));
            return;
        }
        if !asked && storage.exists_in(&to, source.kind, &name) {
            let lines = vec![
                format!("{kind}:{name} is already in {}.", project_label(&to)),
                String::new(),
                "It will be replaced.".to_string(),
            ];
            let dialog = Dialog::new("Copy file", lines, "Replace", self.size);
            self.mode = Mode::Ask(dialog, Pending::Copy(source, to, name));
            return;
        }
        let from = (source.project.as_str(), source.name.as_str());
        self.status = Some(match storage.copy_file(source.kind, from, (&to, &name)) {
            Ok(name) => {
                let place = project_label(&to);
                (
                    format!("Copied {kind}:{} to {place} as {name}.", source.name),
                    false,
                )
            }
            Err(e) => (format!("Could not copy {kind}:{}: {e}", source.name), true),
        });
        if to == self.selected_name() {
            let kept = self.file_selected;
            self.select(self.selected, storage);
            if let Some(i) = kept {
                self.select_file(i);
            }
        }
    }

    fn export(&mut self, name: &str, folder: &Path, storage: &Storage) {
        let label = project_label(name);
        self.status = Some(match transfer::export_project(storage, name, folder) {
            Ok(summary) => (summary.line(&format!("Exported {label}")), false),
            Err(e) => (format!("Could not export {label}: {e}"), true),
        });
    }

    fn ask_name(&mut self, naming: Naming, text: String, message: Option<String>) {
        self.mode = Mode::Name(NameBox {
            naming,
            text,
            message,
        });
    }

    /// Loads the selected file into `slot`.
    fn load_to(&mut self, slot: usize, storage: &mut Storage, unsaved: &[usize]) -> Outcome {
        self.mode = Mode::Browse;
        let Some(file) = self.selected_program() else {
            return Outcome::Stay;
        };
        let open = Open {
            file: file.to_string(),
            slot,
            run: false,
        };
        self.open(open, storage, unsaved, false)
    }

    /// Opens a program of the selected project, switching to it first if
    /// it is not the current one. Replacing a slot's unsaved changes asks
    /// first (unless `asked`); switching asks about all of them.
    fn open(
        &mut self,
        open: Open,
        storage: &mut Storage,
        unsaved: &[usize],
        asked: bool,
    ) -> Outcome {
        let name = self.selected_name().to_string();
        if name != self.current {
            return self.switch(name, storage, unsaved, false, Some(open));
        }
        if !asked && unsaved.contains(&open.slot) {
            let lines = vec![
                format!("PRG{} has changes that were not saved.", open.slot),
                String::new(),
                format!("Loading TXT:{} replaces them.", open.file),
            ];
            let accept = if open.run { "Run" } else { "Load" };
            let dialog = Dialog::new("Unsaved program", lines, accept, self.size);
            self.mode = Mode::Ask(dialog, Pending::Open(open));
            return Outcome::Stay;
        }
        Outcome::Open {
            switched: None,
            open,
        }
    }

    /// Switches to `name`, first asking if slots have unsaved edits
    /// (unless `asked`), or for a new name if the folder's breaks the rules.
    /// Then opens `then`, if given.
    fn switch(
        &mut self,
        name: String,
        storage: &mut Storage,
        unsaved: &[usize],
        asked: bool,
        then: Option<Open>,
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
            self.mode = Mode::Ask(dialog, Pending::Switch(name, then));
            return Outcome::Stay;
        }
        self.mode = Mode::Browse;
        match storage.set_project(&name) {
            Ok(()) => {
                let switched = project_label(storage.project()).to_string();
                match then {
                    Some(open) => Outcome::Open {
                        switched: Some(switched),
                        open,
                    },
                    None => Outcome::Switched(switched),
                }
            }
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
            Pending::Switch(name, then) => return self.switch(name, storage, unsaved, true, then),
            Pending::Open(open) => return self.open(open, storage, unsaved, true),
            Pending::Export(name, folder) => self.export(&name, &folder, storage),
            Pending::Copy(source, to, name) => self.copy(source, to, name, storage, true),
            Pending::DeleteFile(source) => {
                let label = format!("{}:{}", source.kind.name(), source.name);
                let done = storage.delete_in(&source.project, source.kind, &source.name);
                self.status = Some(match done {
                    Ok(()) => (format!("Deleted {label}."), false),
                    Err(e) => (format!("Could not delete {label}: {e}"), true),
                });
                let kept = self.file_selected;
                self.select(self.selected, storage);
                if let Some(i) = kept {
                    self.select_file(i);
                }
            }
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
            Naming::New | Naming::Import { .. } => storage.create_project(&name.text),
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
            Naming::Import { from } => {
                self.status = Some(match transfer::import_project(storage, &from, &new) {
                    Ok(summary) => (summary.line(&format!("Imported {new}")), false),
                    Err(e) => (format!("Could not import into {new}: {e}"), true),
                });
                self.refresh(storage, Some(&new));
            }
            Naming::Rename {
                then_switch: true, ..
            } => return self.switch(new, storage, unsaved, false, None),
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

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, blink: bool) {
        let f = self.frame();
        window(fb, font, f, "Projects");
        let current = format!("Current: {}", project_label(&self.current));
        let x = f.x + f.w - 2 * PAD - text_width(&current);
        draw_text(fb, font, x, f.y + 6, &current, TEXT);

        let list = self.list();
        let files = self.files_rect();
        let heading_y = list.y - LINE + 4;
        draw_text(fb, font, list.x, heading_y, "Projects", TEXT);
        let place = project_label(self.selected_name());
        let heading = format!("Files in {place} ({})", self.files.len());
        draw_text(fb, font, files.x, heading_y, &fit(&heading, files.w), TEXT);

        let shown = self.scroll..self.projects.len().min(self.scroll + self.rows());
        for i in shown {
            let r = self.row(i);
            let name = &self.projects[i];
            if i == self.selected {
                let color = match self.focus {
                    Focus::Projects => HIGHLIGHT,
                    Focus::Files => BORDER,
                };
                fill(fb, r, color);
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

        for i in self.shown_files() {
            let r = self.file_row(i);
            if self.file_selected == Some(i) {
                let color = match self.focus {
                    Focus::Files => HIGHLIGHT,
                    Focus::Projects => BORDER,
                };
                fill(fb, r, color);
            }
            // The size and the date to the minute on the right, if the
            // name leaves room.
            let file = &self.files[i];
            let info = (file.info.as_ref())
                .map(|(size, date)| format!("{size:>7}  {}", date.get(..16).unwrap_or(date)));
            let info_w = info.as_ref().map_or(0, |i| text_width(i) + 2 * PAD);
            let text = fit(&file.label(), r.w - 2 * PAD - info_w);
            draw_text(fb, font, r.x + PAD, r.y + 4, &text, TEXT);
            if let Some(info) = info {
                let x = r.x + r.w - PAD - text_width(&info);
                draw_text(fb, font, x, r.y + 4, &info, DISABLED);
            }
        }
        outline(fb, files, BORDER);

        if let Some((status, error)) = &self.status {
            let y = self.bottom_y() + 4;
            let color = if *error { ERROR } else { TEXT };
            let room = self.button(Button::Switch).x.saturating_sub(list.x + PAD);
            draw_text(fb, font, list.x, y, &fit(status, room), color);
        }
        let browsing = matches!(self.mode, Mode::Browse);
        for b in Self::buttons() {
            let hovered = browsing && self.mouse.is_some_and(|m| self.button(b).contains(m));
            let enabled = self.enabled(b);
            button(fb, font, self.button(b), b.label(), hovered, enabled);
        }
        // The file being dragged, by the mouse.
        if let (Some(drag), Some(at)) = (self.drag, self.mouse)
            && drag.moving
            && let Some(file) = self.files.get(drag.file)
        {
            if let Some(to) = self.project_at(at).filter(|&to| to != self.selected) {
                outline(fb, self.row(to), TEXT);
            }
            let label = file.label();
            let r = Rect {
                x: at.0.max(0) as u32 + PAD,
                y: at.1.max(0) as u32,
                w: text_width(&label) + 2 * PAD,
                h: LINE,
            };
            fill(fb, r, HIGHLIGHT);
            draw_text(fb, font, r.x + PAD, r.y + 4, &label, TEXT);
        }

        match &self.mode {
            Mode::Browse => {}
            Mode::Ask(dialog, _) => dialog.draw(fb, font),
            Mode::Name(name) => self.draw_name(fb, font, name),
            Mode::Slot => self.draw_slot(fb, font),
            Mode::Copy(picker, _) => picker.draw(fb, font, blink),
            Mode::Menu(popup) => self.draw_menu(fb, font, popup),
        }
    }

    fn draw_menu(&self, fb: &mut Framebuffer, font: &Font, popup: &Popup) {
        let first = self.popup_item(popup, 0);
        let all = Rect {
            h: popup.items.len() as u32 * LINE,
            ..first
        };
        fill(fb, all, BACKGROUND | 0xFF00_0000);
        for (i, &item) in popup.items.iter().enumerate() {
            let r = self.popup_item(popup, i);
            let enabled = self.enabled(item);
            if popup.chosen == Some(i) && enabled {
                fill(fb, r, HIGHLIGHT);
            }
            let color = if enabled { TEXT } else { DISABLED };
            draw_text(fb, font, r.x + 2 * PAD, r.y + 4, item.label(), color);
        }
        outline(fb, all, BORDER);
    }

    fn slot_frame(&self) -> Rect {
        let (w, h) = (760, 4 * LINE + 6 * PAD);
        Rect {
            x: self.size.0.saturating_sub(w) / 2,
            y: self.size.1.saturating_sub(h) / 2,
            w,
            h,
        }
    }

    /// PRG0 and on, then Cancel (`SLOTS`), in the slot chooser.
    fn slot_button(&self, i: usize) -> Rect {
        let f = self.slot_frame();
        let w = text_width("PRG0") + 3 * PAD;
        let y = f.y + f.h - 2 * PAD - LINE;
        if i == SLOTS {
            let w = text_width("Cancel") + 4 * PAD;
            return Rect {
                x: f.x + f.w - 2 * PAD - w,
                y,
                w,
                h: LINE,
            };
        }
        Rect {
            x: f.x + 2 * PAD + i as u32 * (w + PAD),
            y,
            w,
            h: LINE,
        }
    }

    fn draw_slot(&self, fb: &mut Framebuffer, font: &Font) {
        let f = self.slot_frame();
        let file = self.selected_program().unwrap_or_default();
        window(
            fb,
            font,
            f,
            &fit(&format!("Load TXT:{file}"), f.w - 4 * PAD),
        );
        let question = format!("Into which slot? (0 to {})", SLOTS - 1);
        draw_text(
            fb,
            font,
            f.x + 2 * PAD,
            f.y + LINE + 2 * PAD + 4,
            &question,
            TEXT,
        );
        for i in 0..=SLOTS {
            let r = self.slot_button(i);
            let label = match i {
                SLOTS => "Cancel".to_string(),
                i => format!("PRG{i}"),
            };
            let hovered = self.mouse.is_some_and(|m| r.contains(m));
            button(fb, font, r, &label, hovered, true);
        }
    }

    fn draw_name(&self, fb: &mut Framebuffer, font: &Font, name: &NameBox) {
        let f = self.name_frame();
        let title = match &name.naming {
            Naming::New => "New project".to_string(),
            Naming::Rename { from, .. } => format!("Rename project {from}"),
            Naming::Import { from } => {
                let folder = from.file_name().unwrap_or_default().to_string_lossy();
                format!("Import project {folder}")
            }
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
        assert!(matches!(m.mode, Mode::Ask(_, Pending::Switch(..))));
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
    fn export_and_import() {
        let tmp = TempDir::new("transfer");
        let mut s = Storage::new(tmp.0.join("projects"));
        s.create_project("GAME").unwrap();
        s.set_project("GAME").unwrap();
        s.write(
            sb_runtime::storage::FileKind::Txt,
            "MAIN.PRG",
            b"?1\n",
            None,
        )
        .unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        let out = tmp.0.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let Outcome::ChooseFolder(export) = m.press(Button::Export, &mut s, &[]) else {
            panic!("asks for a folder")
        };
        assert_eq!(export, Transfer::Export("GAME".to_string()));
        m.folder_chosen(export.clone(), out.clone(), &s);
        assert!(out.join("GAME/TMAIN.PRG").is_file());
        assert!(matches!(m.mode, Mode::Browse));

        // A second export asks before replacing.
        m.folder_chosen(export, out.clone(), &s);
        assert!(matches!(m.mode, Mode::Ask(_, Pending::Export(..))));
        m.key(Keycode::Escape, &mut s, &[]);

        assert_eq!(
            m.press(Button::Import, &mut s, &[]),
            Outcome::ChooseFolder(Transfer::Import)
        );
        m.folder_chosen(Transfer::Import, out.join("GAME"), &s);
        let Mode::Name(name) = &m.mode else {
            panic!("asks for a name")
        };
        assert_eq!(name.text, "GAME");
        m.key(Keycode::Return, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Name(_)), "GAME is taken");
        m.text("2");
        m.key(Keycode::Return, &mut s, &[]);
        assert_eq!(m.selected_name(), "GAME2");
        let files: Vec<String> = m.files.iter().map(File::label).collect();
        assert_eq!(files, ["TXT:MAIN.PRG"]);
    }

    #[test]
    fn load_to_and_run() {
        use sb_runtime::storage::FileKind;
        let tmp = TempDir::new("open");
        let mut s = Storage::new(tmp.0.clone());
        s.write(FileKind::Txt, "A.PRG", b"?1", None).unwrap();
        s.write(FileKind::Dat, "D", b"", None).unwrap();
        s.create_project("GAME").unwrap();
        s.set_project("GAME").unwrap();
        s.write(FileKind::Txt, MAIN, b"?2", None).unwrap();
        s.set_project("").unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        assert!(!m.enabled(Button::LoadTo) && !m.enabled(Button::Run));

        // Tab moves to the files; DAT files cannot be loaded.
        m.key(Keycode::Tab, &mut s, &[]);
        assert_eq!(m.selected_program(), Some("A.PRG"));
        m.key(Keycode::Down, &mut s, &[]);
        assert!(!m.enabled(Button::LoadTo));
        m.key(Keycode::Up, &mut s, &[]);
        m.key(Keycode::Return, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Slot));
        let open = Open {
            file: "A.PRG".to_string(),
            slot: 2,
            run: false,
        };
        assert_eq!(
            m.key(Keycode::_2, &mut s, &[]),
            Outcome::Open {
                switched: None,
                open: open.clone()
            }
        );
        // Over unsaved changes, it asks first.
        m.key(Keycode::Return, &mut s, &[2]);
        m.key(Keycode::_2, &mut s, &[2]);
        assert!(matches!(m.mode, Mode::Ask(_, Pending::Open(_))));
        m.key(Keycode::Left, &mut s, &[2]);
        assert_eq!(
            m.key(Keycode::Return, &mut s, &[2]),
            Outcome::Open {
                switched: None,
                open
            }
        );

        // Run switches to the project first.
        m.key(Keycode::Tab, &mut s, &[]);
        m.key(Keycode::Down, &mut s, &[]);
        assert!(m.enabled(Button::Run));
        let at = centre(m.button(Button::Run));
        assert_eq!(
            m.click(at, &mut s, &[]),
            Outcome::Open {
                switched: Some("GAME".to_string()),
                open: Open {
                    file: MAIN.to_string(),
                    slot: 0,
                    run: true
                }
            }
        );
        assert_eq!(s.project(), "GAME");
    }

    #[test]
    fn long_file_lists_scroll() {
        use sb_runtime::storage::FileKind;
        let tmp = TempDir::new("scroll");
        let mut s = Storage::new(tmp.0.clone());
        for i in 0..60 {
            s.write(FileKind::Dat, &format!("D{i:02}"), b"", None)
                .unwrap();
        }
        let mut m = ProjectManager::new(&s, (1280, 720));
        let rows = m.file_rows();
        assert!(rows < 60);
        m.mouse_move(centre(m.files_rect()));
        m.wheel(3);
        assert_eq!((m.file_scroll, m.scroll), (3, 0));
        m.wheel(100);
        assert_eq!(m.file_scroll, 60 - rows);
        m.key(Keycode::Tab, &mut s, &[]);
        assert_eq!(m.file_scroll, 0, "the selection is kept in view");
        m.key(Keycode::End, &mut s, &[]);
        assert_eq!(m.file_selected, Some(59));
        assert_eq!(m.file_scroll, 60 - rows);
    }

    #[test]
    fn buttons_fit_and_do_not_overlap() {
        let tmp = TempDir::new("layout");
        let s = Storage::new(tmp.0.clone());
        let m = ProjectManager::new(&s, (1280, 720));
        let f = m.frame();
        for row in [&PROJECT_BUTTONS[..], &FILE_BUTTONS, &WINDOW_BUTTONS] {
            for pair in row.windows(2) {
                let (a, b) = (m.button(pair[0]), m.button(pair[1]));
                assert!(a.x + a.w < b.x, "{pair:?}");
            }
        }
        let (list, files) = (m.list(), m.files_rect());
        let last = m.button(Button::More);
        assert!(last.x + last.w <= list.x + list.w, "under the projects");
        let last = m.button(Button::DeleteFile);
        assert!(last.x + last.w <= files.x + files.w, "under the files");
        assert!(m.button(Button::LoadTo).y > files.y + files.h);
        assert!(m.button(Button::Close).y > m.button(Button::LoadTo).y + LINE);
        let last = m.slot_button(SLOTS - 1);
        assert!(last.x + last.w < m.slot_button(SLOTS).x);
        let close = m.button(Button::Close);
        assert!(close.x + close.w <= f.x + f.w);
        assert!(m.rows() >= 10);
    }

    #[test]
    fn double_click_switches_and_closes() {
        let tmp = TempDir::new("double");
        let mut s = Storage::new(tmp.0.clone());
        s.create_project("GAME").unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        let current = centre(m.row(0));
        assert_eq!(m.double_click(current, &mut s, &[]), Outcome::Close);
        let game = centre(m.row(1));
        assert_eq!(
            m.double_click(game, &mut s, &[]),
            Outcome::Switched("GAME".to_string())
        );
    }

    #[test]
    fn files_are_copied_and_deleted() {
        let tmp = TempDir::new("files");
        let mut s = Storage::new(tmp.0.clone());
        s.write(FileKind::Txt, "A.PRG", b"?1", None).unwrap();
        s.write(FileKind::Dat, "D", b"", None).unwrap();
        s.create_project("GAME").unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        let label =
            |m: &ProjectManager| -> Vec<String> { m.files.iter().map(File::label).collect() };

        // Dragging a file onto a project copies it there.
        let a = centre(m.file_row(0));
        m.click(a, &mut s, &[]);
        m.mouse_move((a.0 + 20, a.1));
        m.release(centre(m.row(1)), &s);
        assert!(s.exists_in("GAME", FileKind::Txt, "A.PRG"));
        // Again, it asks before replacing.
        m.click(a, &mut s, &[]);
        m.mouse_move((a.0 + 20, a.1));
        m.release(centre(m.row(1)), &s);
        assert!(matches!(m.mode, Mode::Ask(_, Pending::Copy(..))));
        m.key(Keycode::Escape, &mut s, &[]);
        // A click without moving copies nothing.
        m.click(a, &mut s, &[]);
        m.release(centre(m.row(1)), &s);
        assert!(matches!(m.mode, Mode::Browse));

        // Copy to... names the copy in the file list.
        m.press(Button::CopyTo, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Copy(..)));
        m.text("B.PRG");
        m.key(Keycode::Return, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Browse));
        assert_eq!(label(&m), ["TXT:A.PRG", "TXT:A.PRGB.PRG", "DAT:D"]);
        m.press(Button::CopyTo, &mut s, &[]);
        // Erasing the whole name, then once more, goes up to the projects.
        for _ in 0.."A.PRG".len() {
            m.key(Keycode::Backspace, &mut s, &[]);
        }
        m.text("B");
        m.key(Keycode::Return, &mut s, &[]);
        assert_eq!(label(&m), ["TXT:A.PRG", "TXT:A.PRGB.PRG", "TXT:B", "DAT:D"]);

        // A right click on a file opens its menu; Delete asks first.
        let d = centre(m.file_row(3));
        m.right_click(d, &s);
        let Mode::Menu(popup) = &m.mode else {
            panic!("a menu")
        };
        assert_eq!(popup.items, FILE_BUTTONS);
        assert!(!m.enabled(Button::LoadTo), "a DAT file");
        let delete = m.popup_item(popup, 3);
        m.click(centre(delete), &mut s, &[]);
        assert!(matches!(m.mode, Mode::Ask(_, Pending::DeleteFile(_))));
        m.key(Keycode::Left, &mut s, &[]);
        m.key(Keycode::Return, &mut s, &[]);
        assert!(!s.exists_in("", FileKind::Dat, "D"));
        assert_eq!(m.file_selected, Some(2), "the last one left");
    }

    #[test]
    fn run_takes_the_selected_program() {
        let tmp = TempDir::new("run");
        let mut s = Storage::new(tmp.0.clone());
        s.write(FileKind::Txt, "A.PRG", b"?1", None).unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.key(Keycode::Tab, &mut s, &[]);
        assert_eq!(
            m.press(Button::Run, &mut s, &[]),
            Outcome::Open {
                switched: None,
                open: Open {
                    file: "A.PRG".to_string(),
                    slot: 0,
                    run: true
                }
            }
        );
    }
}

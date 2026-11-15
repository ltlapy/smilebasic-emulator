//! The project manager: lists the projects and their files, and runs,
//! switches, creates, renames, duplicates, deletes, imports and exports
//! projects. It loads a program into a slot, runs one, shows, copies,
//! renames and deletes files, and edits a project's title, description
//! and icon. Drawn over the screen like the menu bar, it takes all input
//! while it is open.
//!
//! The projects are on the left; on the right, an info area above the
//! files shows what the list clicked last has selected: the project, or
//! a preview of the file. Expanded, it covers the files and edits the
//! project's metadata.

use sb_runtime::Framebuffer;
use sb_runtime::confirm::{project_label, switch_lines};
use sb_runtime::env::{
    DESCRIPTION_CHARS, ICON_SIZE, Meta, SB4_META, TITLE_CHARS, meta_files, project_meta,
};
use sb_runtime::font::Font;
use sb_runtime::image;
use sb_runtime::runtime::SLOTS;
use sb_runtime::storage::{self, FileKind, Storage, check_project_name, suggest_project_name};
use sb_runtime::transfer;
use sdl3::gamepad::Button as Pad;
use sdl3::keyboard::Keycode;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::dialog::{Answer, Dialog};
use crate::file_dialogs::{self, FilePicker};
use crate::file_view::Viewer;
use crate::preview::{self, Content, Picture};
use crate::text_field::{Keyed, TextField};
use crate::ui::{
    BACKGROUND, BORDER, CHAR, DISABLED, GO, GO_HOVERED, HIGHLIGHT, LINE, Mark, PAD, Rect, TEXT,
    button, char_width, draw_text, fill, image, mark, mark_button, mark_button_width, outline,
    text_width, window,
};

const WIDTH: u32 = 1200;
/// On a 720-line display, it leaves the menu bar shown above it.
const HEIGHT: u32 = 672;
/// The width of the project list; the info area and the files take the
/// rest.
const LIST_WIDTH: u32 = 460;
/// The height of a project's row: its icon, beside its title over its
/// folder's name.
const ROW: u32 = 2 * LINE;
/// The folder's name under the title.
pub(crate) const FOLDER: u32 = 0xFFB0_B0B0;
/// The gap between a project's icon and its title.
const ICON_GAP: u32 = 6;
/// The current project's row when it is not selected: a faint tint,
/// darker and greyer than both selection colours (HIGHLIGHT, BORDER).
const CURRENT_TINT: u32 = 0xFF2A_3240;
/// The bar at the far left of the current project's row, left of its
/// icon, shown whether the row is selected or not.
const CURRENT_BAR: u32 = 0xFF8C_B8FF;
const CURRENT_BAR_WIDTH: u32 = 4;
/// How far the mouse moves with a file before it is being dragged.
const DRAG_START: i32 = 6;
pub(crate) const ERROR: u32 = 0xFFFF_8080;
/// The longest name the name box takes, a little over the limit so that
/// a name that is too long gets an error rather than being cut short.
const MAX_TYPED: usize = 40;
/// The info area's height, and its height expanded over the files.
const INFO_HEIGHT: u32 = 136;
const INFO_EXPANDED: u32 = 420;
/// The face of the info area and of the lists.
pub(crate) const PANEL: u32 = 0xFF12_141A;
/// Where text and pictures are shown inside the info area.
pub(crate) const WELL: u32 = 0xFF0B_0C10;
/// The project's icon in the info area: the 40-pixel one at twice its
/// size, each pixel a 2x2 square.
const BIG_ICON: u32 = 2 * ICON_SIZE as u32;
/// The gap between that icon and the text beside it.
const BIG_ICON_GAP: u32 = 12;
/// The width of the labels before the fields: "Description" and a gap.
const LABEL_WIDTH: u32 = 11 * CHAR + PAD;
/// The lines of the description's field.
const DESCRIPTION_ROWS: u32 = 9;
/// The width of a button showing only a mark: `⋯`, `▼` and `▲`.
const MARK_WIDTH: u32 = CHAR + 3 * PAD;
/// The View button's face, and its face under the mouse.
const VIEW: (u32, u32) = (0xFF1E_4E9A, HIGHLIGHT);
/// The files' text and outline while the info area covers them.
const DIM_TEXT: u32 = 0xFF50_5050;
const DIM_BORDER: u32 = 0xFF40_4040;
/// A menu's line between groups of items.
pub(crate) const SEPARATOR: u32 = 0xFF44_485A;
/// The program that Run runs.
const MAIN: &str = "MAIN.PRG";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Button {
    /// Run the selected project's MAIN.PRG, switching to it first.
    Run,
    Switch,
    /// Opens the menu of the rest that can be done with projects.
    ProjectMore,
    New,
    Rename,
    /// Copy the project under a free name, then offer to rename it.
    Duplicate,
    Import,
    Export,
    Delete,
    /// Run the selected program.
    RunFile,
    /// Show the selected file large.
    View,
    /// Load the selected program into a slot.
    LoadTo,
    /// Opens the menu of the rest that can be done with files.
    FileMore,
    CopyTo,
    RenameFile,
    DeleteFile,
    /// The window's `×`.
    Close,
    /// Expands or collapses the info area.
    Toggle,
    /// Write the edited title, description and icon.
    Save,
    /// Choose an image file for the icon.
    ChangeIcon,
    RemoveIcon,
}

/// The buttons above the projects, right of their heading.
const PROJECT_TOOLBAR: [Button; 3] = [Button::Run, Button::Switch, Button::ProjectMore];
/// The buttons above the files; Run selected gives its place to View
/// for a file that is not a program.
const FILE_TOOLBAR: [Button; 3] = [Button::RunFile, Button::LoadTo, Button::FileMore];
/// The menus of the `⋯` buttons, with `None` between groups.
const PROJECT_MORE: [Option<Button>; 8] = [
    Some(Button::New),
    Some(Button::Rename),
    Some(Button::Duplicate),
    None,
    Some(Button::Import),
    Some(Button::Export),
    None,
    Some(Button::Delete),
];
/// View is here as well, for the TXT files whose place in the toolbar
/// Run selected takes.
const FILE_MORE: [Option<Button>; 6] = [
    Some(Button::View),
    None,
    Some(Button::CopyTo),
    Some(Button::RenameFile),
    None,
    Some(Button::DeleteFile),
];
/// The menus of a right click on a project and on a file.
const PROJECT_MENU: [Option<Button>; 8] = [
    Some(Button::Run),
    Some(Button::Switch),
    None,
    Some(Button::Rename),
    Some(Button::Duplicate),
    Some(Button::Export),
    None,
    Some(Button::Delete),
];
const FILE_MENU: [Option<Button>; 8] = [
    Some(Button::RunFile),
    Some(Button::View),
    Some(Button::LoadTo),
    None,
    Some(Button::CopyTo),
    Some(Button::RenameFile),
    None,
    Some(Button::DeleteFile),
];

impl Button {
    fn label(self) -> &'static str {
        match self {
            Button::Run => "Run",
            Button::Switch => "Switch",
            Button::New => "New...",
            Button::Rename | Button::RenameFile => "Rename...",
            Button::Duplicate => "Duplicate",
            Button::Import => "Import...",
            Button::Export => "Export...",
            Button::Delete | Button::DeleteFile => "Delete...",
            Button::RunFile => "Run selected",
            Button::View => "View",
            Button::LoadTo => "Load to...",
            Button::CopyTo => "Copy to...",
            Button::Save => "Save",
            Button::ChangeIcon => "Change...",
            Button::RemoveIcon => "Remove",
            Button::ProjectMore | Button::FileMore | Button::Close | Button::Toggle => "",
        }
    }

    fn mark(self) -> Option<Mark> {
        match self {
            Button::Run | Button::RunFile => Some(Mark::Play),
            Button::ProjectMore | Button::FileMore => Some(Mark::More),
            Button::Close => Some(Mark::Close),
            _ => None,
        }
    }

    fn width(self) -> u32 {
        match self {
            Button::ProjectMore | Button::FileMore | Button::Toggle => MARK_WIDTH,
            Button::Close => LINE,
            // The two take the same place.
            Button::RunFile | Button::View => mark_button_width(Some(Mark::Play), "Run selected")
                .max(mark_button_width(None, "View")),
            b => mark_button_width(b.mark(), b.label()),
        }
    }

    fn faces(self) -> (u32, u32) {
        match self {
            Button::Run | Button::RunFile => (GO, GO_HOVERED),
            Button::View => VIEW,
            _ => (BORDER, HIGHLIGHT),
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

/// A file to copy, rename or delete, wherever it is.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Source {
    project: String,
    kind: FileKind,
    name: String,
}

/// A right click's menu, or the one of a `⋯` button.
struct Popup {
    at: (u32, u32),
    /// `None` between groups of items.
    items: Vec<Option<Button>>,
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
    /// Ask for a file, in the folder given if any, then call
    /// [`ProjectManager::folder_chosen`] with it.
    ChooseFile(Transfer, Option<PathBuf>),
    /// Load a TXT file of the current project into a slot, or run it;
    /// the manager closes. `switched` names the project switched to first.
    Open {
        switched: Option<String>,
        open: Open,
    },
}

/// Where the manager's focus and file selection were, to come back to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Memory {
    files: bool,
    file: Option<(FileKind, String)>,
}

/// A program to load from the current project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    pub file: String,
    pub slot: usize,
    /// Run it after loading (into PRG0).
    pub run: bool,
    /// Set by Run of MAIN.PRG: the manager comes back when the program
    /// ends.
    pub home: bool,
}

/// What a chosen folder or file is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transfer {
    /// Export the named project into it, in SmileBASIC 4's files.
    Export(String),
    /// Import SmileBASIC 4's project in it as a new project.
    Import,
    /// An image to make the named project's icon of.
    Icon(String),
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
    RenameFile(Source),
}

struct NameBox {
    naming: Naming,
    text: String,
    message: Option<String>,
}

/// An event put off to ask first whether to drop unsaved edits of the
/// metadata, done again if they are dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Deferred {
    Click((i32, i32)),
    /// With the project clicked before it, which the question's own
    /// click forgets.
    DoubleClick((i32, i32), Option<usize>),
    RightClick((i32, i32)),
    Key(Keycode),
    /// A controller's A on a list.
    Run,
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
    /// Drop the metadata's edits, then go on with the event.
    Discard(Deferred),
}

enum Mode {
    Browse,
    Name(NameBox),
    Ask(Dialog, Pending),
    /// Choosing the slot to load the selected file into; the one the
    /// keys or the controller are on.
    Slot(usize),
    Menu(Popup),
    /// Choosing where to copy a file to.
    Copy(Box<FilePicker>, Source),
    /// A file shown large.
    View(Box<Viewer>),
}

/// Which list Up and Down move in; Tab switches. The info area shows
/// what this list has selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Projects,
    Files,
}

/// A project's title, description and icon from its metadata, read when
/// the list is.
pub(crate) struct Listed {
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) icon: Option<Picture>,
}

impl Listed {
    pub(crate) fn read(storage: &Storage, project: &str) -> Listed {
        let meta = project_meta(project, |name| storage.read_file_in(project, name).ok());
        let pixels: Box<[u32]> = meta.icon.iter().map(|&p| p as u32).collect();
        let side = ICON_SIZE as u32;
        let icon =
            shown_icon(&pixels).then(|| Picture::scaled(&pixels, (side, side), (side, side)));
        Listed {
            title: String::from_utf16_lossy(&meta.title),
            description: String::from_utf16_lossy(&meta.description),
            icon,
        }
    }
}

/// Whether an icon has any pixel to show.
fn shown_icon<T: Copy + Into<u64>>(pixels: &[T]) -> bool {
    pixels.len() == ICON_SIZE * ICON_SIZE && pixels.iter().any(|&p| p.into() >> 24 & 0xFF != 0)
}

/// The icon as large as the info area shows it.
fn big_icon(pixels: &[u32]) -> Option<Picture> {
    let side = ICON_SIZE as u32;
    shown_icon(pixels).then(|| Picture::scaled(pixels, (side, side), (BIG_ICON, BIG_ICON)))
}

/// An image made an icon: fitted into its square, keeping its shape,
/// and centred on transparent pixels.
fn icon_from(img: &image::Image) -> Vec<i32> {
    let side = ICON_SIZE as u32;
    let (w, h) = (img.width as u32, img.height as u32);
    let p = Picture::fitted(&img.pixels, (w, h), (side, side));
    let (x0, y0) = ((side - p.w) / 2, (side - p.h) / 2);
    let mut icon = vec![0; ICON_SIZE * ICON_SIZE];
    for y in 0..p.h {
        for x in 0..p.w {
            icon[((y0 + y) * side + x0 + x) as usize] = p.pixels[(y * p.w + x) as usize] as i32;
        }
    }
    icon
}

/// The parts of the metadata's editor that the keys go to in turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Title,
    Description,
    Change,
    Remove,
    Save,
}

const FIELDS: [Field; 5] = [
    Field::Title,
    Field::Description,
    Field::Change,
    Field::Remove,
    Field::Save,
];

/// The selected project's title, description and icon being edited, in
/// the expanded info area.
struct MetaEdit {
    project: String,
    title: TextField,
    description: TextField,
    icon: Vec<i32>,
    picture: Option<Picture>,
    /// As read, or as last saved.
    saved: (String, String, Vec<i32>),
    /// Whether anything differs from `saved`, kept up as it is edited.
    dirty: bool,
    /// Where the keys go; `None` leaves them to the project list.
    focus: Option<Field>,
}

impl MetaEdit {
    fn changed(&mut self) {
        self.dirty = self.title.text() != self.saved.0
            || self.description.text() != self.saved.1
            || self.icon != self.saved.2;
    }

    fn set_icon(&mut self, icon: Vec<i32>) {
        let pixels: Vec<u32> = icon.iter().map(|&p| p as u32).collect();
        self.picture = big_icon(&pixels);
        self.icon = icon;
        self.changed();
    }
}

/// What the info area shows, read when the selection changes.
enum Info {
    Project {
        /// How many files, and their size.
        summary: String,
        icon: Option<Picture>,
    },
    File {
        label: String,
        /// The lines, the picture's size or the array's shape.
        about: String,
        /// The size.
        info: String,
        lines: Vec<String>,
        picture: Option<Picture>,
        failed: Option<String>,
    },
}

/// What [`Info`] was read for.
#[derive(Clone, Debug, PartialEq, Eq)]
struct InfoKey {
    project: String,
    file: Option<(FileKind, String)>,
    expanded: bool,
}

pub struct ProjectManager {
    /// Display size the window is centered in.
    size: (u32, u32),
    /// The workspace (`""`) first, then the folders as named on disk.
    projects: Vec<String>,
    /// The projects' titles and icons, in the same order.
    listed: Vec<Listed>,
    /// The project that the last click hit, for a double click to count
    /// only on the same one.
    clicked: Option<usize>,
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
    /// Whether the info area covers the files.
    expanded: bool,
    info: Info,
    info_key: Option<InfoKey>,
    edit: Option<MetaEdit>,
    status: Option<(String, bool)>,
    mode: Mode,
    mouse: Option<(i32, i32)>,
    drag: Option<Drag>,
    /// When the storage folder and the selected project's folder last
    /// changed, as the lists were read.
    read_at: (Option<SystemTime>, Option<SystemTime>),
    /// The lists wait to be read again until the window goes back to
    /// browsing.
    stale: bool,
}

/// Whether a folder's name can be used as a project name as it is.
pub(crate) fn usable(name: &str) -> bool {
    name.is_empty() || check_project_name(name).is_ok_and(|n| n == name)
}

/// `text` cut to `width` pixels, with `~` where it was cut.
pub(crate) fn fit(text: &str, width: u32) -> String {
    if text_width(text) <= width {
        return text.to_string();
    }
    let room = width.saturating_sub(text_width("~"));
    let mut used = 0;
    let mut cut: String = (text.chars())
        .take_while(|&c| {
            used += char_width(c);
            used <= room
        })
        .collect();
    cut.push('~');
    cut
}

fn item_height(item: Option<Button>) -> u32 {
    if item.is_some() { LINE } else { PAD }
}

/// A menu's place, kept inside the window's `frame`.
fn popup_rect(frame: Rect, popup: &Popup) -> Rect {
    let w = (popup.items.iter().flatten())
        .map(|b| text_width(b.label()))
        .max()
        .unwrap_or(0)
        + 4 * PAD;
    let h: u32 = popup.items.iter().map(|&i| item_height(i)).sum();
    Rect {
        x: popup.at.0.min(frame.x + frame.w - w - 2),
        y: popup.at.1.min(frame.y + frame.h - h - 2),
        w,
        h,
    }
}

/// A menu item's place, or a line's between groups.
fn popup_item(frame: Rect, popup: &Popup, i: usize) -> Rect {
    let all = popup_rect(frame, popup);
    let above: u32 = popup.items[..i].iter().map(|&i| item_height(i)).sum();
    Rect {
        y: all.y + above,
        h: item_height(popup.items[i]),
        ..all
    }
}

fn popup_hit(frame: Rect, popup: &Popup, at: (i32, i32)) -> Option<usize> {
    (0..popup.items.len())
        .find(|&i| popup.items[i].is_some() && popup_item(frame, popup, i).contains(at))
}

impl ProjectManager {
    pub fn new(storage: &Storage, size: (u32, u32)) -> ProjectManager {
        let mut manager = ProjectManager {
            size,
            projects: Vec::new(),
            listed: Vec::new(),
            clicked: None,
            current: String::new(),
            selected: 0,
            scroll: 0,
            files: Vec::new(),
            file_selected: None,
            file_scroll: 0,
            focus: Focus::Projects,
            expanded: false,
            info: Info::Project {
                summary: String::new(),
                icon: None,
            },
            info_key: None,
            edit: None,
            status: None,
            mode: Mode::Browse,
            mouse: None,
            drag: None,
            read_at: (None, None),
            stale: false,
        };
        manager.refresh(storage, None);
        let current = manager.current.clone();
        manager.select_name(&current, storage);
        manager.sync(storage);
        manager
    }

    /// What [`ProjectManager::reopen`] needs to come back as it was left.
    pub fn memory(&self) -> Memory {
        Memory {
            files: self.focus == Focus::Files,
            file: (self.file_selected.and_then(|i| self.files.get(i)))
                .map(|f| (f.kind, f.name.clone())),
        }
    }

    /// A manager on the current project, with the focus and the file that
    /// `memory` says.
    pub fn reopen(storage: &Storage, size: (u32, u32), memory: &Memory) -> ProjectManager {
        let mut manager = ProjectManager::new(storage, size);
        if let Some((kind, name)) = &memory.file
            && let Some(i) = (manager.files.iter()).position(|f| f.kind == *kind && f.name == *name)
        {
            manager.file_selected = Some(i);
        }
        if memory.files && !manager.files.is_empty() {
            manager.focus = Focus::Files;
        }
        manager.sync(storage);
        manager
    }

    /// Reads the projects again, selecting `select` if given.
    fn refresh(&mut self, storage: &Storage, select: Option<&str>) {
        self.current = storage.project().to_string();
        self.read_at.0 = storage.changed_at(None);
        self.projects = vec![String::new()];
        match storage.projects() {
            Ok(names) => self.projects.extend(names),
            Err(e) => self.status = Some((format!("Could not list the projects: {e}"), true)),
        }
        self.listed = (self.projects.iter())
            .map(|p| Listed::read(storage, p))
            .collect();
        self.clicked = None;
        match select {
            Some(name) => self.select_name(name, storage),
            None => self.select(self.selected, storage),
        }
    }

    /// Reads the lists again for projects and files that other programs
    /// added, removed or renamed, keeping what is selected. Unless
    /// `always`, only when the folders' times say something changed in
    /// them. A question, a menu or a name box waits: it acts on what was
    /// selected when it opened.
    pub fn reread(&mut self, storage: &Storage, always: bool) {
        self.stale |= always;
        if let Mode::Copy(picker, _) = &mut self.mode {
            picker.reread(storage, always);
        }
        if !matches!(self.mode, Mode::Browse | Mode::Copy(..)) || self.drag.is_some() {
            return;
        }
        let now = (
            storage.changed_at(None),
            storage.changed_at(Some(self.selected_name())),
        );
        if !self.stale && now == self.read_at {
            return;
        }
        self.stale = false;
        let (selected, project) = (self.selected, self.selected_name().to_string());
        let file =
            (self.file_selected.and_then(|i| self.files.get(i))).map(|f| (f.kind, f.name.clone()));
        let (kept, focus, file_scroll) = (self.file_selected, self.focus, self.file_scroll);
        let clicked = self.clicked;
        self.refresh(storage, Some(&project));
        // A project gone leaves the one that took its place selected.
        if self.selected_name() != project {
            self.select(selected, storage);
            self.sync(storage);
            return;
        }
        self.clicked = clicked;
        let found = file.and_then(|(kind, name)| {
            (self.files.iter()).position(|f| f.kind == kind && f.name == name)
        });
        if let Some(i) = found.or(kept) {
            self.file_scroll = file_scroll;
            self.select_file(i);
            if !self.files.is_empty() {
                self.focus = focus;
            }
        }
        self.sync(storage);
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
        // The files may have changed: read the info again.
        self.info_key = None;
        let project = self.selected_name().to_string();
        self.read_at.1 = storage.changed_at(Some(&project));
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
        if self.files.is_empty() {
            self.focus = Focus::Projects;
        }
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

    /// Whether the info area shows the selected file rather than the
    /// project.
    fn shows_file(&self) -> bool {
        self.focus == Focus::Files && self.file_selected.is_some()
    }

    /// Run selected for a program or when no file is selected, which
    /// leaves it disabled; View for the other files.
    fn file_action(&self) -> Button {
        match self.file_selected.and_then(|i| self.files.get(i)) {
            Some(f) if f.kind != FileKind::Txt => Button::View,
            _ => Button::RunFile,
        }
    }

    fn dirty(&self) -> bool {
        self.edit.as_ref().is_some_and(|e| e.dirty)
    }

    fn enabled(&self, button: Button) -> bool {
        let name = self.selected_name();
        match button {
            Button::Run => (self.files.iter()).any(|f| f.kind == FileKind::Txt && f.name == MAIN),
            Button::Switch => name != self.current,
            Button::Rename | Button::Delete => !name.is_empty(),
            Button::RunFile | Button::LoadTo => self.selected_program().is_some(),
            Button::View | Button::CopyTo | Button::RenameFile | Button::DeleteFile => {
                self.selected_file().is_some()
            }
            Button::Save => self.dirty(),
            Button::ChangeIcon => self.edit.is_some(),
            Button::RemoveIcon => self.edit.as_ref().is_some_and(|e| e.picture.is_some()),
            Button::New
            | Button::Duplicate
            | Button::ProjectMore
            | Button::FileMore
            | Button::Import
            | Button::Export
            | Button::Close
            | Button::Toggle => true,
        }
    }

    /// Whether the expanded info area covers the button.
    fn blocked(&self, button: Button) -> bool {
        self.expanded && (FILE_TOOLBAR.contains(&button) || button == Button::View)
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

    /// The top of the toolbars and of the info area, under the title.
    fn top(&self) -> u32 {
        self.frame().y + LINE + 2 * PAD
    }

    fn bottom(&self) -> u32 {
        let f = self.frame();
        f.y + f.h - 2 * PAD
    }

    /// The project list, under its toolbar.
    fn list(&self) -> Rect {
        let f = self.frame();
        let y = self.top() + LINE + PAD;
        Rect {
            x: f.x + 2 * PAD,
            y,
            w: LIST_WIDTH,
            h: self.bottom() - y,
        }
    }

    /// The column right of the projects: the info area, then the files.
    fn right(&self) -> Rect {
        let f = self.frame();
        let list = self.list();
        let x = list.x + list.w + 2 * PAD;
        let top = self.top();
        Rect {
            x,
            y: top,
            w: f.x + f.w - 2 * PAD - x,
            h: self.bottom() - top,
        }
    }

    fn info_rect(&self) -> Rect {
        let h = if self.expanded {
            INFO_EXPANDED
        } else {
            INFO_HEIGHT
        };
        Rect { h, ..self.right() }
    }

    /// The files' toolbar, under the collapsed info area.
    fn files_toolbar_y(&self) -> u32 {
        self.top() + INFO_HEIGHT + PAD
    }

    fn files_rect(&self) -> Rect {
        let y = self.files_toolbar_y() + LINE + PAD;
        Rect {
            y,
            h: self.bottom() - y,
            ..self.right()
        }
    }

    fn rows(&self) -> usize {
        (self.list().h / ROW) as usize
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
            y: list.y + (i - self.scroll) as u32 * ROW,
            h: ROW,
            ..list
        }
    }

    /// Where a project row's title and folder name start: just right of
    /// its icon.
    fn title_x(row: Rect) -> u32 {
        row.x + PAD + ICON_SIZE as u32 + ICON_GAP
    }

    /// The current project's marker, at the row's far left inside the
    /// list's two-pixel outline, left of the icon.
    fn current_bar(row: Rect) -> Rect {
        Rect {
            x: row.x + 2,
            y: row.y + 2,
            w: CURRENT_BAR_WIDTH,
            h: ROW - 4,
        }
    }

    /// `button`'s place in a row of them that ends at `right`.
    fn in_row(row: &[Button], button: Button, right: u32, y: u32) -> Rect {
        let all: u32 = row.iter().map(|b| b.width() + PAD).sum::<u32>() - PAD;
        let before: u32 = (row.iter())
            .take_while(|&&b| b != button)
            .map(|b| b.width() + PAD)
            .sum();
        Rect {
            x: right - all + before,
            y,
            w: button.width(),
            h: LINE,
        }
    }

    /// The big icon's place in the info area.
    fn big_icon_rect(&self) -> Rect {
        let r = self.info_rect();
        Rect {
            x: r.x + 2 + PAD,
            y: r.y + 2 + PAD,
            w: BIG_ICON,
            h: BIG_ICON,
        }
    }

    /// The fields of the expanded info area: the title, the description,
    /// and the row of the icon's buttons.
    fn field_rect(&self, field: Field) -> Rect {
        let r = self.info_rect();
        let x = r.x + 2 + PAD + LABEL_WIDTH;
        let w = r.x + r.w - 2 - PAD - x;
        let title_y = self.big_icon_rect().y + BIG_ICON + PAD;
        let description_y = title_y + LINE + PAD;
        let icon_y = description_y + DESCRIPTION_ROWS * LINE + PAD;
        let row = |y, h| Rect { x, y, w, h };
        match field {
            Field::Title => row(title_y, LINE),
            Field::Description => row(description_y, DESCRIPTION_ROWS * LINE),
            Field::Change | Field::Remove | Field::Save => row(icon_y, LINE),
        }
    }

    /// Where the preview of a file goes in the info area, under its name.
    fn preview_rect(&self) -> Rect {
        let r = self.info_rect();
        let x = r.x + 2 + PAD;
        let y = r.y + 2 + PAD + LINE + PAD / 2;
        Rect {
            x,
            y,
            w: r.x + r.w - 2 - PAD - MARK_WIDTH - PAD - x,
            h: r.y + r.h - 2 - PAD - y,
        }
    }

    fn button(&self, button: Button) -> Rect {
        let f = self.frame();
        let info = self.info_rect();
        let info_right = info.x + info.w - 2 - PAD;
        let list = self.list();
        let right = self.right();
        match button {
            Button::Close => Rect {
                x: f.x + f.w - 2 - LINE,
                y: f.y + 2,
                w: LINE,
                h: LINE,
            },
            Button::Toggle => Rect {
                x: info_right - MARK_WIDTH,
                y: info.y + info.h - 2 - PAD - LINE,
                w: MARK_WIDTH,
                h: LINE,
            },
            Button::Save => Rect {
                x: info_right - button.width(),
                y: info.y + 2 + PAD,
                w: button.width(),
                h: LINE,
            },
            Button::ChangeIcon | Button::RemoveIcon => {
                let r = self.field_rect(Field::Change);
                let change = Button::ChangeIcon.width();
                Rect {
                    x: if button == Button::ChangeIcon {
                        r.x
                    } else {
                        r.x + change + PAD
                    },
                    w: button.width(),
                    ..r
                }
            }
            b if PROJECT_TOOLBAR.contains(&b) => {
                Self::in_row(&PROJECT_TOOLBAR, b, list.x + list.w, self.top())
            }
            Button::View => self.button(Button::RunFile),
            b if FILE_TOOLBAR.contains(&b) => {
                Self::in_row(&FILE_TOOLBAR, b, right.x + right.w, self.files_toolbar_y())
            }
            // In menus only.
            _ => Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            },
        }
    }

    /// The buttons shown, which the mouse can press.
    fn buttons(&self) -> impl Iterator<Item = Button> + '_ {
        let edit = [Button::Save, Button::ChangeIcon, Button::RemoveIcon];
        PROJECT_TOOLBAR
            .into_iter()
            .chain([self.file_action(), Button::LoadTo, Button::FileMore])
            .chain([Button::Close, Button::Toggle])
            .chain(edit.into_iter().filter(|_| self.edit.is_some()))
    }

    fn project_at(&self, at: (i32, i32)) -> Option<usize> {
        let shown = self.scroll..self.projects.len().min(self.scroll + self.rows());
        shown.into_iter().find(|&i| self.row(i).contains(at))
    }

    fn file_at(&self, at: (i32, i32)) -> Option<usize> {
        if self.expanded {
            return None;
        }
        self.shown_files().find(|&i| self.file_row(i).contains(at))
    }

    /// The part of the window the expanded info area leaves dimmed, which
    /// takes no input.
    fn covered(&self, at: (i32, i32)) -> bool {
        self.expanded && self.right().contains(at) && !self.info_rect().contains(at)
    }

    /// The item `step` on from `from` that can be chosen, round the end.
    fn next_item(&self, popup: &Popup, from: Option<usize>, step: isize) -> Option<usize> {
        let n = popup.items.len() as isize;
        let start = from.map_or(if step > 0 { -1 } else { n }, |i| i as isize);
        (1..=n)
            .map(|k| (start + k * step).rem_euclid(n) as usize)
            .find(|&i| popup.items[i].is_some_and(|b| self.enabled(b)))
    }

    fn open_menu(&mut self, at: (u32, u32), items: &[Option<Button>], keyed: bool) {
        let mut popup = Popup {
            at,
            items: items.to_vec(),
            chosen: None,
        };
        if keyed {
            popup.chosen = self.next_item(&popup, None, 1);
        }
        self.mode = Mode::Menu(popup);
    }

    /// The menu of a `⋯` button, under it.
    fn open_more(&mut self, more: Button, keyed: bool) {
        let r = self.button(more);
        let items: &[Option<Button>] = match more {
            Button::ProjectMore => &PROJECT_MORE,
            _ => &FILE_MORE,
        };
        self.open_menu((r.x, r.y + r.h), items, keyed);
    }

    /// Reads the info area's contents again if the selection changed,
    /// and starts or ends editing the metadata to match. Every event
    /// that can change what is selected ends with this.
    fn sync(&mut self, storage: &Storage) {
        let key = InfoKey {
            project: self.selected_name().to_string(),
            file: (self.shows_file())
                .then(|| self.selected_file().map(|s| (s.kind, s.name)))
                .flatten(),
            expanded: self.expanded,
        };
        if self.info_key.as_ref() != Some(&key) {
            self.info = self.read_info(storage, &key);
            self.info_key = Some(key);
        }
        let editing = self.expanded && !self.shows_file();
        let same = self
            .edit
            .as_ref()
            .is_some_and(|e| e.project == self.selected_name());
        match (editing, same) {
            (true, true) => {}
            (true, false) => {
                // Moving to another project keeps the keys where they were;
                // expanding puts them in the title.
                let focus = self.edit.as_ref().map_or(Some(Field::Title), |e| e.focus);
                self.edit = Some(self.new_edit(focus));
            }
            (false, _) => self.edit = None,
        }
    }

    fn new_edit(&self, focus: Option<Field>) -> MetaEdit {
        let listed = &self.listed[self.selected];
        let icon: Vec<i32> = match &listed.icon {
            Some(p) => p.pixels.iter().map(|&p| p as i32).collect(),
            None => vec![0; ICON_SIZE * ICON_SIZE],
        };
        let title = TextField::new(
            &listed.title,
            TITLE_CHARS,
            false,
            self.field_rect(Field::Title),
        );
        let description = TextField::new(
            &listed.description,
            DESCRIPTION_CHARS,
            true,
            self.field_rect(Field::Description),
        );
        let pixels: Vec<u32> = icon.iter().map(|&p| p as u32).collect();
        MetaEdit {
            project: self.selected_name().to_string(),
            saved: (title.text(), description.text(), icon.clone()),
            title,
            description,
            picture: big_icon(&pixels),
            icon,
            dirty: false,
            focus,
        }
    }

    fn read_info(&self, storage: &Storage, key: &InfoKey) -> Info {
        let Some((kind, name)) = &key.file else {
            let bytes = self
                .files
                .iter()
                .filter_map(|f| f.info.as_ref())
                .map(|i| i.0)
                .sum();
            let count = match self.files.len() {
                1 => "1 file".to_string(),
                n => format!("{n} files"),
            };
            let icon = (self.listed[self.selected].icon.as_ref()).and_then(|p| big_icon(&p.pixels));
            return Info::Project {
                summary: format!("{count}, {}", preview::size_text(bytes)),
                icon,
            };
        };
        let file = self.files[self.file_selected.expect("a file is shown")].clone();
        // The list beside it shows the date.
        let info = (file.info.as_ref()).map_or(String::new(), |i| preview::size_text(i.0));
        let room = self.preview_rect();
        let rows = (room.h.saturating_sub(4) / LINE) as usize;
        let cols = (room.w.saturating_sub(2 * PAD) / CHAR) as usize;
        let (mut about, mut lines, mut picture, mut failed) = (String::new(), vec![], None, None);
        match preview::read(storage, &key.project, *kind, name) {
            Content::Text { lines: text, count } => {
                about = match count {
                    1 => "1 line".to_string(),
                    n => format!("{n} lines"),
                };
                lines = text.into_iter().take(rows).collect();
            }
            Content::Image { w, h, pixels } => {
                about = format!("{w}x{h}");
                let room = (room.w.saturating_sub(4), room.h.saturating_sub(4));
                picture = Some(Picture::fitted(&pixels, (w, h), room));
            }
            Content::Data(data) => {
                about = format!(
                    "{} {}",
                    preview::element_type(&data),
                    preview::dims_text(&data)
                );
                lines = preview::data_lines(&data, cols, rows);
            }
            Content::Failed(e) => failed = Some(e),
        }
        Info::File {
            label: file.label(),
            about,
            info,
            lines,
            picture,
            failed,
        }
    }

    pub fn mouse_move(&mut self, at: (i32, i32)) {
        self.mouse = Some(at);
        let frame = self.frame();
        match &mut self.mode {
            Mode::Ask(dialog, _) => dialog.mouse_move(at),
            Mode::Copy(picker, _) => picker.mouse_move(at),
            Mode::View(viewer) => viewer.mouse_move(at),
            Mode::Menu(popup) => popup.chosen = popup_hit(frame, popup, at),
            _ => {}
        }
        if let Some(drag) = &mut self.drag {
            let (dx, dy) = (at.0 - drag.from.0, at.1 - drag.from.1);
            drag.moving |= dx.abs().max(dy.abs()) >= DRAG_START;
        }
    }

    /// Scrolls the list under the mouse by `lines`, down for positive.
    pub fn wheel(&mut self, lines: i32) {
        match &mut self.mode {
            Mode::Copy(picker, _) => return picker.wheel(lines),
            Mode::View(viewer) => return viewer.wheel(lines),
            Mode::Browse => {}
            _ => return,
        }
        let Some(at) = self.mouse else {
            return;
        };
        if self.covered(at) || self.info_rect().contains(at) {
            return;
        }
        if self.files_rect().contains(at) {
            let last = self.files.len().saturating_sub(self.file_rows());
            self.file_scroll = (self.file_scroll.saturating_add_signed(lines as isize)).min(last);
            return;
        }
        let last = self.projects.len().saturating_sub(self.rows());
        self.scroll = self.scroll.saturating_add_signed(lines as isize).min(last);
    }

    /// Typed text, for the name box and the metadata's fields.
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
            Mode::Browse => {
                if let Some(edit) = &mut self.edit {
                    match edit.focus {
                        Some(Field::Title) => edit.title.insert(text),
                        Some(Field::Description) => edit.description.insert(text),
                        _ => return,
                    }
                    edit.changed();
                }
            }
            _ => {}
        }
    }

    /// Asks before `deferred` drops the metadata's unsaved edits.
    fn ask_discard(&mut self, deferred: Deferred) -> Outcome {
        let project = self.edit.as_ref().map_or("", |e| e.project.as_str());
        let lines = vec![
            format!(
                "The title, description or icon of {}",
                project_label(project)
            ),
            "were changed and not saved.".to_string(),
            String::new(),
            "Discard the changes?".to_string(),
        ];
        let dialog = Dialog::new("Unsaved changes", lines, "Discard", self.size);
        self.mode = Mode::Ask(dialog, Pending::Discard(deferred));
        Outcome::Stay
    }

    /// `unsaved` lists the slots with unsaved edits, for the warning
    /// before switching.
    pub fn click(&mut self, at: (i32, i32), storage: &mut Storage, unsaved: &[usize]) -> Outcome {
        let outcome = self.click_in(at, storage, unsaved);
        self.sync(storage);
        outcome
    }

    fn click_in(&mut self, at: (i32, i32), storage: &mut Storage, unsaved: &[usize]) -> Outcome {
        self.clicked = None;
        let frame = self.frame();
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
            Mode::Slot(_) => {
                let chosen = (0..=SLOTS).find(|&i| self.slot_button(i).contains(at));
                match chosen {
                    Some(SLOTS) => self.mode = Mode::Browse,
                    Some(slot) => return self.load_to(slot, storage, unsaved),
                    None => {}
                }
                Outcome::Stay
            }
            Mode::Menu(popup) => {
                let hit = popup_hit(frame, popup, at).and_then(|i| popup.items[i]);
                self.mode = Mode::Browse;
                match hit {
                    Some(b) => self.press(b, storage, unsaved),
                    None => Outcome::Stay,
                }
            }
            Mode::Copy(picker, _) => {
                let outcome = picker.click(at, storage);
                self.picked(outcome, storage)
            }
            Mode::View(viewer) => {
                if viewer.click(at) {
                    self.mode = Mode::Browse;
                }
                Outcome::Stay
            }
            Mode::Browse => self.browse_click(at, storage, unsaved),
        }
    }

    fn browse_click(
        &mut self,
        at: (i32, i32),
        storage: &mut Storage,
        unsaved: &[usize],
    ) -> Outcome {
        let hit = self
            .buttons()
            .find(|&b| !self.blocked(b) && self.button(b).contains(at));
        if let Some(b) = hit {
            let keeps_edit = matches!(b, Button::Save | Button::ChangeIcon | Button::RemoveIcon);
            if self.dirty() && !keeps_edit {
                return self.ask_discard(Deferred::Click(at));
            }
            return self.press(b, storage, unsaved);
        }
        if self.edit.is_some() && self.info_rect().contains(at) {
            let title = self.field_rect(Field::Title).contains(at);
            let description = self.field_rect(Field::Description).contains(at);
            if let Some(edit) = &mut self.edit {
                if title {
                    edit.focus = Some(Field::Title);
                } else if description {
                    edit.focus = Some(Field::Description);
                }
            }
            return Outcome::Stay;
        }
        if self.covered(at) {
            return Outcome::Stay;
        }
        if let Some(i) = self.project_at(at) {
            if self.dirty() && i != self.selected {
                return self.ask_discard(Deferred::Click(at));
            }
            if i != self.selected || self.focus != Focus::Projects {
                self.select(i, storage);
            }
            self.focus = Focus::Projects;
            self.clicked = Some(i);
            // The keys go to the list clicked.
            if let Some(edit) = &mut self.edit {
                edit.focus = None;
            }
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

    /// A double click on a project runs it, or switches to it if it has
    /// no MAIN.PRG; on the current project with none, it closes. It
    /// counts only if the click before it hit the same project; one that
    /// went to another project is a click on this one.
    pub fn double_click(
        &mut self,
        at: (i32, i32),
        storage: &mut Storage,
        unsaved: &[usize],
    ) -> Outcome {
        let outcome = self.double_click_in(at, storage, unsaved);
        self.sync(storage);
        outcome
    }

    fn double_click_in(
        &mut self,
        at: (i32, i32),
        storage: &mut Storage,
        unsaved: &[usize],
    ) -> Outcome {
        if !matches!(self.mode, Mode::Browse) {
            return Outcome::Stay;
        }
        let hit = self.project_at(at);
        if hit.is_some() && hit != self.clicked {
            return self.click_in(at, storage, unsaved);
        }
        if hit.is_some() && self.dirty() {
            return self.ask_discard(Deferred::DoubleClick(at, self.clicked));
        }
        // The next click starts over.
        self.clicked = None;
        match hit {
            Some(_) if self.enabled(Button::Run) => self.press(Button::Run, storage, unsaved),
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
            if self.dirty() && i != self.selected {
                self.ask_discard(Deferred::RightClick(at));
                return;
            }
            if i != self.selected {
                self.select(i, storage);
            }
            self.focus = Focus::Projects;
            if let Some(edit) = &mut self.edit {
                edit.focus = None;
            }
            self.open_menu(place, &PROJECT_MENU, false);
        } else if let Some(i) = self.file_at(at) {
            self.select_file(i);
            self.focus = Focus::Files;
            self.open_menu(place, &FILE_MENU, false);
        }
        self.sync(storage);
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
        self.sync(storage);
    }

    /// A controller, mostly as keys: the pad moves and B (the bottom face
    /// button) is Esc, as SmileBASIC 4's buttons are placed. A (the right
    /// one) runs: the project's MAIN.PRG on the project list, the file (or
    /// views it) on the files, as the toolbars' green buttons do; in the
    /// menus, dialogs, viewer and the info area's fields it is Enter. X
    /// goes between the lists (Tab), Y expands or collapses the info area,
    /// and L and R move up and down a page, one step a press. The list's
    /// `⋯` menu opens with the mouse or keys only; L, R or B closes it.
    pub fn pad(&mut self, button: Pad, storage: &mut Storage, unsaved: &[usize]) -> Outcome {
        let browsing = matches!(self.mode, Mode::Browse);
        if browsing && button == Pad::East && !self.in_field() {
            let outcome = self.pad_run(storage, unsaved);
            self.sync(storage);
            return outcome;
        }
        let outcome = match &mut self.mode {
            Mode::Ask(dialog, _) => match dialog.pad(button) {
                Some(answer) => self.answer(answer, storage, unsaved),
                None => Outcome::Stay,
            },
            Mode::View(viewer) => {
                if viewer.pad(button) {
                    self.mode = Mode::Browse;
                }
                Outcome::Stay
            }
            _ => {
                let key = match button {
                    Pad::DPadUp => Keycode::Up,
                    Pad::DPadDown => Keycode::Down,
                    Pad::DPadLeft => Keycode::Left,
                    Pad::DPadRight => Keycode::Right,
                    Pad::East => Keycode::Return,
                    Pad::South => Keycode::Escape,
                    Pad::North if browsing => Keycode::Tab,
                    Pad::West => Keycode::Space,
                    Pad::LeftShoulder if browsing => Keycode::PageUp,
                    Pad::RightShoulder if browsing => Keycode::PageDown,
                    Pad::LeftShoulder | Pad::RightShoulder
                        if matches!(self.mode, Mode::Menu(_)) =>
                    {
                        Keycode::Escape
                    }
                    Pad::LeftShoulder | Pad::RightShoulder => Keycode::Tab,
                    _ => return Outcome::Stay,
                };
                return self.key(key, storage, unsaved);
            }
        };
        self.sync(storage);
        outcome
    }

    /// Which kind of screen the manager is on (list, name box, dialog,
    /// ...), so that a change can be told.
    pub fn mode_id(&self) -> usize {
        match self.mode {
            Mode::Browse => 0,
            Mode::Name(_) => 1,
            Mode::Ask(..) => 2,
            Mode::Slot(_) => 3,
            Mode::Menu(_) => 4,
            Mode::Copy(..) => 5,
            Mode::View(_) => 6,
        }
    }

    /// Whether one of the info area's fields has the keys.
    fn in_field(&self) -> bool {
        self.edit.as_ref().is_some_and(|e| e.focus.is_some())
    }

    /// A on a list: Run on the projects, Run selected or View on the
    /// files. Nothing while the expanded info area covers the files.
    fn pad_run(&mut self, storage: &mut Storage, unsaved: &[usize]) -> Outcome {
        if self.dirty() {
            return self.ask_discard(Deferred::Run);
        }
        match self.focus {
            Focus::Projects => self.press(Button::Run, storage, unsaved),
            Focus::Files if self.expanded => Outcome::Stay,
            Focus::Files => self.press(self.file_action(), storage, unsaved),
        }
    }

    /// Up and Down choose a project, or a file after Tab. Enter switches to
    /// the project, or loads the file into a slot. F5 runs the selected
    /// project's MAIN.PRG on the project list, as A does, and the selected
    /// program on the files, as Run selected does. Space expands the info area, and the menu key opens the list's `⋯`
    /// menu. Esc collapses the info area or closes; F12 closes.
    pub fn key(&mut self, key: Keycode, storage: &mut Storage, unsaved: &[usize]) -> Outcome {
        let outcome = self.key_in(key, storage, unsaved);
        self.sync(storage);
        outcome
    }

    fn key_in(&mut self, key: Keycode, storage: &mut Storage, unsaved: &[usize]) -> Outcome {
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
                let (chosen, items) = (popup.chosen, popup.items.clone());
                let shape = Popup {
                    at: popup.at,
                    items,
                    chosen,
                };
                let moved = match key {
                    Keycode::Up => Some(self.next_item(&shape, chosen, -1)),
                    Keycode::Down => Some(self.next_item(&shape, chosen, 1)),
                    _ => None,
                };
                match key {
                    Keycode::Escape => self.mode = Mode::Browse,
                    Keycode::Return | Keycode::KpEnter | Keycode::Space => {
                        if let Some(b) = chosen.and_then(|i| shape.items[i]) {
                            self.mode = Mode::Browse;
                            return self.press(b, storage, unsaved);
                        }
                    }
                    _ => {
                        if let (Some(to), Mode::Menu(popup)) = (moved, &mut self.mode) {
                            popup.chosen = to.or(popup.chosen);
                        }
                    }
                }
                Outcome::Stay
            }
            Mode::Copy(picker, _) => {
                let outcome = picker.key(key, storage);
                self.picked(outcome, storage)
            }
            Mode::Slot(chosen) => {
                let digit = (key as i32).checked_sub(Keycode::_0 as i32);
                match (key, digit) {
                    (_, Some(d @ 0..)) if (d as usize) < SLOTS => {
                        return self.load_to(d as usize, storage, unsaved);
                    }
                    (Keycode::Left, _) => *chosen = (*chosen + SLOTS) % (SLOTS + 1),
                    (Keycode::Right | Keycode::Tab, _) => *chosen = (*chosen + 1) % (SLOTS + 1),
                    (Keycode::Return | Keycode::KpEnter | Keycode::Space, _) => match *chosen {
                        SLOTS => self.mode = Mode::Browse,
                        slot => return self.load_to(slot, storage, unsaved),
                    },
                    (Keycode::Escape, _) => self.mode = Mode::Browse,
                    _ => {}
                }
                Outcome::Stay
            }
            Mode::View(viewer) => {
                if viewer.key(key) {
                    self.mode = Mode::Browse;
                }
                Outcome::Stay
            }
            Mode::Browse => self.browse_key(key, storage, unsaved),
        }
    }

    /// A key for the metadata's editor, or `None` when it is not for it.
    fn edit_key(&mut self, key: Keycode, storage: &mut Storage) -> Option<Outcome> {
        let edit = self.edit.as_mut()?;
        let field = edit.focus?;
        let used = match field {
            Field::Title => edit.title.key(key),
            Field::Description => edit.description.key(key),
            _ => Keyed::Passed,
        };
        edit.changed();
        if used == Keyed::Used {
            return Some(Outcome::Stay);
        }
        let at = FIELDS.iter().position(|&f| f == field).expect("a field");
        let next = FIELDS.get(at + 1).copied();
        let press = |button| match field {
            Field::Change => Some(Button::ChangeIcon),
            Field::Remove => Some(Button::RemoveIcon),
            Field::Save => Some(Button::Save),
            _ => button,
        };
        match key {
            // From the last back to the project list.
            Keycode::Tab => edit.focus = next,
            Keycode::Down if field == Field::Title => edit.focus = next,
            Keycode::Up if field == Field::Description => edit.focus = Some(Field::Title),
            Keycode::Up if at >= 2 => edit.focus = Some(Field::Description),
            Keycode::Left if at > 2 => edit.focus = Some(FIELDS[at - 1]),
            Keycode::Right if (2..4).contains(&at) => edit.focus = next,
            Keycode::Return | Keycode::KpEnter | Keycode::Space => match press(None) {
                Some(b) => return Some(self.press(b, storage, &[])),
                None if key != Keycode::Space => edit.focus = next,
                None => {}
            },
            // Collapsing and closing; the rest stays with the editor.
            Keycode::Escape | Keycode::F12 => return None,
            _ => {}
        }
        Some(Outcome::Stay)
    }

    fn browse_key(&mut self, key: Keycode, storage: &mut Storage, unsaved: &[usize]) -> Outcome {
        // Ahead of the info area's fields, which would take it.
        if key == Keycode::F5 {
            if self.dirty() {
                return self.ask_discard(Deferred::Key(key));
            }
            // Never View, as A would on a DAT or GRP file: F5 only runs.
            let run = match self.focus {
                Focus::Projects => Button::Run,
                Focus::Files => Button::RunFile,
            };
            return self.press(run, storage, unsaved);
        }
        if let Some(outcome) = self.edit_key(key, storage) {
            return outcome;
        }
        let leaves = matches!(
            key,
            Keycode::Escape
                | Keycode::F12
                | Keycode::Space
                | Keycode::Up
                | Keycode::Down
                | Keycode::PageUp
                | Keycode::PageDown
                | Keycode::Home
                | Keycode::End
                | Keycode::Return
                | Keycode::KpEnter
                | Keycode::Delete
                | Keycode::F2
                | Keycode::Application
        );
        if self.dirty() && leaves {
            return self.ask_discard(Deferred::Key(key));
        }
        match key {
            Keycode::F12 => return Outcome::Close,
            Keycode::Escape if self.expanded => {
                return self.press(Button::Toggle, storage, unsaved);
            }
            Keycode::Escape => return Outcome::Close,
            Keycode::Space => return self.press(Button::Toggle, storage, unsaved),
            Keycode::Application => {
                let more = match self.focus {
                    Focus::Files if !self.expanded => Button::FileMore,
                    _ => Button::ProjectMore,
                };
                self.open_more(more, true);
                return Outcome::Stay;
            }
            Keycode::Tab => {
                match (self.expanded, &mut self.edit, self.focus) {
                    // From the project list into the fields.
                    (true, Some(edit), _) => edit.focus = Some(Field::Title),
                    (true, None, _) => self.focus = Focus::Projects,
                    (false, _, Focus::Projects) if !self.files.is_empty() => {
                        if self.file_selected.is_none() {
                            self.select_file(0);
                        }
                        self.focus = Focus::Files;
                    }
                    (false, ..) => self.focus = Focus::Projects,
                }
                return Outcome::Stay;
            }
            _ => {}
        }
        if self.focus == Focus::Files {
            // The expanded info area covers the files.
            if self.expanded {
                return Outcome::Stay;
            }
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
                Keycode::F2 => return self.press(Button::RenameFile, storage, unsaved),
                _ => return Outcome::Stay,
            };
            self.select_file(to);
            return Outcome::Stay;
        }
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
            _ => return Outcome::Stay,
        };
        if to != self.selected {
            self.select(to, storage);
        }
        Outcome::Stay
    }

    fn press(&mut self, button: Button, storage: &mut Storage, unsaved: &[usize]) -> Outcome {
        if !self.enabled(button) {
            return Outcome::Stay;
        }
        let name = self.selected_name().to_string();
        match button {
            Button::Switch => return self.switch(name, storage, unsaved, false, None),
            Button::LoadTo => self.mode = Mode::Slot(0),
            Button::Run => {
                let open = Open {
                    file: MAIN.to_string(),
                    slot: 0,
                    run: true,
                    home: true,
                };
                return self.open(open, storage, unsaved, false);
            }
            Button::RunFile => {
                let open = Open {
                    file: self.selected_program().expect("enabled").to_string(),
                    slot: 0,
                    run: true,
                    home: false,
                };
                return self.open(open, storage, unsaved, false);
            }
            Button::View => {
                let source = self.selected_file().expect("enabled");
                let viewer = Viewer::open(
                    self.frame(),
                    storage,
                    &source.project,
                    source.kind,
                    &source.name,
                );
                self.mode = Mode::View(Box::new(viewer));
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
            Button::RenameFile => {
                let source = self.selected_file().expect("enabled");
                let text = source.name.clone();
                self.ask_name(Naming::RenameFile(source), text, None);
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
            Button::ProjectMore | Button::FileMore => self.open_more(button, false),
            Button::New => self.ask_name(Naming::New, String::new(), None),
            Button::Rename => {
                let text = suggest_project_name(&name);
                let naming = Naming::Rename {
                    from: name,
                    then_switch: false,
                };
                self.ask_name(naming, text, None);
            }
            Button::Duplicate => match storage.duplicate_project(&name) {
                Ok(copy) => {
                    let label = project_label(&name);
                    self.status = Some((format!("Copied {label} to {copy}."), false));
                    self.refresh(storage, Some(&copy));
                    let naming = Naming::Rename {
                        from: copy.clone(),
                        then_switch: false,
                    };
                    self.ask_name(naming, copy, None);
                }
                Err(e) => {
                    let label = project_label(&name);
                    self.status = Some((format!("Could not copy {label}: {e}"), true));
                }
            },
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
            Button::Toggle => {
                self.expanded = !self.expanded;
                if !self.expanded {
                    self.edit = None;
                }
            }
            Button::Save => self.save_meta(storage),
            Button::ChangeIcon => {
                let start = storage.project_dir(&name).ok().filter(|d| d.is_dir());
                return Outcome::ChooseFile(Transfer::Icon(name), start);
            }
            Button::RemoveIcon => {
                if let Some(edit) = &mut self.edit {
                    edit.set_icon(vec![0; ICON_SIZE * ICON_SIZE]);
                }
            }
        }
        Outcome::Stay
    }

    /// Writes the edited metadata as the emulator keeps it, as METASAVE
    /// does: `@META.TXT` and `@ICON.PNG`, each whole or not at all, and
    /// then deletes SmileBASIC 4's `META`, so that no stale copy stays
    /// behind. Export makes it again from these.
    fn save_meta(&mut self, storage: &Storage) {
        let Some(edit) = &mut self.edit else {
            return;
        };
        let (title, description) = (edit.title.text(), edit.description.text());
        let meta = Meta {
            project: edit.project.clone(),
            home: edit.project.clone(),
            title: title.encode_utf16().collect(),
            description: description.encode_utf16().collect(),
            icon: edit.icon.clone(),
        };
        let label = project_label(&edit.project).to_string();
        let written = meta_files(&meta).and_then(|files| {
            (files.into_iter())
                .try_for_each(|(file, bytes)| storage.write_file_in(&edit.project, file, &bytes))
                .and_then(|()| storage.delete_file_in(&edit.project, SB4_META))
                .map_err(|e| e.to_string())
        });
        match written {
            Ok(()) => {
                edit.saved = (title, description, edit.icon.clone());
                edit.changed();
                self.status = Some((format!("Saved the title and icon of {label}."), false));
                self.listed[self.selected] = Listed::read(storage, &meta.project);
                self.info_key = None;
            }
            Err(e) => self.status = Some((format!("Could not save {label}: {e}"), true)),
        }
    }

    /// Goes on with `transfer` once the frontend's folder or file dialog
    /// has given `folder`.
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
            Transfer::Icon(project) => {
                let Some(edit) = self.edit.as_mut().filter(|e| e.project == project) else {
                    return;
                };
                let read = storage::read_chosen(&folder).map_err(|e| e.to_string());
                match read.and_then(|bytes| image::decode(&bytes)) {
                    Ok(img) if img.width > 0 && img.height > 0 => edit.set_icon(icon_from(&img)),
                    Ok(_) => self.status = Some(("The image is empty.".to_string(), true)),
                    Err(e) => self.status = Some((format!("Could not use that image: {e}"), true)),
                }
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

    /// Selects the project's files again, keeping the selected one's
    /// place, or selecting the file named.
    fn reselect_files(&mut self, storage: &Storage, name: Option<(FileKind, &str)>) {
        let (kept, focus) = (self.file_selected, self.focus);
        self.select(self.selected, storage);
        let found = name.and_then(|(kind, name)| {
            (self.files.iter()).position(|f| f.kind == kind && f.name == name)
        });
        if let Some(i) = found.or(kept) {
            self.select_file(i);
            if !self.files.is_empty() {
                self.focus = focus;
            }
        }
    }

    /// Copies a file to `to` as `name`, first asking before replacing a
    /// file there (unless `asked`).
    fn copy(&mut self, source: Source, to: String, name: String, storage: &Storage, asked: bool) {
        let name = name.to_ascii_uppercase();
        let kind = source.kind.name();
        if to == source.project && name == source.name {
            self.status = Some((format!("Can't copy {kind}:{name} onto itself."), true));
            return;
        }
        if !asked && storage.exists_in(&to, source.kind, &name) {
            let lines = vec![
                format!("{} already has {kind}:{name}.", project_label(&to)),
                String::new(),
                "Copying replaces it.".to_string(),
            ];
            let dialog = Dialog::new("Copy file", lines, "Replace", self.size);
            self.mode = Mode::Ask(dialog, Pending::Copy(source, to, name));
            return;
        }
        let from = (source.project.as_str(), source.name.as_str());
        self.status = Some(match storage.copy_file(source.kind, from, (&to, &name)) {
            Ok(name) => {
                let place = project_label(&to);
                let from = &source.name;
                match name == *from {
                    true => (format!("Copied {kind}:{from} to {place}."), false),
                    false => (format!("Copied {kind}:{from} to {place} as {name}."), false),
                }
            }
            Err(e) => (format!("Could not copy {kind}:{}: {e}", source.name), true),
        });
        if to == self.selected_name() {
            self.reselect_files(storage, None);
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
            home: false,
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
                self.reselect_files(storage, None);
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
            Pending::Discard(deferred) => {
                // Dropped; the event finds nothing unsaved now.
                self.edit = None;
                self.sync(storage);
                if let Some(edit) = &mut self.edit {
                    edit.focus = None;
                }
                return match deferred {
                    Deferred::Click(at) => self.click_in(at, storage, unsaved),
                    Deferred::DoubleClick(at, clicked) => {
                        self.clicked = clicked;
                        self.double_click_in(at, storage, unsaved)
                    }
                    Deferred::RightClick(at) => {
                        self.right_click(at, storage);
                        Outcome::Stay
                    }
                    Deferred::Key(key) => self.key_in(key, storage, unsaved),
                    Deferred::Run => self.pad_run(storage, unsaved),
                };
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
            Naming::RenameFile(source) => {
                storage.rename_in(&source.project, source.kind, &source.name, &name.text)
            }
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
        if let Naming::RenameFile(source) = &name.naming {
            let kind = source.kind.name();
            let from = &source.name;
            self.status = Some((format!("Renamed {kind}:{from} to {new}."), false));
            self.reselect_files(storage, Some((source.kind, &new)));
            return Outcome::Stay;
        }
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
            Naming::RenameFile(_) => unreachable!("done above"),
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
        let browsing = matches!(self.mode, Mode::Browse);
        let hovered = |r: Rect| browsing && self.mouse.is_some_and(|m| r.contains(m));
        // The last message, or else the current project, left of `×`.
        let close = self.button(Button::Close);
        let room = close
            .x
            .saturating_sub(f.x + 2 * PAD + text_width("Projects") + 2 * PAD);
        let (top, color) = match &self.status {
            Some((status, error)) => (fit(status, room), if *error { ERROR } else { TEXT }),
            None => (format!("Current: {}", project_label(&self.current)), TEXT),
        };
        let x = close.x - PAD - text_width(&top);
        draw_text(fb, font, x, f.y + 6, &top, color);
        if hovered(close) {
            fill(fb, close, BORDER);
        }
        mark(fb, Mark::Close, close.x + 4, close.y + 4, TEXT);

        self.draw_projects(fb, font);
        self.draw_files(fb, font);
        for b in PROJECT_TOOLBAR.into_iter().chain([
            self.file_action(),
            Button::LoadTo,
            Button::FileMore,
        ]) {
            let r = self.button(b);
            let blocked = self.blocked(b);
            let state = (!blocked && hovered(r), !blocked && self.enabled(b));
            mark_button(fb, font, r, b.mark(), b.label(), state, b.faces());
        }
        self.draw_info(fb, font, blink);
        let toggle = self.button(Button::Toggle);
        let m = if self.expanded { Mark::Up } else { Mark::Down };
        mark_button(
            fb,
            font,
            toggle,
            Some(m),
            "",
            (hovered(toggle), true),
            Button::Toggle.faces(),
        );

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
            Mode::Slot(chosen) => self.draw_slot(fb, font, *chosen),
            Mode::Copy(picker, _) => picker.draw(fb, font, blink),
            Mode::Menu(popup) => self.draw_menu(fb, font, popup),
            Mode::View(viewer) => viewer.draw(fb, font),
        }
    }

    fn draw_projects(&self, fb: &mut Framebuffer, font: &Font) {
        let list = self.list();
        draw_text(fb, font, list.x, self.top() + 4, "Projects", TEXT);
        fill(fb, list, PANEL);
        let shown = self.scroll..self.projects.len().min(self.scroll + self.rows());
        for i in shown {
            let r = self.row(i);
            let name = &self.projects[i];
            let current = *name == self.current;
            if i == self.selected {
                let color = match self.focus {
                    Focus::Projects => HIGHLIGHT,
                    Focus::Files => BORDER,
                };
                fill(fb, r, color);
            } else if current {
                fill(fb, r, CURRENT_TINT);
            }
            if current {
                fill(fb, Self::current_bar(r), CURRENT_BAR);
            }
            let listed = &self.listed[i];
            let side = ICON_SIZE as u32;
            if let Some(icon) = &listed.icon {
                let y = r.y + (ROW - side) / 2;
                image(fb, r.x + PAD, y, side, &icon.pixels, icon.key);
            }
            // The title over the folder's name, always shown.
            let x = Self::title_x(r);
            let room = (r.x + r.w).saturating_sub(x + PAD);
            draw_text(fb, font, x, r.y + 6, &fit(&listed.title, room), TEXT);
            let folder = project_label(name);
            let (label, color) = match usable(name) {
                true => (fit(folder, room), FOLDER),
                false => (fit(&format!("{folder} (rename to use)"), room), ERROR),
            };
            draw_text(fb, font, x, r.y + 26, &label, color);
        }
        outline(fb, list, BORDER);
    }

    fn draw_files(&self, fb: &mut Framebuffer, font: &Font) {
        let files = self.files_rect();
        let dim = self.expanded;
        let text = if dim { DIM_TEXT } else { TEXT };
        let heading = format!("Files ({})", self.files.len());
        draw_text(
            fb,
            font,
            files.x,
            self.files_toolbar_y() + 4,
            &heading,
            text,
        );
        fill(fb, files, PANEL);
        for i in self.shown_files() {
            let r = self.file_row(i);
            if self.file_selected == Some(i) {
                let color = match (dim, self.focus) {
                    (true, _) => DIM_BORDER,
                    (false, Focus::Files) => HIGHLIGHT,
                    (false, Focus::Projects) => BORDER,
                };
                fill(fb, r, color);
            }
            // The size and the date to the minute on the right, if the
            // name leaves room.
            let file = &self.files[i];
            let info = (file.info.as_ref())
                .map(|(size, date)| format!("{size:>7}  {}", date.get(..16).unwrap_or(date)));
            let info_w = info.as_ref().map_or(0, |i| text_width(i) + 2 * PAD);
            let label = fit(&file.label(), r.w - 2 * PAD - info_w);
            draw_text(fb, font, r.x + PAD, r.y + 4, &label, text);
            if let Some(info) = info {
                let x = r.x + r.w - PAD - text_width(&info);
                let color = if dim { DIM_TEXT } else { DISABLED };
                draw_text(fb, font, x, r.y + 4, &info, color);
            }
        }
        outline(fb, files, if dim { DIM_BORDER } else { BORDER });
    }

    fn draw_info(&self, fb: &mut Framebuffer, font: &Font, blink: bool) {
        let r = self.info_rect();
        fill(fb, r, PANEL);
        outline(fb, r, BORDER);
        match &self.info {
            Info::Project { summary, icon } => {
                let listed = &self.listed[self.selected];
                let name = self.selected_name();
                let at = self.big_icon_rect();
                let icon = match &self.edit {
                    Some(edit) => edit.picture.as_ref(),
                    None => icon.as_ref(),
                };
                match icon {
                    Some(p) => image(fb, at.x, at.y, p.w, &p.pixels, p.key),
                    None => outline(fb, at, DIM_BORDER),
                }
                let x = at.x + BIG_ICON + BIG_ICON_GAP;
                let save = self.edit.as_ref().map_or(0, |_| Button::Save.width() + PAD);
                let room = (r.x + r.w - 2 - PAD).saturating_sub(x + save);
                draw_text(fb, font, x, at.y + 4, &fit(&listed.title, room), TEXT);
                let (folder, color) = match usable(name) {
                    true => (project_label(name).to_string(), FOLDER),
                    false => (format!("{name} (rename to use)"), ERROR),
                };
                draw_text(fb, font, x, at.y + LINE + 4, &fit(&folder, room), color);
                draw_text(
                    fb,
                    font,
                    x,
                    at.y + 2 * LINE + 4,
                    &fit(summary, room),
                    DISABLED,
                );
                match &self.edit {
                    Some(edit) => self.draw_edit(fb, font, edit, blink),
                    None => {
                        let y = at.y + BIG_ICON + PAD + 4;
                        let room = self.button(Button::Toggle).x.saturating_sub(at.x + PAD);
                        let first = listed.description.lines().next().unwrap_or("");
                        match first {
                            "" => draw_text(fb, font, at.x, y, "(no description)", DISABLED),
                            line => draw_text(fb, font, at.x, y, &fit(line, room), TEXT),
                        }
                    }
                }
            }
            Info::File {
                label,
                about,
                info,
                lines,
                picture,
                failed,
            } => {
                let x = r.x + 2 + PAD;
                let y = r.y + 2 + PAD + 4;
                let right = r.x + r.w - 2 - PAD;
                let label_w = text_width(label) + 2 * PAD;
                draw_text(fb, font, x, y, &fit(label, right - x), TEXT);
                let about_w = text_width(about) + 2 * PAD;
                if label_w + about_w <= right - x {
                    draw_text(fb, font, x + label_w, y, about, DISABLED);
                }
                if label_w + about_w + text_width(info) <= right - x {
                    draw_text(fb, font, right - text_width(info), y, info, DISABLED);
                }
                let room = self.preview_rect();
                if let Some(e) = failed {
                    draw_text(fb, font, room.x, room.y + 4, &fit(e, room.w), ERROR);
                } else if let Some(p) = picture {
                    let well = Rect {
                        w: p.w + 4,
                        h: p.h + 4,
                        ..room
                    };
                    fill(fb, well, WELL);
                    outline(fb, well, DIM_BORDER);
                    image(fb, well.x + 2, well.y + 2, p.w, &p.pixels, p.key);
                } else {
                    fill(fb, room, WELL);
                    outline(fb, room, DIM_BORDER);
                    for (i, line) in lines.iter().enumerate() {
                        let y = room.y + 4 + i as u32 * LINE;
                        let text = fit(line, room.w - 2 * PAD);
                        draw_text(fb, font, room.x + PAD, y, &text, TEXT);
                    }
                }
            }
        }
    }

    fn draw_edit(&self, fb: &mut Framebuffer, font: &Font, edit: &MetaEdit, blink: bool) {
        let x = self.info_rect().x + 2 + PAD;
        let focused = |f: Field| edit.focus == Some(f);
        for (field, label) in [
            (Field::Title, "Title"),
            (Field::Description, "Description"),
            (Field::Change, "Icon"),
        ] {
            let r = self.field_rect(field);
            draw_text(fb, font, x, r.y + 4, label, TEXT);
        }
        let title = self.field_rect(Field::Title);
        edit.title
            .draw(fb, font, title, focused(Field::Title), blink);
        let description = self.field_rect(Field::Description);
        (edit.description).draw(fb, font, description, focused(Field::Description), blink);
        let browsing = matches!(self.mode, Mode::Browse);
        for (b, field) in [
            (Button::ChangeIcon, Field::Change),
            (Button::RemoveIcon, Field::Remove),
            (Button::Save, Field::Save),
        ] {
            let r = self.button(b);
            let hovered = browsing && self.mouse.is_some_and(|m| r.contains(m));
            mark_button(
                fb,
                font,
                r,
                None,
                b.label(),
                (hovered, self.enabled(b)),
                b.faces(),
            );
            if focused(field) {
                outline(fb, r, TEXT);
            }
        }
    }

    fn draw_menu(&self, fb: &mut Framebuffer, font: &Font, popup: &Popup) {
        let frame = self.frame();
        let all = popup_rect(frame, popup);
        fill(fb, all, BACKGROUND | 0xFF00_0000);
        for (i, &item) in popup.items.iter().enumerate() {
            let r = popup_item(frame, popup, i);
            let Some(item) = item else {
                let line = Rect {
                    x: r.x + 2,
                    y: r.y + r.h / 2 - 1,
                    w: r.w - 4,
                    h: 2,
                };
                fill(fb, line, SEPARATOR);
                continue;
            };
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

    fn draw_slot(&self, fb: &mut Framebuffer, font: &Font, chosen: usize) {
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
            let hovered = i == chosen || self.mouse.is_some_and(|m| r.contains(m));
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
            Naming::RenameFile(source) => format!("Rename {}:{}", source.kind.name(), source.name),
        };
        window(fb, font, f, &fit(&title, f.w - 4 * PAD));
        let x = f.x + 2 * PAD;
        let line = |i: u32| f.y + LINE + 2 * PAD + i * LINE + 4;
        let hint = match name.naming {
            Naming::RenameFile(_) => "@ first for a backup's name.",
            _ => "# first for a system project.",
        };
        draw_text(
            fb,
            font,
            x,
            line(0),
            "Letters, digits, _ - and . (up to 32);",
            TEXT,
        );
        draw_text(fb, font, x, line(1), hint, TEXT);
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
        m.click(centre(m.button(Button::ProjectMore)), &mut s, &[]);
        let Mode::Menu(popup) = &m.mode else {
            panic!("a menu")
        };
        assert_eq!(popup.items, PROJECT_MORE);
        let new = popup_item(m.frame(), popup, 0);
        assert_eq!(m.click(centre(new), &mut s, &[]), Outcome::Stay);
        assert!(matches!(m.mode, Mode::Name(_)));
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
        assert!(!m.enabled(Button::LoadTo) && !m.enabled(Button::RunFile));
        assert!(!m.enabled(Button::Run), "the workspace has no MAIN.PRG");

        // Tab moves to the files; DAT files cannot be loaded.
        m.key(Keycode::Tab, &mut s, &[]);
        assert_eq!(m.selected_program(), Some("A.PRG"));
        m.key(Keycode::Down, &mut s, &[]);
        assert!(!m.enabled(Button::LoadTo));
        m.key(Keycode::Up, &mut s, &[]);
        m.key(Keycode::Return, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Slot(0)));
        let open = Open {
            file: "A.PRG".to_string(),
            slot: 2,
            run: false,
            home: false,
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

        // Run runs the project's MAIN.PRG, switching to it first.
        m.key(Keycode::Tab, &mut s, &[]);
        m.key(Keycode::Down, &mut s, &[]);
        assert_eq!(m.selected_name(), "GAME");
        assert!(m.enabled(Button::Run));
        let at = centre(m.button(Button::Run));
        assert_eq!(
            m.click(at, &mut s, &[]),
            Outcome::Open {
                switched: Some("GAME".to_string()),
                open: Open {
                    file: MAIN.to_string(),
                    slot: 0,
                    run: true,
                    home: true,
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
    fn the_layout_follows_the_mockup() {
        let tmp = TempDir::new("layout");
        let s = Storage::new(tmp.0.clone());
        let mut m = ProjectManager::new(&s, (1280, 720));
        let f = m.frame();
        assert_eq!((f.w, f.h), (1200, 672));
        let (list, info, files) = (m.list(), m.info_rect(), m.files_rect());
        assert_eq!(list.w, LIST_WIDTH);
        assert_eq!(m.row(0).h, 48);
        assert!(m.rows() >= 11);
        // The info area over the files, beside the projects.
        assert_eq!((info.y, info.h), (m.top(), INFO_HEIGHT));
        assert!(info.x > list.x + list.w);
        assert!(files.y > info.y + info.h + LINE, "the toolbar between");
        assert_eq!(files.x + files.w, info.x + info.w);
        assert!(m.file_rows() >= 16);
        assert!(list.y + list.h <= f.y + f.h && files.y + files.h <= f.y + f.h);
        // Each toolbar in a row at the right of its column, above its list.
        let rows = [
            (&PROJECT_TOOLBAR, list, "Projects"),
            (&FILE_TOOLBAR, files, "Files (999)"),
        ];
        for (row, column, heading) in rows {
            for pair in row.windows(2) {
                let (a, b) = (m.button(pair[0]), m.button(pair[1]));
                assert!(a.x + a.w < b.x && a.y == b.y, "{pair:?}");
            }
            let (first, last) = (m.button(row[0]), m.button(row[2]));
            assert_eq!(last.x + last.w, column.x + column.w);
            assert!(first.x > column.x + text_width(heading));
            assert!(last.y + last.h < column.y);
        }
        assert_eq!(m.button(Button::View), m.button(Button::RunFile));
        let toggle = m.button(Button::Toggle);
        assert!(info.contains(centre(toggle)));
        assert_eq!(
            toggle.x + toggle.w + PAD + 2,
            info.x + info.w,
            "bottom right"
        );
        let close = m.button(Button::Close);
        assert!(close.x + close.w <= f.x + f.w && close.y < list.y);
        let last = m.slot_button(SLOTS - 1);
        assert!(last.x + last.w < m.slot_button(SLOTS).x);

        // Expanded, it covers most of the files; its fields fit.
        m.expanded = true;
        let info = m.info_rect();
        assert_eq!(info.h, INFO_EXPANDED);
        assert!(info.y + info.h > files.y && info.y + info.h < files.y + files.h);
        let toggle = m.button(Button::Toggle);
        assert_eq!(toggle.y + toggle.h + PAD + 2, info.y + info.h);
        let remove = m.button(Button::RemoveIcon);
        assert!(remove.y + remove.h < toggle.y);
        let change = m.button(Button::ChangeIcon);
        assert!(change.x + change.w < remove.x);
        let save = m.button(Button::Save);
        assert!(save.y + save.h < m.field_rect(Field::Title).y);
        let description = m.field_rect(Field::Description);
        assert!(description.y + description.h < remove.y);
        assert!(description.x + description.w <= info.x + info.w);
        let preview = m.preview_rect();
        assert!(preview.x + preview.w < toggle.x);
    }

    #[test]
    fn double_click_switches_and_closes() {
        let tmp = TempDir::new("double");
        let mut s = Storage::new(tmp.0.clone());
        s.create_project("GAME").unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        let current = centre(m.row(0));
        assert_eq!(m.click(current, &mut s, &[]), Outcome::Stay);
        assert_eq!(m.double_click(current, &mut s, &[]), Outcome::Close);
        let game = centre(m.row(1));
        assert_eq!(m.click(game, &mut s, &[]), Outcome::Stay);
        assert_eq!(
            m.double_click(game, &mut s, &[]),
            Outcome::Switched("GAME".to_string())
        );
    }

    #[test]
    fn double_clicks_count_on_one_project_only() {
        let tmp = TempDir::new("double-two");
        let mut s = Storage::new(tmp.0.clone());
        s.create_project("A").unwrap();
        s.create_project("B").unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        let (a, b) = (centre(m.row(1)), centre(m.row(2)));
        // A, then B quickly: a click on B, not a double click.
        assert_eq!(m.click(a, &mut s, &[]), Outcome::Stay);
        assert_eq!(m.double_click(b, &mut s, &[]), Outcome::Stay);
        assert_eq!(m.selected_name(), "B");
        // A third click on B is its second.
        assert_eq!(
            m.double_click(b, &mut s, &[]),
            Outcome::Switched("B".to_string())
        );
        // A button between the two clicks breaks them up too.
        let mut m = ProjectManager::new(&s, (1280, 720));
        let a = centre(m.row(1));
        m.click(centre(m.button(Button::ProjectMore)), &mut s, &[]);
        m.mode = Mode::Browse;
        assert_eq!(m.double_click(a, &mut s, &[]), Outcome::Stay);
        assert_eq!(m.selected_name(), "A");
    }

    #[test]
    fn rows_show_titles_and_icons() {
        let tmp = TempDir::new("meta");
        let s = Storage::new(tmp.0.clone());
        s.create_project("GAME").unwrap();
        s.create_project("PLAIN").unwrap();
        let mut icon = vec![0; ICON_SIZE * ICON_SIZE];
        icon[0] = 0xFF12_3456_u32 as i32;
        let meta = sb_runtime::env::Meta {
            title: "Space game".encode_utf16().collect(),
            icon,
            ..Default::default()
        };
        for (name, bytes) in sb_runtime::env::meta_files(&meta).unwrap() {
            s.write_file_in("GAME", name, &bytes).unwrap();
        }
        let m = ProjectManager::new(&s, (1280, 720));
        let game = &m.listed[1];
        assert_eq!(game.title, "Space game");
        assert_eq!(game.icon.as_ref().unwrap().pixels[0], 0xFF12_3456);
        let plain = &m.listed[2];
        assert_eq!(plain.title, "PLAIN");
        assert!(plain.icon.is_none());
        assert_eq!(m.listed[0].title, "[WORKSPACE]");
        // Two lines of text to a row.
        assert_eq!(m.row(1).h, 2 * LINE);
        let mut fb = Framebuffer::new(1280, 720);
        m.draw(&mut fb, &Font::blank(), false);
        let r = m.row(1);
        let at = (r.x + PAD, r.y + (ROW - ICON_SIZE as u32) / 2);
        assert_eq!(fb.get(at.0, at.1).map(|p| p & 0xFF_FFFF), Some(0x12_3456));
        // The title starts a few pixels after the icon.
        assert_eq!(
            ProjectManager::title_x(r),
            r.x + PAD + ICON_SIZE as u32 + ICON_GAP
        );
    }

    #[test]
    fn the_info_area_shows_the_icon_at_twice_its_size() {
        let tmp = TempDir::new("big");
        let mut s = Storage::new(tmp.0.clone());
        s.create_project("GAME").unwrap();
        // One pixel at (1, 0), so that its square starts at (2, 0).
        let mut icon = vec![0; ICON_SIZE * ICON_SIZE];
        icon[1] = 0xFF12_3456_u32 as i32;
        let meta = sb_runtime::env::Meta {
            icon,
            ..Default::default()
        };
        for (name, bytes) in sb_runtime::env::meta_files(&meta).unwrap() {
            s.write_file_in("GAME", name, &bytes).unwrap();
        }
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.key(Keycode::Down, &mut s, &[]);
        assert_eq!(BIG_ICON, 80);
        let at = m.big_icon_rect();
        assert_eq!((at.w, at.h), (2 * ICON_SIZE as u32, 2 * ICON_SIZE as u32));
        let mut fb = Framebuffer::new(1280, 720);
        m.draw(&mut fb, &Font::blank(), false);
        let rgb = |x, y| fb.get(at.x + x, at.y + y).map(|p| p & 0xFF_FFFF);
        for (x, y) in [(2, 0), (3, 0), (2, 1), (3, 1)] {
            assert_eq!(rgb(x, y), Some(0x12_3456), "({x}, {y})");
        }
        assert_ne!(rgb(1, 0), Some(0x12_3456));
        assert_ne!(rgb(4, 0), Some(0x12_3456));
        // The collapsed area keeps its height: the icon and the
        // description's line under it fit above its bottom, left of the
        // expand button.
        let info = m.info_rect();
        assert_eq!(info.h, INFO_HEIGHT);
        let description_bottom = at.y + BIG_ICON + PAD + LINE;
        assert!(description_bottom <= info.y + info.h - 2 - PAD);
        // Expanded, the fields still fit above the expand button.
        m.key(Keycode::Space, &mut s, &[]);
        let icon_row = m.field_rect(Field::Change);
        assert!(icon_row.y + icon_row.h <= m.button(Button::Toggle).y);
    }

    #[test]
    fn the_current_project_is_tinted_and_marked() {
        let tmp = TempDir::new("current");
        let mut s = Storage::new(tmp.0.clone());
        s.create_project("A").unwrap();
        s.create_project("B").unwrap();
        s.set_project("B").unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        assert_eq!(m.projects, ["", "A", "B"]);
        let pixel = |m: &ProjectManager, i: usize, (x, y): (u32, u32)| {
            let mut fb = Framebuffer::new(1280, 720);
            m.draw(&mut fb, &Font::blank(), false);
            let r = m.row(i);
            fb.get(r.x + x, r.y + y).unwrap()
        };
        // Past the bar and the icon, under the text.
        let body = (PAD + ICON_SIZE as u32 + 2, ROW - 3);
        let bar = (3, ROW / 2);
        // The current project opens selected: the selection's colour,
        // with the bar at the far left.
        assert_eq!(m.selected_name(), "B");
        assert_eq!(pixel(&m, 2, body), HIGHLIGHT);
        assert_eq!(pixel(&m, 2, bar), CURRENT_BAR);
        // Selecting another leaves it tinted and still marked; the other
        // project has neither.
        m.select_name("A", &s);
        assert_eq!(pixel(&m, 1, body), HIGHLIGHT);
        assert_ne!(pixel(&m, 1, bar), CURRENT_BAR);
        assert_eq!(pixel(&m, 2, body), CURRENT_TINT);
        assert_eq!(pixel(&m, 2, bar), CURRENT_BAR);
        assert_ne!(pixel(&m, 0, body), CURRENT_TINT);
        // The tint differs from both selection colours and the buttons'.
        for other in [HIGHLIGHT, BORDER, BACKGROUND | 0xFF00_0000] {
            assert_ne!(CURRENT_TINT, other);
        }
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
        assert_eq!(popup.items, FILE_MENU);
        assert!(!m.enabled(Button::LoadTo), "a DAT file");
        let at = FILE_MENU
            .iter()
            .position(|&b| b == Some(Button::DeleteFile));
        let delete = popup_item(m.frame(), popup, at.unwrap());
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
            m.press(Button::RunFile, &mut s, &[]),
            Outcome::Open {
                switched: None,
                open: Open {
                    file: "A.PRG".to_string(),
                    slot: 0,
                    run: true,
                    home: false,
                }
            }
        );
    }

    #[test]
    fn only_run_of_main_comes_home_and_the_manager_comes_back_as_left() {
        let tmp = TempDir::new("home");
        let mut s = Storage::new(tmp.0.clone());
        s.write(FileKind::Txt, "MAIN.PRG", b"?1", None).unwrap();
        s.write(FileKind::Txt, "A.PRG", b"?1", None).unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        let home = |o: Outcome| matches!(o, Outcome::Open { open, .. } if open.home);
        assert!(home(m.press(Button::Run, &mut s, &[])));
        m.key(Keycode::Tab, &mut s, &[]);
        m.key(Keycode::Down, &mut s, &[]);
        assert!(!home(m.press(Button::RunFile, &mut s, &[])));
        let left = m.memory();
        let back = ProjectManager::reopen(&s, (1280, 720), &left);
        assert_eq!(back.focus, Focus::Files);
        assert_eq!(back.file_selected, m.file_selected);
        assert_eq!(back.memory(), left);
    }

    #[test]
    fn rereads_what_other_programs_changed() {
        let tmp = TempDir::new("reread");
        let mut s = Storage::new(tmp.0.clone());
        s.create_project("GAME").unwrap();
        s.create_project("ZOO").unwrap();
        s.set_project("GAME").unwrap();
        s.write(FileKind::Txt, "B", b"", None).unwrap();
        s.set_project("").unwrap();
        let game = tmp.0.join("GAME");
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.key(Keycode::Down, &mut s, &[]);
        m.key(Keycode::Tab, &mut s, &[]);
        assert_eq!(
            (m.selected_name(), m.selected_program()),
            ("GAME", Some("B"))
        );
        let names = |m: &ProjectManager| -> Vec<String> {
            m.files.iter().map(|f| f.name.clone()).collect()
        };

        std::fs::write(game.join("TA"), "?1").unwrap();
        std::fs::create_dir(tmp.0.join("ADDED")).unwrap();
        // The folders' times as the lists were read: nothing to read again.
        m.read_at = (s.changed_at(None), s.changed_at(Some("GAME")));
        m.reread(&s, false);
        assert_eq!((m.projects.len(), names(&m)), (3, vec!["B".to_string()]));
        // Read before the change: both lists again, B still selected.
        m.read_at.1 = Some(SystemTime::UNIX_EPOCH);
        m.reread(&s, false);
        assert_eq!(m.projects, ["", "ADDED", "GAME", "ZOO"]);
        assert_eq!(names(&m), ["A", "B"]);
        assert_eq!(
            (m.selected_name(), m.selected_program()),
            ("GAME", Some("B"))
        );
        assert_eq!(m.focus, Focus::Files);

        // A question waits, and the lists come once it is answered.
        std::fs::remove_file(game.join("TB")).unwrap();
        m.press(Button::DeleteFile, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Ask(..)));
        m.reread(&s, true);
        assert_eq!(names(&m), ["A", "B"]);
        m.key(Keycode::Escape, &mut s, &[]);
        m.reread(&s, false);
        assert_eq!(names(&m), ["A"]);
        assert_eq!(m.selected_program(), Some("A"), "the row left");

        // The selected project gone: the one after it in its place.
        std::fs::remove_dir_all(&game).unwrap();
        m.reread(&s, true);
        assert_eq!(m.projects, ["", "ADDED", "ZOO"]);
        assert_eq!((m.selected_name(), m.focus), ("ZOO", Focus::Projects));
        assert!(m.files.is_empty());
    }

    /// A GRP file of `w` x `h` pixels, as SAVEG writes it (a PNG).
    fn grp(w: usize, h: usize) -> Vec<u8> {
        let img = image::Image {
            width: w,
            height: h,
            pixels: vec![0xFF00_FF00; w * h],
        };
        image::encode_png(&img).unwrap()
    }

    #[test]
    fn run_selected_turns_into_view() {
        let tmp = TempDir::new("view");
        let mut s = Storage::new(tmp.0.clone());
        s.write(FileKind::Txt, "A.PRG", b"?1\n?2\n", None).unwrap();
        s.write(FileKind::Dat, "D", b"", None).unwrap();
        s.write(FileKind::Grp, "G", &grp(8, 4), None).unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        // Nothing selected: Run selected, disabled.
        assert_eq!(m.file_action(), Button::RunFile);
        assert!(!m.enabled(Button::RunFile));
        let names = ["A.PRG", "D", "G"];
        let expected = [Button::RunFile, Button::View, Button::View];
        m.key(Keycode::Tab, &mut s, &[]);
        for (name, action) in names.into_iter().zip(expected) {
            assert_eq!(m.selected_file().unwrap().name, name);
            assert_eq!(m.file_action(), action, "{name}");
            assert!(m.enabled(action));
            m.key(Keycode::Down, &mut s, &[]);
        }
        // View opens the viewer over the window; Esc closes it.
        let view = centre(m.button(Button::View));
        assert_eq!(m.click(view, &mut s, &[]), Outcome::Stay);
        assert!(matches!(m.mode, Mode::View(_)));
        let mut fb = Framebuffer::new(1280, 720);
        m.draw(&mut fb, &Font::blank(), false);
        m.key(Keycode::Escape, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Browse));
        // The menu's View shows a program too.
        m.key(Keycode::Home, &mut s, &[]);
        m.key(Keycode::Application, &mut s, &[]);
        let Mode::Menu(popup) = &m.mode else {
            panic!("the files' menu")
        };
        assert_eq!(popup.items, FILE_MORE);
        assert_eq!(popup.chosen, Some(0), "View, the first item");
        m.key(Keycode::Return, &mut s, &[]);
        assert!(matches!(m.mode, Mode::View(_)));
    }

    #[test]
    fn the_info_follows_the_list_clicked_last() {
        let tmp = TempDir::new("info");
        let mut s = Storage::new(tmp.0.clone());
        s.write(FileKind::Txt, "A.PRG", b"OPTION STRICT\n\tVAR A\n", None)
            .unwrap();
        let data = sb_runtime::sbfile::Data {
            dims: vec![2, 2],
            values: sb_runtime::sbfile::DataValues::Int(vec![1, 2, 3, 4]),
        };
        let body = sb_runtime::sbfile::data_body(&data);
        s.write(FileKind::Dat, "D", &body, None).unwrap();
        s.write(FileKind::Grp, "G", &grp(512, 256), None).unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        let Info::Project { summary, .. } = &m.info else {
            panic!("the project's info")
        };
        assert!(summary.starts_with("3 files, "), "{summary}");
        m.click(centre(m.file_row(0)), &mut s, &[]);
        let Info::File { about, lines, .. } = &m.info else {
            panic!("the file's info")
        };
        assert_eq!(about, "2 lines");
        assert_eq!(lines, &["OPTION STRICT", "    VAR A"]);
        m.click(centre(m.file_row(1)), &mut s, &[]);
        let Info::File { about, lines, .. } = &m.info else {
            panic!("the file's info")
        };
        assert_eq!(
            (about.as_str(), &lines[..]),
            ("int 2x2", &["1 2".into(), "3 4".into()][..])
        );
        m.click(centre(m.file_row(2)), &mut s, &[]);
        let Info::File { about, picture, .. } = &m.info else {
            panic!("the file's info")
        };
        assert_eq!(about, "512x256");
        let p = picture.as_ref().unwrap();
        let room = m.preview_rect();
        assert!(p.w <= room.w && p.h <= room.h && p.w == 2 * p.h);
        // Expanded, the picture is larger.
        let small = p.w;
        m.key(Keycode::Space, &mut s, &[]);
        let Info::File { picture, .. } = &m.info else {
            panic!("the file's info")
        };
        assert!(picture.as_ref().unwrap().w > small);
        m.key(Keycode::Space, &mut s, &[]);
        // A click on a project shows it again.
        m.click(centre(m.row(0)), &mut s, &[]);
        assert!(matches!(m.info, Info::Project { .. }));
        let mut fb = Framebuffer::new(1280, 720);
        m.draw(&mut fb, &Font::blank(), false);
    }

    #[test]
    fn the_expanded_info_takes_the_files_input() {
        let tmp = TempDir::new("expand");
        let mut s = Storage::new(tmp.0.clone());
        for i in 0..30 {
            s.write(FileKind::Dat, &format!("D{i:02}"), b"", None)
                .unwrap();
        }
        s.create_project("GAME").unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        let (low_file, toolbar) = (centre(m.file_row(17)), centre(m.button(Button::LoadTo)));
        m.click(centre(m.button(Button::Toggle)), &mut s, &[]);
        assert!(m.expanded && m.edit.is_some());
        assert_eq!(m.edit.as_ref().unwrap().focus, Some(Field::Title));
        // The files under it and beside it take no clicks, wheels or drags.
        m.click(low_file, &mut s, &[]);
        assert_eq!(m.file_selected, None);
        m.click(toolbar, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Browse));
        m.mouse_move(low_file);
        m.wheel(5);
        assert_eq!(m.file_scroll, 0);
        m.right_click(low_file, &s);
        assert!(matches!(m.mode, Mode::Browse));
        // The projects still do, and the info follows.
        m.click(centre(m.row(1)), &mut s, &[]);
        assert_eq!(m.selected_name(), "GAME");
        assert_eq!(m.edit.as_ref().unwrap().project, "GAME");
        assert_eq!(m.edit.as_ref().unwrap().focus, None, "the keys on the list");
        // Esc collapses, then closes.
        assert_eq!(m.key(Keycode::Escape, &mut s, &[]), Outcome::Stay);
        assert!(!m.expanded && m.edit.is_none());
        m.click(low_file, &mut s, &[]);
        assert!(m.file_selected.is_none(), "GAME has no files");
        m.click(centre(m.row(0)), &mut s, &[]);
        m.click(low_file, &mut s, &[]);
        assert_eq!(m.file_selected, Some(17));
        // Expanded over a file, the file list takes no keys.
        m.key(Keycode::Space, &mut s, &[]);
        assert!(m.expanded && m.edit.is_none());
        m.key(Keycode::Down, &mut s, &[]);
        assert_eq!(m.file_selected, Some(17));
        m.click(centre(m.button(Button::Toggle)), &mut s, &[]);
        assert!(!m.expanded);
        assert_eq!(m.key(Keycode::Escape, &mut s, &[]), Outcome::Close);
    }

    #[test]
    fn metadata_is_edited_and_saved() {
        let tmp = TempDir::new("meta-save");
        let mut s = Storage::new(tmp.0.clone());
        s.create_project("GAME").unwrap();
        // SmileBASIC 4's own META is read until the emulator's files are
        // saved, and then goes.
        let sb4 = sb_runtime::transfer::export_meta(
            &Meta {
                title: "Old".encode_utf16().collect(),
                ..Meta::default()
            },
            sb_runtime::sbfile::Date::now(),
        );
        s.write_file_in("GAME", "META", &sb4).unwrap();
        s.write_file_in("GAME", "G", &grp(80, 40)).unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.key(Keycode::Down, &mut s, &[]);
        assert_eq!(m.listed[1].title, "Old");
        m.key(Keycode::Space, &mut s, &[]);
        assert!(!m.enabled(Button::Save));
        m.key(Keycode::End, &mut s, &[]);
        m.text(" Game!");
        m.key(Keycode::Return, &mut s, &[]);
        assert_eq!(m.edit.as_ref().unwrap().focus, Some(Field::Description));
        m.text("Line 1\n");
        m.key(Keycode::Return, &mut s, &[]);
        m.text("Line 2");
        assert!(m.enabled(Button::Save));
        // The arrows stay in the fields, not moving the project list.
        for key in [Keycode::Up, Keycode::Up, Keycode::Down, Keycode::PageDown] {
            m.key(key, &mut s, &[]);
        }
        assert!(matches!(m.mode, Mode::Browse));
        assert_eq!(m.selected_name(), "GAME");
        // An image file becomes the icon, fitted into its square.
        let image = tmp.0.join("GAME").join("G");
        m.folder_chosen(Transfer::Icon("GAME".into()), image, &s);
        let edit = m.edit.as_ref().unwrap();
        assert_eq!(edit.icon[10 * ICON_SIZE], 0xFF00_FF00_u32 as i32);
        assert_eq!(edit.icon[0], 0, "a margin above the wide image");
        // Leaving asks first; Cancel stays.
        m.key(Keycode::Escape, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Ask(_, Pending::Discard(_))));
        m.key(Keycode::Escape, &mut s, &[]);
        assert!(m.expanded && m.dirty());
        m.click(centre(m.button(Button::Save)), &mut s, &[]);
        assert!(!m.dirty());
        let meta = project_meta("GAME", |name| s.read_file_in("GAME", name).ok());
        assert_eq!(String::from_utf16_lossy(&meta.title), "Old Game!");
        assert_eq!(
            String::from_utf16_lossy(&meta.description),
            "Line 1\n\nLine 2"
        );
        assert_eq!(meta.icon[10 * ICON_SIZE], 0xFF00_FF00_u32 as i32);
        assert!(!tmp.0.join("GAME").join("META").exists());
        assert_eq!(m.listed[1].title, "Old Game!");
        // Removing the icon, and dropping that change.
        m.click(centre(m.button(Button::RemoveIcon)), &mut s, &[]);
        assert!(m.dirty() && !m.enabled(Button::RemoveIcon));
        m.key(Keycode::Escape, &mut s, &[]);
        m.key(Keycode::Left, &mut s, &[]);
        m.key(Keycode::Return, &mut s, &[]);
        assert!(!m.expanded, "discarded, then collapsed");
        let meta = project_meta("GAME", |name| s.read_file_in("GAME", name).ok());
        assert_ne!(meta.icon[10 * ICON_SIZE], 0);
        // A title is cut at SmileBASIC's limit.
        m.key(Keycode::Space, &mut s, &[]);
        m.text(&"x".repeat(100));
        assert_eq!(m.edit.as_ref().unwrap().title.text().len(), TITLE_CHARS);
    }

    #[test]
    fn a_failed_metadata_save_keeps_meta() {
        let tmp = TempDir::new("meta-fail");
        let mut s = Storage::new(tmp.0.clone());
        s.create_project("GAME").unwrap();
        s.write_file_in("GAME", "META", b"old").unwrap();
        std::fs::create_dir(tmp.0.join("GAME").join("@ICON.PNG")).unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.key(Keycode::Down, &mut s, &[]);
        m.key(Keycode::Space, &mut s, &[]);
        m.text("x");
        m.click(centre(m.button(Button::Save)), &mut s, &[]);
        assert!(m.status.as_ref().is_some_and(|(_, error)| *error));
        assert!(m.dirty());
        assert_eq!(s.read_file_in("GAME", "META").unwrap(), b"old");
    }

    #[test]
    fn double_click_runs_a_project_with_a_main() {
        let tmp = TempDir::new("double-run");
        let mut s = Storage::new(tmp.0.clone());
        s.create_project("GAME").unwrap();
        s.write_file_in("GAME", "TMAIN.PRG", b"?1").unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        let game = centre(m.row(1));
        m.click(game, &mut s, &[]);
        assert!(m.enabled(Button::Run));
        assert_eq!(
            m.double_click(game, &mut s, &[]),
            Outcome::Open {
                switched: Some("GAME".to_string()),
                open: Open {
                    file: MAIN.to_string(),
                    slot: 0,
                    run: true,
                    home: true,
                }
            }
        );
    }

    #[test]
    fn menus_duplicate_and_rename() {
        let tmp = TempDir::new("menus");
        let mut s = Storage::new(tmp.0.clone());
        s.create_project("GAME").unwrap();
        s.write_file_in("GAME", "TMAIN.PRG", b"?1").unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.key(Keycode::Down, &mut s, &[]);
        // The menu key opens the projects' menu on its first item, and the
        // keys skip the lines between groups.
        m.key(Keycode::Application, &mut s, &[]);
        let chosen = |m: &ProjectManager| match &m.mode {
            Mode::Menu(popup) => popup.chosen.and_then(|i| popup.items[i]),
            _ => None,
        };
        assert_eq!(chosen(&m), Some(Button::New));
        m.key(Keycode::Up, &mut s, &[]);
        assert_eq!(chosen(&m), Some(Button::Delete));
        m.key(Keycode::Up, &mut s, &[]);
        assert_eq!(chosen(&m), Some(Button::Export));
        m.key(Keycode::Up, &mut s, &[]);
        m.key(Keycode::Up, &mut s, &[]);
        assert_eq!(chosen(&m), Some(Button::Duplicate));
        m.key(Keycode::Return, &mut s, &[]);
        // Duplicate copies, then offers to rename the copy.
        assert!(s.exists_in("GAME2", FileKind::Txt, "MAIN.PRG"));
        let Mode::Name(name) = &m.mode else {
            panic!("asks for a name")
        };
        assert_eq!(name.text, "GAME2");
        for _ in 0..5 {
            m.key(Keycode::Backspace, &mut s, &[]);
        }
        m.text("copy");
        m.key(Keycode::Return, &mut s, &[]);
        assert_eq!(m.projects, ["", "COPY", "GAME"]);
        assert_eq!(m.selected_name(), "COPY");

        // Renaming a file, from the files' menu.
        m.key(Keycode::Tab, &mut s, &[]);
        m.click(centre(m.button(Button::FileMore)), &mut s, &[]);
        let Mode::Menu(popup) = &m.mode else {
            panic!("a menu")
        };
        let at = FILE_MORE
            .iter()
            .position(|&b| b == Some(Button::RenameFile));
        let rename = popup_item(m.frame(), popup, at.unwrap());
        m.click(centre(rename), &mut s, &[]);
        let Mode::Name(name) = &m.mode else {
            panic!("asks for a name")
        };
        assert_eq!(name.text, "MAIN.PRG");
        m.key(Keycode::Backspace, &mut s, &[]);
        m.key(Keycode::Backspace, &mut s, &[]);
        m.key(Keycode::Backspace, &mut s, &[]);
        m.text("TXT");
        m.key(Keycode::Return, &mut s, &[]);
        assert!(s.exists_in("COPY", FileKind::Txt, "MAIN.TXT"));
        assert_eq!(m.selected_file().unwrap().name, "MAIN.TXT");
        assert_eq!(m.focus, Focus::Files);
        // F2 renames too.
        m.key(Keycode::F2, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Name(_)));
    }

    #[test]
    fn a_controller_drives_the_window() {
        let tmp = TempDir::new("pad");
        let mut s = Storage::new(tmp.0.clone());
        s.write(FileKind::Txt, "A.PRG", b"?1", None).unwrap();
        s.write(FileKind::Dat, "B", b"", None).unwrap();
        s.create_project("GAME").unwrap();
        s.set_project("GAME").unwrap();
        s.write(FileKind::Txt, MAIN, b"?2", None).unwrap();
        s.set_project("").unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.pad(Pad::DPadDown, &mut s, &[]);
        assert_eq!(m.selected_name(), "GAME");
        m.pad(Pad::DPadUp, &mut s, &[]);
        // X goes to the files and back.
        m.pad(Pad::North, &mut s, &[]);
        assert_eq!(m.focus, Focus::Files);
        m.pad(Pad::North, &mut s, &[]);
        assert_eq!(m.focus, Focus::Projects);
        m.pad(Pad::North, &mut s, &[]);
        assert_eq!(m.focus, Focus::Files);
        // A runs the selected program...
        let run = |file: &str| Outcome::Open {
            switched: None,
            open: Open {
                file: file.to_string(),
                slot: 0,
                run: true,
                home: false,
            },
        };
        assert_eq!(m.pad(Pad::East, &mut s, &[]), run("A.PRG"));
        // ...and views the other files.
        m.pad(Pad::DPadDown, &mut s, &[]);
        assert_eq!(m.selected_file().unwrap().name, "B");
        assert_eq!(m.pad(Pad::East, &mut s, &[]), Outcome::Stay);
        assert!(matches!(m.mode, Mode::View(_)));
        // In the viewer B goes back.
        m.pad(Pad::South, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Browse));
        // No button opens the list's menu; opened otherwise, A chooses in
        // it and B, L or R closes it.
        m.pad(Pad::Start, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Browse));
        m.key(Keycode::Application, &mut s, &[]);
        m.pad(Pad::LeftShoulder, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Browse));
        m.key(Keycode::Application, &mut s, &[]);
        m.pad(Pad::RightShoulder, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Browse));
        m.key(Keycode::Application, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Menu(_)));
        m.pad(Pad::South, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Browse));
        // L and R move a page; with two files that is to the ends.
        m.pad(Pad::RightShoulder, &mut s, &[]);
        assert_eq!(m.selected_file().unwrap().name, "B");
        m.pad(Pad::LeftShoulder, &mut s, &[]);
        assert_eq!(m.selected_file().unwrap().name, "A.PRG");
        m.pad(Pad::DPadDown, &mut s, &[]);
        m.pad(Pad::DPadUp, &mut s, &[]);
        m.key(Keycode::Application, &mut s, &[]);
        // The file menu starts on View.
        m.pad(Pad::East, &mut s, &[]);
        assert!(matches!(m.mode, Mode::View(_)));
        // On the project list, A runs MAIN.PRG, switching first; with no
        // MAIN.PRG it does nothing, as Run is disabled.
        m.mode = Mode::Browse;
        m.pad(Pad::North, &mut s, &[]);
        assert_eq!(m.focus, Focus::Projects);
        assert_eq!(m.pad(Pad::East, &mut s, &[]), Outcome::Stay);
        assert!(matches!(m.mode, Mode::Browse));
        m.pad(Pad::DPadDown, &mut s, &[]);
        assert_eq!(m.selected_name(), "GAME");
        assert_eq!(
            m.pad(Pad::East, &mut s, &[]),
            Outcome::Open {
                switched: Some("GAME".to_string()),
                open: Open {
                    file: MAIN.to_string(),
                    slot: 0,
                    run: true,
                    home: true,
                }
            }
        );
        // Over an unsaved title, A asks first, then runs.
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.pad(Pad::DPadDown, &mut s, &[]);
        m.pad(Pad::West, &mut s, &[]);
        m.pad(Pad::North, &mut s, &[]);
        assert!(m.in_field());
        m.text("X");
        while m.in_field() {
            m.pad(Pad::North, &mut s, &[]);
        }
        m.pad(Pad::East, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Ask(_, Pending::Discard(_))));
        m.pad(Pad::DPadLeft, &mut s, &[]);
        assert!(matches!(
            m.pad(Pad::East, &mut s, &[]),
            Outcome::Open {
                open: Open { run: true, .. },
                ..
            }
        ));
        // The keyboard's Enter still switches.
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.key(Keycode::Up, &mut s, &[]);
        assert_eq!(
            m.key(Keycode::Return, &mut s, &[]),
            Outcome::Switched("[WORKSPACE]".to_string())
        );
        // Y expands the info area, B collapses it, then closes.
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.pad(Pad::West, &mut s, &[]);
        assert!(m.expanded);
        m.pad(Pad::South, &mut s, &[]);
        assert!(!m.expanded);
        assert_eq!(m.pad(Pad::South, &mut s, &[]), Outcome::Close);
    }

    #[test]
    fn f5_runs_the_selected_project() {
        let tmp = TempDir::new("f5");
        let mut s = Storage::new(tmp.0.clone());
        s.write(FileKind::Txt, "A.PRG", b"?1", None).unwrap();
        s.create_project("GAME").unwrap();
        s.set_project("GAME").unwrap();
        s.write(FileKind::Txt, MAIN, b"?2", None).unwrap();
        s.write(FileKind::Txt, "B.PRG", b"?3", None).unwrap();
        s.set_project("").unwrap();
        let main = Outcome::Open {
            switched: Some("GAME".to_string()),
            open: Open {
                file: MAIN.to_string(),
                slot: 0,
                run: true,
                home: true,
            },
        };
        // With no MAIN.PRG it does nothing, as Run is disabled.
        let mut m = ProjectManager::new(&s, (1280, 720));
        assert_eq!(m.key(Keycode::F5, &mut s, &[]), Outcome::Stay);
        assert!(matches!(m.mode, Mode::Browse));
        // On the project list it runs MAIN.PRG, switching first, as A does.
        m.key(Keycode::Down, &mut s, &[]);
        assert_eq!(m.selected_name(), "GAME");
        assert_eq!(m.key(Keycode::F5, &mut s, &[]), main);
        // Back on the project list after the files, it still runs MAIN.PRG.
        s.set_project("").unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.key(Keycode::Down, &mut s, &[]);
        m.key(Keycode::Tab, &mut s, &[]);
        m.key(Keycode::Tab, &mut s, &[]);
        assert_eq!(m.focus, Focus::Projects);
        assert_eq!(m.selected_file().unwrap().name, "B.PRG");
        assert_eq!(m.key(Keycode::F5, &mut s, &[]), main);
        // Over an unsaved title it asks first, then runs.
        s.set_project("").unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.key(Keycode::Down, &mut s, &[]);
        m.key(Keycode::Space, &mut s, &[]);
        m.key(Keycode::Tab, &mut s, &[]);
        assert!(m.in_field());
        m.text("X");
        m.key(Keycode::F5, &mut s, &[]);
        assert!(matches!(m.mode, Mode::Ask(_, Pending::Discard(_))));
        m.pad(Pad::DPadLeft, &mut s, &[]);
        assert_eq!(m.pad(Pad::East, &mut s, &[]), main);
    }

    #[test]
    fn f5_on_the_files_runs_the_selected_program() {
        let tmp = TempDir::new("f5file");
        let mut s = Storage::new(tmp.0.clone());
        s.create_project("GAME").unwrap();
        s.set_project("GAME").unwrap();
        s.write(FileKind::Txt, MAIN, b"?2", None).unwrap();
        s.write(FileKind::Txt, "B.PRG", b"?3", None).unwrap();
        s.set_project("").unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.key(Keycode::Down, &mut s, &[]);
        m.key(Keycode::Tab, &mut s, &[]);
        assert_eq!(m.focus, Focus::Files);
        assert_eq!(m.selected_file().unwrap().name, "B.PRG");
        assert_eq!(
            m.key(Keycode::F5, &mut s, &[]),
            Outcome::Open {
                switched: Some("GAME".to_string()),
                open: Open {
                    file: "B.PRG".to_string(),
                    slot: 0,
                    run: true,
                    home: false,
                },
            }
        );
    }

    #[test]
    fn f5_on_a_file_that_is_not_a_program_does_nothing() {
        let tmp = TempDir::new("f5dat");
        let mut s = Storage::new(tmp.0.clone());
        s.write(FileKind::Dat, "D", b"", None).unwrap();
        s.write(FileKind::Txt, MAIN, b"?1", None).unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        m.key(Keycode::Tab, &mut s, &[]);
        m.key(Keycode::End, &mut s, &[]);
        assert_eq!(m.focus, Focus::Files);
        assert_eq!(m.selected_file().unwrap().kind, FileKind::Dat);
        // Neither MAIN.PRG nor the viewer A would open.
        assert_eq!(m.key(Keycode::F5, &mut s, &[]), Outcome::Stay);
        assert!(matches!(m.mode, Mode::Browse));
    }

    #[test]
    fn drawing_is_the_same_until_something_changes() {
        let tmp = TempDir::new("dry");
        let mut s = Storage::new(tmp.0.clone());
        s.write(FileKind::Grp, "G", &grp(64, 64), None).unwrap();
        let mut m = ProjectManager::new(&s, (1280, 720));
        let font = Font::blank();
        let key = |m: &ProjectManager| {
            let mut fb = Framebuffer::new(1280, 720);
            crate::ui::dry_run(|| m.draw(&mut fb, &font, false)).key
        };
        let before = key(&m);
        assert_eq!(key(&m), before);
        m.click(centre(m.file_row(0)), &mut s, &[]);
        let file = key(&m);
        assert_ne!(file, before);
        m.key(Keycode::Space, &mut s, &[]);
        assert_ne!(key(&m), file);
    }
}

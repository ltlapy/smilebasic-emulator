//! Handheld mode: the frontend laid out for a small screen and a gamepad
//! alone. The UI is as many pixels as the screen at a whole scale, the
//! emulator starts in a list of the projects instead of DIRECT mode, and
//! Select (SDL's Back, the − button) with Start opens a pause menu.
//! Programs never see Select: BUTTON has no bit for it.

use sb_runtime::Framebuffer;
use sb_runtime::confirm::project_label;
use sb_runtime::env::ICON_SIZE;
use sb_runtime::font::Font;
use sb_runtime::screen::Filter;
use sb_runtime::storage::{FileKind, Storage};
use sdl3::gamepad::Button;
use sdl3::keyboard::Keycode;
use sdl3::render::FRect;

use crate::program_dialog::wrap;
use crate::projects::{ERROR, FOLDER, Listed, fit, usable};
use crate::ui::{
    BORDER, CHAR, DISABLED, HIGHLIGHT, LINE, PAD, Rect, TEXT, draw_text, fill, image, outline,
    text_width, window,
};

/// The UI's height at a scale of 1: at a whole multiple of it or more,
/// UI pixels are that many screen pixels across.
const UI_LINES: u32 = 480;

/// The whole number of screen pixels a UI pixel takes on an output this
/// high.
pub fn ui_scale(out_h: u32) -> u32 {
    (out_h / UI_LINES).max(1)
}

/// The UI's size on an output of `out` pixels: the output at
/// [`ui_scale`], so that the 16-pixel UI font stays sharp. 640x480 gives
/// 640x480, 1920x1080 gives 960x540.
pub fn ui_size((w, h): (u32, u32)) -> (u32, u32) {
    let scale = ui_scale(h);
    ((w / scale).max(1), (h / scale).max(1))
}

/// Where a UI of `ui` pixels goes on an output of `out` pixels: at the
/// largest whole scale that fits, centered across and at the top, as the
/// desktop's display is.
pub fn display_rect((w, h): (u32, u32), (out_w, out_h): (u32, u32)) -> FRect {
    let scale = (out_w / w.max(1)).min(out_h / h.max(1)).max(1);
    let (sw, sh) = (w * scale, h * scale);
    FRect::new(
        out_w.saturating_sub(sw) as f32 / 2.0,
        0.0,
        sw as f32,
        sh as f32,
    )
}

/// The screen filter as shown: the sharp choice keeps to whole factors on
/// the desktop, which leaves a small screen mostly empty, so in handheld
/// mode it fills the screen with the pixel art filter instead.
pub fn shown_filter(chosen: Option<Filter>, handheld: bool) -> Option<Filter> {
    match chosen {
        Some(Filter::Nearest) if handheld => Some(Filter::SmartNearest),
        chosen => chosen,
    }
}

/// What a press of Start is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// With Select held, in handheld mode: the pause menu.
    Menu,
    /// The + button, as ever.
    Plus,
}

/// Select as the modifier of handheld mode's hotkeys.
#[derive(Debug, Default)]
pub struct Hotkeys {
    select: bool,
    /// The Start held now opened the menu, so its release is not the +
    /// button's.
    menu_start: bool,
}

impl Hotkeys {
    /// Select went down or up, on any controller.
    pub fn select(&mut self, down: bool) {
        self.select = down;
    }

    /// Start went down.
    pub fn start_down(&mut self, handheld: bool) -> Start {
        if handheld && self.select {
            self.menu_start = true;
            Start::Menu
        } else {
            Start::Plus
        }
    }

    /// Start went up. False when that Start opened the menu, so that its
    /// release does not act as the + button's.
    pub fn start_up(&mut self) -> bool {
        !std::mem::take(&mut self.menu_start)
    }
}

/// What the pause menu offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Close the menu; a paused program goes on.
    Resume,
    /// Run the program that the game list ran from the start.
    Restart,
    /// Stop the program and show the game list.
    List,
    Quit,
}

/// The menu that Select+Start opens, over a program or the game list.
pub struct PauseMenu {
    items: Vec<Choice>,
    focus: usize,
    /// A program runs, which the menu pauses.
    running: bool,
}

impl PauseMenu {
    /// The menu for a program that `running` or waits after its error
    /// (`program`), with Restart if `restart`.
    pub fn new(running: bool, program: bool, restart: bool) -> PauseMenu {
        let mut items = vec![Choice::Resume];
        if restart && program {
            items.push(Choice::Restart);
        }
        if program {
            items.push(Choice::List);
        }
        items.push(Choice::Quit);
        PauseMenu {
            items,
            focus: 0,
            running,
        }
    }

    /// Up and Down move, Enter chooses, Esc and Backspace close.
    pub fn key(&mut self, key: Keycode) -> Option<Choice> {
        match key {
            Keycode::Up => self.focus = self.focus.saturating_sub(1),
            Keycode::Down => self.focus = (self.focus + 1).min(self.items.len() - 1),
            Keycode::Return | Keycode::KpEnter => return Some(self.items[self.focus]),
            Keycode::Escape | Keycode::Backspace => return Some(Choice::Resume),
            _ => {}
        }
        None
    }

    /// A chooses and B closes, placed as SmileBASIC 4's are (SDL's East
    /// and South).
    pub fn pad(&mut self, button: Button) -> Option<Choice> {
        match button {
            Button::DPadUp => self.key(Keycode::Up),
            Button::DPadDown => self.key(Keycode::Down),
            Button::East => self.key(Keycode::Return),
            Button::South => self.key(Keycode::Escape),
            _ => None,
        }
    }
}

// The pause menu's layout and drawing, apart from what it offers and
// where the focus is: a new look replaces this block alone.

/// The pause menu's buttons are at least this wide.
const MENU_WIDTH: u32 = 280;
const MENU_HINT: &str = "A: choose  B: close";

impl PauseMenu {
    fn label(&self, choice: Choice) -> &'static str {
        match choice {
            Choice::Resume if self.running => "Resume",
            Choice::Resume => "Close",
            Choice::Restart => "Restart the program",
            Choice::List => "Back to the game list",
            Choice::Quit => "Quit the emulator",
        }
    }

    fn frame(&self, (w, h): (u32, u32)) -> Rect {
        let inner = (self.items.iter().map(|&c| text_width(self.label(c))))
            .chain([text_width(MENU_HINT)])
            .max()
            .unwrap_or(0)
            + 4 * PAD;
        let fw = (inner.max(MENU_WIDTH) + 4 * PAD).min(w);
        // Title bar, the buttons with gaps, the hint.
        let n = self.items.len() as u32;
        let fh = LINE + 2 * PAD + n * (LINE + PAD) + LINE + PAD;
        Rect {
            x: w.saturating_sub(fw) / 2,
            y: h.saturating_sub(fh) / 2,
            w: fw,
            h: fh,
        }
    }

    fn item(&self, f: Rect, i: usize) -> Rect {
        Rect {
            x: f.x + 2 * PAD,
            y: f.y + LINE + 2 * PAD + i as u32 * (LINE + PAD),
            w: f.w.saturating_sub(4 * PAD),
            h: LINE,
        }
    }

    /// Draws the menu centered on `fb`.
    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        let f = self.frame((fb.width(), fb.height()));
        let title = if self.running { "Paused" } else { "Menu" };
        window(fb, font, f, title);
        for (i, &choice) in self.items.iter().enumerate() {
            let r = self.item(f, i);
            fill(fb, r, if i == self.focus { HIGHLIGHT } else { BORDER });
            let label = self.label(choice);
            let x = r.x + r.w.saturating_sub(text_width(label)) / 2;
            draw_text(fb, font, x, r.y + 4, label, TEXT);
        }
        let y = f.y + f.h - LINE - PAD / 2 + 4;
        draw_text(fb, font, f.x + 2 * PAD, y, MENU_HINT, DISABLED);
    }
}

const MAIN: &str = "MAIN.PRG";

/// What the frontend should do after an input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    /// Run the TXT file `file` of `project`.
    Run {
        project: String,
        file: String,
    },
    /// Something to tell, as a notice.
    Notice(String),
}

/// Where the list was when a program ran from it, to come back to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Memory {
    project: Option<String>,
    program: Option<String>,
}

/// A project in the list.
struct Game {
    name: String,
    listed: Listed,
    has_main: bool,
}

/// A project's programs, shown in place of the projects.
struct Programs {
    game: usize,
    files: Vec<String>,
    selected: usize,
    scroll: usize,
}

/// The list of projects that handheld mode starts in and comes back to.
/// It runs a project's MAIN.PRG, or one of its programs, and manages no
/// files. This is what it lists and where the selection is; how it looks
/// is [`ListView`]'s.
pub struct GameList {
    size: (u32, u32),
    games: Vec<Game>,
    selected: usize,
    scroll: usize,
    programs: Option<Programs>,
    /// Why there is no list, if the projects could not be read.
    status: Option<String>,
    /// Where the selection is, as `3/12`.
    counter: String,
    view: ListView,
}

impl GameList {
    /// The projects of `storage`, in a UI of `size`, back where `memory`
    /// says. The workspace comes last, and only when it has programs.
    pub fn new(storage: &Storage, size: (u32, u32), memory: &Memory) -> GameList {
        let mut names = Vec::new();
        let mut status = None;
        match storage.projects() {
            Ok(projects) => names.extend(projects),
            Err(e) => status = Some(format!("Could not list the projects: {e}")),
        }
        if storage
            .list(Some(FileKind::Txt), Some(""))
            .is_ok_and(|files| !files.is_empty())
        {
            names.push(String::new());
        }
        let games: Vec<Game> = (names.into_iter())
            .map(|name| Game {
                listed: Listed::read(storage, &name),
                has_main: storage.exists_in(&name, FileKind::Txt, MAIN),
                name,
            })
            .collect();
        let mut list = GameList {
            size,
            view: ListView::new(size, &games, None),
            games,
            selected: 0,
            scroll: 0,
            programs: None,
            status,
            counter: String::new(),
        };
        let at =
            (memory.project.as_ref()).and_then(|p| list.games.iter().position(|g| g.name == *p));
        if let Some(i) = at {
            list.select(i);
            if let Some(program) = &memory.program {
                list.open_programs(storage);
                let found = (list.programs.as_ref())
                    .and_then(|p| p.files.iter().position(|f| f == program));
                list.select_program(found.unwrap_or(0));
            }
        }
        list.moved();
        list
    }

    /// Where the list is now, for [`GameList::new`] to come back to.
    pub fn memory(&self) -> Memory {
        Memory {
            project: self.games.get(self.selected).map(|g| g.name.clone()),
            program: (self.programs.as_ref()).and_then(|p| p.files.get(p.selected).cloned()),
        }
    }

    /// Which list it shows, so that a change can be told.
    pub fn mode_id(&self) -> usize {
        usize::from(self.programs.is_some())
    }

    /// Lays the list out for a UI of another size.
    pub fn set_size(&mut self, size: (u32, u32)) {
        if size != self.size {
            self.size = size;
            self.shown_changed();
        }
    }

    /// The size, or which list shows, changed: the view is made again, and
    /// the selection kept in sight.
    fn shown_changed(&mut self) {
        self.view = ListView::new(self.size, &self.games, self.programs.as_ref());
        self.select(self.selected);
        if let Some(p) = &self.programs {
            self.select_program(p.selected);
        }
    }

    /// The first row shown for `selected` to show, from `scroll`.
    fn scrolled(selected: usize, scroll: usize, rows: usize) -> usize {
        if selected < scroll {
            selected
        } else if selected >= scroll + rows {
            selected + 1 - rows
        } else {
            scroll
        }
    }

    fn select(&mut self, i: usize) {
        self.selected = i.min(self.games.len().saturating_sub(1));
        self.scroll = Self::scrolled(self.selected, self.scroll, self.view.rows());
    }

    fn select_program(&mut self, i: usize) {
        let rows = self.view.rows();
        if let Some(p) = &mut self.programs {
            p.selected = i.min(p.files.len().saturating_sub(1));
            p.scroll = Self::scrolled(p.selected, p.scroll, rows);
        }
    }

    /// The selection moved: what the counter says.
    fn moved(&mut self) {
        let (at, of) = match &self.programs {
            Some(p) => (p.selected, p.files.len()),
            None => (self.selected, self.games.len()),
        };
        self.counter = match of {
            0 => String::new(),
            n => format!("{}/{n}", at + 1),
        };
    }

    fn open_programs(&mut self, storage: &Storage) -> Outcome {
        let Some(game) = self.games.get(self.selected) else {
            return Outcome::Stay;
        };
        if !usable(&game.name) {
            return Outcome::Notice(format!(
                "Rename the folder {} to use it as a project",
                game.name
            ));
        }
        let files = match storage.list(Some(FileKind::Txt), Some(&game.name)) {
            Ok(files) => files.into_iter().map(|(_, name)| name).collect(),
            Err(e) => return Outcome::Notice(format!("Could not list the programs: {e}")),
        };
        self.programs = Some(Programs {
            game: self.selected,
            files,
            selected: 0,
            scroll: 0,
        });
        self.shown_changed();
        Outcome::Stay
    }

    /// A on a project: its MAIN.PRG, or else its programs to pick from.
    fn run_project(&mut self, storage: &Storage) -> Outcome {
        match self.games.get(self.selected) {
            Some(game) if game.has_main && usable(&game.name) => Outcome::Run {
                project: game.name.clone(),
                file: MAIN.to_string(),
            },
            Some(_) => self.open_programs(storage),
            None => Outcome::Stay,
        }
    }

    /// Up and Down move, Page Up and Page Down (L and R) a page, Home and
    /// End to the ends. Enter runs, Tab shows the project's programs or
    /// goes back, Esc and Backspace go back.
    pub fn key(&mut self, key: Keycode, storage: &Storage) -> Outcome {
        let page = self.view.rows() as isize;
        let step = match key {
            Keycode::Up => Some(-1),
            Keycode::Down => Some(1),
            Keycode::PageUp => Some(-page),
            Keycode::PageDown => Some(page),
            Keycode::Home => Some(isize::MIN / 2),
            Keycode::End => Some(isize::MAX / 2),
            _ => None,
        };
        let outcome = match (step, &self.programs) {
            (Some(step), Some(p)) => {
                self.select_program(p.selected.saturating_add_signed(step));
                Outcome::Stay
            }
            (Some(step), None) => {
                self.select(self.selected.saturating_add_signed(step));
                Outcome::Stay
            }
            (None, Some(p)) => match key {
                Keycode::Return | Keycode::KpEnter => match p.files.get(p.selected) {
                    Some(file) => Outcome::Run {
                        project: self.games[p.game].name.clone(),
                        file: file.clone(),
                    },
                    None => Outcome::Stay,
                },
                Keycode::Escape | Keycode::Backspace | Keycode::Tab => {
                    self.programs = None;
                    self.shown_changed();
                    Outcome::Stay
                }
                _ => Outcome::Stay,
            },
            (None, None) => match key {
                Keycode::Return | Keycode::KpEnter => self.run_project(storage),
                Keycode::Tab => self.open_programs(storage),
                _ => Outcome::Stay,
            },
        };
        self.moved();
        outcome
    }

    /// The D-pad moves (the left stick comes as the D-pad), L and R move
    /// a page, A runs, X shows the programs and B goes back.
    pub fn pad(&mut self, button: Button, storage: &Storage) -> Outcome {
        let key = match button {
            Button::DPadUp => Keycode::Up,
            Button::DPadDown => Keycode::Down,
            Button::LeftShoulder => Keycode::PageUp,
            Button::RightShoulder => Keycode::PageDown,
            Button::East => Keycode::Return,
            Button::South => Keycode::Escape,
            Button::North => Keycode::Tab,
            _ => return Outcome::Stay,
        };
        self.key(key, storage)
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        self.view.draw(fb, font, self);
    }
}

// The game list's layout and drawing, all of it, apart from what it lists
// and where the selection is: a new look replaces this part alone.

/// The game list's widest content, centered on a wider UI.
const MAX_WIDTH: u32 = 960;
/// The title line at the top.
const HEADER: u32 = LINE + PAD;
/// A project's row: its icon beside its title over its folder's name.
const ROW: u32 = 2 * LINE;
/// A program's row.
const PROGRAM_ROW: u32 = LINE + PAD;
/// The gap between a project's icon and its title.
const ICON_GAP: u32 = 6;
/// Lines of the selected project's description.
const ABOUT_LINES: usize = 3;
const ABOUT: u32 = ABOUT_LINES as u32 * LINE + PAD;
/// The hint line at the bottom.
const FOOTER: u32 = LINE;
/// Behind everything: the list covers the screens.
const BACKDROP: u32 = 0xFF0B_0C10;
const PANEL: u32 = 0xFF12_141A;
const PROJECTS_HINT: &str = "A: run  X: programs  L/R: page  Select+Start: menu";
const PROGRAMS_HINT: &str = "A: run  B: back  L/R: page  Select+Start: menu";

/// Where the game list's parts go in a UI of a given size, and its text
/// cut to fit there, made when the size or the list shown changes, so
/// that drawing a frame cuts nothing.
struct ListView {
    size: (u32, u32),
    /// What the list takes: all of the UI, up to its widest.
    area: Rect,
    list: Rect,
    about: Rect,
    row_h: u32,
    /// Each project's title, folder and description lines.
    titles: Vec<String>,
    folders: Vec<String>,
    abouts: Vec<Vec<String>>,
    /// The programs shown, and the heading over them.
    labels: Vec<String>,
    heading: String,
    hint: String,
}

impl ListView {
    fn new(size: (u32, u32), games: &[Game], programs: Option<&Programs>) -> ListView {
        let w = size.0.min(MAX_WIDTH);
        let area = Rect {
            x: (size.0 - w) / 2,
            y: 0,
            w,
            h: size.1,
        };
        let list = Rect {
            x: area.x + PAD,
            y: HEADER,
            w: area.w.saturating_sub(2 * PAD),
            h: (area.h.saturating_sub(HEADER + ABOUT + FOOTER + PAD)).max(ROW),
        };
        let about = Rect {
            y: list.y + list.h + PAD / 2,
            h: ABOUT - PAD,
            ..list
        };
        let room = list.w.saturating_sub(Self::title_x(list) - list.x + PAD);
        let about_w = about.w.saturating_sub(2 * PAD).max(CHAR);
        let folder = |name: &str| match usable(name) {
            true => fit(project_label(name), room),
            false => fit(&format!("{} (rename to use)", project_label(name)), room),
        };
        let (labels, heading, hint) = match programs {
            Some(p) => (
                (p.files.iter())
                    .map(|f| fit(f, list.w.saturating_sub(2 * PAD)))
                    .collect(),
                fit(&games[p.game].listed.title, w.saturating_sub(12 * CHAR)),
                PROGRAMS_HINT,
            ),
            None => (Vec::new(), "Games".to_string(), PROJECTS_HINT),
        };
        ListView {
            size,
            area,
            list,
            about,
            row_h: if programs.is_some() { PROGRAM_ROW } else { ROW },
            titles: games.iter().map(|g| fit(&g.listed.title, room)).collect(),
            folders: games.iter().map(|g| folder(&g.name)).collect(),
            abouts: (games.iter())
                .map(|g| about_lines(&g.listed.description, about_w))
                .collect(),
            labels,
            heading,
            hint: fit(hint, w.saturating_sub(2 * PAD)),
        }
    }

    /// Rows the list shows at once, at least one.
    fn rows(&self) -> usize {
        ((self.list.h / self.row_h) as usize).max(1)
    }

    fn row(&self, shown: usize) -> Rect {
        Rect {
            y: self.list.y + shown as u32 * self.row_h,
            h: self.row_h,
            ..self.list
        }
    }

    fn title_x(row: Rect) -> u32 {
        row.x + PAD + ICON_SIZE as u32 + ICON_GAP
    }

    fn draw(&self, fb: &mut Framebuffer, font: &Font, state: &GameList) {
        let all = Rect {
            x: 0,
            y: 0,
            w: self.size.0,
            h: self.size.1,
        };
        fill(fb, all, BACKDROP);
        let a = self.area;
        draw_text(fb, font, a.x + PAD, PAD, &self.heading, TEXT);
        let counter = &state.counter;
        let counter_x = (a.x + a.w).saturating_sub(PAD + text_width(counter));
        draw_text(fb, font, counter_x, PAD, counter, DISABLED);
        fill(fb, self.list, PANEL);
        match &state.programs {
            Some(p) => self.draw_programs(fb, font, p),
            None => self.draw_projects(fb, font, state),
        }
        outline(fb, self.list, BORDER);
        if let Some(lines) = self.abouts.get(state.selected) {
            for (i, line) in lines.iter().enumerate() {
                let y = self.about.y + i as u32 * LINE + (LINE - CHAR) / 2;
                draw_text(fb, font, self.about.x + PAD, y, line, TEXT);
            }
        }
        let y = self.size.1.saturating_sub(FOOTER) + (LINE - CHAR) / 2;
        draw_text(fb, font, a.x + PAD, y, &self.hint, DISABLED);
    }

    /// A line in the middle of the list, when it has nothing to show.
    fn draw_empty(&self, fb: &mut Framebuffer, font: &Font, text: &str, color: u32) {
        let list = self.list;
        let text = fit(text, list.w.saturating_sub(2 * PAD));
        let x = list.x + list.w.saturating_sub(text_width(&text)) / 2;
        draw_text(fb, font, x, list.y + list.h / 2 - CHAR / 2, &text, color);
    }

    fn draw_projects(&self, fb: &mut Framebuffer, font: &Font, state: &GameList) {
        if state.games.is_empty() {
            match &state.status {
                Some(status) => self.draw_empty(fb, font, status, ERROR),
                None => self.draw_empty(fb, font, "No projects found.", DISABLED),
            }
            return;
        }
        let end = state.games.len().min(state.scroll + self.rows());
        for (shown, i) in (state.scroll..end).enumerate() {
            let r = self.row(shown);
            let game = &state.games[i];
            if i == state.selected {
                fill(fb, r, HIGHLIGHT);
            }
            let side = ICON_SIZE as u32;
            if let Some(icon) = &game.listed.icon {
                image(
                    fb,
                    r.x + PAD,
                    r.y + (ROW - side) / 2,
                    side,
                    &icon.pixels,
                    icon.key,
                );
            }
            let x = Self::title_x(r);
            draw_text(fb, font, x, r.y + 6, &self.titles[i], TEXT);
            let color = if usable(&game.name) { FOLDER } else { ERROR };
            draw_text(fb, font, x, r.y + 26, &self.folders[i], color);
        }
    }

    fn draw_programs(&self, fb: &mut Framebuffer, font: &Font, p: &Programs) {
        if p.files.is_empty() {
            let text = "This project has no programs.";
            self.draw_empty(fb, font, text, DISABLED);
            return;
        }
        let end = p.files.len().min(p.scroll + self.rows());
        for (shown, i) in (p.scroll..end).enumerate() {
            let r = self.row(shown);
            if i == p.selected {
                fill(fb, r, HIGHLIGHT);
            }
            let y = r.y + (PROGRAM_ROW - CHAR) / 2;
            draw_text(fb, font, r.x + PAD, y, &self.labels[i], TEXT);
        }
    }
}

/// A description's first lines, wrapped to `width`; the last shown ends
/// in `~` when there is more.
fn about_lines(description: &str, width: u32) -> Vec<String> {
    let mut lines = (description.lines())
        .flat_map(|line| wrap(line, width))
        .filter(|line| !line.is_empty());
    let mut shown: Vec<String> = lines.by_ref().take(ABOUT_LINES).collect();
    if lines.next().is_some()
        && let Some(last) = shown.last_mut()
    {
        last.push('~');
        *last = fit(last, width);
    }
    shown
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
                std::env::temp_dir().join(format!("sb-handheld-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Projects named `GAME0` and up, with a MAIN.PRG in the even ones
    /// and two other programs in each.
    fn storage(tmp: &TempDir, projects: usize) -> Storage {
        let s = Storage::new(tmp.0.clone());
        for i in 0..projects {
            let name = format!("GAME{i:02}");
            s.create_project(&name).unwrap();
            let mut files = vec!["EDIT.PRG", "LEVELS.PRG"];
            if i % 2 == 0 {
                files.push(MAIN);
            }
            for file in files {
                let path = tmp.0.join(&name).join(format!("T{file}"));
                std::fs::write(path, "?1\n").unwrap();
            }
        }
        s
    }

    #[test]
    fn the_ui_keeps_whole_pixels() {
        assert_eq!(ui_size((640, 480)), (640, 480));
        assert_eq!(ui_size((1920, 1080)), (960, 540));
        assert_eq!(ui_size((1280, 720)), (1280, 720));
        assert_eq!(ui_size((2560, 1440)), (853, 480));
        // Smaller than the line count, a UI pixel is still a screen's.
        assert_eq!(ui_size((320, 240)), (320, 240));
        let box_of = |r: FRect| (r.x, r.y, r.w, r.h);
        assert_eq!(
            box_of(display_rect((640, 480), (640, 480))),
            (0.0, 0.0, 640.0, 480.0)
        );
        assert_eq!(
            box_of(display_rect((960, 540), (1920, 1080))),
            (0.0, 0.0, 1920.0, 1080.0)
        );
        // An odd output loses a pixel's part rather than its whole scale.
        let r = display_rect(ui_size((2560, 1441)), (2560, 1441));
        assert_eq!(box_of(r), (0.5, 0.0, 2559.0, 1440.0));
    }

    #[test]
    fn the_sharp_filter_fills_a_handheld_screen() {
        let sharp = Some(Filter::Nearest);
        assert_eq!(shown_filter(sharp, false), sharp);
        assert_eq!(shown_filter(sharp, true), Some(Filter::SmartNearest));
        for other in [None, Some(Filter::Bilinear), Some(Filter::SmartNearest)] {
            assert_eq!(shown_filter(other, true), other);
        }
    }

    #[test]
    fn select_and_start_open_the_menu_and_start_alone_is_plus() {
        let mut keys = Hotkeys::default();
        assert_eq!(keys.start_down(true), Start::Plus);
        assert!(keys.start_up(), "its release is the + button's");
        keys.select(true);
        assert_eq!(keys.start_down(true), Start::Menu);
        // Its release does nothing, even with Select let go first.
        keys.select(false);
        assert!(!keys.start_up());
        assert_eq!(keys.start_down(true), Start::Plus);
        assert!(keys.start_up());
        // On the desktop, Select changes nothing.
        keys.select(true);
        assert_eq!(keys.start_down(false), Start::Plus);
        assert!(keys.start_up());
    }

    #[test]
    fn the_pause_menu_offers_what_applies() {
        let running = PauseMenu::new(true, true, true);
        let all = [Choice::Resume, Choice::Restart, Choice::List, Choice::Quit];
        assert_eq!(running.items, all);
        let unknown = PauseMenu::new(true, true, false);
        assert_eq!(unknown.items, [Choice::Resume, Choice::List, Choice::Quit]);
        let list = PauseMenu::new(false, false, true);
        assert_eq!(list.items, [Choice::Resume, Choice::Quit]);
        assert_eq!(list.label(Choice::Resume), "Close");
    }

    #[test]
    fn the_pause_menu_answers_the_pad_and_keys() {
        let mut menu = PauseMenu::new(true, true, true);
        assert_eq!(menu.pad(Button::DPadUp), None);
        assert_eq!(menu.pad(Button::East), Some(Choice::Resume));
        menu.pad(Button::DPadDown);
        assert_eq!(menu.pad(Button::East), Some(Choice::Restart));
        for _ in 0..5 {
            menu.pad(Button::DPadDown);
        }
        assert_eq!(menu.key(Keycode::Return), Some(Choice::Quit));
        // B and Esc close, wherever the focus is.
        assert_eq!(menu.pad(Button::South), Some(Choice::Resume));
        assert_eq!(menu.key(Keycode::Escape), Some(Choice::Resume));
        assert_eq!(menu.pad(Button::North), None);
    }

    #[test]
    fn the_pause_menu_fits_a_small_screen() {
        let menu = PauseMenu::new(true, true, true);
        let f = menu.frame((640, 480));
        assert!(f.x + f.w <= 640 && f.y + f.h <= 480, "{f:?}");
        let mut fb = Framebuffer::new(640, 480);
        menu.draw(&mut fb, &Font::blank());
        let last = menu.item(f, 3);
        assert!(
            last.y + last.h + LINE <= f.y + f.h,
            "the hint below the buttons"
        );
    }

    fn run(project: &str, file: &str) -> Outcome {
        Outcome::Run {
            project: project.to_string(),
            file: file.to_string(),
        }
    }

    #[test]
    fn a_runs_main_or_shows_the_programs() {
        let tmp = TempDir::new("run");
        let s = storage(&tmp, 3);
        let mut list = GameList::new(&s, (640, 480), &Memory::default());
        assert_eq!(list.games.len(), 3, "the workspace has no programs");
        assert_eq!(list.pad(Button::East, &s), run("GAME00", MAIN));
        assert_eq!(list.mode_id(), 0);
        // No MAIN.PRG: its programs.
        list.pad(Button::DPadDown, &s);
        assert_eq!(list.pad(Button::East, &s), Outcome::Stay);
        assert_eq!(list.mode_id(), 1);
        assert_eq!(list.counter, "1/2");
        list.pad(Button::DPadDown, &s);
        assert_eq!(list.pad(Button::East, &s), run("GAME01", "LEVELS.PRG"));
        // B goes back to the projects, where it was.
        assert_eq!(list.pad(Button::South, &s), Outcome::Stay);
        assert_eq!((list.mode_id(), list.selected), (0, 1));
        assert_eq!(list.pad(Button::South, &s), Outcome::Stay);
        assert_eq!(list.mode_id(), 0);
        // X shows the programs of one with a MAIN.PRG too.
        list.pad(Button::DPadUp, &s);
        list.pad(Button::North, &s);
        assert_eq!(list.programs.as_ref().unwrap().files.len(), 3);
        assert_eq!(list.key(Keycode::Tab, &s), Outcome::Stay);
        assert_eq!(list.mode_id(), 0);
    }

    #[test]
    fn l_and_r_move_a_page_and_the_list_scrolls() {
        let tmp = TempDir::new("page");
        let s = storage(&tmp, 20);
        let mut list = GameList::new(&s, (640, 480), &Memory::default());
        let rows = list.view.rows();
        assert_eq!(rows, 7, "rows at 640x480");
        list.pad(Button::RightShoulder, &s);
        assert_eq!(list.selected, rows);
        assert_eq!(list.scroll, 1, "the selection stays shown");
        list.pad(Button::RightShoulder, &s);
        list.pad(Button::RightShoulder, &s);
        assert_eq!(list.selected, 19, "no further than the last");
        assert_eq!(list.counter, "20/20");
        list.pad(Button::DPadDown, &s);
        assert_eq!(list.selected, 19);
        list.pad(Button::LeftShoulder, &s);
        assert_eq!(list.selected, 19 - rows);
        list.key(Keycode::Home, &s);
        assert_eq!((list.selected, list.scroll), (0, 0));
        list.pad(Button::DPadUp, &s);
        assert_eq!(list.selected, 0);
        // More rows on a taller UI, and the selection still shows.
        list.key(Keycode::End, &s);
        list.set_size((1280, 720));
        assert!(list.view.rows() > rows);
        assert!(list.scroll <= 19 && 19 < list.scroll + list.view.rows());
    }

    #[test]
    fn the_list_comes_back_where_it_ran_from() {
        let tmp = TempDir::new("memory");
        let s = storage(&tmp, 4);
        let mut list = GameList::new(&s, (640, 480), &Memory::default());
        list.pad(Button::DPadDown, &s);
        list.pad(Button::DPadDown, &s);
        let memory = list.memory();
        let back = GameList::new(&s, (640, 480), &memory);
        assert_eq!((back.selected, back.mode_id()), (2, 0));
        list.pad(Button::North, &s);
        list.pad(Button::DPadDown, &s);
        let memory = list.memory();
        let back = GameList::new(&s, (640, 480), &memory);
        assert_eq!(back.mode_id(), 1);
        assert_eq!(back.programs.as_ref().unwrap().selected, 1);
        assert_eq!(back.counter, "2/3");
        // A project gone since starts at the top.
        std::fs::remove_dir_all(tmp.0.join("GAME02")).unwrap();
        let back = GameList::new(&s, (640, 480), &memory);
        assert_eq!((back.selected, back.mode_id()), (0, 0));
    }

    #[test]
    fn titles_descriptions_and_icons_fit_a_small_screen() {
        let tmp = TempDir::new("meta");
        let s = storage(&tmp, 1);
        let mut icon = vec![0; ICON_SIZE * ICON_SIZE];
        icon[0] = 0xFF12_3456_u32 as i32;
        let long = "A long description of a game that goes on and on. ".repeat(6);
        let meta = sb_runtime::env::Meta {
            // As long as a title gets, in full-width characters.
            title: "宇宙".repeat(12).encode_utf16().collect(),
            description: long.encode_utf16().collect(),
            icon,
            ..Default::default()
        };
        for (name, bytes) in sb_runtime::env::meta_files(&meta).unwrap() {
            s.write_file_in("GAME00", name, &bytes).unwrap();
        }
        let list = GameList::new(&s, (320, 240), &Memory::default());
        let view = &list.view;
        assert!(view.titles[0].ends_with('~'), "{}", view.titles[0]);
        let about = &view.abouts[0];
        assert_eq!(about.len(), ABOUT_LINES);
        assert!(about[2].ends_with('~'), "{}", about[2]);
        assert!(
            about
                .iter()
                .all(|l| text_width(l) <= view.about.w - 2 * PAD)
        );
        assert!(view.about.y + view.about.h + FOOTER <= 240);
        let mut fb = Framebuffer::new(320, 240);
        list.draw(&mut fb, &Font::blank());
        let r = view.row(0);
        let at = (r.x + PAD, r.y + (ROW - ICON_SIZE as u32) / 2);
        assert_eq!(fb.get(at.0, at.1).map(|p| p & 0xFF_FFFF), Some(0x12_3456));
    }

    #[test]
    fn the_workspace_comes_last_with_programs_and_bad_names_do_not_run() {
        let tmp = TempDir::new("workspace");
        let s = storage(&tmp, 1);
        std::fs::create_dir_all(tmp.0.join("@WORKSPACE")).unwrap();
        std::fs::write(tmp.0.join("@WORKSPACE").join("TMAIN.PRG"), "?1\n").unwrap();
        std::fs::create_dir_all(tmp.0.join("bad name")).unwrap();
        let mut list = GameList::new(&s, (640, 480), &Memory::default());
        let names: Vec<&str> = list.games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names.last(), Some(&""), "{names:?}");
        assert_eq!(list.view.folders.last().unwrap(), "[WORKSPACE]");
        list.key(Keycode::End, &s);
        assert_eq!(list.key(Keycode::Return, &s), run("", MAIN));
        if let Some(bad) = list.games.iter().position(|g| !usable(&g.name)) {
            list.select(bad);
            assert!(matches!(list.key(Keycode::Return, &s), Outcome::Notice(_)));
            assert_eq!(list.mode_id(), 0);
        }
    }

    #[test]
    fn an_empty_list_says_so() {
        let tmp = TempDir::new("empty");
        let s = Storage::new(tmp.0.clone());
        let mut list = GameList::new(&s, (640, 480), &Memory::default());
        assert!(list.games.is_empty());
        assert_eq!(list.pad(Button::East, &s), Outcome::Stay);
        assert_eq!(list.pad(Button::DPadDown, &s), Outcome::Stay);
        assert_eq!(list.counter, "");
        let mut fb = Framebuffer::new(640, 480);
        list.draw(&mut fb, &Font::blank());
    }
}

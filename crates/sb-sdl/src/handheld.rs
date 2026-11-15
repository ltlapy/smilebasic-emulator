//! Handheld mode: the frontend laid out for a small screen and a gamepad
//! alone. The UI is as many pixels as the screen at a whole scale, the
//! emulator starts in a list of the projects instead of DIRECT mode,
//! Select (SDL's Back, the − button) with A opens a pause menu, and Select
//! with Start quits. Programs never see Select: BUTTON has no bit for it.

use sb_runtime::Framebuffer;
use sb_runtime::confirm::project_label;
use sb_runtime::env::ICON_SIZE;
use sb_runtime::font::Font;
use sb_runtime::screen::{Filter, ScreenMode};
use sb_runtime::settings::HandheldFilter;
use sb_runtime::storage::{FileKind, Storage};
use sdl3::gamepad::Button;
use sdl3::keyboard::Keycode;
use sdl3::render::FRect;

use crate::keyhint::{BUTTON_TEXT, draw_arrow};
use crate::program_dialog::wrap;
use crate::projects::{ERROR, FOLDER, Listed, PANEL, SEPARATOR, WELL, fit, usable};
use crate::ui::{
    BACKGROUND, CHAR, DISABLED, HIGHLIGHT, LINE, PAD, Rect, TEXT, draw_text, draw_text_scaled,
    fill, image_scaled, text_width,
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

/// The filter handheld mode scales the screen with, by `choice`, when the
/// frame of `mode` is drawn `shown` pixels large. The screen fits the
/// display at its aspect ratio whatever the filter, so it is seldom a
/// whole number of times its size, and it can be smaller than its frame:
/// XSCREEN goes up to 1280x720.
pub fn screen_filter(choice: HandheldFilter, mode: &ScreenMode, (w, h): (f32, f32)) -> Filter {
    match choice {
        HandheldFilter::Auto => {
            let (fw, fh) = mode.frame();
            let (sx, sy) = (w / fw.max(1) as f32, h / fh.max(1) as f32);
            let whole = |s: f32| s >= 1.0 && (s - s.round()).abs() < 0.001;
            if whole(sx) && whole(sy) {
                Filter::Nearest
            } else if sx >= 1.0 && sy >= 1.0 {
                Filter::SmartNearest
            } else {
                Filter::Bilinear
            }
        }
        HandheldFilter::PixelArt => Filter::SmartNearest,
        HandheldFilter::Bilinear => Filter::Bilinear,
        HandheldFilter::Nearest => Filter::Nearest,
        HandheldFilter::Xscreen => mode.filter,
    }
}

/// What a press of Start is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// With Select held, in handheld mode: quit the emulator, as the
    /// frontends of such handhelds do.
    Quit,
    /// The + button, as ever.
    Plus,
}

/// Select as the modifier of handheld mode's hotkeys.
#[derive(Debug, Default)]
pub struct Hotkeys {
    select: bool,
    /// The Start held now went down with Select, so its release is not
    /// the + button's.
    select_start: bool,
}

impl Hotkeys {
    /// Select went down or up, on any controller.
    pub fn select(&mut self, down: bool) {
        self.select = down;
    }

    /// A went down: true when, with Select held in handheld mode, it opens
    /// or closes the pause menu and is no press of A.
    pub fn a_down(&self, handheld: bool) -> bool {
        handheld && self.select
    }

    /// Start went down.
    pub fn start_down(&mut self, handheld: bool) -> Start {
        if handheld && self.select {
            self.select_start = true;
            Start::Quit
        } else {
            Start::Plus
        }
    }

    /// Start went up. False when that Start went down with Select, so
    /// that its release does not act as the + button's.
    pub fn start_up(&mut self) -> bool {
        !std::mem::take(&mut self.select_start)
    }
}

/// What the pause menu offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Close the menu; a paused program goes on.
    Resume,
    /// Run the program that the game list ran from the start.
    Restart,
    /// The screen filter changed to [`PauseMenu::filter`]; the menu stays
    /// open.
    Filter,
    /// Stop the program and show the game list.
    List,
    Quit,
}

/// The menu that Select+A opens, over a program or the game list.
pub struct PauseMenu {
    items: Vec<Choice>,
    focus: usize,
    /// A program runs, which the menu pauses.
    running: bool,
    filter: HandheldFilter,
}

impl PauseMenu {
    /// The menu for a program that `running` or waits after its error
    /// (`program`), with Restart if `restart`, showing `filter` as the
    /// screen filter.
    pub fn new(running: bool, program: bool, restart: bool, filter: HandheldFilter) -> PauseMenu {
        let mut items = vec![Choice::Resume];
        if restart && program {
            items.push(Choice::Restart);
        }
        items.push(Choice::Filter);
        if program {
            items.push(Choice::List);
        }
        items.push(Choice::Quit);
        PauseMenu {
            items,
            focus: 0,
            running,
            filter,
        }
    }

    /// The screen filter it shows.
    pub fn filter(&self) -> HandheldFilter {
        self.filter
    }

    /// Up and Down move, Enter chooses, Esc and Backspace close. On the
    /// screen filter, Left and Right change it, and Enter steps it on.
    pub fn key(&mut self, key: Keycode) -> Option<Choice> {
        let on_filter = self.items[self.focus] == Choice::Filter;
        let step = match key {
            Keycode::Up => {
                self.focus = self.focus.saturating_sub(1);
                return None;
            }
            Keycode::Down => {
                self.focus = (self.focus + 1).min(self.items.len() - 1);
                return None;
            }
            Keycode::Escape | Keycode::Backspace => return Some(Choice::Resume),
            Keycode::Left if on_filter => -1,
            Keycode::Right | Keycode::Return | Keycode::KpEnter if on_filter => 1,
            Keycode::Return | Keycode::KpEnter => return Some(self.items[self.focus]),
            _ => return None,
        };
        self.filter = self.filter.step(step);
        Some(Choice::Filter)
    }

    /// A chooses and B closes, placed as SmileBASIC 4's are (SDL's East
    /// and South).
    pub fn pad(&mut self, button: Button) -> Option<Choice> {
        match button {
            Button::DPadUp => self.key(Keycode::Up),
            Button::DPadDown => self.key(Keycode::Down),
            Button::DPadLeft => self.key(Keycode::Left),
            Button::DPadRight => self.key(Keycode::Right),
            Button::East => self.key(Keycode::Return),
            Button::South => self.key(Keycode::Escape),
            _ => None,
        }
    }
}

// The pause menu's layout and drawing, apart from what it offers and
// where the focus is: a new look replaces this block alone.

/// The pause menu is at least this wide.
const MENU_WIDTH: u32 = 256;
/// The dim heading over the items.
const MENU_HEADING: u32 = 32;
const FILTER_LABEL: &str = "Screen filter:";
/// The room for an arrow on either side of the filter's value.
const ARROW: u32 = 16;
const MENU_HINTS: [Hint; 3] = [
    (&[Cap::Name("A")], "Choose"),
    (&[Cap::Name("B")], "Resume"),
    (&[Cap::Left, Cap::Right], "Filter"),
];
/// The same where nothing is paused.
const CLOSE_HINTS: [Hint; 3] = [
    (&[Cap::Name("A")], "Choose"),
    (&[Cap::Name("B")], "Close"),
    (&[Cap::Left, Cap::Right], "Filter"),
];

/// A filter's name in the menu.
fn filter_name(filter: HandheldFilter) -> &'static str {
    match filter {
        HandheldFilter::Auto => "Auto",
        HandheldFilter::PixelArt => "Pixel art",
        HandheldFilter::Bilinear => "Smooth",
        HandheldFilter::Nearest => "Sharp",
        HandheldFilter::Xscreen => "Program",
    }
}

/// The value's place: the widest filter name and a gap either side, so
/// that the arrows stay put.
fn filter_name_width() -> u32 {
    let widest = HandheldFilter::ALL
        .iter()
        .map(|&f| text_width(filter_name(f)));
    widest.max().unwrap_or(0) + PAD
}

impl PauseMenu {
    fn label(&self, choice: Choice) -> &'static str {
        match choice {
            Choice::Resume if self.running => "Resume",
            Choice::Resume => "Close",
            Choice::Restart => "Restart",
            Choice::Filter => FILTER_LABEL,
            Choice::List => "Return to Game list",
            Choice::Quit => "Quit Emulator",
        }
    }

    fn frame(&self, (w, h): (u32, u32)) -> Rect {
        let filter_row = text_width(FILTER_LABEL) + PAD + 2 * ARROW + filter_name_width();
        let widest = (self.items.iter().map(|&c| text_width(self.label(c))))
            .chain([filter_row])
            .max()
            .unwrap_or(0);
        let fw = (widest + 2 * PAD).max(MENU_WIDTH).min(w);
        let fh = PAD + MENU_HEADING + self.items.len() as u32 * LINE + PAD;
        Rect {
            x: w.saturating_sub(fw) / 2,
            y: h.saturating_sub(fh) / 2,
            w: fw,
            h: fh,
        }
    }

    fn item(&self, f: Rect, i: usize) -> Rect {
        Rect {
            x: f.x + 1,
            y: f.y + PAD + MENU_HEADING + i as u32 * LINE,
            w: f.w.saturating_sub(2),
            h: LINE,
        }
    }

    /// Where the filter's value and its arrows go in its row `r`: the
    /// left arrow, the value's place, the right arrow.
    fn value_places(r: Rect) -> (Rect, Rect, Rect) {
        let slot = filter_name_width();
        let right = (r.x + r.w).saturating_sub(PAD);
        let arrow = |x| Rect {
            x,
            y: r.y + (LINE - ARROW) / 2,
            w: ARROW,
            h: ARROW,
        };
        let left = arrow(right.saturating_sub(2 * ARROW + slot));
        let value = Rect {
            x: left.x + ARROW,
            w: slot,
            ..r
        };
        (left, value, arrow(right.saturating_sub(ARROW)))
    }

    /// Draws the menu centered on `fb`, with its hints at the bottom.
    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        let size = (fb.width(), fb.height());
        let f = self.frame(size);
        let shadow = Rect {
            x: f.x.saturating_sub(1),
            y: f.y.saturating_sub(1),
            w: f.w + 2,
            h: f.h + 2,
        };
        fill(fb, shadow, 0xFF00_0000);
        fill(fb, f, BACKDROP);
        border(fb, f, DISABLED);
        let heading = if self.running { "Paused" } else { "Menu" };
        draw_text(fb, font, f.x + PAD, f.y + PAD + 4, heading, DISABLED);
        for (i, &choice) in self.items.iter().enumerate() {
            let r = self.item(f, i);
            let focused = i == self.focus;
            if focused {
                fill(fb, r, HIGHLIGHT);
            }
            let y = r.y + (LINE - CHAR) / 2;
            draw_text(fb, font, f.x + PAD, y, self.label(choice), TEXT);
            if choice == Choice::Filter {
                let (left, value, right) = Self::value_places(r);
                let arrows = if focused { TEXT } else { DISABLED };
                draw_arrow(fb, left, (-1, 0), arrows);
                draw_arrow(fb, right, (1, 0), arrows);
                let name = filter_name(self.filter);
                let x = value.x + value.w.saturating_sub(text_width(name)) / 2;
                draw_text(fb, font, x, y, name, TEXT);
            }
        }
        let hints = if self.running {
            &MENU_HINTS
        } else {
            &CLOSE_HINTS
        };
        draw_hint_bar(fb, font, size, hints);
    }
}

// The bars at the top and the bottom of handheld mode's screens, which
// the game list and the pause menu share.

/// What a hint shows for a button: its name, or an arrow for the D-pad.
#[derive(Clone, Copy)]
enum Cap {
    Name(&'static str),
    Left,
    Right,
}

/// Buttons, side by side, and what they do.
type Hint = (&'static [Cap], &'static str);

/// A cap is this high, inside a bar a [`LINE`] high.
const CAP_HEIGHT: u32 = 20;
/// Between a hint's caps, and after its last cap.
const CAP_GAP: u32 = 2;
/// Between hints.
const HINT_GAP: u32 = 16;
/// Behind everything: handheld mode's screens cover the program's.
const BACKDROP: u32 = BACKGROUND | 0xFF00_0000;

fn cap_width(cap: Cap) -> u32 {
    match cap {
        Cap::Name(name) => (text_width(name) + 10).max(CAP_HEIGHT),
        Cap::Left | Cap::Right => CAP_HEIGHT,
    }
}

fn hint_width((caps, label): Hint) -> u32 {
    let caps: u32 = caps.iter().map(|&c| cap_width(c) + CAP_GAP).sum();
    caps + CAP_GAP + text_width(label)
}

/// How many of `hints` fit, in order, across a bar `w` wide.
fn hints_shown(w: u32, hints: &[Hint]) -> usize {
    let mut x = PAD;
    let fits = |&hint: &&Hint| {
        x += hint_width(*hint);
        let fits = x + PAD <= w;
        x += HINT_GAP;
        fits
    };
    hints.iter().take_while(fits).count()
}

/// A bar across the UI from `y`, `LINE` high, with a line at `line_y`
/// on the edge that faces the rest.
fn bar(fb: &mut Framebuffer, w: u32, y: u32, line_y: u32) {
    let r = Rect {
        x: 0,
        y,
        w,
        h: LINE,
    };
    fill(fb, r, PANEL);
    fill(
        fb,
        Rect {
            y: line_y,
            h: 1,
            ..r
        },
        SEPARATOR,
    );
}

/// The bar at the top: what shows, and where the selection is in it.
fn draw_top_bar(fb: &mut Framebuffer, font: &Font, w: u32, title: &str, counter: &str) {
    bar(fb, w, 0, LINE - 1);
    let y = (LINE - CHAR) / 2;
    draw_text(fb, font, PAD, y, title, TEXT);
    let x = w.saturating_sub(PAD + text_width(counter));
    draw_text(fb, font, x, y, counter, DISABLED);
}

/// The bar at the bottom of a UI of `size`, with as many of `hints` as
/// fit, in order.
fn draw_hint_bar(fb: &mut Framebuffer, font: &Font, (w, h): (u32, u32), hints: &[Hint]) {
    let top = h.saturating_sub(LINE);
    bar(fb, w, top, top);
    let mut x = PAD;
    for &(caps, label) in &hints[..hints_shown(w, hints)] {
        for &cap in caps {
            let r = Rect {
                x,
                y: top + (LINE - CAP_HEIGHT) / 2,
                w: cap_width(cap),
                h: CAP_HEIGHT,
            };
            draw_cap(fb, font, r, cap);
            x += r.w + CAP_GAP;
        }
        x += CAP_GAP;
        draw_text(fb, font, x, top + (LINE - CHAR) / 2, label, TEXT);
        x += text_width(label) + HINT_GAP;
    }
}

/// A button's name in a rounded outline, as on the controller.
fn draw_cap(fb: &mut Framebuffer, font: &Font, r: Rect, cap: Cap) {
    pill(fb, r, BUTTON_TEXT, PANEL);
    let arrow = Rect {
        x: r.x + 2,
        y: r.y + 2,
        w: r.w.saturating_sub(4),
        h: r.h.saturating_sub(4),
    };
    match cap {
        Cap::Name(name) => {
            let x = r.x + r.w.saturating_sub(text_width(name)) / 2;
            let y = r.y + r.h.saturating_sub(CHAR) / 2;
            draw_text(fb, font, x, y, name, BUTTON_TEXT);
        }
        Cap::Left => draw_arrow(fb, arrow, (-1, 0), BUTTON_TEXT),
        Cap::Right => draw_arrow(fb, arrow, (1, 0), BUTTON_TEXT),
    }
}

/// A one-pixel outline in `color` round `r`, with ends as round as `r`
/// is high, filled with `inside`.
fn pill(fb: &mut Framebuffer, r: Rect, color: u32, inside: u32) {
    if crate::ui::dry(fb, r, ("pill", r, color, inside)) {
        return;
    }
    // Where a row of the shape `r` starts and ends across.
    let span = |r: Rect, y: u32| {
        let radius = r.h as f32 / 2.0;
        let dy = (radius - (y as f32 + 0.5)).abs();
        let cut = radius - (radius * radius - dy * dy).max(0.0).sqrt();
        let cut = (cut.round() as u32).min(r.w / 2);
        (r.x + cut, r.x + r.w - cut)
    };
    let inner = Rect {
        x: r.x + 1,
        y: r.y + 1,
        w: r.w.saturating_sub(2),
        h: r.h.saturating_sub(2),
    };
    for (shape, c) in [(r, color), (inner, inside)] {
        for y in 0..shape.h {
            let (x0, x1) = span(shape, y);
            for x in x0..x1 {
                fb.set(x, shape.y + y, c);
            }
        }
    }
}

/// A one-pixel frame just inside `r`.
fn border(fb: &mut Framebuffer, r: Rect, color: u32) {
    let right = (r.x + r.w).saturating_sub(1);
    let bottom = (r.y + r.h).saturating_sub(1);
    for side in [
        Rect { h: 1, ..r },
        Rect {
            y: bottom,
            h: 1,
            ..r
        },
        Rect { w: 1, ..r },
        Rect {
            x: right,
            w: 1,
            ..r
        },
    ] {
        fill(fb, side, color);
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

/// The rows take this many twentieths of the UI's width, beside the
/// selected project's info.
const LIST_SHARE: u32 = 9;
/// Where the rows start, below the top bar.
const BODY_TOP: u32 = LINE + PAD;
/// The scroll thumb's width, in the gap right of the rows.
const THUMB: u32 = 4;
/// The selected project's icon, at twice its size.
const ICON_SCALE: u32 = 2;
const BIG_ICON: u32 = ICON_SCALE * ICON_SIZE as u32;
/// Lines of the selected project's title.
const TITLE_LINES: usize = 2;
/// Between its folder's name and its description.
const ABOUT_GAP: u32 = 12;
/// What marks an icon's place when a project has none.
const NO_ICON: u32 = PANEL;
const NO_ICON_TEXT: u32 = SEPARATOR;
const PROJECTS_HINTS: [Hint; 4] = [
    (&[Cap::Name("A")], "Run"),
    (&[Cap::Name("X")], "Programs"),
    (&[Cap::Name("Select"), Cap::Name("A")], "Menu"),
    (&[Cap::Name("L"), Cap::Name("R")], "Page"),
];
const PROGRAMS_HINTS: [Hint; 4] = [
    (&[Cap::Name("A")], "Run"),
    (&[Cap::Name("B")], "Back"),
    (&[Cap::Name("Select"), Cap::Name("A")], "Menu"),
    (&[Cap::Name("L"), Cap::Name("R")], "Page"),
];

/// What the info beside the rows shows of a project, cut to fit there.
struct Info {
    title: Vec<String>,
    folder: String,
    folder_color: u32,
    about: Vec<String>,
    /// The title's first letter, in place of a missing icon.
    letter: String,
}

/// Where the game list's parts go in a UI of a given size, and its text
/// cut to fit there, made when the size or the list shown changes, so
/// that drawing a frame cuts nothing.
struct ListView {
    size: (u32, u32),
    /// Between the bars.
    body: Rect,
    /// The rows: one title or program each.
    list: Rect,
    /// The selected project's info, right of the rows. Beside a project's
    /// programs the rows take its place.
    pane: Rect,
    rows: usize,
    /// Each project's row, and its info.
    titles: Vec<String>,
    infos: Vec<Info>,
    /// The programs shown, and the heading over them.
    labels: Vec<String>,
    heading: String,
}

impl ListView {
    fn new(size: (u32, u32), games: &[Game], programs: Option<&Programs>) -> ListView {
        let (w, h) = size;
        let body = Rect {
            x: 0,
            y: BODY_TOP,
            w,
            h: h.saturating_sub(BODY_TOP + LINE + PAD),
        };
        let rows = ((body.h / LINE) as usize).max(1);
        let list_w = match programs {
            Some(_) => w.saturating_sub(PAD),
            None => w * LIST_SHARE / 20,
        };
        let list = Rect {
            w: list_w,
            h: rows as u32 * LINE,
            ..body
        };
        let pane = Rect {
            x: list_w + PAD,
            w: w.saturating_sub(list_w + 2 * PAD),
            ..body
        };
        let room = list_w.saturating_sub(2 * PAD);
        let mut view = ListView {
            size,
            body,
            list,
            pane,
            rows,
            titles: Vec::new(),
            infos: Vec::new(),
            labels: Vec::new(),
            heading: "Games".to_string(),
        };
        match programs {
            Some(p) => {
                view.labels = p.files.iter().map(|f| fit(f, room)).collect();
                let title = &games[p.game].listed.title;
                view.heading = fit(title, w.saturating_sub(12 * CHAR));
            }
            None => {
                view.titles = games.iter().map(|g| fit(&g.listed.title, room)).collect();
                view.infos = games.iter().map(|g| view.info(g)).collect();
            }
        }
        view
    }

    /// `game`'s info, cut to the pane.
    fn info(&self, game: &Game) -> Info {
        let room = self.pane.w.saturating_sub(2 * PAD).max(CHAR);
        let title = about_lines(&game.listed.title, room, TITLE_LINES);
        let label = project_label(&game.name);
        let (folder, folder_color) = match usable(&game.name) {
            true => (fit(label, room), DISABLED),
            false => (fit(&format!("{label} (rename to use)"), room), ERROR),
        };
        let top = self.about_top(title.len());
        let bottom = (self.pane.y + self.pane.h).saturating_sub(PAD);
        let lines = (bottom.saturating_sub(top) / LINE) as usize;
        let letter = (game.listed.title.chars())
            .find(|c| c.is_alphanumeric())
            .map_or(String::new(), String::from);
        Info {
            about: about_lines(&game.listed.description, room, lines),
            title,
            folder,
            folder_color,
            letter,
        }
    }

    /// Where the title starts in the pane, below the icon.
    fn title_top(&self) -> u32 {
        self.pane.y + PAD + BIG_ICON + PAD
    }

    /// Where the description starts below `title_lines` of the title.
    fn about_top(&self, title_lines: usize) -> u32 {
        self.title_top() + (title_lines as u32 + 1) * LINE + ABOUT_GAP
    }

    fn icon_place(&self) -> Rect {
        Rect {
            x: self.pane.x + PAD,
            y: self.pane.y + PAD,
            w: BIG_ICON,
            h: BIG_ICON,
        }
    }

    /// Rows the list shows at once, at least one.
    fn rows(&self) -> usize {
        self.rows
    }

    fn row(&self, shown: usize) -> Rect {
        Rect {
            y: self.list.y + shown as u32 * LINE,
            h: LINE,
            ..self.list
        }
    }

    /// The scroll thumb for `total` rows from `scroll`, when there are
    /// more than show.
    fn thumb(&self, scroll: usize, total: usize) -> Option<Rect> {
        let rows = self.rows;
        if total <= rows {
            return None;
        }
        let track = self.list.h;
        let h = (track * rows as u32 / total as u32).clamp(LINE / 2, track);
        let moved = (track - h) as usize * scroll.min(total - rows) / (total - rows);
        Some(Rect {
            x: self.list.x + self.list.w + (PAD - THUMB) / 2,
            y: self.list.y + moved as u32,
            w: THUMB,
            h,
        })
    }

    fn draw(&self, fb: &mut Framebuffer, font: &Font, state: &GameList) {
        let (w, h) = self.size;
        fill(fb, Rect { x: 0, y: 0, w, h }, BACKDROP);
        draw_top_bar(fb, font, w, &self.heading, &state.counter);
        let hints = match &state.programs {
            Some(p) => {
                self.draw_programs(fb, font, p);
                &PROGRAMS_HINTS
            }
            None => {
                self.draw_projects(fb, font, state);
                &PROJECTS_HINTS
            }
        };
        draw_hint_bar(fb, font, self.size, hints);
    }

    /// A line in the middle, when there is nothing to show.
    fn draw_empty(&self, fb: &mut Framebuffer, font: &Font, text: &str, color: u32) {
        let body = self.body;
        let text = fit(text, body.w.saturating_sub(2 * PAD));
        let x = body.x + body.w.saturating_sub(text_width(&text)) / 2;
        draw_text(fb, font, x, body.y + body.h / 2 - CHAR / 2, &text, color);
    }

    /// Draws `labels` from `scroll`, with the row of `selected` filled
    /// and its label white.
    fn draw_rows<'a>(
        &self,
        fb: &mut Framebuffer,
        font: &Font,
        (scroll, selected): (usize, usize),
        labels: impl Iterator<Item = (&'a str, u32)>,
    ) {
        for (shown, (label, mut color)) in labels.skip(scroll).take(self.rows).enumerate() {
            let r = self.row(shown);
            if scroll + shown == selected {
                fill(fb, r, HIGHLIGHT);
                color = TEXT;
            }
            draw_text(fb, font, r.x + PAD, r.y + (LINE - CHAR) / 2, label, color);
        }
    }

    fn draw_projects(&self, fb: &mut Framebuffer, font: &Font, state: &GameList) {
        if state.games.is_empty() {
            match &state.status {
                Some(status) => self.draw_empty(fb, font, status, ERROR),
                None => self.draw_empty(fb, font, "No projects found.", DISABLED),
            }
            return;
        }
        let colored = (self.titles.iter().zip(&state.games))
            .map(|(t, g)| (t.as_str(), if usable(&g.name) { TEXT } else { ERROR }));
        self.draw_rows(fb, font, (state.scroll, state.selected), colored);
        if let Some(r) = self.thumb(state.scroll, state.games.len()) {
            fill(fb, r, DISABLED);
        }
        let (Some(game), Some(info)) = (
            state.games.get(state.selected),
            self.infos.get(state.selected),
        ) else {
            return;
        };
        let pane = self.pane;
        fill(fb, pane, WELL);
        border(fb, pane, SEPARATOR);
        let at = self.icon_place();
        match &game.listed.icon {
            Some(icon) => {
                let (x, y) = (at.x, at.y);
                image_scaled(fb, (x, y), icon.w, &icon.pixels, icon.key, ICON_SCALE);
            }
            None => {
                fill(fb, at, NO_ICON);
                border(fb, at, SEPARATOR);
                // The letter at twice the UI's size, in the middle.
                let side = 2 * CHAR;
                let x = at.x + (BIG_ICON - side) / 2;
                let y = at.y + (BIG_ICON - side) / 2;
                draw_text_scaled(fb, font, x, y, &info.letter, NO_ICON_TEXT, 4);
            }
        }
        let x = pane.x + PAD;
        let line_y = |top: u32, i: usize| top + i as u32 * LINE + (LINE - CHAR) / 2;
        let top = self.title_top();
        for (i, line) in info.title.iter().enumerate() {
            draw_text(fb, font, x, line_y(top, i), line, TEXT);
        }
        let y = line_y(top, info.title.len());
        draw_text(fb, font, x, y, &info.folder, info.folder_color);
        let top = self.about_top(info.title.len());
        for (i, line) in info.about.iter().enumerate() {
            draw_text(fb, font, x, line_y(top, i), line, FOLDER);
        }
    }

    fn draw_programs(&self, fb: &mut Framebuffer, font: &Font, p: &Programs) {
        if p.files.is_empty() {
            let text = "This project has no programs.";
            self.draw_empty(fb, font, text, DISABLED);
            return;
        }
        let labels = self.labels.iter().map(|l| (l.as_str(), TEXT));
        self.draw_rows(fb, font, (p.scroll, p.selected), labels);
        if let Some(r) = self.thumb(p.scroll, p.files.len()) {
            fill(fb, r, DISABLED);
        }
    }
}

/// A text's first `max` lines, wrapped to `width`; the last shown ends
/// in `~` when there is more.
fn about_lines(text: &str, width: u32, max: usize) -> Vec<String> {
    let mut lines = (text.lines())
        .flat_map(|line| wrap(line, width))
        .filter(|line| !line.is_empty());
    let mut shown: Vec<String> = lines.by_ref().take(max).collect();
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

    /// UI sizes from the smallest handled to the largest expected.
    const SIZES: [(u32, u32); 4] = [(320, 240), (640, 480), (960, 540), (1280, 720)];

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
    fn auto_smooths_a_smaller_screen_and_keeps_pixels_sharp_otherwise() {
        let auto = |mode: &ScreenMode, shown| screen_filter(HandheldFilter::Auto, mode, shown);
        // 0.5x, 1x, 1.6x and 2x on a 640x480 display.
        assert_eq!(
            auto(&ScreenMode::new(1280, 720), (640.0, 360.0)),
            Filter::Bilinear
        );
        assert_eq!(
            auto(&ScreenMode::new(640, 360), (640.0, 360.0)),
            Filter::Nearest
        );
        let small = ScreenMode::new(400, 240);
        assert_eq!(auto(&small, (640.0, 384.0)), Filter::SmartNearest);
        assert_eq!(
            auto(&ScreenMode::new(320, 240), (640.0, 480.0)),
            Filter::Nearest
        );
        // The frame counts, which XSCREEN's sampling makes larger.
        let sampled = ScreenMode { sample: 2, ..small };
        assert_eq!(auto(&sampled, (640.0, 384.0)), Filter::Bilinear);
        // Pixels made wider by an aspect ratio are not whole.
        let wide = ScreenMode {
            aspect: Some(2.0),
            ..ScreenMode::new(320, 240)
        };
        assert_eq!(auto(&wide, (640.0, 320.0)), Filter::SmartNearest);
        // The other choices take no notice of the scale, and Program
        // takes XSCREEN's.
        for (choice, filter) in [
            (HandheldFilter::PixelArt, Filter::SmartNearest),
            (HandheldFilter::Bilinear, Filter::Bilinear),
            (HandheldFilter::Nearest, Filter::Nearest),
        ] {
            for (mode, shown) in [(small, (640.0, 384.0)), (small, (400.0, 240.0))] {
                assert_eq!(screen_filter(choice, &mode, shown), filter);
            }
        }
        let mode_1 = ScreenMode {
            filter: Filter::SmartNearest,
            ..small
        };
        let program = |mode| screen_filter(HandheldFilter::Xscreen, mode, (640.0, 384.0));
        assert_eq!(program(&mode_1), Filter::SmartNearest);
        assert_eq!(program(&small), Filter::Bilinear);
    }

    #[test]
    fn select_and_a_open_the_menu_select_and_start_quit_and_start_alone_is_plus() {
        let mut keys = Hotkeys::default();
        assert!(!keys.a_down(true));
        assert_eq!(keys.start_down(true), Start::Plus);
        assert!(keys.start_up(), "its release is the + button's");
        keys.select(true);
        assert!(keys.a_down(true));
        assert_eq!(keys.start_down(true), Start::Quit);
        // Its release is not the + button's, even with Select let go
        // first.
        keys.select(false);
        assert!(!keys.start_up());
        assert!(!keys.a_down(true));
        assert_eq!(keys.start_down(true), Start::Plus);
        assert!(keys.start_up());
        // On the desktop, Select changes nothing.
        keys.select(true);
        assert!(!keys.a_down(false));
        assert_eq!(keys.start_down(false), Start::Plus);
        assert!(keys.start_up());
    }

    #[test]
    fn the_pause_menu_offers_what_applies() {
        let auto = HandheldFilter::Auto;
        let running = PauseMenu::new(true, true, true, auto);
        let all = [
            Choice::Resume,
            Choice::Restart,
            Choice::Filter,
            Choice::List,
            Choice::Quit,
        ];
        assert_eq!(running.items, all);
        assert_eq!(running.label(Choice::Resume), "Resume");
        let unknown = PauseMenu::new(true, true, false, auto);
        let items = [Choice::Resume, Choice::Filter, Choice::List, Choice::Quit];
        assert_eq!(unknown.items, items);
        let list = PauseMenu::new(false, false, true, auto);
        assert_eq!(list.items, [Choice::Resume, Choice::Filter, Choice::Quit]);
        assert_eq!(list.label(Choice::Resume), "Close");
    }

    #[test]
    fn the_pause_menu_answers_the_pad_and_keys() {
        let mut menu = PauseMenu::new(true, true, true, HandheldFilter::Auto);
        assert_eq!(menu.pad(Button::DPadUp), None);
        assert_eq!(menu.pad(Button::DPadRight), None, "not on the filter");
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
        assert_eq!(menu.filter(), HandheldFilter::Auto, "unchanged");
    }

    #[test]
    fn the_screen_filter_steps_both_ways_and_round() {
        let mut menu = PauseMenu::new(false, false, false, HandheldFilter::Auto);
        menu.pad(Button::DPadDown);
        assert_eq!(menu.items[menu.focus], Choice::Filter);
        assert_eq!(menu.pad(Button::DPadRight), Some(Choice::Filter));
        assert_eq!(menu.filter(), HandheldFilter::PixelArt);
        assert_eq!(menu.key(Keycode::Left), Some(Choice::Filter));
        assert_eq!(menu.pad(Button::DPadLeft), Some(Choice::Filter));
        assert_eq!(menu.filter(), HandheldFilter::Xscreen, "round to the last");
        // A steps on, and the menu stays.
        assert_eq!(menu.pad(Button::East), Some(Choice::Filter));
        assert_eq!(menu.filter(), HandheldFilter::Auto);
        let names: Vec<&str> = (0..5)
            .map(|_| {
                menu.key(Keycode::Right);
                filter_name(menu.filter())
            })
            .collect();
        assert_eq!(names, ["Pixel art", "Smooth", "Sharp", "Program", "Auto"]);
    }

    #[test]
    fn the_pause_menu_fits_a_small_screen() {
        let menu = PauseMenu::new(true, true, true, HandheldFilter::PixelArt);
        for size in SIZES {
            let f = menu.frame(size);
            assert!(f.x + f.w <= size.0 && f.y + f.h <= size.1, "{f:?}");
            let last = menu.item(f, menu.items.len() - 1);
            assert!(last.y + last.h + PAD <= f.y + f.h);
            // The filter's arrows and value stay right of its label.
            let (left, value, right) = PauseMenu::value_places(menu.item(f, 2));
            assert!(left.x >= f.x + PAD + text_width(FILTER_LABEL) + PAD);
            assert!(value.x + value.w <= right.x && right.x + right.w < f.x + f.w);
            // The window stays clear of the hints.
            assert!(f.y + f.h <= size.1 - LINE, "{size:?}");
            let mut fb = Framebuffer::new(size.0, size.1);
            menu.draw(&mut fb, &Font::blank());
        }
    }

    #[test]
    fn hints_that_do_not_fit_are_left_out() {
        assert_eq!(hints_shown(640, &PROJECTS_HINTS), 4);
        assert_eq!(hints_shown(640, &PROGRAMS_HINTS), 4);
        assert_eq!(hints_shown(640, &MENU_HINTS), 3);
        // The menu's hint is the last to go.
        assert_eq!(hints_shown(320, &PROJECTS_HINTS), 3);
        assert_eq!(hints_shown(0, &PROJECTS_HINTS), 0);
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
        let s = storage(&tmp, 40);
        let mut list = GameList::new(&s, (640, 480), &Memory::default());
        let rows = list.view.rows();
        assert_eq!(rows, 17, "rows at 640x480");
        assert_eq!(list.view.thumb(0, 40).map(|r| r.y), Some(BODY_TOP));
        list.pad(Button::RightShoulder, &s);
        assert_eq!(list.selected, rows);
        assert_eq!(list.scroll, 1, "the selection stays shown");
        for _ in 0..3 {
            list.pad(Button::RightShoulder, &s);
        }
        assert_eq!(list.selected, 39, "no further than the last");
        assert_eq!(list.counter, "40/40");
        // The thumb is at the end of its track.
        let thumb = list.view.thumb(list.scroll, 40).unwrap();
        assert_eq!(thumb.y + thumb.h, list.view.list.y + list.view.list.h);
        list.pad(Button::DPadDown, &s);
        assert_eq!(list.selected, 39);
        list.pad(Button::LeftShoulder, &s);
        assert_eq!(list.selected, 39 - rows);
        list.key(Keycode::Home, &s);
        assert_eq!((list.selected, list.scroll), (0, 0));
        list.pad(Button::DPadUp, &s);
        assert_eq!(list.selected, 0);
        // More rows on a taller UI, fewer on a smaller one, and the
        // selection still shows.
        list.key(Keycode::End, &s);
        for (size, more) in [((1280, 720), true), ((320, 240), false)] {
            list.set_size(size);
            assert_eq!(list.view.rows() > rows, more);
            assert!(list.scroll <= 39 && 39 < list.scroll + list.view.rows());
        }
    }

    #[test]
    fn the_thumb_shows_only_when_the_rows_do_not_all_fit() {
        let tmp = TempDir::new("thumb");
        let s = storage(&tmp, 5);
        let list = GameList::new(&s, (640, 480), &Memory::default());
        assert_eq!(list.view.thumb(0, 5), None);
        let r = list.view.thumb(0, 34).unwrap();
        assert_eq!(r.h, list.view.list.h / 2);
        // Clear of the rows and of the info.
        assert!(r.x >= list.view.list.x + list.view.list.w);
        assert!(r.x + r.w <= list.view.pane.x);
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
    fn the_parts_follow_the_ui_size() {
        let tmp = TempDir::new("sizes");
        let s = storage(&tmp, 30);
        for size in SIZES {
            let mut list = GameList::new(&s, size, &Memory::default());
            let v = &list.view;
            let (w, h) = size;
            // The rows, the gap with the thumb, the pane, side by side.
            assert_eq!(v.list.x + v.list.w + PAD, v.pane.x, "{size:?}");
            assert_eq!(v.pane.x + v.pane.w + PAD, w, "{size:?}");
            assert!(v.pane.w >= BIG_ICON + 2 * PAD, "{size:?}");
            // Between the bars.
            assert!(v.list.y >= LINE && v.list.y + v.list.h <= h - LINE);
            assert!(v.pane.y + v.pane.h <= h - LINE);
            // The title's lines and the folder's below the icon, at least.
            let folder_bottom = v.title_top() + (TITLE_LINES as u32 + 1) * LINE;
            assert!(folder_bottom + PAD <= v.pane.y + v.pane.h, "{size:?}");
            let mut fb = Framebuffer::new(w, h);
            list.draw(&mut fb, &Font::blank());
            // The programs take the whole width, but for the thumb.
            list.pad(Button::North, &s);
            let v = &list.view;
            assert_eq!(v.list.x + v.list.w + PAD, w);
            list.draw(&mut fb, &Font::blank());
        }
    }

    #[test]
    fn titles_descriptions_and_icons_fit_their_places() {
        let tmp = TempDir::new("meta");
        let s = storage(&tmp, 2);
        let mut icon = vec![0; ICON_SIZE * ICON_SIZE];
        icon[0] = 0xFF12_3456_u32 as i32;
        // A line to wrap, and more lines than any pane has room for.
        let mut long = "A long description of a game that goes on and on.".to_string();
        for i in 0..20 {
            long.push_str(&format!("\nLine {i:02}"));
        }
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
        for size in SIZES {
            let mut list = GameList::new(&s, size, &Memory::default());
            let view = &list.view;
            let room = view.pane.w - 2 * PAD;
            // The row cuts it up to 640 wide, and from there the info has
            // it all.
            let title = &view.titles[0];
            assert_eq!(title.ends_with('~'), size.0 <= 640, "{title}");
            assert!(text_width(title) + 2 * PAD <= view.list.w);
            let info = &view.infos[0];
            let whole = info.title.concat() == "宇宙".repeat(12);
            assert_eq!(whole, size.0 >= 640, "{:?}", info.title);
            assert!(info.title.len() <= TITLE_LINES);
            let lines = info.title.iter().chain(&info.about);
            assert!(lines.clone().all(|l| text_width(l) <= room), "{size:?}");
            let bottom = view.about_top(info.title.len()) + info.about.len() as u32 * LINE;
            assert!(info.about.is_empty() || bottom + PAD <= view.pane.y + view.pane.h);
            if let Some(last) = info.about.last() {
                assert!(last.ends_with('~'), "{last}");
            }
            let mut fb = Framebuffer::new(size.0, size.1);
            list.draw(&mut fb, &Font::blank());
            // The icon's first pixel, twice its size.
            let at = view.icon_place();
            for (x, y) in [(at.x, at.y), (at.x + 1, at.y + 1)] {
                assert_eq!(fb.get(x, y).map(|p| p & 0xFF_FFFF), Some(0x12_3456));
            }
            assert_ne!(
                fb.get(at.x + 2, at.y).map(|p| p & 0xFF_FFFF),
                Some(0x12_3456)
            );
            // One with no icon has a place for it all the same.
            list.pad(Button::DPadDown, &s);
            assert_eq!(list.view.infos[1].letter, "G");
            list.draw(&mut fb, &Font::blank());
            assert_eq!(fb.get(at.x, at.y), Some(SEPARATOR));
        }
        // At 640x480, the description has nine lines below a title of
        // two, and ten below one.
        let list = GameList::new(&s, (640, 480), &Memory::default());
        assert_eq!(list.view.infos[0].about.len(), 9);
        let mut meta = meta;
        meta.title = "SHORT".encode_utf16().collect();
        for (name, bytes) in sb_runtime::env::meta_files(&meta).unwrap() {
            s.write_file_in("GAME00", name, &bytes).unwrap();
        }
        let list = GameList::new(&s, (640, 480), &Memory::default());
        assert_eq!(list.view.infos[0].about.len(), 10);
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
        assert_eq!(list.view.infos.last().unwrap().folder, "[WORKSPACE]");
        list.key(Keycode::End, &s);
        assert_eq!(list.key(Keycode::Return, &s), run("", MAIN));
        if let Some(bad) = list.games.iter().position(|g| !usable(&g.name)) {
            let info = &list.view.infos[bad];
            assert_eq!(info.folder_color, ERROR);
            assert!(info.folder.ends_with("(rename to use)"), "{}", info.folder);
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

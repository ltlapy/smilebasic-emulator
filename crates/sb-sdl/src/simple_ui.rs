//! The Simple UI: the frontend laid out for a small screen and a gamepad
//! alone. The UI is as many pixels as the screen at a whole scale, or at
//! one and a half times one at the Medium size, the
//! emulator starts in a list of the projects instead of DIRECT mode,
//! Select (SDL's Back, the − button) with A opens a pause menu, and Select
//! with Start quits. Programs never see Select: BUTTON has no bit for it.

use sb_runtime::Framebuffer;
use sb_runtime::confirm::project_label;
use sb_runtime::env::ICON_SIZE;
use sb_runtime::font::Font;
use sb_runtime::screen::{Filter, ScreenMode};
use sb_runtime::settings::{Settings, SimpleUiFilter, SimpleUiSize};
use sb_runtime::storage::{FileKind, Storage};
use sdl3::gamepad::Button;
use sdl3::keyboard::Keycode;
use sdl3::render::FRect;

use crate::keyhint::{BUTTON_TEXT, draw_arrow};
use crate::program_dialog::wrap;
use crate::projects::{ERROR, FOLDER, Listed, PANEL, SEPARATOR, WELL, fit, usable};
use crate::ui::{
    BACKGROUND, CHAR, DISABLED, HIGHLIGHT, LINE, PAD, Rect, TEXT, draw_text, draw_text_double,
    draw_text_scaled, fill, image_scaled, text_width,
};

/// The UI's height at a scale of 1: at a whole multiple of it or more,
/// UI pixels are that many screen pixels across.
const UI_LINES: u32 = 480;
/// The smallest UI that the Simple UI's screens are laid out for.
const SMALLEST_UI: (u32, u32) = (320, 240);

/// How many half screen pixels a UI pixel takes on an output of `out`
/// pixels: two for every 480 lines at [`SimpleUiSize::Normal`], and 1.5
/// or 2 times that many at Medium and Large as long as the UI keeps
/// 320x240 pixels. Normal and Large are whole scales; Medium is a half
/// more than a whole one where Normal's is odd: 1.5 on a 640x480 or
/// 1280x720 display, 3 on a 1920x1080 one.
fn ui_halves((w, h): (u32, u32), size: SimpleUiSize) -> u32 {
    let normal = 2 * (h / UI_LINES).max(1);
    let most = (2 * w / SMALLEST_UI.0).min(2 * h / SMALLEST_UI.1);
    match size {
        SimpleUiSize::Normal => normal,
        SimpleUiSize::Medium => (normal * 3 / 2).min(most).max(normal),
        // A whole scale.
        SimpleUiSize::Large => (2 * normal).min(most & !1).max(normal),
    }
}

/// The UI's size on an output of `out` pixels: the output at the scale
/// [`ui_halves`] gives, so that the 16-pixel UI font stays sharp. 640x480 gives
/// 640x480, 427x320 at Medium or 320x240 at Large; 1920x1080 gives
/// 960x540, 640x360 or 480x270. At a scale that is not whole, the UI
/// takes the part of a pixel left over, so that it covers the output.
pub fn ui_size((w, h): (u32, u32), size: SimpleUiSize) -> (u32, u32) {
    let halves = ui_halves((w, h), size);
    let across = |n: u32| match halves % 2 {
        0 => n / (halves / 2),
        _ => (2 * n).div_ceil(halves),
    };
    (across(w).max(1), across(h).max(1))
}

/// Where a UI of `ui` pixels goes on an output of `out` pixels at
/// `size`: at the largest whole scale that fits, centered across and at
/// the top, as the desktop's display is. At a scale that is not whole,
/// it starts at the output's top left corner, on a whole pixel, and may
/// go past the right and bottom edges by less than a pixel of its own.
pub fn display_rect((w, h): (u32, u32), (out_w, out_h): (u32, u32), size: SimpleUiSize) -> FRect {
    let halves = ui_halves((out_w, out_h), size);
    if halves % 2 == 1 && (w, h) == ui_size((out_w, out_h), size) {
        let scale = halves as f32 / 2.0;
        return FRect::new(0.0, 0.0, w as f32 * scale, h as f32 * scale);
    }
    let scale = (out_w / w.max(1)).min(out_h / h.max(1)).max(1);
    let (sw, sh) = (w * scale, h * scale);
    FRect::new(
        out_w.saturating_sub(sw) as f32 / 2.0,
        0.0,
        sw as f32,
        sh as f32,
    )
}

/// The filter the Simple UI scales the screen with, by `choice`, when the
/// frame of `mode` is drawn `shown` pixels large. The screen fits the
/// display at its aspect ratio whatever the filter, so it is seldom a
/// whole number of times its size, and it can be smaller than its frame:
/// XSCREEN goes up to 1280x720.
pub fn screen_filter(choice: SimpleUiFilter, mode: &ScreenMode, (w, h): (f32, f32)) -> Filter {
    match choice {
        SimpleUiFilter::Auto => {
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
        SimpleUiFilter::PixelArt => Filter::SmartNearest,
        SimpleUiFilter::Bilinear => Filter::Bilinear,
        SimpleUiFilter::Nearest => Filter::Nearest,
        SimpleUiFilter::Xscreen => mode.filter,
    }
}

/// What a press of Start is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// With Select held and the Simple UI on: quit the emulator, as the
    /// frontends of such handhelds do.
    Quit,
    /// The + button, as ever.
    Plus,
}

/// Select as the modifier of the Simple UI's hotkeys.
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

    /// A went down: true when, with Select held and the Simple UI on, it opens
    /// or closes the pause menu and is no press of A.
    pub fn a_down(&self, simple_ui: bool) -> bool {
        simple_ui && self.select
    }

    /// Start went down.
    pub fn start_down(&mut self, simple_ui: bool) -> Start {
        if simple_ui && self.select {
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
    /// Stop the program and show the game list.
    List,
    Quit,
    /// A setting changed, as [`PauseMenu::settings`] has it now; the menu
    /// stays open.
    Filter,
    UiSize,
    Swap,
}

impl Choice {
    /// Whether it is one of the settings, which Left and Right change.
    fn is_setting(self) -> bool {
        matches!(self, Choice::Filter | Choice::UiSize | Choice::Swap)
    }
}

/// The settings that the pause menu changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuSettings {
    pub filter: SimpleUiFilter,
    pub ui_size: SimpleUiSize,
    pub swap: bool,
}

impl MenuSettings {
    pub fn of(s: &Settings) -> MenuSettings {
        MenuSettings {
            filter: s.simple_ui_filter,
            ui_size: s.simple_ui_size,
            swap: s.swap_ab_xy,
        }
    }

    /// Puts them in `s`, to show at once and be kept.
    pub fn apply(self, s: &mut Settings) {
        s.simple_ui_filter = self.filter;
        s.simple_ui_size = self.ui_size;
        s.swap_ab_xy = self.swap;
    }
}

/// The menu that Select+A opens, over a program or the game list: what it
/// does first, then, below a divider, the settings.
pub struct PauseMenu {
    /// Top to bottom; `None` is the divider over the settings.
    rows: Vec<Option<Choice>>,
    /// A row with a choice.
    focus: usize,
    /// The first row shown, when they do not all fit.
    scroll: usize,
    /// A program runs, which the menu pauses.
    running: bool,
    settings: MenuSettings,
    /// The UI's size.
    size: (u32, u32),
}

impl PauseMenu {
    /// The menu for a program that `running` or waits after its error
    /// (`program`), with Restart if `restart`, showing `settings`, in a
    /// UI of `size`.
    pub fn new(
        running: bool,
        program: bool,
        restart: bool,
        settings: MenuSettings,
        size: (u32, u32),
    ) -> PauseMenu {
        let mut rows = vec![Some(Choice::Resume)];
        if restart && program {
            rows.push(Some(Choice::Restart));
        }
        if program {
            rows.push(Some(Choice::List));
        }
        rows.extend([Some(Choice::Quit), None]);
        rows.extend(SETTINGS.map(Some));
        PauseMenu {
            rows,
            focus: 0,
            scroll: 0,
            running,
            settings,
            size,
        }
    }

    /// The settings it shows.
    pub fn settings(&self) -> MenuSettings {
        self.settings
    }

    /// Lays the menu out for a UI of another size.
    pub fn set_size(&mut self, size: (u32, u32)) {
        if size != self.size {
            self.size = size;
            self.keep_in_sight();
        }
    }

    fn focused(&self) -> Choice {
        self.rows[self.focus].unwrap_or(Choice::Resume)
    }

    /// Moves the focus to the next choice up (-1) or down (1), past the
    /// divider, and no further than the ends.
    fn go(&mut self, by: isize) {
        let mut at = self.focus;
        while let Some(next) = at.checked_add_signed(by).filter(|&i| i < self.rows.len()) {
            at = next;
            if self.rows[at].is_some() {
                self.focus = at;
                break;
            }
        }
        self.keep_in_sight();
    }

    /// Scrolls so that the focus shows, with the divider over it when it
    /// is the first setting.
    fn keep_in_sight(&mut self) {
        let shown = self.shown_rows();
        let mut top = self.focus;
        if top > 0 && self.rows[top - 1].is_none() && shown > 1 {
            top -= 1;
        }
        if top < self.scroll {
            self.scroll = top;
        } else if self.focus >= self.scroll + shown {
            self.scroll = self.focus + 1 - shown;
        }
        self.scroll = self.scroll.min(self.rows.len() - shown);
    }

    /// Steps the setting `choice` by `by`.
    fn step(&mut self, choice: Choice, by: isize) -> Option<Choice> {
        let s = &mut self.settings;
        match choice {
            Choice::Filter => s.filter = s.filter.step(by),
            Choice::UiSize => s.ui_size = s.ui_size.step(by),
            Choice::Swap => s.swap ^= true,
            _ => return None,
        }
        Some(choice)
    }

    /// Up and Down move, Enter chooses, Esc and Backspace close. On a
    /// setting, Left and Right change it, and Enter steps it on.
    pub fn key(&mut self, key: Keycode) -> Option<Choice> {
        let focused = self.focused();
        let setting = focused.is_setting();
        match key {
            Keycode::Up => self.go(-1),
            Keycode::Down => self.go(1),
            Keycode::Escape | Keycode::Backspace => return Some(Choice::Resume),
            Keycode::Left if setting => return self.step(focused, -1),
            Keycode::Right | Keycode::Return | Keycode::KpEnter if setting => {
                return self.step(focused, 1);
            }
            Keycode::Return | Keycode::KpEnter => return Some(focused),
            _ => {}
        }
        None
    }

    /// A chooses and B closes, A on the right and B at the bottom unless
    /// the buttons are swapped.
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
/// The dim heading over the rows.
const MENU_HEADING: u32 = 32;
/// The heading of the settings, on the divider.
const SETTINGS_HEADING: &str = "Settings";
/// The room for an arrow on either side of a setting's value.
const ARROW: u32 = 16;
const MENU_HINTS: [Hint; 3] = [
    (&[Cap::Name("A")], "Choose"),
    (&[Cap::Name("B")], "Resume"),
    (&[Cap::Left, Cap::Right], "Change"),
];
/// The same where nothing is paused.
const CLOSE_HINTS: [Hint; 3] = [
    (&[Cap::Name("A")], "Choose"),
    (&[Cap::Name("B")], "Close"),
    (&[Cap::Left, Cap::Right], "Change"),
];
const SETTINGS: [Choice; 3] = [Choice::Filter, Choice::UiSize, Choice::Swap];

/// A filter's name in the menu.
fn filter_name(filter: SimpleUiFilter) -> &'static str {
    match filter {
        SimpleUiFilter::Auto => "Auto",
        SimpleUiFilter::PixelArt => "Pixel art",
        SimpleUiFilter::Bilinear => "Smooth",
        SimpleUiFilter::Nearest => "Sharp",
        SimpleUiFilter::Xscreen => "Program",
    }
}

fn ui_size_name(size: SimpleUiSize) -> &'static str {
    match size {
        SimpleUiSize::Normal => "Normal",
        SimpleUiSize::Medium => "Medium",
        SimpleUiSize::Large => "Large",
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "On" } else { "Off" }
}

/// The values' place: the widest value of any setting and a gap either
/// side, so that the arrows stay put and line up.
fn value_width() -> u32 {
    let filters = SimpleUiFilter::ALL.map(filter_name);
    let sizes = SimpleUiSize::ALL.map(ui_size_name);
    let switches = [on_off(true), on_off(false)];
    let widest = (filters.iter().chain(&sizes).chain(&switches)).map(|name| text_width(name));
    widest.max().unwrap_or(0) + PAD
}

/// What the menu needs across for a setting labelled `label`.
fn setting_width(label: &str) -> u32 {
    text_width(label) + PAD + 2 * ARROW + value_width()
}

impl PauseMenu {
    fn label(&self, choice: Choice) -> &'static str {
        match choice {
            Choice::Resume if self.running => "Resume",
            Choice::Resume => "Close",
            Choice::Restart => "Restart",
            Choice::List => "Return to Game list",
            Choice::Quit => "Quit Emulator",
            Choice::Filter => "Screen filter:",
            Choice::UiSize => "UI size:",
            Choice::Swap => "Swap A/B and X/Y:",
        }
    }

    /// What a setting is set to, or nothing for the other rows.
    fn value(&self, choice: Choice) -> Option<&'static str> {
        let s = &self.settings;
        match choice {
            Choice::Filter => Some(filter_name(s.filter)),
            Choice::UiSize => Some(ui_size_name(s.ui_size)),
            Choice::Swap => Some(on_off(s.swap)),
            _ => None,
        }
    }

    /// The rows that fit in the UI at once, between the heading and the
    /// hints, and at least one.
    fn shown_rows(&self) -> usize {
        let room = (self.size.1.saturating_sub(LINE + 2 * PAD + MENU_HEADING) / LINE) as usize;
        room.clamp(1, self.rows.len())
    }

    fn frame(&self) -> Rect {
        let (w, h) = self.size;
        let labels = (self.rows.iter().flatten()).map(|&c| match c.is_setting() {
            true => setting_width(self.label(c)),
            false => text_width(self.label(c)),
        });
        let widest = labels.chain([text_width(SETTINGS_HEADING)]).max();
        // The padding either side, and the frame's border.
        let fw = (widest.unwrap_or(0) + 2 * PAD + 2).max(MENU_WIDTH).min(w);
        let fh = PAD + MENU_HEADING + self.shown_rows() as u32 * LINE + PAD;
        // In the middle, or as low as it can go clear of the hints.
        let above_hints = h.saturating_sub(LINE + fh);
        Rect {
            x: w.saturating_sub(fw) / 2,
            y: (h.saturating_sub(fh) / 2).min(above_hints),
            w: fw,
            h: fh,
        }
    }

    /// The place of the row shown `i`th from the top.
    fn item(&self, f: Rect, i: usize) -> Rect {
        Rect {
            x: f.x + 1,
            y: f.y + PAD + MENU_HEADING + i as u32 * LINE,
            w: f.w.saturating_sub(2),
            h: LINE,
        }
    }

    /// The scroll thumb, in the frame's right margin, when not every row
    /// shows.
    fn thumb(&self, f: Rect) -> Option<Rect> {
        let (shown, total) = (self.shown_rows(), self.rows.len());
        if shown >= total {
            return None;
        }
        let track = shown as u32 * LINE;
        let h = (track * shown as u32 / total as u32).max(LINE / 2);
        let moved = (track - h) * self.scroll as u32 / (total - shown) as u32;
        Some(Rect {
            x: (f.x + f.w).saturating_sub(2 + THUMB),
            y: f.y + PAD + MENU_HEADING + moved,
            w: THUMB,
            h,
        })
    }

    /// Where a setting's value and its arrows go in its row `r`: the
    /// left arrow, the value's place, the right arrow.
    fn value_places(r: Rect) -> (Rect, Rect, Rect) {
        let slot = value_width();
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
        let f = self.frame();
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
        let shown = self.rows.iter().enumerate().skip(self.scroll);
        for (i, (at, &row)) in shown.take(self.shown_rows()).enumerate() {
            let r = self.item(f, i);
            let y = r.y + (LINE - CHAR) / 2;
            let Some(choice) = row else {
                // A dim heading, and a line on to the right.
                draw_text(fb, font, f.x + PAD, y, SETTINGS_HEADING, DISABLED);
                let x = f.x + 2 * PAD + text_width(SETTINGS_HEADING);
                let line = Rect {
                    x,
                    y: r.y + LINE / 2,
                    w: (f.x + f.w).saturating_sub(x + PAD),
                    h: 1,
                };
                fill(fb, line, SEPARATOR);
                continue;
            };
            let focused = at == self.focus;
            if focused {
                fill(fb, r, HIGHLIGHT);
            }
            draw_text(fb, font, f.x + PAD, y, self.label(choice), TEXT);
            if let Some(name) = self.value(choice) {
                let (left, value, right) = Self::value_places(r);
                let arrows = if focused { TEXT } else { DISABLED };
                draw_arrow(fb, left, (-1, 0), arrows);
                draw_arrow(fb, right, (1, 0), arrows);
                let x = value.x + value.w.saturating_sub(text_width(name)) / 2;
                draw_text(fb, font, x, y, name, TEXT);
            }
        }
        if let Some(r) = self.thumb(f) {
            fill(fb, r, DISABLED);
        }
        let hints = if self.running {
            &MENU_HINTS
        } else {
            &CLOSE_HINTS
        };
        draw_hint_bar(fb, font, size, hints);
    }
}

/// What the welcome screen says, below the emulator's name.
const WELCOME_LINES: [&str; 3] = [
    "Runs SmileBASIC 4 games and programs.",
    "Select+A opens the menu.",
    "Select+Start quits.",
];
/// Around the A on "Press A to continue".
const PRESS: (&str, &str) = ("Press", "to continue");
const WELCOME_NOTE: &str = "The button you press is used as A.";

/// What the welcome screen asks for next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Welcomed {
    Stay,
    /// Go on to the game list, with Swap A/B and X/Y set to this if a
    /// button said which is A.
    Continue(Option<bool>),
}

/// The screen the Simple UI shows the first time it starts: what this is,
/// how to open the menu, and "Press A to continue". The button pressed for
/// it is A from then on, the right or the bottom one, whatever the
/// controller prints on it or the system's mapping makes of it.
pub struct Welcome {
    /// SDL's right or bottom face button, by its place, that went down
    /// while the screen showed. Its release goes on, so that a button held
    /// when the screen came does nothing, and the setting does not change
    /// under a held button.
    pressed: Option<Button>,
    size: (u32, u32),
    /// The lines, wrapped to the UI's width.
    lines: Vec<String>,
    note: Vec<String>,
    /// The name is drawn twice its size where it fits.
    big_title: bool,
}

impl Welcome {
    pub fn new(size: (u32, u32)) -> Welcome {
        let mut welcome = Welcome {
            pressed: None,
            size: (0, 0),
            lines: Vec::new(),
            note: Vec::new(),
            big_title: false,
        };
        welcome.set_size(size);
        welcome
    }

    pub fn set_size(&mut self, size: (u32, u32)) {
        if size == self.size {
            return;
        }
        self.size = size;
        let room = size.0.saturating_sub(2 * PAD).max(CHAR);
        let wrapped = |text: &str| wrap(text, room);
        self.lines = WELCOME_LINES.iter().flat_map(|l| wrapped(l)).collect();
        self.note = wrapped(WELCOME_NOTE);
        self.big_title = 2 * text_width(crate::TITLE) <= room;
    }

    /// A button went down: `button` as [`crate::controls::face`] made it
    /// with `swap`, the setting now.
    pub fn pad_down(&mut self, button: Button, swap: bool) {
        let placed = crate::controls::face(button, swap);
        if self.pressed.is_none() && matches!(placed, Button::East | Button::South) {
            self.pressed = Some(placed);
        }
    }

    /// A button went up: the one pressed for the screen says which is A.
    pub fn pad_up(&mut self, button: Button, swap: bool) -> Welcomed {
        let placed = crate::controls::face(button, swap);
        match self.pressed {
            Some(pressed) if pressed == placed => Welcomed::Continue(Some(placed == Button::South)),
            _ => Welcomed::Stay,
        }
    }

    /// Enter goes on and leaves the setting alone.
    pub fn key(&self, key: Keycode) -> Welcomed {
        match key {
            Keycode::Return | Keycode::KpEnter => Welcomed::Continue(None),
            _ => Welcomed::Stay,
        }
    }

    /// How high the text is, from the name to the note.
    fn text_height(&self) -> u32 {
        let title = if self.big_title { 2 * LINE } else { LINE };
        let lines = (self.lines.len() + 1 + self.note.len()) as u32 * LINE;
        title + PAD + lines + 2 * PAD
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        let (w, h) = self.size;
        fill(fb, Rect { x: 0, y: 0, w, h }, BACKDROP);
        let centered = |text_w: u32| w.saturating_sub(text_w) / 2;
        let mut y = h.saturating_sub(self.text_height()) / 2;
        let name = crate::TITLE;
        if self.big_title {
            let x = centered(2 * text_width(name));
            draw_text_double(fb, font, x, y + LINE - CHAR, name, TEXT);
            y += 2 * LINE;
        } else {
            draw_text(fb, font, centered(text_width(name)), y + 4, name, TEXT);
            y += LINE;
        }
        y += PAD;
        let text_y = |y: u32| y + (LINE - CHAR) / 2;
        for line in &self.lines {
            draw_text(
                fb,
                font,
                centered(text_width(line)),
                text_y(y),
                line,
                FOLDER,
            );
            y += LINE;
        }
        y += PAD;
        // "Press", the A as on the hints, "to continue".
        let (before, after) = PRESS;
        let cap = Cap::Name("A");
        let gap = CHAR / 2;
        let whole = text_width(before) + gap + cap_width(cap) + gap + text_width(after);
        let mut x = centered(whole);
        draw_text(fb, font, x, text_y(y), before, TEXT);
        x += text_width(before) + gap;
        let r = Rect {
            x,
            y: y + (LINE - CAP_HEIGHT) / 2,
            w: cap_width(cap),
            h: CAP_HEIGHT,
        };
        draw_cap(fb, font, r, cap);
        x += r.w + gap;
        draw_text(fb, font, x, text_y(y), after, TEXT);
        y += LINE + PAD;
        for line in &self.note {
            draw_text(
                fb,
                font,
                centered(text_width(line)),
                text_y(y),
                line,
                DISABLED,
            );
            y += LINE;
        }
    }
}

// The bars at the top and the bottom of the Simple UI's screens, which
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
/// Behind everything: the Simple UI's screens cover the program's.
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

/// The list of projects that the Simple UI starts in and comes back to.
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
                std::env::temp_dir().join(format!("sb-simple-ui-{tag}-{}", std::process::id()));
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

    /// UI sizes from the smallest handled to the largest expected, with
    /// Medium's on 640x480 and 1280x720.
    const SIZES: [(u32, u32); 7] = [
        (320, 240),
        (427, 320),
        (640, 360),
        (640, 480),
        (854, 480),
        (960, 540),
        (1280, 720),
    ];

    const NORMAL: SimpleUiSize = SimpleUiSize::Normal;
    const MEDIUM: SimpleUiSize = SimpleUiSize::Medium;
    const LARGE: SimpleUiSize = SimpleUiSize::Large;

    #[test]
    fn the_ui_keeps_whole_pixels() {
        let ui_size = |out| super::ui_size(out, NORMAL);
        assert_eq!(ui_size((640, 480)), (640, 480));
        assert_eq!(ui_size((1920, 1080)), (960, 540));
        assert_eq!(ui_size((1280, 720)), (1280, 720));
        assert_eq!(ui_size((2560, 1440)), (853, 480));
        // Smaller than the line count, a UI pixel is still a screen's.
        assert_eq!(ui_size((320, 240)), (320, 240));
        let box_of = |r: FRect| (r.x, r.y, r.w, r.h);
        assert_eq!(
            box_of(display_rect((640, 480), (640, 480), NORMAL)),
            (0.0, 0.0, 640.0, 480.0)
        );
        assert_eq!(
            box_of(display_rect((960, 540), (1920, 1080), NORMAL)),
            (0.0, 0.0, 1920.0, 1080.0)
        );
        // An odd output loses a pixel's part rather than its whole scale.
        let r = display_rect(ui_size((2560, 1441)), (2560, 1441), NORMAL);
        assert_eq!(box_of(r), (0.5, 0.0, 2559.0, 1440.0));
    }

    #[test]
    fn large_doubles_the_scale_down_to_320x240() {
        for (out, normal, large) in [
            ((640, 480), (640, 480), (320, 240)),
            ((1280, 720), (1280, 720), (640, 360)),
            ((1920, 1080), (960, 540), (480, 270)),
            // No smaller than the smallest UI the screens are laid out for.
            ((480, 272), (480, 272), (480, 272)),
            ((320, 240), (320, 240), (320, 240)),
            ((960, 544), (960, 544), (480, 272)),
        ] {
            assert_eq!(ui_size(out, NORMAL), normal, "{out:?}");
            assert_eq!(ui_size(out, LARGE), large, "{out:?}");
            // Either fills the output as the other does, so the program's
            // screen keeps its place.
            let place = |size| {
                let r = display_rect(ui_size(out, size), out, size);
                (r.x, r.y, r.w, r.h)
            };
            assert_eq!(place(NORMAL), place(LARGE), "{out:?}");
        }
        assert_eq!(ui_halves((640, 480), LARGE), 4);
        assert_eq!(ui_halves((1920, 1080), LARGE), 8);
    }

    #[test]
    fn medium_is_one_and_a_half_times_normal() {
        for (out, halves, medium) in [
            // 1.5 times: 24-pixel text on a 640x480 display.
            ((640, 480), 3, (427, 320)),
            ((1280, 720), 3, (854, 480)),
            ((800, 480), 3, (534, 320)),
            ((960, 544), 3, (640, 363)),
            // 4.5 times, where Normal's is 3.
            ((2560, 1440), 9, (569, 320)),
            // Where Normal's scale is even, 1.5 times it is whole.
            ((1920, 1080), 6, (640, 360)),
            // No smaller than 320x240: as Normal.
            ((480, 272), 2, (480, 272)),
            ((320, 240), 2, (320, 240)),
        ] {
            assert_eq!(ui_halves(out, MEDIUM), halves, "{out:?}");
            let ui = ui_size(out, MEDIUM);
            assert_eq!(ui, medium, "{out:?}");
            assert!(ui.0 >= SMALLEST_UI.0 && ui.1 >= SMALLEST_UI.1);
            let r = display_rect(ui, out, MEDIUM);
            let scale = halves as f32 / 2.0;
            if halves % 2 == 1 {
                // At the corner, scaled the same both ways, covering the
                // output and past it by less than a UI pixel.
                assert_eq!((r.x, r.y), (0.0, 0.0), "{out:?}");
                assert_eq!((r.w / ui.0 as f32, r.h / ui.1 as f32), (scale, scale));
                for (shown, out) in [(r.w, out.0), (r.h, out.1)] {
                    assert!(shown >= out as f32 && shown < out as f32 + scale);
                }
            } else {
                // A whole scale, laid out as Normal and Large are.
                let whole = (out.0 / ui.0).min(out.1 / ui.1);
                assert_eq!(r.w, (ui.0 * whole) as f32, "{out:?}");
            }
        }
        // Between Normal and Large, wherever the three differ.
        for out in [(640, 480), (1280, 720), (2560, 1440)] {
            let [n, m, l] = SimpleUiSize::ALL.map(|size| ui_halves(out, size));
            assert!(n < m && m < l, "{out:?}");
        }
    }

    #[test]
    fn auto_smooths_a_smaller_screen_and_keeps_pixels_sharp_otherwise() {
        let auto = |mode: &ScreenMode, shown| screen_filter(SimpleUiFilter::Auto, mode, shown);
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
            (SimpleUiFilter::PixelArt, Filter::SmartNearest),
            (SimpleUiFilter::Bilinear, Filter::Bilinear),
            (SimpleUiFilter::Nearest, Filter::Nearest),
        ] {
            for (mode, shown) in [(small, (640.0, 384.0)), (small, (400.0, 240.0))] {
                assert_eq!(screen_filter(choice, &mode, shown), filter);
            }
        }
        let mode_1 = ScreenMode {
            filter: Filter::SmartNearest,
            ..small
        };
        let program = |mode| screen_filter(SimpleUiFilter::Xscreen, mode, (640.0, 384.0));
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

    fn menu_settings() -> MenuSettings {
        MenuSettings::of(&Settings::default())
    }

    fn menu(running: bool, program: bool, restart: bool, size: (u32, u32)) -> PauseMenu {
        PauseMenu::new(running, program, restart, menu_settings(), size)
    }

    /// The rows `actions` and the settings under the divider.
    fn rows_of(actions: &[Choice]) -> Vec<Option<Choice>> {
        let settings = [None].into_iter().chain(SETTINGS.map(Some));
        actions.iter().copied().map(Some).chain(settings).collect()
    }

    #[test]
    fn the_pause_menu_offers_what_applies() {
        let running = menu(true, true, true, (640, 480));
        let all = [Choice::Resume, Choice::Restart, Choice::List, Choice::Quit];
        assert_eq!(running.rows, rows_of(&all));
        assert_eq!(running.label(Choice::Resume), "Resume");
        let unknown = menu(true, true, false, (640, 480));
        let actions = [Choice::Resume, Choice::List, Choice::Quit];
        assert_eq!(unknown.rows, rows_of(&actions));
        let list = menu(false, false, true, (640, 480));
        assert_eq!(list.rows, rows_of(&[Choice::Resume, Choice::Quit]));
        assert_eq!(list.label(Choice::Resume), "Close");
        for choice in SETTINGS {
            assert!(choice.is_setting());
            assert!(list.value(choice).is_some());
        }
        assert_eq!(list.value(Choice::Quit), None);
    }

    #[test]
    fn the_pause_menu_answers_the_pad_and_keys() {
        let mut menu = menu(true, true, true, (640, 480));
        assert_eq!(menu.pad(Button::DPadUp), None);
        assert_eq!(menu.pad(Button::DPadRight), None, "not on a setting");
        assert_eq!(menu.pad(Button::East), Some(Choice::Resume));
        menu.pad(Button::DPadDown);
        assert_eq!(menu.pad(Button::East), Some(Choice::Restart));
        menu.pad(Button::DPadDown);
        menu.pad(Button::DPadDown);
        assert_eq!(menu.key(Keycode::Return), Some(Choice::Quit));
        // Down goes past the divider, and no further than the last.
        menu.pad(Button::DPadDown);
        assert_eq!(menu.focused(), Choice::Filter);
        for _ in 0..5 {
            menu.pad(Button::DPadDown);
        }
        assert_eq!(menu.focused(), Choice::Swap);
        for _ in 0..3 {
            menu.key(Keycode::Up);
        }
        assert_eq!(menu.focused(), Choice::Quit, "back over the divider");
        // B and Esc close, wherever the focus is.
        assert_eq!(menu.pad(Button::South), Some(Choice::Resume));
        assert_eq!(menu.key(Keycode::Escape), Some(Choice::Resume));
        assert_eq!(menu.pad(Button::North), None);
        assert_eq!(menu.settings(), menu_settings(), "unchanged");
    }

    /// The menu with the focus on `choice`.
    fn focused_on(mut menu: PauseMenu, choice: Choice) -> PauseMenu {
        while menu.focused() != choice {
            menu.key(Keycode::Down);
        }
        menu
    }

    #[test]
    fn the_screen_filter_steps_both_ways_and_round() {
        let mut menu = focused_on(menu(false, false, false, (640, 480)), Choice::Filter);
        assert_eq!(menu.pad(Button::DPadRight), Some(Choice::Filter));
        assert_eq!(menu.settings().filter, SimpleUiFilter::PixelArt);
        assert_eq!(menu.key(Keycode::Left), Some(Choice::Filter));
        assert_eq!(menu.pad(Button::DPadLeft), Some(Choice::Filter));
        assert_eq!(menu.settings().filter, SimpleUiFilter::Xscreen, "round");
        // A steps on, and the menu stays.
        assert_eq!(menu.pad(Button::East), Some(Choice::Filter));
        assert_eq!(menu.settings().filter, SimpleUiFilter::Auto);
        let names: Vec<&str> = (0..5)
            .map(|_| {
                menu.key(Keycode::Right);
                filter_name(menu.settings().filter)
            })
            .collect();
        assert_eq!(names, ["Pixel art", "Smooth", "Sharp", "Program", "Auto"]);
    }

    #[test]
    fn ui_size_and_the_swap_step_as_the_filter_does() {
        let mut menu = focused_on(menu(true, true, true, (640, 480)), Choice::UiSize);
        assert_eq!(menu.pad(Button::DPadRight), Some(Choice::UiSize));
        assert_eq!(menu.settings().ui_size, MEDIUM);
        assert_eq!(menu.value(Choice::UiSize), Some("Medium"));
        assert_eq!(menu.pad(Button::East), Some(Choice::UiSize));
        assert_eq!(menu.settings().ui_size, LARGE);
        assert_eq!(menu.pad(Button::East), Some(Choice::UiSize));
        assert_eq!(menu.settings().ui_size, NORMAL, "round");
        assert_eq!(menu.key(Keycode::Left), Some(Choice::UiSize));
        assert_eq!(menu.value(Choice::UiSize), Some("Large"));
        assert_eq!(menu.key(Keycode::Left), Some(Choice::UiSize));
        assert_eq!(menu.value(Choice::UiSize), Some("Medium"));
        assert_eq!(menu.key(Keycode::Right), Some(Choice::UiSize));
        menu.key(Keycode::Down);
        for (step, on) in [(Button::East, true), (Button::DPadLeft, false)] {
            assert_eq!(menu.pad(step), Some(Choice::Swap));
            assert_eq!(menu.settings().swap, on);
        }
        assert_eq!(menu.pad(Button::DPadRight), Some(Choice::Swap));
        assert_eq!(menu.value(Choice::Swap), Some("On"));
        // What it changed goes into the settings, and nothing else.
        let mut settings = Settings::default();
        menu.settings().apply(&mut settings);
        let changed = Settings {
            simple_ui_size: LARGE,
            swap_ab_xy: true,
            ..Settings::default()
        };
        assert_eq!(settings, changed);
        assert_eq!(MenuSettings::of(&changed), menu.settings());
    }

    #[test]
    fn the_pause_menu_fits_a_small_screen() {
        for size in SIZES {
            for (running, program, restart) in [(true, true, true), (false, false, false)] {
                let mut menu = menu(running, program, restart, size);
                let f = menu.frame();
                assert!(f.x + f.w <= size.0 && f.y + f.h <= size.1, "{f:?}");
                // The window stays clear of the hints.
                assert!(f.y + f.h <= size.1 - LINE, "{size:?}");
                let shown = menu.shown_rows();
                let last = menu.item(f, shown - 1);
                assert!(last.y + last.h + PAD <= f.y + f.h);
                // Every row comes into sight as the focus goes down, the
                // divider with the first setting.
                for _ in 0..menu.rows.len() {
                    let at = menu.focus;
                    assert!(menu.scroll <= at && at < menu.scroll + shown, "{size:?}");
                    if menu.focused() == Choice::Filter {
                        assert!(menu.scroll < at, "the divider shows");
                    }
                    let mut fb = Framebuffer::new(size.0, size.1);
                    menu.draw(&mut fb, &Font::blank());
                    menu.key(Keycode::Down);
                }
                let all_shown = shown == menu.rows.len();
                assert_eq!(menu.thumb(f).is_none(), all_shown, "{size:?}");
                // A value's arrows and place stay right of its label, and
                // left of the thumb.
                let (left, value, right) = PauseMenu::value_places(menu.item(f, 0));
                for choice in SETTINGS {
                    assert!(left.x >= f.x + PAD + text_width(menu.label(choice)) + PAD);
                }
                assert!(value.x + value.w <= right.x && right.x + right.w < f.x + f.w);
                if let Some(thumb) = menu.thumb(f) {
                    assert!(right.x + right.w <= thumb.x, "{size:?}");
                }
            }
        }
        // Everything shows at 640x480; the menu over a program scrolls at
        // 320x240.
        let big = menu(true, true, true, (640, 480));
        assert_eq!(big.shown_rows(), big.rows.len());
        let small = menu(true, true, true, (320, 240));
        assert!(small.shown_rows() < small.rows.len());
    }

    #[test]
    fn the_screens_lay_out_at_each_size_on_640x480() {
        let tmp = TempDir::new("each-size");
        let s = storage(&tmp, 30);
        for (size, ui, rows) in [
            (NORMAL, (640, 480), 17),
            (MEDIUM, (427, 320), 10),
            (LARGE, (320, 240), 7),
        ] {
            assert_eq!(ui_size((640, 480), size), ui);
            let list = GameList::new(&s, ui, &Memory::default());
            assert_eq!(list.view.rows(), rows, "{size:?}");
            // The pause menu shows every row down to Medium.
            let menu = menu(true, true, true, ui);
            let all = menu.shown_rows() == menu.rows.len();
            assert_eq!(all, size != LARGE, "{size:?}");
            // The welcome screen wraps no line, with the name large.
            let welcome = Welcome::new(ui);
            assert_eq!(welcome.lines.len(), WELCOME_LINES.len(), "{size:?}");
            assert!(welcome.big_title && welcome.text_height() <= ui.1);
            // The hints keep the menu's.
            assert!(hints_shown(ui.0, &PROJECTS_HINTS) >= 3, "{size:?}");
        }
    }

    #[test]
    fn a_new_size_keeps_the_focus_in_sight() {
        let mut menu = focused_on(menu(true, true, true, (640, 480)), Choice::Swap);
        assert_eq!(menu.scroll, 0);
        menu.set_size((320, 240));
        assert!(menu.focus < menu.scroll + menu.shown_rows() && menu.scroll > 0);
        menu.set_size((640, 480));
        assert_eq!(menu.scroll, 0);
    }

    #[test]
    fn the_welcome_screen_takes_whichever_button_is_pressed_as_a() {
        for swap in [false, true] {
            // The right one is A: the setting goes off; the bottom one
            // turns it on. Either is read by its place, whatever the
            // setting made of it.
            for (placed, swapped) in [(Button::East, false), (Button::South, true)] {
                let mut w = Welcome::new((640, 480));
                let button = crate::controls::face(placed, swap);
                w.pad_down(button, swap);
                assert_eq!(w.pad_up(button, swap), Welcomed::Continue(Some(swapped)));
            }
        }
        // Enter goes on and leaves the setting alone.
        let w = Welcome::new((640, 480));
        assert_eq!(w.key(Keycode::Return), Welcomed::Continue(None));
        assert_eq!(w.key(Keycode::KpEnter), Welcomed::Continue(None));
        assert_eq!(w.key(Keycode::Escape), Welcomed::Stay);
    }

    #[test]
    fn the_welcome_screen_ignores_other_buttons_and_ones_held_before() {
        let mut w = Welcome::new((640, 480));
        // Held when it came: only the release is seen.
        assert_eq!(w.pad_up(Button::East, false), Welcomed::Stay);
        for other in [Button::North, Button::West, Button::DPadDown, Button::Start] {
            w.pad_down(other, false);
            assert_eq!(w.pad_up(other, false), Welcomed::Stay, "{other:?}");
        }
        // Two pressed together: the first counts, on its release.
        w.pad_down(Button::South, false);
        w.pad_down(Button::East, false);
        assert_eq!(w.pad_up(Button::East, false), Welcomed::Stay);
        assert_eq!(
            w.pad_up(Button::South, false),
            Welcomed::Continue(Some(true))
        );
    }

    #[test]
    fn the_welcome_screen_fits_a_small_screen() {
        for size in SIZES {
            let w = Welcome::new(size);
            assert!(w.text_height() <= size.1, "{size:?}");
            let room = size.0 - 2 * PAD;
            assert!(w.lines.iter().chain(&w.note).all(|l| text_width(l) <= room));
            // The name is large on all of them, 320 pixels across too.
            assert!(w.big_title, "{size:?}");
            let mut fb = Framebuffer::new(size.0, size.1);
            w.draw(&mut fb, &Font::blank());
        }
        let mut w = Welcome::new((320, 240));
        assert_eq!(w.lines.len(), WELCOME_LINES.len(), "no line wraps");
        w.set_size((200, 240));
        assert!(w.lines.len() > WELCOME_LINES.len());
        assert!(!w.big_title);
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
            // The row cuts it below 960 wide, and from 427 the info has
            // it all.
            let title = &view.titles[0];
            assert_eq!(title.ends_with('~'), size.0 < 960, "{title}");
            assert!(text_width(title) + 2 * PAD <= view.list.w);
            let info = &view.infos[0];
            let whole = info.title.concat() == "宇宙".repeat(12);
            // Two lines of twelve where they fit, from 427 wide.
            assert_eq!(whole, room >= 12 * CHAR, "{:?}", info.title);
            assert_eq!(whole, size.0 >= 427, "{:?}", info.title);
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

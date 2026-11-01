//! File > Settings: the emulator's settings by kind, each with a line on
//! what it does. Those toggled often stay in the menus as well.

use sb_runtime::Framebuffer;
use sb_runtime::confirm::FileConfirm;
use sb_runtime::font::{Font, FontSize};
use sb_runtime::settings::{Language, Settings};
use sdl3::keyboard::Keycode;

use crate::shortcuts::wrap;
use crate::ui::{
    BORDER, CHAR, DISABLED, HIGHLIGHT, LINE, PAD, Rect, TEXT, draw_text, fill, outline, window,
};

const WIDTH: u32 = 1100;
const HEIGHT: u32 = 620;
const PAGES_WIDTH: u32 = 180;
const HINT: &str = "Up/Down: choose  Tab: pages  Space, Left/Right: set  Esc: close";

/// How a setting is set.
enum Control {
    Check(fn(&Settings) -> bool, fn(&mut Settings)),
    /// One of the options, shown on a line of their own.
    Choice(
        &'static [&'static str],
        fn(&Settings) -> usize,
        fn(&mut Settings, usize),
    ),
}

struct Row {
    label: &'static str,
    about: &'static str,
    control: Control,
}

struct Page {
    title: &'static str,
    rows: &'static [Row],
}

const fn check(
    label: &'static str,
    about: &'static str,
    get: fn(&Settings) -> bool,
    flip: fn(&mut Settings),
) -> Row {
    Row {
        label,
        about,
        control: Control::Check(get, flip),
    }
}

/// The instruments row's option that asks for a SoundFont file.
const SOUND_FONT_FILE: usize = 2;
const INSTRUMENTS: &str = "Instruments";

const PAGES: &[Page] = &[
    Page {
        title: "General",
        rows: &[
            check(
                "Upper-case letters",
                "Keys type capitals unless Shift or Caps Lock is on, as on SmileBASIC 4. \
                 Also in the Edit menu.",
                |s| s.upper_case_letters,
                |s| s.upper_case_letters ^= true,
            ),
            check(
                "Ctrl+C stops program",
                "Ctrl+C stops a running program, besides F5 and Pause/Break.",
                |s| s.ctrl_c_stops,
                |s| s.ctrl_c_stops ^= true,
            ),
            check(
                "Localized error messages",
                "Print a line explaining each error message under it, as SmileBASIC \
                 4's Japanese edition does. The line is in Japanese only.",
                |s| s.localized_errors,
                |s| s.localized_errors ^= true,
            ),
            Row {
                label: "System language",
                about: "What SYSPARAM(\"LANG\") tells programs: en-US as the English \
                        edition, or ja.",
                control: Control::Choice(
                    &["English (en-US)", "Japanese (ja)"],
                    |s| usize::from(s.language == Language::Japanese),
                    |s, i| {
                        s.language = match i {
                            0 => Language::English,
                            _ => Language::Japanese,
                        }
                    },
                ),
            },
            Row {
                label: "Menu and window font",
                about: "The emulator's menus and windows draw SmileBASIC's 8-pixel font \
                        at twice its size, or the 16-pixel one.",
                control: Control::Choice(
                    &["8 pixels", "16 pixels"],
                    |s| usize::from(s.ui_font == FontSize::Large),
                    |s, i| {
                        s.ui_font = match i {
                            0 => FontSize::Small,
                            _ => FontSize::Large,
                        }
                    },
                ),
            },
        ],
    },
    Page {
        title: "Input",
        rows: &[
            check(
                "Keys press buttons",
                "The keyboard also presses controller 1's buttons. Also in the Run menu.",
                |s| s.keyboard_buttons,
                |s| s.keyboard_buttons ^= true,
            ),
            check(
                "Always show mouse cursor",
                "Keep the computer's cursor shown over the window when a program \
                 hides it with SYSPARAM \"MOUSECURSOR\".",
                |s| s.keep_mouse_cursor,
                |s| s.keep_mouse_cursor ^= true,
            ),
        ],
    },
    Page {
        title: "Files",
        rows: &[
            check(
                "Half-width on SAVE",
                "SAVE turns full-width letters and digits into ASCII.",
                |s| s.half_width_on_save,
                |s| s.half_width_on_save ^= true,
            ),
            Row {
                label: "File access",
                about: "Whether file commands ask first, as on SmileBASIC 4, go ahead \
                        with a notice, or go ahead silently.",
                control: Control::Choice(
                    &["Always ask", "Notify", "Silent"],
                    |s| s.file_confirm as usize,
                    |s, i| {
                        s.file_confirm = match i {
                            0 => FileConfirm::Ask,
                            1 => FileConfirm::Notify,
                            _ => FileConfirm::Silent,
                        }
                    },
                ),
            },
        ],
    },
    Page {
        title: "Sound",
        rows: &[Row {
            label: INSTRUMENTS,
            about: "What plays MML and BGM: SmileBASIC 4's own instruments (imported \
                    with smilebasic-import), the bundled GeneralUser GS SoundFont, or a \
                    SoundFont file.",
            control: Control::Choice(
                &["SmileBASIC 4", "GeneralUser GS", "File..."],
                |s| match (s.smilebasic_instruments, &s.sound_font) {
                    (true, _) => 0,
                    (false, None) => 1,
                    (false, Some(_)) => SOUND_FONT_FILE,
                },
                |s, i| match i {
                    0 => s.smilebasic_instruments = true,
                    1 => {
                        s.smilebasic_instruments = false;
                        s.sound_font = None;
                    }
                    // The frontend asks for the file.
                    _ => {}
                },
            ),
        }],
    },
    Page {
        title: "Editor",
        rows: &[
            Row {
                label: "Tab width",
                about: "How far Tab moves in the editor; programs read it as \
                        SYSPARAM(\"TABSTEP\").",
                control: Control::Choice(
                    &["1", "2", "4", "8"],
                    |s| match s.editor_tab_step {
                        1 => 0,
                        2 => 1,
                        4 => 2,
                        _ => 3,
                    },
                    |s, i| s.editor_tab_step = [1, 2, 4, 8][i],
                ),
            },
            check(
                "Editor in its own window",
                "The editor opens in a window of its own beside the screen, \
                 and programs go on running while it is open.",
                |s| s.editor_window,
                |s| s.editor_window ^= true,
            ),
            check(
                "Complete words in direct mode",
                "A list of words that fit what is typed shows under it.",
                |s| s.complete_direct,
                |s| s.complete_direct ^= true,
            ),
            check(
                "Complete words in the editor",
                "The same in the editor.",
                |s| s.complete_editor,
                |s| s.complete_editor ^= true,
            ),
            Row {
                label: "Letters before the list shows",
                about: "How many letters to type before the list shows. After GOTO, @ \
                        and # it shows at once.",
                control: Control::Choice(
                    &["1", "2", "3", "4"],
                    |s| s.complete_after as usize - 1,
                    |s, i| s.complete_after = i as u32 + 1,
                ),
            },
            check(
                "SmileBASIC's words",
                "The list offers SmileBASIC's instructions, functions and reserved words.",
                |s| s.complete_commands,
                |s| s.complete_commands ^= true,
            ),
            check(
                "Names in the program",
                "The list also offers the program's variables, labels, constants and DEFs.",
                |s| s.complete_names,
                |s| s.complete_names ^= true,
            ),
            check(
                "Enter takes a picked word",
                "Once Up or Down picks a word, Enter puts it in, as Tab always does.",
                |s| s.complete_enter,
                |s| s.complete_enter ^= true,
            ),
            check(
                "Show how commands are used",
                "While a command's arguments are typed, the forms the help gives for it \
                 show, with the argument being typed marked. Ctrl+Space hides them.",
                |s| s.complete_hints,
                |s| s.complete_hints ^= true,
            ),
            check(
                "Show them by the cursor",
                "The forms show by the cursor rather than at the bottom of the screen.",
                |s| s.complete_above,
                |s| s.complete_above ^= true,
            ),
        ],
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
    /// The instruments changed; the frontend loads them.
    Instruments,
    /// Ask for a SoundFont file to play with.
    ChooseSoundFont,
}

#[derive(Clone, Copy)]
pub struct SettingsWindow {
    size: (u32, u32),
    page: usize,
    chosen: usize,
    /// Up and Down move in the pages rather than in the settings.
    in_pages: bool,
}

impl SettingsWindow {
    pub fn new(size: (u32, u32)) -> SettingsWindow {
        SettingsWindow {
            size,
            page: 0,
            chosen: 0,
            in_pages: false,
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

    fn pages_rect(&self) -> Rect {
        let f = self.frame();
        Rect {
            x: f.x + 2 * PAD,
            y: f.y + LINE + 2 * PAD,
            w: PAGES_WIDTH,
            // Under it: what the chosen setting does, and the hint.
            h: f.h - 6 * LINE - 5 * PAD,
        }
    }

    fn page_row(&self, i: usize) -> Rect {
        let p = self.pages_rect();
        Rect {
            y: p.y + i as u32 * LINE,
            h: LINE,
            ..p
        }
    }

    fn rows_rect(&self) -> Rect {
        let f = self.frame();
        let p = self.pages_rect();
        let x = p.x + p.w + 2 * PAD;
        Rect {
            x,
            w: f.x + f.w - 2 * PAD - x,
            ..p
        }
    }

    fn row_height(row: &Row) -> u32 {
        match row.control {
            Control::Check(..) => LINE,
            Control::Choice(..) => 2 * LINE,
        }
    }

    fn rows(&self) -> &'static [Row] {
        PAGES[self.page].rows
    }

    fn row_rect(&self, i: usize) -> Rect {
        let r = self.rows_rect();
        let rows = self.rows();
        let y = r.y + rows[..i].iter().map(Self::row_height).sum::<u32>();
        Rect {
            y,
            h: Self::row_height(&rows[i]),
            ..r
        }
    }

    /// Option `i` of a choice row, on its second line.
    fn option_rect(&self, row: usize, i: usize) -> Rect {
        let r = self.row_rect(row);
        let Control::Choice(options, ..) = self.rows()[row].control else {
            unreachable!("only choices have options")
        };
        let x = r.x
            + 4 * CHAR
            + (options[..i].iter())
                .map(|o| (o.chars().count() as u32 + 6) * CHAR)
                .sum::<u32>();
        Rect {
            x,
            y: r.y + LINE,
            w: (options[i].chars().count() as u32 + 4) * CHAR,
            h: LINE,
        }
    }

    fn open_page(&mut self, page: usize) {
        self.page = page;
        self.chosen = 0;
    }

    /// Sets row `row`: flips a check, or picks option `pick` of a choice
    /// (the next one if `None`, the one before if `back`).
    fn set(&mut self, row: usize, pick: Option<usize>, back: bool, s: &mut Settings) -> Outcome {
        let row = &self.rows()[row];
        match row.control {
            Control::Check(_, flip) => flip(s),
            Control::Choice(options, get, put) => {
                let n = options.len();
                let at = get(s);
                let to = pick.unwrap_or(if back { (at + n - 1) % n } else { (at + 1) % n });
                if row.label == INSTRUMENTS {
                    if to == SOUND_FONT_FILE {
                        return Outcome::ChooseSoundFont;
                    }
                    put(s, to);
                    return Outcome::Instruments;
                }
                put(s, to);
            }
        }
        Outcome::Stay
    }

    pub fn key(&mut self, key: Keycode, settings: &mut Settings) -> Outcome {
        let (pages, rows) = (PAGES.len(), self.rows().len());
        match key {
            Keycode::Escape => return Outcome::Close,
            Keycode::Tab => self.in_pages = !self.in_pages,
            Keycode::Up if self.in_pages => self.open_page((self.page + pages - 1) % pages),
            Keycode::Down if self.in_pages => self.open_page((self.page + 1) % pages),
            Keycode::Up => self.chosen = (self.chosen + rows - 1) % rows,
            Keycode::Down => self.chosen = (self.chosen + 1) % rows,
            Keycode::Right if self.in_pages => self.in_pages = false,
            Keycode::Left | Keycode::Right
                if matches!(self.rows()[self.chosen].control, Control::Choice(..)) =>
            {
                return self.set(self.chosen, None, key == Keycode::Left, settings);
            }
            Keycode::Space | Keycode::Return | Keycode::KpEnter if !self.in_pages => {
                return self.set(self.chosen, None, false, settings);
            }
            Keycode::Return | Keycode::KpEnter => self.in_pages = false,
            _ => {}
        }
        Outcome::Stay
    }

    pub fn mouse_move(&mut self, at: (i32, i32)) {
        if let Some(i) = (0..self.rows().len()).find(|&i| self.row_rect(i).contains(at)) {
            self.chosen = i;
            self.in_pages = false;
        }
    }

    /// A click on a page opens it; on a check flips it; on an option picks
    /// it. One outside the window closes it.
    pub fn click(&mut self, at: (i32, i32), settings: &mut Settings) -> Outcome {
        if !self.frame().contains(at) {
            return Outcome::Close;
        }
        if let Some(i) = (0..PAGES.len()).find(|&i| self.page_row(i).contains(at)) {
            self.open_page(i);
            self.in_pages = true;
            return Outcome::Stay;
        }
        let Some(row) = (0..self.rows().len()).find(|&i| self.row_rect(i).contains(at)) else {
            return Outcome::Stay;
        };
        self.chosen = row;
        match self.rows()[row].control {
            Control::Check(..) => self.set(row, None, false, settings),
            Control::Choice(options, ..) => {
                match (0..options.len()).find(|&i| self.option_rect(row, i).contains(at)) {
                    Some(i) => self.set(row, Some(i), false, settings),
                    None => Outcome::Stay,
                }
            }
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, settings: &Settings) {
        let f = self.frame();
        window(fb, font, f, "Settings");
        for (i, page) in PAGES.iter().enumerate() {
            let r = self.page_row(i);
            if i == self.page {
                fill(fb, r, if self.in_pages { HIGHLIGHT } else { BORDER });
            }
            draw_text(fb, font, r.x + PAD, r.y + 4, page.title, TEXT);
        }
        outline(fb, self.pages_rect(), BORDER);
        for (i, row) in self.rows().iter().enumerate() {
            let r = self.row_rect(i);
            if i == self.chosen && !self.in_pages {
                fill(fb, Rect { h: LINE, ..r }, HIGHLIGHT);
            }
            let label = match row.control {
                Control::Check(get, _) => {
                    let mark = if get(settings) { "[x]" } else { "[ ]" };
                    format!("{mark} {}", row.label)
                }
                Control::Choice(..) => format!("    {}", row.label),
            };
            draw_text(fb, font, r.x + PAD, r.y + 4, &label, TEXT);
            if let Control::Choice(options, get, _) = row.control {
                let at = get(settings);
                for (j, option) in options.iter().enumerate() {
                    let o = self.option_rect(i, j);
                    let mark = if j == at { "(*)" } else { "( )" };
                    let text = format!("{mark} {option}");
                    draw_text(fb, font, o.x, o.y + 4, &text, TEXT);
                }
            }
        }
        // What the chosen setting does.
        let rows = self.rows_rect();
        let about = self.rows()[self.chosen].about;
        let width = (f.w - 4 * PAD) / CHAR;
        let top = rows.y + rows.h + PAD;
        for (i, line) in wrap(about, width as usize).iter().take(3).enumerate() {
            let y = top + i as u32 * LINE + 4;
            draw_text(fb, font, f.x + 2 * PAD, y, line, TEXT);
        }
        let y = f.y + f.h - LINE - PAD + 4;
        draw_text(fb, font, f.x + 2 * PAD, y, HINT, DISABLED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centre(r: Rect) -> (i32, i32) {
        ((r.x + r.w / 2) as i32, (r.y + r.h / 2) as i32)
    }

    #[test]
    fn keys_and_clicks_set() {
        let mut s = Settings::default();
        let mut w = SettingsWindow::new((1280, 720));
        // General: Space flips Upper-case letters.
        w.key(Keycode::Space, &mut s);
        assert!(!s.upper_case_letters);
        // Up goes round to the last row, the UI font; Left on the
        // language goes round.
        w.key(Keycode::Up, &mut s);
        w.key(Keycode::Left, &mut s);
        assert_eq!(s.ui_font, FontSize::Small);
        w.key(Keycode::Up, &mut s);
        w.key(Keycode::Left, &mut s);
        assert_eq!(s.language, Language::Japanese);
        // Tab to the pages, down to Files, and click Silent.
        w.key(Keycode::Tab, &mut s);
        w.key(Keycode::Down, &mut s);
        w.key(Keycode::Down, &mut s);
        assert_eq!(PAGES[w.page].title, "Files");
        let silent = w.option_rect(1, 2);
        assert_eq!(w.click(centre(silent), &mut s), Outcome::Stay);
        assert_eq!(s.file_confirm, FileConfirm::Silent);
        assert_eq!(w.chosen, 1);
        assert_eq!(w.click((0, 0), &mut s), Outcome::Close);
    }

    #[test]
    fn instruments_tell_the_frontend() {
        let mut s = Settings::default();
        let mut w = SettingsWindow::new((1280, 720));
        let sound = PAGES.iter().position(|p| p.title == "Sound").unwrap();
        w.click(centre(w.page_row(sound)), &mut s);
        assert_eq!(
            w.click(centre(w.option_rect(0, 0)), &mut s),
            Outcome::Instruments
        );
        assert!(s.smilebasic_instruments);
        assert_eq!(
            w.click(centre(w.option_rect(0, SOUND_FONT_FILE)), &mut s),
            Outcome::ChooseSoundFont
        );
        assert!(s.smilebasic_instruments, "unchanged until a file is chosen");
    }

    #[test]
    fn everything_fits() {
        let w = SettingsWindow::new((1280, 720));
        for (page, p) in PAGES.iter().enumerate() {
            let mut w = SettingsWindow { page, ..w };
            w.chosen = 0;
            let last = w.row_rect(p.rows.len() - 1);
            let rows = w.rows_rect();
            assert!(last.y + last.h <= rows.y + rows.h, "{}", p.title);
            for (i, row) in p.rows.iter().enumerate() {
                if let Control::Choice(options, ..) = row.control {
                    let o = w.option_rect(i, options.len() - 1);
                    assert!(o.x + o.w <= rows.x + rows.w, "{}", row.label);
                }
            }
        }
    }
}

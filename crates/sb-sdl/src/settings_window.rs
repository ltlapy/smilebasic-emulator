//! File > Settings: the emulator's settings by kind, each with a line on
//! what it does. Those toggled often stay in the menus as well.

use sb_runtime::Framebuffer;
use sb_runtime::confirm::FileConfirm;
use sb_runtime::extra_glyphs::FontSource;
use sb_runtime::font::Font;
use sb_runtime::settings::{Language, Renderer, Settings, TOOLS, UiFont};
use sb_runtime::speed::Speed;
use sdl3::keyboard::Keycode;

use crate::shortcuts::wrap;
use crate::ui::{
    BORDER, CHAR, DISABLED, HIGHLIGHT, LINE, PAD, Rect, TEXT, button, draw_text, fill, outline,
    text_width, window,
};

const WIDTH: u32 = 1100;
const HEIGHT: u32 = 620;
const PAGES_WIDTH: u32 = 180;
/// Lines under the settings for what the chosen one does.
const ABOUT_LINES: usize = 3;
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
    /// The program F9, F10 or F11 (0 to 2) starts, with buttons under it
    /// to choose one, set none or go back to the default.
    Tool(usize),
}

/// A tool row's buttons, in the order Left and Right go through them.
const TOOL_BUTTONS: [&str; 3] = ["Choose...", "None", "Default"];
const CHOOSE: usize = 0;
const NONE: usize = 1;
const DEFAULT: usize = 2;

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

const fn tool(n: usize) -> Row {
    Row {
        label: ["F9 tool", "F10 tool", "F11 tool"][n],
        about: [
            "The program F9 starts as a subprogram, also while a program runs. \
             SYSPARAM(\"TOOL1\") returns it. Default: #TOOL/SBSMILE.PRG \
             (SMILETOOL). Left/Right: pick a button, Delete: None.",
            "The program F10 starts as a subprogram, also while a program runs. \
             SYSPARAM(\"TOOL2\") returns it. Default: #TOOL/GAHAKU.PRG \
             (GAHAKU). Left/Right: pick a button, Delete: None.",
            "The program F11 starts as a subprogram, also while a program runs. \
             SYSPARAM(\"TOOL3\") returns it. Default: none. Left/Right: pick a \
             button, Delete: None.",
        ][n],
        control: Control::Tool(n),
    }
}

/// The tool rows' labels are padded to this width, so that their colons
/// line up.
const TOOL_LABEL_WIDTH: usize = 8;

/// What a tool row shows for the program set.
fn tool_label(file: &str) -> &str {
    match file {
        "" => "(none)",
        file => file,
    }
}

/// The instruments row's option that asks for a SoundFont file.
const SOUND_FONT_FILE: usize = 2;
const INSTRUMENTS: &str = "Instruments for MML and BGM";
/// The extra font row's option that asks for a font file.
const EXTRA_FONT_FILE: usize = 1;
const EXTRA_FONT: &str = "Font file";
const PROGRAM_FONT: [&str; 2] = ["Programs' 8-pixel font", "Programs' 16-pixel font"];
/// The font sources, in the options' order.
const SOURCES: [FontSource; 3] = [
    FontSource::SmileBasic,
    FontSource::File,
    FontSource::Bundled,
];
const SOURCE_NAMES: &[&str] = &["SmileBASIC", "File", "Bundled"];

fn source_index(source: FontSource) -> usize {
    SOURCES.iter().position(|&s| s == source).unwrap_or(0)
}

/// A row choosing the 8- (0) or 16-pixel (1) font page's source.
const fn program_font(n: usize) -> Row {
    Row {
        label: PROGRAM_FONT[n],
        about: "Puts the file's or the bundled fonts' glyphs on the font page (GRP5) \
                for the characters SmileBASIC's font has, so programs draw with them \
                and read them from GRP5. Set now and by ACLS. Not SmileBASIC 4's. \
                Default: SmileBASIC.",
        control: Control::Choice(
            SOURCE_NAMES,
            [
                |s: &Settings| source_index(s.program_font[0]),
                |s: &Settings| source_index(s.program_font[1]),
            ][n],
            [
                |s: &mut Settings, i: usize| s.program_font[0] = SOURCES[i],
                |s: &mut Settings, i: usize| s.program_font[1] = SOURCES[i],
            ][n],
        ),
    }
}

const PAGES: &[Page] = &[
    Page {
        title: "General",
        rows: &[
            check(
                "Type capitals without Shift",
                "Letter keys type capitals, and Shift or Caps Lock types lower case, \
                 as SmileBASIC 4's keyboard does. On by default. Also in the Edit \
                 menu.",
                |s| s.upper_case_letters,
                |s| s.upper_case_letters ^= true,
            ),
            check(
                "Ctrl+C stops the running program",
                "Ctrl+C stops a running program, as Pause/Break and F5 do. Off by \
                 default, so that programs can read Ctrl+C as a key. In the editor \
                 Ctrl+C still copies.",
                |s| s.ctrl_c_stops,
                |s| s.ctrl_c_stops ^= true,
            ),
            check(
                "Japanese explanation under errors",
                "Under each error message, also print the Japanese line that the \
                 Japanese edition of SmileBASIC 4 shows for that error. Off by \
                 default.",
                |s| s.localized_errors,
                |s| s.localized_errors ^= true,
            ),
            check(
                "Allow system-only features in every program",
                "Features SmileBASIC 4 keeps for programs started from #SYS work in \
                 any program: HELPINFO OUT gives the command at the editor's cursor \
                 instead of DEFAULTHELPKEY. Off by default, as on hardware.",
                |s| s.helpinfo_cursor,
                |s| s.helpinfo_cursor ^= true,
            ),
            Row {
                label: "Speed",
                about: "Unlimited runs programs as fast as this computer can (default). \
                        Switch 2 and Switch 1 give them a frame's work as on that \
                        console: a program too slow there misses frames here too.",
                control: Control::Choice(
                    &["Unlimited", "Switch 2", "Switch 1"],
                    |s| Speed::ALL.iter().position(|&v| v == s.speed).unwrap_or(0),
                    |s, i| s.speed = Speed::ALL[i],
                ),
            },
            Row {
                label: "Language reported to programs",
                about: "The value SYSPARAM(\"LANG\") returns: \"en-US\" as in the \
                        English edition (default), or \"ja\" as in the Japanese one. \
                        The emulator's own menus stay in English.",
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
        ],
    },
    Page {
        title: "Input",
        rows: &[
            check(
                "Use the keyboard as a controller",
                "Keys also press controller 1's buttons for BUTTON and the like: \
                 cursor keys, Z X A S for A B Y X, Q W for L R, 1 2 for ZL ZR. Not \
                 a key the program reads with KEYBOARD or INKEY$. On by default.",
                |s| s.keyboard_buttons,
                |s| s.keyboard_buttons ^= true,
            ),
            check(
                "Ignore mouse cursor commands",
                "The cursor stays visible even while a program hides it with SYSPARAM \
                 \"MOUSECURSOR\",0, and stays where it is when a program moves it \
                 with MOUSE X,Y. Off by default.",
                |s| s.keep_mouse_cursor,
                |s| s.keep_mouse_cursor ^= true,
            ),
        ],
    },
    Page {
        title: "Files",
        rows: &[
            check(
                "SAVE turns full-width letters into ASCII",
                "When SAVE writes a program, full-width letters A-Z, a-z and digits \
                 0-9 become plain ASCII ones. Other full-width characters are kept. \
                 On by default.",
                |s| s.half_width_on_save,
                |s| s.half_width_on_save ^= true,
            ),
            Row {
                label: "Before file commands such as SAVE and LOAD",
                about: "Ask first: a Yes/No question, as SmileBASIC 4 asks (default). \
                        Show a notice: go ahead and say what was done. Silent: just go \
                        ahead. Losing unsaved edits always asks.",
                control: Control::Choice(
                    &["Ask first", "Show a notice", "Silent"],
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
        title: "Tools",
        rows: &[tool(0), tool(1), tool(2)],
    },
    Page {
        title: "Sound",
        rows: &[Row {
            label: INSTRUMENTS,
            about: "The sounds BGMPLAY's MML and built-in songs play with: SmileBASIC \
                    4's own instruments (default; import them with smilebasic-import), \
                    the bundled GeneralUser GS SoundFont, or a .sf2 file you choose.",
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
            check(
                "Narrow half-width characters",
                "The editor draws ASCII and half-width kana half as wide as other \
                 characters. Each character still takes one column, so lines look \
                 shorter than in SmileBASIC 4 and text that mixes half-width and \
                 full-width characters no longer lines up. Off by default.",
                |s| s.editor_half_width,
                |s| s.editor_half_width ^= true,
            ),
            Row {
                label: "Tab width",
                about: "Tab in the editor pads with spaces to the next multiple of this \
                        many columns. SYSPARAM(\"TABSTEP\") returns it. Default 1, as \
                        the editor's characters are already wide.",
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
                "Open the editor in a separate window",
                "F4 opens the editor in a second window beside the screen instead of \
                 in place of it, so the screen stays visible and programs and direct \
                 mode keep running. Also in the View menu.",
                |s| s.editor_window,
                |s| s.editor_window ^= true,
            ),
            check(
                "Remember the editor window's position and size",
                "The separate editor window opens where it was last, at the same \
                 size, if that place is still on a display. Off: it opens beside the \
                 screen each time. Off by default.",
                |s| s.editor_window_keep_place,
                |s| s.editor_window_keep_place ^= true,
            ),
            check(
                "Inline suggestions in direct mode",
                "While you type a line in direct mode, a list of words that start \
                 with what is typed shows under the cursor; Tab puts the first in. \
                 On by default.",
                |s| s.complete_direct,
                |s| s.complete_direct ^= true,
            ),
            check(
                "Inline suggestions in the editor",
                "The same list of suggestions while you type in the editor. On by \
                 default.",
                |s| s.complete_editor,
                |s| s.complete_editor ^= true,
            ),
            Row {
                label: "Minimum letters to show suggestions",
                about: "How many letters of a word to type before suggestions show. \
                        Default 1, as in SmileBASIC 4. After GOTO, GOSUB, RESTORE, a \
                        lone @ or # they show at once.",
                control: Control::Choice(
                    &["1", "2", "3", "4"],
                    |s| s.complete_after as usize - 1,
                    |s, i| s.complete_after = i as u32 + 1,
                ),
            },
            check(
                "Suggest SmileBASIC instructions",
                "Suggestions include SmileBASIC 4's instructions, functions and \
                 reserved words, such as PRINT, RND and ENDIF. On by default.",
                |s| s.complete_commands,
                |s| s.complete_commands ^= true,
            ),
            check(
                "Suggest names from the edited program",
                "Suggestions also include the variables, DEFs and labels written in \
                 the slot open in the editor, also in direct mode. Off by default. \
                 Labels after GOTO and # constants show either way.",
                |s| s.complete_names,
                |s| s.complete_names ^= true,
            ),
            check(
                "Suggest from all slots",
                "With the option above, the names of every slot's program are \
                 suggested too, after those of the slot being edited. Off by \
                 default.",
                |s| s.complete_all_slots,
                |s| s.complete_all_slots ^= true,
            ),
            check(
                "Enter accepts the chosen suggestion",
                "In the editor, after Up or Down chooses a suggestion, Enter puts it \
                 in instead of a new line. Off: only Tab does. In direct mode Up and \
                 Down recall history, and Tab puts in the first.",
                |s| s.complete_enter,
                |s| s.complete_enter ^= true,
            ),
            check(
                "Show command usage",
                "While you type a command's or DEF's arguments, show its forms from \
                 the help, with the argument being typed marked; also under the \
                 suggestions. Ctrl+Space hides them.",
                |s| s.complete_hints,
                |s| s.complete_hints ^= true,
            ),
            check(
                "Show command usage next to the cursor",
                "Command usage shows next to the cursor instead of at the bottom of \
                 the screen (or the top while the cursor is down there). Off by \
                 default.",
                |s| s.complete_above,
                |s| s.complete_above ^= true,
            ),
        ],
    },
    Page {
        title: "Fonts",
        rows: &[
            Row {
                label: "Menus and windows",
                about: "The font of the menu bar, this window and the emulator's other \
                        windows (not the SmileBASIC screen): the bundled 8-pixel fonts at \
                        twice their size (GalmuriMono7 first), SmileBASIC's 16-pixel font \
                        (default), or smooth outlines with soft edges (M PLUS 1p Bold and \
                        M PLUS Code Latin 50 Bold).",
                control: Control::Choice(
                    &["8-pixel, doubled", "16-pixel", "Smooth"],
                    |s| match s.ui_font {
                        UiFont::Doubled => 0,
                        UiFont::Large => 1,
                        UiFont::Smooth => 2,
                    },
                    |s, i| {
                        s.ui_font = match i {
                            0 => UiFont::Doubled,
                            1 => UiFont::Large,
                            _ => UiFont::Smooth,
                        }
                    },
                ),
            },
            Row {
                label: EXTRA_FONT,
                about: "A BDF, TTF or OTF file to draw the Unicode characters SmileBASIC's \
                        font lacks. The rows below set where it comes in the order. \
                        Default: none.",
                control: Control::Choice(
                    &["None", "File..."],
                    |s| match s.editor_extra_font {
                        None => 0,
                        Some(_) => EXTRA_FONT_FILE,
                    },
                    |s, i| {
                        if i == 0 {
                            s.editor_extra_font = None;
                        }
                    },
                ),
            },
            Row {
                label: "Editor's first font",
                about: "Where the editor looks for a character first, in the code, line \
                        numbers, status line, suggestions and hints. A character it lacks \
                        comes from the others in the order SmileBASIC, file, bundled, then \
                        the box. Default: SmileBASIC.",
                control: Control::Choice(
                    SOURCE_NAMES,
                    |s| source_index(s.editor_font_first),
                    |s, i| s.editor_font_first = SOURCES[i],
                ),
            },
            program_font(0),
            program_font(1),
        ],
    },
    Page {
        title: "Screen",
        rows: &[Row {
            label: "Renderer",
            about: "Auto uses GPU rendering if the start-up test passes. CPU is exact \
                    but slower. Help > About shows which is in use.",
            control: Control::Choice(
                &["Auto", "CPU"],
                |s| match s.renderer {
                    Renderer::Auto => 0,
                    Renderer::Cpu => 1,
                },
                |s, i| {
                    s.renderer = match i {
                        0 => Renderer::Auto,
                        _ => Renderer::Cpu,
                    }
                },
            ),
        }],
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
    /// The editor's extra font changed; the frontend loads it.
    ExtraFont,
    /// Ask for a font file for the editor's extra characters.
    ChooseExtraFont,
    /// A program font changed; the frontend writes it on the font page.
    ProgramFont,
    /// Ask for the program tool `n` (0 to 2) starts.
    ChooseTool(usize),
}

#[derive(Clone, Copy)]
pub struct SettingsWindow {
    size: (u32, u32),
    page: usize,
    chosen: usize,
    /// Up and Down move in the pages rather than in the settings.
    in_pages: bool,
    /// The chosen tool row's button.
    button: usize,
}

impl SettingsWindow {
    pub fn new(size: (u32, u32)) -> SettingsWindow {
        SettingsWindow {
            size,
            page: 0,
            chosen: 0,
            in_pages: false,
            button: CHOOSE,
        }
    }

    /// Shows the page titled `title`, for the bench.
    pub fn show_page(&mut self, title: &str) {
        self.page = PAGES.iter().position(|p| p.title == title).unwrap_or(0);
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
            Control::Choice(..) | Control::Tool(_) => 2 * LINE,
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

    /// Option `i` of a choice row, or button `i` of a tool row, on its
    /// second line.
    fn option_rect(&self, row: usize, i: usize) -> Rect {
        let r = self.row_rect(row);
        let options = match self.rows()[row].control {
            Control::Choice(options, ..) => options,
            Control::Tool(_) => &TOOL_BUTTONS,
            Control::Check(..) => unreachable!("only choices and tools have options"),
        };
        let x = r.x
            + 4 * CHAR
            + (options[..i].iter())
                .map(|o| text_width(o) + 6 * CHAR)
                .sum::<u32>();
        Rect {
            x,
            y: r.y + LINE,
            w: text_width(options[i]) + 4 * CHAR,
            h: LINE,
        }
    }

    fn open_page(&mut self, page: usize) {
        self.page = page;
        self.chosen = 0;
        self.button = CHOOSE;
    }

    fn choose(&mut self, row: usize) {
        if row != self.chosen {
            self.button = CHOOSE;
        }
        self.chosen = row;
    }

    /// Sets row `row`: flips a check, picks option `pick` of a choice
    /// (the next one if `None`, the one before if `back`), or presses
    /// button `pick` of a tool row (the chosen one if `None`).
    fn set(&mut self, row: usize, pick: Option<usize>, back: bool, s: &mut Settings) -> Outcome {
        let row = &self.rows()[row];
        match row.control {
            Control::Check(_, flip) => flip(s),
            Control::Tool(n) => match pick.unwrap_or(self.button) {
                CHOOSE => return Outcome::ChooseTool(n),
                NONE => s.tools[n].clear(),
                DEFAULT => s.tools[n] = TOOLS[n].to_string(),
                _ => {}
            },
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
                if row.label == EXTRA_FONT {
                    if to == EXTRA_FONT_FILE {
                        return Outcome::ChooseExtraFont;
                    }
                    put(s, to);
                    return Outcome::ExtraFont;
                }
                put(s, to);
                if PROGRAM_FONT.contains(&row.label) {
                    return Outcome::ProgramFont;
                }
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
            Keycode::Up => self.choose((self.chosen + rows - 1) % rows),
            Keycode::Down => self.choose((self.chosen + 1) % rows),
            Keycode::Right if self.in_pages => self.in_pages = false,
            Keycode::Left | Keycode::Right
                if matches!(self.rows()[self.chosen].control, Control::Tool(_)) =>
            {
                let n = TOOL_BUTTONS.len();
                let step = if key == Keycode::Left { n - 1 } else { 1 };
                self.button = (self.button + step) % n;
            }
            Keycode::Delete | Keycode::Backspace
                if matches!(self.rows()[self.chosen].control, Control::Tool(_)) =>
            {
                return self.set(self.chosen, Some(NONE), false, settings);
            }
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
            self.choose(i);
            self.in_pages = false;
            if let Control::Tool(_) = self.rows()[i].control
                && let Some(b) =
                    (0..TOOL_BUTTONS.len()).find(|&b| self.option_rect(i, b).contains(at))
            {
                self.button = b;
            }
        }
    }

    /// A click on a page opens it; on a check flips it; on an option picks
    /// it; on a button presses it. One outside the window closes it.
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
        self.choose(row);
        match self.rows()[row].control {
            Control::Check(..) => self.set(row, None, false, settings),
            Control::Tool(_) => {
                match (0..TOOL_BUTTONS.len()).find(|&i| self.option_rect(row, i).contains(at)) {
                    Some(i) => {
                        self.button = i;
                        self.set(row, Some(i), false, settings)
                    }
                    None => Outcome::Stay,
                }
            }
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
                Control::Tool(n) => {
                    let label = format!(
                        "    {:<TOOL_LABEL_WIDTH$}: {}",
                        row.label,
                        tool_label(&settings.tools[n])
                    );
                    fit(&label, (r.w - 2 * PAD) / CHAR)
                }
            };
            draw_text(fb, font, r.x + PAD, r.y + 4, &label, TEXT);
            if let Control::Tool(n) = row.control {
                let file = &settings.tools[n];
                let can = [true, !file.is_empty(), file != TOOLS[n]];
                for (j, name) in TOOL_BUTTONS.iter().enumerate() {
                    let on = i == self.chosen && !self.in_pages && j == self.button;
                    button(fb, font, self.option_rect(i, j), name, on, can[j]);
                }
            }
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
        let width = f.w - 4 * PAD;
        let top = rows.y + rows.h + PAD;
        for (i, line) in (wrap(about, width).iter()).take(ABOUT_LINES).enumerate() {
            let y = top + i as u32 * LINE + 4;
            draw_text(fb, font, f.x + 2 * PAD, y, line, TEXT);
        }
        let y = f.y + f.h - LINE - PAD + 4;
        draw_text(fb, font, f.x + 2 * PAD, y, HINT, DISABLED);
    }
}

/// `text` cut to `chars` characters, ending in "..." when cut.
fn fit(text: &str, chars: u32) -> String {
    let chars = chars as usize;
    if text.chars().count() <= chars {
        return text.to_string();
    }
    let kept: String = text.chars().take(chars.saturating_sub(3)).collect();
    kept + "..."
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
        // Up goes round to the last row, the language; Left goes round.
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
    fn the_extra_font_tells_the_frontend() {
        let mut s = Settings {
            editor_extra_font: Some("a.bdf".into()),
            ..Settings::default()
        };
        let mut w = SettingsWindow::new((1280, 720));
        let fonts = PAGES.iter().position(|p| p.title == "Fonts").unwrap();
        w.click(centre(w.page_row(fonts)), &mut s);
        // The first row is the menus' font.
        w.click(centre(w.option_rect(0, 2)), &mut s);
        assert_eq!(s.ui_font, UiFont::Smooth);
        assert_eq!(
            w.click(centre(w.option_rect(1, EXTRA_FONT_FILE)), &mut s),
            Outcome::ChooseExtraFont
        );
        assert!(
            s.editor_extra_font.is_some(),
            "unchanged until a file is chosen"
        );
        assert_eq!(
            w.click(centre(w.option_rect(1, 0)), &mut s),
            Outcome::ExtraFont
        );
        assert_eq!(s.editor_extra_font, None);
        // The order is a setting; the program fonts tell the frontend.
        w.click(centre(w.option_rect(2, 2)), &mut s);
        assert_eq!(s.editor_font_first, FontSource::Bundled);
        assert_eq!(
            w.click(centre(w.option_rect(4, 1)), &mut s),
            Outcome::ProgramFont
        );
        assert_eq!(s.program_font, [FontSource::SmileBasic, FontSource::File]);
    }

    #[test]
    fn tools_choose_clear_and_restore() {
        let mut s = Settings::default();
        let mut w = SettingsWindow::new((1280, 720));
        let tools = PAGES.iter().position(|p| p.title == "Tools").unwrap();
        w.click(centre(w.page_row(tools)), &mut s);
        // Enter asks for a program; Delete sets none.
        w.key(Keycode::Tab, &mut s);
        assert_eq!(w.key(Keycode::Return, &mut s), Outcome::ChooseTool(0));
        w.key(Keycode::Delete, &mut s);
        assert_eq!(s.tools[0], "");
        // Left goes round to Default, which a move to another row forgets.
        w.key(Keycode::Left, &mut s);
        w.key(Keycode::Space, &mut s);
        assert_eq!(s.tools[0], TOOLS[0]);
        w.key(Keycode::Down, &mut s);
        assert_eq!(w.key(Keycode::Return, &mut s), Outcome::ChooseTool(1));
        // Clicks on the buttons.
        w.click(centre(w.option_rect(1, NONE)), &mut s);
        assert_eq!(s.tools[1], "");
        w.click(centre(w.option_rect(1, DEFAULT)), &mut s);
        assert_eq!(s.tools[1], TOOLS[1]);
        assert_eq!(
            w.click(centre(w.option_rect(2, CHOOSE)), &mut s),
            Outcome::ChooseTool(2)
        );
        assert_eq!(w.chosen, 2);
    }

    #[test]
    fn tool_labels() {
        assert_eq!(tool_label(""), "(none)");
        assert_eq!(tool_label("#TOOL/SBSMILE.PRG"), "#TOOL/SBSMILE.PRG");
        assert_eq!(fit("F9 tool: /A.PRG", 20), "F9 tool: /A.PRG");
        assert_eq!(fit("F9 tool: /ABCDEFGH.PRG", 12), "F9 tool: ...");
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
            let f = w.frame();
            // What the chosen setting does, above the hint.
            let about_bottom = rows.y + rows.h + PAD + ABOUT_LINES as u32 * LINE;
            assert!(about_bottom <= f.y + f.h - LINE - PAD, "{}", p.title);
            let about_width = f.w - 4 * PAD;
            let label_width = rows.w - 2 * PAD;
            for (i, row) in p.rows.iter().enumerate() {
                let lines = wrap(row.about, about_width);
                assert!(lines.len() <= ABOUT_LINES, "{}: {lines:?}", row.label);
                // After "[x] " or four spaces.
                assert!(
                    text_width(row.label) + 4 * CHAR <= label_width,
                    "{}",
                    row.label
                );
                let last = match row.control {
                    Control::Choice(options, ..) => Some(options.len() - 1),
                    Control::Tool(_) => Some(TOOL_BUTTONS.len() - 1),
                    Control::Check(..) => None,
                };
                if let Some(last) = last {
                    let o = w.option_rect(i, last);
                    assert!(o.x + o.w <= rows.x + rows.w, "{}", row.label);
                }
            }
        }
    }
}

//! The emulator's settings, and their text form for keeping them between
//! runs: one `key=value` per line, `#` for comments. Unknown keys and bad
//! values are skipped, so files from other versions still load.

use crate::confirm::FileConfirm;
use crate::editor::EditorSize;
use crate::extra_glyphs::{BundledSet, FontSource};
use crate::font::FontSize;
use crate::palette::Palette;
use crate::screen::Filter;
use crate::speed::Speed;

/// The Switch's language, as SYSPARAM("LANG") reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    English,
    Japanese,
}

impl Language {
    /// The name SmileBASIC 4 gives: `en-US` in the English edition, `ja`
    /// in the Japanese one.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en-US",
            Language::Japanese => "ja",
        }
    }
}

/// The font of the emulator's menus and windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UiFont {
    /// SmileBASIC's 16-pixel font.
    #[default]
    Large,
    /// Outlines drawn at 16 pixels with gray edges (see
    /// [`crate::ui_glyphs`]).
    Smooth,
}

/// The renderer of the screens: GPU rendering if a test at start-up
/// passes, or CPU always.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Renderer {
    Auto,
    Cpu,
}

impl Renderer {
    pub const DEFAULT: Renderer = Renderer::Auto;

    pub const fn other(self) -> Renderer {
        match self {
            Renderer::Auto => Renderer::Cpu,
            Renderer::Cpu => Renderer::Auto,
        }
    }
}

/// The Simple UI's filter for scaling the screen to its display, which is
/// rarely a whole number of times the screen's size and can be smaller
/// than it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SimpleUiFilter {
    /// The pixel art filter where the screen is drawn larger than its
    /// frame, bilinear where it is drawn smaller, and nearest at a whole
    /// factor.
    #[default]
    Auto,
    PixelArt,
    Bilinear,
    /// Nearest at the fitted size, so that pixels differ in width.
    Nearest,
    /// XSCREEN's composite mode, as SmileBASIC 4 shows it.
    Xscreen,
}

impl SimpleUiFilter {
    /// Every choice, in the order the pause menu steps through them.
    pub const ALL: [SimpleUiFilter; 5] = [
        SimpleUiFilter::Auto,
        SimpleUiFilter::PixelArt,
        SimpleUiFilter::Bilinear,
        SimpleUiFilter::Nearest,
        SimpleUiFilter::Xscreen,
    ];

    /// The choice `step` places after this one, wrapping around from the
    /// last to the first.
    pub fn step(self, step: isize) -> SimpleUiFilter {
        let n = Self::ALL.len() as isize;
        let at = Self::ALL.iter().position(|&f| f == self).unwrap_or(0) as isize;
        Self::ALL[(at + step).rem_euclid(n) as usize]
    }
}

/// The size the Simple UI draws its own screens at: the game list, the
/// pause menu, dialogs and notices. The program's screen keeps its size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SimpleUiSize {
    /// The largest whole scale that leaves 480 lines or more: 640x480 on
    /// a 640x480 display.
    #[default]
    Normal,
    /// One and a half times that scale: 427x320 on a 640x480 display,
    /// where the 16-pixel text is 24 pixels tall.
    Medium,
    /// Twice that scale, so that the text is twice as tall: 320x240 on a
    /// 640x480 display.
    Large,
}

impl SimpleUiSize {
    /// Every choice, in the order the pause menu steps through them.
    pub const ALL: [SimpleUiSize; 3] = [
        SimpleUiSize::Normal,
        SimpleUiSize::Medium,
        SimpleUiSize::Large,
    ];

    /// The choice `step` places after this one, wrapping around from the
    /// last to the first.
    pub fn step(self, step: isize) -> SimpleUiSize {
        let n = Self::ALL.len() as isize;
        let at = Self::ALL.iter().position(|&s| s == self).unwrap_or(0) as isize;
        Self::ALL[(at + step).rem_euclid(n) as usize]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Letters type in upper case unless Shift or Caps Lock is on.
    pub upper_case_letters: bool,
    /// SAVE turns full-width letters and digits into ASCII.
    pub half_width_on_save: bool,
    /// SAVE, SAVEV, SAVEG, DELETE, RENAME and METASAVE write in SmileBASIC
    /// 4's own `#` projects instead of raising Can't SAVE to system
    /// project, and Ctrl+S saves there. SmileBASIC 4 does not do this.
    pub write_system_projects: bool,
    /// The editor wraps long lines instead of scrolling sideways.
    pub editor_wrap: bool,
    /// The editor draws line numbers on SmileBASIC 4's number tiles; when
    /// off, in as many plain digits as the last line needs.
    pub editor_number_tiles: bool,
    /// An overview of the program behind the editor's right side.
    pub editor_map: bool,
    /// The editor shows in a separate window, beside the screen, so that
    /// programs continue while it is open.
    pub editor_window: bool,
    /// That window opens at its last position and size.
    pub editor_window_keep_place: bool,
    /// A program that ENDs or is stopped with the Break key leaves the
    /// screens and sound the way ACLS and SNDSTOP leave them. SmileBASIC 4
    /// does not do this.
    pub tidy_on_end: bool,
    /// The last position and size of the editor's window: x, y, width and
    /// height in the system's window coordinates.
    pub editor_window_place: Option<[i32; 4]>,
    /// The editor draws the 8- or the 16-pixel font.
    pub editor_font: FontSize,
    pub editor_size: EditorSize,
    /// Spaces the editor's Tab key inserts, which SYSPARAM("TABSTEP")
    /// reports.
    pub editor_tab_step: u32,
    pub editor_colors: Palette,
    /// A BDF, TrueType or OpenType file for the characters SmileBASIC's
    /// font lacks, which the editor tries before its bundled ones; `None`
    /// for the bundled ones alone.
    pub editor_extra_font: Option<String>,
    /// The editor draws half-width characters half a cell wide; each
    /// still takes one column. SmileBASIC 4 has no such setting.
    pub editor_half_width: bool,
    /// The font source the editor looks characters up in first; the
    /// others follow in the order of [`FontSource`].
    pub editor_font_first: FontSource,
    /// The font source whose glyphs replace SmileBASIC's glyphs on the
    /// font page, for the 8- and the 16-pixel font. SmileBASIC 4 always
    /// uses its own font there.
    pub program_font: [FontSource; 2],
    /// The bundled 16-pixel glyphs that come first, in every place they
    /// are drawn: the editor, the fallback font, the menus and windows.
    pub bundled_glyphs16: BundledSet,
    pub file_confirm: FileConfirm,
    /// A file question has already said how to turn such questions off.
    pub file_hint_shown: bool,
    /// The filter that scales the screen to the window; `None` follows
    /// XSCREEN.
    pub screen_filter: Option<Filter>,
    /// The main window covers its display, as View > Fullscreen sets it
    /// on the desktop. The Simple UI is fullscreen regardless of this
    /// setting.
    pub fullscreen: bool,
    /// The main window's size out of fullscreen when the emulator last
    /// quit, in the system's window coordinates, which View > Window size
    /// sets; `None` for the profile's display.
    pub window_size: Option<[u32; 2]>,
    /// View > "Keep 16:9 when resizing": the main window keeps the
    /// display's shape while it is resized, out of fullscreen.
    pub keep_16_9: bool,
    /// View > "Wait for vsync": the main window's presents wait for the
    /// display's refresh, which paces the frames on a 60 Hz display.
    /// When off, the clock paces the frames.
    pub vsync: bool,
    /// The renderer of the screens.
    pub renderer: Renderer,
    /// Keys also press the first controller's buttons.
    pub keyboard_buttons: bool,
    /// Controllers' A and B are swapped, and so are X and Y, for programs
    /// and for the emulator's own screens: for controllers with A at the
    /// bottom. SmileBASIC 4 does not do this.
    pub swap_ab_xy: bool,
    /// Play SmileBASIC 4's own instruments when they are imported.
    pub smilebasic_instruments: bool,
    /// The SoundFont file instruments come from otherwise; `None` for the
    /// bundled one.
    pub sound_font: Option<String>,
    pub language: Language,
    /// Error messages are followed by a line explaining them in Japanese,
    /// as SmileBASIC 4's Japanese edition does.
    pub localized_errors: bool,
    /// The font of the emulator's menus and windows.
    pub ui_font: UiFont,
    /// The mouse cursor stays shown when a program hides it with
    /// SYSPARAM "MOUSECURSOR", and stays where it is when a program moves
    /// it with MOUSE X,Y. (The key keeps its old name.)
    pub keep_mouse_cursor: bool,
    /// The right stick moves the mouse pointer and R3 is its left button,
    /// like SmileBASIC 4's option for the R Stick, while no program runs
    /// or the program shows the cursor; and with Select held regardless of
    /// what the program does.
    pub right_stick_mouse: bool,
    /// Anything that only #SYS's programs may do works in any program: so
    /// far, HELPINFO OUT reading the editor's cursor.
    pub helpinfo_cursor: bool,
    /// The help panel draws its glyphs this many times their size.
    pub help_text_size: u32,
    /// Completion's list shows in DIRECT mode, and in the editor.
    pub complete_direct: bool,
    pub complete_editor: bool,
    /// Letters typed before the list shows.
    pub complete_after: u32,
    /// The list has SmileBASIC's words, and the program's names.
    pub complete_commands: bool,
    pub complete_names: bool,
    /// Those names come from every slot, the edited one first.
    pub complete_all_slots: bool,
    /// Enter takes the chosen word, besides Tab.
    pub complete_enter: bool,
    /// The forms of commands show under the list and while their
    /// arguments are typed.
    pub complete_hints: bool,
    /// Hints show by the cursor instead of at the bottom of the screen.
    pub complete_above: bool,
    /// A breakpoint or a debugger's step opens the editor at the line the
    /// program stopped before.
    pub debug_open_editor: bool,
    /// The debugger's panel draws its glyphs this many times their size.
    pub debug_text_size: u32,
    /// The speed programs run at: as fast as they can, or about as fast as
    /// on a Switch 2 or a Switch 1.
    pub speed: Speed,
    /// Frames pass while a program too slow for the computer is part-way
    /// through its main loop, so that it keeps time. Off by default: an
    /// iteration of the program's main loop keeps its frame and the
    /// program slows down instead (the frontend sets
    /// `Machine::keep_frames` to the opposite).
    pub skip_frames: bool,
    /// A notice says where `skip_frames` is when a program runs slower
    /// than real time for a while. On by default.
    pub slow_hint: bool,
    /// F1 to F5's contents, which KEY reads, as SmileBASIC 4's settings
    /// set them.
    pub function_keys: [String; 5],
    /// The programs F9 to F11 start, which SYSPARAM "TOOL1" to "TOOL3"
    /// reads, as SmileBASIC 4's settings set them.
    pub tools: [String; 3],
    /// The Simple UI: the frontend is laid out for a gamepad alone and
    /// a small screen: fullscreen, a game list instead of DIRECT mode, no
    /// menu bar.
    pub simple_ui: bool,
    /// The Simple UI's filter for scaling the screen; `screen_filter` is
    /// the one for the desktop.
    pub simple_ui_filter: SimpleUiFilter,
    /// The size the Simple UI draws its own screens at.
    pub simple_ui_size: SimpleUiSize,
    /// The Simple UI's welcome screen has been shown. It asks for A
    /// and sets `swap_ab_xy` by the button pressed.
    pub simple_ui_welcome_shown: bool,
}

/// F1 to F5 as SmileBASIC 4 starts (checked on hardware).
pub const FUNCTION_KEYS: [&str; 5] = ["FILES", "LOAD\"", "SAVE\"", "LIST ERR", "RUN"];
/// F9 to F11's programs as SmileBASIC 4 starts (checked on hardware).
pub const TOOLS: [&str; 3] = ["#TOOL/SBSMILE.PRG", "#TOOL/GAHAKU.PRG", ""];

impl Default for Settings {
    fn default() -> Self {
        Settings {
            upper_case_letters: true,
            half_width_on_save: true,
            write_system_projects: false,
            editor_wrap: false,
            editor_number_tiles: true,
            editor_map: true,
            editor_window: false,
            editor_window_keep_place: false,
            tidy_on_end: false,
            editor_window_place: None,
            editor_font: FontSize::Large,
            editor_size: EditorSize::Medium,
            // Characters are shown full-width, so one is already wide.
            editor_tab_step: 1,
            editor_colors: Palette::default(),
            editor_extra_font: None,
            editor_half_width: false,
            editor_font_first: FontSource::SmileBasic,
            program_font: [FontSource::SmileBasic; 2],
            bundled_glyphs16: BundledSet::Antialiased,
            file_confirm: FileConfirm::Ask,
            file_hint_shown: false,
            screen_filter: Some(Filter::Nearest),
            fullscreen: false,
            window_size: None,
            keep_16_9: true,
            vsync: true,
            renderer: Renderer::DEFAULT,
            keyboard_buttons: true,
            swap_ab_xy: false,
            smilebasic_instruments: true,
            sound_font: None,
            language: Language::English,
            localized_errors: false,
            ui_font: UiFont::Large,
            keep_mouse_cursor: false,
            right_stick_mouse: true,
            helpinfo_cursor: false,
            help_text_size: 2,
            complete_direct: true,
            complete_editor: true,
            complete_after: 1,
            complete_commands: true,
            complete_names: false,
            complete_all_slots: false,
            complete_enter: true,
            complete_hints: true,
            complete_above: false,
            debug_open_editor: true,
            debug_text_size: 2,
            speed: Speed::Unlimited,
            skip_frames: false,
            slow_hint: true,
            function_keys: FUNCTION_KEYS.map(String::from),
            tools: TOOLS.map(String::from),
            simple_ui: false,
            simple_ui_filter: SimpleUiFilter::Auto,
            simple_ui_size: SimpleUiSize::Normal,
            simple_ui_welcome_shown: false,
        }
    }
}

impl Settings {
    /// The settings tests run with. Each value is written out, so a
    /// change to a default changes nothing a test sees. A program that
    /// ends leaves its screens and sound, as on SmileBASIC 4, for tests
    /// to read.
    pub fn for_tests() -> Self {
        Settings {
            upper_case_letters: true,
            half_width_on_save: true,
            write_system_projects: false,
            editor_wrap: false,
            editor_number_tiles: true,
            editor_map: true,
            editor_window: false,
            editor_window_keep_place: false,
            tidy_on_end: false,
            editor_window_place: None,
            editor_font: FontSize::Large,
            editor_size: EditorSize::Medium,
            editor_tab_step: 1,
            editor_colors: Palette::default(),
            editor_extra_font: None,
            editor_half_width: false,
            editor_font_first: FontSource::SmileBasic,
            program_font: [FontSource::SmileBasic; 2],
            bundled_glyphs16: BundledSet::Antialiased,
            file_confirm: FileConfirm::Ask,
            file_hint_shown: false,
            screen_filter: Some(Filter::Nearest),
            fullscreen: false,
            window_size: None,
            keep_16_9: true,
            vsync: true,
            renderer: Renderer::Auto,
            keyboard_buttons: true,
            swap_ab_xy: false,
            smilebasic_instruments: true,
            sound_font: None,
            language: Language::English,
            localized_errors: false,
            ui_font: UiFont::Large,
            keep_mouse_cursor: false,
            right_stick_mouse: true,
            helpinfo_cursor: false,
            help_text_size: 2,
            complete_direct: true,
            complete_editor: true,
            complete_after: 1,
            complete_commands: true,
            complete_names: false,
            complete_all_slots: false,
            complete_enter: true,
            complete_hints: true,
            complete_above: false,
            debug_open_editor: true,
            debug_text_size: 2,
            speed: Speed::Unlimited,
            skip_frames: false,
            slow_hint: true,
            function_keys: FUNCTION_KEYS.map(String::from),
            tools: TOOLS.map(String::from),
            simple_ui: false,
            simple_ui_filter: SimpleUiFilter::Auto,
            simple_ui_size: SimpleUiSize::Normal,
            simple_ui_welcome_shown: false,
        }
    }
}

/// Settings lines for F1 to F5 start with this and the key's number.
const FUNCTION_KEY: &str = "key";
/// Settings lines for F9 to F11's programs start with this and 1 to 3.
const TOOL: &str = "tool";

const SCREEN_FILTER: [(Option<Filter>, &str); 4] = [
    (None, "xscreen"),
    (Some(Filter::Nearest), "nearest"),
    (Some(Filter::SmartNearest), "pixel_art"),
    (Some(Filter::Bilinear), "bilinear"),
];

const SIMPLE_UI_FILTER: [(SimpleUiFilter, &str); 5] = [
    (SimpleUiFilter::Auto, "auto"),
    (SimpleUiFilter::PixelArt, "pixel_art"),
    (SimpleUiFilter::Bilinear, "bilinear"),
    (SimpleUiFilter::Nearest, "nearest"),
    (SimpleUiFilter::Xscreen, "xscreen"),
];

const SIMPLE_UI_SIZE: [(SimpleUiSize, &str); 3] = [
    (SimpleUiSize::Normal, "normal"),
    (SimpleUiSize::Medium, "medium"),
    (SimpleUiSize::Large, "large"),
];

const RENDERER: [(Renderer, &str); 2] = [(Renderer::Auto, "auto"), (Renderer::Cpu, "cpu")];

const FILE_CONFIRM: [(FileConfirm, &str); 3] = [
    (FileConfirm::Ask, "ask"),
    (FileConfirm::Notify, "notify"),
    (FileConfirm::Silent, "silent"),
];

const FONT_SIZE: [(FontSize, &str); 2] = [(FontSize::Small, "8"), (FontSize::Large, "16")];

/// `ui_font=8`, the 8-pixel fonts doubled, is gone: it loads as the
/// default.
const UI_FONT: [(UiFont, &str); 2] = [(UiFont::Large, "16"), (UiFont::Smooth, "smooth")];

const FONT_SOURCE: [(FontSource, &str); 3] = [
    (FontSource::SmileBasic, "smilebasic"),
    (FontSource::File, "file"),
    (FontSource::Bundled, "bundled"),
];

const BUNDLED_SET: [(BundledSet, &str); 2] = [
    (BundledSet::Antialiased, "antialiased"),
    (BundledSet::Pixel, "pixel"),
];

const EDITOR_SIZE: [(EditorSize, &str); 4] = [
    (EditorSize::Large, "large"),
    (EditorSize::Medium, "medium"),
    (EditorSize::Small, "small"),
    (EditorSize::Tiny, "tiny"),
];

/// The editor's Tab key inserts this many spaces at most.
pub const TAB_STEPS: std::ops::RangeInclusive<u32> = 1..=8;

pub const HELP_TEXT_SIZES: std::ops::RangeInclusive<u32> = 1..=3;
/// The debugger panel's text sizes.
pub const DEBUG_TEXT_SIZES: std::ops::RangeInclusive<u32> = 1..=2;

pub const COMPLETE_AFTER: std::ops::RangeInclusive<u32> = 1..=4;

const LANGUAGE: [(Language, &str); 2] = [(Language::English, "en"), (Language::Japanese, "ja")];

/// Keys of the editor's colors start with this.
const EDITOR_COLOR: &str = "editor_color.";

/// The value's entry in a table of choices and their names.
fn choice<T: Copy>(table: &[(T, &str)], value: &str) -> Option<T> {
    table.iter().find(|(_, n)| *n == value).map(|&(v, _)| v)
}

/// The name of a choice in its table.
fn choice_name<T: PartialEq>(table: &[(T, &'static str)], value: &T) -> &'static str {
    table
        .iter()
        .find(|(v, _)| v == value)
        .map_or("", |(_, n)| n)
}

/// Four numbers separated by commas, with a size above zero.
fn parse_place(value: &str) -> Option<[i32; 4]> {
    let mut parts = value.split(',').map(|n| n.trim().parse::<i32>());
    let mut place = [0; 4];
    for n in &mut place {
        *n = parts.next()?.ok()?;
    }
    (parts.next().is_none() && place[2] > 0 && place[3] > 0).then_some(place)
}

/// A window's width and height, as `1280,720`, each above 0.
fn parse_size(value: &str) -> Option<[u32; 2]> {
    let (w, h) = value.split_once(',')?;
    let size = [w.trim().parse().ok()?, h.trim().parse().ok()?];
    (size[0] > 0 && size[1] > 0).then_some(size)
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

impl Settings {
    /// Reads settings over the defaults.
    pub fn from_text(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            if let Some(part) = key.strip_prefix(EDITOR_COLOR) {
                s.editor_colors.set(part, value);
                continue;
            }
            // The value keeps its inner spaces: "key4=LIST ERR".
            if let Some(n) = key.strip_prefix(FUNCTION_KEY)
                && let Ok(n @ 1..=5) = n.parse::<usize>()
            {
                s.function_keys[n - 1] = value.to_string();
                continue;
            }
            if let Some(n) = key.strip_prefix(TOOL)
                && let Ok(n @ 1..=3) = n.parse::<usize>()
            {
                s.tools[n - 1] = value.to_string();
                continue;
            }
            let flag = match key {
                "upper_case_letters" => &mut s.upper_case_letters,
                "localized_errors" => &mut s.localized_errors,
                "half_width_on_save" => &mut s.half_width_on_save,
                "write_system_projects" => &mut s.write_system_projects,
                "editor_wrap" => &mut s.editor_wrap,
                "editor_number_tiles" => &mut s.editor_number_tiles,
                "editor_map" => &mut s.editor_map,
                "editor_half_width" => &mut s.editor_half_width,
                "editor_window" => &mut s.editor_window,
                "editor_window_keep_place" => &mut s.editor_window_keep_place,
                "tidy_on_end" => &mut s.tidy_on_end,
                "file_hint_shown" => &mut s.file_hint_shown,
                "keyboard_buttons" => &mut s.keyboard_buttons,
                "swap_ab_xy" => &mut s.swap_ab_xy,
                "smilebasic_instruments" => &mut s.smilebasic_instruments,
                "keep_mouse_cursor" => &mut s.keep_mouse_cursor,
                "right_stick_mouse" => &mut s.right_stick_mouse,
                "skip_frames" => &mut s.skip_frames,
                "slow_hint" => &mut s.slow_hint,
                "helpinfo_cursor" => &mut s.helpinfo_cursor,
                "complete_direct" => &mut s.complete_direct,
                "complete_editor" => &mut s.complete_editor,
                "complete_commands" => &mut s.complete_commands,
                "complete_names" => &mut s.complete_names,
                "complete_all_slots" => &mut s.complete_all_slots,
                "complete_enter" => &mut s.complete_enter,
                "debug_open_editor" => &mut s.debug_open_editor,
                "complete_hints" => &mut s.complete_hints,
                "complete_above" => &mut s.complete_above,
                "simple_ui" => &mut s.simple_ui,
                "simple_ui_welcome_shown" => &mut s.simple_ui_welcome_shown,
                "fullscreen" => &mut s.fullscreen,
                "keep_16_9" => &mut s.keep_16_9,
                "vsync" => &mut s.vsync,
                "complete_after" => {
                    if let Some(n) = value.parse().ok().filter(|n| COMPLETE_AFTER.contains(n)) {
                        s.complete_after = n;
                    }
                    continue;
                }
                "language" => {
                    if let Some(l) = choice(&LANGUAGE, value) {
                        s.language = l;
                    }
                    continue;
                }
                "help_text_size" => {
                    if let Some(n) = value.parse().ok().filter(|n| HELP_TEXT_SIZES.contains(n)) {
                        s.help_text_size = n;
                    }
                    continue;
                }
                "debug_text_size" => {
                    if let Some(n) = value.parse().ok().filter(|n| DEBUG_TEXT_SIZES.contains(n)) {
                        s.debug_text_size = n;
                    }
                    continue;
                }
                "editor_tab_step" => {
                    if let Some(n) = value.parse().ok().filter(|n| TAB_STEPS.contains(n)) {
                        s.editor_tab_step = n;
                    }
                    continue;
                }
                "file_confirm" => {
                    if let Some(mode) = choice(&FILE_CONFIRM, value) {
                        s.file_confirm = mode;
                    }
                    continue;
                }
                "editor_window_place" => {
                    if let Some(place) = parse_place(value) {
                        s.editor_window_place = Some(place);
                    }
                    continue;
                }
                "window_size" => {
                    s.window_size = parse_size(value);
                    continue;
                }
                "sound_font" => {
                    s.sound_font = (!value.is_empty()).then(|| value.to_string());
                    continue;
                }
                "editor_extra_font" => {
                    s.editor_extra_font = (!value.is_empty()).then(|| value.to_string());
                    continue;
                }
                "editor_font_first" | "program_font8" | "program_font16" => {
                    if let Some(f) = choice(&FONT_SOURCE, value) {
                        *match key {
                            "editor_font_first" => &mut s.editor_font_first,
                            "program_font8" => &mut s.program_font[0],
                            _ => &mut s.program_font[1],
                        } = f;
                    }
                    continue;
                }
                "bundled_glyphs16" => {
                    if let Some(set) = choice(&BUNDLED_SET, value) {
                        s.bundled_glyphs16 = set;
                    }
                    continue;
                }
                "editor_font" => {
                    if let Some(f) = choice(&FONT_SIZE, value) {
                        s.editor_font = f;
                    }
                    continue;
                }
                "ui_font" => {
                    if let Some(f) = choice(&UI_FONT, value) {
                        s.ui_font = f;
                    }
                    continue;
                }
                "editor_size" => {
                    if let Some(size) = choice(&EDITOR_SIZE, value) {
                        s.editor_size = size;
                    }
                    continue;
                }
                "renderer" => {
                    if let Some(renderer) = choice(&RENDERER, value) {
                        s.renderer = renderer;
                    }
                    continue;
                }
                "speed" => {
                    if let Some(speed) = Speed::from_name(value) {
                        s.speed = speed;
                    }
                    continue;
                }
                "simple_ui_filter" => {
                    if let Some(f) = choice(&SIMPLE_UI_FILTER, value) {
                        s.simple_ui_filter = f;
                    }
                    continue;
                }
                "simple_ui_size" => {
                    if let Some(size) = choice(&SIMPLE_UI_SIZE, value) {
                        s.simple_ui_size = size;
                    }
                    continue;
                }
                "screen_filter" => {
                    if let Some(f) = choice(&SCREEN_FILTER, value) {
                        s.screen_filter = f;
                    }
                    continue;
                }
                _ => continue,
            };
            if let Some(v) = parse_bool(value) {
                *flag = v;
            }
        }
        s
    }

    pub fn to_text(&self) -> String {
        let mode = choice_name(&FILE_CONFIRM, &self.file_confirm);
        let filter = choice_name(&SCREEN_FILTER, &self.screen_filter);
        // Only a choice is kept, so that the default can change for
        // anyone who made none.
        let renderer = match self.renderer {
            Renderer::DEFAULT => String::new(),
            r => format!("renderer={}\n", choice_name(&RENDERER, &r)),
        };
        format!(
            "# SmileBASIC emulator settings\n\
             upper_case_letters={}\n\
             half_width_on_save={}\n\
             write_system_projects={}\n\
             editor_wrap={}\n\
             editor_number_tiles={}\n\
             editor_map={}\n\
             editor_window={}\n\
             editor_window_keep_place={}\n\
             tidy_on_end={}\n\
             editor_window_place={}\n\
             editor_font={}\n\
             editor_size={}\n\
             editor_tab_step={}\n\
             {}\
             editor_extra_font={}\n\
             editor_half_width={}\n\
             editor_font_first={}\n\
             program_font8={}\n\
             program_font16={}\n\
             bundled_glyphs16={}\n\
             file_confirm={mode}\n\
             file_hint_shown={}\n\
             screen_filter={filter}\n\
             fullscreen={}\n\
             window_size={}\n\
             keep_16_9={}\n\
             vsync={}\n\
             {renderer}\
             keyboard_buttons={}\n\
             swap_ab_xy={}\n\
             smilebasic_instruments={}\n\
             sound_font={}\n\
             language={}\n\
             localized_errors={}\n\
             ui_font={}\n\
             keep_mouse_cursor={}\n\
             right_stick_mouse={}\n\
             helpinfo_cursor={}\n\
             help_text_size={}\n\
             complete_direct={}\n\
             complete_editor={}\n\
             complete_after={}\n\
             complete_commands={}\n\
             complete_names={}\n\
             complete_all_slots={}\n\
             complete_enter={}\n\
             complete_hints={}\n\
             complete_above={}\n\
             debug_open_editor={}\n\
             debug_text_size={}\n\
             speed={}\n\
             skip_frames={}\n\
             slow_hint={}\n\
             simple_ui={}\n\
             simple_ui_filter={}\n\
             simple_ui_size={}\n\
             simple_ui_welcome_shown={}\n\
             {}{}",
            self.upper_case_letters,
            self.half_width_on_save,
            self.write_system_projects,
            self.editor_wrap,
            self.editor_number_tiles,
            self.editor_map,
            self.editor_window,
            self.editor_window_keep_place,
            self.tidy_on_end,
            (self.editor_window_place)
                .map_or(String::new(), |[x, y, w, h]| format!("{x},{y},{w},{h}")),
            choice_name(&FONT_SIZE, &self.editor_font),
            choice_name(&EDITOR_SIZE, &self.editor_size),
            self.editor_tab_step,
            self.editor_colors.lines(EDITOR_COLOR),
            self.editor_extra_font.as_deref().unwrap_or(""),
            self.editor_half_width,
            choice_name(&FONT_SOURCE, &self.editor_font_first),
            choice_name(&FONT_SOURCE, &self.program_font[0]),
            choice_name(&FONT_SOURCE, &self.program_font[1]),
            choice_name(&BUNDLED_SET, &self.bundled_glyphs16),
            self.file_hint_shown,
            self.fullscreen,
            (self.window_size).map_or(String::new(), |[w, h]| format!("{w},{h}")),
            self.keep_16_9,
            self.vsync,
            self.keyboard_buttons,
            self.swap_ab_xy,
            self.smilebasic_instruments,
            self.sound_font.as_deref().unwrap_or(""),
            choice_name(&LANGUAGE, &self.language),
            self.localized_errors,
            choice_name(&UI_FONT, &self.ui_font),
            self.keep_mouse_cursor,
            self.right_stick_mouse,
            self.helpinfo_cursor,
            self.help_text_size,
            self.complete_direct,
            self.complete_editor,
            self.complete_after,
            self.complete_commands,
            self.complete_names,
            self.complete_all_slots,
            self.complete_enter,
            self.complete_hints,
            self.complete_above,
            self.debug_open_editor,
            self.debug_text_size,
            self.speed.name(),
            self.skip_frames,
            self.slow_hint,
            self.simple_ui,
            choice_name(&SIMPLE_UI_FILTER, &self.simple_ui_filter),
            choice_name(&SIMPLE_UI_SIZE, &self.simple_ui_size),
            self.simple_ui_welcome_shown,
            (self.function_keys.iter().enumerate())
                .map(|(i, k)| format!("{FUNCTION_KEY}{}={k}\n", i + 1))
                .collect::<String>(),
            (self.tools.iter().enumerate())
                .map(|(i, t)| format!("{TOOL}{}={t}\n", i + 1))
                .collect::<String>(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_test_settings() {
        assert_eq!(Settings::default(), Settings::for_tests());
    }

    #[test]
    fn round_trips() {
        let s = Settings {
            upper_case_letters: false,
            editor_wrap: true,
            editor_window_keep_place: true,
            tidy_on_end: true,
            editor_window_place: Some([-1200, 40, 640, 480]),
            editor_number_tiles: false,
            editor_font: FontSize::Small,
            editor_size: EditorSize::Small,
            editor_colors: crate::palette::PRESETS[2].1,
            file_confirm: FileConfirm::Notify,
            file_hint_shown: true,
            screen_filter: None,
            fullscreen: true,
            window_size: Some([2560, 1440]),
            keep_16_9: false,
            vsync: false,
            renderer: Renderer::DEFAULT.other(),
            keyboard_buttons: false,
            smilebasic_instruments: false,
            sound_font: Some("/fonts/a b.sf2".to_string()),
            editor_extra_font: Some("/fonts/unicode 16.bdf".to_string()),
            editor_half_width: true,
            editor_font_first: FontSource::Bundled,
            program_font: [FontSource::File, FontSource::Bundled],
            bundled_glyphs16: BundledSet::Pixel,
            editor_tab_step: 2,
            language: Language::Japanese,
            localized_errors: true,
            ui_font: UiFont::Smooth,
            keep_mouse_cursor: true,
            right_stick_mouse: false,
            helpinfo_cursor: true,
            write_system_projects: true,
            complete_all_slots: true,
            speed: Speed::Switch1,
            skip_frames: true,
            slow_hint: false,
            function_keys: ["A", "B", "", "LIST ERR", "\"Q"].map(String::from),
            tools: ["#TOOL/X.PRG", "", "MINE/EDIT.PRG"].map(String::from),
            simple_ui: true,
            simple_ui_filter: SimpleUiFilter::Xscreen,
            simple_ui_size: SimpleUiSize::Large,
            simple_ui_welcome_shown: true,
            swap_ab_xy: true,
            ..Settings::default()
        };
        assert_eq!(Settings::from_text(&s.to_text()), s);
        assert_eq!(
            Settings::from_text(&Settings::default().to_text()),
            Settings::default()
        );
    }

    #[test]
    fn programs_slow_down_by_default_and_the_notice_shows() {
        let s = Settings::default();
        assert!(!s.skip_frames && s.slow_hint);
        let text = s.to_text();
        assert!(text.contains("skip_frames=false\n") && text.contains("slow_hint=true\n"));
        let s = Settings::from_text("skip_frames=true\nslow_hint=false\n");
        assert!(s.skip_frames && !s.slow_hint);
        // keep_frames, the setting's name for a few hours on main, means
        // nothing now, like any unknown key.
        let s = Settings::from_text("keep_frames=true\nkeep_frames=false\nno_such_key=1\n");
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn the_main_window_keeps_its_size_and_fullscreen() {
        let s = Settings::from_text("window_size=1920,1080\nfullscreen=true\n");
        assert_eq!(s.window_size, Some([1920, 1080]));
        assert!(s.fullscreen);
        for bad in ["0,720", "1280", "1280,x", "-1,720", ""] {
            let s = Settings::from_text(&format!("window_size={bad}\n"));
            assert_eq!(s.window_size, None, "{bad}");
        }
        let text = Settings::default().to_text();
        assert!(text.contains("window_size=\n") && text.contains("fullscreen=false\n"));
        // The main window keeps 16:9 unless told not to.
        assert!(Settings::default().keep_16_9 && text.contains("keep_16_9=true\n"));
        assert!(!Settings::from_text("keep_16_9=false\n").keep_16_9);
        // Presents wait for vsync unless told not to.
        assert!(Settings::default().vsync && text.contains("vsync=true\n"));
        assert!(!Settings::from_text("vsync=false\n").vsync);
    }

    #[test]
    fn the_simple_ui_filter_has_its_own_key_and_steps_round() {
        for f in SimpleUiFilter::ALL {
            let s = Settings {
                simple_ui_filter: f,
                ..Settings::default()
            };
            let back = Settings::from_text(&s.to_text());
            assert_eq!(back.simple_ui_filter, f);
            assert_eq!(back.screen_filter, Settings::default().screen_filter);
        }
        let s = Settings::from_text("simple_ui_filter=blurry\nscreen_filter=bilinear\n");
        assert_eq!(s.simple_ui_filter, SimpleUiFilter::Auto);
        assert_eq!(s.screen_filter, Some(Filter::Bilinear));
        let s = Settings::from_text("simple_ui_filter=xscreen\n");
        assert_eq!(s.simple_ui_filter, SimpleUiFilter::Xscreen);
        assert_eq!(s.screen_filter, Settings::default().screen_filter);
        let f = SimpleUiFilter::Auto;
        assert_eq!(f.step(1), SimpleUiFilter::PixelArt);
        assert_eq!(f.step(-1), SimpleUiFilter::Xscreen);
        assert_eq!(SimpleUiFilter::Xscreen.step(1), f);
        assert_eq!(f.step(5), f);
    }

    #[test]
    fn the_simple_ui_settings_have_keys_of_their_own() {
        for size in SimpleUiSize::ALL {
            let s = Settings {
                simple_ui_size: size,
                ..Settings::default()
            };
            assert_eq!(Settings::from_text(&s.to_text()).simple_ui_size, size);
        }
        let text = Settings::default().to_text();
        for line in [
            "simple_ui_size=normal\n",
            "simple_ui_welcome_shown=false\n",
            "swap_ab_xy=false\n",
        ] {
            assert!(text.contains(line), "{line}");
        }
        let s = Settings::from_text(
            "simple_ui_size=large\nsimple_ui_welcome_shown=true\nswap_ab_xy=true\n",
        );
        assert_eq!(s.simple_ui_size, SimpleUiSize::Large);
        assert!(s.simple_ui_welcome_shown && s.swap_ab_xy);
        assert!(!s.simple_ui, "each has a separate key");
        let s = Settings::from_text("simple_ui_size=huge\nswap_ab_xy=1\n");
        assert_eq!(s, Settings::default());
        for (text, size) in [
            ("normal", SimpleUiSize::Normal),
            ("medium", SimpleUiSize::Medium),
            ("large", SimpleUiSize::Large),
        ] {
            let s = Settings::from_text(&format!("simple_ui_size={text}\n"));
            assert_eq!(s.simple_ui_size, size, "{text}");
            assert!(s.to_text().contains(&format!("simple_ui_size={text}\n")));
        }
        // Unknown values, and names in another case, keep the default.
        for text in ["Medium", "1.5", ""] {
            let s = Settings::from_text(&format!("simple_ui_size={text}\n"));
            assert_eq!(s.simple_ui_size, SimpleUiSize::Normal, "{text:?}");
        }
        let (normal, medium, large) = (
            SimpleUiSize::Normal,
            SimpleUiSize::Medium,
            SimpleUiSize::Large,
        );
        assert_eq!(normal.step(1), medium);
        assert_eq!(medium.step(1), large);
        assert_eq!(large.step(1), normal);
        assert_eq!(normal.step(-1), large);
        assert_eq!(large.step(-1), medium);
    }

    #[test]
    fn the_renderer_is_kept_only_if_chosen() {
        let chosen = Settings {
            renderer: Renderer::DEFAULT.other(),
            ..Settings::default()
        };
        assert!(chosen.to_text().contains("renderer="));
        assert!(!Settings::default().to_text().contains("renderer="));
        for (text, renderer) in [("auto", Renderer::Auto), ("cpu", Renderer::Cpu)] {
            let s = Settings::from_text(&format!("renderer={text}\n"));
            assert_eq!(s.renderer, renderer);
        }
        let s = Settings::from_text("renderer=quantum\n");
        assert_eq!(s.renderer, Renderer::DEFAULT);
    }

    #[test]
    fn the_doubled_menu_font_loads_as_the_default() {
        let s = Settings::from_text("ui_font=8\n");
        assert_eq!(s.ui_font, UiFont::Large);
        assert!(s.to_text().contains("ui_font=16\n"));
        let s = Settings::from_text("ui_font=smooth\n");
        assert_eq!(s.ui_font, UiFont::Smooth);
    }

    #[test]
    fn skips_what_it_does_not_know() {
        let s = Settings::from_text(
            "# comment\nfuture_option=1\nupper_case_letters = false\neditor_wrap=maybe\n\
             file_confirm=loud\neditor_window_place=1,2,0,4\n\
             editor_window_place=1,2,3\nno equals sign\neditor_tab_step=0\nlanguage=fr\n",
        );
        assert_eq!(
            s,
            Settings {
                upper_case_letters: false,
                ..Settings::default()
            }
        );
    }

    /// The setting that made Ctrl+C stop programs is gone: a file that
    /// still has it loads, and the next save does not write it.
    #[test]
    fn drops_ctrl_c_stops() {
        let s = Settings::from_text("ctrl_c_stops=true\neditor_wrap=true\n");
        assert_eq!(
            s,
            Settings {
                editor_wrap: true,
                ..Settings::default()
            }
        );
        assert!(!s.to_text().contains("ctrl_c"));
    }
}

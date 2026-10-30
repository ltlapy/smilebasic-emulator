//! The emulator's settings, and their text form for keeping them between
//! runs: one `key=value` per line, `#` for comments. Unknown keys and bad
//! values are skipped, so files from other versions still load.

use crate::confirm::FileConfirm;
use crate::editor::EditorSize;
use crate::font::FontSize;
use crate::palette::Palette;
use crate::screen::Filter;

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Letters type in upper case unless Shift or Caps Lock is on.
    pub upper_case_letters: bool,
    /// Ctrl+C stops the program, besides Pause/Break. Off by default, since
    /// programs can read Ctrl+C.
    pub ctrl_c_stops: bool,
    /// SAVE turns full-width letters and digits into ASCII.
    pub half_width_on_save: bool,
    /// The editor wraps long lines instead of scrolling sideways.
    pub editor_wrap: bool,
    /// The editor draws line numbers on SmileBASIC 4's number tiles; off,
    /// in as many plain digits as the last line needs.
    pub editor_number_tiles: bool,
    /// The editor draws the 8- or the 16-pixel font.
    pub editor_font: FontSize,
    pub editor_size: EditorSize,
    /// Spaces the editor's Tab key inserts, which SYSPARAM("TABSTEP")
    /// reports.
    pub editor_tab_step: u32,
    pub editor_colors: Palette,
    pub file_confirm: FileConfirm,
    /// A file question has already said how to turn such questions off.
    pub file_hint_shown: bool,
    /// How the screen is scaled to the window; `None` follows XSCREEN.
    pub screen_filter: Option<Filter>,
    /// Keys also press the first controller's buttons.
    pub keyboard_buttons: bool,
    /// Play SmileBASIC 4's own instruments when they are imported.
    pub smilebasic_instruments: bool,
    /// The SoundFont file instruments come from otherwise; `None` for the
    /// bundled one.
    pub sound_font: Option<String>,
    pub language: Language,
    /// The mouse cursor stays shown when a program hides it with
    /// SYSPARAM "MOUSECURSOR".
    pub keep_mouse_cursor: bool,
    /// F1 to F5's contents, which KEY reads, as SmileBASIC 4's settings
    /// set them.
    pub function_keys: [String; 5],
}

/// F1 to F5 as SmileBASIC 4 starts (checked on hardware).
pub const FUNCTION_KEYS: [&str; 5] = ["FILES", "LOAD\"", "SAVE\"", "LIST ERR", "RUN"];

impl Default for Settings {
    fn default() -> Self {
        Settings {
            upper_case_letters: true,
            ctrl_c_stops: false,
            half_width_on_save: true,
            editor_wrap: false,
            editor_number_tiles: true,
            editor_font: FontSize::Large,
            editor_size: EditorSize::Medium,
            // Characters are shown full-width, so one is already wide.
            editor_tab_step: 1,
            editor_colors: Palette::default(),
            file_confirm: FileConfirm::Ask,
            file_hint_shown: false,
            screen_filter: Some(Filter::Nearest),
            keyboard_buttons: true,
            smilebasic_instruments: true,
            sound_font: None,
            language: Language::English,
            keep_mouse_cursor: false,
            function_keys: FUNCTION_KEYS.map(String::from),
        }
    }
}

/// Settings lines for F1 to F5 start with this and the key's number.
const FUNCTION_KEY: &str = "key";

const SCREEN_FILTER: [(Option<Filter>, &str); 4] = [
    (None, "xscreen"),
    (Some(Filter::Nearest), "nearest"),
    (Some(Filter::SmartNearest), "pixel_art"),
    (Some(Filter::Bilinear), "bilinear"),
];

const FILE_CONFIRM: [(FileConfirm, &str); 3] = [
    (FileConfirm::Ask, "ask"),
    (FileConfirm::Notify, "notify"),
    (FileConfirm::Silent, "silent"),
];

const EDITOR_FONT: [(FontSize, &str); 2] = [(FontSize::Small, "8"), (FontSize::Large, "16")];

const EDITOR_SIZE: [(EditorSize, &str); 4] = [
    (EditorSize::Large, "large"),
    (EditorSize::Medium, "medium"),
    (EditorSize::Small, "small"),
    (EditorSize::Tiny, "tiny"),
];

/// The editor's Tab key inserts this many spaces at most.
pub const TAB_STEPS: std::ops::RangeInclusive<u32> = 1..=8;

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
            let flag = match key {
                "upper_case_letters" => &mut s.upper_case_letters,
                "ctrl_c_stops" => &mut s.ctrl_c_stops,
                "half_width_on_save" => &mut s.half_width_on_save,
                "editor_wrap" => &mut s.editor_wrap,
                "editor_number_tiles" => &mut s.editor_number_tiles,
                "file_hint_shown" => &mut s.file_hint_shown,
                "keyboard_buttons" => &mut s.keyboard_buttons,
                "smilebasic_instruments" => &mut s.smilebasic_instruments,
                "keep_mouse_cursor" => &mut s.keep_mouse_cursor,
                "language" => {
                    if let Some(l) = choice(&LANGUAGE, value) {
                        s.language = l;
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
                    if let Some(&(mode, _)) = FILE_CONFIRM.iter().find(|(_, n)| *n == value) {
                        s.file_confirm = mode;
                    }
                    continue;
                }
                "sound_font" => {
                    s.sound_font = (!value.is_empty()).then(|| value.to_string());
                    continue;
                }
                "editor_font" => {
                    if let Some(f) = choice(&EDITOR_FONT, value) {
                        s.editor_font = f;
                    }
                    continue;
                }
                "editor_size" => {
                    if let Some(size) = choice(&EDITOR_SIZE, value) {
                        s.editor_size = size;
                    }
                    continue;
                }
                "screen_filter" => {
                    if let Some(&(f, _)) = SCREEN_FILTER.iter().find(|(_, n)| *n == value) {
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
        let mode = FILE_CONFIRM
            .iter()
            .find(|(m, _)| *m == self.file_confirm)
            .map_or("ask", |(_, n)| n);
        let filter = SCREEN_FILTER
            .iter()
            .find(|(f, _)| *f == self.screen_filter)
            .map_or("nearest", |(_, n)| n);
        format!(
            "# SmileBASIC emulator settings\n\
             upper_case_letters={}\n\
             ctrl_c_stops={}\n\
             half_width_on_save={}\n\
             editor_wrap={}\n\
             editor_number_tiles={}\n\
             editor_font={}\n\
             editor_size={}\n\
             editor_tab_step={}\n\
             {}\
             file_confirm={mode}\n\
             file_hint_shown={}\n\
             screen_filter={filter}\n\
             keyboard_buttons={}\n\
             smilebasic_instruments={}\n\
             sound_font={}\n\
             language={}\n\
             keep_mouse_cursor={}\n\
             {}",
            self.upper_case_letters,
            self.ctrl_c_stops,
            self.half_width_on_save,
            self.editor_wrap,
            self.editor_number_tiles,
            choice_name(&EDITOR_FONT, &self.editor_font),
            choice_name(&EDITOR_SIZE, &self.editor_size),
            self.editor_tab_step,
            self.editor_colors.lines(EDITOR_COLOR),
            self.file_hint_shown,
            self.keyboard_buttons,
            self.smilebasic_instruments,
            self.sound_font.as_deref().unwrap_or(""),
            choice_name(&LANGUAGE, &self.language),
            self.keep_mouse_cursor,
            (self.function_keys.iter().enumerate())
                .map(|(i, k)| format!("{FUNCTION_KEY}{}={k}\n", i + 1))
                .collect::<String>(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let s = Settings {
            upper_case_letters: false,
            ctrl_c_stops: true,
            editor_wrap: true,
            editor_number_tiles: false,
            editor_font: FontSize::Small,
            editor_size: EditorSize::Small,
            editor_colors: crate::palette::PRESETS[2].1,
            file_confirm: FileConfirm::Notify,
            file_hint_shown: true,
            screen_filter: None,
            keyboard_buttons: false,
            smilebasic_instruments: false,
            sound_font: Some("/fonts/a b.sf2".to_string()),
            editor_tab_step: 2,
            language: Language::Japanese,
            keep_mouse_cursor: true,
            function_keys: ["A", "B", "", "LIST ERR", "\"Q"].map(String::from),
            ..Settings::default()
        };
        assert_eq!(Settings::from_text(&s.to_text()), s);
        assert_eq!(
            Settings::from_text(&Settings::default().to_text()),
            Settings::default()
        );
    }

    #[test]
    fn skips_what_it_does_not_know() {
        let s = Settings::from_text(
            "# comment\nfuture_option=1\nctrl_c_stops = true\neditor_wrap=maybe\n\
             file_confirm=loud\nno equals sign\neditor_tab_step=0\nlanguage=fr\n",
        );
        assert_eq!(
            s,
            Settings {
                ctrl_c_stops: true,
                ..Settings::default()
            }
        );
    }
}

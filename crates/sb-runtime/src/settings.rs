//! The emulator's settings, and their text form for keeping them between
//! runs: one `key=value` per line, `#` for comments. Unknown keys and bad
//! values are skipped, so files from other versions still load.

use crate::confirm::FileConfirm;
use crate::screen::Filter;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    pub file_confirm: FileConfirm,
    /// A file question has already said how to turn such questions off.
    pub file_hint_shown: bool,
    /// How the screen is scaled to the window; `None` follows XSCREEN.
    pub screen_filter: Option<Filter>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            upper_case_letters: true,
            ctrl_c_stops: false,
            half_width_on_save: true,
            editor_wrap: false,
            file_confirm: FileConfirm::Ask,
            file_hint_shown: false,
            screen_filter: Some(Filter::Nearest),
        }
    }
}

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
            let flag = match key {
                "upper_case_letters" => &mut s.upper_case_letters,
                "ctrl_c_stops" => &mut s.ctrl_c_stops,
                "half_width_on_save" => &mut s.half_width_on_save,
                "editor_wrap" => &mut s.editor_wrap,
                "file_hint_shown" => &mut s.file_hint_shown,
                "file_confirm" => {
                    if let Some(&(mode, _)) = FILE_CONFIRM.iter().find(|(_, n)| *n == value) {
                        s.file_confirm = mode;
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
             file_confirm={mode}\n\
             file_hint_shown={}\n\
             screen_filter={filter}\n",
            self.upper_case_letters,
            self.ctrl_c_stops,
            self.half_width_on_save,
            self.editor_wrap,
            self.file_hint_shown,
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
            file_confirm: FileConfirm::Notify,
            file_hint_shown: true,
            screen_filter: None,
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
             file_confirm=loud\nno equals sign\n",
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

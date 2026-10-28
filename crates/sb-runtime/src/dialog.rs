//! DIALOG: a window over the screen that the program waits on. The
//! frontend draws it and answers clicks and typing through
//! [`Runtime::answer_dialog`]; controller buttons, the touch screen and
//! the timeout are watched here.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::Value;

use crate::input::{A, B};
use crate::runtime::{Command, Runtime, illegal, opt_int};

const TEXT_CHARS: usize = 256;
const CAPTION_CHARS: usize = 46;
const NAME_CAPTION_CHARS: usize = 39;
const NAME_CHARS: usize = 32;
const DEFAULT_CAPTION: &str = "DIALOG";
/// DIALOG's button bit for the touch screen.
const TOUCH_BIT: u32 = 1 << 17;
const FRAMES_PER_SECOND: i32 = 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogKind {
    /// Buttons to choose from, left to right; with two, the left one is
    /// the negative answer.
    Choice(Vec<&'static str>),
    /// Waits for any of these controller buttons (DIALOG's bit numbers),
    /// without buttons of its own.
    Buttons(u32),
    /// A file name, typed in a field of at most `max` characters.
    Name { initial: String, max: usize },
}

/// A dialog the program waits on, for the frontend to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramDialog {
    pub caption: String,
    pub lines: Vec<String>,
    pub kind: DialogKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogAnswer {
    /// The index of the button chosen.
    Choice(usize),
    /// DIALOG's bits of what was pressed or touched.
    Buttons(u32),
    /// The name typed, or `None` when cancelled.
    Name(Option<String>),
}

pub(crate) struct Waiting {
    dialog: ProgramDialog,
    /// Frames before it closes by itself.
    frames_left: Option<u32>,
}

/// Labels of SELECTION types 0 to 5.
fn choices(kind: i32) -> Result<Vec<&'static str>> {
    Ok(match kind {
        0 => vec!["OK"],
        1 => vec!["No", "Yes"],
        2 => vec!["Back", "Next"],
        3 => vec!["Cancel", "OK"],
        4 => vec!["Cancel", "Execute"],
        5 => vec!["Next"],
        n => {
            return Err(Error::with_detail(
                ErrorKind::OutOfRange,
                format!("type {n}"),
            ));
        }
    })
}

/// RESULT() for a choice: 1 for the right (or only) button, -1 for the
/// left of two.
fn choice_result(buttons: usize, chosen: usize) -> i32 {
    if buttons == 2 && chosen == 0 { -1 } else { 1 }
}

fn text(v: &Value) -> Result<String> {
    Ok(String::from_utf16_lossy(&v.clone().into_str()?.borrow()))
}

fn shortened(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn lines(text: &str) -> Vec<String> {
    let text = shortened(text, TEXT_CHARS).replace("\r\n", "\n");
    text.split(['\n', '\r']).map(str::to_string).collect()
}

/// A timeout in seconds, or in frames when negative; 0 waits for ever.
fn timeout(v: Option<&Value>) -> Result<Option<u32>> {
    Ok(match v.map(opt_int).transpose()?.flatten().unwrap_or(0) {
        0 => None,
        n if n < 0 => Some(n.unsigned_abs()),
        n => Some(n.saturating_mul(FRAMES_PER_SECOND) as u32),
    })
}

fn caption(v: Option<&Value>, max: usize) -> Result<String> {
    match v {
        None | Some(Value::Default) => Ok(DEFAULT_CAPTION.to_string()),
        Some(v) => Ok(shortened(&text(v)?, max)),
    }
}

/// Whether `c` may be typed in a file name.
pub fn name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')
}

impl Runtime {
    /// The dialog the program waits on, if any.
    pub fn program_dialog(&self) -> Option<&ProgramDialog> {
        self.dialog.as_ref().map(|w| &w.dialog)
    }

    /// Closes the program's dialog with an answer.
    pub fn answer_dialog(&mut self, answer: DialogAnswer) {
        if self.dialog.is_some() {
            self.dialog_answer = Some(answer);
        }
    }

    pub(crate) fn dialog_command(
        &mut self,
        _command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        let (dialog, frames) = match (want, args) {
            (Want::Outs(0), [message, rest @ ..]) if rest.len() <= 3 => {
                let kind = rest.first().map(opt_int).transpose()?.flatten();
                let dialog = ProgramDialog {
                    caption: caption(rest.get(1), CAPTION_CHARS)?,
                    lines: lines(&text(message)?),
                    kind: DialogKind::Choice(choices(kind.unwrap_or(0))?),
                };
                (dialog, timeout(rest.get(2))?)
            }
            (Want::Value, [initial, Value::Str(_), rest @ ..]) if rest.len() <= 1 => {
                let initial = text(initial)?;
                if !initial.chars().all(name_char) {
                    let detail = format!("file name {initial:?}");
                    return Err(Error::with_detail(ErrorKind::IllegalFunctionCall, detail));
                }
                let max = match rest.first().map(opt_int).transpose()?.flatten() {
                    None => NAME_CHARS,
                    Some(n @ 1..=32) => n as usize,
                    Some(n) => {
                        return Err(Error::with_detail(ErrorKind::OutOfRange, format!("{n}")));
                    }
                };
                let dialog = ProgramDialog {
                    caption: caption(args.get(1), NAME_CAPTION_CHARS)?,
                    lines: Vec::new(),
                    kind: DialogKind::Name {
                        initial: shortened(&initial, max),
                        max,
                    },
                };
                (dialog, None)
            }
            (Want::Value, [message, mask, rest @ ..]) if rest.len() <= 2 => {
                let mask = mask.to_int()? as u32;
                if mask == 0 {
                    return Err(Error::with_detail(ErrorKind::OutOfRange, "no buttons"));
                }
                let dialog = ProgramDialog {
                    caption: caption(rest.first(), CAPTION_CHARS)?,
                    lines: lines(&text(message)?),
                    kind: DialogKind::Buttons(mask),
                };
                (dialog, timeout(rest.get(1))?)
            }
            _ => return Err(illegal()),
        };
        let answer = match self.dialog.as_mut() {
            None => {
                self.dialog = Some(Waiting {
                    dialog,
                    frames_left: frames,
                });
                None
            }
            Some(waiting) => self.dialog_answer.take().or_else(|| {
                let answer = self.input.dialog_answer(&waiting.dialog.kind);
                match &mut waiting.frames_left {
                    _ if answer.is_some() => answer,
                    // Timing out answers 0, for a choice as for buttons.
                    Some(0) => Some(DialogAnswer::Buttons(0)),
                    Some(n) => {
                        *n -= 1;
                        None
                    }
                    None => None,
                }
            }),
        };
        let Some(answer) = answer else {
            self.retry = true;
            return Ok(vec![Value::Int(0)]);
        };
        let kind = self.dialog.take().expect("a dialog was open").dialog.kind;
        match (kind, answer) {
            (DialogKind::Choice(labels), DialogAnswer::Choice(i)) => {
                self.result = choice_result(labels.len(), i);
                Ok(vec![])
            }
            (DialogKind::Choice(_), _) => {
                self.result = 0;
                Ok(vec![])
            }
            (DialogKind::Buttons(mask), DialogAnswer::Buttons(bits)) => {
                Ok(vec![Value::Int((bits & mask) as i32)])
            }
            (DialogKind::Buttons(_), _) => Ok(vec![Value::Int(0)]),
            (DialogKind::Name { .. }, DialogAnswer::Name(Some(name))) => {
                self.result = 1;
                Ok(vec![Value::str(&name)])
            }
            (DialogKind::Name { .. }, _) => {
                self.result = -1;
                Ok(vec![Value::str("")])
            }
        }
    }
}

impl crate::input::Input {
    /// What the controllers and touch screen answer a dialog with this
    /// frame: A chooses the right (or only) button and B the left of two;
    /// a wait for buttons ends on any of them going down.
    // UNVERIFIED: which buttons choose in SmileBASIC 4's dialogs.
    pub(crate) fn dialog_answer(&self, kind: &DialogKind) -> Option<DialogAnswer> {
        let pressed = self.pressed();
        match kind {
            DialogKind::Choice(labels) if pressed & 1 << A != 0 => {
                Some(DialogAnswer::Choice(labels.len() - 1))
            }
            DialogKind::Choice(labels) if labels.len() == 2 && pressed & 1 << B != 0 => {
                Some(DialogAnswer::Choice(0))
            }
            DialogKind::Buttons(mask) => {
                let bits = pressed | if self.touched() { TOUCH_BIT } else { 0 };
                (bits & mask != 0).then_some(DialogAnswer::Buttons(bits & mask))
            }
            _ => None,
        }
    }
}

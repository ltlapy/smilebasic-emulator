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
/// #B_RANY, #B_LANY and #B_ANY.
const ANY_BITS: u32 = 0b111 << 14;
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
    /// Buttons pressed since it opened, which choose when let go.
    pressed: u32,
    /// For the dialog that says a file command failed: what the command
    /// returns once it is closed.
    failure: Option<Vec<Value>>,
}

/// How the dialog that says a file command failed stands.
pub(crate) enum Failure {
    /// There is none.
    None,
    /// It waits for an answer; the command runs again next frame.
    Waiting(Vec<Value>),
    /// It was just closed.
    Closed(Vec<Value>),
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

/// A caption longer than `max` keeps one character less and ends in
/// "……" (checked on hardware: 45 characters of a 50-character caption
/// show). UNVERIFIED: the same for the file name dialog's 39.
fn cut_caption(text: &str, max: usize) -> String {
    match text.chars().count() > max {
        true => format!("{}……", shortened(text, max - 1)),
        false => text.to_string(),
    }
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
        Some(v) => Ok(cut_caption(&text(v)?, max)),
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
                    let detail = format!("{initial:?}");
                    return Err(Error::with_detail(ErrorKind::IllegalFileName, detail));
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
                    pressed: 0,
                    failure: None,
                });
                None
            }
            Some(waiting) => self.dialog_answer.take().or_else(|| {
                waiting.pressed |= self.input.pressed();
                let answer = self
                    .input
                    .dialog_answer(&waiting.dialog.kind, waiting.pressed);
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
        // The frontend answers on Enter going down, and A on being let go,
        // which the program would see in the frame the dialog returns in.
        self.input.hide_presses();
        match (kind, answer) {
            (DialogKind::Choice(labels), DialogAnswer::Choice(i)) => {
                self.result = choice_result(labels.len(), i);
                Ok(vec![])
            }
            (DialogKind::Choice(_), _) => {
                self.result = 0;
                Ok(vec![])
            }
            (DialogKind::Buttons(_), DialogAnswer::Buttons(bits)) => {
                Ok(vec![Value::Int(bits as i32)])
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

impl Runtime {
    /// A running program's file command failed: SmileBASIC 4 says so in a
    /// dialog with one button, and the command waits until it is closed,
    /// however long that takes. Then RESULT() is 0 and the program goes on.
    // UNVERIFIED: the dialog for failures other than a missing file.
    pub(crate) fn open_failure(&mut self, what: &str, message: String) {
        let mut chars = message.chars();
        let first = chars.next().map(|c| c.to_ascii_uppercase());
        let message = first.into_iter().chain(chars).collect();
        self.dialog = Some(Waiting {
            dialog: ProgramDialog {
                caption: format!("{what} failed"),
                lines: vec![message],
                kind: DialogKind::Choice(vec!["OK"]),
            },
            frames_left: None,
            pressed: 0,
            failure: Some(Vec::new()),
        });
    }

    /// Whether [`Runtime::open_failure`]'s dialog is open.
    pub(crate) fn failure_open(&self) -> bool {
        self.dialog.as_ref().is_some_and(|w| w.failure.is_some())
    }

    /// Watches the dialog [`Runtime::open_failure`] opened, as DIALOG
    /// watches its own: A or the frontend's answer closes it.
    pub(crate) fn watch_failure(&mut self) -> Failure {
        let Some(waiting) = self.dialog.as_mut() else {
            return Failure::None;
        };
        let Some(outs) = &waiting.failure else {
            return Failure::None;
        };
        waiting.pressed |= self.input.pressed();
        let answered = self.dialog_answer.take().is_some()
            || (self.input)
                .dialog_answer(&waiting.dialog.kind, waiting.pressed)
                .is_some();
        if !answered {
            self.retry = true;
            return Failure::Waiting(outs.clone());
        }
        let outs = self.dialog.take().and_then(|w| w.failure);
        self.input.hide_presses();
        self.result = 0;
        Failure::Closed(outs.unwrap_or_default())
    }

    /// Runs a file command, which waits while the dialog of its failure
    /// is open and then returns what it returned when it failed, without
    /// running again (it may have asked a question first).
    pub(crate) fn after_failure(
        &mut self,
        run: impl FnOnce(&mut Runtime) -> Result<Vec<Value>>,
    ) -> Result<Vec<Value>> {
        if self.dialog.is_some() {
            match self.watch_failure() {
                Failure::Waiting(outs) | Failure::Closed(outs) => return Ok(outs),
                Failure::None => {}
            }
        }
        let outs = run(self);
        if let Some(waiting) = self.dialog.as_mut()
            && let Some(saved) = &mut waiting.failure
        {
            match &outs {
                Ok(outs) => {
                    saved.clone_from(outs);
                    self.retry = true;
                }
                // The program stops with the error instead.
                Err(_) => self.dialog = None,
            }
        }
        outs
    }
}

impl crate::input::Input {
    /// What the controllers and touch screen answer a dialog with this
    /// frame: A chooses the right (or only) button and B the left of two,
    /// when let go after being pressed with the dialog open (checked on
    /// hardware for A); a wait for buttons ends on
    /// any of them going down.
    pub(crate) fn dialog_answer(&self, kind: &DialogKind, since: u32) -> Option<DialogAnswer> {
        let released = self.released() & since;
        match kind {
            DialogKind::Choice(labels) if released & 1 << A != 0 => {
                Some(DialogAnswer::Choice(labels.len() - 1))
            }
            DialogKind::Choice(labels) if labels.len() == 2 && released & 1 << B != 0 => {
                Some(DialogAnswer::Choice(0))
            }
            // The answer keeps the buttons waited for with #B_RANY, #B_LANY
            // and #B_ANY as they go on, and leaves out other buttons held.
            DialogKind::Buttons(mask) => {
                let touch = if self.touched() { TOUCH_BIT } else { 0 };
                let bits = self.pressed() | touch;
                (bits & mask != 0).then_some(DialogAnswer::Buttons(bits & (mask | ANY_BITS)))
            }
            _ => None,
        }
    }
}

//! DIALOG: a window over the screen that the program waits on. The
//! frontend draws it and answers clicks and typing through
//! [`Runtime::answer_dialog`]; controller buttons, the touch screen and
//! the timeout are watched here.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::Value;

use crate::input::{A, ANY_ID, B, Input, LANY_ID, LEFT, RANY_ID, RIGHT};
use crate::runtime::{Command, Runtime, checking, illegal, opt_int};
use crate::storage;

const TEXT_CHARS: usize = 256;
/// The file name form cuts its caption the same as the other forms do, not
/// at the 39 characters the manual gives.
const CAPTION_CHARS: usize = 46;
const NAME_CHARS: usize = 32;
const DEFAULT_CAPTION: &str = "DIALOG";
/// DIALOG's button bit for the touch screen.
const TOUCH_BIT: u32 = 1 << 17;
const ANY_BITS: u32 = 1 << RANY_ID | 1 << LANY_ID | 1 << ANY_ID;
/// Every button DIALOG can wait for, the touch screen's bit included.
const BUTTON_BITS: i32 = (TOUCH_BIT << 1) as i32 - 1;
const FRAMES_PER_SECOND: i32 = 60;
/// The distance the left stick is pushed before it moves the focus between
/// a dialog's buttons.
// UNVERIFIED: the distance, and that each push moves it once.
const FOCUS_STICK: f64 = 0.5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogKind {
    /// Buttons to choose from, left to right; with two, the left one is
    /// the negative answer.
    Choice(Vec<&'static str>),
    /// Waits for any of these controller buttons (DIALOG's bit numbers),
    /// without its own buttons.
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
    /// Buttons pressed since it opened, which choose when released.
    pressed: u32,
    /// The button of a choice that A chooses, which the D-pad, the left
    /// stick and the frontend move.
    focus: usize,
    /// The direction the left stick was pushed in the last frame (-1, 0 or
    /// 1), or `None` before the first frame, so that a stick already pushed
    /// when the dialog opens moves nothing.
    stick: Option<i8>,
    /// For the dialog that says a file command failed: what the command
    /// returns once it is closed.
    failure: Option<Vec<Value>>,
    /// Opened since [`link_dialogs`] last checked.
    fresh: bool,
    /// This call's own form, in which the answer is given, while the later
    /// dialog of the other interpreter shows in place of this one.
    own: Option<DialogKind>,
    /// Waits for the answer [`link_dialogs`] passes on, without watching
    /// the controls or a timeout itself.
    passive: bool,
}

impl Waiting {
    fn new(dialog: ProgramDialog, frames_left: Option<u32>, failure: Option<Vec<Value>>) -> Self {
        // The right (or only) button has the focus at first.
        let focus = match &dialog.kind {
            DialogKind::Choice(labels) => labels.len().saturating_sub(1),
            _ => 0,
        };
        Waiting {
            dialog,
            frames_left,
            pressed: 0,
            focus,
            stick: None,
            failure,
            fresh: true,
            own: None,
            passive: false,
        }
    }

    /// The number of buttons of a choice, if it is one.
    fn button_count(&self) -> Option<usize> {
        match &self.dialog.kind {
            DialogKind::Choice(labels) => Some(labels.len()),
            _ => None,
        }
    }

    /// Moves the focus of a choice by one button for each press of Left or
    /// Right on the D-pad and each push of the left stick, and stops at the
    /// ends.
    // UNVERIFIED: that it stops at the ends rather than wrapping around, and
    // that Up and Down do nothing.
    fn steer(&mut self, input: &Input) {
        let Some(n) = self.button_count() else {
            return;
        };
        let x = input.left_stick().0;
        let over = match x {
            x if x < -FOCUS_STICK => -1,
            x if x > FOCUS_STICK => 1,
            _ => 0,
        };
        let pushed = match self.stick.replace(over) {
            Some(was) if was != over => over,
            _ => 0,
        };
        let pressed = input.pressed();
        let left = pressed & 1 << LEFT != 0 || pushed < 0;
        let right = pressed & 1 << RIGHT != 0 || pushed > 0;
        match (left, right) {
            (true, false) => self.focus = self.focus.saturating_sub(1),
            (false, true) => self.focus = (self.focus + 1).min(n - 1),
            _ => {}
        }
    }
}

/// The state of the dialog that says a file command failed.
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
    checking(v, || {
        Ok(String::from_utf16_lossy(&v.clone().into_str()?.borrow()))
    })
}

fn shortened(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// A caption longer than `max` keeps one character less and ends in
/// "……": 45 characters of a 50-character caption show.
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

/// A timeout in seconds, or in frames when negative; 0 waits forever.
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

    /// The button of the program's dialog that A and Enter choose, for a
    /// dialog with buttons to choose from.
    pub fn dialog_focus(&self) -> Option<usize> {
        let waiting = self.dialog.as_ref()?;
        waiting.button_count().map(|_| waiting.focus)
    }

    /// Moves the focus of the program's dialog to button `i`, for the
    /// frontend's keys and mouse. A dialog without buttons to choose from
    /// keeps no focus.
    pub fn set_dialog_focus(&mut self, i: usize) {
        if let Some(waiting) = self.dialog.as_mut()
            && let Some(n) = waiting.button_count()
        {
            waiting.focus = i.min(n.saturating_sub(1));
        }
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
        // Each form checks its arguments in order, the same as SmileBASIC 4's
        // handler does (4.4.8).
        let (dialog, frames) = match (want, args) {
            (Want::Outs(0), [message, rest @ ..]) if rest.len() <= 3 => {
                let lines = lines(&text(message)?);
                let kind = match rest.first() {
                    Some(kind) => checking(kind, || choices(opt_int(kind)?.unwrap_or(0)))?,
                    None => choices(0)?,
                };
                let dialog = ProgramDialog {
                    caption: caption(rest.get(1), CAPTION_CHARS)?,
                    lines,
                    kind: DialogKind::Choice(kind),
                };
                (dialog, timeout(rest.get(2))?)
            }
            // Two strings ask for a file name; a number after the message
            // waits for buttons.
            (Want::Value, [initial @ Value::Str(_), Value::Str(_), rest @ ..])
                if rest.len() <= 1 =>
            {
                let max = match rest.first().map(opt_int).transpose()?.flatten() {
                    None => NAME_CHARS,
                    Some(n @ 1..=32) => n as usize,
                    Some(n) => {
                        let e = Error::with_detail(ErrorKind::OutOfRange, format!("{n}"));
                        return Err(e.of_arg(2));
                    }
                };
                // After it is cut to the maximum length, the initial name is
                // checked the same way SmileBASIC 4's file commands check a
                // name, and it must have no project.
                let initial = shortened(&text(initial)?, max);
                if !storage::split_name(&initial).is_some_and(|parts| parts.project.is_none()) {
                    let detail = format!("{initial:?}");
                    let error = Error::with_detail(ErrorKind::IllegalFileName, detail);
                    return Err(error.of_arg(0));
                }
                let dialog = ProgramDialog {
                    caption: caption(args.get(1), CAPTION_CHARS)?,
                    lines: Vec::new(),
                    kind: DialogKind::Name { initial, max },
                };
                (dialog, None)
            }
            (Want::Value, [message, mask, rest @ ..]) if rest.len() <= 2 => {
                let lines = lines(&text(message)?);
                let mask = checking(mask, || match mask.to_int()? {
                    m @ 1..=BUTTON_BITS => Ok(m as u32),
                    m => Err(Error::with_detail(
                        ErrorKind::OutOfRange,
                        format!("buttons {m}"),
                    )),
                })?;
                let dialog = ProgramDialog {
                    caption: caption(rest.first(), CAPTION_CHARS)?,
                    lines,
                    kind: DialogKind::Buttons(mask),
                };
                (dialog, timeout(rest.get(1))?)
            }
            _ => return Err(illegal()),
        };
        let answer = match self.dialog {
            None => {
                self.dialog = Some(Waiting::new(dialog, frames, None));
                None
            }
            Some(_) => self.poll_dialog(),
        };
        let Some(answer) = answer else {
            self.retry = true;
            return Ok(vec![Value::Int(0)]);
        };
        let waiting = self.dialog.take().expect("a dialog was open");
        self.dialog_closed = Some(answer.clone());
        // The frontend answers when Enter is pressed, and A answers when A
        // is released. The program would see either in the frame the dialog
        // returns in.
        self.input.hide_presses();
        // The answer is read by the dialog shown, which may belong to the
        // other interpreter, and given in the form this call asked for. A
        // choice takes RESULT() 1 for any buttons pressed, and a wait for
        // buttons takes -1 for the left of two.
        let shown_buttons = match &waiting.dialog.kind {
            DialogKind::Choice(labels) => labels.len(),
            _ => 0,
        };
        let kind = waiting.own.unwrap_or(waiting.dialog.kind);
        match (kind, answer) {
            (DialogKind::Choice(_), DialogAnswer::Choice(i)) => {
                self.result = choice_result(shown_buttons, i);
                Ok(vec![])
            }
            (DialogKind::Choice(_), DialogAnswer::Buttons(bits)) => {
                self.result = i32::from(bits != 0);
                Ok(vec![])
            }
            (DialogKind::Choice(_), _) => {
                self.result = 0;
                Ok(vec![])
            }
            (DialogKind::Buttons(_), DialogAnswer::Buttons(bits)) => {
                Ok(vec![Value::Int(bits as i32)])
            }
            // The right button of two gives 0 and RESULT() 1.
            // UNVERIFIED: RESULT() for the left one, and for an only button.
            (DialogKind::Buttons(_), DialogAnswer::Choice(i)) => {
                self.result = choice_result(shown_buttons, i);
                Ok(vec![Value::Int(self.result.min(0))])
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
    /// however long that takes. Then RESULT() is 0 and the program continues.
    // UNVERIFIED: the dialog for failures other than a missing file.
    pub(crate) fn open_failure(&mut self, what: &str, message: String) {
        let mut chars = message.chars();
        let first = chars.next().map(|c| c.to_ascii_uppercase());
        let message = first.into_iter().chain(chars).collect();
        let dialog = ProgramDialog {
            caption: format!("{what} failed"),
            lines: vec![message],
            kind: DialogKind::Choice(vec!["OK"]),
        };
        self.dialog = Some(Waiting::new(dialog, None, Some(Vec::new())));
    }

    /// Whether [`Runtime::open_failure`]'s dialog is open.
    pub(crate) fn failure_open(&self) -> bool {
        self.dialog.as_ref().is_some_and(|w| w.failure.is_some())
    }

    /// Watches the dialog [`Runtime::open_failure`] opened, the same way
    /// DIALOG watches its own: A or the frontend's answer closes it.
    pub(crate) fn watch_failure(&mut self) -> Failure {
        let Some(waiting) = self.dialog.as_mut() else {
            return Failure::None;
        };
        if waiting.failure.is_none() {
            return Failure::None;
        }
        let Some(answer) = self.poll_dialog() else {
            self.retry = true;
            let outs = self.dialog.as_ref().and_then(|w| w.failure.clone());
            return Failure::Waiting(outs.unwrap_or_default());
        };
        self.dialog_closed = Some(answer);
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

impl Runtime {
    /// The answer the open dialog gets this frame: from the frontend, the
    /// controls, the touch screen or the timeout.
    fn poll_dialog(&mut self) -> Option<DialogAnswer> {
        let waiting = self.dialog.as_mut()?;
        if let Some(answer) = self.dialog_answer.take() {
            return Some(answer);
        }
        if waiting.passive {
            return None;
        }
        waiting.pressed |= self.input.pressed();
        waiting.steer(&self.input);
        let since = waiting.pressed;
        let answer = (self.input).dialog_answer(&waiting.dialog.kind, since, waiting.focus);
        match &mut waiting.frames_left {
            _ if answer.is_some() => answer,
            // Timing out answers 0, both for a choice and for buttons.
            Some(0) => Some(DialogAnswer::Buttons(0)),
            Some(n) => {
                *n -= 1;
                None
            }
            None => None,
        }
    }
}

/// Makes the dialogs of the main and the sub interpreter one dialog, the
/// same as SmileBASIC 4, which has one for both. The text of the later call
/// shows, and one answer closes both in the same frame. Called after the
/// frame of each interpreter, the main interpreter first.
///
/// The dialog of the later call replaces the earlier one permanently, with
/// its own buttons and timeout. A main program behind the dialog of the
/// subprogram watches that dialog itself, because it runs first in a
/// frame. A subprogram behind the dialog of the main program takes the
/// answer passed on here, after the frame of the main program and before
/// its own.
///
/// Neither program stops while both wait, except for the stop answer of
/// the dialog, which stops both (see [`crate::machine::Machine::stop`]).
pub(crate) fn link_dialogs(main: &mut Runtime, sub: Option<&mut Runtime>) {
    let Some(sub) = sub else {
        main.dialog_closed = None;
        return;
    };
    link(main, sub, true);
    link(sub, main, false);
}

/// Hands `from`'s dialog to `to`, which is behind it.
fn link(from: &mut Runtime, to: &mut Runtime, to_is_sub: bool) {
    if let Some(answer) = from.dialog_closed.take()
        && to.dialog.as_ref().is_some_and(|w| w.own.is_some())
    {
        to.dialog_answer.get_or_insert(answer);
    }
    if let Some(new) = from.dialog.as_mut().filter(|w| w.fresh) {
        new.fresh = false;
        if let Some(old) = to.dialog.as_mut() {
            if old.own.is_none() {
                old.own = Some(old.dialog.kind.clone());
            }
            old.dialog.clone_from(&new.dialog);
            old.frames_left = new.frames_left;
            old.pressed = new.pressed;
            old.focus = new.focus;
            old.stick = new.stick;
            old.fresh = false;
            old.passive = to_is_sub;
        }
    }
    // If the later call still ends without an answer, the earlier one keeps
    // waiting in its place and watches the controls itself.
    if from.dialog.is_none()
        && to.dialog_answer.is_none()
        && let Some(waiting) = to.dialog.as_mut()
    {
        waiting.passive = false;
    }
}

impl crate::input::Input {
    /// The answer the controllers and touch screen give a dialog this frame.
    /// A and B answer when released after being pressed with the dialog open
    /// (checked on hardware for A): A chooses the button with the focus, and
    /// B chooses the left of two regardless of the focus. A wait for buttons
    /// ends when any of them is pressed.
    pub(crate) fn dialog_answer(
        &self,
        kind: &DialogKind,
        since: u32,
        focus: usize,
    ) -> Option<DialogAnswer> {
        let released = self.released() & since;
        match kind {
            DialogKind::Choice(labels) if released & 1 << A != 0 => {
                Some(DialogAnswer::Choice(focus.min(labels.len() - 1)))
            }
            DialogKind::Choice(labels) if labels.len() == 2 && released & 1 << B != 0 => {
                Some(DialogAnswer::Choice(0))
            }
            // The answer keeps the buttons waited for, with #B_RANY, #B_LANY
            // and #B_ANY as they turn on, and removes other buttons held.
            DialogKind::Buttons(mask) => {
                let touch = if self.touched() { TOUCH_BIT } else { 0 };
                let bits = self.pressed() | touch;
                (bits & mask != 0).then_some(DialogAnswer::Buttons(bits & (mask | ANY_BITS)))
            }
            _ => None,
        }
    }
}

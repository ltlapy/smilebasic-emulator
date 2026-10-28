//! Programs editing other slots' source: PRGEDIT, PRGGET$, PRGSET, PRGINS,
//! PRGDEL, PRGSEEK, PRGSIZE and PRGNAME$.
//!
//! Lines are counted as the editor shows them: an empty slot has one empty
//! line. The current line may also be one past the last, where PRGGET$
//! gives an empty string and PRGSET adds a line.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::Value;

use crate::editor::split_lines;
use crate::runtime::{Command, Runtime, illegal, opt_int, slot_arg};

/// The slot PRGEDIT chose and its current line, from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrgEdit {
    pub slot: usize,
    pub line: usize,
}

/// What PRGSIZE reports as free characters.
// UNVERIFIED: how much source a slot holds on SmileBASIC 4; this is the
// emulator's own limit for reporting only.
const SLOT_CHARS: i32 = 1 << 20;

fn lines_of(source: &str) -> Vec<Vec<u16>> {
    split_lines(source)
}

fn text_of(lines: &[Vec<u16>]) -> String {
    let lines: Vec<String> = lines.iter().map(|l| String::from_utf16_lossy(l)).collect();
    lines.join("\n")
}

fn str_units(v: &Value) -> Result<Vec<u16>> {
    Ok(v.clone().into_str()?.borrow().clone())
}

/// A string's lines, split at CHR$(10) or CHR$(13).
fn new_lines(text: &[u16]) -> Vec<Vec<u16>> {
    lines_of(&String::from_utf16_lossy(text))
}

impl Runtime {
    /// Forgets PRGEDIT's editing when a slot's program is replaced.
    pub(crate) fn drop_prg_edit(&mut self, slot: usize) {
        if self.prg_edit.is_some_and(|e| e.slot == slot) {
            self.prg_edit = None;
        }
    }

    /// The slot PRGNAME$ and PRGSIZE mean when none is given: the running
    /// one.
    // UNVERIFIED: slot 0 in direct mode.
    fn current_slot(&self, arg: Option<&Value>) -> Result<usize> {
        match arg.map(opt_int).transpose()?.flatten() {
            None => Ok(self.running_slot.unwrap_or(0)),
            Some(_) => slot_arg(arg),
        }
    }

    fn editing(&self) -> Result<PrgEdit> {
        self.prg_edit.ok_or_else(|| ErrorKind::UsePrgedit.into())
    }

    /// Changes the edited slot's lines, marking it as unsaved.
    fn change_lines(&mut self, edit: PrgEdit, f: impl FnOnce(&mut Vec<Vec<u16>>, &mut usize)) {
        let mut lines = lines_of(&self.slots[edit.slot]);
        let mut line = edit.line;
        f(&mut lines, &mut line);
        if lines.is_empty() {
            lines.push(Vec::new());
        }
        self.slots[edit.slot] = text_of(&lines);
        self.slot_modified[edit.slot] = true;
        self.prg_edit = Some(PrgEdit {
            slot: edit.slot,
            line: line.min(lines.len()),
        });
    }

    pub(crate) fn prg_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        match (command, want, args) {
            (Command::Prgedit, Want::Outs(0), [slot, rest @ ..]) if rest.len() <= 1 => {
                let slot = slot_arg(Some(slot))?;
                if self.running_slot == Some(slot) {
                    let detail = format!("slot {slot}");
                    return Err(Error::with_detail(
                        ErrorKind::CantEditRunningProgram,
                        detail,
                    ));
                }
                let count = lines_of(&self.slots[slot]).len();
                let line = match rest.first().map(opt_int).transpose()?.flatten() {
                    None => 0,
                    Some(-1) => count - 1,
                    Some(n @ 1..) if n as usize <= count => n as usize - 1,
                    Some(n) => {
                        let detail = format!("line {n}");
                        return Err(Error::with_detail(ErrorKind::OutOfRange, detail));
                    }
                };
                self.prg_edit = Some(PrgEdit { slot, line });
                Ok(vec![])
            }
            (Command::Prgedit, Want::Value, []) => {
                Ok(vec![Value::Int(self.editing()?.line as i32 + 1)])
            }
            (Command::Prgedit, Want::Outs(n @ (1 | 2)), []) => {
                let edit = self.editing()?;
                let line = Value::Int(edit.line as i32 + 1);
                Ok(match n {
                    1 => vec![line],
                    _ => vec![Value::Int(edit.slot as i32), line],
                })
            }
            (Command::Prgget, Want::Value, _) if args.len() <= 1 => {
                let seek = match args.first().map(opt_int).transpose()?.flatten() {
                    None => true,
                    Some(n) => n != 0,
                };
                let edit = self.editing()?;
                let lines = lines_of(&self.slots[edit.slot]);
                let text = lines.get(edit.line).cloned().unwrap_or_default();
                if seek && edit.line < lines.len() {
                    self.prg_edit = Some(PrgEdit {
                        line: edit.line + 1,
                        ..edit
                    });
                }
                Ok(vec![Value::Str(sb_core::value::new_str(text))])
            }
            (Command::Prgseek, Want::Value | Want::Outs(0), _) if args.len() <= 1 => {
                let step = args
                    .first()
                    .map(opt_int)
                    .transpose()?
                    .flatten()
                    .unwrap_or(1);
                let edit = self.editing()?;
                let count = lines_of(&self.slots[edit.slot]).len();
                // UNVERIFIED: PRGSEEK stops at the last line, while PRGGET$
                // goes one past it.
                let to = match step {
                    1 if edit.line + 1 < count => Some(edit.line + 1),
                    -1 if edit.line > 0 => Some(edit.line - 1),
                    1 | -1 => None,
                    n => {
                        let detail = format!("direction {n}");
                        return Err(Error::with_detail(ErrorKind::OutOfRange, detail));
                    }
                };
                if let Some(line) = to {
                    self.prg_edit = Some(PrgEdit { line, ..edit });
                }
                Ok(match want {
                    Want::Value => vec![Value::Int(i32::from(to.is_some()))],
                    _ => vec![],
                })
            }
            (Command::Prgset, Want::Outs(0), [text]) => {
                let new = new_lines(&str_units(text)?);
                let edit = self.editing()?;
                self.change_lines(edit, |lines, line| {
                    let end = (*line + 1).min(lines.len());
                    lines.splice(*line..end, new);
                });
                Ok(vec![])
            }
            // UNVERIFIED: where the current line is afterwards; here it
            // stays on the line it was, so that inserting before it again
            // keeps the order, and after inserting after, it is the last
            // line inserted.
            (Command::Prgins, Want::Outs(0), [text, rest @ ..]) if rest.len() <= 1 => {
                let new = new_lines(&str_units(text)?);
                let after = match rest.first().map(opt_int).transpose()?.flatten() {
                    None | Some(0) => false,
                    Some(1) => true,
                    Some(n) => {
                        return Err(Error::with_detail(ErrorKind::OutOfRange, format!("{n}")));
                    }
                };
                let edit = self.editing()?;
                self.change_lines(edit, |lines, line| {
                    let at = (*line + usize::from(after)).min(lines.len());
                    let n = new.len();
                    lines.splice(at..at, new);
                    *line += n;
                    if after {
                        *line = (*line).min(at + n - 1);
                    }
                });
                Ok(vec![])
            }
            // A negative count deletes every line (checked on hardware).
            (Command::Prgdel, Want::Outs(0), _) if args.len() <= 1 => {
                let n = args
                    .first()
                    .map(opt_int)
                    .transpose()?
                    .flatten()
                    .unwrap_or(1);
                let edit = self.editing()?;
                self.change_lines(edit, |lines, line| {
                    let (start, end) = match usize::try_from(n) {
                        Ok(n) => {
                            let start = (*line).min(lines.len());
                            (start, (start + n).min(lines.len()))
                        }
                        Err(_) => (0, lines.len()),
                    };
                    lines.drain(start..end);
                    if n < 0 {
                        *line = 0;
                    }
                });
                Ok(vec![])
            }
            (Command::Prgsize, Want::Value, _) if args.len() <= 2 => {
                let slot = self.current_slot(args.first())?;
                let source = &self.slots[slot];
                let chars = source.encode_utf16().count() as i32;
                let n = match args.get(1).map(opt_int).transpose()?.flatten() {
                    None | Some(0) => lines_of(source).len() as i32,
                    Some(1) => chars,
                    Some(2) => (SLOT_CHARS - chars).max(0),
                    Some(n) => {
                        return Err(Error::with_detail(
                            ErrorKind::OutOfRange,
                            format!("type {n}"),
                        ));
                    }
                };
                Ok(vec![Value::Int(n)])
            }
            (Command::Prgname, Want::Value, _) if args.len() <= 1 => {
                let slot = self.current_slot(args.first())?;
                Ok(vec![Value::str(&self.slot_names[slot])])
            }
            _ => Err(illegal()),
        }
    }
}

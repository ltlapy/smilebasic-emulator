//! Programs editing other slots' source: PRGEDIT, PRGGET$, PRGSET, PRGINS,
//! PRGDEL, PRGSEEK, PRGSIZE and PRGNAME$.
//!
//! As SmileBASIC 4 does it (checked on hardware with sb4_test07):
//! - A line is its text with the CHR$(10) that ends it, and PRGGET$ gives
//!   both. A last line without one is read as if it had it.
//! - An empty slot reads as one empty line, yet PRGSIZE counts 0 lines and
//!   1 character.
//! - The current line may go one past the last, where PRGGET$ gives an
//!   empty string; PRGEDIT() then still reports the last line.
//! - PRGSET on an empty line (or past the last) inserts before it, and
//!   otherwise replaces the line; either way the current line moves past
//!   what was set.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::Value;

use crate::runtime::{Command, Runtime, illegal, opt_int, slot_arg};

/// The slot PRGEDIT chose and its current line, from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrgEdit {
    pub slot: usize,
    pub line: usize,
}

/// What a slot holds, counting PRGSIZE's free characters: 1 MiB less one
/// for an empty slot (checked on hardware).
const SLOT_CHARS: i32 = 1 << 20;
const LF: u16 = 10;

/// A slot's source as lines, each ending in LF.
struct Source {
    lines: Vec<Vec<u16>>,
    /// The slot was empty, and its one line is only how it reads.
    empty: bool,
}

impl Source {
    fn of(text: &str) -> Source {
        let text: Vec<u16> = text.encode_utf16().collect();
        if text.is_empty() {
            return Source {
                lines: vec![vec![LF]],
                empty: true,
            };
        }
        Source {
            lines: split(&text),
            empty: false,
        }
    }

    /// Lines as PRGSIZE counts them.
    fn count(&self) -> usize {
        if self.empty { 0 } else { self.lines.len() }
    }

    fn chars(&self) -> usize {
        self.lines.iter().map(Vec::len).sum()
    }

    fn text(&self) -> String {
        let text: Vec<u16> = self.lines.concat();
        // UNVERIFIED: that a slot left with one empty line is empty again.
        if text == [LF] {
            return String::new();
        }
        String::from_utf16_lossy(&text)
    }
}

/// Lines each ending in LF, the last given one if it has none.
fn split(text: &[u16]) -> Vec<Vec<u16>> {
    let mut lines: Vec<Vec<u16>> = text
        .split_inclusive(|&c| c == LF)
        .map(<[u16]>::to_vec)
        .collect();
    if let Some(last) = lines.last_mut()
        && last.last() != Some(&LF)
    {
        last.push(LF);
    }
    lines
}

/// The lines a PRGSET or PRGINS string makes: it ends with a line break
/// of its own.
fn new_lines(v: &Value) -> Result<Vec<Vec<u16>>> {
    let mut text = v.clone().into_str()?.borrow().clone();
    text.push(LF);
    Ok(split(&text))
}

/// The line number PRGEDIT() reports for an index: never past the last.
fn reported(source: &Source, line: usize) -> i32 {
    (line + 1).min(source.count().max(1)) as i32
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
        let mut source = Source::of(&self.slots[edit.slot]);
        let mut line = edit.line;
        f(&mut source.lines, &mut line);
        source.empty = false;
        self.slots[edit.slot] = source.text();
        self.slot_modified[edit.slot] = true;
        self.prg_edit = Some(PrgEdit {
            slot: edit.slot,
            line: line.min(source.lines.len()),
        });
    }

    pub(crate) fn prg_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        let flag = |v: Option<&Value>, default: i32| -> Result<i32> {
            Ok(v.map(opt_int).transpose()?.flatten().unwrap_or(default))
        };
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
                let last = Source::of(&self.slots[slot]).count().max(1);
                let line = match flag(rest.first(), 1)? {
                    -1 => last - 1,
                    n @ 1.. if n as usize <= last => n as usize - 1,
                    n => {
                        let detail = format!("line {n}");
                        return Err(Error::with_detail(ErrorKind::OutOfRange, detail));
                    }
                };
                self.prg_edit = Some(PrgEdit { slot, line });
                Ok(vec![])
            }
            (Command::Prgedit, Want::Value | Want::Outs(1 | 2), []) => {
                let edit = self.editing()?;
                let line = Value::Int(reported(&Source::of(&self.slots[edit.slot]), edit.line));
                Ok(match want {
                    Want::Outs(2) => vec![Value::Int(edit.slot as i32), line],
                    _ => vec![line],
                })
            }
            (Command::Prgget, Want::Value, _) if args.len() <= 1 => {
                let seek = flag(args.first(), 1)? != 0;
                let edit = self.editing()?;
                let source = Source::of(&self.slots[edit.slot]);
                let text = source.lines.get(edit.line).cloned().unwrap_or_default();
                if seek && edit.line < source.lines.len() {
                    self.prg_edit = Some(PrgEdit {
                        line: edit.line + 1,
                        ..edit
                    });
                }
                Ok(vec![Value::Str(sb_core::value::new_str(text))])
            }
            // PRGSEEK stops at the last line, while PRGGET$ goes one past
            // it (checked on hardware).
            (Command::Prgseek, Want::Value | Want::Outs(0), _) if args.len() <= 1 => {
                let edit = self.editing()?;
                let len = Source::of(&self.slots[edit.slot]).lines.len();
                let to = match flag(args.first(), 1)? {
                    1 if edit.line + 1 < len => Some(edit.line + 1),
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
                let new = new_lines(text)?;
                let edit = self.editing()?;
                self.change_lines(edit, |lines, line| {
                    let n = new.len();
                    match lines.get(*line) {
                        Some(old) if *old != [LF] => {
                            lines.splice(*line..=*line, new);
                        }
                        _ => {
                            lines.splice(*line..*line, new);
                        }
                    }
                    *line += n;
                });
                Ok(vec![])
            }
            // The current line stays on the line it was when inserting
            // before it, and moves to the last line inserted after it
            // (checked on hardware).
            (Command::Prgins, Want::Outs(0), [text, rest @ ..]) if rest.len() <= 1 => {
                let new = new_lines(text)?;
                let after = match flag(rest.first(), 0)? {
                    0 => false,
                    1 => true,
                    n => {
                        return Err(Error::with_detail(ErrorKind::OutOfRange, format!("{n}")));
                    }
                };
                let edit = self.editing()?;
                self.change_lines(edit, |lines, line| {
                    let at = (*line + usize::from(after)).min(lines.len());
                    let n = new.len();
                    lines.splice(at..at, new);
                    *line = if after { at + n - 1 } else { *line + n };
                });
                Ok(vec![])
            }
            // A negative count deletes every line (checked on hardware).
            (Command::Prgdel, Want::Outs(0), _) if args.len() <= 1 => {
                let n = flag(args.first(), 1)?;
                let edit = self.editing()?;
                self.change_lines(edit, |lines, line| match usize::try_from(n) {
                    Ok(n) => {
                        let start = (*line).min(lines.len());
                        lines.drain(start..(start + n).min(lines.len()));
                    }
                    Err(_) => {
                        lines.clear();
                        *line = 0;
                    }
                });
                Ok(vec![])
            }
            (Command::Prgsize, Want::Value, _) if args.len() <= 2 => {
                let slot = self.current_slot(args.first())?;
                let source = Source::of(&self.slots[slot]);
                let chars = source.chars() as i32;
                let n = match flag(args.get(1), 0)? {
                    0 => source.count() as i32,
                    1 => chars,
                    2 => (SLOT_CHARS - chars).max(0),
                    n => {
                        let detail = format!("type {n}");
                        return Err(Error::with_detail(ErrorKind::OutOfRange, detail));
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

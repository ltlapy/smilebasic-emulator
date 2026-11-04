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
//!
//! The rest follows the model of these commands in SmileBASIC Debugger
//! (otya's SBDBG 1.2.8, which tests it against the real ones; TEST35
//! checked the cases below on hardware too), with the
//! place past the last line kept as the line count:
//! - PRGSET replaces the current line, and puts the lines after it. On the
//!   last line it adds an empty line first, and past the last line it
//!   replaces the last one and stays past the new last line.
//! - PRGSEEK 1 from the last line goes past it, and PRGDEL deletes back
//!   towards the start once it reaches the end.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::Value;

use crate::editor::LineShift;
use crate::runtime::{Command, Runtime, checking, no_form, opt_int, slot_arg};

/// The slot PRGEDIT chose and its current line, from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrgEdit {
    pub slot: usize,
    pub line: usize,
}

/// How a command changed a slot's lines, for what is kept by line, such
/// as breakpoints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SlotEdit {
    /// Lines moved, or went, as an edit in the editor moves them.
    Shift(LineShift),
    /// Another program took the slot's place: LOAD, NEW and the like.
    /// The programs' files, as `PROJECT/NAME`, when they have one.
    Replaced {
        from: Option<String>,
        to: Option<String>,
    },
    /// EXEC compiled the slot's program as it is now.
    Compiled,
}

/// `count` lines put in before line `at`, from 0.
fn inserted(at: usize, count: usize) -> LineShift {
    LineShift::Broken { at: (at, 0), count }
}

/// Line `at`, from 0, taken out.
fn deleted(at: usize) -> LineShift {
    LineShift::Joined {
        start: (at, 0),
        end: (at + 1, 0),
    }
}

/// What a slot holds, counting PRGSIZE's free characters: 1 MiB less one
/// for an empty slot (checked on hardware).
const SLOT_CHARS: i32 = 1 << 20;
const LF: u16 = 10;

/// Where a slot's lines start in its text, kept so that reading a long
/// program line by line with PRGGET$, or editing it a line at a time,
/// does not split all of it again for each line. A line is its text up
/// to and with the LF that ends it; a last line without one reads as if
/// it had it, and an empty slot reads as one empty line.
#[derive(Default)]
pub(crate) struct LinesCache {
    /// The slot, and the address and length of the text the lines were
    /// found in or put into.
    key: Option<(usize, usize, usize)>,
    /// The byte where each line starts; never empty once found.
    starts: Vec<usize>,
    /// The text's length in UTF-16 units, once PRGSIZE asked for it.
    units: Option<usize>,
}

impl LinesCache {
    /// Drops the lines kept. Whatever puts another text into a slot
    /// calls it; the address and length checked besides only catch a
    /// change that forgot to.
    pub(crate) fn forget(&mut self) {
        self.key = None;
    }
}

/// What tells whether a slot's text is still the one the cache holds,
/// as a guard behind [`LinesCache::forget`].
fn text_key(slot: usize, text: &str) -> (usize, usize, usize) {
    (slot, text.as_ptr() as usize, text.len())
}

/// The byte where each line of `text` starts.
fn line_starts(text: &str, starts: &mut Vec<usize>) {
    starts.clear();
    starts.push(0);
    let bytes = text.as_bytes();
    starts.extend(
        bytes
            .iter()
            .enumerate()
            // A line after the last LF only when there is text after it.
            .filter(|&(i, &b)| b == b'\n' && i + 1 < bytes.len())
            .map(|(i, _)| i + 1),
    );
}

/// A slot's lines, read through its text and where its lines start.
struct Source<'a> {
    text: &'a str,
    starts: &'a [usize],
}

impl Source<'_> {
    /// Lines as PRGSIZE counts them.
    fn count(&self) -> usize {
        if self.text.is_empty() {
            0
        } else {
            self.starts.len()
        }
    }

    /// Lines as the current line counts them: an empty slot has one.
    fn len(&self) -> usize {
        self.starts.len()
    }

    /// Line `line` from 0, with its LF.
    fn line(&self, line: usize) -> Option<Vec<u16>> {
        let start = *self.starts.get(line)?;
        let end = self.starts.get(line + 1).copied();
        let text = &self.text[start..end.unwrap_or(self.text.len())];
        let mut units: Vec<u16> = text.encode_utf16().collect();
        if units.last() != Some(&LF) {
            units.push(LF);
        }
        Some(units)
    }

    /// The line number PRGEDIT() reports for an index: never past the
    /// last.
    fn reported(&self, line: usize) -> i32 {
        (line + 1).min(self.count().max(1)) as i32
    }

    /// Characters as PRGSIZE counts them, given the text's UTF-16 length:
    /// with the LF a last line reads as having.
    fn chars(&self, units: usize) -> usize {
        units + usize::from(!self.text.ends_with('\n'))
    }
}

/// The lines a PRGSET or PRGINS string makes, as text: it ends with a
/// line break of its own.
struct NewLines {
    text: String,
    count: usize,
}

fn new_lines(v: &Value) -> Result<NewLines> {
    checking(v, || {
        let s = v.clone().into_str()?;
        let s = s.borrow();
        let mut text = String::from_utf16_lossy(&s);
        text.push('\n');
        let count = s.iter().filter(|&&c| c == LF).count() + 1;
        Ok(NewLines { text, count })
    })
}

/// The edited slot's lines while a command changes them: the text always
/// ends with LF then, so that each line has its own.
struct LinesEdit<'a> {
    text: &'a mut String,
    starts: &'a mut Vec<usize>,
    units: &'a mut Option<usize>,
}

impl LinesEdit<'_> {
    fn len(&self) -> usize {
        self.starts.len()
    }

    fn offset(&self, line: usize) -> usize {
        self.starts.get(line).copied().unwrap_or(self.text.len())
    }

    /// Lines `range` replaced by `new`.
    fn splice(&mut self, range: std::ops::Range<usize>, new: &NewLines) {
        let (from, to) = (self.offset(range.start), self.offset(range.end));
        if let Some(units) = self.units.as_mut() {
            *units -= self.text[from..to].encode_utf16().count();
            *units += new.text.encode_utf16().count();
        }
        self.text.replace_range(from..to, &new.text);
        let removed = to - from;
        let added = new.text.len();
        let tail = range.end..self.starts.len();
        // The lines after keep their places relative to each other.
        for s in &mut self.starts[tail] {
            *s = *s - removed + added;
        }
        let firsts = (new.count > 0).then_some(from).into_iter().chain(
            (new.text.match_indices('\n'))
                .map(|(i, _)| from + i + 1)
                .take(new.count.saturating_sub(1)),
        );
        self.starts.splice(range, firsts);
    }

    /// An empty line put after the last.
    fn push_empty(&mut self) {
        let len = self.len();
        let new = NewLines {
            text: "\n".into(),
            count: 1,
        };
        self.splice(len..len, &new);
    }

    fn remove(&mut self, range: std::ops::Range<usize>) {
        let none = NewLines {
            text: String::new(),
            count: 0,
        };
        self.splice(range, &none);
    }

    /// Only one empty line left.
    fn clear(&mut self) {
        self.text.clear();
        self.text.push('\n');
        self.starts.clear();
        self.starts.push(0);
        *self.units = Some(1);
    }
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

    /// Notes that a slot's program was replaced by file `to`'s, so that
    /// what is kept by its lines goes with the old one. Earlier changes
    /// to its lines no longer matter.
    pub(crate) fn replaced_lines(&mut self, slot: usize, to: Option<String>) {
        let earlier = self.slot_edits.iter().find_map(|(s, edit)| match edit {
            SlotEdit::Replaced { from, .. } if *s == slot => Some(from.clone()),
            _ => None,
        });
        let from = earlier.unwrap_or_else(|| self.slot_file(slot));
        self.prg_lines.forget();
        self.slot_edits.retain(|&(s, _)| s != slot);
        self.slot_edits
            .push((slot, SlotEdit::Replaced { from, to }));
    }

    /// The file a slot's program came from or went to, as `PROJECT/NAME`.
    pub(crate) fn slot_file(&self, slot: usize) -> Option<String> {
        let name = &self.slot_names[slot];
        (!name.is_empty()).then(|| format!("{}/{name}", self.storage.project()))
    }

    /// Changes the edited slot's lines. `f` tells how it moved them, in
    /// order. The slot does not count as unsaved for that.
    fn change_lines(
        &mut self,
        edit: PrgEdit,
        f: impl FnOnce(&mut LinesEdit, &mut usize, &mut Vec<LineShift>),
    ) {
        self.source(edit.slot);
        let mut text = std::mem::take(&mut self.slots[edit.slot]);
        let cache = &mut self.prg_lines;
        // An empty slot reads as one empty line, and a last line as if it
        // ended with LF; an edit gives them the LF.
        if !text.ends_with('\n') {
            text.push('\n');
            if let Some(units) = cache.units.as_mut() {
                *units += 1;
            }
        }
        let mut line = edit.line;
        let mut shifts = Vec::new();
        let mut lines = LinesEdit {
            text: &mut text,
            starts: &mut cache.starts,
            units: &mut cache.units,
        };
        f(&mut lines, &mut line, &mut shifts);
        (self.slot_edits).extend(shifts.into_iter().map(|s| (edit.slot, SlotEdit::Shift(s))));
        let len = cache.starts.len();
        // A slot left with one empty line is empty again.
        if text == "\n" {
            text.clear();
            cache.units = Some(0);
        }
        cache.key = Some(text_key(edit.slot, &text));
        self.slots[edit.slot] = text;
        self.slot_changes[edit.slot] += 1;
        self.prg_edit = Some(PrgEdit {
            slot: edit.slot,
            line: line.min(len),
        });
    }

    /// A slot's lines, found again only when its text changed since the
    /// last time.
    fn source(&mut self, slot: usize) -> Source<'_> {
        let cache = &mut self.prg_lines;
        let text = &self.slots[slot];
        let key = text_key(slot, text);
        if cache.key != Some(key) {
            line_starts(text, &mut cache.starts);
            cache.units = None;
            cache.key = Some(key);
        }
        Source {
            text,
            starts: &cache.starts,
        }
    }

    /// A slot's length in UTF-16 units.
    fn units(&mut self, slot: usize) -> usize {
        self.source(slot);
        let text = &self.slots[slot];
        *(self.prg_lines.units).get_or_insert_with(|| text.encode_utf16().count())
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
                let source = self.source(slot);
                let line = match flag(rest.first(), 1)? {
                    // Past the last line, and lines after the last go to
                    // it.
                    -1 => source.len(),
                    n @ 1.. => (n as usize).min(source.len()) - 1,
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
                let line = Value::Int(self.source(edit.slot).reported(edit.line));
                Ok(match want {
                    Want::Outs(2) => vec![Value::Int(edit.slot as i32), line],
                    _ => vec![line],
                })
            }
            (Command::Prgget, Want::Value, _) if args.len() <= 1 => {
                let seek = flag(args.first(), 1)? != 0;
                let edit = self.editing()?;
                let source = self.source(edit.slot);
                let text = source.line(edit.line).unwrap_or_default();
                if seek && edit.line < source.len() {
                    self.prg_edit = Some(PrgEdit {
                        line: edit.line + 1,
                        ..edit
                    });
                }
                Ok(vec![Value::Str(sb_core::value::new_str(text))])
            }
            (Command::Prgseek, Want::Value | Want::Outs(0), _) if args.len() <= 1 => {
                let edit = self.editing()?;
                let len = self.source(edit.slot).len();
                let cur = edit.line.min(len - 1);
                let (to, moved) = match flag(args.first(), 1)? {
                    1 if cur + 1 < len => (cur + 1, true),
                    // From the last line it goes past it, and says it
                    // did not move.
                    1 => (len, false),
                    -1 if cur > 0 => (cur - 1, true),
                    -1 => (cur, false),
                    n => {
                        let detail = format!("direction {n}");
                        return Err(Error::with_detail(ErrorKind::OutOfRange, detail));
                    }
                };
                self.prg_edit = Some(PrgEdit { line: to, ..edit });
                Ok(match want {
                    Want::Value => vec![Value::Int(i32::from(moved))],
                    _ => vec![],
                })
            }
            (Command::Prgset, Want::Outs(0), [text]) => {
                let new = new_lines(text)?;
                let edit = self.editing()?;
                self.change_lines(edit, |lines, line, shifts| {
                    let n = new.count;
                    let past = *line >= lines.len();
                    let at = (*line).min(lines.len() - 1);
                    // The replaced line keeps what it had, as one whose
                    // text is typed over in the editor does; the lines
                    // after it move down past the new ones.
                    shifts.push(inserted(at + 1, n - 1));
                    if past {
                        // Past the last line, the last is replaced and an
                        // empty line follows, still past the end.
                        lines.splice(at..at + 1, &new);
                        lines.push_empty();
                        *line = lines.len();
                    } else {
                        if at + 1 == lines.len() {
                            lines.push_empty();
                        }
                        lines.splice(at..at + 1, &new);
                        *line = at + n;
                    }
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
                self.change_lines(edit, |lines, line, shifts| {
                    // Past the last line, it is the last line that lines
                    // go before or after, and the place stays past the end.
                    let past = *line >= lines.len();
                    let cur = (*line).min(lines.len() - 1);
                    let at = cur + usize::from(after);
                    let n = new.count;
                    shifts.push(inserted(at, n));
                    lines.splice(at..at, &new);
                    *line = match (past, after) {
                        (true, _) => lines.len(),
                        (false, true) => at + n - 1,
                        (false, false) => cur + n,
                    };
                });
                Ok(vec![])
            }
            // A negative count deletes every line (checked on hardware).
            (Command::Prgdel, Want::Outs(0), _) if args.len() <= 1 => {
                let n = flag(args.first(), 1)?;
                let edit = self.editing()?;
                self.change_lines(edit, |lines, line, shifts| {
                    let past = *line >= lines.len();
                    match usize::try_from(n) {
                        // Lines after the current one go first; at the end
                        // it deletes back towards the start.
                        Ok(n) => {
                            let start = (*line).min(lines.len() - 1);
                            let (mut cur, mut len) = (start, lines.len());
                            // The lines deleted always make one run: from
                            // the current line on, then back before it.
                            let mut first = start;
                            for _ in 0..n {
                                if len == 0 {
                                    break;
                                }
                                first = first.min(cur);
                                len -= 1;
                                shifts.push(deleted(cur));
                                if cur == len {
                                    cur = cur.saturating_sub(1);
                                }
                            }
                            let gone = lines.len() - len;
                            lines.remove(first..first + gone);
                            if len == 0 {
                                lines.clear();
                                cur = 0;
                            }
                            *line = if past { lines.len() } else { cur };
                        }
                        Err(_) => {
                            shifts.push(LineShift::Joined {
                                start: (0, 0),
                                end: (lines.len(), 0),
                            });
                            lines.clear();
                            *line = if past { 1 } else { 0 };
                        }
                    }
                });
                Ok(vec![])
            }
            (Command::Prgsize, Want::Value, _) if args.len() <= 2 => {
                let slot = self.current_slot(args.first())?;
                let kind = flag(args.get(1), 0)?;
                let units = if kind == 0 { 0 } else { self.units(slot) };
                let source = self.source(slot);
                let chars = source.chars(units) as i32;
                let n = match kind {
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
            _ => Err(no_form()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::Font;

    /// The commands as they worked on a slot split into lines anew each
    /// time, before the lines were kept.
    struct Model {
        text: String,
        edit: Option<PrgEdit>,
    }

    fn split_units(text: &[u16]) -> Vec<Vec<u16>> {
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

    fn model_lines(text: &str) -> (Vec<Vec<u16>>, bool) {
        let text: Vec<u16> = text.encode_utf16().collect();
        if text.is_empty() {
            (vec![vec![LF]], true)
        } else {
            (split_units(&text), false)
        }
    }

    fn model_text(lines: &[Vec<u16>]) -> String {
        let text: Vec<u16> = lines.concat();
        if text == [LF] {
            return String::new();
        }
        String::from_utf16_lossy(&text)
    }

    #[derive(Clone, Debug)]
    enum Op {
        Edit(i32),
        Get(bool),
        Where,
        Seek(i32),
        Set(Vec<u16>),
        Ins(Vec<u16>, bool),
        Del(i32),
        Size(i32),
    }

    impl Model {
        /// What the op returns, or None for an error.
        fn apply(&mut self, op: &Op) -> Option<Vec<i64>> {
            let (mut lines, empty) = model_lines(&self.text);
            if let Op::Edit(n) = *op {
                let line = match n {
                    -1 => lines.len(),
                    n @ 1.. => (n as usize).min(lines.len()) - 1,
                    _ => return None,
                };
                self.edit = Some(PrgEdit { slot: 3, line });
                return Some(vec![]);
            }
            if let Op::Size(kind) = *op {
                let chars: usize = lines.iter().map(Vec::len).sum();
                let count = if empty { 0 } else { lines.len() };
                return Some(vec![match kind {
                    0 => count as i64,
                    1 => chars as i64,
                    _ => (i64::from(SLOT_CHARS) - chars as i64).max(0),
                }]);
            }
            let edit = self.edit?;
            let mut line = edit.line;
            let out = match op {
                Op::Get(seek) => {
                    let text = lines.get(line).cloned().unwrap_or_default();
                    if *seek && line < lines.len() {
                        line += 1;
                    }
                    self.edit = Some(PrgEdit { line, ..edit });
                    return Some(text.into_iter().map(i64::from).collect());
                }
                Op::Where => {
                    let count = if empty { 0 } else { lines.len() };
                    return Some(vec![(line + 1).min(count.max(1)) as i64]);
                }
                Op::Seek(dir) => {
                    let len = lines.len();
                    let cur = line.min(len - 1);
                    let (to, moved) = match dir {
                        1 if cur + 1 < len => (cur + 1, true),
                        1 => (len, false),
                        -1 if cur > 0 => (cur - 1, true),
                        _ => (cur, false),
                    };
                    self.edit = Some(PrgEdit { line: to, ..edit });
                    return Some(vec![i64::from(moved)]);
                }
                Op::Set(s) => {
                    let mut s = s.clone();
                    s.push(LF);
                    let new = split_units(&s);
                    let n = new.len();
                    let past = line >= lines.len();
                    let at = line.min(lines.len() - 1);
                    if past {
                        lines.splice(at..=at, new);
                        lines.push(vec![LF]);
                        line = lines.len();
                    } else {
                        if at + 1 == lines.len() {
                            lines.push(vec![LF]);
                        }
                        lines.splice(at..=at, new);
                        line = at + n;
                    }
                    vec![]
                }
                Op::Ins(s, after) => {
                    let mut s = s.clone();
                    s.push(LF);
                    let new = split_units(&s);
                    let past = line >= lines.len();
                    let cur = line.min(lines.len() - 1);
                    let at = cur + usize::from(*after);
                    let n = new.len();
                    lines.splice(at..at, new);
                    line = match (past, after) {
                        (true, _) => lines.len(),
                        (false, true) => at + n - 1,
                        (false, false) => cur + n,
                    };
                    vec![]
                }
                Op::Del(n) => {
                    let past = line >= lines.len();
                    match usize::try_from(*n) {
                        Ok(n) => {
                            let mut cur = line.min(lines.len() - 1);
                            for _ in 0..n {
                                if lines.is_empty() {
                                    break;
                                }
                                lines.remove(cur);
                                if cur == lines.len() {
                                    cur = cur.saturating_sub(1);
                                }
                            }
                            if lines.is_empty() {
                                lines.push(vec![LF]);
                                cur = 0;
                            }
                            line = if past { lines.len() } else { cur };
                        }
                        Err(_) => {
                            lines = vec![vec![LF]];
                            line = usize::from(past);
                        }
                    }
                    vec![]
                }
                Op::Edit(_) | Op::Size(_) => unreachable!(),
            };
            self.text = model_text(&lines);
            self.edit = Some(PrgEdit {
                line: line.min(lines.len()),
                ..edit
            });
            Some(out)
        }
    }

    fn run(rt: &mut Runtime, op: &Op) -> Option<Vec<i64>> {
        let s = |u: &Vec<u16>| Value::Str(sb_core::value::new_str(u.clone()));
        let (command, want, args) = match op {
            Op::Edit(n) => (
                Command::Prgedit,
                Want::Outs(0),
                vec![Value::Int(3), Value::Int(*n)],
            ),
            Op::Get(seek) => (
                Command::Prgget,
                Want::Value,
                vec![Value::Int((*seek).into())],
            ),
            Op::Where => (Command::Prgedit, Want::Value, vec![]),
            Op::Seek(d) => (Command::Prgseek, Want::Value, vec![Value::Int(*d)]),
            Op::Set(u) => (Command::Prgset, Want::Outs(0), vec![s(u)]),
            Op::Ins(u, after) => (
                Command::Prgins,
                Want::Outs(0),
                vec![s(u), Value::Int((*after).into())],
            ),
            Op::Del(n) => (Command::Prgdel, Want::Outs(0), vec![Value::Int(*n)]),
            Op::Size(k) => (
                Command::Prgsize,
                Want::Value,
                vec![Value::Int(3), Value::Int(*k)],
            ),
        };
        let out = rt.prg_command(command, &args, want).ok()?;
        let units = |v: Value| match v {
            Value::Int(n) => vec![i64::from(n)],
            Value::Str(s) => s.borrow().iter().map(|&c| i64::from(c)).collect(),
            _ => panic!("unexpected value"),
        };
        Some(out.into_iter().flat_map(units).collect())
    }

    /// The kept lines give what splitting the slot anew each time gave,
    /// over many random edits.
    #[test]
    fn kept_lines_match_splitting_anew() {
        let pieces: [&[u16]; 7] = [
            &[0x41],
            &[LF],
            &[0x3042],
            &[],
            &[0xD800],
            &[0x42, 0x43, LF, LF],
            &[0xD83D, 0xDE00],
        ];
        let starts = ["", "A", "A\n", "A\nB", "\n\n", "あ\nい\n\nう"];
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        for round in 0..300 {
            let mut rt = Runtime::new(400, 240, Font::blank());
            let start = starts[round % starts.len()];
            rt.slots[3] = start.to_string();
            let mut model = Model {
                text: start.to_string(),
                edit: None,
            };
            for step in 0..60 {
                let mut text = Vec::new();
                for _ in 0..next(4) {
                    text.extend_from_slice(pieces[next(pieces.len() as u64) as usize]);
                }
                let op = match next(9) {
                    0 => Op::Edit(next(8) as i32 - 1),
                    1 => Op::Get(next(4) != 0),
                    2 => Op::Where,
                    3 => Op::Seek(if next(2) == 0 { 1 } else { -1 }),
                    4 => Op::Set(text),
                    5 | 6 => Op::Ins(text, next(2) == 0),
                    7 => Op::Del(next(6) as i32 - 1),
                    _ => Op::Size(next(3) as i32),
                };
                let want = model.apply(&op);
                let got = run(&mut rt, &op);
                let at = format!("round {round} step {step} {op:?}");
                assert_eq!(got, want, "{at}");
                assert_eq!(rt.slots[3], model.text, "{at}");
                assert_eq!(rt.prg_edit, model.edit, "{at}");
            }
        }
    }
}

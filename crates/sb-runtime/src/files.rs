//! File commands: LOAD, SAVE, FILES, PROJECT, CHKFILE, DELETE, RENAME
//! and RESULT.
//!
//! Failures do not stop the program; they set RESULT() to 0. In direct mode
//! a message also explains what went wrong.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{Array, ArrayData, ElemType, Value, new_str};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::confirm::{Ask, Confirm, FileAction, FileConfirm, project_label, switch_lines};
use crate::runtime::{Command, Runtime, SLOTS, no_form, slot_arg};
use crate::sbfile;
use crate::storage::{
    self, FileKind, FileRef, StorageError, parse_file_name, parse_listing, parse_read_name,
};
use crate::text::CONSOLE;

pub(crate) fn str_arg(v: &Value) -> Result<String> {
    Ok(String::from_utf16_lossy(&v.clone().into_str()?.borrow()))
}

/// Full-width letters and digits to ASCII, for SAVE.
pub fn half_width(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '０'..='９' | 'Ａ'..='Ｚ' | 'ａ'..='ｚ' => {
                char::from_u32(u32::from(c) - 0xFEE0).expect("ASCII")
            }
            _ => c,
        })
        .collect()
}

/// A program file's text: SmileBASIC 4's container or plain UTF-8.
pub(crate) fn program_text(bytes: &[u8]) -> storage::Result<String> {
    let bad = |e: String| StorageError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e));
    if bytes.starts_with(&[4, 0]) {
        let file = sbfile::parse(bytes).map_err(|e| bad(e.to_string()))?;
        Ok(file.text().map_err(|e| bad(e.to_string()))?.to_owned())
    } else {
        String::from_utf8(bytes.to_vec()).map_err(|_| bad("not UTF-8 text".to_string()))
    }
}

fn str_array(items: Vec<String>) -> Array {
    let data = items
        .iter()
        .map(|s| new_str(s.encode_utf16().collect()))
        .collect();
    Array {
        dims: vec![items.len()],
        data: ArrayData::Str(data),
    }
}

/// Replaces the contents of a one-dimensional string array.
fn fill_array(target: &Value, items: Vec<String>) -> Result<()> {
    let Value::Array(array) = target else {
        return Err(ErrorKind::TypeMismatch.into());
    };
    let mut array = array.borrow_mut();
    if array.elem_type() != ElemType::Str || array.dims.len() != 1 {
        return Err(ErrorKind::TypeMismatch.into());
    }
    *array = str_array(items);
    Ok(())
}

/// What FILES shows while it waits between screens.
const LISTING_PROMPT: &str = "=== Press ENTER ===";

impl Runtime {
    pub(crate) fn file_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        match (command, want, args.len()) {
            (Command::Result, Want::Value, 0) => Ok(vec![Value::Int(self.result)]),
            (Command::Project, Want::Value, 0) => Ok(vec![Value::str(self.storage.project())]),
            (Command::Project, Want::Outs(0), 1) => {
                self.direct_only()?;
                let name = match str_arg(&args[0])?.as_str() {
                    "" => String::new(),
                    name => storage::check_project_name(name).map_err(|_| {
                        Error::with_detail(ErrorKind::IllegalProjectName, format!("{name:?}"))
                    })?,
                };
                if !self.storage.project_exists(&name) {
                    let detail = format!("no project {name}");
                    return Err(Error::with_detail(ErrorKind::UndefinedProjectName, detail));
                }
                let unsaved = self.unsaved_among(0..SLOTS);
                if !unsaved.is_empty() && name != self.storage.project() {
                    let old = self.project_label();
                    let lines = switch_lines(project_label(&name), &old, &unsaved);
                    let ask = self.ask(|_| Confirm {
                        title: "Switch project".to_string(),
                        lines,
                        accept: "Switch",
                    });
                    if self.held(ask) {
                        return Ok(vec![]);
                    }
                }
                let outcome = self.storage.set_project(&name);
                self.finish("PROJECT", outcome);
                Ok(vec![])
            }
            (Command::Chkfile, Want::Value, 1) => {
                let found = parse_read_name(&str_arg(&args[0])?, FileKind::Txt)
                    .is_ok_and(|file| self.storage.exists_ref(&file));
                Ok(vec![Value::Int(i32::from(found))])
            }
            (Command::Load, Want::Outs(0), 1 | 2) => {
                let spec = str_arg(&args[0])?;
                let slot = slot_arg(args.get(1))?;
                self.load_asking("LOAD", &spec, slot);
                Ok(vec![])
            }
            (Command::Save, Want::Outs(0), 1 | 2) => {
                let spec = str_arg(&args[0])?;
                let slot = slot_arg(args.get(1))?;
                if let Ok((kind, name)) = parse_file_name(&spec, FileKind::Txt) {
                    let file = format!("{}:{name}", kind.name());
                    let mut details = self.details(&file, None);
                    if self.storage.exists(kind, &name) {
                        details.push(String::new());
                        details.push("The file there now will be replaced.".to_string());
                    }
                    let action = FileAction {
                        title: format!("Slot {slot}: Save file"),
                        summary: format!("Save slot {slot} as this file."),
                        details,
                        accept: "Save",
                        notice: format!("Saving slot {slot} as {file}"),
                        in_programs: true,
                    };
                    let ask = self.confirm_file(action, &[]);
                    if self.held(ask) {
                        return Ok(vec![]);
                    }
                }
                let outcome = self.save(&spec, slot);
                self.finish("SAVE", outcome);
                Ok(vec![])
            }
            (Command::Delete, Want::Outs(0), 1) => {
                let spec = str_arg(&args[0])?;
                if let Some(file) = self.existing_read(&spec) {
                    let action = FileAction {
                        title: "Delete file".to_string(),
                        summary: "Delete this file.".to_string(),
                        details: self.details(&file, None),
                        accept: "Delete",
                        notice: format!("Deleting {file}"),
                        in_programs: true,
                    };
                    let ask = self.confirm_file(action, &[]);
                    if self.held(ask) {
                        return Ok(vec![]);
                    }
                }
                // Backups (`@BACKUP.PRG`) can be deleted, but only in the
                // current project.
                let outcome =
                    self.read_name(&spec, FileKind::Txt)
                        .and_then(|file| match file.project {
                            None => self.storage.delete(file.kind, &file.name),
                            Some(_) => Err(StorageError::BadName(spec.clone())),
                        });
                self.finish("DELETE", outcome);
                Ok(vec![])
            }
            (Command::Rename, Want::Outs(0), 2) => {
                let (from, to) = (str_arg(&args[0])?, str_arg(&args[1])?);
                if let Some(file) = self.existing(&from) {
                    let action = FileAction {
                        title: "Rename file".to_string(),
                        summary: "Rename this file.".to_string(),
                        details: self.details(&file, Some(&to)),
                        accept: "Rename",
                        notice: format!("Renaming {file} to {to}"),
                        in_programs: true,
                    };
                    let ask = self.confirm_file(action, &[]);
                    if self.held(ask) {
                        return Ok(vec![]);
                    }
                }
                let outcome = self.rename(&from, &to);
                self.finish("RENAME", outcome);
                Ok(vec![])
            }
            (Command::Files, Want::Value, 0 | 1) => {
                let spec = args.first().map(str_arg).transpose()?.unwrap_or_default();
                let items = self.listing(&spec);
                let items = self.finish("FILES", items).unwrap_or_default();
                Ok(vec![Value::Array(Rc::new(RefCell::new(str_array(items))))])
            }
            (Command::Files, Want::Outs(0), 0..=2) => {
                let (spec, array) = match args {
                    [] => (String::new(), None),
                    [Value::Array(_)] => (String::new(), Some(&args[0])),
                    [spec] => (str_arg(spec)?, None),
                    [spec, array] => (str_arg(spec)?, Some(array)),
                    _ => unreachable!(),
                };
                if let Some(left) = self.listing_left.take() {
                    return Ok(self.show_listing(left));
                }
                let items = self.listing(&spec);
                let items = self.finish("FILES", items).unwrap_or_default();
                match array {
                    Some(array) => fill_array(array, items)?,
                    None => {
                        let console = &mut self.text[CONSOLE];
                        if console.cursor().0 != 0 {
                            console.newline();
                        }
                        // UNVERIFIED: the header for project lists and for one file.
                        let mut lines = VecDeque::from([format!("-- {} files --", items.len())]);
                        lines.extend(items);
                        return Ok(self.show_listing_page(lines));
                    }
                }
                Ok(vec![])
            }
            _ => Err(no_form()),
        }
    }

    /// Goes on with a FILES listing once a key or button is pressed. A
    /// key held down goes on as it repeats (checked on hardware).
    fn show_listing(&mut self, left: VecDeque<String>) -> Vec<Value> {
        if self.keys.pop_front().is_some() || self.input.pressed() != 0 {
            // UNVERIFIED: that the prompt is taken away.
            let console = &mut self.text[CONSOLE];
            LISTING_PROMPT.chars().for_each(|_| console.backspace());
            return self.show_listing_page(left);
        }
        self.listing_left = Some(left);
        self.retry = true;
        vec![]
    }

    /// Prints a screen of a FILES listing, and waits to print the rest,
    /// as SmileBASIC 4 does in direct mode and in programs: all the
    /// console's rows but the last, which says to press Enter, however
    /// many rows XSCREEN gives it (checked on hardware).
    fn show_listing_page(&mut self, mut lines: VecDeque<String>) -> Vec<Value> {
        let console = &mut self.text[CONSOLE];
        let page = console.size().1.saturating_sub(1).max(1);
        for _ in 0..page {
            let Some(line) = lines.pop_front() else {
                return vec![];
            };
            console.print(&line.encode_utf16().collect::<Vec<_>>());
            console.newline();
        }
        if !lines.is_empty() {
            console.print(&LISTING_PROMPT.encode_utf16().collect::<Vec<_>>());
            self.listing_left = Some(lines);
            self.retry = true;
        }
        vec![]
    }

    /// LOAD, and EXEC with a file: asks first as set (or, in a program,
    /// only over unsaved changes). `None` while the question waits;
    /// otherwise whether it loaded, which RESULT() tells too.
    pub(crate) fn load_asking(&mut self, what: &str, spec: &str, slot: usize) -> Option<bool> {
        // A file that is not there fails without asking first.
        if let Some(file) = self.existing_read(spec) {
            let action = FileAction {
                title: format!("Slot {slot}: Load file"),
                summary: format!("Load this file into slot {slot}."),
                details: self.details(&file, None),
                accept: "Load",
                notice: format!("Loading {file} into slot {slot}"),
                in_programs: false,
            };
            match self.confirm_file(action, &[slot]) {
                Ask::Go => {}
                Ask::Wait => return None,
                Ask::Refused => {
                    self.result = -1;
                    return Some(false);
                }
            }
        }
        let outcome = self.load(spec, slot);
        Some(self.finish(what, outcome).is_some())
    }

    /// A file named for reading, in this project or another: programs
    /// LOAD and LOADV other projects' files too (checked on hardware,
    /// against the manual's word that only EXEC does).
    pub(crate) fn read_name(&self, spec: &str, default: FileKind) -> storage::Result<FileRef> {
        let mut file = parse_read_name(spec, default)?;
        if file.project.as_deref() == Some(self.storage.project()) {
            file.project = None;
        }
        Ok(file)
    }

    /// A file named for reading, as [`FileRef::label`], if it exists.
    pub(crate) fn existing_read(&self, spec: &str) -> Option<String> {
        let file = self.read_name(spec, FileKind::Txt).ok()?;
        self.storage.exists_ref(&file).then(|| file.label())
    }

    /// The file a name refers to, as `TYPE:NAME`, if it exists.
    pub(crate) fn existing(&self, spec: &str) -> Option<String> {
        let (kind, name) = parse_file_name(spec, FileKind::Txt).ok()?;
        self.storage
            .exists(kind, &name)
            .then(|| format!("{}:{name}", kind.name()))
    }

    /// The project and file lines of a file question.
    pub(crate) fn details(&self, file: &str, new_name: Option<&str>) -> Vec<String> {
        let mut lines = vec![
            format!("  Project:  {}", self.project_label()),
            format!("  File:     {file}"),
        ];
        if let Some(new_name) = new_name {
            lines.push(format!("  New name: {new_name}"));
        }
        lines
    }

    /// Whether a command must stop here for its question: it waits for
    /// the answer, or was cancelled, which RESULT() reports as -1.
    pub(crate) fn held(&mut self, ask: Ask) -> bool {
        match ask {
            Ask::Go => false,
            Ask::Wait => true,
            Ask::Refused => {
                self.result = -1;
                true
            }
        }
    }

    /// Sets RESULT() from an outcome; in direct mode, also explains a
    /// failure on the console, and in a program shows it as a notice
    /// unless file access is silent (SmileBASIC 4 shows a dialog there).
    pub(crate) fn finish<T>(&mut self, what: &str, outcome: storage::Result<T>) -> Option<T> {
        match outcome {
            Ok(value) => {
                self.result = 1;
                Some(value)
            }
            Err(e) => {
                self.result = 0;
                if self.direct {
                    // UNVERIFIED: SmileBASIC 4 shows its own dialogs instead.
                    let console = &mut self.text[CONSOLE];
                    if console.cursor().0 != 0 {
                        console.newline();
                    }
                    let message = format!("{what} failed: {e}");
                    console.print(&message.encode_utf16().collect::<Vec<_>>());
                    console.newline();
                } else if self.settings.file_confirm != FileConfirm::Silent {
                    self.notices.push(format!("{what} failed: {e}"));
                }
                None
            }
        }
    }

    fn load(&mut self, spec: &str, slot: usize) -> storage::Result<()> {
        let file = self.read_name(spec, FileKind::Txt)?;
        if file.kind != FileKind::Txt {
            return Err(StorageError::BadKind(file.kind.name().to_string()));
        }
        self.load_ref(&file, slot)
    }

    /// Loads the current project's TXT file `name`, as listed, into a slot.
    pub(crate) fn load_file(&mut self, name: &str, slot: usize) -> storage::Result<()> {
        let file = FileRef {
            kind: FileKind::Txt,
            project: None,
            name: name.to_string(),
        };
        self.load_ref(&file, slot)
    }

    /// Loading over a slot that EXEC ran keeps what its program defined
    /// until the next EXEC, and a program can LOAD over itself and goes
    /// on, although the manual says otherwise (checked on hardware).
    fn load_ref(&mut self, file: &FileRef, slot: usize) -> storage::Result<()> {
        self.slots[slot] = program_text(&self.storage.read_ref(file)?)?;
        self.drop_prg_edit(slot);
        self.slot_names[slot] = file.name.clone();
        self.slot_modified[slot] = false;
        Ok(())
    }

    fn save(&mut self, spec: &str, slot: usize) -> storage::Result<()> {
        let (kind, name) = parse_file_name(spec, FileKind::Txt)?;
        if kind != FileKind::Txt {
            return Err(StorageError::BadKind(kind.name().to_string()));
        }
        let text = match self.settings.half_width_on_save {
            true => half_width(&self.slots[slot]),
            false => self.slots[slot].clone(),
        };
        self.storage
            .write(kind, &name, text.as_bytes(), Some(storage::BACKUP_NAME))?;
        self.slot_names[slot] = name;
        self.slot_modified[slot] = false;
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> storage::Result<()> {
        let (kind, from) = parse_file_name(from, FileKind::Txt)?;
        let (to_kind, to) = parse_file_name(to, kind)?;
        if to_kind != kind {
            return Err(StorageError::BadKind(to_kind.name().to_string()));
        }
        self.storage.rename(kind, &from, &to)
    }

    /// FILES' entries: projects for `"//"`, else files of a project, each
    /// with its `TYPE:` prefix, also when one type was asked for (checked
    /// on hardware: `FILES "DAT:"` lists `DAT:T5A`).
    fn listing(&self, spec: &str) -> storage::Result<Vec<String>> {
        if spec == "//" {
            return self.storage.projects();
        }
        let (kind, project) = parse_listing(spec)?;
        let files = self.storage.list(kind, project.as_deref())?;
        Ok(files
            .into_iter()
            .map(|(k, name)| format!("{}:{name}", k.name()))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_width_converts_letters_and_digits_only() {
        assert_eq!(half_width("ＰＲＩＮＴ　\"ａ１！\""), "PRINT　\"a1！\"");
    }
}

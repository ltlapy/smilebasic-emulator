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
use crate::runtime::{Command, Runtime, SLOTS, checking, no_form, slot_arg};
use crate::sbfile;
use crate::storage::{
    self, FileKind, FileRef, NameError, NamedKind, StorageError, parse_file_name, parse_listing,
    parse_read_name,
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
pub fn program_text(bytes: &[u8]) -> storage::Result<String> {
    let bad = |e: String| StorageError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e));
    let file;
    let body = if bytes.starts_with(&[4, 0]) {
        file = sbfile::parse(bytes).map_err(|e| bad(e.to_string()))?;
        &file.body
    } else {
        bytes
    };
    sb_core::wtf8::decode(body).ok_or_else(|| bad("not UTF-8 text".to_string()))
}

fn str_array(items: Vec<String>) -> Array {
    let data = items
        .iter()
        .map(|s| new_str(s.encode_utf16().collect()))
        .collect();
    Array {
        dims: vec![items.len()],
        data: ArrayData::Str(data),
        system: false,
    }
}

/// Replaces the contents of a one-dimensional string array.
fn fill_array(target: &Value, items: Vec<String>) -> Result<()> {
    checking(target, || {
        let Value::Array(array) = target else {
            return Err(ErrorKind::TypeMismatch.into());
        };
        let mut array = array.borrow_mut();
        if array.elem_type() != ElemType::Str || array.dims.len() != 1 {
            return Err(ErrorKind::TypeMismatch.into());
        }
        *array = str_array(items);
        Ok(())
    })
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
        self.after_failure(|rt| rt.file_command_now(command, args, want))
    }

    fn file_command_now(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        match (command, want, args.len()) {
            (Command::Result, Want::Value, 0) => Ok(vec![Value::Int(self.result)]),
            (Command::Project, Want::Value, 0) => Ok(vec![Value::str(self.storage.project())]),
            (Command::Project, Want::Outs(0), 1) => {
                self.direct_only(ErrorKind::CantUseInProgram)?;
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
                        accept_first: false,
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
                let spec = str_arg(&args[0])?;
                check_file_name(&spec, 0)?;
                let found = parse_read_name(&spec, FileKind::Txt)
                    .is_ok_and(|file| self.storage.exists_ref(&file));
                Ok(vec![Value::Int(i32::from(found))])
            }
            (Command::Load, Want::Outs(0), 1 | 2) => {
                let spec = str_arg(&args[0])?;
                let slot = slot_arg(args.get(1))?;
                check_file_name(&spec, 0)?;
                program_type(&spec)?;
                self.load_asking("LOAD", &spec, slot);
                Ok(vec![])
            }
            (Command::Save, Want::Outs(0), 1 | 2) => {
                let spec = str_arg(&args[0])?;
                let slot = slot_arg(args.get(1))?;
                check_file_name(&spec, 0)?;
                program_type(&spec)?;
                self.refuse_save_project(&spec)?;
                if let Ok(file) = self.write_name(&spec, FileKind::Txt) {
                    let exists = self.storage.exists_ref(&file);
                    let originals = self.originals_of(&file, Some(storage::BACKUP_NAME));
                    let file = file.label();
                    let mut details = self.details(&file, None);
                    if exists {
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
                        originals,
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
                check_file_name(&spec, 0)?;
                self.refuse_system_project(&spec).map_err(|e| e.of_arg(0))?;
                refuse_project(&spec).map_err(|e| e.of_arg(0))?;
                if let Some(file) = self.existing(&spec) {
                    let action = FileAction {
                        title: "Delete file".to_string(),
                        summary: "Delete this file.".to_string(),
                        details: self.details(&file, None),
                        accept: "Delete",
                        notice: format!("Deleting {file}"),
                        in_programs: true,
                        originals: self.originals_named(&[&spec]),
                    };
                    let ask = self.confirm_file(action, &[]);
                    if self.held(ask) {
                        return Ok(vec![]);
                    }
                }
                // Backups (`@BACKUP.PRG`) can be deleted too.
                let outcome = parse_file_name(&spec, FileKind::Txt)
                    .and_then(|(kind, name)| self.storage.delete(kind, &name));
                self.finish("DELETE", outcome);
                Ok(vec![])
            }
            (Command::Rename, Want::Outs(0), 2) => {
                // The first name is checked before the second is read, and
                // the types before the second's project. Each name's `#`
                // project comes before its other project.
                let from = str_arg(&args[0])?;
                check_file_name(&from, 0)?;
                self.refuse_system_project(&from).map_err(|e| e.of_arg(0))?;
                refuse_project(&from).map_err(|e| e.of_arg(0))?;
                let to = str_arg(&args[1])?;
                check_file_name(&to, 1)?;
                same_type(&from, &to)?;
                self.refuse_system_project(&to).map_err(|e| e.of_arg(1))?;
                refuse_project(&to).map_err(|e| e.of_arg(1))?;
                if let Some(file) = self.existing(&from) {
                    let action = FileAction {
                        title: "Rename file".to_string(),
                        summary: "Rename this file.".to_string(),
                        details: self.details(&file, Some(&to)),
                        accept: "Rename",
                        notice: format!("Renaming {file} to {to}"),
                        in_programs: true,
                        originals: self.originals_named(&[&from, &to]),
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
            // SmileBASIC 4 takes a lone argument that is neither
            // a string nor an array for Type mismatch naming it, whatever
            // the OUT count.
            (Command::Files, _, 1) if !matches!(args[0], Value::Str(_) | Value::Array(_)) => {
                Err(Error::new(ErrorKind::TypeMismatch).of_arg(0))
            }
            (Command::Files, Want::Value, 0 | 1) => {
                let spec = args.first().map(str_arg).transpose()?;
                if let Some(spec) = &spec {
                    check_listing_name(spec)?;
                }
                let spec = spec.unwrap_or_default();
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
                if let [Value::Str(_), ..] = args {
                    check_listing_name(&spec)?;
                }
                let items = self.listing(&spec);
                let items = self.finish("FILES", items).unwrap_or_default();
                match array {
                    Some(array) => fill_array(array, items)?,
                    None => {
                        // A file's name (`FILES "TXT:NAME"`) is taken for a
                        // project's, and a project that is not there prints
                        // nothing, not even the header.
                        // UNVERIFIED: the same for a name that is no file's.
                        if items.is_empty() && self.no_such_project(&spec) {
                            return Ok(vec![]);
                        }
                        let console = &mut self.text[CONSOLE];
                        if console.cursor().0 != 0 {
                            console.newline();
                        }
                        // A list starts with its count.
                        let header = match spec.as_str() {
                            "//" => format!("-- {} projects --", items.len()),
                            _ => format!("-- {} files --", items.len()),
                        };
                        let lines = std::iter::once(header).chain(items);
                        let lines = lines.map(|l| l.encode_utf16().collect()).collect();
                        let page = self.text[CONSOLE].size().1.saturating_sub(1);
                        return Ok(self.show_listing_page(lines, page));
                    }
                }
                Ok(vec![])
            }
            _ => Err(no_form()),
        }
    }

    /// INSPECT (`??`): prints a value's details, a screen at a time.
    pub(crate) fn inspect(&mut self, value: &Value) -> Vec<Value> {
        if let Some(left) = self.inspect_left.take() {
            if self.keys.pop_front().is_none() && self.input.pressed() == 0 {
                self.inspect_left = Some(left);
                self.retry = true;
                return vec![];
            }
            let console = &mut self.text[CONSOLE];
            let rows = console.size().1 as i32;
            let _ = console.locate(Some(0), Some(rows - 1));
            let blank = vec![u16::from(b' '); LISTING_PROMPT.len()];
            console.print(&blank);
            let _ = console.locate(Some(0), Some(rows - 2));
            return self.inspect_page(left);
        }
        let console = &mut self.text[CONSOLE];
        if console.cursor().0 != 0 {
            console.newline();
        }
        self.inspect_page(sb_core::inspect::lines(value).into())
    }

    /// Prints a page of INSPECT's lines: all the console's rows but two.
    /// Meanwhile the console scrolls above its last row, which is kept
    /// for the prompt to press Enter.
    fn inspect_page(&mut self, mut lines: VecDeque<Vec<u16>>) -> Vec<Value> {
        let console = &mut self.text[CONSOLE];
        let rows = console.size().1;
        for _ in 0..rows.saturating_sub(2).max(1) {
            let Some(line) = lines.pop_front() else {
                return vec![];
            };
            console.print(&line);
            console.newline();
            if rows >= 2 && console.cursor().1 == rows - 1 {
                console.newline();
                let _ = console.locate(Some(0), Some(rows as i32 - 2));
            }
        }
        if !lines.is_empty() {
            let _ = console.locate(Some(0), Some(rows as i32 - 1));
            console.print(&LISTING_PROMPT.encode_utf16().collect::<Vec<_>>());
            self.inspect_left = Some(lines);
            self.retry = true;
        }
        vec![]
    }

    /// Goes on with a FILES listing once a key or button is pressed. A
    /// key held down goes on as it repeats (checked on hardware).
    fn show_listing(&mut self, left: VecDeque<Vec<u16>>) -> Vec<Value> {
        if self.keys.pop_front().is_some() || self.input.pressed() != 0 {
            // UNVERIFIED: that the prompt is taken away.
            let console = &mut self.text[CONSOLE];
            LISTING_PROMPT.chars().for_each(|_| console.backspace());
            let page = console.size().1.saturating_sub(1);
            return self.show_listing_page(left, page);
        }
        self.listing_left = Some(left);
        self.retry = true;
        vec![]
    }

    /// Prints `page` lines of a FILES listing, and waits to print the
    /// rest, as SmileBASIC 4 does in direct mode and in programs. A page
    /// is all the console's rows but the last, which says to press Enter,
    /// however many rows XSCREEN gives it (checked on hardware).
    fn show_listing_page(&mut self, mut lines: VecDeque<Vec<u16>>, page: u32) -> Vec<Value> {
        let console = &mut self.text[CONSOLE];
        for _ in 0..page.max(1) {
            let Some(line) = lines.pop_front() else {
                return vec![];
            };
            console.print(&line);
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
        // EXEC waits on the dialog of its failure here.
        match self.watch_failure() {
            crate::dialog::Failure::None => {}
            crate::dialog::Failure::Waiting(_) => return None,
            crate::dialog::Failure::Closed(_) => return Some(false),
        }
        // A file that is not there fails without asking first.
        if let Some(file) = self.existing_read(spec) {
            let action = FileAction {
                title: format!("Slot {slot}: Load file"),
                summary: format!("Load this file into slot {slot}."),
                details: self.details(&file, None),
                accept: "Load",
                notice: format!("Loading {file} into slot {slot}"),
                in_programs: false,
                originals: Vec::new(),
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
        let loaded = self.finish(what, outcome).is_some();
        (!self.failure_open()).then_some(loaded)
    }

    /// A file named for reading, in this project or another: programs
    /// LOAD, LOADV and LOADG other projects' files too, while EXEC checks
    /// [`names_project`] first.
    pub(crate) fn read_name(&self, spec: &str, default: FileKind) -> storage::Result<FileRef> {
        let mut file = parse_read_name(spec, default)?;
        if file.project.as_deref() == Some(self.storage.project()) {
            file.project = None;
        }
        Ok(file)
    }

    /// SAVE, SAVEV and SAVEG into one of SmileBASIC 4's own `#` projects
    /// (see [`Runtime::refuse_system_project`]): Can't SAVE to system
    /// project, in every mode. Then a main program may not name a
    /// project, not even its own or the workspace's `/NAME`: Can't access
    /// other project in this mode. A subprogram and DIRECT mode write to
    /// any other project.
    pub(crate) fn refuse_save_project(&self, spec: &str) -> Result<()> {
        self.refuse_system_project(spec)?;
        // A project part counts before the name is checked: a main
        // program's `SAVEV "TXT:/#A"` is refused this way too.
        let project = storage::split_name(spec).is_some_and(|parts| parts.project.is_some());
        match project {
            true if !self.direct && self.env_type == 0 => {
                Err(ErrorKind::CantAccessOtherProject.into())
            }
            _ => Ok(()),
        }
    }

    /// Can't SAVE to system project for a file in one of SmileBASIC 4's
    /// own `#` projects: one named in the file, or the current project
    /// when the file names none, as SmileBASIC 4's path builder flags
    /// both. The setting to write in them lets the file through.
    pub(crate) fn refuse_system_project(&self, spec: &str) -> Result<()> {
        if self.settings.write_system_projects {
            return Ok(());
        }
        let project = parse_read_name(spec, FileKind::Txt).map(|f| f.project);
        let system = match project {
            Ok(Some(p)) => p.starts_with('#'),
            Ok(None) => self.storage.project().starts_with('#'),
            Err(_) => false,
        };
        match system {
            true => Err(ErrorKind::CantSaveToSystemProject.into()),
            false => Ok(()),
        }
    }

    /// Whether the current project is one of SmileBASIC 4's own `#`
    /// projects while the setting to write in them is off, so that
    /// METASAVE and the editor's save write nothing there.
    pub fn current_project_read_only(&self) -> bool {
        !self.settings.write_system_projects && self.storage.project().starts_with('#')
    }

    /// A file named for writing, in the current project or, named with
    /// its project, in another one, which must exist. SmileBASIC 4's own
    /// `#` projects take no files: the commands refuse them first (see
    /// [`Runtime::refuse_save_project`]), and writing one otherwise fails,
    /// unless the setting to write in them is on.
    pub(crate) fn write_name(&self, spec: &str, default: FileKind) -> storage::Result<FileRef> {
        let file = self.read_name(spec, default)?;
        match &file.project {
            Some(p) if p.starts_with('#') && !self.settings.write_system_projects => {
                Err(StorageError::BadName(spec.to_string()))
            }
            _ => Ok(file),
        }
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

    /// Of the files there from the start in the folder a program was run
    /// from, those that writing `file` changes: the file, and when it is
    /// there, the backup it moves to.
    pub(crate) fn originals_of(&self, file: &FileRef, backup: Option<&str>) -> Vec<String> {
        let mut found: Vec<String> = self.storage.original(file).into_iter().collect();
        if let Some(backup) = backup
            && backup != file.name
            && self.storage.exists_ref(file)
        {
            let backup = FileRef {
                name: backup.to_string(),
                ..file.clone()
            };
            found.extend(self.storage.original(&backup));
        }
        found
    }

    /// [`Runtime::originals_of`] for DELETE's and RENAME's names, in the
    /// current project. A name without a type takes the type before it.
    fn originals_named(&self, specs: &[&str]) -> Vec<String> {
        let (mut kind, mut found) = (FileKind::Txt, Vec::new());
        for spec in specs {
            let Ok((named, name)) = parse_file_name(spec, kind) else {
                continue;
            };
            kind = named;
            let file = FileRef {
                kind,
                project: None,
                name,
            };
            found.extend(self.storage.original(&file));
        }
        found
    }

    /// [`Runtime::originals_of`] for saving a slot as `spec`, for Ctrl+S
    /// to ask first.
    pub fn original_txt(&self, spec: &str) -> Vec<String> {
        match self.write_name(spec, FileKind::Txt) {
            Ok(file) => self.originals_of(&file, Some(storage::BACKUP_NAME)),
            Err(_) => Vec::new(),
        }
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
    /// failure on the console. In a program a failure opens a dialog that
    /// the command waits on, as on SmileBASIC 4, when file access asks;
    /// otherwise it is a notice, or nothing when file access is silent.
    pub(crate) fn finish<T>(&mut self, what: &str, outcome: storage::Result<T>) -> Option<T> {
        match outcome {
            Ok(value) => {
                self.result = 1;
                Some(value)
            }
            Err(e) => {
                self.result = 0;
                if !self.direct && self.settings.file_confirm == FileConfirm::Ask {
                    self.open_failure(what, e.to_string());
                } else {
                    self.report_failure(what, &e);
                }
                None
            }
        }
    }

    /// Explains a failure as [`Runtime::finish`] does, leaving RESULT()
    /// as it is.
    pub(crate) fn report_failure(&mut self, what: &str, e: &StorageError) {
        if self.direct {
            // SmileBASIC 4 shows its own dialogs instead (see docs/differences.md).
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
        // A listed name may break the naming rules, but is only ever one
        // file's name.
        if name.contains(['/', '\\', ':']) {
            return Err(StorageError::BadName(name.to_string()));
        }
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
        self.slot_changes[slot] += 1;
        let project = file.project.as_deref().unwrap_or(self.storage.project());
        self.replaced_lines(slot, Some(format!("{project}/{}", file.name)));
        self.drop_prg_edit(slot);
        self.slot_names[slot] = file.name.clone();
        self.slot_modified[slot] = false;
        Ok(())
    }

    /// Ctrl+S: a slot saved under a name, without asking. A `#` project
    /// takes nothing (see [`Runtime::current_project_read_only`]), as
    /// SmileBASIC 4's editor refuses to save there before it asks for a
    /// name.
    pub(crate) fn save_slot_as(&mut self, spec: &str, slot: usize) -> storage::Result<()> {
        if self.current_project_read_only() {
            return Err(StorageError::SystemProject);
        }
        self.save(spec, slot)
    }

    /// Ctrl+L: a slot loaded from a file, without asking.
    pub(crate) fn load_slot_from(&mut self, spec: &str, slot: usize) -> storage::Result<()> {
        self.load(spec, slot)
    }

    /// Whether a TXT file of this name is in the current project.
    pub fn txt_exists(&self, spec: &str) -> bool {
        parse_file_name(spec, FileKind::Txt)
            .is_ok_and(|(kind, name)| self.storage.exists(kind, &name))
    }

    fn save(&mut self, spec: &str, slot: usize) -> storage::Result<()> {
        let file = self.write_name(spec, FileKind::Txt)?;
        if file.kind != FileKind::Txt {
            return Err(StorageError::BadKind(file.kind.name().to_string()));
        }
        let text = match self.settings.half_width_on_save {
            true => half_width(&self.slots[slot]),
            false => self.slots[slot].clone(),
        };
        let bytes = sb_core::wtf8::encode(&text);
        (self.storage).write_ref(&file, &bytes, Some(storage::BACKUP_NAME))?;
        self.slot_names[slot] = file.name;
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

    /// Whether FILES names a project that is not there.
    fn no_such_project(&self, spec: &str) -> bool {
        matches!(parse_listing(spec), Ok((_, Some(p))) if !self.storage.project_exists(&p))
    }

    /// FILES' entries: projects for `"//"`, else files of a project, each
    /// with its `TYPE:` prefix, also when one type was asked for (checked
    /// on hardware: `FILES "DAT:"` lists `DAT:T5A`).
    fn listing(&self, spec: &str) -> storage::Result<Vec<String>> {
        // Projects end in `/`, the workspace is `/` itself, and the system's
        // `#` projects come twice, as SmileBASIC 4 lists them.
        if spec == "//" {
            let mut names: Vec<String> = Vec::new();
            for p in self.storage.projects()? {
                let twice = p.starts_with('#');
                names.push(format!("{p}/"));
                if twice {
                    names.push(format!("{p}/"));
                }
            }
            names.push("/".to_string());
            // In ASCII order, so that the workspace's `/` comes after `#`
            // and `-` names.
            names.sort();
            return Ok(names);
        }
        let (kind, project) = parse_listing(spec)?;
        let files = self.storage.list(kind, project.as_deref())?;
        Ok(files
            .into_iter()
            .map(|(k, name)| format!("{}:{name}", k.name()))
            .collect())
    }
}

/// Whether a file name has a project part: another project's, the
/// current one's own, or the workspace's `/NAME`.
fn has_project(spec: &str) -> bool {
    parse_read_name(spec, FileKind::Txt).is_ok_and(|f| f.project.is_some())
}

/// RENAME and DELETE take no name with a project part, in a program, a
/// subprogram or at the prompt: Can't access other project in this mode.
fn refuse_project(spec: &str) -> Result<()> {
    match has_project(spec) {
        true => Err(ErrorKind::CantAccessOtherProject.into()),
        false => Ok(()),
    }
}

/// RENAME keeps a file's type: a second name of another type is File
/// type mismatch, before anything is asked. A second name without a
/// type takes the first's.
fn same_type(from: &str, to: &str) -> Result<()> {
    let kind_of = |spec| storage::split_name(spec).map(|parts| parts.kind);
    let (Some(kind), Some(Some(to_kind))) = (kind_of(from), kind_of(to)) else {
        return Ok(());
    };
    let kind = kind.unwrap_or(NamedKind::File(FileKind::Txt));
    match to_kind != kind {
        true => {
            let detail = format!("{} file to {}", kind.name(), to_kind.name());
            Err(Error::with_detail(ErrorKind::FileTypeMismatch, detail))
        }
        false => Ok(()),
    }
}

/// Whether a file name names a project, the way EXEC refuses in programs
/// (see its caller). The program's own project counts, and the
/// workspace's `/NAME`; SmileBASIC 4's own `#` projects do not.
pub(crate) fn names_project(spec: &str) -> bool {
    parse_read_name(spec, FileKind::Txt)
        .is_ok_and(|file| file.project.is_some_and(|p| !p.starts_with('#')))
}

/// The error for a name that SmileBASIC 4's file commands refuse before
/// they look for the file or ask anything, on every system.
fn name_error(error: NameError, spec: &str) -> Error {
    let kind = match error {
        NameError::Illegal => ErrorKind::IllegalFormat,
        NameError::TooLong => ErrorKind::FileNameTooLong,
    };
    Error::with_detail(kind, format!("{spec:?}"))
}

/// A file's name as argument `arg` of CHKFILE, LOAD, SAVE, LOADV, SAVEV,
/// LOADG, SAVEG, DELETE or RENAME: Illegal format where SmileBASIC 4's
/// parser fails or no file is named, as `CHKFILE("TXT:A:B")`,
/// `SAVE "NUL/A"` and `DELETE "TXT:"`, and File name is too long for a
/// project's or file's name of over 32 characters, both numbering the
/// argument (`(CHKFILE:1)`).
pub(crate) fn check_file_name(spec: &str, arg: usize) -> Result<()> {
    match storage::file_name_error(spec) {
        Some(error) => Err(name_error(error, spec).of_arg(arg)),
        None => Ok(()),
    }
}

/// FILES' `[TYPE:][PROJECT][/]`, refused as [`check_file_name`] refuses
/// a file's name, also when empty, or when it names a file in a project.
/// `//` lists the projects. Unlike other commands' names, FILES' are
/// refused naming no argument.
// UNVERIFIED: for FILES() as for the command.
fn check_listing_name(spec: &str) -> Result<()> {
    match storage::listing_name_error(spec) {
        Some(error) if spec != "//" => Err(name_error(error, spec)),
        _ => Ok(()),
    }
}

/// EXEC's file, refused as [`check_file_name`] refuses a name but without
/// the argument's number. A name SmileBASIC 4's parser takes, but of
/// another type than a program's, is File type mismatch before the rest
/// is checked: `EXEC "DAT:",1` is error 77, not 71.
pub(crate) fn check_exec_name(spec: &str) -> Result<()> {
    let error = match storage::split_name(spec) {
        None => Some(NameError::Illegal),
        Some(_) => {
            program_type(spec)?;
            storage::file_name_error(spec)
        }
    };
    match error {
        Some(error) => Err(name_error(error, spec)),
        None => Ok(()),
    }
}

/// LOAD and SAVE take only text files, before anything is asked: another
/// type of file (`DAT:`, `PNG:`) is File type mismatch, whether there is
/// such a file or not. A type that is none (`GRP0:`) is refused before,
/// by [`check_file_name`].
fn program_type(spec: &str) -> Result<()> {
    match storage::split_name(spec).and_then(|parts| parts.kind) {
        None | Some(NamedKind::File(FileKind::Txt)) => Ok(()),
        Some(kind) => {
            let detail = format!("{} file", kind.name());
            Err(Error::with_detail(ErrorKind::FileTypeMismatch, detail))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_width_converts_letters_and_digits_only() {
        assert_eq!(half_width("ＰＲＩＮＴ　\"ａ１！\""), "PRINT　\"a1！\"");
    }

    #[test]
    fn names_project_sees_a_project_before_the_name() {
        for named in [
            "GAME/MAIN.PRG",
            "TXT:GAME/MAIN.PRG",
            "GRP:test05/HG16",
            "/MAIN.PRG",
        ] {
            assert!(names_project(named), "{named}");
        }
        for plain in ["MAIN.PRG", "GRP:HG16", "#TOOL/T.PRG", "A/B/C"] {
            assert!(!names_project(plain), "{plain}");
        }
    }
}

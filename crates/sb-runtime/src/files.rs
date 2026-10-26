//! File commands: LOAD, SAVE, FILES, PROJECT, CHKFILE, DELETE, RENAME
//! and RESULT.
//!
//! Failures do not stop the program; they set RESULT() to 0. In direct mode
//! a message also explains what went wrong.

use sb_core::bytecode::Want;
use sb_core::error::{ErrorKind, Result};
use sb_core::value::{Array, ArrayData, ElemType, Value, new_str};
use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::{Command, Runtime, illegal, slot_arg};
use crate::sbfile;
use crate::storage::{self, FileKind, StorageError, parse_file_name, parse_listing};
use crate::text::CONSOLE;

fn str_arg(v: &Value) -> Result<String> {
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
fn program_text(bytes: &[u8]) -> storage::Result<String> {
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
                let name = str_arg(&args[0])?;
                let outcome = self.storage.set_project(&name);
                self.finish("PROJECT", outcome);
                Ok(vec![])
            }
            (Command::Chkfile, Want::Value, 1) => {
                let found = parse_file_name(&str_arg(&args[0])?, FileKind::Txt)
                    .is_ok_and(|(kind, name)| self.storage.exists(kind, &name));
                Ok(vec![Value::Int(i32::from(found))])
            }
            (Command::Load, Want::Outs(0), 1 | 2) => {
                let spec = str_arg(&args[0])?;
                let slot = slot_arg(args.get(1))?;
                let outcome = self.load(&spec, slot);
                self.finish("LOAD", outcome);
                Ok(vec![])
            }
            (Command::Save, Want::Outs(0), 1 | 2) => {
                let spec = str_arg(&args[0])?;
                let slot = slot_arg(args.get(1))?;
                let outcome = self.save(&spec, slot);
                self.finish("SAVE", outcome);
                Ok(vec![])
            }
            (Command::Delete, Want::Outs(0), 1) => {
                let spec = str_arg(&args[0])?;
                let outcome = parse_file_name(&spec, FileKind::Txt)
                    .and_then(|(kind, name)| self.storage.delete(kind, &name));
                self.finish("DELETE", outcome);
                Ok(vec![])
            }
            (Command::Rename, Want::Outs(0), 2) => {
                let (from, to) = (str_arg(&args[0])?, str_arg(&args[1])?);
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
                        let header = format!("-- {} files --", items.len());
                        console.print(&header.encode_utf16().collect::<Vec<_>>());
                        console.newline();
                        for item in items {
                            console.print(&item.encode_utf16().collect::<Vec<_>>());
                            console.newline();
                        }
                    }
                }
                Ok(vec![])
            }
            _ => Err(illegal()),
        }
    }

    /// Sets RESULT() from an outcome; in direct mode, also explains a
    /// failure on the console.
    fn finish<T>(&mut self, what: &str, outcome: storage::Result<T>) -> Option<T> {
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
                }
                None
            }
        }
    }

    fn load(&mut self, spec: &str, slot: usize) -> storage::Result<()> {
        let (kind, name) = parse_file_name(spec, FileKind::Txt)?;
        if kind != FileKind::Txt {
            return Err(StorageError::BadKind(kind.name().to_string()));
        }
        self.slots[slot] = program_text(&self.storage.read(kind, &name)?)?;
        self.slot_names[slot] = name;
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
        self.storage.write(kind, &name, text.as_bytes(), true)?;
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

    /// FILES' entries: projects for `"//"`, else files of a project. Names
    /// carry a `TYPE:` prefix unless one type was asked for, as on SmileBASIC
    /// 4 (`TXT:MAIN.PRG`).
    // UNVERIFIED: how other types are shown; a GRP file appeared as its
    // stored name (`GDEFFONT`) on hardware.
    fn listing(&self, spec: &str) -> storage::Result<Vec<String>> {
        if spec == "//" {
            return self.storage.projects();
        }
        let (kind, project) = parse_listing(spec)?;
        let files = self.storage.list(kind, project.as_deref())?;
        Ok(files
            .into_iter()
            .map(|(k, name)| match kind {
                Some(_) => name,
                None => format!("{}:{name}", k.name()),
            })
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

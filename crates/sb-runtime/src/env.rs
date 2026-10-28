//! The program's surroundings: ENVTYPE, ENVSTAT, ENVFOCUS, ENVINPUT$,
//! ENVLOAD, ENVSAVE and ENVPROJECT as the main interpreter sees them,
//! CLIPBOARD, and the project's metadata (METALOAD, METAEDIT, METASAVE).
//!
//! There are no subprograms yet, so programs always run on the main
//! interpreter, which has the focus.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{Array, ArrayData, Value, new_str};

use crate::confirm::{FileAction, project_label};
use crate::image::{self, Image};
use crate::runtime::{Command, Runtime, illegal, opt_int};
use crate::sbfile;
use crate::storage;

/// Direct-mode lines that ENVINPUT$ can read back.
pub const HISTORY: usize = 32;
/// The icon's side in pixels.
pub const ICON_SIZE: usize = 40;
const TITLE_CHARS: usize = 24;
const DESCRIPTION_CHARS: usize = 240;
/// The metadata's files in the project folder: the title on the first
/// line and the description after it, and the icon as a PNG. Their names
/// start with `@`, so they never clash with SmileBASIC's typed files.
const META_TEXT: &str = "@META.TXT";
const META_ICON: &str = "@ICON.PNG";
/// SmileBASIC 4's own metadata file, read when the emulator's are missing.
pub const SB4_META: &str = "META";

/// Somewhere to keep the text that CLIPBOARD sets and gets: the
/// computer's clipboard in the frontend.
pub trait Clipboard {
    fn text(&mut self) -> String;
    fn set_text(&mut self, text: &str);
}

/// A clipboard of the emulator's own, for headless runs and tests.
#[derive(Default)]
pub struct MemoryClipboard(String);

impl Clipboard for MemoryClipboard {
    fn text(&mut self) -> String {
        self.0.clone()
    }

    fn set_text(&mut self, text: &str) {
        self.0 = text.to_string();
    }
}

/// The project's title, description and 40x40 icon, as METAEDIT edits
/// them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    /// The project they belong to.
    pub project: String,
    pub title: Vec<u16>,
    pub description: Vec<u16>,
    pub icon: Vec<i32>,
}

impl Default for Meta {
    fn default() -> Self {
        Meta {
            project: String::new(),
            title: Vec::new(),
            description: Vec::new(),
            icon: vec![0; ICON_SIZE * ICON_SIZE],
        }
    }
}

/// What the main interpreter is doing, as ENVSTAT reports it.
mod status {
    pub const DIRECT: i32 = 0;
    pub const RUNNING: i32 = 3;
}

fn str_units(v: &Value) -> Result<Vec<u16>> {
    Ok(v.clone().into_str()?.borrow().clone())
}

impl Runtime {
    /// Remembers a line typed in direct mode, for ENVINPUT$.
    pub(crate) fn remember_input(&mut self, line: &str) {
        self.history.push_front(line.encode_utf16().collect());
        self.history.truncate(HISTORY);
    }

    /// The metadata being edited. Switching projects starts it afresh,
    /// titled with the project's name.
    fn meta(&mut self) -> &mut Meta {
        if self.meta.project != self.storage.project() {
            self.meta = self.blank_meta();
        }
        &mut self.meta
    }

    fn blank_meta(&self) -> Meta {
        project_meta(self.storage.project(), |_| None)
    }

    fn status(&self) -> i32 {
        match self.running_slot {
            Some(_) => status::RUNNING,
            None => status::DIRECT,
        }
    }

    pub(crate) fn env_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        let int = |n: i32| Value::Int(n);
        match (command, want, args) {
            (Command::Envtype, Want::Value, []) => Ok(vec![int(0)]),
            // The manual says -1 for user programs; it is 0 on hardware.
            (Command::Envtype, Want::Outs(2), []) => Ok(vec![int(0), int(0)]),
            (Command::Envstat, Want::Value, []) => Ok(vec![int(self.status())]),
            // Only subprograms set the main interpreter's status.
            (Command::Envstat, Want::Outs(0), [s]) => {
                s.to_int()?;
                Ok(vec![])
            }
            (Command::Envstat, Want::Outs(3), []) => Ok(vec![
                int(self.status()),
                int(self.running_slot.unwrap_or(0) as i32),
                int(self.edit_slot as i32),
            ]),
            (Command::Envstat, Want::Outs(6), []) => {
                let mut out = vec![int(self.status())];
                out.extend([0; 5].map(int));
                Ok(out)
            }
            (Command::Envfocus, Want::Value, []) => Ok(vec![int(1)]),
            (Command::Envfocus, Want::Outs(0), [flag]) => {
                flag.to_int()?;
                Ok(vec![])
            }
            (Command::Envinput, Want::Value, _) if args.len() <= 1 => {
                let n = args
                    .first()
                    .map(opt_int)
                    .transpose()?
                    .flatten()
                    .unwrap_or(0);
                let text = match n {
                    0 => self.typed_line().unwrap_or_default(),
                    1..=32 => (self.history.get(n as usize - 1).cloned()).unwrap_or_default(),
                    n => {
                        let detail = format!("history {n}");
                        return Err(Error::with_detail(ErrorKind::OutOfRange, detail));
                    }
                };
                Ok(vec![Value::Str(new_str(text))])
            }
            // Only subprograms copy; on the main interpreter they do
            // nothing (checked on hardware for ENVLOAD).
            (Command::Envload | Command::Envsave, Want::Outs(0), [a, b, c]) => {
                for v in [a, b, c] {
                    v.to_int()?;
                }
                Ok(vec![])
            }
            (Command::Envproject, Want::Outs(0), []) => Ok(vec![]),
            (Command::Clipboard, Want::Value, []) => {
                let text = self.clipboard.text();
                Ok(vec![Value::str(&text)])
            }
            (Command::Clipboard, Want::Outs(0), [text]) => {
                let text = String::from_utf16_lossy(&str_units(text)?);
                self.clipboard.set_text(&text);
                Ok(vec![])
            }
            (Command::Metaload, Want::Outs(0), []) => {
                self.meta = self.read_meta();
                Ok(vec![])
            }
            (Command::Metaedit, Want::Outs(0), [item, value]) => {
                match item.to_int()? {
                    0 => self.meta().title = limited(str_units(value)?, TITLE_CHARS),
                    1 => self.meta().description = limited(str_units(value)?, DESCRIPTION_CHARS),
                    2 => {
                        let Value::Array(a) = value else {
                            return Err(ErrorKind::TypeMismatch.into());
                        };
                        let a = a.borrow();
                        let icon = match &a.data {
                            ArrayData::Int(v) => v.clone(),
                            ArrayData::Real(v) => (v.iter())
                                .map(|&x| sb_core::value::real_to_int(x))
                                .collect::<Result<_>>()?,
                            ArrayData::Str(_) => return Err(ErrorKind::TypeMismatch.into()),
                        };
                        if icon.len() != ICON_SIZE * ICON_SIZE {
                            let detail = format!("{} elements", icon.len());
                            return Err(Error::with_detail(ErrorKind::OutOfRange, detail));
                        }
                        self.meta().icon = icon;
                    }
                    n => {
                        return Err(Error::with_detail(
                            ErrorKind::OutOfRange,
                            format!("item {n}"),
                        ));
                    }
                }
                Ok(vec![])
            }
            (Command::Metaedit, Want::Outs(1), [item]) => Ok(vec![match item.to_int()? {
                0 => Value::Str(new_str(self.meta().title.clone())),
                1 => Value::Str(new_str(self.meta().description.clone())),
                2 => Value::Array(Rc::new(RefCell::new(Array {
                    dims: vec![ICON_SIZE, ICON_SIZE],
                    data: ArrayData::Int(self.meta().icon.clone()),
                }))),
                n => {
                    return Err(Error::with_detail(
                        ErrorKind::OutOfRange,
                        format!("item {n}"),
                    ));
                }
            }]),
            (Command::Metasave, Want::Outs(0), []) => {
                let action = FileAction {
                    title: "Save metadata".to_string(),
                    summary: "Save the project's title, description and icon.".to_string(),
                    details: vec![format!("  Project:  {}", self.project_label())],
                    accept: "Save",
                    notice: format!("Saving the metadata of {}", self.project_label()),
                    in_programs: true,
                };
                let ask = self.confirm_file(action, &[]);
                if self.held(ask) {
                    return Ok(vec![]);
                }
                let outcome = self.write_meta();
                self.finish("METASAVE", outcome);
                Ok(vec![])
            }
            _ => Err(illegal()),
        }
    }

    /// The project's metadata, read again from its files.
    fn read_meta(&self) -> Meta {
        let storage = &self.storage;
        project_meta(storage.project(), |name| {
            storage.read_project_file(name).ok()
        })
    }

    fn write_meta(&mut self) -> storage::Result<()> {
        self.meta();
        for (name, bytes) in meta_files(&self.meta) {
            self.storage.write_project_file(name, &bytes)?;
        }
        Ok(())
    }
}

/// A project's metadata from its files, which `read` gives by name: the
/// emulator's own, or else SmileBASIC 4's META file, or else empty
/// metadata titled with the project's name (checked on hardware: the
/// workspace's is "[WORKSPACE]").
pub fn project_meta(project: &str, read: impl Fn(&str) -> Option<Vec<u8>>) -> Meta {
    let mut meta = Meta {
        project: project.to_string(),
        title: project_label(project).encode_utf16().collect(),
        ..Meta::default()
    };
    let sb4 = read(SB4_META)
        .and_then(|bytes| sbfile::parse(&bytes).ok())
        .and_then(|file| sbfile::parse_meta(&file.body).ok());
    if let Some(sb4) = sb4 {
        meta.title = sb4.title;
        meta.description = sb4.description;
        meta.icon = sb4.icon.into_iter().map(|p| p as i32).collect();
    }
    if let Some(bytes) = read(META_TEXT) {
        let text = String::from_utf8_lossy(&bytes);
        let (title, description) = text.split_once('\n').unwrap_or((&text, ""));
        meta.title = title.encode_utf16().take(TITLE_CHARS).collect();
        meta.description = description.encode_utf16().take(DESCRIPTION_CHARS).collect();
    }
    if let Some(bytes) = read(META_ICON)
        && let Ok(icon) = image::decode(&bytes)
        && icon.pixels.len() == ICON_SIZE * ICON_SIZE
    {
        meta.icon = icon.pixels.into_iter().map(|p| p as i32).collect();
    }
    meta
}

/// Whether `name` is one of the files that hold a project's metadata.
pub fn is_meta_file(name: &str) -> bool {
    matches!(name, META_TEXT | META_ICON | SB4_META)
}

/// The emulator's files for `meta`, by name.
pub fn meta_files(meta: &Meta) -> [(&'static str, Vec<u8>); 2] {
    let mut text = String::from_utf16_lossy(&meta.title);
    text.push('\n');
    text.push_str(&String::from_utf16_lossy(&meta.description));
    let icon = Image {
        width: ICON_SIZE,
        height: ICON_SIZE,
        pixels: meta.icon.iter().map(|&p| p as u32).collect(),
    };
    [
        (META_TEXT, text.into_bytes()),
        (META_ICON, image::encode_png(&icon)),
    ]
}

/// A title or description, cut to `max` characters.
// Checked on hardware: a longer title is no error. UNVERIFIED: that it
// is cut.
fn limited(mut text: Vec<u16>, max: usize) -> Vec<u16> {
    text.truncate(max);
    text
}

/// The history type, newest first.
pub(crate) type History = VecDeque<Vec<u16>>;

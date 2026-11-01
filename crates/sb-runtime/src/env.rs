//! The program's surroundings: ENVTYPE, ENVSTAT, ENVFOCUS, ENVINPUT$,
//! ENVLOAD, ENVSAVE and ENVPROJECT as the main interpreter sees them,
//! CLIPBOARD, and the project's metadata (METALOAD, METAEDIT, METASAVE).
//!
//! The sub interpreter's own side of them is here too; where they reach
//! the main interpreter, see [`crate::sub`].

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{Array, ArrayData, Value, new_str};

use crate::confirm::{FileAction, project_label};
use crate::files::str_arg;
use crate::image::{self, Image};
use crate::input::in_range;
use crate::runtime::{Command, Request, Runtime, no_form, opt_int};
use crate::sbfile;
use crate::storage;

/// Direct-mode lines that ENVINPUT$ can read back.
pub const HISTORY: usize = 32;
/// The icon's side in pixels.
pub const ICON_SIZE: usize = 40;
pub const TITLE_CHARS: usize = 24;
pub const DESCRIPTION_CHARS: usize = 240;
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
    /// The current project when they were loaded: METALOAD may load
    /// another's, which switching projects drops.
    pub home: String,
    pub title: Vec<u16>,
    pub description: Vec<u16>,
    pub icon: Vec<i32>,
}

impl Default for Meta {
    fn default() -> Self {
        Meta {
            project: String::new(),
            home: String::new(),
            title: Vec::new(),
            description: Vec::new(),
            icon: vec![0; ICON_SIZE * ICON_SIZE],
        }
    }
}

/// What the main interpreter is doing, as ENVSTAT reports it.
mod status {
    pub const RUNNING: i32 = 3;
}

fn str_units(v: &Value) -> Result<Vec<u16>> {
    Ok(v.clone().into_str()?.borrow().clone())
}

impl Runtime {
    /// Remembers a line typed in direct mode, for ENVINPUT$.
    pub(crate) fn remember_input(&mut self, line: &str) {
        let line: Vec<u16> = line.encode_utf16().collect();
        // Running the line just before again adds nothing (checked on
        // hardware); UNVERIFIED: whether an older equal line moves up.
        if self.history.front() != Some(&line) {
            self.history.push_front(line);
        }
        self.history.truncate(HISTORY);
    }

    /// The metadata being edited. Switching projects starts it afresh,
    /// titled with the project's name.
    fn meta(&mut self) -> &mut Meta {
        if self.meta.home != self.storage.project() {
            self.meta = self.blank_meta();
        }
        &mut self.meta
    }

    fn blank_meta(&self) -> Meta {
        project_meta(self.storage.project(), |_| None)
    }

    /// The main interpreter's status: always running, as the code that
    /// asks is running on it, even when typed in direct mode while a
    /// program is stopped. Only subprograms see the others.
    fn status(&self) -> i32 {
        status::RUNNING
    }

    /// ENVLOCATE: where the console's cursor is on the 1280x720 display,
    /// for a subprogram to show something by it, as SmileBASIC 4's soft
    /// keyboard does. The command is not in the manual. The screen is
    /// scaled to fit the display and centered, and the point is the top
    /// of the cursor's cell, a character to its right. While the editor
    /// shows, it is 0,0.
    pub(crate) fn cursor_on_display(&self) -> (i32, i32) {
        if self.env_status == crate::sub::status::EDITING {
            return (0, 0);
        }
        let console = &self.text[crate::text::CONSOLE];
        let (cx, cy) = console.cursor();
        let size = f64::from(console.font().1);
        let x = (f64::from(cx) + 1.0) * size + console.pos.0;
        let y = f64::from(cy) * size + console.pos.1;
        let (w, h) = (f64::from(self.mode.width), f64::from(self.mode.height));
        let scale = (1280.0 / w).min(720.0 / h);
        let (left, top) = ((1280.0 - w * scale) / 2.0, (720.0 - h * scale) / 2.0);
        ((left + x * scale) as i32, (top + y * scale) as i32)
    }

    pub(crate) fn env_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        let int = |n: i32| Value::Int(n);
        match (command, want, args) {
            (Command::Envtype, Want::Value, []) => Ok(vec![int(self.env_type)]),
            // The manual says -1 for user programs; it is 0 on hardware.
            // A subprogram's is -1.
            (Command::Envtype, Want::Outs(2), []) => {
                Ok(vec![int(self.env_type), int(-self.env_type)])
            }
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
            (Command::Envfocus, Want::Value, []) => Ok(vec![int(self.focus.into())]),
            (Command::Xsubscreen, Want::Outs(0), [off]) if self.env_type == 1 => {
                match off.to_int()? {
                    0 => self.sub_screen = None,
                    n => return Err(Error::with_detail(ErrorKind::OutOfRange, format!("{n}"))),
                }
                Ok(vec![])
            }
            (Command::Xsubscreen, Want::Outs(0), [x, y, w, h]) if self.env_type == 1 => {
                self.sub_screen = Some([
                    in_range("x", x, -32768..=32767)?,
                    in_range("y", y, -32768..=32767)?,
                    in_range("width", w, 1..=1280)?,
                    in_range("height", h, 1..=720)?,
                ]);
                Ok(vec![])
            }
            // Before XSUBSCREEN, the whole display.
            (Command::Xsubscreen, Want::Outs(n @ 2..=4), []) if self.env_type == 1 => {
                let rect = self.sub_screen.unwrap_or([0, 0, 1280, 720]);
                Ok(rect[..usize::from(n)].iter().map(|&v| int(v)).collect())
            }
            // The sub-screen does nothing outside a subprogram, and gives
            // 0s.
            (Command::Xsubscreen, Want::Outs(0), [_] | [_, _, _, _]) => {
                for a in args {
                    a.to_int()?;
                }
                Ok(vec![])
            }
            (Command::Xsubscreen, Want::Outs(n @ 2..=4), []) => Ok(vec![int(0); usize::from(n)]),
            // The system's own UI state (soft keyboard and help shown), which
            // reads 0 for the main interpreter.
            // UNVERIFIED: the other numbers, and that setting it does nothing
            // here.
            (Command::Uistate, Want::Value, [n]) => {
                n.to_int()?;
                Ok(vec![int(0)])
            }
            (Command::Uistate, Want::Outs(0), [n, v]) => {
                n.to_int()?;
                v.to_int()?;
                Ok(vec![])
            }
            // 1 takes the focus and 0 gives it to the other interpreter,
            // if a subprogram runs beside the main one. The focus moves a
            // few frames later, and ENVFOCUS meanwhile does nothing.
            (Command::Envfocus, Want::Outs(0), [flag]) => {
                let take = flag.to_int()? != 0;
                let other = self.env_type == 1 || self.sub_running;
                if other && !self.focus_busy && take != self.focus {
                    self.focus_wish.get_or_insert(take);
                }
                Ok(vec![])
            }
            // Direct mode only: SUBRUN starts the file's program on the sub
            // interpreter, and the others stop, show or hide it.
            (Command::Subrun, Want::Outs(0), [file]) => {
                let file = str_arg(file)?;
                self.request(Request::Subrun(file))
            }
            (Command::Substop, Want::Outs(0), []) => self.request(Request::Substop),
            (Command::Subshow, Want::Outs(0), []) => self.request(Request::Subshow(true)),
            (Command::Subhide, Want::Outs(0), []) => self.request(Request::Subshow(false)),
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
            (Command::Envlocate, Want::Outs(2), []) => {
                let (x, y) = self.cursor_on_display();
                Ok(vec![int(x), int(y)])
            }
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
                self.meta = self.read_meta(self.storage.project().to_string());
                Ok(vec![])
            }
            (Command::Metaload, Want::Outs(0), [name]) => {
                let name = match str_arg(name)?.as_str() {
                    "" => String::new(),
                    name => storage::check_project_name(name).map_err(|_| {
                        Error::with_detail(ErrorKind::IllegalProjectName, format!("{name:?}"))
                    })?,
                };
                if !self.storage.project_exists(&name) {
                    let detail = format!("no project {name}");
                    return Err(Error::with_detail(ErrorKind::UndefinedProjectName, detail));
                }
                self.meta = self.read_meta(name);
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
            (Command::Metaedit, Want::Outs(1) | Want::Value, [item]) => {
                Ok(vec![match item.to_int()? {
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
                }])
            }
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
            _ => Err(no_form()),
        }
    }

    /// A project's metadata, read again from its files.
    fn read_meta(&self, project: String) -> Meta {
        let storage = &self.storage;
        let meta = project_meta(&project, |name| storage.read_file_in(&project, name).ok());
        Meta {
            home: storage.project().to_string(),
            ..meta
        }
    }

    /// Into the current project, as the manual says, whichever project's
    /// METALOAD read.
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
        home: project.to_string(),
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

//! The [`Host`] that programs run against: text screens now, graphics,
//! sprites and sound later.

use std::collections::VecDeque;
use std::rc::Rc;

use sb_core::bytecode::{Compiled, Literal, Want};
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::host::Host;
use sb_core::value::{Value, new_str};

use crate::confirm::{Ask, Confirm, unsaved_lines};
use crate::editor::Editor;
use crate::font::Font;
use crate::framebuffer::Framebuffer;
use crate::graphics::Graphics;
pub use crate::settings::Settings;
use crate::sprites::{self, Sprites};
use crate::storage::Storage;
use crate::text::{CONSOLE, SCREENS, TextScreen};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Animdef,
    Chkfile,
    Clear,
    Cls,
    Color,
    Delete,
    Files,
    Gbox,
    Gcircle,
    Gclip,
    Gcls,
    Gcolor,
    Gcopy,
    Gfill,
    Gline,
    Gload,
    Gpaint,
    Gpget,
    Gpset,
    Gputchr,
    Gsample,
    Gsave,
    Gtarget,
    Gtri,
    Inkey,
    Load,
    Locate,
    New,
    Project,
    Rename,
    Result,
    Run,
    Save,
    Spanim,
    Spchk,
    Spchr,
    Spclr,
    Spcol,
    Spcolor,
    Spcolvec,
    Spdef,
    Sphide,
    Sphitinfo,
    Sphitrc,
    Sphitsp,
    Sphome,
    Splayer,
    Splink,
    Spofs,
    Sppage,
    Sprot,
    Spscale,
    Spset,
    Spshow,
    Spstart,
    Spstop,
    Spunlink,
    Spused,
    Spvar,
}

const COMMANDS: &[(&str, Command)] = &[
    ("ANIMDEF", Command::Animdef),
    ("CHKFILE", Command::Chkfile),
    ("CLEAR", Command::Clear),
    ("CLS", Command::Cls),
    ("COLOR", Command::Color),
    ("DELETE", Command::Delete),
    ("FILES", Command::Files),
    ("GBOX", Command::Gbox),
    ("GCIRCLE", Command::Gcircle),
    ("GCLIP", Command::Gclip),
    ("GCLS", Command::Gcls),
    ("GCOLOR", Command::Gcolor),
    ("GCOPY", Command::Gcopy),
    ("GFILL", Command::Gfill),
    ("GLINE", Command::Gline),
    ("GLOAD", Command::Gload),
    ("GPAINT", Command::Gpaint),
    ("GPGET", Command::Gpget),
    ("GPSET", Command::Gpset),
    ("GPUTCHR", Command::Gputchr),
    ("GSAMPLE", Command::Gsample),
    ("GSAVE", Command::Gsave),
    ("GTARGET", Command::Gtarget),
    ("GTRI", Command::Gtri),
    ("INKEY$", Command::Inkey),
    ("LOAD", Command::Load),
    ("LOCATE", Command::Locate),
    ("NEW", Command::New),
    ("PROJECT", Command::Project),
    ("RENAME", Command::Rename),
    ("RESULT", Command::Result),
    ("RUN", Command::Run),
    ("SAVE", Command::Save),
    ("SPANIM", Command::Spanim),
    ("SPCHK", Command::Spchk),
    ("SPCHR", Command::Spchr),
    ("SPCLR", Command::Spclr),
    ("SPCOL", Command::Spcol),
    ("SPCOLOR", Command::Spcolor),
    ("SPCOLVEC", Command::Spcolvec),
    ("SPDEF", Command::Spdef),
    ("SPHIDE", Command::Sphide),
    ("SPHITINFO", Command::Sphitinfo),
    ("SPHITRC", Command::Sphitrc),
    ("SPHITSP", Command::Sphitsp),
    ("SPHOME", Command::Sphome),
    ("SPLAYER", Command::Splayer),
    ("SPLINK", Command::Splink),
    ("SPOFS", Command::Spofs),
    ("SPPAGE", Command::Sppage),
    ("SPROT", Command::Sprot),
    ("SPSCALE", Command::Spscale),
    ("SPSET", Command::Spset),
    ("SPSHOW", Command::Spshow),
    ("SPSTART", Command::Spstart),
    ("SPSTOP", Command::Spstop),
    ("SPUNLINK", Command::Spunlink),
    ("SPUSED", Command::Spused),
    ("SPVAR", Command::Spvar),
];

/// Program slots PRG0 to PRG5.
pub const SLOTS: usize = 6;

/// Work for the [`crate::machine::Machine`] around the runtime, asked for
/// by direct-mode commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Run(usize),
    Clear,
}

/// UNVERIFIED: how many unread keys SmileBASIC keeps.
pub const KEY_BUFFER: usize = 256;
/// Frames per half period of the INPUT cursor's blink: at 60 fps, a full
/// blink takes about 833 ms.
const CURSOR_BLINK: u32 = 25;

/// Character codes for keys that do not produce text, as INKEY$ returns them.
pub mod keys {
    pub const HOME: u16 = 0x02;
    pub const END: u16 = 0x03;
    pub const BACKSPACE: u16 = 0x08;
    pub const TAB: u16 = 0x09;
    pub const ENTER: u16 = 0x0D;
    pub const DELETE: u16 = 0x10;
    pub const PAGE_UP: u16 = 0x11;
    pub const PAGE_DOWN: u16 = 0x12;
    pub const INSERT: u16 = 0x1A;
    pub const ESCAPE: u16 = 0x1B;
    pub const RIGHT: u16 = 0x1C;
    pub const LEFT: u16 = 0x1D;
    pub const UP: u16 = 0x1E;
    pub const DOWN: u16 = 0x1F;
}

pub struct Runtime {
    pub text: Vec<TextScreen>,
    pub(crate) font: Font,
    fb: Framebuffer,
    /// Graphic pages GRP0-GRP5.
    pub grp: Graphics,
    pub sprites: Sprites,
    /// Typed characters not yet read by INKEY$ or INPUT.
    keys: VecDeque<u16>,
    /// The line being typed for INPUT or LINPUT.
    line: Option<Vec<u16>>,
    frame: u32,
    /// Whether the code running was typed in direct mode, where RUN, NEW
    /// and CLEAR are allowed.
    pub direct: bool,
    requests: Vec<Request>,
    /// Source of each program slot.
    pub slots: Vec<String>,
    /// The file each slot was last loaded from or saved to; empty if none.
    pub slot_names: Vec<String>,
    /// Slots changed in the editor since they were last loaded or saved.
    pub slot_modified: Vec<bool>,
    pub settings: Settings,
    pub storage: Storage,
    /// What RESULT() returns: 1 if the last file command succeeded, 0 if
    /// it failed, -1 if the user cancelled it.
    pub(crate) result: i32,
    pub(crate) confirm: Option<Confirm>,
    pub(crate) answer: Option<bool>,
    /// The last command waits for an answer; see [`Host::retry`].
    pub(crate) retry: bool,
    pub(crate) notices: Vec<String>,
    /// The program last started, whose DATA commands like SPANIM read.
    pub(crate) program: Option<Rc<Compiled>>,
}

/// Reads DATA values in order; see [`Runtime::label_data`].
pub(crate) struct DataReader {
    program: Rc<Compiled>,
    pos: usize,
}

impl DataReader {
    pub(crate) fn number(&mut self) -> Result<f64> {
        let item = self
            .program
            .data
            .get(self.pos)
            .ok_or(ErrorKind::OutOfData)?;
        self.pos += 1;
        match item {
            Literal::Int(n) => Ok(f64::from(*n)),
            Literal::Real(x) => Ok(*x),
            Literal::Str(_) => Err(ErrorKind::TypeMismatch.into()),
        }
    }

    pub(crate) fn numbers(&mut self, n: usize) -> Result<Vec<f64>> {
        (0..n).map(|_| self.number()).collect()
    }
}

pub(crate) fn illegal() -> Error {
    ErrorKind::IllegalFunctionCall.into()
}

fn screen_id(v: &Value) -> Result<usize> {
    let id = v.to_int()?;
    usize::try_from(id)
        .ok()
        .filter(|&id| id < SCREENS)
        .ok_or_else(|| Error::with_detail(ErrorKind::OutOfRange, format!("screen {id}")))
}

/// An omittable integer argument.
pub(crate) fn opt_int(v: &Value) -> Result<Option<i32>> {
    match v {
        Value::Default => Ok(None),
        v => v.to_int().map(Some),
    }
}

/// An optional program slot number, 0 when omitted.
pub(crate) fn slot_arg(v: Option<&Value>) -> Result<usize> {
    let slot = v.map(opt_int).transpose()?.flatten().unwrap_or(0);
    usize::try_from(slot)
        .ok()
        .filter(|&s| s < SLOTS)
        .ok_or_else(|| Error::with_detail(ErrorKind::OutOfRange, format!("slot {slot}")))
}

/// Splits an optional leading screen id off the arguments: commands take
/// either `rest` arguments or a screen id plus `rest`.
fn with_screen(args: &[Value], rest: usize) -> Result<(usize, &[Value])> {
    match args.len() {
        n if n == rest => Ok((CONSOLE, args)),
        n if n == rest + 1 => Ok((screen_id(&args[0])?, &args[1..])),
        _ => Err(illegal()),
    }
}

impl Runtime {
    pub fn new(width: u32, height: u32, font: Font) -> Runtime {
        Runtime {
            text: (0..SCREENS)
                .map(|_| TextScreen::new(width, height))
                .collect(),
            font,
            fb: Framebuffer::new(width, height),
            grp: Graphics::default(),
            sprites: Sprites::default(),
            keys: VecDeque::new(),
            line: None,
            frame: 0,
            direct: false,
            requests: Vec::new(),
            slots: vec![String::new(); SLOTS],
            slot_names: vec![String::new(); SLOTS],
            slot_modified: vec![false; SLOTS],
            settings: Settings::default(),
            storage: Storage::none(),
            result: 1,
            confirm: None,
            answer: None,
            retry: false,
            notices: Vec::new(),
            program: None,
        }
    }

    /// The DATA values from `label` on, for commands that take a
    /// `"@label"`. They are read apart from READ, which keeps its place.
    // UNVERIFIED: whether these commands move READ's place.
    pub(crate) fn label_data(&self, label: &Value) -> Result<DataReader> {
        let text =
            String::from_utf16_lossy(&label.clone().into_str()?.borrow()).to_ascii_uppercase();
        let name = text.split_once(':').map_or(text.as_str(), |(_, rest)| rest);
        let name = name.strip_prefix('@').unwrap_or(name);
        let program = self.program.clone().unwrap_or_default();
        let pos = *program
            .data_labels
            .get(name)
            .ok_or_else(|| Error::with_detail(ErrorKind::UndefinedLabel, format!("@{name}")))?;
        Ok(DataReader {
            program,
            pos: pos as usize,
        })
    }

    pub fn font(&self) -> &Font {
        &self.font
    }

    /// Stops waiting for an INPUT line, if one is being typed.
    pub fn cancel_input(&mut self) {
        self.line = None;
    }

    /// Direct-mode requests made since the last call.
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    pub(crate) fn direct_only(&self) -> Result<()> {
        if self.direct {
            Ok(())
        } else {
            // UNVERIFIED: the error SmileBASIC 4 gives for these in a program.
            Err(Error::with_detail(
                ErrorKind::IllegalStatement,
                "only in direct mode",
            ))
        }
    }

    fn request(&mut self, request: Request) -> Result<Vec<Value>> {
        self.direct_only()?;
        self.requests.push(request);
        Ok(vec![])
    }

    /// Queues a typed character, or one of the [`keys`] codes. Keys typed
    /// while the buffer is full are dropped.
    pub fn key_press(&mut self, code: u16) {
        if self.keys.len() < KEY_BUFFER {
            self.keys.push_back(code);
        }
    }

    /// Takes the next unread key, for the editor.
    pub(crate) fn next_key(&mut self) -> Option<u16> {
        self.keys.pop_front()
    }

    /// Composes the current frame.
    pub fn render(&mut self) -> &Framebuffer {
        self.frame = self.frame.wrapping_add(1);
        self.fb.clear(0xFF00_0000);
        // Back to front: sprites behind the text, the text screens, then
        // sprites in front of it.
        // UNVERIFIED: text screens sit at Z 0 and win ties, and are drawn
        // in id order, until TOFS and layers exist.
        let order = self.sprites.draw_order();
        let (back, front): (Vec<_>, Vec<_>) = order.into_iter().partition(|(_, pose)| pose.z >= 0);
        for (i, pose) in back {
            sprites::draw_posed(&self.sprites.list[i], &pose, &self.grp, &mut self.fb);
        }
        for screen in &self.text {
            screen.draw(&mut self.fb, &self.font);
        }
        for (i, pose) in front {
            sprites::draw_posed(&self.sprites.list[i], &pose, &self.grp, &mut self.fb);
        }
        if self.line.is_some() && (self.frame / CURSOR_BLINK).is_multiple_of(2) {
            self.text[CONSOLE].draw_cursor(&mut self.fb);
        }
        &self.fb
    }

    /// Composes a frame showing the editor instead of the screens.
    pub(crate) fn render_editor(&mut self, editor: &mut Editor) -> &Framebuffer {
        self.frame = self.frame.wrapping_add(1);
        let slot = editor.slot();
        let mut name = self.slot_names[slot].clone();
        // The slot has edits that were not saved.
        if self.slot_modified[slot] || editor.is_modified() {
            name.push_str(" *");
        }
        let blink = (self.frame / CURSOR_BLINK).is_multiple_of(2);
        editor.draw(
            &mut self.fb,
            &self.font,
            &name,
            self.settings.editor_wrap,
            blink,
        );
        &self.fb
    }

    fn screen(&mut self, id: Option<i32>) -> Result<&mut TextScreen> {
        let id = match id {
            None => CONSOLE,
            Some(id) => screen_id(&Value::Int(id))?,
        };
        Ok(&mut self.text[id])
    }

    fn run(&mut self, command: Command, args: &[Value], want: Want) -> Result<Vec<Value>> {
        match (command, want) {
            (Command::Cls, Want::Outs(0)) => {
                let (id, _) = with_screen(args, 0)?;
                self.text[id].cls();
                Ok(vec![])
            }
            (Command::Color, Want::Value) => {
                let (id, _) = with_screen(args, 0)?;
                Ok(vec![Value::Int(self.text[id].color() as i32)])
            }
            (Command::Color, Want::Outs(0)) => {
                let (id, rest) = with_screen(args, 1)?;
                let color = rest[0].to_int()? as u32;
                self.text[id].set_color(color);
                Ok(vec![])
            }
            (Command::Clear, Want::Outs(0)) if args.is_empty() => self.request(Request::Clear),
            (Command::New, Want::Outs(0)) if args.len() <= 1 => {
                self.direct_only()?;
                let slots = match args.first() {
                    None => 0..SLOTS,
                    slot => slot_arg(slot).map(|s| s..s + 1)?,
                };
                let unsaved = self.unsaved_among(slots.clone());
                // Without unsaved edits, NEW clears at once (checked on hardware).
                if !unsaved.is_empty() {
                    let summary = match args.first() {
                        None => "Erase the programs in all slots.".to_string(),
                        Some(_) => format!("Erase the program in slot {}.", slots.start),
                    };
                    let ask = self.ask(|_| {
                        let mut lines = vec![summary, String::new()];
                        lines.extend(unsaved_lines(&unsaved));
                        Confirm {
                            title: "Erase programs".to_string(),
                            lines,
                            accept: "Erase",
                        }
                    });
                    if !matches!(ask, Ask::Go) {
                        return Ok(vec![]);
                    }
                }
                for slot in slots {
                    self.slots[slot].clear();
                    self.slot_names[slot].clear();
                    self.slot_modified[slot] = false;
                }
                Ok(vec![])
            }
            (Command::Run, Want::Outs(0)) if args.len() <= 1 => {
                let slot = slot_arg(args.first())?;
                self.request(Request::Run(slot))
            }
            (Command::Inkey, Want::Value) if args.len() <= 1 => {
                // TODO: without the flag, controller buttons also count as keys.
                if let Some(flag) = args.first() {
                    opt_int(flag)?;
                }
                let key = self.keys.pop_front();
                Ok(vec![Value::Str(new_str(key.into_iter().collect()))])
            }
            (Command::Locate, Want::Outs(0)) => {
                let (id, rest) = with_screen(args, 2)?;
                let (x, y) = (opt_int(&rest[0])?, opt_int(&rest[1])?);
                if self.text[id].locate(x, y) {
                    Ok(vec![])
                } else {
                    Err(ErrorKind::OutOfRange.into())
                }
            }
            (Command::Locate, Want::Outs(2)) => {
                let (id, _) = with_screen(args, 0)?;
                let (x, y) = self.text[id].cursor();
                Ok(vec![Value::Int(x as i32), Value::Int(y as i32)])
            }
            (
                Command::Chkfile
                | Command::Delete
                | Command::Files
                | Command::Load
                | Command::Project
                | Command::Rename
                | Command::Result
                | Command::Save,
                _,
            ) => self.file_command(command, args, want),
            (
                Command::Gbox
                | Command::Gcircle
                | Command::Gclip
                | Command::Gcls
                | Command::Gcolor
                | Command::Gcopy
                | Command::Gfill
                | Command::Gline
                | Command::Gload
                | Command::Gpaint
                | Command::Gpget
                | Command::Gpset
                | Command::Gputchr
                | Command::Gsample
                | Command::Gsave
                | Command::Gtarget
                | Command::Gtri,
                _,
            ) => self.grp_command(command, args, want),
            (
                Command::Animdef
                | Command::Spanim
                | Command::Spchk
                | Command::Spchr
                | Command::Spclr
                | Command::Spcol
                | Command::Spcolor
                | Command::Spcolvec
                | Command::Spdef
                | Command::Sphide
                | Command::Sphitinfo
                | Command::Sphitrc
                | Command::Sphitsp
                | Command::Sphome
                | Command::Splayer
                | Command::Splink
                | Command::Spofs
                | Command::Sppage
                | Command::Sprot
                | Command::Spscale
                | Command::Spset
                | Command::Spshow
                | Command::Spstart
                | Command::Spstop
                | Command::Spunlink
                | Command::Spused
                | Command::Spvar,
                _,
            ) => self.sp_command(command, args, want),
            _ => Err(illegal()),
        }
    }
}

impl Host for Runtime {
    fn lookup(&self, name: &str) -> Option<u32> {
        COMMANDS
            .iter()
            .position(|(n, _)| *n == name)
            .map(|i| i as u32)
    }

    fn call(&mut self, id: u32, args: &[Value], want: Want) -> Result<Vec<Value>> {
        let command = COMMANDS[id as usize].1;
        self.run(command, args, want)
    }

    fn print(&mut self, screen: Option<i32>, text: &[u16]) -> Result<()> {
        self.screen(screen)?.print(text);
        Ok(())
    }

    fn print_tab(&mut self, screen: Option<i32>) -> Result<()> {
        self.screen(screen)?.tab();
        Ok(())
    }

    fn print_newline(&mut self, screen: Option<i32>) -> Result<()> {
        self.screen(screen)?.newline();
        Ok(())
    }

    fn input_start(&mut self) -> Result<()> {
        self.line = Some(Vec::new());
        Ok(())
    }

    fn retry(&mut self) -> bool {
        std::mem::take(&mut self.retry)
    }

    // UNVERIFIED: only typing, Backspace and Enter edit the line; the
    // cursor keys do nothing.
    fn input_poll(&mut self) -> Result<Option<Vec<u16>>> {
        let Some(line) = self.line.as_mut() else {
            return Ok(None);
        };
        let console = &mut self.text[CONSOLE];
        while let Some(key) = self.keys.pop_front() {
            match key {
                keys::ENTER => {
                    console.newline();
                    return Ok(self.line.take());
                }
                keys::BACKSPACE => {
                    if line.pop().is_some() {
                        console.backspace();
                    }
                }
                0..0x20 | 0x7F => {}
                c => {
                    line.push(c);
                    console.print(&[c]);
                }
            }
        }
        Ok(None)
    }
}

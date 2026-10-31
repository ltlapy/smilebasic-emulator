//! The [`Host`] that programs run against: text screens now, graphics,
//! sprites and sound later.

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;

use sb_core::bytecode::{Callbacks, Compiled, Literal, Want};
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::host::{Exec, Host};
use sb_core::value::{Value, new_str};

use crate::complete::{self, Completion, Library, Options};
use crate::compose::{self, Compose, LAYERS, Layer};
use crate::confirm::{Ask, Confirm, unsaved_lines};
use crate::editor::split_lines;
use crate::editor::{Editor, Look, blend, frame_size, menu_height};
use crate::filter::Filter;
use crate::font::Font;
use crate::framebuffer::Framebuffer;
use crate::graphics::{Clip, Graphics};
use crate::hint;
use crate::input::Input;
use crate::line::LineEdit;
use crate::palette::Part;
use crate::screen::{Fade, ScreenMode};
pub use crate::settings::Settings;
use crate::sound::Sound;
use crate::sprites::{self, Sprites};
use crate::storage::Storage;
use crate::syntax::is_ident_char;
use crate::system::System;
use crate::text::{CONSOLE, FONT_PAGE, SCREENS, SPRITE_PAGE, TextScreen};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Accel,
    Acls,
    Animdef,
    Attr,
    Backcolor,
    Backtrace,
    Beep,
    Beeppan,
    Beeppit,
    Beepstop,
    Beepvol,
    Bgmchk,
    Bgmclear,
    Bgmcont,
    Bgmpause,
    Bgmpitch,
    Bgmplay,
    Bgmset,
    Bgmsetd,
    Bgmstop,
    Bgmvar,
    Bgmvol,
    Bgmwet,
    Brepeat,
    Button,
    Chkchr,
    Chkfile,
    Chkmml,
    Clear,
    Clipboard,
    Cls,
    Color,
    Cont,
    Controller,
    Date,
    Delete,
    Dialog,
    Dtread,
    Efcen,
    Efcset,
    Efcwet,
    Envfocus,
    Envinput,
    Envload,
    Envproject,
    Envsave,
    Envstat,
    Envtype,
    Fade,
    Fadechk,
    Files,
    Fontinfo,
    Garray,
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
    Gputchrp,
    Gsample,
    Gsave,
    Gtarget,
    Gtri,
    Gupdate,
    Gyroa,
    Gyrosync,
    Gyrov,
    Helpget,
    Helpinfo,
    Inkey,
    Inspect,
    Irread,
    Irsprite,
    Irstart,
    Irstate,
    Irstop,
    Key,
    Keyboard,
    Layer,
    Lclip,
    Lfilter,
    Lmatrix,
    Load,
    Loadg,
    Loadv,
    Locate,
    Maincnt,
    Mbutton,
    Metaedit,
    Metaload,
    Metasave,
    Millisec,
    Perfbegin,
    Perfend,
    Mouse,
    New,
    Pcmcont,
    Pcmpos,
    Pcmstop,
    Pcmstream,
    Pcmvol,
    Prgdel,
    Prgedit,
    Prgget,
    Prgins,
    Prgname,
    Prgseek,
    Prgset,
    Prgsize,
    Project,
    Pushkey,
    Randomize,
    Recchk,
    Recdata,
    Reclen,
    Recpos,
    Recsave,
    Recstart,
    Recstop,
    Rename,
    Result,
    Rnd,
    Rndf,
    Run,
    Save,
    Saveg,
    Savev,
    Scroll,
    Sndmsbal,
    Sndmvol,
    Sndstop,
    Spanim,
    Spchk,
    Spchr,
    Spclr,
    Spcol,
    Spcolor,
    Spcolvec,
    Spdef,
    Spfunc,
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
    Stick,
    Sysparam,
    Talk,
    Talkchk,
    Talkstop,
    Tanim,
    Tarray,
    Tblend,
    Tcbike,
    Tccar,
    Tcfishing,
    Tchk,
    Tchouse,
    Tcolor,
    Tcoord,
    Tcopy,
    Tcpiano,
    Tcplane,
    Tcrobot,
    Tcsubm,
    Tcvehicle,
    Tcvisor,
    Tfill,
    Tfunc,
    Thide,
    Thome,
    Time,
    Tlayer,
    Tload,
    Tmread,
    Tofs,
    Touch,
    Tpage,
    Tput,
    Trace,
    Trot,
    Tsave,
    Tscale,
    Tscreen,
    Tshow,
    Tstart,
    Tstop,
    Tupdate,
    Tvar,
    Vibrate,
    Wavset,
    Wavseta,
    Xctrlstyle,
    Xscreen,
    Xsubscreen,
    Uistate,
}

const COMMANDS: &[(&str, Command)] = &[
    ("ACCEL", Command::Accel),
    ("ACLS", Command::Acls),
    ("ANIMDEF", Command::Animdef),
    ("ATTR", Command::Attr),
    ("BACKCOLOR", Command::Backcolor),
    ("BACKTRACE", Command::Backtrace),
    ("BEEP", Command::Beep),
    ("BEEPPAN", Command::Beeppan),
    ("BEEPPIT", Command::Beeppit),
    ("BEEPSTOP", Command::Beepstop),
    ("BEEPVOL", Command::Beepvol),
    ("BGMCHK", Command::Bgmchk),
    ("BGMCLEAR", Command::Bgmclear),
    ("BGMCONT", Command::Bgmcont),
    ("BGMPAUSE", Command::Bgmpause),
    ("BGMPITCH", Command::Bgmpitch),
    ("BGMPLAY", Command::Bgmplay),
    ("BGMSET", Command::Bgmset),
    ("BGMSETD", Command::Bgmsetd),
    ("BGMSTOP", Command::Bgmstop),
    ("BGMVAR", Command::Bgmvar),
    ("BGMVOL", Command::Bgmvol),
    ("BGMWET", Command::Bgmwet),
    ("BREPEAT", Command::Brepeat),
    ("BUTTON", Command::Button),
    ("CHKCHR", Command::Chkchr),
    ("CHKFILE", Command::Chkfile),
    ("CHKMML", Command::Chkmml),
    ("CLEAR", Command::Clear),
    ("CLIPBOARD", Command::Clipboard),
    ("CLS", Command::Cls),
    ("COLOR", Command::Color),
    ("CONT", Command::Cont),
    ("CONTROLLER", Command::Controller),
    ("DATE$", Command::Date),
    ("DELETE", Command::Delete),
    ("DIALOG", Command::Dialog),
    ("DTREAD", Command::Dtread),
    ("EFCEN", Command::Efcen),
    ("EFCSET", Command::Efcset),
    ("EFCWET", Command::Efcwet),
    ("ENVFOCUS", Command::Envfocus),
    ("ENVINPUT$", Command::Envinput),
    ("ENVLOAD", Command::Envload),
    ("ENVPROJECT", Command::Envproject),
    ("ENVSAVE", Command::Envsave),
    ("ENVSTAT", Command::Envstat),
    ("ENVTYPE", Command::Envtype),
    ("FADE", Command::Fade),
    ("FADECHK", Command::Fadechk),
    ("FILES", Command::Files),
    ("FONTINFO", Command::Fontinfo),
    ("GARRAY", Command::Garray),
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
    ("GPUTCHRP", Command::Gputchrp),
    ("GSAMPLE", Command::Gsample),
    ("GSAVE", Command::Gsave),
    ("GTARGET", Command::Gtarget),
    ("GTRI", Command::Gtri),
    ("GUPDATE", Command::Gupdate),
    ("GYROA", Command::Gyroa),
    ("GYROSYNC", Command::Gyrosync),
    ("GYROV", Command::Gyrov),
    ("HELPGET", Command::Helpget),
    ("HELPINFO", Command::Helpinfo),
    ("INKEY$", Command::Inkey),
    ("INSPECT", Command::Inspect),
    ("IRREAD", Command::Irread),
    ("IRSPRITE", Command::Irsprite),
    ("IRSTART", Command::Irstart),
    ("IRSTATE", Command::Irstate),
    ("IRSTOP", Command::Irstop),
    ("KEY", Command::Key),
    ("KEYBOARD", Command::Keyboard),
    ("LAYER", Command::Layer),
    ("LCLIP", Command::Lclip),
    ("LFILTER", Command::Lfilter),
    ("LMATRIX", Command::Lmatrix),
    ("LOAD", Command::Load),
    ("LOADG", Command::Loadg),
    ("LOADV", Command::Loadv),
    ("LOCATE", Command::Locate),
    ("MAINCNT", Command::Maincnt),
    ("MBUTTON", Command::Mbutton),
    ("METAEDIT", Command::Metaedit),
    ("METALOAD", Command::Metaload),
    ("METASAVE", Command::Metasave),
    ("MILLISEC", Command::Millisec),
    ("PERFBEGIN", Command::Perfbegin),
    ("PERFEND", Command::Perfend),
    ("MOUSE", Command::Mouse),
    ("NEW", Command::New),
    ("PCMCONT", Command::Pcmcont),
    ("PCMPOS", Command::Pcmpos),
    ("PCMSTOP", Command::Pcmstop),
    ("PCMSTREAM", Command::Pcmstream),
    ("PCMVOL", Command::Pcmvol),
    ("PRGDEL", Command::Prgdel),
    ("PRGEDIT", Command::Prgedit),
    ("PRGGET$", Command::Prgget),
    ("PRGINS", Command::Prgins),
    ("PRGNAME$", Command::Prgname),
    ("PRGSEEK", Command::Prgseek),
    ("PRGSET", Command::Prgset),
    ("PRGSIZE", Command::Prgsize),
    ("PROJECT", Command::Project),
    ("PUSHKEY", Command::Pushkey),
    ("RANDOMIZE", Command::Randomize),
    ("RECCHK", Command::Recchk),
    ("RECDATA", Command::Recdata),
    ("RECLEN", Command::Reclen),
    ("RECPOS", Command::Recpos),
    ("RECSAVE", Command::Recsave),
    ("RECSTART", Command::Recstart),
    ("RECSTOP", Command::Recstop),
    ("RENAME", Command::Rename),
    ("RESULT", Command::Result),
    ("RND", Command::Rnd),
    ("RNDF", Command::Rndf),
    ("RUN", Command::Run),
    ("SAVE", Command::Save),
    ("SAVEG", Command::Saveg),
    ("SAVEV", Command::Savev),
    ("SCROLL", Command::Scroll),
    ("SNDMSBAL", Command::Sndmsbal),
    ("SNDMVOL", Command::Sndmvol),
    ("SNDSTOP", Command::Sndstop),
    ("SPANIM", Command::Spanim),
    ("SPCHK", Command::Spchk),
    ("SPCHR", Command::Spchr),
    ("SPCLR", Command::Spclr),
    ("SPCOL", Command::Spcol),
    ("SPCOLOR", Command::Spcolor),
    ("SPCOLVEC", Command::Spcolvec),
    ("SPDEF", Command::Spdef),
    ("SPFUNC", Command::Spfunc),
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
    ("STICK", Command::Stick),
    ("SYSPARAM", Command::Sysparam),
    ("TALK", Command::Talk),
    ("TALKCHK", Command::Talkchk),
    ("TALKSTOP", Command::Talkstop),
    ("TANIM", Command::Tanim),
    ("TARRAY", Command::Tarray),
    ("TBLEND", Command::Tblend),
    ("TCBIKE", Command::Tcbike),
    ("TCCAR", Command::Tccar),
    ("TCFISHING", Command::Tcfishing),
    ("TCHK", Command::Tchk),
    ("TCHOUSE", Command::Tchouse),
    ("TCOLOR", Command::Tcolor),
    ("TCOORD", Command::Tcoord),
    ("TCOPY", Command::Tcopy),
    ("TCPIANO", Command::Tcpiano),
    ("TCPLANE", Command::Tcplane),
    ("TCROBOT", Command::Tcrobot),
    ("TCSUBM", Command::Tcsubm),
    ("TCVEHICLE", Command::Tcvehicle),
    ("TCVISOR", Command::Tcvisor),
    ("TFILL", Command::Tfill),
    ("TFUNC", Command::Tfunc),
    ("THIDE", Command::Thide),
    ("THOME", Command::Thome),
    ("TIME$", Command::Time),
    ("TLAYER", Command::Tlayer),
    ("TLOAD", Command::Tload),
    ("TMREAD", Command::Tmread),
    ("TOFS", Command::Tofs),
    ("TOUCH", Command::Touch),
    ("TPAGE", Command::Tpage),
    ("TPUT", Command::Tput),
    ("TRACE", Command::Trace),
    ("TROT", Command::Trot),
    ("TSAVE", Command::Tsave),
    ("TSCALE", Command::Tscale),
    ("TSCREEN", Command::Tscreen),
    ("TSHOW", Command::Tshow),
    ("TSTART", Command::Tstart),
    ("TSTOP", Command::Tstop),
    ("TUPDATE", Command::Tupdate),
    ("TVAR", Command::Tvar),
    ("VIBRATE", Command::Vibrate),
    ("WAVSET", Command::Wavset),
    ("WAVSETA", Command::Wavseta),
    ("XCTRLSTYLE", Command::Xctrlstyle),
    ("XSCREEN", Command::Xscreen),
    ("XSUBSCREEN", Command::Xsubscreen),
    ("UISTATE", Command::Uistate),
];

/// Program slots PRG0 to PRG5.
pub const SLOTS: usize = 6;

/// Work for the [`crate::machine::Machine`] around the runtime, asked for
/// by direct-mode commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Run(usize),
    Clear,
    Cont,
    Trace,
}

/// How the last program stopped, if it has not ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stopped {
    #[default]
    No,
    /// By STOP or the + button, so that CONT can go on.
    Paused,
    /// By an error.
    Error,
}

/// UNVERIFIED: how many unread keys SmileBASIC keeps.
pub const KEY_BUFFER: usize = 256;
/// Frames per half period of the INPUT cursor's blink: at 60 fps, a full
/// blink takes about 833 ms.
const CURSOR_BLINK: u32 = 25;
/// The pixels between the sides of a split editor, and their color.
const SPLIT_DIVIDER: u32 = 2;
const SPLIT_COLOR: u32 = 0xFF30_3040;
/// The frame drawn around the side of a split editor that gets the keys,
/// when it changes: shown whole for `SPLIT_FLASH_HOLD` frames, then
/// fading out over `SPLIT_FLASH_FADE`.
const SPLIT_FLASH: u32 = 0xFF58_A0FF;
const SPLIT_FLASH_HOLD: u32 = 12;
const SPLIT_FLASH_FADE: u32 = 30;

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

/// A direct-mode line that Up and Down walk through the history from.
struct Recall {
    /// The history entry shown, newest first; `None` is the line that was
    /// being typed.
    at: Option<usize>,
    /// The line that was being typed, which Down brings back.
    draft: Vec<u16>,
}

pub struct Runtime {
    pub text: Vec<TextScreen>,
    pub(crate) font: Font,
    fb: Framebuffer,
    /// Graphic pages GRP0-GRP5.
    pub grp: Graphics,
    pub sprites: Sprites,
    pub input: Input,
    pub(crate) system: System,
    pub sound: Sound,
    /// Typed characters not yet read by INKEY$ or INPUT.
    pub(crate) keys: VecDeque<u16>,
    /// The line being typed for INPUT, LINPUT or direct mode.
    line: Option<LineEdit>,
    /// Where the Up and Down keys have taken the direct-mode line in the
    /// history, if it is being typed in direct mode and they have.
    recall: Option<Recall>,
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
    /// The slot whose program is running; `None` in direct mode.
    pub(crate) running_slot: Option<usize>,
    /// How the last program stopped, kept up to date by the machine.
    pub(crate) stopped: Stopped,
    /// Where it stopped, innermost first, as slots and lines: the line
    /// itself, then each DEF call and GOSUB it was in.
    pub(crate) backtrace: Vec<(usize, u32)>,
    /// The slot and line that PRGEDIT chose, for PRGGET$ and the rest.
    pub(crate) prg_edit: Option<crate::prg::PrgEdit>,
    /// The slot the editor shows, or last showed.
    pub(crate) edit_slot: usize,
    /// Direct-mode lines typed, newest first, for ENVINPUT$.
    pub(crate) history: crate::env::History,
    /// What CLIPBOARD reads and writes.
    pub clipboard: Box<dyn crate::env::Clipboard>,
    /// The DIALOG the program waits on, and the frontend's answer.
    pub(crate) dialog: Option<crate::dialog::Waiting>,
    pub(crate) dialog_answer: Option<crate::dialog::DialogAnswer>,
    /// Lines of a FILES listing still to show, after a key is pressed.
    pub(crate) listing_left: Option<VecDeque<Vec<u16>>>,
    /// Lines of an INSPECT still to show, after a key is pressed.
    pub(crate) inspect_left: Option<VecDeque<Vec<u16>>>,
    /// The project metadata that METAEDIT edits.
    pub(crate) meta: crate::env::Meta,
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
    /// Each slot's program once the run has gone on in it, for DATA
    /// named `"n:@LABEL"`.
    pub(crate) slot_programs: [Option<Rc<Compiled>>; SLOTS],
    /// READ's place in the program's DATA, during a command.
    data_pos: Cell<usize>,
    pub layers: [Layer; LAYERS],
    /// LFILTER of each layer.
    pub filters: [crate::filter::Filter; LAYERS],
    /// BACKCOLOR: what shows behind every layer.
    pub back_color: u32,
    pub(crate) fade: Fade,
    /// Where a layer that is composed is drawn first.
    scratch: Framebuffer,
    /// A layer with LMATRIX's 4x4 form, drawn flat before it is put
    /// through the matrix.
    flat: Framebuffer,
    /// XSCREEN, and what ACLS goes back to.
    pub(crate) mode: ScreenMode,
    startup_mode: ScreenMode,
    /// The editor draws at a multiple of the start-up resolution, for its
    /// font and size (see [`frame_size`]).
    editor_fb: Framebuffer,
    /// SmileBASIC's words with their forms from the help, for completion
    /// and argument hints.
    pub library: Rc<Library>,
    /// SmileBASIC 4's help, for HELPGET and HELPINFO.
    pub help: Option<Rc<crate::help::Help>>,
    /// The editor's line and cursor column when the program started,
    /// where HELPINFO looks for a command.
    pub(crate) help_cursor: (Vec<u16>, usize),
    /// Ctrl+Space hid completion's list and the hints in direct mode.
    line_quiet: bool,
    /// One side of a split editor, drawn before going into `editor_fb`.
    side_fb: Framebuffer,
    /// Where the second side of a split editor starts on its frame: a
    /// column, or a row when the sides are one above the other.
    pub(crate) split_at: Option<u32>,
    /// The last split drawn, as whether its sides were one above the
    /// other and which side had the keys, and the frame that side's
    /// frame started to show.
    split_shown: Option<(bool, usize)>,
    split_flash: u32,
    /// GRP4 at start-up, which ACLS puts back.
    sprite_page: Option<Vec<u32>>,
}

/// The text screens at start-up: the console in front of everything else
/// with 8-pixel characters, drawn from the 16-pixel font where the frame
/// is fine enough, and the others with 16-pixel characters.
// UNVERIFIED: from the manual's program equivalent to ACLS, which sets
// TSCREEN #TCONSOLE,16,8 and TSCREEN I,16,16 at a sample magnification
// of 2.
fn startup_screens(mode: &ScreenMode) -> Vec<TextScreen> {
    let (w, h) = (mode.width, mode.height);
    (0..SCREENS)
        .map(|i| {
            let mut t = TextScreen::new(w, h);
            if i == CONSOLE {
                t.z = -4095;
                if mode.sample >= 2 {
                    t.set_font(16, 8, w / 8, h / 8);
                }
            } else {
                t.set_font(16, 16, w / 16, h / 16);
            }
            t
        })
        .collect()
}

/// Something drawn on the frame.
#[derive(Clone, Copy)]
enum Element {
    Text(usize),
    Sprite(usize, sprites::Pose),
}

/// Reads DATA values in order, moving READ's place along; see
/// [`Runtime::label_data`].
pub(crate) struct DataReader<'a> {
    program: Rc<Compiled>,
    pos: &'a Cell<usize>,
}

impl DataReader<'_> {
    pub(crate) fn number(&mut self) -> Result<f64> {
        let item = self
            .program
            .data
            .get(self.pos.get())
            .ok_or(ErrorKind::OutOfData)?;
        self.pos.set(self.pos.get() + 1);
        match item {
            Literal::Int(n) => Ok(f64::from(*n)),
            Literal::Real(x) => Ok(*x),
            Literal::Str(_) => Err(ErrorKind::TypeMismatch.into()),
        }
    }

    /// Whether the program's DATA has all been read.
    pub(crate) fn at_end(&self) -> bool {
        self.pos.get() >= self.program.data.len()
    }

    /// The next value, of whatever type.
    pub(crate) fn literal(&mut self) -> Result<Literal> {
        let item = self
            .program
            .data
            .get(self.pos.get())
            .ok_or(ErrorKind::OutOfData)?;
        self.pos.set(self.pos.get() + 1);
        Ok(item.clone())
    }

    pub(crate) fn numbers(&mut self, n: usize) -> Result<Vec<f64>> {
        (0..n).map(|_| self.number()).collect()
    }
}

pub(crate) fn illegal() -> Error {
    ErrorKind::IllegalFunctionCall.into()
}

/// No form of a command takes the arguments given: SmileBASIC 4 says
/// Wrong number of arguments (checked on hardware for LOCATE and TOFS).
// UNVERIFIED: the same for a command used as a function or the other way
// round.
pub(crate) fn no_form() -> Error {
    ErrorKind::WrongNumberOfArguments.into()
}

fn is_no_form(e: &Error) -> bool {
    e.kind == ErrorKind::WrongNumberOfArguments && e.detail.is_none()
}

pub(crate) fn screen_id(v: &Value) -> Result<usize> {
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
pub(crate) fn with_screen(args: &[Value], rest: usize) -> Result<(usize, &[Value])> {
    match args.len() {
        n if n == rest => Ok((CONSOLE, args)),
        n if n == rest + 1 => Ok((screen_id(&args[0])?, &args[1..])),
        _ => Err(no_form()),
    }
}

impl Runtime {
    pub fn new(width: u32, height: u32, font: Font) -> Runtime {
        let mode = ScreenMode::new(width, height);
        Runtime {
            text: startup_screens(&mode),
            font,
            fb: Framebuffer::new(width, height),
            grp: Graphics::default(),
            sprites: Sprites::default(),
            input: Input::default(),
            system: System::default(),
            sound: Sound::default(),
            keys: VecDeque::new(),
            line: None,
            recall: None,
            frame: 0,
            direct: false,
            requests: Vec::new(),
            slots: vec![String::new(); SLOTS],
            slot_names: vec![String::new(); SLOTS],
            slot_modified: vec![false; SLOTS],
            running_slot: None,
            stopped: Stopped::No,
            backtrace: Vec::new(),
            prg_edit: None,
            edit_slot: 0,
            history: Default::default(),
            clipboard: Box::new(crate::env::MemoryClipboard::default()),
            meta: Default::default(),
            dialog: None,
            dialog_answer: None,
            settings: Settings::default(),
            storage: Storage::none(),
            result: 1,
            confirm: None,
            answer: None,
            retry: false,
            listing_left: None,
            inspect_left: None,
            notices: Vec::new(),
            program: None,
            slot_programs: Default::default(),
            data_pos: Cell::new(0),
            layers: [Layer::default(); LAYERS],
            filters: Default::default(),
            back_color: 0xFF00_0000,
            fade: Fade::default(),
            scratch: Framebuffer::new(width, height),
            flat: Framebuffer::new(width, height),
            mode,
            startup_mode: mode,
            editor_fb: Framebuffer::new(width, height),
            side_fb: Framebuffer::new(0, 0),
            library: Rc::default(),
            help: None,
            help_cursor: Default::default(),
            line_quiet: false,
            split_at: None,
            split_shown: None,
            split_flash: 0,
            sprite_page: None,
        }
        .with_font_page()
    }

    pub fn screen_mode(&self) -> ScreenMode {
        self.mode
    }

    pub fn startup_mode(&self) -> ScreenMode {
        self.startup_mode
    }

    /// The resolution to start with and to go back to on ACLS, which
    /// resets the screens for it as ACLS does. The editor uses its size.
    pub fn set_startup_mode(&mut self, mode: ScreenMode) {
        self.startup_mode = mode;
        self.editor_fb = Framebuffer::new(mode.width, mode.height);
        self.text = startup_screens(&mode);
        self.set_mode(mode);
    }

    /// The sprite image on GRP4, also put back by ACLS.
    pub fn set_sprite_page(&mut self, pixels: Vec<u32>) {
        self.grp.set_page(SPRITE_PAGE, pixels.clone());
        self.sprite_page = Some(pixels);
    }

    /// XSCREEN: the console is resized to fill the screen, keeping its
    /// font, and `#GSPRITE` shows as much of GRP0.
    // UNVERIFIED: whether the console is cleared, and how much of
    // #GSPRITE's settings are reset.
    pub(crate) fn set_mode(&mut self, mode: ScreenMode) {
        self.mode = mode;
        let (fw, fh) = mode.frame();
        self.fb = Framebuffer::new(fw, fh);
        self.scratch = Framebuffer::new(fw, fh);
        self.flat = Framebuffer::new(fw, fh);
        let console = &mut self.text[CONSOLE];
        let (ft, cs) = console.font();
        console.set_font(ft, cs, mode.width / cs, mode.height / cs);
        self.sprites.reset_gsprite((mode.width, mode.height));
    }

    /// ACLS: the start-up state of the screens, sprites, layers and
    /// graphic pages, keeping what the flags ask for.
    // Checked on hardware: SPPAGE, SPLAYER, GSAMPLE, TOFS, COLOR, ATTR and
    // TCOLOR go back too, though the manual's equivalent program leaves
    // them out.
    pub(crate) fn acls(
        &mut self,
        keep_grp: bool,
        keep_defs: bool,
        keep_font: bool,
        keep_anims: bool,
    ) {
        self.text = startup_screens(&self.startup_mode);
        self.set_mode(self.startup_mode);
        self.back_color = 0xFF00_0000;
        // UNVERIFIED: ACLS clears FADE.
        self.fade = Fade::default();
        self.layers = [Layer::default(); LAYERS];
        // UNVERIFIED: ACLS clears LFILTER.
        self.filters = Default::default();
        self.sprites.reset(keep_defs, keep_anims);
        if !keep_grp {
            for page in 0..FONT_PAGE {
                self.grp.clear_page(page);
            }
            if let Some(pixels) = &self.sprite_page {
                self.grp.set_page(SPRITE_PAGE, pixels.clone());
            }
        }
        if !keep_font {
            self.grp.clear_page(FONT_PAGE);
            let page = self.font.page_image();
            if page.iter().any(|&p| p != 0) {
                self.grp.set_page(FONT_PAGE, page);
            }
        }
        self.grp.reset_state();
    }

    /// The DATA values from `label` on, for commands that take a
    /// `"@label"`. As on hardware, they move READ's place: RESTORE to
    /// the label, then READ.
    /// `"n:@LABEL"` reads slot n's DATA, once the run has gone on there.
    pub(crate) fn label_data(&self, label: &Value) -> Result<DataReader<'_>> {
        let text =
            String::from_utf16_lossy(&label.clone().into_str()?.borrow()).to_ascii_uppercase();
        let (slot, name) = match text.split_once(':') {
            Some((slot, rest)) => (slot.trim().parse::<usize>().ok(), rest),
            None => (None, text.as_str()),
        };
        let name = name.strip_prefix('@').unwrap_or(name);
        let undefined = || Error::with_detail(ErrorKind::UndefinedLabel, format!("@{name}"));
        let program = match slot {
            Some(slot) => {
                (self.slot_programs.get(slot).cloned().flatten()).ok_or_else(undefined)?
            }
            None => self.program.clone().unwrap_or_default(),
        };
        let pos = *program.data_labels.get(name).ok_or_else(undefined)?;
        self.data_pos.set(pos as usize);
        Ok(DataReader {
            program,
            pos: &self.data_pos,
        })
    }

    /// Copies the font into GRP5, where programs can change it.
    fn with_font_page(mut self) -> Runtime {
        let page = self.font.page_image();
        if page.iter().any(|&p| p != 0) {
            self.grp.set_page(crate::text::FONT_PAGE, page);
        }
        self
    }

    /// The screen's size in pixels, before XSCREEN's sampling.
    pub(crate) fn frame_size(&self) -> (u32, u32) {
        (self.mode.width, self.mode.height)
    }

    fn draw_element(&self, fb: &mut Framebuffer, e: Element, layer: &Layer, cursor: bool) {
        let clip = layer.clip.as_ref();
        let projection = layer.projection.as_ref().map(|m| (m, self.mode.sample));
        match e {
            Element::Text(i) => self.text[i].draw_placed(
                fb,
                &self.font,
                &self.grp,
                &layer.matrix,
                clip,
                cursor && i == CONSOLE,
            ),
            Element::Sprite(i, pose) => sprites::draw_posed(
                &self.sprites.list[i],
                &pose,
                &self.grp,
                fb,
                &layer.matrix,
                clip,
                projection,
            ),
        }
    }

    pub fn font(&self) -> &Font {
        &self.font
    }

    /// The line being typed for INPUT or in direct mode, if any.
    pub(crate) fn typed_line(&self) -> Option<Vec<u16>> {
        Some(self.line.as_ref()?.text.clone())
    }

    /// The text selected in the line being typed.
    pub fn line_selection(&self) -> Option<String> {
        let text = self.line.as_ref()?.selected_text()?;
        Some(String::from_utf16_lossy(&text))
    }

    /// Ctrl+Space in direct mode: hides completion's list and the hints,
    /// or shows them again with the list for the word at the cursor.
    pub fn toggle_line_popups(&mut self) {
        let Some(options) = Options::from_settings(&self.settings, false) else {
            return;
        };
        self.line_quiet = !self.line_quiet;
        if let Some(line) = &mut self.line {
            line.completion = None;
        }
        if !self.line_quiet && self.recall.is_some() {
            self.line_words(Options {
                after: 1,
                ..options
            });
        }
    }

    /// Completion's list for the word before the cursor of the line typed
    /// in direct mode.
    fn line_words(&mut self, options: Options) {
        // The edited slot's names, also for labels after GOTO.
        let lines = split_lines(&self.slots[self.edit_slot]);
        let names = complete::names(lines.iter().map(Vec::as_slice));
        if let Some(line) = &mut self.line
            && line.selection().is_none()
        {
            line.completion =
                Completion::find(&line.text, line.cursor, &options, &self.library, &names);
            line.draw(&mut self.text[CONSOLE]);
        }
    }

    /// Whether INPUT, LINPUT or direct mode waits for a line.
    pub fn wants_line(&self) -> bool {
        self.line.is_some()
    }

    /// Stops waiting for an INPUT line, if one is being typed.
    pub fn cancel_input(&mut self) {
        self.line = None;
        self.recall = None;
    }

    /// Moves through the history from the line being typed in direct mode:
    /// `older` for Up, else Down. Whatever was typed before the first Up
    /// comes back with the last Down; edits made on a recalled line are
    /// lost when the line changes.
    fn recall_line(&mut self, older: bool) {
        let (Some(recall), Some(line)) = (self.recall.as_mut(), self.line.as_mut()) else {
            return;
        };
        let at = match (recall.at, older) {
            (None, true) if self.history.is_empty() => return,
            (None, true) => {
                recall.draft = line.text.clone();
                Some(0)
            }
            (None, false) => return,
            (Some(i), true) => Some((i + 1).min(self.history.len() - 1)),
            (Some(0), false) => None,
            (Some(i), false) => Some(i - 1),
        };
        recall.at = at;
        let text = match at {
            Some(i) => self.history[i].clone(),
            None => recall.draft.clone(),
        };
        line.set_text(text);
        line.draw(&mut self.text[CONSOLE]);
    }

    /// Waits for a line typed in direct mode, where Up and Down recall the
    /// lines typed before.
    pub(crate) fn direct_input_start(&mut self) {
        self.line = Some(LineEdit::new(&self.text[CONSOLE]));
        self.recall = Some(Recall {
            at: None,
            draft: Vec::new(),
        });
    }

    /// Direct-mode requests made since the last call.
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    pub(crate) fn direct_only(&self) -> Result<()> {
        if self.direct {
            Ok(())
        } else {
            // Checked on hardware: CLEAR in a program is a Syntax error
            // when it runs. UNVERIFIED: that NEW, RUN and PROJECT are too.
            Err(Error::with_detail(
                ErrorKind::SyntaxError,
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

    /// Drops the keys a program left unread and what its buttons were
    /// doing, so that none of it reaches direct mode or the next run.
    pub(crate) fn clear_input(&mut self) {
        self.keys.clear();
        self.input.forget();
    }

    /// Takes the next unread key, for the editor.
    pub(crate) fn next_key(&mut self) -> Option<u16> {
        self.keys.pop_front()
    }

    /// Composes the current frame: layers 7 to 0, each back to front.
    pub fn render(&mut self) -> &Framebuffer {
        self.frame = self.frame.wrapping_add(1);
        let cursor = self.line.is_some() && (self.frame / CURSOR_BLINK).is_multiple_of(2);
        // Within a layer, larger Z is further back; at equal Z text screens
        // go behind sprites, and among either, higher ids go behind.
        let mut elements: Vec<(i32, i32, u8, usize, Element)> = (self.text.iter().enumerate())
            .filter(|(_, t)| t.visible)
            .map(|(i, t)| (t.layer, t.z, 0, i, Element::Text(i)))
            .collect();
        for (i, pose) in self.sprites.draw_order() {
            elements.push((pose.layer, pose.z, 1, i, Element::Sprite(i, pose)));
        }
        elements.sort_by_key(|&(layer, z, kind, id, _)| {
            (
                std::cmp::Reverse(layer),
                std::cmp::Reverse(z),
                kind,
                std::cmp::Reverse(id),
            )
        });
        let mut fb = std::mem::replace(&mut self.fb, Framebuffer::new(0, 0));
        let mut scratch = std::mem::replace(&mut self.scratch, Framebuffer::new(0, 0));
        let mut flat = std::mem::replace(&mut self.flat, Framebuffer::new(0, 0));
        fb.clear(self.back_color | 0xFF00_0000);
        for layer in (0..LAYERS as i32).rev() {
            let mut in_layer = elements.iter().filter(|e| e.0 == layer).peekable();
            let settings = self.fine(&self.layers[layer as usize]);
            let filter = &self.filters[layer as usize];
            let direct = settings.compose == Compose::Direct;
            // A layer that is not composed filters the whole frame as drawn
            // so far, the layers behind and BACKCOLOR included (checked on
            // hardware); a composed one only its own elements.
            // UNVERIFIED: that it does so with no elements in the layer.
            if in_layer.peek().is_none() && !(direct && !matches!(filter, Filter::None)) {
                continue;
            }
            let target = match direct {
                true => &mut fb,
                false => {
                    scratch.clear(0);
                    &mut scratch
                }
            };
            match &settings.projection {
                None => {
                    for &(_, _, _, _, element) in in_layer {
                        self.draw_element(target, element, &settings, cursor);
                    }
                }
                // Sprites go through the matrix themselves, whole; text
                // screens are drawn flat on the screen first, each run of
                // them put on in its turn.
                Some(m) => {
                    let unclipped = Layer {
                        clip: None,
                        projection: None,
                        ..settings
                    };
                    let clip = settings.clip.as_ref();
                    let mut flat_drawn = false;
                    for &(_, _, _, _, element) in in_layer {
                        if let Element::Sprite(..) = element {
                            if flat_drawn {
                                compose::project(target, &flat, m, self.mode.sample, clip);
                                flat_drawn = false;
                            }
                            self.draw_element(target, element, &settings, cursor);
                            continue;
                        }
                        if !flat_drawn {
                            flat.clear(0);
                            flat_drawn = true;
                        }
                        self.draw_element(&mut flat, element, &unclipped, cursor);
                    }
                    if flat_drawn {
                        compose::project(target, &flat, m, self.mode.sample, clip);
                    }
                }
            }
            filter.apply(target, self.mode.sample);
            // The frame stays opaque where deformation left nothing, which
            // shows black.
            if direct && matches!(filter, Filter::Raster { .. }) {
                for p in target.pixels_mut() {
                    *p |= 0xFF00_0000;
                }
            }
            if !direct {
                compose::compose(&mut fb, &scratch, settings.compose, settings.color);
            }
        }
        self.fade.cover(fb.pixels_mut());
        // In direct mode, the hint for the command being typed above the
        // line and completion's list below the word.
        let options = Options::from_settings(&self.settings, false).filter(|_| !self.line_quiet);
        if let (Some(line), Some(_), Some(options)) = (&self.line, &self.recall, options) {
            let console = &self.text[CONSOLE];
            let (w, _) = console.size();
            let (_, size) = console.font();
            let sample = i64::from(self.mode.sample);
            let scale = sample * i64::from(size);
            let (ox, oy) = console.pos;
            let at = |(x, y): (i64, i64)| {
                (
                    ox as i64 * sample + x * scale,
                    oy as i64 * sample + y * scale,
                )
            };
            let hint = (options.hints)
                .then(|| hint::find(&line.text, line.cursor, &self.library, &[]))
                .flatten();
            if let Some(cell) = line.cell_of(line.cursor, w) {
                let (_, top) = at(cell);
                let (left, screen_top) = at((0, 0));
                if options.bottom {
                    let band = hint::Band {
                        left,
                        top: screen_top,
                        bottom: i64::from(fb.height()),
                        cursor: top,
                    };
                    let syntax = line.completion.as_ref().and_then(Completion::syntax);
                    hint::draw_band(
                        &mut fb,
                        &self.font,
                        band,
                        (hint.as_ref(), syntax),
                        scale as u32,
                    );
                } else if let Some(hint) = &hint {
                    hint.draw(&mut fb, &self.font, left, (top, top + scale), scale as u32);
                }
            }
            if let Some(list) = &line.completion
                && let Some(cell) = line.cell_of(list.start, w)
            {
                let (left, top) = at(cell);
                let colors = &self.settings.editor_colors;
                list.draw(
                    &mut fb,
                    &self.font,
                    (left, top + scale),
                    top,
                    scale as u32,
                    colors,
                );
            }
        }
        self.fb = fb;
        self.scratch = scratch;
        self.flat = flat;
        &self.fb
    }

    /// Starts a frame: the input it sees, and MAINCNT.
    pub fn begin_frame(&mut self) {
        self.input.latch();
        self.system.frames = self.system.frames.wrapping_add(1);
    }

    /// Plays one frame of the sprites' and text screens' animations.
    pub fn animate(&mut self) {
        // UNVERIFIED: a fade steps with the animations, starting on the
        // frame after FADE.
        self.fade.step();
        self.sprites.animate();
        for t in &mut self.text {
            t.animate();
        }
    }

    /// Clears SPVAR's and TVAR's variables, as starting a program does.
    pub fn clear_vars(&mut self) {
        self.sprites.clear_vars();
        for t in &mut self.text {
            t.vars.clear();
        }
    }

    /// A layer's settings for a frame that XSCREEN's sampling makes finer.
    fn fine(&self, layer: &Layer) -> Layer {
        let s = self.mode.sample as i32;
        if s == 1 {
            return *layer;
        }
        let f = f64::from(s);
        let scale = compose::Affine {
            a: f,
            d: f,
            ..compose::Affine::IDENTITY
        };
        Layer {
            matrix: layer.matrix.then(&scale),
            clip: layer.clip.map(|c| Clip {
                x0: c.x0 * s,
                y0: c.y0 * s,
                x1: c.x1 * s + s - 1,
                y1: c.y1 * s + s - 1,
            }),
            ..*layer
        }
    }

    /// Composes a frame showing the editor instead of the screens.
    /// `split` is `None` for one side, else whether the sides are one
    /// above the other.
    pub(crate) fn render_editors(
        &mut self,
        editors: &mut [Editor; 2],
        active: usize,
        split: Option<bool>,
    ) -> &Framebuffer {
        self.frame = self.frame.wrapping_add(1);
        let blink = (self.frame / CURSOR_BLINK).is_multiple_of(2);
        let s = &self.settings;
        let mode = self.startup_mode;
        let (w, h) = frame_size((mode.width, mode.height), s.editor_font, s.editor_size);
        if (self.editor_fb.width(), self.editor_fb.height()) != (w, h) {
            self.editor_fb = Framebuffer::new(w, h);
        }
        let look = Look {
            wrap: s.editor_wrap,
            tiles: s.editor_number_tiles,
            map: s.editor_map,
            top: None,
            font: s.editor_font,
            colors: &s.editor_colors,
        };
        let name = |editor: &Editor| {
            let slot = editor.slot();
            let mut name = self.slot_names[slot].clone();
            // The slot has edits that were not saved.
            if self.slot_modified[slot] || editor.is_modified() {
                name.push_str(" *");
            }
            name
        };
        let Some(rows) = split else {
            self.split_at = None;
            self.split_shown = None;
            let name = name(&editors[active]);
            editors[active].draw(&mut self.editor_fb, &self.font, &name, look, blink);
            return &self.editor_fb;
        };
        // Two sides with a divider between them; only the upper one
        // leaves room for the menu bar.
        let length = if rows { h } else { w };
        let half = (length - SPLIT_DIVIDER) / 2;
        let second = length - half;
        self.split_at = Some(second);
        if self.split_shown != Some((rows, active)) {
            self.split_shown = Some((rows, active));
            self.split_flash = self.frame;
        }
        let side_size = if rows { (w, half) } else { (half, h) };
        if (self.side_fb.width(), self.side_fb.height()) != side_size {
            self.side_fb = Framebuffer::new(side_size.0, side_size.1);
        }
        for (side, editor) in editors.iter_mut().enumerate() {
            let name = name(editor);
            let blink = blink && side == active;
            let top = match (rows, side) {
                (true, 0) => Some(menu_height(h)),
                (true, _) => Some(0),
                (false, _) => None,
            };
            let look = Look { top, ..look };
            editor.draw(&mut self.side_fb, &self.font, &name, look, blink);
            if side != active {
                editor.dim_margins(&mut self.side_fb, s.editor_colors[Part::Background]);
            }
            let at = if side == 0 { 0 } else { second };
            let (x0, y0) = if rows { (0, at) } else { (at, 0) };
            for y in 0..side_size.1 {
                for x in 0..side_size.0 {
                    let pixel = self.side_fb.get(x, y).unwrap_or(0);
                    self.editor_fb.set(x0 + x, y0 + y, pixel);
                }
            }
        }
        for i in half..second {
            for j in 0..if rows { w } else { h } {
                let (x, y) = if rows { (j, i) } else { (i, j) };
                self.editor_fb.set(x, y, SPLIT_COLOR);
            }
        }
        let age = self.frame.wrapping_sub(self.split_flash);
        let alpha = match age.checked_sub(SPLIT_FLASH_HOLD) {
            None => 256,
            Some(fading) => 256 * SPLIT_FLASH_FADE.saturating_sub(fading) / SPLIT_FLASH_FADE,
        };
        if alpha > 0 {
            // Below the menu bar, which only the upper side leaves room for.
            let at = if active == 0 { 0 } else { second };
            let (x0, y0) = if rows { (0, at) } else { (at, 0) };
            let top = if rows && active == 1 {
                y0
            } else {
                menu_height(h)
            };
            let (x1, y1) = (x0 + side_size.0, y0 + side_size.1);
            let thick = (s.editor_font as u32 / 4).max(1);
            for y in top..y1 {
                for x in x0..x1 {
                    let edge =
                        x < x0 + thick || x >= x1 - thick || y < top + thick || y >= y1 - thick;
                    if let (true, Some(c)) = (edge, self.editor_fb.get(x, y)) {
                        self.editor_fb.set(x, y, blend(c, SPLIT_FLASH, alpha));
                    }
                }
            }
        }
        &self.editor_fb
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
                    self.drop_prg_edit(slot);
                    self.slots[slot].clear();
                    self.slot_names[slot].clear();
                    self.slot_modified[slot] = false;
                }
                Ok(vec![])
            }
            // CONT: direct mode only (UNVERIFIED: with the same Syntax
            // error as CLEAR in a program).
            (Command::Cont, Want::Outs(0)) if args.is_empty() => {
                self.direct_only()?;
                match self.stopped {
                    Stopped::Paused => self.request(Request::Cont),
                    _ => Err(ErrorKind::CantContinue.into()),
                }
            }
            // Unlike SmileBASIC 4, the program can go on afterwards. To
            // stop it as SmileBASIC 4 does, bring back the lines below.
            (Command::Backtrace, Want::Outs(0)) if args.is_empty() => {
                self.direct_only()?;
                for (slot, line) in self.backtrace.clone() {
                    let text: Vec<u16> = format!("{slot}:{line:6}").encode_utf16().collect();
                    self.text[CONSOLE].print(&text);
                    self.text[CONSOLE].newline();
                }
                // if self.stopped == Stopped::Paused {
                //     self.stopped = Stopped::Error;
                // }
                Ok(vec![])
            }
            (Command::Trace, Want::Outs(0)) if args.is_empty() => {
                self.direct_only()?;
                match self.stopped {
                    Stopped::Paused => self.request(Request::Trace),
                    _ => Err(ErrorKind::CantTrace.into()),
                }
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
                Command::Prgdel
                | Command::Prgedit
                | Command::Prgget
                | Command::Prgins
                | Command::Prgname
                | Command::Prgseek
                | Command::Prgset
                | Command::Prgsize,
                _,
            ) => self.prg_command(command, args, want),
            (
                Command::Clipboard
                | Command::Envfocus
                | Command::Uistate
                | Command::Xsubscreen
                | Command::Envinput
                | Command::Envload
                | Command::Envproject
                | Command::Envsave
                | Command::Envstat
                | Command::Envtype
                | Command::Metaedit
                | Command::Metaload
                | Command::Metasave,
                _,
            ) => self.env_command(command, args, want),
            (Command::Dialog, _) => self.dialog_command(command, args, want),
            (Command::Helpget | Command::Helpinfo, _) => self.help_command(command, args, want),
            (Command::Inspect, Want::Outs(0)) if args.len() == 1 => Ok(self.inspect(&args[0])),
            (Command::Loadg | Command::Loadv | Command::Saveg | Command::Savev, _) => {
                self.loadsave_command(command, args, want)
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
                | Command::Gputchrp
                | Command::Gsample
                | Command::Gsave
                | Command::Garray
                | Command::Gtarget
                | Command::Gtri
                | Command::Gupdate,
                _,
            ) => self.grp_command(command, args, want),
            (
                Command::Acls
                | Command::Attr
                | Command::Backcolor
                | Command::Fade
                | Command::Fadechk
                | Command::Layer
                | Command::Lclip
                | Command::Lfilter
                | Command::Lmatrix
                | Command::Tblend
                | Command::Tcolor
                | Command::Thide
                | Command::Thome
                | Command::Tlayer
                | Command::Tofs
                | Command::Tpage
                | Command::Trot
                | Command::Tscale
                | Command::Tscreen
                | Command::Tshow
                | Command::Xscreen,
                _,
            ) => self.screen_command(command, args, want),
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
                | Command::Spfunc
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
            (
                Command::Chkchr
                | Command::Scroll
                | Command::Tarray
                | Command::Tcoord
                | Command::Tcopy
                | Command::Tfill
                | Command::Tload
                | Command::Tanim
                | Command::Tchk
                | Command::Tfunc
                | Command::Tput
                | Command::Tsave
                | Command::Tstart
                | Command::Tstop
                | Command::Tupdate
                | Command::Tvar,
                _,
            ) => self.text_command(command, args, want),
            (
                Command::Accel
                | Command::Brepeat
                | Command::Button
                | Command::Controller
                | Command::Gyroa
                | Command::Gyrosync
                | Command::Gyrov
                | Command::Key
                | Command::Keyboard
                | Command::Mbutton
                | Command::Mouse
                | Command::Pushkey
                | Command::Stick
                | Command::Touch
                | Command::Xctrlstyle,
                _,
            ) => self.input_command(command, args, want),
            (
                Command::Date
                | Command::Dtread
                | Command::Fontinfo
                | Command::Maincnt
                | Command::Millisec
                | Command::Perfbegin
                | Command::Perfend
                | Command::Randomize
                | Command::Rnd
                | Command::Rndf
                | Command::Sysparam
                | Command::Time
                | Command::Tmread,
                _,
            ) => self.system_command(command, args, want),
            (
                Command::Beep
                | Command::Recchk
                | Command::Recdata
                | Command::Reclen
                | Command::Recpos
                | Command::Recsave
                | Command::Recstart
                | Command::Recstop
                | Command::Efcen
                | Command::Efcset
                | Command::Efcwet
                | Command::Pcmcont
                | Command::Pcmpos
                | Command::Pcmstop
                | Command::Pcmstream
                | Command::Pcmvol
                | Command::Sndmsbal
                | Command::Talk
                | Command::Talkchk
                | Command::Talkstop
                | Command::Beeppan
                | Command::Beeppit
                | Command::Beepstop
                | Command::Beepvol
                | Command::Wavset
                | Command::Wavseta
                | Command::Bgmchk
                | Command::Bgmclear
                | Command::Bgmcont
                | Command::Bgmpause
                | Command::Bgmpitch
                | Command::Bgmplay
                | Command::Bgmset
                | Command::Bgmsetd
                | Command::Bgmstop
                | Command::Bgmvar
                | Command::Bgmvol
                | Command::Bgmwet
                | Command::Chkmml
                | Command::Sndmvol
                | Command::Sndstop,
                _,
            ) => self.sound_command(command, args, want),
            (
                Command::Irread
                | Command::Irsprite
                | Command::Irstart
                | Command::Irstate
                | Command::Irstop
                | Command::Tcbike
                | Command::Tccar
                | Command::Tcfishing
                | Command::Tchouse
                | Command::Tcpiano
                | Command::Tcplane
                | Command::Tcrobot
                | Command::Tcsubm
                | Command::Tcvehicle
                | Command::Tcvisor
                | Command::Vibrate,
                _,
            ) => self.device_command(command, args, want),
            _ => Err(no_form()),
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
        match self.run(command, args, want) {
            // A function called as a command with one OUT variable, such
            // as `SPSET 242 OUT IX`, runs as the function.
            Err(e) if want == Want::Outs(1) && is_no_form(&e) => {
                self.run(command, args, Want::Value)
            }
            result => result,
        }
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
        self.line = Some(LineEdit::new(&self.text[CONSOLE]));
        self.recall = None;
        Ok(())
    }

    fn retry(&mut self) -> bool {
        std::mem::take(&mut self.retry)
    }

    fn exec_source(&mut self, file: Option<&[u16]>, slot: i32) -> Result<Exec> {
        // Checked on hardware.
        if self.direct {
            return Err(ErrorKind::CantUseFromDirectMode.into());
        }
        let slot = slot_arg(Some(&Value::Int(slot)))?;
        if let Some(file) = file {
            // A file that cannot be loaded is Load failed, after the
            // notice that stands for SmileBASIC 4's dialog (checked on
            // hardware). UNVERIFIED: what cancelling does.
            match self.load_asking("EXEC", &String::from_utf16_lossy(file), slot) {
                None => {
                    self.retry = false;
                    return Ok(Exec::Wait);
                }
                Some(false) if self.result == -1 => return Ok(Exec::Skip),
                Some(false) => return Err(ErrorKind::LoadFailed.into()),
                Some(true) => {}
            }
        }
        Ok(Exec::Run(self.slots[slot].clone()))
    }

    fn program_name(&self, slot: usize) -> Vec<u16> {
        self.slot_names
            .get(slot)
            .map_or(Vec::new(), |n| n.encode_utf16().collect())
    }

    fn entered_slot(&mut self, slot: usize, program: &Rc<Compiled>) {
        self.program = Some(Rc::clone(program));
        self.slot_programs[slot] = Some(Rc::clone(program));
        self.running_slot = Some(slot);
    }

    // Checked on hardware: CALLIDX() is 4096 after CALL SPRITE and 5
    // after CALL TEXT.
    fn callbacks_end(&self, kind: Callbacks) -> i32 {
        match kind {
            Callbacks::Sprite => sprites::SPRITES as i32,
            Callbacks::Text => SCREENS as i32,
        }
    }

    // Checked on hardware: GARRAY and TARRAY take nothing from FREEMEM.
    fn own_arrays(&self) -> Vec<sb_core::value::ArrayRef> {
        let text = self.text.iter().filter_map(|t| t.array_if_made());
        text.chain(self.grp.arrays()).collect()
    }

    fn set_data_pos(&mut self, pos: usize) {
        self.data_pos.set(pos);
    }

    fn data_pos(&self) -> Option<usize> {
        Some(self.data_pos.get())
    }

    // Checked on hardware: in id order, and a callback that clears a
    // later sprite skips it, while one that sets a later sprite's SPFUNC
    // gets it called.
    fn next_callback(&mut self, kind: Callbacks, after: i32) -> Option<(i32, Vec<u16>)> {
        let from = (after + 1).max(0) as usize;
        match kind {
            Callbacks::Sprite => (self.sprites.list.iter().enumerate().skip(from))
                .filter(|(_, s)| s.used)
                .find_map(|(i, s)| Some((i as i32, s.func.clone()?))),
            Callbacks::Text => (self.text.iter().enumerate().skip(from))
                .find_map(|(i, t)| Some((i as i32, t.func.clone()?))),
        }
    }

    // Up and Down recall history in direct mode
    fn input_poll(&mut self) -> Result<Option<Vec<u16>>> {
        if self.line.is_none() {
            return Ok(None);
        }
        // Only lines typed in direct mode, not INPUT's, are completed.
        let options = match self.recall {
            Some(_) => Options::from_settings(&self.settings, false),
            None => None,
        };
        while let Some(key) = self.keys.pop_front() {
            let console = &mut self.text[CONSOLE];
            let line = self.line.as_mut().expect("checked above");
            // Up and Down stay the history's; Tab takes a word.
            if matches!(key, keys::UP | keys::DOWN) {
                line.completion = None;
            }
            if let Some(list) = &mut line.completion {
                let outcome = list.key(key, options.is_some_and(|o| o.enter));
                match outcome {
                    complete::Outcome::Used => {}
                    complete::Outcome::Close => line.completion = None,
                    complete::Outcome::Accept => {
                        let list = line.completion.take().expect("open");
                        let word: Vec<u16> = list.chosen().encode_utf16().collect();
                        line.replace_before(list.start, &word);
                    }
                    complete::Outcome::Pass => {}
                }
                if outcome != complete::Outcome::Pass {
                    line.draw(console);
                    continue;
                }
            }
            if matches!(key, keys::UP | keys::DOWN) {
                self.recall_line(key == keys::UP);
                continue;
            }
            if key == keys::ENTER {
                let line = self.line.take().expect("checked above");
                return Ok(Some(line.finish(console)));
            }
            let listing = line.completion.take().is_some();
            line.key(key);
            let looks = match key {
                keys::BACKSPACE => listing,
                c => is_ident_char(c) || c == 0x40 || c == 0x23,
            };
            line.draw(console);
            if let (true, Some(options), false) = (looks, options, self.line_quiet) {
                self.line_words(options);
            }
        }
        Ok(None)
    }
}

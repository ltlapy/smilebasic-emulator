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
use crate::compose::{self, LAYERS, Layer};
use crate::confirm::{Ask, Confirm, unsaved_lines};
use crate::editor::split_lines;
use crate::editor::{Area, Editor, Look, blend, frame_size, menu_height};
use crate::extra_glyphs::{ExtraGlyphs, FontSource, Fonts};
use crate::font::{Font, FontSize};
use crate::framebuffer::Framebuffer;
use crate::graphics::Graphics;
use crate::half_width::HalfGlyphs;
use crate::hint;
use crate::input::Input;
use crate::line::{self, LineEdit, codes};
use crate::palette::Part;
use crate::program_font::FontPatch;
use crate::raster::{CpuRasterizer, RasterInput, Rasterizer, SceneBackend};
use crate::scene::{Scene, SceneInput};
use crate::screen::{Fade, ScreenMode, darken};
pub use crate::settings::Settings;
use crate::sound::Sound;
use crate::speed::Meter;
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
    Envlocate,
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
    Subhide,
    Subrun,
    Subshow,
    Substop,
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

impl Command {
    /// Whether the command never changes what the screens show, so that
    /// a program that only runs such commands leaves the frame as it is.
    fn unseen(self) -> bool {
        use Command::*;
        matches!(
            self,
            Accel
                | Beep
                | Beeppan
                | Beeppit
                | Beepstop
                | Beepvol
                | Bgmchk
                | Bgmclear
                | Bgmcont
                | Bgmpause
                | Bgmpitch
                | Bgmplay
                | Bgmset
                | Bgmsetd
                | Bgmstop
                | Bgmvar
                | Bgmvol
                | Bgmwet
                | Brepeat
                | Button
                | Chkchr
                | Chkfile
                | Chkmml
                | Date
                | Efcen
                | Efcset
                | Efcwet
                | Envstat
                | Envtype
                | Fadechk
                | Fontinfo
                | Gpget
                | Gyroa
                | Gyrosync
                | Gyrov
                | Inkey
                | Irread
                | Irstate
                | Maincnt
                | Mbutton
                | Millisec
                | Mouse
                | Pcmcont
                | Pcmpos
                | Pcmstop
                | Pcmstream
                | Pcmvol
                | Perfbegin
                | Perfend
                | Randomize
                | Recchk
                | Recdata
                | Reclen
                | Recpos
                | Recsave
                | Recstart
                | Recstop
                | Rnd
                | Rndf
                | Sndmsbal
                | Sndmvol
                | Sndstop
                | Spchk
                // Templates show only once SPSET or SPCHR uses them.
                | Spdef
                | Sphitinfo
                | Sphitrc
                | Sphitsp
                | Spused
                | Spvar
                | Stick
                | Talk
                | Talkchk
                | Talkstop
                | Tchk
                | Time
                | Touch
                | Tvar
                | Vibrate
                | Wavset
                | Wavseta
        )
    }

    /// Whether the command's forms that give values only read, so that
    /// they leave the screens as they are.
    fn reads(self) -> bool {
        use Command::*;
        matches!(
            self,
            Attr | Backcolor
                | Color
                | Gcolor
                | Gtarget
                | Keyboard
                | Locate
                | Prgget
                | Spchr
                | Spcol
                | Spcolor
                | Sphide
                | Sphome
                | Splayer
                | Splink
                | Spofs
                | Sppage
                | Sprot
                | Spscale
                | Spshow
                | Tcolor
                | Thome
                | Tlayer
                | Tmread
                | Tofs
                | Trot
                | Tscale
                | Xscreen
        )
    }
}

/// What a command call can do to the screens.
enum Sight {
    /// Nothing, or nothing beyond what render notices itself.
    Unseen,
    /// Change how one sprite shows, and nothing else.
    Sprite(usize),
    /// Anything.
    Seen,
}

pub(crate) const COMMANDS: &[(&str, Command)] = &[
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
    ("ENVLOCATE", Command::Envlocate),
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
    ("SUBHIDE", Command::Subhide),
    ("SUBRUN", Command::Subrun),
    ("SUBSHOW", Command::Subshow),
    ("SUBSTOP", Command::Substop),
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Run(usize),
    Clear,
    Cont,
    Trace,
    /// SUBRUN's file name, as typed.
    Subrun(String),
    Substop,
    /// SUBSHOW (true) or SUBHIDE.
    Subshow(bool),
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

/// How many unread keys each key buffer keeps; the oldest goes for a new one.
pub const KEY_BUFFER: usize = 128;
/// How many keys go a frame from the buffer typed keys wait in to the one
/// programs read.
const KEYS_A_FRAME: usize = 32;
/// Frames per half period of the editor cursor's blink: at 60 fps, a full
/// blink takes about 833 ms.
const CURSOR_BLINK: u32 = 25;
/// Frames per half period of the INPUT cursor's blink.
const LINE_CURSOR_BLINK: u32 = 20;
/// The pixels between the sides of a split editor, and their color.
const SPLIT_DIVIDER: u32 = 2;
const SPLIT_COLOR: u32 = 0xFF30_3040;
/// The frame drawn around the side of a split editor that gets the keys,
/// when it changes: shown whole for `SPLIT_FLASH_HOLD` frames, then
/// fading out over `SPLIT_FLASH_FADE`.
const SPLIT_FLASH: u32 = 0xFF58_A0FF;
const SPLIT_FLASH_HOLD: u32 = 12;
const SPLIT_FLASH_FADE: u32 = 30;
/// The share of the editor's frame, out of 20, below it for the
/// debugger's panel.
const PANEL_SHARE: u32 = 7;

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

/// A key the frontend reports, as one of the [`keys`] values, as SmileBASIC 4's
/// code.
fn key_code(key: u16) -> u32 {
    match key {
        keys::UP => codes::UP,
        keys::DOWN => codes::DOWN,
        keys::LEFT => codes::LEFT,
        keys::RIGHT => codes::RIGHT,
        keys::BACKSPACE => codes::BACKSPACE,
        keys::DELETE => codes::DELETE,
        keys::PAGE_UP => codes::PAGE_UP,
        keys::PAGE_DOWN => codes::PAGE_DOWN,
        keys::TAB => codes::TAB,
        keys::HOME => codes::HOME,
        keys::END => codes::END,
        keys::ESCAPE => codes::ESCAPE,
        keys::INSERT => codes::INSERT,
        c => u32::from(c),
    }
}

/// What INKEY$ gives for a code, a [`keys`] value or a character; 0 for
/// nothing, and it then goes on to the next key.
pub(crate) fn key_value(code: u32) -> u16 {
    match code & codes::CODE {
        codes::UP => keys::UP,
        codes::DOWN => keys::DOWN,
        codes::LEFT => keys::LEFT,
        codes::RIGHT => keys::RIGHT,
        codes::BACKSPACE => keys::BACKSPACE,
        codes::DELETE => keys::DELETE,
        codes::PAGE_UP => keys::PAGE_UP,
        codes::PAGE_DOWN => keys::PAGE_DOWN,
        codes::TAB => keys::TAB,
        codes::HOME | 0x1000C => keys::HOME,
        codes::END | 0x1000D => keys::END,
        codes::ESCAPE => keys::ESCAPE,
        codes::INSERT => keys::INSERT,
        c => u16::try_from(c).unwrap_or(0),
    }
}

/// A code the system takes on its way to the program, such as
/// #SOFTKEY_HIDDEN's &H10056. No more keys go on that frame.
fn system_key(code: u32) -> bool {
    matches!(code,
        0x10020..=0x10023
        | 0x10028
        | 0x1003A
        | 0x1003C
        | 0x1004A..=0x10052
        | 0x10054..=0x10056
        | 0x10058
        | 0x1005A..=0x1005E
        | 0x11000)
}

fn push_key(buffer: &mut VecDeque<u32>, code: u32) {
    if buffer.len() == KEY_BUFFER {
        buffer.pop_front();
    }
    buffer.push_back(code);
}

/// A direct-mode line that Up and Down walk through the history from.
struct Recall {
    /// The history entry shown, newest first; `None` is the line that was
    /// being typed.
    at: Option<usize>,
    /// The line that was being typed, which Down brings back.
    draft: Vec<u16>,
}

/// The slot left out of the other slots' names, and the slots' change
/// counts they were found at.
type NamesKey = (usize, Vec<u32>);

pub struct Runtime {
    pub text: Vec<TextScreen>,
    pub(crate) font: Font,
    /// The editor's other font sources: the chosen file and the bundled
    /// glyphs. The sub interpreter shares them.
    pub(crate) extra_glyphs: Rc<ExtraGlyphs>,
    /// The editor's half-width glyphs with the chosen file's, made for
    /// the font order they were made with; `None` while no file is read,
    /// when the font's own serve.
    editor_halves: Option<(FontSource, Rc<HalfGlyphs>)>,
    /// What the settings' program font puts over the font page.
    font_patch: FontPatch,
    /// Graphic pages GRP0-GRP5.
    pub grp: Graphics,
    pub sprites: Sprites,
    pub input: Input,
    pub(crate) system: System,
    pub sound: Sound,
    /// Typed characters not yet read by INKEY$ or INPUT.
    pub(crate) keys: VecDeque<u32>,
    /// Keys on their way to [`Runtime::keys`], typed or from PUSHKEY. Each
    /// frame takes up to [`KEYS_A_FRAME`] of them there.
    pub(crate) typed: VecDeque<u32>,
    /// Pasted text's keys, which the prompt's dropping of keys leaves alone.
    pub(crate) pasted: VecDeque<u32>,
    /// A pasted line has been sent, and the next waits for a line to type in.
    paste_held: bool,
    /// The line being typed for INPUT, LINPUT or direct mode.
    line: Option<LineEdit>,
    /// Where the Up and Down keys have taken the direct-mode line in the
    /// history, if it is being typed in direct mode and they have.
    recall: Option<Recall>,
    pub(crate) frame: u32,
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
    /// Counts each slot's changes by programs and commands, so that an
    /// editor showing it while code runs can take them.
    pub(crate) slot_changes: Vec<u32>,
    /// The other slots' names for completion, by the slot left out and
    /// the slots' change counts they were found at.
    other_names: Option<(NamesKey, Rc<Vec<complete::Item>>)>,
    /// How PRGINS, LOAD and the rest changed slots' lines since the
    /// machine last took them, in order.
    pub(crate) slot_edits: Vec<(usize, crate::prg::SlotEdit)>,
    /// The slot whose program is running; `None` in direct mode.
    pub(crate) running_slot: Option<usize>,
    /// How the last program stopped, kept up to date by the machine.
    pub(crate) stopped: Stopped,
    /// Where it stopped, innermost first, as slots and lines: the line
    /// itself, then each DEF call and GOSUB it was in.
    pub(crate) backtrace: Vec<(usize, u32)>,
    /// The slot and line that PRGEDIT chose, for PRGGET$ and the rest.
    pub(crate) prg_edit: Option<crate::prg::PrgEdit>,
    /// A slot's source split into lines for the PRG commands.
    pub(crate) prg_lines: crate::prg::LinesCache,
    /// The slot the editor shows, or last showed.
    pub(crate) edit_slot: usize,
    /// Direct-mode lines typed, newest first, for ENVINPUT$.
    pub(crate) history: crate::env::History,
    /// Which interpreter this is, as ENVTYPE gives it: 0 for the main
    /// one, 1 for the sub interpreter that runs SUBRUN's program.
    pub(crate) env_type: i32,
    /// Whether this interpreter has the focus, and so the controls and
    /// the keyboard.
    pub(crate) focus: bool,
    /// On the sub interpreter, XSUBSCREEN's rectangle on the display:
    /// x, y, width and height. It shows nothing until one is set.
    pub(crate) sub_screen: Option<[i32; 4]>,
    /// On the main interpreter, whether a subprogram runs beside it, which
    /// ENVFOCUS can give the focus to.
    pub(crate) sub_running: bool,
    /// ENVFOCUS asked for the focus (true) or gave it away during the
    /// frame, which moves it some frames later.
    pub(crate) focus_wish: Option<bool>,
    /// The focus is moving, and ENVFOCUS does nothing until it has.
    pub(crate) focus_busy: bool,
    /// On the main interpreter, what it is doing and its last error, as
    /// ENVSTAT gives them to the sub interpreter: a [`crate::sub::status`],
    /// then the error's number, argument, slot, character and line.
    pub(crate) env_status: i32,
    pub(crate) env_error: [i32; 5],
    /// A subprogram's ENVSTAT 0 asked the main interpreter to forget its
    /// stopped program.
    pub(crate) forget_stop: bool,
    /// Drawn darker, as the side without the focus while a subprogram
    /// runs.
    pub(crate) dimmed: bool,
    /// Something may have changed what the screens show since the last
    /// frame was drawn.
    changed: bool,
    /// What the last frame was drawn with besides the screens' state: the
    /// cursor's blink and the dimming.
    drawn_with: Option<(Option<bool>, bool)>,
    /// Counts the frames drawn, for the frontend to upload only new ones.
    revision: u64,
    /// The last frame was drawn by a [`SceneBackend`], which holds it.
    on_backend: bool,
    /// What goes over a frame a backend draws; made when first wanted.
    overlay: Option<Framebuffer>,
    /// What CLIPBOARD reads and writes.
    pub clipboard: Box<dyn crate::env::Clipboard>,
    /// The DIALOG the program waits on, and the frontend's answer.
    pub(crate) dialog: Option<crate::dialog::Waiting>,
    pub(crate) dialog_answer: Option<crate::dialog::DialogAnswer>,
    /// The answer that closed the last DIALOG, for the other interpreter's.
    pub(crate) dialog_closed: Option<crate::dialog::DialogAnswer>,
    /// The other interpreter's DIALOG is open: what the controllers type
    /// goes to it.
    pub(crate) dialog_beside: bool,
    /// Lines of a FILES listing still to show, after a key is pressed.
    pub(crate) listing_left: Option<VecDeque<Vec<u16>>>,
    /// Lines of an INSPECT still to show, after a key is pressed.
    pub(crate) inspect_left: Option<VecDeque<Vec<u16>>>,
    /// The project metadata that METAEDIT edits.
    pub(crate) meta: crate::env::Meta,
    pub settings: Settings,
    /// The console's time the program has used, with a console's speed
    /// chosen.
    pub meter: Option<Meter>,
    pub storage: Storage,
    /// What RESULT() returns: 1 if the last file command succeeded, 0 if
    /// it failed, -1 if the user cancelled it.
    pub(crate) result: i32,
    pub(crate) confirm: Option<Confirm>,
    pub(crate) answer: Option<bool>,
    /// The last command waits for an answer; see [`Host::retry`].
    pub(crate) retry: bool,
    /// See [`Runtime::hold_for_a_frame`].
    frame_waited: bool,
    /// See [`Runtime::mode_taken`]; until the next frame or VSYNC.
    pub(crate) in_frame_job: bool,
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
    /// The frame being drawn, kept so as to keep its memory.
    scene: Scene,
    /// Draws it, and holds the frame.
    raster: CpuRasterizer,
    /// XSCREEN, and what ACLS goes back to.
    pub(crate) mode: ScreenMode,
    startup_mode: ScreenMode,
    /// The editor draws at a multiple of the start-up resolution, for its
    /// font and size (see [`frame_size`]).
    editor_fb: Framebuffer,
    /// What `editor_fb` shows, as [`Runtime::editor_key`] gave it, while
    /// it can be shown again without drawing it.
    pub(crate) editor_key: Option<u64>,
    /// Counts the frames drawn into `editor_fb`.
    editor_revision: u64,
    /// SmileBASIC's words with their forms from the help, for completion
    /// and argument hints.
    pub library: Rc<Library>,
    /// SmileBASIC 4's help, for HELPGET and HELPINFO.
    pub help: Option<Rc<crate::help::Help>>,
    /// The frame the cursor of the line typed last started blinking.
    line_blink_from: u32,
    /// Lines overwrite rather than insert, from Insert on a line before.
    line_overwrite: bool,
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
    /// Where the debugger's panel starts on the editor's frame, when it
    /// is shown.
    pub(crate) panel_at: Option<u32>,
    /// GRP4 at start-up, which ACLS puts back.
    sprite_page: Option<Vec<u32>>,
}

/// The text screens at start-up: the console in front of everything else
/// with 8-pixel characters, drawn from the 16-pixel font where the frame
/// is fine enough, and the others with 16-pixel characters.
// From the manual's program equivalent to ACLS, which sets TSCREEN
// #TCONSOLE,16,8 and TSCREEN I,16,16 at a sample magnification of 2.
fn startup_screens(mode: &ScreenMode) -> Vec<TextScreen> {
    let (w, h) = (mode.width, mode.height);
    (0..SCREENS)
        .map(|i| {
            let mut t = TextScreen::new(w, h);
            if i == CONSOLE {
                t.inverts = true;
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
/// Wrong number of arguments, a command used as a function too, or Wrong
/// number of OUT arguments as [`counts`] has it for the command.
pub(crate) fn no_form() -> Error {
    ErrorKind::WrongNumberOfArguments.into()
}

/// An error from command `name`, which names it after the place, with the
/// argument that was wrong when that is one of `args`: `(LOCATE:2)`.
#[cold]
pub(crate) fn command_error(e: Error, name: &str, args: &[Value]) -> Error {
    match e.kind {
        // Not the program's to see, or shown with no place.
        ErrorKind::MemoryCheck
        | ErrorKind::CantUseFromDirectMode
        | ErrorKind::CantContinue
        | ErrorKind::CantTrace => e,
        ErrorKind::OutOfMemory if e.detail.is_some() => e,
        _ => e.in_args(args).of_command(name),
    }
}

fn is_no_form(e: &Error) -> bool {
    e.kind == ErrorKind::WrongNumberOfArguments && e.detail.is_none()
}

/// How SmileBASIC 4 tells a call no form takes: `Wrong number of OUT
/// arguments` (53) when its argument count is one of `args` and its OUT
/// count none of `outs`, `Wrong number of arguments` (52) otherwise. Each
/// command's handler checks the two counts itself, in its own order. Most
/// check the OUT count first ([`Counts::outs`]), some the argument count
/// ([`Counts::args`]), and some sprite commands want a sprite before the
/// OUT count. Bit n of a mask stands for n, bit 31 for 31 and more.
#[derive(Clone, Copy)]
struct Counts {
    args: u32,
    outs: u32,
}

const fn mask(counts: &[u8]) -> u32 {
    let mut m = 0;
    let mut i = 0;
    while i < counts.len() {
        m |= 1 << counts[i];
        i += 1;
    }
    m
}

const fn at_least(n: u8) -> u32 {
    u32::MAX << n
}

impl Counts {
    /// Never 53.
    const NONE: Counts = Counts { args: 0, outs: 0 };

    /// The OUT count is checked first: any but `outs` is 53.
    const fn outs(outs: &[u8]) -> Counts {
        Counts::new(u32::MAX, outs)
    }

    /// The argument count is checked first: with one of `args`, a wrong OUT
    /// count is 53.
    const fn args(args: &[u8]) -> Counts {
        Counts::new(mask(args), &[])
    }

    const fn new(args: u32, outs: &[u8]) -> Counts {
        Counts {
            args,
            outs: mask(outs),
        }
    }

    fn out_error(self, args: usize, outs: usize) -> bool {
        let bit = |n: usize| 1u32 << n.min(31);
        self.args & bit(args) != 0 && self.outs & bit(outs) == 0
    }
}

/// The counts SmileBASIC 4's handler for `command` checks (4.4.8).
fn counts(command: Command) -> Counts {
    match command {
        Command::Prgedit => Counts::outs(&[0, 1, 2]),
        Command::Envstat => Counts::outs(&[0, 1, 3, 6]),
        Command::Attr => Counts::outs(&[0, 1, 3]),
        Command::Backcolor
        | Command::Beep
        | Command::Bgmvar
        | Command::Clipboard
        | Command::Color
        | Command::Dialog
        | Command::Envfocus
        | Command::Fade
        | Command::Gcolor
        | Command::Gputchrp
        | Command::Gsample
        | Command::Gtarget
        | Command::Key
        | Command::Lfilter
        | Command::Metaedit
        | Command::Pcmpos
        | Command::Project
        | Command::Sndmvol
        | Command::Sphide
        | Command::Splayer
        | Command::Splink
        | Command::Sppage
        | Command::Spset
        | Command::Spshow
        | Command::Spvar
        | Command::Tblend
        | Command::Tcolor
        | Command::Thide
        | Command::Tlayer
        | Command::Trot
        | Command::Tshow
        | Command::Tvar
        | Command::Vibrate
        | Command::Uistate => Counts::outs(&[0, 1]),
        Command::Xscreen => Counts::outs(&[0, 2, 3, 4, 5]),
        Command::Tofs => Counts::outs(&[0, 2, 3]),
        Command::Brepeat
        | Command::Loadg
        | Command::Locate
        | Command::Thome
        | Command::Tscale
        | Command::Xctrlstyle => Counts::outs(&[0, 2]),
        Command::Tpage => Counts::outs(&[0, 3]),
        Command::Gclip | Command::Lclip | Command::Tscreen | Command::Xsubscreen => {
            Counts::outs(&[0, 4])
        }
        Command::Acls
        | Command::Animdef
        | Command::Beeppan
        | Command::Beeppit
        | Command::Beepstop
        | Command::Beepvol
        | Command::Bgmclear
        | Command::Bgmcont
        | Command::Bgmpause
        | Command::Bgmpitch
        | Command::Bgmplay
        | Command::Bgmset
        | Command::Bgmsetd
        | Command::Bgmstop
        | Command::Bgmvol
        | Command::Bgmwet
        | Command::Cls
        | Command::Efcset
        | Command::Efcwet
        | Command::Envload
        | Command::Envsave
        | Command::Gbox
        | Command::Gcircle
        | Command::Gcls
        | Command::Gcopy
        | Command::Gfill
        | Command::Gline
        | Command::Gload
        | Command::Gpaint
        | Command::Gpset
        | Command::Gputchr
        | Command::Gsave
        | Command::Gtri
        | Command::Gupdate
        | Command::Gyrosync
        | Command::Inspect
        | Command::Irsprite
        | Command::Irstart
        | Command::Irstop
        | Command::Layer
        | Command::Lmatrix
        | Command::Load
        | Command::Metaload
        | Command::Perfbegin
        | Command::Pcmcont
        | Command::Pcmstop
        | Command::Pcmstream
        | Command::Pcmvol
        | Command::Prgdel
        | Command::Prgins
        | Command::Recsave
        | Command::Recstart
        | Command::Recstop
        | Command::Save
        | Command::Saveg
        | Command::Savev
        | Command::Scroll
        | Command::Spanim
        | Command::Spclr
        | Command::Spfunc
        | Command::Spstart
        | Command::Spstop
        | Command::Talk
        | Command::Talkstop
        | Command::Tanim
        | Command::Tcopy
        | Command::Tfill
        | Command::Tfunc
        | Command::Tload
        | Command::Tput
        | Command::Tsave
        | Command::Tstart
        | Command::Tstop
        | Command::Tupdate
        | Command::Wavset
        | Command::Wavseta => Counts::outs(&[0]),
        Command::Chkchr => Counts::outs(&[1, 2]),
        Command::Bgmchk
        | Command::Button
        | Command::Chkmml
        | Command::Envinput
        | Command::Inkey
        | Command::Irstate
        | Command::Mbutton
        | Command::Prgget
        | Command::Prgname
        | Command::Prgsize
        | Command::Recchk
        | Command::Reclen
        | Command::Recpos
        | Command::Rnd
        | Command::Sphitrc
        | Command::Sphitsp
        | Command::Talkchk
        | Command::Tarray => Counts::outs(&[1]),
        Command::Recdata | Command::Stick | Command::Tcoord => Counts::outs(&[2]),
        Command::Accel | Command::Gyroa | Command::Gyrov => Counts::outs(&[3]),
        Command::Irread => Counts::outs(&[4]),
        // Two arguments take the same way out as a wrong OUT count.
        Command::Rndf => Counts::outs(&[]),
        Command::Files | Command::Sndmsbal | Command::Touch => Counts::args(&[0, 1, 2]),
        Command::Mouse => Counts::new(mask(&[0, 1]), &[0]),
        Command::Dtread
        | Command::Efcen
        | Command::Helpinfo
        | Command::Prgseek
        | Command::Tmread => Counts::args(&[0, 1]),
        Command::Date
        | Command::Envlocate
        | Command::Envproject
        | Command::Envtype
        | Command::Fadechk
        | Command::Maincnt
        | Command::Metasave
        | Command::Millisec
        | Command::Result
        | Command::Sndstop
        | Command::Sphitinfo
        | Command::Time => Counts::args(&[0]),
        Command::Spofs => Counts::new(mask(&[1, 2]), &[0]),
        Command::Keyboard | Command::Randomize => Counts::args(&[1, 2]),
        Command::Spcolvec => Counts::args(&[1, 3]),
        Command::Spchr | Command::Spdef => Counts::new(mask(&[1]), &[0]),
        Command::Chkfile
        | Command::Controller
        | Command::Delete
        | Command::Garray
        | Command::Perfend
        | Command::Prgset
        | Command::Pushkey
        | Command::Spchk
        | Command::Spunlink
        | Command::Spused
        | Command::Tcbike
        | Command::Tccar
        | Command::Tcfishing
        | Command::Tchk
        | Command::Tchouse
        | Command::Tcpiano
        | Command::Tcplane
        | Command::Tcrobot
        | Command::Tcsubm
        | Command::Tcvehicle
        | Command::Tcvisor => Counts::args(&[1]),
        Command::Fontinfo | Command::Gpget | Command::Helpget | Command::Rename => {
            Counts::args(&[2])
        }
        Command::Spcolor | Command::Sprot => Counts::new(at_least(1), &[0, 1]),
        Command::Sphome | Command::Spscale => Counts::new(at_least(1), &[0, 2]),
        Command::Spcol | Command::Sysparam => Counts::new(at_least(1), &[0]),
        // LOADV checks the counts after the file name, by the file's type.
        _ => Counts::NONE,
    }
}

/// The error for a call of `command` with `args` arguments and `outs` OUT
/// variables that no form takes.
#[cold]
fn no_form_error(command: Command, args: usize, outs: usize) -> Error {
    if counts(command).out_error(args, outs) {
        ErrorKind::WrongNumberOfOutArguments.into()
    } else {
        no_form()
    }
}

/// Runs `check` on `v`, so that an error it gives names `v` as the
/// argument that was wrong, for ENVSTAT.
pub(crate) fn checking<T>(v: &Value, check: impl FnOnce() -> Result<T>) -> Result<T> {
    check().map_err(|e| e.of_value(v))
}

pub(crate) fn screen_id(v: &Value) -> Result<usize> {
    checking(v, || {
        let id = v.to_int()?;
        usize::try_from(id)
            .ok()
            .filter(|&id| id < SCREENS)
            .ok_or_else(|| Error::with_detail(ErrorKind::OutOfRange, format!("screen {id}")))
    })
}

/// An omittable integer argument.
pub(crate) fn opt_int(v: &Value) -> Result<Option<i32>> {
    checking(v, || match v {
        Value::Default => Ok(None),
        v => v.to_int().map(Some),
    })
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
    /// A 400x240 runtime with [`Settings::for_tests`].
    pub fn for_tests(font: Font) -> Runtime {
        let mut rt = Runtime::new(400, 240, font);
        rt.settings = Settings::for_tests();
        rt
    }

    pub fn new(width: u32, height: u32, font: Font) -> Runtime {
        let mode = ScreenMode::new(width, height);
        Runtime {
            text: startup_screens(&mode),
            font,
            extra_glyphs: Rc::new(ExtraGlyphs::bundled()),
            editor_halves: None,
            font_patch: FontPatch::default(),
            grp: Graphics::default(),
            sprites: Sprites::default(),
            input: Input::default(),
            system: System::default(),
            sound: Sound::default(),
            keys: VecDeque::new(),
            typed: VecDeque::new(),
            pasted: VecDeque::new(),
            paste_held: false,
            line: None,
            recall: None,
            frame: 0,
            direct: false,
            requests: Vec::new(),
            slots: vec![String::new(); SLOTS],
            slot_names: vec![String::new(); SLOTS],
            slot_modified: vec![false; SLOTS],
            slot_changes: vec![0; SLOTS],
            other_names: None,
            slot_edits: Vec::new(),
            running_slot: None,
            stopped: Stopped::No,
            backtrace: Vec::new(),
            prg_edit: None,
            prg_lines: Default::default(),
            edit_slot: 0,
            history: Default::default(),
            env_type: 0,
            focus: true,
            sub_screen: None,
            sub_running: false,
            focus_wish: None,
            focus_busy: false,
            env_status: 0,
            // UNVERIFIED: the error slot, -1.
            env_error: [0, 0, -1, -1, 0],
            forget_stop: false,
            dimmed: false,
            changed: true,
            drawn_with: None,
            revision: 0,
            on_backend: false,
            overlay: None,
            clipboard: Box::new(crate::env::MemoryClipboard::default()),
            meta: Default::default(),
            dialog: None,
            dialog_beside: false,
            dialog_answer: None,
            dialog_closed: None,
            settings: Settings::default(),
            meter: None,
            storage: Storage::none(),
            result: 1,
            confirm: None,
            answer: None,
            retry: false,
            frame_waited: false,
            in_frame_job: false,
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
            scene: Scene::default(),
            raster: CpuRasterizer::new(width, height),
            mode,
            startup_mode: mode,
            editor_fb: Framebuffer::new(width, height),
            editor_key: None,
            editor_revision: 0,
            side_fb: Framebuffer::new(0, 0),
            library: Rc::default(),
            help: None,
            line_blink_from: 0,
            line_overwrite: false,
            help_cursor: Default::default(),
            line_quiet: false,
            split_at: None,
            split_shown: None,
            split_flash: 0,
            panel_at: None,
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
    /// Makes every run the same: see [`System::fix`].
    pub fn fix_time_and_chance(&mut self) {
        self.system.fix();
    }

    pub fn set_startup_mode(&mut self, mode: ScreenMode) {
        self.startup_mode = mode;
        self.editor_fb = Framebuffer::new(mode.width, mode.height);
        self.editor_key = None;
        self.text = startup_screens(&mode);
        self.set_mode(mode);
    }

    /// The sprite image on GRP4, also put back by ACLS.
    /// The page that sprites start on, as set by [`Runtime::set_sprite_page`].
    pub(crate) fn sprite_page(&self) -> Option<&[u32]> {
        self.sprite_page.as_deref()
    }

    pub fn set_sprite_page(&mut self, pixels: Vec<u32>) {
        self.grp.set_page(SPRITE_PAGE, pixels.clone());
        self.sprite_page = Some(pixels);
    }

    /// XSCREEN: the console is resized to fill the screen, keeping its
    /// font, and `#GSPRITE` shows as much of GRP0.
    // The console is cleared, and all of #GSPRITE's settings are reset.
    pub(crate) fn set_mode(&mut self, mode: ScreenMode) {
        self.mode = mode;
        let (fw, fh) = mode.frame();
        self.raster.set_size(fw, fh);
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
        // Before set_mode, which puts #GSPRITE on SPLAYER's layer.
        self.sprites.reset(keep_defs, keep_anims);
        self.set_mode(self.startup_mode);
        self.back_color = 0xFF00_0000;
        self.fade = Fade::default();
        self.layers = [Layer::default(); LAYERS];
        self.filters = Default::default();
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
            let page = self.font_page();
            if page.iter().any(|&p| p != 0) {
                self.grp.set_page(FONT_PAGE, page);
            }
        }
        self.grp.reset_state();
    }

    /// The DATA values from `label` on, for commands that take a
    /// `"@label"`. As on hardware, they move READ's place: RESTORE to
    /// the label, then READ.
    /// A callback's name as SPFUNC and TFUNC keep it: with the slot they
    /// were set in, whose DEFs and labels CALL SPRITE and CALL TEXT look
    /// in from any slot.
    pub(crate) fn callback_name(&self, name: &[u16]) -> Vec<u16> {
        match self.running_slot {
            Some(slot) if !self.direct && !name.contains(&u16::from(b':')) => {
                let mut kept: Vec<u16> = format!("{slot}:").encode_utf16().collect();
                kept.extend_from_slice(name);
                kept
            }
            _ => name.to_vec(),
        }
    }

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
        let page = self.font_page();
        if page.iter().any(|&p| p != 0) {
            self.grp.set_page(crate::text::FONT_PAGE, page);
        }
        self
    }

    /// The sub interpreter's fonts: the main one's font file, and its
    /// program font on the font page.
    pub(crate) fn share_fonts(&mut self, main: &Runtime) {
        self.extra_glyphs = Rc::clone(&main.extra_glyphs);
        self.apply_program_font();
    }

    /// The font page as start-up and ACLS put it on GRP5: SmileBASIC's,
    /// with the program font's glyphs over it.
    fn font_page(&self) -> Vec<u32> {
        let mut page = self.font.page_image();
        if !self.font_patch.is_empty() {
            self.font_patch.apply(&mut page);
        }
        page
    }

    /// Makes the program font's glyphs for the settings and writes them
    /// over GRP5, putting back SmileBASIC's where the last ones were.
    /// Only those cells change; the rest of the page keeps what the
    /// program drew there. Start-up and ACLS write them again.
    pub fn apply_program_font(&mut self) {
        let patch = FontPatch::build(&self.font, &self.extra_glyphs, self.settings.program_font);
        if patch.is_empty() && self.font_patch.is_empty() {
            return;
        }
        let original = self.font.page_image();
        let mut page = match self.grp.page(FONT_PAGE) {
            [] => vec![0; original.len()],
            pixels => pixels.to_vec(),
        };
        self.font_patch.restore(&mut page, &original);
        patch.apply(&mut page);
        self.font_patch = patch;
        self.grp.set_page(FONT_PAGE, page);
    }

    /// The screen's size in pixels, before XSCREEN's sampling.
    pub(crate) fn frame_size(&self) -> (u32, u32) {
        (self.mode.width, self.mode.height)
    }

    /// For the debugger: the corners of a sprite's image where
    /// [`Runtime::render`] draws it, in the frame's pixels, clockwise from
    /// its top left; `None` for a sprite not in use, or one on an LMATRIX
    /// layer with a corner behind the view.
    pub fn sprite_corners(&self, id: usize) -> Option<[(f64, f64); 4]> {
        let s = self.sprites.list().get(id).filter(|s| s.used)?;
        let pose = self.sprites.pose(id);
        let layer = (self.layers.get(usize::try_from(pose.layer).ok()?)?).fine(self.mode.sample);
        let (w, h) = (f64::from(s.w), f64::from(s.h));
        let (w, h) = match s.attr & sprites::attr::ROTATION {
            1 | 3 => (h, w),
            _ => (w, h),
        };
        let m = compose::Affine::place(pose.pos, s.home, pose.rot, pose.scale).then(&layer.matrix);
        let corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].map(|(x, y)| m.apply(x, y));
        let Some(p) = &layer.projection else {
            return Some(corners);
        };
        let sample = f64::from(self.mode.sample);
        let frame = self.mode.frame();
        let mut projected = [(0.0, 0.0); 4];
        for (to, (x, y)) in projected.iter_mut().zip(corners) {
            *to = compose::project_point(p, (x / sample, y / sample), frame)?;
        }
        Some(projected)
    }

    pub fn font(&self) -> &Font {
        &self.font
    }

    /// The line being typed for INPUT or in direct mode, if any.
    pub(crate) fn typed_line(&self) -> Option<Vec<u16>> {
        Some(self.line.as_ref()?.text.clone())
    }

    /// The selection of the line being typed was copied: it ends, and the
    /// text stays.
    pub fn line_copied(&mut self) {
        if let Some(line) = &mut self.line {
            line.drop_selection();
            line.draw(&mut self.text[CONSOLE]);
        }
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

    /// The names of the slots but `except`, in order, when the settings
    /// offer every slot's; kept until a slot changes.
    pub(crate) fn other_slot_names(&mut self, except: usize) -> Option<Rc<Vec<complete::Item>>> {
        if !self.settings.complete_all_slots {
            return None;
        }
        let key = (except, self.slot_changes.clone());
        if let Some((cached, names)) = &self.other_names
            && *cached == key
        {
            return Some(names.clone());
        }
        let mut names = Vec::new();
        for (_, text) in self.slots.iter().enumerate().filter(|&(s, _)| s != except) {
            let lines = split_lines(text);
            names = complete::merge(names, &complete::names(lines.iter().map(Vec::as_slice)));
        }
        let names = Rc::new(names);
        self.other_names = Some((key, names.clone()));
        Some(names)
    }

    /// Completion's list for the word before the cursor of the line typed
    /// in direct mode.
    fn line_words(&mut self, options: Options) {
        // The edited slot's names, also for labels after GOTO.
        let lines = split_lines(&self.slots[self.edit_slot]);
        let mut names = complete::names(lines.iter().map(Vec::as_slice));
        if let Some(others) = self.other_slot_names(self.edit_slot) {
            names = complete::merge(names, &others);
        }
        if let Some(line) = &mut self.line
            && line.selection().is_none()
        {
            line.completion =
                Completion::find(&line.text, line.cursor, &options, &self.library, &names);
            line.draw(&mut self.text[CONSOLE]);
        }
    }

    /// Whether `count` more characters fit the line being typed. A
    /// selection is not counted off.
    pub(crate) fn line_has_room(&self, count: usize) -> bool {
        (self.line.as_ref()).is_none_or(|l| l.text.len() + count <= line::LINE_MAX)
    }

    /// Whether INPUT, LINPUT or direct mode waits for a line.
    pub fn wants_line(&self) -> bool {
        self.line.is_some()
    }

    /// The line being typed and the cursor's place in it.
    pub fn line_and_cursor(&self) -> Option<(&[u16], usize)> {
        self.line.as_ref().map(|l| (&l.text[..], l.cursor))
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

    fn start_line(&mut self) {
        self.paste_held = false;
        let console = &mut self.text[CONSOLE];
        let free = line::rows_kept_free(self.mode.height, self.mode.aspect(), console.font().1);
        // Rows are kept free at once, for a LINPUT without a guide too.
        line::keep_rows_free(console, free);
        let mut line = LineEdit::new(console, free);
        line.overwrite = self.line_overwrite;
        self.line = Some(line);
        self.line_blink_from = self.system.frames;
    }

    /// Waits for a line typed in direct mode, where Up and Down recall the
    /// lines typed before.
    pub(crate) fn direct_input_start(&mut self) {
        self.start_line();
        if let Some(line) = &mut self.line {
            line.selects = true;
        }
        self.recall = Some(Recall {
            at: None,
            draft: Vec::new(),
        });
    }

    /// Direct-mode requests made since the last call.
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    /// A command for DIRECT mode only, run in a program, gives `kind`
    /// when it runs: a Syntax error for CLEAR, NEW and RUN, Can't use in
    /// program for PROJECT.
    pub(crate) fn direct_only(&self, kind: ErrorKind) -> Result<()> {
        match (self.direct, kind) {
            (true, _) => Ok(()),
            (false, ErrorKind::SyntaxError) => Err(Error::with_detail(
                ErrorKind::SyntaxError,
                "only in direct mode",
            )),
            (false, kind) => Err(kind.into()),
        }
    }

    pub(crate) fn request(&mut self, request: Request) -> Result<Vec<Value>> {
        self.direct_only(ErrorKind::SyntaxError)?;
        self.requests.push(request);
        Ok(vec![])
    }

    /// Queues a typed character, or one of the [`keys`] codes.
    pub fn key_press(&mut self, code: u16) {
        self.changed = true;
        push_key(&mut self.typed, key_code(code));
    }

    /// Queues what `PUSHKEY` gives: a character or one of the [`codes`].
    pub(crate) fn push_code(&mut self, code: u32) {
        self.changed = true;
        push_key(&mut self.typed, code);
    }

    /// Queues pasted text's keys, of which none may be lost, however many.
    pub(crate) fn paste_key(&mut self, code: u16) {
        self.changed = true;
        self.pasted.push_back(key_code(code));
    }

    /// Takes the frame's keys to where programs and lines read them. A code
    /// of 0 ends it for this frame.
    pub fn take_typed_keys(&mut self) {
        // Pasted lines go one at a time, so that the prompt after one finds
        // the next still waiting.
        if self.typed.is_empty() && !self.paste_held {
            while self.typed.len() < KEYS_A_FRAME {
                let Some(code) = self.pasted.pop_front() else {
                    break;
                };
                self.typed.push_back(code);
                if code == key_code(keys::ENTER) {
                    self.paste_held = true;
                    break;
                }
            }
        }
        for _ in 0..KEYS_A_FRAME {
            match self.typed.pop_front() {
                None | Some(0) => break,
                Some(code) if system_key(code) => break,
                Some(code) => push_key(&mut self.keys, code),
            }
        }
    }

    /// For a command that does its work and then holds the program until
    /// the next frame, leaving VSYNC's count alone: at the end of its work
    /// it calls this, and the VM calls it again on the next frame.
    pub(crate) fn hold_for_a_frame(&mut self) {
        self.frame_waited = true;
        self.retry = true;
    }

    /// Whether this call of the command is the VM's second, after
    /// [`Runtime::hold_for_a_frame`], with nothing left to do.
    pub(crate) fn frame_wait_over(&mut self) -> bool {
        std::mem::take(&mut self.frame_waited)
    }

    /// The end of XSCREEN and ACLS, which hold the program until the frame
    /// job has taken the mode: it goes on while that job is still running.
    pub(crate) fn mode_taken(&mut self) -> bool {
        let over = self.frame_wait_over();
        self.in_frame_job |= over;
        over
    }

    /// Drops the keys a program left unread and what its buttons were
    /// doing, so that none of it reaches direct mode or the next run.
    pub(crate) fn clear_input(&mut self) {
        self.clear_keys();
        self.pasted.clear();
        self.paste_held = false;
        self.input.forget();
    }

    /// Drops the keys waiting to be read.
    pub(crate) fn clear_keys(&mut self) {
        self.frame_waited = false;
        self.keys.clear();
        self.typed.clear();
    }

    /// Takes the next unread key, for the editor.
    pub(crate) fn next_key(&mut self) -> Option<u16> {
        self.next_key_of(false)
    }

    /// Takes the next unread key; `keyboard_only` drops those a controller
    /// typed on the way to it.
    fn next_key_of(&mut self, keyboard_only: bool) -> Option<u16> {
        while let Some(code) = self.keys.pop_front() {
            if keyboard_only && code >= codes::FROM_PAD {
                continue;
            }
            match key_value(code) {
                0 => {}
                key => return Some(key),
            }
        }
        None
    }

    /// Marks the screens as possibly changed, to be drawn again.
    pub fn touch(&mut self) {
        self.changed = true;
    }

    /// Which frame [`Runtime::render`] gave last: it changes only when the
    /// frame was drawn again.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether the line's cursor shows this frame. The period after a
    /// restart is one frame short, the frame it comes in being counted.
    fn line_blink_on(&self) -> bool {
        let n = self.system.frames.wrapping_sub(self.line_blink_from);
        (n.wrapping_add(1) / LINE_CURSOR_BLINK).is_multiple_of(2)
    }

    /// The frame [`Runtime::render`] gave last.
    pub fn screen_frame(&self) -> &Framebuffer {
        self.raster.frame()
    }

    /// Composes the current frame: layers 7 to 0, each back to front. The
    /// last one is given again while nothing has changed.
    pub fn render(&mut self) -> &Framebuffer {
        self.render_with(None);
        self.raster.frame()
    }

    /// Whether the frame given last is on the [`SceneBackend`] and not in
    /// [`Runtime::screen_frame`].
    pub fn frame_on_backend(&self) -> bool {
        self.on_backend
    }

    /// [`Runtime::render`], offering the scene, if one is drawn, to a backend
    /// first.
    pub fn render_with(&mut self, mut backend: Option<&mut dyn SceneBackend>) {
        self.frame = self.frame.wrapping_add(1);
        // A frame the backend holds has to be drawn again for whoever wants
        // it in the framebuffer.
        self.changed |= self.on_backend && backend.is_none();
        // Counted in frames played, not in frames drawn, which are not
        // one a frame when frames are skipped or only some are looked at.
        // Whether it is a block, if it shows.
        let cursor = (self.line.is_some() && self.line_blink_on()).then_some(self.line_overwrite);
        let with = Some((cursor, self.dimmed));
        let mut cells = false;
        for t in &mut self.text {
            cells |= t.take_changed();
        }
        if !self.changed && !cells && self.drawn_with == with {
            return;
        }
        self.changed = false;
        self.drawn_with = with;
        self.revision += 1;
        self.scene.build(&SceneInput {
            mode: &self.mode,
            layers: &self.layers,
            filters: &self.filters,
            sprites: &self.sprites,
            text: &self.text,
            grp: &self.grp,
            back_color: self.back_color,
            sub: self.env_type != 0,
            fade: self.fade.color,
            dimmed: self.dimmed,
            cursor,
        });
        let input = RasterInput {
            grp: &self.grp,
            font: &self.font,
            cells: std::array::from_fn(|i| self.text[i].cells()),
        };
        // Offered first; the CPU then draws what the backend did not, and what
        // it did only to compare.
        self.on_backend = match &mut backend {
            Some(backend) => backend.draw(&self.scene, &input, self.mode.frame()),
            None => false,
        };
        let backend = backend.filter(|_| self.on_backend);
        let compare = backend.as_ref().is_some_and(|b| b.compares());
        if backend.is_none() || compare {
            self.raster.draw(&self.scene, &input);
        }
        let Some(backend) = backend else {
            self.draw_overlay(false);
            return;
        };
        let shows = self.draw_overlay(true);
        let overlay = (self.overlay.as_ref()).filter(|_| shows);
        self.on_backend = backend.overlay(overlay);
        if !self.on_backend {
            let input = RasterInput {
                grp: &self.grp,
                font: &self.font,
                cells: std::array::from_fn(|i| self.text[i].cells()),
            };
            self.raster.draw(&self.scene, &input);
            self.draw_overlay(false);
        } else if compare {
            self.draw_overlay(false);
            backend.compare(self.raster.frame());
        }
    }

    /// What the emulator itself shows over the frame, after FADE, which is
    /// no part of SmileBASIC 4's picture: on the CPU's frame, or for a backend
    /// on a clear one of its own. Whether anything was to be shown.
    fn draw_overlay(&mut self, on_clear: bool) -> bool {
        // In direct mode, the hint for the command being typed above the
        // line and completion's list below the word.
        let options = Options::from_settings(&self.settings, false).filter(|_| !self.line_quiet);
        let mut shows = false;
        if let (Some(line), Some(_), Some(options)) = (&self.line, &self.recall, options) {
            let fb = match on_clear {
                true => {
                    let (w, h) = self.mode.frame();
                    let fb = self.overlay.get_or_insert_with(|| Framebuffer::new(w, h));
                    if (fb.width(), fb.height()) != (w, h) {
                        *fb = Framebuffer::new(w, h);
                    }
                    fb.clear(0);
                    fb
                }
                false => self.raster.frame_mut(),
            };
            // Direct mode shows the console's font, as programs see it.
            let plain = Fonts::plain(&self.font, FontSize::Small);
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
                    shows |= hint.is_some() || syntax.is_some();
                    hint::draw_band(fb, plain, band, (hint.as_ref(), syntax), scale as u32);
                } else if let Some(hint) = &hint {
                    shows = true;
                    hint.draw(fb, plain, left, (top, top + scale), scale as u32);
                }
            }
            if let Some(list) = &line.completion
                && let Some(cell) = line.cell_of(list.start, w)
            {
                shows = true;
                let (left, top) = at(cell);
                let colors = &self.settings.editor_colors;
                list.draw(fb, plain, (left, top + scale), top, scale as u32, colors);
            }
        }
        shows
    }

    /// MAINCNT, as a program reads it between frames.
    pub fn main_count(&self) -> u32 {
        self.system.frames
    }

    /// Starts a frame: the input it sees, and MAINCNT.
    pub fn begin_frame(&mut self) {
        self.input.latch();
        // What the controllers type while a DIALOG is open goes nowhere,
        // but a key held on after it closes repeats into the queue.
        if self.dialog.is_some() || self.dialog_beside {
            self.input.drop_pad_key();
        }
        // A controller's key can be read on the frame the button goes
        // down, not on the next.
        else if let Some(code) = self.input.pad_key() {
            self.changed = true;
            push_key(&mut self.typed, code | codes::FROM_PAD);
        }
        self.take_typed_keys();
        self.system.frames = self.system.frames.wrapping_add(1);
    }

    /// Plays one frame of the sprites' and text screens' animations.
    pub fn animate(&mut self) {
        // A fade steps with the animations, starting on the frame after
        // FADE.
        let mut moved = self.fade.fading();
        self.fade.step();
        moved |= self.sprites.animate();
        for t in &mut self.text {
            moved |= t.animate();
        }
        self.changed |= moved;
    }

    /// Clears SPVAR's and TVAR's variables, as starting a program does.
    pub fn clear_vars(&mut self) {
        self.sprites.clear_vars();
        for t in &mut self.text {
            t.vars.clear();
        }
    }

    /// Composes a frame showing the editor instead of the screens.
    /// `split` is `None` for one side, else whether the sides are one
    /// above the other. `panel` leaves room below for the debugger's.
    /// The frame is `area`'s if given, and otherwise the size SmileBASIC
    /// 4's rows take at the screen's shape.
    pub(crate) fn render_editors(
        &mut self,
        editors: &mut [Editor; 2],
        active: usize,
        split: Option<bool>,
        panel: bool,
        area: Option<Area>,
    ) -> &Framebuffer {
        self.frame = self.frame.wrapping_add(1);
        let blink = (self.frame / CURSOR_BLINK).is_multiple_of(2);
        let s = &self.settings;
        let mode = self.startup_mode;
        let (w, h) = match area {
            Some(area) => (area.size.0.max(1), area.size.1.max(1)),
            None => frame_size((mode.width, mode.height), s.editor_font, s.editor_size),
        };
        let panel = (panel, area.map(|area| area.top));
        if (self.editor_fb.width(), self.editor_fb.height()) != (w, h) {
            self.editor_fb = Framebuffer::new(w, h);
            self.editor_key = None;
        }
        if let Some(rows) = split
            && self.split_shown != Some((rows, active))
        {
            self.split_shown = Some((rows, active));
            self.split_flash = self.frame;
        }
        // Nothing it shows changed since it was last drawn.
        let key = self.editor_key(editors, active, split, panel, blink);
        if self.editor_key == Some(key) {
            return &self.editor_fb;
        }
        self.draw_editors(editors, active, split, panel, blink);
        self.editor_revision += 1;
        // Drawing scrolls the view to the cursor. Until it has nothing
        // left to move, the next frame draws it again.
        let drawn = self.editor_key(editors, active, split, panel, blink);
        self.editor_key = (drawn == key).then_some(key);
        &self.editor_fb
    }

    /// Counts the frames drawn into the editor's frame, which stays as
    /// it is while the count does.
    pub(crate) fn editor_revision(&self) -> u64 {
        self.editor_revision
    }

    /// The editor's frame as last drawn.
    pub(crate) fn editor_frame(&self) -> &Framebuffer {
        &self.editor_fb
    }

    /// Reads the settings' `editor_extra_font` for the editor's
    /// characters beyond SmileBASIC's font. A file that cannot be read
    /// leaves the bundled glyphs in use and the setting cleared, and the
    /// error is returned to be shown.
    pub fn load_extra_font(&mut self) -> std::result::Result<(), String> {
        let result = self.read_extra_font();
        self.editor_halves = None;
        self.apply_program_font();
        result
    }

    fn read_extra_font(&mut self) -> std::result::Result<(), String> {
        let loaded = match &self.settings.editor_extra_font {
            None => Ok(ExtraGlyphs::bundled()),
            Some(path) => ExtraGlyphs::load(std::path::Path::new(path))
                .map_err(|e| format!("Using the bundled editor font: {path}: {e}")),
        };
        self.editor_key = None;
        let result = match loaded {
            Ok(glyphs) => {
                self.extra_glyphs = Rc::new(glyphs);
                Ok(())
            }
            Err(e) => {
                self.extra_glyphs = Rc::new(ExtraGlyphs::bundled());
                self.settings.editor_extra_font = None;
                Err(e)
            }
        };
        self.extra_glyphs
            .set_bundled(self.settings.bundled_glyphs16);
        result
    }

    /// Draws the bundled 16-pixel glyphs from the set the settings'
    /// `bundled_glyphs16` picks: in the editor, in the fallback font and
    /// on its font page (GRP5), where only the pixels that differ change,
    /// and in the program font. Nothing changes if the set is the same.
    pub fn apply_bundled_glyphs(&mut self) {
        let set = self.settings.bundled_glyphs16;
        let font = self.font.with_bundled_set(set);
        if self.extra_glyphs.bundled_set() == set && font.is_none() {
            return;
        }
        self.extra_glyphs.set_bundled(set);
        self.editor_key = None;
        self.editor_halves = None;
        if let Some(font) = font {
            let before = self.font.page_image();
            self.font = font;
            let after = self.font.page_image();
            let mut page = match self.grp.page(FONT_PAGE) {
                [] => vec![0; after.len()],
                pixels => pixels.to_vec(),
            };
            for ((p, old), new) in page.iter_mut().zip(&before).zip(&after) {
                if old != new {
                    *p = *new;
                }
            }
            self.grp.set_page(FONT_PAGE, page);
        }
        self.apply_program_font();
    }

    /// A fingerprint of what [`Runtime::draw_editors`] draws from.
    fn editor_key(
        &self,
        editors: &[Editor; 2],
        active: usize,
        split: Option<bool>,
        panel: (bool, Option<u32>),
        blink: bool,
    ) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut state = std::hash::DefaultHasher::new();
        let s = &self.settings;
        (self.editor_fb.width(), self.editor_fb.height()).hash(&mut state);
        (s.editor_wrap, s.editor_number_tiles, s.editor_map).hash(&mut state);
        (s.editor_half_width, s.editor_font_first, s.bundled_glyphs16).hash(&mut state);
        (s.editor_font, &s.editor_colors, self.dimmed).hash(&mut state);
        (active, split, panel, blink).hash(&mut state);
        if split.is_some() {
            self.split_alpha().hash(&mut state);
        }
        let shown = match split {
            Some(_) => &editors[..],
            None => std::slice::from_ref(&editors[active]),
        };
        for editor in shown {
            let slot = editor.slot();
            (&self.slot_names[slot], self.slot_modified[slot]).hash(&mut state);
            editor.draw_key(&mut state);
        }
        state.finish()
    }

    /// How strongly, out of 256, the side of a split editor with the keys
    /// is outlined, fading out after it has them.
    fn split_alpha(&self) -> u32 {
        let age = self.frame.wrapping_sub(self.split_flash);
        match age.checked_sub(SPLIT_FLASH_HOLD) {
            None => 256,
            Some(fading) => 256 * SPLIT_FLASH_FADE.saturating_sub(fading) / SPLIT_FLASH_FADE,
        }
    }

    fn draw_editors(
        &mut self,
        editors: &mut [Editor; 2],
        active: usize,
        split: Option<bool>,
        (panel, top): (bool, Option<u32>),
        blink: bool,
    ) {
        let s = &self.settings;
        let first = s.editor_font_first;
        if s.editor_half_width
            && self.extra_glyphs.has_file()
            && self.editor_halves.as_ref().is_none_or(|(f, _)| *f != first)
        {
            let halves = HalfGlyphs::with_file(&self.font, Some(&self.extra_glyphs), first);
            self.editor_halves = Some((first, Rc::new(halves)));
        }
        let (w, h) = (self.editor_fb.width(), self.editor_fb.height());
        let reserve = if panel { h * PANEL_SHARE / 20 } else { 0 };
        self.panel_at = panel.then_some(h - reserve);
        let look = Look {
            wrap: s.editor_wrap,
            tiles: s.editor_number_tiles,
            map: s.editor_map,
            top,
            bottom: reserve,
            font: s.editor_font,
            colors: &s.editor_colors,
            extra: &self.extra_glyphs,
            first,
            half_width: s.editor_half_width,
            halves: (self.editor_halves.as_ref())
                .filter(|_| self.extra_glyphs.has_file())
                .map(|(_, h)| &**h),
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
            // The editor is darker too while a subprogram has the focus.
            if self.dimmed {
                dim(&mut self.editor_fb);
            }
            return;
        };
        // Two sides with a divider between them; only the upper one
        // leaves room for the menu bar.
        let length = if rows { h - reserve } else { w };
        let half = length.saturating_sub(SPLIT_DIVIDER) / 2;
        let menu = top.unwrap_or_else(|| menu_height(h));
        let second = length - half;
        self.split_at = Some(second);
        let side_size = if rows { (w, half) } else { (half, h) };
        if (self.side_fb.width(), self.side_fb.height()) != side_size {
            self.side_fb = Framebuffer::new(side_size.0, side_size.1);
        }
        for (side, editor) in editors.iter_mut().enumerate() {
            let name = name(editor);
            let blink = blink && side == active;
            let top = match (rows, side) {
                (true, 0) => Some(menu),
                (true, _) => Some(0),
                (false, _) => Some(menu),
            };
            // One above the other, the room is left below both.
            let bottom = if rows { 0 } else { reserve };
            let look = Look {
                top,
                bottom,
                ..look
            };
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
        for y in h - reserve..h {
            for x in 0..w {
                self.editor_fb.set(x, y, SPLIT_COLOR);
            }
        }
        let alpha = self.split_alpha();
        if alpha > 0 {
            // Below the menu bar, which only the upper side leaves room for.
            let at = if active == 0 { 0 } else { second };
            let (x0, y0) = if rows { (0, at) } else { (at, 0) };
            let top = if rows && active == 1 { y0 } else { menu };
            let (x1, y1) = (x0 + side_size.0, y0 + side_size.1);
            let thick = (s.editor_font as u32 / 4).max(1);
            for y in top..y1 {
                for x in x0..x1 {
                    let edge =
                        x < x0 + thick || x + thick >= x1 || y < top + thick || y + thick >= y1;
                    if let (true, Some(c)) = (edge, self.editor_fb.get(x, y)) {
                        self.editor_fb.set(x, y, blend(c, SPLIT_FLASH, alpha));
                    }
                }
            }
        }
        if self.dimmed {
            dim(&mut self.editor_fb);
        }
    }

    fn screen(&mut self, id: Option<i32>) -> Result<&mut TextScreen> {
        let id = match id {
            None => CONSOLE,
            Some(id) => screen_id(&Value::Int(id))?,
        };
        Ok(&mut self.text[id])
    }

    /// What calling `command` can do to the screens.
    fn sight(&self, command: Command, args: &[Value], want: Want) -> Sight {
        use Command::*;
        if command.unseen() || (want != Want::Outs(0) && command.reads()) {
            return Sight::Unseen;
        }
        // A DIALOG asks again every frame until answered; the frontend
        // shows it over the screens.
        if command == Dialog && self.dialog.is_some() {
            return Sight::Unseen;
        }
        if want != Want::Outs(0) {
            return Sight::Seen;
        }
        match command {
            // They change only the cells, which tell when they change, and
            // the cursor, which shows only while a line is typed.
            Attr | Cls | Color | Locate | Tfill | Tput if self.line.is_none() => Sight::Unseen,
            // What SPSET gives new sprites.
            Sppage | Splayer if args.len() == 1 => Sight::Unseen,
            Spchr | Spcolor | Sphide | Sphome | Splayer | Spofs | Sppage | Sprot | Spscale
            | Spshow => match args.first().map(crate::sp::sprite_id) {
                Some(Ok(i)) => Sight::Sprite(i),
                _ => Sight::Seen,
            },
            _ => Sight::Seen,
        }
    }

    /// Runs `command` as a call: a function called as a command with one
    /// OUT variable, such as `SPSET 242 OUT IX`, runs as the function, and
    /// a call no form takes is the count error SmileBASIC 4 gives for it.
    pub(crate) fn run_counted(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        let result = match self.run(command, args, want) {
            Err(e) if want == Want::Outs(1) && is_no_form(&e) => {
                self.run(command, args, Want::Value)
            }
            result => result,
        };
        result.map_err(|e| match is_no_form(&e) {
            true => no_form_error(command, args.len(), want.count()),
            false => e,
        })
    }

    pub(crate) fn run(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
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
                self.direct_only(ErrorKind::SyntaxError)?;
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
                            accept_first: false,
                        }
                    });
                    if !matches!(ask, Ask::Go) {
                        return Ok(vec![]);
                    }
                }
                for slot in slots {
                    self.drop_prg_edit(slot);
                    self.slots[slot].clear();
                    self.slot_changes[slot] += 1;
                    self.replaced_lines(slot, None);
                    self.slot_names[slot].clear();
                    self.slot_modified[slot] = false;
                }
                Ok(vec![])
            }
            // CONT: direct mode only, with the same Syntax error as CLEAR in a
            // program.
            (Command::Cont, Want::Outs(0)) if args.is_empty() => {
                self.direct_only(ErrorKind::SyntaxError)?;
                match self.stopped {
                    Stopped::Paused => self.request(Request::Cont),
                    _ => Err(ErrorKind::CantContinue.into()),
                }
            }
            // Unlike SmileBASIC 4, the program can go on afterwards. To
            // stop it as SmileBASIC 4 does, bring back the lines below.
            (Command::Backtrace, Want::Outs(0)) if args.is_empty() => {
                self.direct_only(ErrorKind::SyntaxError)?;
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
                self.direct_only(ErrorKind::SyntaxError)?;
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
                self.input.polled = true;
                self.input.keys_read.typed();
                let keyboard_only = match args.first() {
                    Some(flag) => opt_int(flag)?.is_some_and(|flag| flag != 0),
                    None => false,
                };
                let key = self.next_key_of(keyboard_only);
                Ok(vec![Value::Str(new_str(key.into_iter().collect()))])
            }
            (Command::Locate, Want::Outs(0)) => {
                let (id, rest) = with_screen(args, 2)?;
                let (x, y) = (opt_int(&rest[0])?, opt_int(&rest[1])?);
                match self.text[id].locate(x, y) {
                    Ok(()) => Ok(vec![]),
                    Err(n) => Err(Error::new(ErrorKind::OutOfRange).of_value(&rest[n])),
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
                | Command::Envlocate
                | Command::Envproject
                | Command::Envsave
                | Command::Envstat
                | Command::Envtype
                | Command::Subhide
                | Command::Subrun
                | Command::Subshow
                | Command::Substop
                | Command::Metaedit
                | Command::Metaload
                | Command::Metasave,
                _,
            ) => self.env_command(command, args, want),
            // SmileBASIC 4 checks the counts before the arguments.
            (Command::Dialog, Want::Outs(0)) if (1..=4).contains(&args.len()) => {
                self.dialog_command(command, args, want)
            }
            (Command::Dialog, Want::Value) if (2..=4).contains(&args.len()) => {
                self.dialog_command(command, args, want)
            }
            (Command::Dialog, _) => Err(no_form()),
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

    fn work(&mut self, work: sb_core::vm::Work) {
        if let Some(meter) = &mut self.meter {
            meter.heard(work);
        }
    }

    fn out_of_time(&self) -> bool {
        self.meter.as_ref().is_some_and(Meter::spent)
    }

    fn call(&mut self, id: u32, args: &[Value], want: Want) -> Result<Vec<Value>> {
        let command = COMMANDS[id as usize].1;
        if self.meter.is_some() {
            let cost = self.command_cost(command, args, want == Want::Value);
            if let Some(meter) = &mut self.meter {
                meter.charge(cost);
            }
        }
        let sight = self.sight(command, args, want);
        let before = match sight {
            Sight::Unseen => None,
            Sight::Sprite(i) => Some((i, self.sprites.list()[i].appearance())),
            Sight::Seen => {
                self.changed = true;
                None
            }
        };
        let result = self.run_counted(command, args, want);
        if let Some((i, before)) = before {
            self.changed |= self.sprites.list()[i].appearance() != before;
        }
        result.map_err(|e| command_error(e, COMMANDS[id as usize].0, args))
    }

    fn print(&mut self, screen: Option<i32>, text: &[u16]) -> Result<()> {
        if self.meter.is_some() {
            let s = self.screen(screen)?;
            let scrolled = crate::speed::scrolling(s.cursor(), s.size(), text);
            if let Some(meter) = &mut self.meter {
                meter.charge_print(text.len(), scrolled);
            }
        }
        // The cells tell when they change; the cursor shows while a line
        // is typed.
        self.changed |= self.line.is_some();
        self.screen(screen)?.print(text);
        Ok(())
    }

    fn print_tab(&mut self, screen: Option<i32>) -> Result<()> {
        self.screen(screen)?.tab();
        Ok(())
    }

    fn print_newline(&mut self, screen: Option<i32>) -> Result<()> {
        if self.meter.is_some() {
            let s = self.screen(screen)?;
            let scrolled = crate::speed::scrolling(s.cursor(), s.size(), &[10]);
            if let Some(meter) = &mut self.meter {
                meter.charge_print(0, scrolled);
            }
        }
        self.screen(screen)?.newline();
        Ok(())
    }

    fn input_start(&mut self) -> Result<()> {
        // Keys queued while the program read none, PUSHKEY's too, are not
        // typed into the line.
        self.keys.clear();
        self.start_line();
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
        // The name is checked before the slot.
        let file = file.map(String::from_utf16_lossy);
        if let Some(file) = &file {
            crate::files::check_exec_name(file)?;
        }
        let slot = slot_arg(Some(&Value::Int(slot)))?;
        if let Some(file) = &file {
            // A program or subprogram may not EXEC a file named with a
            // project, not even its own.
            if crate::files::names_project(file) {
                return Err(ErrorKind::CantAccessOtherProject.into());
            }
            // A file that cannot be loaded is Load failed, after the
            // dialog that says so (checked on hardware), or the notice
            // that stands for it. Back on the question over the slot's
            // unsaved edits sets RESULT to -1 and is Load failed too.
            // UNVERIFIED: the Load failed after Back.
            match self.load_asking("EXEC", file, slot) {
                None => {
                    self.retry = false;
                    return Ok(Exec::Wait);
                }
                Some(false) => return Err(ErrorKind::LoadFailed.into()),
                Some(true) => {}
            }
            if let Some(meter) = &mut self.meter {
                meter.charge_read(file, self.slots[slot].len() as u64, true);
            }
        }
        self.slot_edits.push((slot, crate::prg::SlotEdit::Compiled));
        if let Some(meter) = &mut self.meter {
            meter.charge_compile(&self.slots[slot], file.is_some());
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
            Callbacks::Sprite => (self.sprites.list().iter().enumerate().skip(from))
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
            let key = key & codes::CODE;
            let console = &mut self.text[CONSOLE];
            let line = self.line.as_mut().expect("checked above");
            // Up and Down stay the history's; Tab takes a word.
            if matches!(key, codes::UP | codes::DOWN) {
                line.completion = None;
            }
            // A control character that PUSHKEY gave is no key of the list's.
            let literal = key < 0x20 && key != codes::ENTER;
            if let Some(list) = line.completion.as_mut().filter(|_| !literal) {
                let outcome = list.key(key_value(key), options.is_some_and(|o| o.enter));
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
                    self.line_blink_from = self.system.frames;
                    continue;
                }
            }
            if matches!(key, codes::UP | codes::DOWN) {
                self.recall_line(key == codes::UP);
                self.line_blink_from = self.system.frames;
                continue;
            }
            if key == codes::ENTER {
                let line = self.line.take().expect("checked above");
                return Ok(Some(line.finish(console)));
            }
            match key {
                codes::COPY | codes::CUT => {
                    if let Some(text) = line.selected_text() {
                        self.clipboard.set_text(&String::from_utf16_lossy(&text));
                    }
                    if key == codes::CUT {
                        line.key(codes::DELETE);
                    }
                    line.drop_selection();
                    line.draw(console);
                    continue;
                }
                codes::PASTE => {
                    let text: Vec<u16> = self.clipboard.text().encode_utf16().collect();
                    line.replace_before(line.cursor, &text);
                    line.draw(console);
                    continue;
                }
                _ => {}
            }
            let listing = line.completion.take().is_some();
            if line.key(key) {
                self.line_blink_from = self.system.frames;
            }
            self.line_overwrite = line.overwrite;
            let looks = match key {
                codes::BACKSPACE => listing,
                0x1_0000.. => false,
                c => is_ident_char(c as u16) || c == 0x40 || c == 0x23,
            };
            line.draw(console);
            if let (true, Some(options), false) = (looks, options, self.line_quiet) {
                self.line_words(options);
            }
        }
        Ok(None)
    }
}

/// Beside a subprogram, the side without the focus shows a little
/// darker.
fn dim(fb: &mut Framebuffer) {
    darken(fb.pixels_mut());
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_core::vm::{Status, Vm};

    /// The error `line` gives after the setup line, if any.
    fn error_of(line: &str) -> Option<ErrorKind> {
        let source = format!("SPSET 0,0:DIM A[16]:Z=0:S$=\"A\"\n{line}");
        let mut rt = Runtime::for_tests(Font::blank());
        rt.begin_frame();
        let mut vm = Vm::new(sb_core::compile(&source, &rt).expect("compiles"));
        for _ in 0..100 {
            match vm.run(&mut rt, 1_000_000) {
                Err(e) => return Some(e.kind),
                Ok(Status::Ended) => return None,
                Ok(Status::Retry) => rt.begin_frame(),
                Ok(_) => {}
            }
        }
        None
    }

    // Each command's handler in SmileBASIC 4 (4.4.8) checks
    // the argument and OUT counts in its own order.
    #[test]
    fn wrong_counts_are_52_or_53_as_the_handler_checks_them() {
        const ARGS: Option<ErrorKind> = Some(ErrorKind::WrongNumberOfArguments);
        const OUTS: Option<ErrorKind> = Some(ErrorKind::WrongNumberOfOutArguments);
        let cases = [
            // The OUT count first.
            ("ACLS OUT Q0", OUTS),
            ("CLS OUT Q0", OUTS),
            ("LOCATE Z OUT Q0,Q1,Q2", OUTS),
            ("LOCATE 0,0,0,0,0", ARGS),
            ("LOCATE 0", ARGS),
            ("TOFS 0,30", ARGS),
            ("COLOR Z OUT Q0,Q1", OUTS),
            ("COLOR Z,Z OUT Q0", ARGS),
            ("BACKCOLOR OUT Q0,Q1", OUTS),
            ("BACKCOLOR Z OUT Q0", ARGS),
            ("Q=BACKCOLOR(Z)", ARGS),
            ("GCLIP OUT Q0,Q1,Q2,Q3,Q4", OUTS),
            ("BGMPLAY Z,Z,64,Z OUT Q0", OUTS),
            ("XSCREEN OUT Q0,Q1,Q2,Q3,Q4,Q5", OUTS),
            ("INKEY$ 1 OUT Q0,Q1", OUTS),
            ("Q$=INKEY$(1,2)", ARGS),
            ("SPSET Z,Z,Z,Z,16,16,1,Z OUT Q0,Q1", OUTS),
            ("ENVSTAT OUT Q0,Q1,Q2,Q3,Q4,Q5,Q6", OUTS),
            // The argument count first.
            ("DATE$ OUT Q0,Q1", OUTS),
            ("Q$=DATE$(1)", ARGS),
            ("DTREAD OUT Q0,Q1,Q2,Q3,Q4", OUTS),
            ("RANDOMIZE Z,Z OUT Q0", OUTS),
            ("RANDOMIZE Z,Z,Z OUT Q0", ARGS),
            ("DELETE S$ OUT Q0", OUTS),
            ("DELETE Z,Z,S$ OUT Q0", ARGS),
            ("MOUSE Z OUT Q0", OUTS),
            ("MOUSE Z,Z OUT Q0", ARGS),
            ("RNDF Z,Z OUT Q", OUTS),
            // A sprite first.
            ("SPROT Z OUT Q0,Q1", OUTS),
            ("SPROT OUT Q0,Q1", ARGS),
            ("SPOFS Z,Z OUT Q0,Q1,Q2,Q3", OUTS),
            ("SPOFS Z,Z,Z OUT Q0", ARGS),
            // DIALOG checks the counts before the arguments.
            ("DIALOG S$,Z,S$,Z,Z", ARGS),
            ("DIALOG S$,Z,S$,Z OUT Q0,Q1", OUTS),
            ("Q=DIALOG(S$)", ARGS),
        ];
        for (line, expected) in cases {
            assert_eq!(error_of(line), expected, "{line}");
        }
    }
}

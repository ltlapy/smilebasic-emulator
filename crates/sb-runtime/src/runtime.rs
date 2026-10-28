//! The [`Host`] that programs run against: text screens now, graphics,
//! sprites and sound later.

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;

use sb_core::bytecode::{Callbacks, Compiled, Literal, Want};
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::host::Host;
use sb_core::value::{Value, new_str};

use crate::compose::{self, Compose, LAYERS, Layer};
use crate::confirm::{Ask, Confirm, unsaved_lines};
use crate::editor::{Editor, Look, frame_size};
use crate::font::Font;
use crate::framebuffer::Framebuffer;
use crate::graphics::{Clip, Graphics};
use crate::input::Input;
use crate::screen::{Fade, ScreenMode};
pub use crate::settings::Settings;
use crate::sound::Sound;
use crate::sprites::{self, Sprites};
use crate::storage::Storage;
use crate::system::System;
use crate::text::{CONSOLE, FONT_PAGE, SCREENS, SPRITE_PAGE, TextScreen};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Accel,
    Acls,
    Animdef,
    Attr,
    Backcolor,
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
    Gyroa,
    Gyrosync,
    Gyrov,
    Inkey,
    Key,
    Keyboard,
    Layer,
    Lclip,
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
    Talk,
    Talkchk,
    Talkstop,
    Tanim,
    Tarray,
    Tblend,
    Tchk,
    Tcolor,
    Tcoord,
    Tcopy,
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
    Trot,
    Tsave,
    Tscale,
    Tscreen,
    Tshow,
    Tstart,
    Tstop,
    Tupdate,
    Tvar,
    Wavset,
    Wavseta,
    Xctrlstyle,
    Xscreen,
}

const COMMANDS: &[(&str, Command)] = &[
    ("ACCEL", Command::Accel),
    ("ACLS", Command::Acls),
    ("ANIMDEF", Command::Animdef),
    ("ATTR", Command::Attr),
    ("BACKCOLOR", Command::Backcolor),
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
    ("GYROA", Command::Gyroa),
    ("GYROSYNC", Command::Gyrosync),
    ("GYROV", Command::Gyrov),
    ("INKEY$", Command::Inkey),
    ("KEY", Command::Key),
    ("KEYBOARD", Command::Keyboard),
    ("LAYER", Command::Layer),
    ("LCLIP", Command::Lclip),
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
    ("TALK", Command::Talk),
    ("TALKCHK", Command::Talkchk),
    ("TALKSTOP", Command::Talkstop),
    ("TANIM", Command::Tanim),
    ("TARRAY", Command::Tarray),
    ("TBLEND", Command::Tblend),
    ("TCHK", Command::Tchk),
    ("TCOLOR", Command::Tcolor),
    ("TCOORD", Command::Tcoord),
    ("TCOPY", Command::Tcopy),
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
    ("TROT", Command::Trot),
    ("TSAVE", Command::Tsave),
    ("TSCALE", Command::Tscale),
    ("TSCREEN", Command::Tscreen),
    ("TSHOW", Command::Tshow),
    ("TSTART", Command::Tstart),
    ("TSTOP", Command::Tstop),
    ("TUPDATE", Command::Tupdate),
    ("TVAR", Command::Tvar),
    ("WAVSET", Command::Wavset),
    ("WAVSETA", Command::Wavseta),
    ("XCTRLSTYLE", Command::Xctrlstyle),
    ("XSCREEN", Command::Xscreen),
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
    pub input: Input,
    pub(crate) system: System,
    pub sound: Sound,
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
    /// The slot whose program is running; `None` in direct mode.
    pub(crate) running_slot: Option<usize>,
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
    /// READ's place in the program's DATA, during a command.
    data_pos: Cell<usize>,
    pub layers: [Layer; LAYERS],
    /// BACKCOLOR: what shows behind every layer.
    pub back_color: u32,
    pub(crate) fade: Fade,
    /// Where a layer that is composed is drawn first.
    scratch: Framebuffer,
    /// XSCREEN, and what ACLS goes back to.
    pub(crate) mode: ScreenMode,
    startup_mode: ScreenMode,
    /// The editor draws at a multiple of the start-up resolution, for its
    /// font and size (see [`frame_size`]).
    editor_fb: Framebuffer,
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
        _ => Err(illegal()),
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
            frame: 0,
            direct: false,
            requests: Vec::new(),
            slots: vec![String::new(); SLOTS],
            slot_names: vec![String::new(); SLOTS],
            slot_modified: vec![false; SLOTS],
            running_slot: None,
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
            notices: Vec::new(),
            program: None,
            data_pos: Cell::new(0),
            layers: [Layer::default(); LAYERS],
            back_color: 0xFF00_0000,
            fade: Fade::default(),
            scratch: Framebuffer::new(width, height),
            mode,
            startup_mode: mode,
            editor_fb: Framebuffer::new(width, height),
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
    pub(crate) fn label_data(&self, label: &Value) -> Result<DataReader<'_>> {
        let text =
            String::from_utf16_lossy(&label.clone().into_str()?.borrow()).to_ascii_uppercase();
        let name = text.split_once(':').map_or(text.as_str(), |(_, rest)| rest);
        let name = name.strip_prefix('@').unwrap_or(name);
        let program = self.program.clone().unwrap_or_default();
        let pos = *program
            .data_labels
            .get(name)
            .ok_or_else(|| Error::with_detail(ErrorKind::UndefinedLabel, format!("@{name}")))?;
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
            ),
        }
    }

    pub fn font(&self) -> &Font {
        &self.font
    }

    /// The line being typed for INPUT or in direct mode, if any.
    pub(crate) fn typed_line(&self) -> Option<Vec<u16>> {
        self.line.clone()
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
        fb.clear(self.back_color | 0xFF00_0000);
        for layer in (0..LAYERS as i32).rev() {
            let mut in_layer = elements.iter().filter(|e| e.0 == layer).peekable();
            if in_layer.peek().is_none() {
                continue;
            }
            let settings = self.fine(&self.layers[layer as usize]);
            let target = match settings.compose {
                Compose::Direct => &mut fb,
                _ => {
                    scratch.clear(0);
                    &mut scratch
                }
            };
            for &(_, _, _, _, element) in in_layer {
                self.draw_element(target, element, &settings, cursor);
            }
            if settings.compose != Compose::Direct {
                compose::compose(&mut fb, &scratch, settings.compose, settings.color);
            }
        }
        self.fade.cover(fb.pixels_mut());
        self.fb = fb;
        self.scratch = scratch;
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
    pub(crate) fn render_editor(&mut self, editor: &mut Editor) -> &Framebuffer {
        self.frame = self.frame.wrapping_add(1);
        let slot = editor.slot();
        let mut name = self.slot_names[slot].clone();
        // The slot has edits that were not saved.
        if self.slot_modified[slot] || editor.is_modified() {
            name.push_str(" *");
        }
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
            font: s.editor_font,
            colors: &s.editor_colors,
        };
        editor.draw(&mut self.editor_fb, &self.font, &name, look, blink);
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
                | Command::Gsample
                | Command::Gsave
                | Command::Gtarget
                | Command::Gtri,
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
                | Command::Maincnt
                | Command::Millisec
                | Command::Randomize
                | Command::Rnd
                | Command::Rndf
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

    // Checked on hardware: CALLIDX() is 4096 after CALL SPRITE and 5
    // after CALL TEXT.
    fn callbacks_end(&self, kind: Callbacks) -> i32 {
        match kind {
            Callbacks::Sprite => sprites::SPRITES as i32,
            Callbacks::Text => SCREENS as i32,
        }
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

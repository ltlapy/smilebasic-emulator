//! The [`Host`] that programs run against: text screens now, graphics,
//! sprites and sound later.

use std::collections::VecDeque;
use std::rc::Rc;

use sb_core::bytecode::{Callbacks, Compiled, Literal, Want};
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::host::Host;
use sb_core::value::{Value, new_str};

use crate::compose::{self, Compose, LAYERS, Layer};
use crate::confirm::{Ask, Confirm, unsaved_lines};
use crate::editor::Editor;
use crate::font::Font;
use crate::framebuffer::Framebuffer;
use crate::graphics::{Clip, Graphics};
use crate::screen::ScreenMode;
pub use crate::settings::Settings;
use crate::sprites::{self, Sprites};
use crate::storage::Storage;
use crate::text::{CONSOLE, FONT_PAGE, SCREENS, SPRITE_PAGE, TextScreen};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Acls,
    Animdef,
    Attr,
    Backcolor,
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
    Layer,
    Lclip,
    Lmatrix,
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
    Tblend,
    Tcolor,
    Thide,
    Thome,
    Tlayer,
    Tofs,
    Tpage,
    Trot,
    Tscale,
    Tscreen,
    Tshow,
    Xscreen,
}

const COMMANDS: &[(&str, Command)] = &[
    ("ACLS", Command::Acls),
    ("ANIMDEF", Command::Animdef),
    ("ATTR", Command::Attr),
    ("BACKCOLOR", Command::Backcolor),
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
    ("LAYER", Command::Layer),
    ("LCLIP", Command::Lclip),
    ("LMATRIX", Command::Lmatrix),
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
    ("TBLEND", Command::Tblend),
    ("TCOLOR", Command::Tcolor),
    ("THIDE", Command::Thide),
    ("THOME", Command::Thome),
    ("TLAYER", Command::Tlayer),
    ("TOFS", Command::Tofs),
    ("TPAGE", Command::Tpage),
    ("TROT", Command::Trot),
    ("TSCALE", Command::Tscale),
    ("TSCREEN", Command::Tscreen),
    ("TSHOW", Command::Tshow),
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
    pub layers: [Layer; LAYERS],
    /// BACKCOLOR: what shows behind every layer.
    pub back_color: u32,
    /// Where a layer that is composed is drawn first.
    scratch: Framebuffer,
    /// XSCREEN, and what ACLS goes back to.
    pub(crate) mode: ScreenMode,
    startup_mode: ScreenMode,
    /// The editor keeps the start-up resolution.
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
            layers: [Layer::default(); LAYERS],
            back_color: 0xFF00_0000,
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
    // UNVERIFIED: that SPPAGE, SPLAYER, GSAMPLE and the text screens'
    // other settings go back too; the manual's equivalent program leaves
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
        self.fb = fb;
        self.scratch = scratch;
        &self.fb
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
        editor.draw(
            &mut self.editor_fb,
            &self.font,
            &name,
            self.settings.editor_wrap,
            blink,
        );
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
                Command::Acls
                | Command::Attr
                | Command::Backcolor
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

    // UNVERIFIED: sprites are called in id order, and the list is fixed
    // when CALL SPRITE starts.
    fn callbacks(&mut self, kind: Callbacks) -> Vec<(i32, Vec<u16>)> {
        match kind {
            Callbacks::Sprite => (self.sprites.list.iter().enumerate())
                .filter(|(_, s)| s.used)
                .filter_map(|(i, s)| Some((i as i32, s.func.clone()?)))
                .collect(),
            // TFUNC is not implemented yet.
            Callbacks::Text => Vec::new(),
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

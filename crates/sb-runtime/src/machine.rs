//! The whole computer: direct mode, the editor, the program slots, and the
//! program running on the [`Runtime`]. Frontends feed it keys, call
//! [`Machine::frame`] once per frame and show [`Machine::render`].

use std::collections::HashMap;

use sb_core::error::{Error, ErrorKind};
use sb_core::host::Host;
use sb_core::value::Value;
use sb_core::vm::{Status, Vm};

use crate::editor::Editor;
use crate::framebuffer::Framebuffer;
use crate::runtime::{Request, Runtime, SLOTS};
use crate::text::CONSOLE;

/// UNVERIFIED: instructions per frame for code that does not VSYNC;
/// SmileBASIC 4's real speed has not been measured.
pub const FRAME_BUDGET: u32 = 200_000;
/// Shown by FREEMEM on SmileBASIC 4.4.9 right after start-up.
// TODO: report the memory programs can really use, together with FREEMEM,
// once memory such as graphic pages is accounted for.
pub const FREE_MEMORY: u32 = 134_216_720;

pub struct Machine {
    pub rt: Runtime,
    vm: Option<Vm>,
    /// Frames left before the VM runs again.
    wait: u32,
    /// Globals of the last program or direct-mode line, kept for the next
    /// direct-mode line.
    vars: HashMap<String, Value>,
    pub editor: Editor,
    /// Whether the editor is shown instead of direct mode.
    editing: bool,
    /// The slot whose program is running, if one is.
    running_slot: Option<usize>,
    /// Where the last program error happened, for LIST ERR: the slot and
    /// line.
    last_error: Option<(usize, u32)>,
}

/// What a direct-mode LIST asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum List {
    /// A slot (the edited one when `None`) and a 1-based line.
    At(Option<usize>, Option<usize>),
    Error,
}

/// Reads LIST, LIST line, LIST slot:line and LIST ERR. Returns `None` for
/// lines that are not a LIST; LIST is handled here because its `:` is not
/// the statement separator.
// UNVERIFIED: only number literals are accepted, not expressions.
fn parse_list(line: &str) -> Option<Result<List, Error>> {
    let line = line.trim();
    let head = line.get(..4)?;
    let rest = &line[4..];
    if !head.eq_ignore_ascii_case("LIST")
        || rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    {
        return None;
    }
    let rest = rest.trim();
    let number = |s: &str| s.trim().parse::<usize>().ok();
    let list = if rest.is_empty() {
        Some(List::At(None, None))
    } else if rest.eq_ignore_ascii_case("ERR") {
        Some(List::Error)
    } else if let Some((slot, line)) = rest.split_once(':') {
        number(slot)
            .zip(number(line))
            .map(|(s, l)| List::At(Some(s), Some(l)))
    } else {
        number(rest).map(|l| List::At(None, Some(l)))
    };
    Some(match list {
        Some(List::At(Some(slot), _)) if slot >= SLOTS => Err(Error::with_detail(
            ErrorKind::OutOfRange,
            format!("slot {slot}"),
        )),
        Some(list) => Ok(list),
        None => Err(Error::with_detail(ErrorKind::SyntaxError, "LIST")),
    })
}

/// ASCII to full-width forms, as SmileBASIC writes its own messages.
fn full_width(text: &str) -> Vec<u16> {
    text.encode_utf16()
        .map(|c| match c {
            0x20 => 0x3000,
            0x21..=0x7E => c + 0xFEE0,
            _ => c,
        })
        .collect()
}

impl Machine {
    pub fn new(rt: Runtime) -> Machine {
        let (w, h) = rt.text[CONSOLE].size();
        Machine {
            rt,
            vm: None,
            wait: 0,
            vars: HashMap::new(),
            editor: Editor::new(w * 8, h * 8),
            editing: false,
            running_slot: None,
            last_error: None,
        }
    }

    /// Prints the start-up message and waits for a direct-mode line.
    pub fn boot(&mut self) {
        let version = env!("CARGO_PKG_VERSION");
        for line in [
            format!("SmileBASIC emulator ver {version}"),
            "(SmileBASIC 4.4.9 compatible)".to_string(),
            format!("{FREE_MEMORY} bytes free"),
        ] {
            self.print_line(&full_width(&line));
        }
        self.rt.text[CONSOLE].newline();
        self.ready();
    }

    pub fn slot(&self, slot: usize) -> &str {
        &self.rt.slots[slot]
    }

    pub fn set_slot(&mut self, slot: usize, source: String) {
        self.rt.slots[slot] = source;
    }

    /// Whether a program or direct-mode line is executing.
    pub fn is_running(&self) -> bool {
        self.vm.is_some()
    }

    pub fn is_editing(&self) -> bool {
        self.editing
    }

    /// Switches between direct mode and the editor, as F4 does. Entering
    /// the editor shows the last error's line once, like LIST ERR.
    pub fn toggle_editor(&mut self) {
        if self.vm.is_some() {
            return;
        }
        if self.editing {
            self.close_editor();
        } else {
            self.list_error();
        }
    }

    /// Opens the editor at the last error, or where it was left.
    fn list_error(&mut self) {
        match self.last_error.take() {
            Some((slot, line)) => self.open_editor(slot, Some(line as usize)),
            None => self.open_editor(self.editor.slot(), None),
        }
    }

    fn open_editor(&mut self, slot: usize, line: Option<usize>) {
        self.editor.open(slot, &self.rt.slots[slot], line);
        self.editing = true;
    }

    fn close_editor(&mut self) {
        if let Some(source) = self.editor.close() {
            let slot = self.editor.slot();
            self.rt.slots[slot] = source;
            self.rt.slot_modified[slot] = true;
        }
        self.editing = false;
    }

    /// Slots with edits that were not saved, including the one open in the
    /// editor.
    pub fn unsaved_slots(&self) -> Vec<usize> {
        (0..SLOTS)
            .filter(|&slot| {
                self.rt.slot_modified[slot]
                    || (self.editing && self.editor.slot() == slot && self.editor.is_modified())
            })
            .collect()
    }

    /// Starts the program in a slot, as RUN does.
    pub fn run_slot(&mut self, slot: usize) {
        self.vars.clear();
        self.start(&self.rt.slots[slot].clone(), Some(slot));
    }

    /// Stops the running program or direct-mode line, as the Break key
    /// does, and returns to direct mode.
    pub fn stop(&mut self) {
        let Some(vm) = self.vm.take() else {
            return;
        };
        self.vars = vm.globals().collect();
        self.rt.take_requests();
        // UNVERIFIED: the wording of SmileBASIC 4's break message.
        let text: Vec<u16> = format!("Break in {}", vm.line()).encode_utf16().collect();
        self.print_line(&text);
        self.ready();
    }

    /// Text typed on the keyboard, as the OS reports it.
    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            let c = if self.rt.settings.upper_case_letters {
                swap_case(c)
            } else {
                c
            };
            // SmileBASIC strings hold only the Basic Multilingual Plane.
            if let Ok(code) = u16::try_from(u32::from(c)) {
                self.rt.key_press(code);
            }
        }
    }

    /// Runs one frame: the program, the editor, or direct-mode line
    /// editing.
    pub fn frame(&mut self) {
        if self.editing {
            while let Some(key) = self.rt.next_key() {
                self.editor.key(key);
            }
            return;
        }
        let Some(vm) = &mut self.vm else {
            if let Ok(Some(line)) = self.rt.input_poll() {
                self.direct(&String::from_utf16_lossy(&line));
            }
            return;
        };
        self.wait = self.wait.saturating_sub(1);
        if self.wait > 0 {
            return;
        }
        match vm.run(&mut self.rt, FRAME_BUDGET) {
            Ok(Status::Running) => {}
            Ok(Status::Wait { frames, .. }) => self.wait = frames,
            Ok(Status::Ended) => self.finish(None),
            Err(e) => self.finish(Some(e)),
        }
    }

    pub fn render(&mut self) -> &Framebuffer {
        if self.editing {
            self.rt.render_editor(&mut self.editor)
        } else {
            self.rt.render()
        }
    }

    /// Runs a line typed in direct mode.
    fn direct(&mut self, line: &str) {
        if line.trim().is_empty() {
            self.rt.input_start().expect("console input");
            return;
        }
        match parse_list(line) {
            Some(Ok(list)) => {
                match list {
                    List::At(slot, line) => {
                        self.open_editor(slot.unwrap_or(self.editor.slot()), line)
                    }
                    List::Error => self.list_error(),
                }
                self.rt.input_start().expect("console input");
            }
            Some(Err(e)) => {
                self.report(&e.at(1));
                self.ready();
            }
            None => self.start(line, None),
        }
    }

    /// Starts a program from `slot`, or a direct-mode line.
    fn start(&mut self, source: &str, slot: Option<usize>) {
        self.rt.cancel_input();
        self.rt.direct = slot.is_none();
        self.running_slot = slot;
        match sb_core::compile(source, &self.rt) {
            Ok(program) => {
                self.vm = Some(Vm::with_globals(program, &self.vars));
                self.wait = 0;
            }
            Err(e) => {
                self.last_error = slot.map(|slot| (slot, e.line)).or(self.last_error);
                self.report(&e);
                self.ready();
            }
        }
    }

    /// The program or line ended, by itself or with an error.
    fn finish(&mut self, error: Option<Error>) {
        let vm = self.vm.take().expect("finish follows a run");
        self.vars = vm.globals().collect();
        if let Some(e) = error {
            if let Some(slot) = self.running_slot {
                self.last_error = Some((slot, e.line));
            }
            self.report(&e);
        }
        self.running_slot = None;
        let mut run = None;
        for request in self.rt.take_requests() {
            match request {
                Request::Run(slot) => run = Some(slot),
                Request::Clear => self.vars.clear(),
            }
        }
        match run {
            Some(slot) => self.run_slot(slot),
            None => self.ready(),
        }
    }

    /// Shows OK and waits for the next direct-mode line.
    fn ready(&mut self) {
        self.rt.direct = true;
        self.print_line(&"OK".encode_utf16().collect::<Vec<_>>());
        self.rt.input_start().expect("console input");
    }

    fn report(&mut self, error: &Error) {
        let text: Vec<u16> = error.to_string().encode_utf16().collect();
        self.print_line(&text);
    }

    /// Prints on a line of its own on the console.
    fn print_line(&mut self, text: &[u16]) {
        let console = &mut self.rt.text[CONSOLE];
        if console.cursor().0 != 0 {
            console.newline();
        }
        console.print(text);
        console.newline();
    }
}

/// Swaps the case of ASCII letters, so that unshifted keys type capitals.
fn swap_case(c: char) -> char {
    if c.is_ascii_lowercase() {
        c.to_ascii_uppercase()
    } else {
        c.to_ascii_lowercase()
    }
}

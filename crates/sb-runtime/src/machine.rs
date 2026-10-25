//! The whole computer: direct mode, the program slots, and the program
//! running on the [`Runtime`]. Frontends feed it keys, call
//! [`Machine::frame`] once per frame and show [`Machine::render`].

use std::collections::HashMap;

use sb_core::host::Host;
use sb_core::value::Value;
use sb_core::vm::{Status, Vm};

use crate::framebuffer::Framebuffer;
use crate::runtime::{Request, Runtime};
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
        Machine {
            rt,
            vm: None,
            wait: 0,
            vars: HashMap::new(),
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

    /// Starts the program in a slot, as RUN does.
    pub fn run_slot(&mut self, slot: usize) {
        self.vars.clear();
        self.start(&self.rt.slots[slot].clone(), false);
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

    /// Runs one frame: the program, or direct-mode line editing.
    pub fn frame(&mut self) {
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
        self.rt.render()
    }

    /// Runs a line typed in direct mode.
    fn direct(&mut self, line: &str) {
        if line.trim().is_empty() {
            self.rt.input_start().expect("console input");
            return;
        }
        self.start(line, true);
    }

    fn start(&mut self, source: &str, direct: bool) {
        self.rt.cancel_input();
        self.rt.direct = direct;
        match sb_core::compile(source, &self.rt) {
            Ok(program) => {
                self.vm = Some(Vm::with_globals(program, &self.vars));
                self.wait = 0;
            }
            Err(e) => {
                self.report(&e);
                self.ready();
            }
        }
    }

    /// The program or line ended, by itself or with an error.
    fn finish(&mut self, error: Option<sb_core::error::Error>) {
        let vm = self.vm.take().expect("finish follows a run");
        self.vars = vm.globals().collect();
        if let Some(e) = error {
            self.report(&e);
        }
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

    fn report(&mut self, error: &sb_core::error::Error) {
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

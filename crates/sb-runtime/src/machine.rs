//! The whole computer: direct mode, the editor, the program slots, and the
//! program running on the [`Runtime`]. Frontends feed it keys, call
//! [`Machine::frame`] once per frame and show [`Machine::render`].

use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::time::{Duration, Instant};

use sb_core::error::{Error, ErrorKind};
use sb_core::host::Host;
use sb_core::value::Value;
use sb_core::vm::{Call, Status, Step, Vm};

use crate::complete::Options;
use crate::editor::Editor;
use crate::framebuffer::Framebuffer;
use crate::runtime::{Request, Runtime, SLOTS, Stopped, keys};
use crate::screen::ScreenMode;
use crate::storage;
use crate::text::CONSOLE;

/// Instructions a frame runs for code that does not VSYNC, when the
/// frame has no time limit (see [`Machine::time_limit`]), so that tests
/// and headless runs come out the same each time.
pub const FRAME_BUDGET: u32 = 1_000_000;
/// With a time limit, how many instructions run between looks at the
/// clock.
const SLICE: u32 = 20_000;
/// Shown by FREEMEM on SmileBASIC 4.4.9 right after start-up.
pub const FREE_MEMORY: u32 = sb_core::memory::TOTAL;

pub struct Machine {
    pub rt: Runtime,
    vm: Option<Vm>,
    /// Frames left before the VM runs again.
    wait: u32,
    /// Frames run so far.
    frame: u64,
    /// The frame the last VSYNC waited for, which the next one counts
    /// from.
    vsync_at: u64,
    /// Globals of the last program and the direct-mode lines since, kept
    /// for the next direct-mode line; each line adds its own.
    vars: HashMap<String, Value>,
    /// The editor's two sides. Without a split display only the active
    /// one is shown. When both show the same slot, the other one mirrors
    /// the active one, which owns the text and its undo history.
    editors: [Editor; 2],
    active: usize,
    split: bool,
    /// The sides are one above the other rather than side by side.
    split_rows: bool,
    /// Whether the editor is shown instead of direct mode.
    editing: bool,
    /// The slot whose program is running, if one is.
    running_slot: Option<usize>,
    /// Where the last program error happened, for LIST ERR and F4: the
    /// slot, line and column of the statement.
    last_error: Option<(usize, u32, u32)>,
    /// The program that STOP, the + button or an error stopped, kept for
    /// CONT; `rt.stopped` says which.
    halted: Option<Vm>,
    /// How long a frame may run code that does not VSYNC: programs run
    /// as fast as the computer allows rather than at SmileBASIC 4's
    /// speed, which no program needs. `None` runs [`FRAME_BUDGET`]
    /// instructions instead.
    pub time_limit: Option<Duration>,
    /// Lines with breakpoints, by slot. They last for the session.
    breakpoints: Vec<BTreeSet<u32>>,
}

/// How a stopped program goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resume {
    Cont,
    /// TRACE: one statement, naming its line.
    Trace,
    /// A debugger's step, stopping before the next statement.
    Step(Step),
    /// On until the slot's line, or a breakpoint, is about to run.
    RunTo(usize, u32),
}

/// What stopped a program: the + button, or STOP and TRACE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Halt {
    User,
    Stop,
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
            frame: 0,
            vsync_at: 0,
            vars: HashMap::new(),
            editors: [Editor::new(w * 8, h * 8), Editor::new(w * 8, h * 8)],
            active: 0,
            split: false,
            split_rows: false,
            editing: false,
            running_slot: None,
            last_error: None,
            halted: None,
            breakpoints: vec![BTreeSet::new(); SLOTS],
            time_limit: None,
        }
    }

    /// Prints the start-up message and waits for a direct-mode line.
    pub fn boot(&mut self) {
        // KEY starts from the settings; programs change only this run's.
        // UNVERIFIED: whether KEY in a program changes the settings too.
        let keys = &self.rt.settings.function_keys;
        self.rt.input.function_keys = keys.clone().map(|k| k.encode_utf16().collect());
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
            Some((slot, line, col)) => {
                self.open_editor(slot, Some((line as usize, col as usize)));
            }
            None => self.open_editor(self.editor().slot(), None),
        }
    }

    /// The side of the editor that takes the keys.
    pub fn editor(&self) -> &Editor {
        &self.editors[self.active]
    }

    fn other(&self) -> usize {
        1 - self.active
    }

    /// Whether the other side of a split display shows the active side's
    /// slot.
    fn mirrors(&self) -> bool {
        self.split && self.editors[0].slot() == self.editors[1].slot()
    }

    /// Opens the editor on a slot, from direct mode or from another slot
    /// (Ctrl+1 to Ctrl+6, like SmileBASIC 4's EDIT0 to EDIT3). A slot that
    /// the other side of a split display shows gets the keys there.
    pub fn edit_slot(&mut self, slot: usize) {
        if self.vm.is_some() || slot >= SLOTS {
            return;
        }
        if self.editing {
            if self.editor().slot() == slot {
                return;
            }
            if self.split && self.editors[self.other()].slot() == slot {
                self.switch_side();
                return;
            }
            self.leave_slot();
        }
        self.open_editor(slot, None);
    }

    /// Lets the active side go of its slot, which the other side keeps
    /// if it shows it too.
    fn leave_slot(&mut self) {
        let a = self.active;
        if self.mirrors() {
            let [first, second] = &mut self.editors;
            let (active, other) = if a == 0 {
                (first, second)
            } else {
                (second, first)
            };
            other.take_document(active);
            active.close();
        } else {
            self.close_side(a);
        }
    }

    fn open_editor(&mut self, slot: usize, at: Option<(usize, usize)>) {
        self.rt.edit_slot = slot;
        let (a, o) = (self.active, self.other());
        if self.split && !self.editing {
            // Back from direct mode, the other side shows its slot again.
            let shown = self.editors[o].slot();
            self.editors[o].open(shown, &self.rt.slots[shown], None);
        }
        if self.split && self.editing && self.editors[o].slot() == slot {
            let text = self.editors[o].text();
            let [first, second] = &mut self.editors;
            let (active, other) = if a == 0 {
                (first, second)
            } else {
                (second, first)
            };
            active.open(slot, &text, at);
            active.take_document(other);
        } else {
            self.editors[a].open(slot, &self.rt.slots[slot], at);
        }
        self.editing = true;
    }

    /// Closes one side, keeping its changes in the slot.
    fn close_side(&mut self, side: usize) {
        if let Some(source) = self.editors[side].close() {
            let slot = self.editors[side].slot();
            self.rt.slots[slot] = source;
            self.rt.slot_modified[slot] = true;
        }
    }

    fn close_editor(&mut self) {
        let other = self.other();
        if self.mirrors() {
            // Only its place is kept; the active side has the text.
            self.editors[other].close();
        } else if self.split {
            self.close_side(other);
        }
        self.close_side(self.active);
        self.editing = false;
    }

    /// Splits the editor in two sides side by side, both showing the
    /// active slot at first, then one above the other, then goes back to
    /// one (Ctrl+/).
    pub fn toggle_split(&mut self) {
        if !self.editing {
            return;
        }
        let other = self.other();
        if self.split && !self.split_rows {
            self.split_rows = true;
            return;
        }
        if self.split {
            self.split_rows = false;
            if self.mirrors() {
                self.editors[other].close();
            } else {
                self.close_side(other);
            }
            self.split = false;
            return;
        }
        let slot = self.editor().slot();
        let text = self.editor().text();
        self.editors[other].open(slot, &text, None);
        self.split = true;
    }

    pub fn is_split(&self) -> bool {
        self.split
    }

    /// Whether a split display has its sides one above the other.
    pub fn is_split_rows(&self) -> bool {
        self.split && self.split_rows
    }

    /// Which side of a split display takes the keys: 0 for the left.
    pub fn active_side(&self) -> usize {
        self.active
    }

    /// Gives the keys to the other side of a split display (Ctrl+Tab).
    pub fn switch_side(&mut self) {
        if !self.split || !self.editing {
            return;
        }
        // Keys typed so far go to the side they were typed on.
        self.edit();
        // Only the side with the keys completes and hints.
        self.editors[self.active].complete = None;
        let (a, o) = (self.active, self.other());
        if self.mirrors() {
            let [first, second] = &mut self.editors;
            let (active, other) = if a == 0 {
                (first, second)
            } else {
                (second, first)
            };
            other.take_document(active);
        }
        self.active = o;
        self.rt.edit_slot = self.editors[o].slot();
    }

    /// A press of the mouse on the editor's frame: on the other side of a
    /// split display, it gets the keys first.
    pub fn editor_mouse_down(&mut self, x: i32, y: i32, extend: bool) {
        let along = if self.split_rows { y } else { x };
        let side = match self.rt.split_at {
            Some(at) if self.split && along >= at as i32 => 1,
            _ => 0,
        };
        if self.split && side != self.active {
            self.switch_side();
        }
        let (x, y) = self.on_side(x, y);
        self.edit().mouse_down(x, y, extend);
    }

    /// The mouse moved with its button held since the press.
    pub fn editor_mouse_drag(&mut self, x: i32, y: i32) {
        let (x, y) = self.on_side(x, y);
        self.edit().mouse_drag(x, y);
    }

    /// A point of the editor's frame on the active side's.
    fn on_side(&self, x: i32, y: i32) -> (i32, i32) {
        match self.rt.split_at {
            Some(at) if self.split && self.active == 1 && self.split_rows => (x, y - at as i32),
            Some(at) if self.split && self.active == 1 => (x - at as i32, y),
            _ => (x, y),
        }
    }

    /// The slot the editor shows, or last showed.
    pub fn edited_slot(&self) -> usize {
        self.rt.edit_slot
    }

    /// Saves the edited slot under a name, editor changes included.
    pub fn save_edited(&mut self, name: &str) -> storage::Result<()> {
        let slot = self.rt.edit_slot;
        if self.editing && self.editor().is_modified() {
            self.rt.slots[slot] = self.editor().text();
        }
        self.rt.save_slot_as(name, slot)?;
        if self.editing {
            let editor = &mut self.editors[self.active];
            editor.mark_saved();
            editor.set_note(format!("Saved {}", self.rt.slot_names[slot]));
        }
        Ok(())
    }

    /// Loads a file into the edited slot, dropping its changes.
    pub fn load_edited(&mut self, name: &str) -> storage::Result<()> {
        let slot = self.rt.edit_slot;
        let editing = self.editing;
        if editing {
            self.close_editor();
        }
        let outcome = self.rt.load_slot_from(name, slot);
        if editing {
            self.open_editor(slot, None);
        }
        outcome
    }

    /// Slots with edits that were not saved, including the ones open in
    /// the editor.
    pub fn unsaved_slots(&self) -> Vec<usize> {
        let shown = |side: usize| self.editing && (side == self.active || self.split);
        (0..SLOTS)
            .filter(|&slot| {
                self.rt.slot_modified[slot]
                    || (0..2).any(|side| {
                        let e = &self.editors[side];
                        shown(side) && e.slot() == slot && e.is_modified()
                    })
            })
            .collect()
    }

    /// Starts the program in a slot, as RUN does.
    pub fn run_slot(&mut self, slot: usize) {
        // Running a program ends PRGEDIT's editing.
        self.rt.prg_edit = None;
        self.vars.clear();
        self.rt.clear_vars();
        // Each program starts with the mouse cursor hidden; END leaves it
        // as the program set it (checked on hardware).
        self.rt.system.mouse_cursor = false;
        self.start(&self.rt.slots[slot].clone(), Some(slot));
    }

    /// Loads the current project's TXT file `name` into a slot, for the
    /// project manager. A running program stops first.
    pub fn load_slot(&mut self, slot: usize, name: &str) -> storage::Result<()> {
        self.stop();
        let editing = self.editing;
        if editing {
            self.close_editor();
        }
        let outcome = self.rt.load_file(name, slot);
        if editing {
            self.open_editor(slot, None);
        }
        outcome
    }

    /// Loads the current project's `name` into PRG0 and runs it, as
    /// SmileBASIC 4's menu runs a project's MAIN.PRG.
    pub fn run_file(&mut self, name: &str) -> storage::Result<()> {
        self.stop();
        if self.editing {
            self.close_editor();
        }
        self.rt.load_file(name, 0)?;
        self.run_slot(0);
        Ok(())
    }

    /// The + button (F5): stops a running program, or jumps to its ON
    /// BREAK GOTO label, or runs PRG0 from direct mode or the editor.
    pub fn sysreq(&mut self) {
        if let Some(vm) = &mut self.vm {
            if vm.take_break(&mut self.rt) {
                // Whatever the program waited for is left behind.
                self.wait = 0;
                self.rt.cancel_input();
                self.rt.cancel_confirm();
            } else {
                self.stop();
            }
            return;
        }
        if self.editing {
            self.close_editor();
        }
        self.run_slot(0);
    }

    /// Stops the running program or direct-mode line, as the Break key
    /// does, and returns to direct mode.
    pub fn stop(&mut self) {
        self.stop_on(None, Halt::User);
    }

    /// Stops, naming `line` rather than the one about to run.
    fn stop_on(&mut self, line: Option<u32>, how: Halt) {
        let Some(vm) = self.vm.take() else {
            return;
        };
        self.vars.extend(vm.globals());
        let program = self.running_slot.is_some();
        if program {
            self.rt.clear_input();
        }
        self.running_slot = None;
        self.rt.running_slot = None;
        self.rt.take_requests();
        self.rt.cancel_confirm();
        // A direct-mode line counts as slot 0's line 1 (checked on
        // hardware, as is the program's form, which a forced stop shares).
        let slot = vm.slot().unwrap_or(0);
        let line = line.unwrap_or(vm.line());
        // The format is SmileBASIC 4's own, from SmileBASIC 4.
        let what = match how {
            Halt::User => "User break",
            Halt::Stop => "Break",
        };
        let text: Vec<u16> = format!("{what} on {slot}:{line}").encode_utf16().collect();
        // It follows whatever the line printed, as after INPUT's prompt.
        let console = &mut self.rt.text[CONSOLE];
        console.print(&text);
        console.newline();
        let mut vm = vm;
        if let Some(message) = vm.take_stop_message() {
            self.print_line(&message);
        }
        // UNVERIFIED: a line typed in direct mode cannot go on.
        if program {
            vm.leave_input();
            self.halt(vm, Stopped::Paused, line);
        }
        self.ready();
    }

    /// Keeps a program stopped on `line` for CONT, or for BACKTRACE after
    /// an error.
    fn halt(&mut self, vm: Vm, how: Stopped, line: u32) {
        let at = (vm.slot().unwrap_or(0), line);
        self.rt.backtrace = std::iter::once(at)
            .chain(vm.call_sites().into_iter().rev())
            .collect();
        self.rt.stopped = how;
        self.halted = Some(vm);
    }

    /// Forgets the stopped program, once another starts or CLEAR runs.
    fn forget_halted(&mut self) {
        self.halted = None;
        self.rt.stopped = Stopped::No;
        self.rt.backtrace.clear();
    }

    /// Goes on with the stopped program, with the variables as direct
    /// mode left them.
    fn resume(&mut self, how: Resume) {
        let Some(mut vm) = self.halted.take() else {
            return self.ready();
        };
        self.forget_halted();
        vm.set_globals(&self.vars);
        self.arm(&mut vm);
        // A line run to earlier is forgotten once something else stopped
        // the program.
        vm.run_to(None);
        match how {
            Resume::Cont => {}
            Resume::Trace => vm.trace(),
            Resume::Step(step) => vm.step_by(step),
            Resume::RunTo(slot, line) => vm.run_to(Some((slot, line))),
        }
        self.rt.direct = false;
        self.running_slot = vm.slot();
        self.rt.running_slot = vm.slot();
        self.rt.program = Some(vm.program());
        self.vm = Some(vm);
        self.wait = 0;
        self.vsync_at = self.frame + 1;
    }

    /// Goes on with a program that STOP, the + button or the debugger
    /// stopped, as CONT does from direct mode. False when there is none
    /// that can go on.
    pub fn debug(&mut self, how: Resume) -> bool {
        if self.vm.is_some() || self.halted.is_none() || self.rt.stopped != Stopped::Paused {
            return false;
        }
        self.rt.cancel_input();
        self.resume(how);
        true
    }

    /// Where the stopped program goes on: its slot and the line about to
    /// run.
    pub fn paused_at(&self) -> Option<(usize, u32)> {
        let vm = self.halted.as_ref()?;
        Some((vm.slot()?, vm.line()))
    }

    /// The stopped program's top level and DEFs and GOSUBs under way,
    /// the innermost last.
    pub fn calls(&self) -> Vec<Call> {
        self.halted.as_ref().map(Vm::calls).unwrap_or_default()
    }

    /// The locals of one of the stopped program's DEF calls, by
    /// [`Call::frame`].
    pub fn frame_locals(&self, frame: usize) -> Vec<(String, Value)> {
        (self.halted.as_ref()).map_or_else(Vec::new, |vm| vm.frame_locals(frame))
    }

    /// The globals of a slot of the stopped program, with what direct
    /// mode has changed since it stopped.
    pub fn slot_globals(&self, slot: usize) -> Vec<(String, Value)> {
        let Some(vm) = &self.halted else {
            return Vec::new();
        };
        let mut globals = vm.slot_globals(slot);
        if vm.slot() == Some(slot) {
            for (name, value) in &mut globals {
                if let Some(v) = self.vars.get(name) {
                    *value = v.clone();
                }
            }
        }
        globals
    }

    /// Sets or clears a breakpoint on a slot's 1-based line; true when
    /// it is set now.
    pub fn toggle_breakpoint(&mut self, slot: usize, line: u32) -> bool {
        let lines = &mut self.breakpoints[slot];
        let set = lines.insert(line) || !lines.remove(&line);
        self.arm_all();
        set
    }

    /// The lines of a slot with breakpoints, in order.
    pub fn breakpoints(&self, slot: usize) -> Vec<u32> {
        self.breakpoints[slot].iter().copied().collect()
    }

    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.iter_mut().for_each(BTreeSet::clear);
        self.arm_all();
    }

    /// Gives a program the breakpoints.
    fn arm(&self, vm: &mut Vm) {
        for (slot, lines) in self.breakpoints.iter().enumerate() {
            vm.set_breakpoints(slot, &lines.iter().copied().collect::<Vec<_>>());
        }
    }

    /// Gives the running and the stopped program the breakpoints.
    fn arm_all(&mut self) {
        let mut vms = [self.vm.take(), self.halted.take()];
        for vm in vms.iter_mut().flatten() {
            self.arm(vm);
        }
        [self.vm, self.halted] = vms;
    }

    /// The editor, after it has taken the keys typed so far, so that its
    /// other commands come in order with them.
    pub fn edit(&mut self) -> &mut Editor {
        let editor = &mut self.editors[self.active];
        editor.tab_step = self.rt.settings.editor_tab_step as usize;
        editor.complete = Options::from_settings(&self.rt.settings, true);
        if !Rc::ptr_eq(&editor.library, &self.rt.library) {
            editor.library = self.rt.library.clone();
        }
        while let Some(key) = self.rt.next_key() {
            editor.key(key);
        }
        editor
    }

    /// Pasted text: into the editor, or typed as keys, with line breaks
    /// as Enter and letters as they are.
    pub fn paste(&mut self, text: &str) {
        if self.editing {
            self.edit().paste(text);
            return;
        }
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            let code = match c {
                '\r' => {
                    chars.next_if_eq(&'\n');
                    keys::ENTER
                }
                '\n' => keys::ENTER,
                '\t' => keys::TAB,
                c if c < ' ' => continue,
                // SmileBASIC strings hold only the Basic Multilingual Plane.
                c => match u16::try_from(u32::from(c)) {
                    Ok(code) => code,
                    Err(_) => continue,
                },
            };
            self.rt.key_press(code);
        }
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
        self.frame += 1;
        self.rt.begin_frame();
        self.rt.sound.render_frame();
        // Animations play in direct mode too, and go on while the editor
        // hides the screen (checked on hardware).
        self.rt.animate();
        if self.editing {
            self.edit();
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
        // A VSYNC that is behind goes on within the frame's budget.
        let start = vm.executed();
        let deadline = self.time_limit.map(|limit| Instant::now() + limit);
        let has_time = |vm: &Vm| match deadline {
            Some(deadline) => Instant::now() < deadline,
            None => vm.executed() - start < u64::from(FRAME_BUDGET),
        };
        loop {
            let left = match deadline {
                Some(_) => SLICE,
                None => FRAME_BUDGET.saturating_sub((vm.executed() - start) as u32),
            };
            match vm.run(&mut self.rt, left) {
                Ok(Status::Running) if has_time(vm) => continue,
                Ok(Status::Running) => {}
                Ok(Status::Retry) => self.wait = 1,
                // WAIT, WAIT 0 too, starts VSYNC's count anew; INPUT does
                // not (checked on hardware).
                Ok(Status::Wait {
                    frames,
                    vsync: false,
                }) => {
                    self.wait = frames;
                    self.vsync_at = self.frame + u64::from(frames);
                    if frames == 0 && has_time(vm) {
                        continue;
                    }
                }
                // VSYNC n waits until n frames after the one the last VSYNC
                // waited for. When that has gone by, it goes straight on
                // and counts from now (checked on hardware: a second
                // behind, ten VSYNCs took nine frames, and VSYNC 200 took
                // 140).
                Ok(Status::Wait {
                    frames,
                    vsync: true,
                }) => {
                    self.vsync_at += u64::from(frames);
                    if self.vsync_at > self.frame {
                        self.wait = (self.vsync_at - self.frame) as u32;
                    } else {
                        self.vsync_at = self.frame;
                        if has_time(vm) {
                            continue;
                        }
                    }
                }
                Ok(Status::Ended) => self.finish(None),
                Ok(Status::Break { line }) => self.stop_on(Some(line), Halt::User),
                Ok(Status::Stop { line }) => self.stop_on(Some(line), Halt::Stop),
                Ok(Status::Traced { line }) => self.stop_on(Some(line), Halt::Stop),
                Ok(Status::Debug { line }) => self.stop_on(Some(line), Halt::Stop),
                Err(e) => self.finish(Some(e)),
            }
            break;
        }
    }

    /// How the frame that [`Machine::render`] returns is to be shown. The
    /// editor keeps the start-up resolution, sampled once.
    pub fn screen_mode(&self) -> ScreenMode {
        if self.editing {
            let m = self.rt.startup_mode();
            ScreenMode::new(m.width, m.height)
        } else {
            self.rt.screen_mode()
        }
    }

    pub fn render(&mut self) -> &Framebuffer {
        if self.editing {
            if self.mirrors() {
                let [first, second] = &mut self.editors;
                match self.active {
                    0 => second.mirror(first),
                    _ => first.mirror(second),
                }
            }
            self.rt.render_editors(
                &mut self.editors,
                self.active,
                self.split.then_some(self.split_rows),
            )
        } else {
            self.rt.render()
        }
    }

    /// Runs a line typed in direct mode.
    fn direct(&mut self, line: &str) {
        if line.trim().is_empty() {
            self.rt.direct_input_start();
            return;
        }
        self.rt.remember_input(line);
        match parse_list(line) {
            Some(Ok(list)) => {
                match list {
                    List::At(slot, line) => {
                        let at = line.map(|line| (line, 0));
                        self.open_editor(slot.unwrap_or(self.editor().slot()), at)
                    }
                    List::Error => self.list_error(),
                }
                self.rt.direct_input_start();
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
        let (line, col) = self.editors[self.active].cursor_line();
        self.rt.help_cursor = (line.to_vec(), col);
        if slot.is_some() {
            self.forget_halted();
        }
        self.rt.direct = slot.is_none();
        self.running_slot = slot;
        self.rt.running_slot = slot;
        let compiled = match slot {
            Some(_) => sb_core::compile(source, &self.rt),
            None => sb_core::compile_direct(source, &self.rt),
        };
        match compiled {
            Ok(program) => {
                let vm = Vm::with_globals(program, &self.vars);
                let mut vm = match slot {
                    Some(slot) => vm.in_slot(slot),
                    None => vm,
                };
                self.arm(&mut vm);
                self.rt.program = Some(vm.program());
                self.vm = Some(vm);
                self.wait = 0;
                // The VM first runs on the next frame.
                self.vsync_at = self.frame + 1;
            }
            Err(e) => {
                let e = e.in_slot(slot.map(|s| s as u32));
                self.last_error = slot.map(|slot| (slot, e.line, e.col)).or(self.last_error);
                self.report(&e);
                self.ready();
            }
        }
    }

    /// The program or line ended, by itself or with an error.
    fn finish(&mut self, error: Option<Error>) {
        let vm = self.vm.take().expect("finish follows a run");
        self.vars.extend(vm.globals());
        let program = self.running_slot.is_some();
        if let Some(e) = &error {
            if let Some(slot) = e.slot {
                self.last_error = Some((slot as usize, e.line, e.col));
            }
            self.report(e);
        }
        if program {
            self.rt.clear_input();
            match &error {
                Some(e) => self.halt(vm, Stopped::Error, e.line),
                None => self.forget_halted(),
            }
        }
        self.running_slot = None;
        self.rt.running_slot = None;
        let mut run = None;
        // CONT, or TRACE for one statement.
        let mut cont = None;
        for request in self.rt.take_requests() {
            match request {
                Request::Run(slot) => run = Some(slot),
                Request::Clear => {
                    self.vars.clear();
                    self.forget_halted();
                }
                Request::Cont => cont = Some(Resume::Cont),
                Request::Trace => cont = Some(Resume::Trace),
            }
        }
        match (run, cont) {
            (Some(slot), _) => self.run_slot(slot),
            (None, Some(how)) => self.resume(how),
            (None, None) => self.ready(),
        }
    }

    /// Shows OK and waits for the next direct-mode line.
    fn ready(&mut self) {
        self.rt.direct = true;
        self.print_line(&"OK".encode_utf16().collect::<Vec<_>>());
        self.rt.direct_input_start();
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

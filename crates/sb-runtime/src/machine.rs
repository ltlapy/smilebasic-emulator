//! The whole computer: direct mode, the editor, the program slots, and the
//! program running on the [`Runtime`]. Frontends feed it keys, call
//! [`Machine::frame`] once per frame and show [`Machine::render`].

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::time::{Duration, Instant};

use sb_core::error::{Error, ErrorKind};
use sb_core::host::Host;
use sb_core::value::{Value, format_number};
use sb_core::vm::{Breakpoint, Call, DebugNote, Status, Step, Vm};
use sb_core::watch::Watch;

use crate::complete::Options;
use crate::editor::{Editor, Marks};
use crate::framebuffer::Framebuffer;
use crate::prg::SlotEdit;
use crate::runtime::{Request, Runtime, SLOTS, Stopped, keys};
use crate::screen::ScreenMode;
use crate::storage;
use crate::sub::{Linked, status};
use crate::text::CONSOLE;

/// Instructions a frame runs for code that does not VSYNC, when the
/// frame has no time limit (see [`Machine::time_limit`]), so that tests
/// and headless runs come out the same each time.
pub const FRAME_BUDGET: u32 = 1_000_000;
/// With a time limit, how many instructions run between looks at the
/// clock.
const SLICE: u32 = 20_000;
/// The frames the border marking the focus shows whole, and then takes
/// to fade out.
pub const FLASH_HOLD: u32 = 14;
pub const FLASH_FADE: u32 = 15;
/// With a time limit, the part of a frame's that a subprogram is sure of:
/// a quarter.
const SUB_SHARE: u32 = 4;
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
    /// Lines with breakpoints, with their conditions, by slot. They last
    /// for the session.
    breakpoints: Vec<BTreeMap<u32, Option<Rc<Watch>>>>,
    /// The breakpoints of files no slot holds, by `PROJECT/NAME`, for
    /// when one is loaded again.
    kept: HashMap<String, BTreeMap<u32, Option<Rc<Watch>>>>,
    /// Watches that stop programs when their values change.
    changes: Vec<Rc<Watch>>,
    /// Why the stopped program stopped.
    stop_reason: Option<StopReason>,
    /// A debugger's step started from the editor, which stays shown
    /// while it runs unless the program waits for input.
    stepping: Option<Step>,
    /// The step running has waited for input, so it shows the screen
    /// until it stops.
    step_waited: bool,
    /// The slot RUN last started, for the debugger's restart.
    last_run: Option<usize>,
    /// The debugger's panel and the stopped line were put away until the
    /// next stop.
    panel_closed: bool,
    /// The debugger's panel was asked for, so it shows with the editor
    /// whether a program is stopped or not.
    panel_pinned: bool,
    /// Slots whose lines the editor moved since the program started. Its
    /// code there is the old one, as CONT goes on with it (checked on
    /// hardware), so it keeps the breakpoints it had.
    moved: Vec<bool>,
    /// A line typed in direct mode failed since the last program: for
    /// ENVSTAT, the main interpreter is then suspended.
    direct_failed: bool,
    /// The editor shows apart from the screens, in a window of its own,
    /// so that programs and direct mode go on while it is open.
    detached: bool,
    /// With the editor apart, whether it takes the keys.
    editor_focus: bool,
    /// The slots' change counts the editor's sides last took.
    slot_changes: Vec<u32>,
    /// The sub interpreter, once SUBRUN has started a program on it. It
    /// stays after the program ends, keeping its pages and screens for
    /// the next one, but shows only while one runs.
    sub: Option<Box<Machine>>,
    /// SUBSHOW and SUBHIDE.
    sub_shown: bool,
    /// The focus on its way to the subprogram (true) or the main
    /// interpreter, and the frames before it gets there.
    focus_moving: Option<(bool, u32)>,
    /// Where the focus went last (the subprogram: true) and the frames
    /// since, for the border that marks it.
    focus_flash: Option<(bool, u32)>,
    /// On the sub interpreter, how its last program ended, for the main
    /// one to tell: `Some(None)` by itself, else with the error or break
    /// message the main one prints.
    ended: Option<Option<String>>,
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

/// What stopped a program: the + button, STOP and TRACE, or a
/// breakpoint or a debugger's step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Halt {
    User,
    Stop,
    Trace,
    Debug,
}

/// Why the stopped program stopped, for the debugger's panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The Break key or the + button.
    Paused,
    Stop,
    Traced,
    Stepped,
    RanTo,
    /// A breakpoint, with its condition when it has one.
    Breakpoint(Option<String>),
    /// A watch's value changed from one to the other, written as a note
    /// shows values.
    Changed {
        watch: String,
        from: String,
        to: String,
    },
    /// A breakpoint's condition could not be worked out.
    ConditionFailed(String),
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
            breakpoints: vec![BTreeMap::new(); SLOTS],
            kept: HashMap::new(),
            changes: Vec::new(),
            stop_reason: None,
            moved: vec![false; SLOTS],
            direct_failed: false,
            stepping: None,
            step_waited: false,
            last_run: None,
            panel_closed: false,
            panel_pinned: false,
            time_limit: None,
            detached: false,
            editor_focus: false,
            slot_changes: vec![0; SLOTS],
            sub: None,
            sub_shown: true,
            focus_moving: None,
            focus_flash: None,
            ended: None,
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

    /// Shows the editor apart from the screens, or in their place. Put
    /// back in their place while a program runs, it closes.
    pub fn set_detached(&mut self, detached: bool) {
        if self.detached == detached {
            return;
        }
        self.detached = detached;
        self.editor_focus = false;
        if !detached && self.editing && self.vm.is_some() {
            self.close_editor();
        }
    }

    pub fn is_detached(&self) -> bool {
        self.detached
    }

    /// With the editor apart, whether keys go to it rather than to the
    /// program or direct mode.
    pub fn focus_editor(&mut self, focused: bool) {
        self.editor_focus = focused;
    }

    /// Whether the editor is open and takes the keys: not while a
    /// subprogram beside it has the focus.
    pub fn editor_has_keys(&self) -> bool {
        self.editing && (!self.detached || self.editor_focus) && !self.sub_has_focus()
    }

    /// Whether the editor can open or change slots now: not while code
    /// runs in its place.
    fn can_edit(&self) -> bool {
        self.vm.is_none() || self.detached
    }

    /// Switches between direct mode and the editor, as F4 does. Entering
    /// the editor shows the last error's line once, like LIST ERR.
    pub fn toggle_editor(&mut self) {
        if !self.can_edit() {
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
        if !self.can_edit() || slot >= SLOTS {
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
        self.slot_changes.clone_from(&self.rt.slot_changes);
        self.editing = true;
    }

    /// Closes one side, keeping its changes in the slot.
    fn close_side(&mut self, side: usize) {
        self.editors[side].take_changed();
        if let Some(source) = self.editors[side].close() {
            let slot = self.editors[side].slot();
            self.rt.slots[slot] = source;
            self.rt.slot_modified[slot] = true;
        }
    }

    /// The sides of the editor with text of their own: not one that
    /// mirrors the other.
    fn own_sides(&self) -> [Option<usize>; 2] {
        let other = (self.split && !self.mirrors()).then_some(self.other());
        [Some(self.active), other]
    }

    /// Keeps the editor and the slots it shows the same while code can
    /// run with it open: its edits go into the slots, and a slot that a
    /// program or command changed comes into the editor.
    fn sync_slots(&mut self) {
        if !self.editing {
            return;
        }
        for side in self.own_sides().into_iter().flatten() {
            let slot = self.editors[side].slot();
            if self.editors[side].take_changed() {
                self.rt.slots[slot] = self.editors[side].text();
                self.rt.slot_modified[slot] = true;
            } else if self.slot_changes[slot] != self.rt.slot_changes[slot] {
                let (line, col) = self.editors[side].cursor();
                self.editors[side].open(slot, &self.rt.slots[slot], Some((line, col + 1)));
            }
            self.slot_changes[slot] = self.rt.slot_changes[slot];
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
        let side = self.side_at(x, y);
        if self.split && side != self.active {
            self.switch_side();
        }
        let (x, y) = self.on_side(x, y);
        self.edit().mouse_down(x, y, extend);
    }

    /// Which side of the editor's frame a point is on.
    fn side_at(&self, x: i32, y: i32) -> usize {
        let along = if self.split_rows { y } else { x };
        match self.rt.split_at {
            Some(at) if self.split && along >= at as i32 => 1,
            _ => 0,
        }
    }

    /// A press of the mouse on the line numbers' margin sets or clears a
    /// breakpoint on that line. False when it is not on a number.
    pub fn editor_margin_click(&mut self, x: i32, y: i32) -> bool {
        let side = self.side_at(x, y);
        let (x, y) = match self.rt.split_at {
            Some(at) if self.split && side == 1 && self.split_rows => (x, y - at as i32),
            Some(at) if self.split && side == 1 => (x - at as i32, y),
            _ => (x, y),
        };
        let editor = &self.editors[side];
        let Some(line) = editor.margin_line(x, y) else {
            return false;
        };
        self.toggle_breakpoint(editor.slot(), line as u32);
        true
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
        // The last error's line stays.
        self.rt.env_error = [0, -1, -1, -1, self.rt.env_error[4]];
        self.direct_failed = false;
        self.vars.clear();
        self.rt.clear_vars();
        // Each program starts with the mouse cursor hidden; END leaves it
        // as the program set it (checked on hardware).
        self.rt.system.mouse_cursor = false;
        self.rt.reset_perf_stats();
        self.last_run = Some(slot);
        self.sync_slots();
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
        if self.editing && !self.detached {
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
        // A subprogram has no direct mode to stop in: it ends, and the
        // main interpreter prints why.
        if self.rt.env_type == 1 {
            if let Some(vm) = &self.vm {
                let what = match how {
                    Halt::User => "User break",
                    Halt::Stop | Halt::Trace | Halt::Debug => "Break",
                };
                let (slot, line) = (vm.slot().unwrap_or(0), line.unwrap_or(vm.line()));
                self.end_sub(Some(format!("{what} on {slot}:{line}")));
            }
            return;
        }
        let Some(vm) = self.vm.take() else {
            return;
        };
        self.vars.extend(vm.globals());
        let program = self.running_slot.is_some();
        if program {
            self.rt.clear_input();
            if how == Halt::User {
                self.tidy_after_run();
            }
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
            Halt::Stop | Halt::Trace | Halt::Debug => "Break",
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
        let reason = match (how, vm.take_debug_note()) {
            (Halt::User, _) => StopReason::Paused,
            (Halt::Stop, _) => StopReason::Stop,
            (Halt::Trace, _) => StopReason::Traced,
            (Halt::Debug, Some(DebugNote::Breakpoint(c))) => {
                StopReason::Breakpoint(c.map(|w| w.source().to_string()))
            }
            (Halt::Debug, Some(DebugNote::RanTo)) => StopReason::RanTo,
            (Halt::Debug, Some(DebugNote::Changed(i, from, to))) => StopReason::Changed {
                watch: self.changes.get(i).map_or("", |w| w.source()).to_string(),
                from: note_value(&from),
                to: note_value(&to),
            },
            (Halt::Debug, Some(DebugNote::Condition(e))) => {
                StopReason::ConditionFailed(e.to_string())
            }
            (Halt::Debug, Some(DebugNote::Step) | None) => StopReason::Stepped,
        };
        // Those the screen alone does not tell are noted after the line.
        let note = match &reason {
            StopReason::Changed { watch, from, to } => {
                Some(format!("{watch} changed: {from} -> {to}"))
            }
            StopReason::ConditionFailed(e) => Some(format!("Condition failed: {e}")),
            _ => None,
        };
        self.stop_reason = program.then_some(reason);
        if let Some(note) = note {
            let note: Vec<u16> = note.encode_utf16().collect();
            self.print_line(&note);
        }
        // UNVERIFIED: a line typed in direct mode cannot go on.
        if program {
            vm.pass_here();
            vm.leave_input();
            self.halt(vm, Stopped::Paused, line);
        }
        self.ready();
        self.panel_closed = false;
        let stepped = self.stepping.take().is_some();
        if program && (stepped || how == Halt::Debug && self.rt.settings.debug_open_editor) {
            self.show_paused();
        }
    }

    /// Opens the editor at the line the stopped program goes on from,
    /// keeping what is being edited.
    fn show_paused(&mut self) {
        if let Some((slot, line)) = self.paused_at() {
            self.show_line(slot, line);
        }
    }

    /// Shows a slot's 1-based line in the editor, opening it if need be.
    pub fn show_line(&mut self, slot: usize, line: u32) {
        if !self.can_edit() || slot >= SLOTS {
            return;
        }
        if !self.editing {
            self.open_editor(slot, Some((line as usize, 0)));
            return;
        }
        self.edit_slot(slot);
        self.edit().go_to(line as usize);
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
        self.direct_failed = false;
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
    /// From the editor, CONT shows the screen again, while a step leaves
    /// the editor shown until the program waits for input.
    pub fn debug(&mut self, how: Resume) -> bool {
        if self.vm.is_some() || self.halted.is_none() || self.rt.stopped != Stopped::Paused {
            return false;
        }
        if let Resume::RunTo(slot, _) = how
            && self.lines_moved(slot)
        {
            return false;
        }
        if self.editing {
            match how {
                Resume::Cont | Resume::Trace if self.detached => {}
                Resume::Cont | Resume::Trace => self.close_editor(),
                Resume::Step(step) => self.stepping = Some(step),
                Resume::RunTo(..) => self.stepping = Some(Step::Over),
            }
        }
        self.step_waited = false;
        self.rt.input.polled = false;
        self.panel_closed = false;
        self.rt.cancel_input();
        self.resume(how);
        true
    }

    /// Runs the program RUN last started again from the start, showing
    /// the screen. False when there is none.
    pub fn restart(&mut self) -> bool {
        let Some(slot) = self.last_run else {
            return false;
        };
        if self.vm.is_some() {
            self.stop();
        }
        if self.editing && !self.detached {
            self.close_editor();
        }
        self.rt.cancel_input();
        self.run_slot(slot);
        true
    }

    /// Whether the stopped program was stopped by an error, so that it
    /// cannot go on.
    pub fn stopped_by_error(&self) -> bool {
        self.rt.stopped == Stopped::Error
    }

    /// Whether the debugger's panel shows below the editor: once asked
    /// for, and while a program is stopped or a step runs.
    fn panel_shown(&self) -> bool {
        self.editing && (self.panel_pinned || !self.panel_closed && self.panel_screens())
    }

    /// Whether the panel has screens to show: those a stopped program, or
    /// one a step runs, left.
    pub fn panel_screens(&self) -> bool {
        self.halted.is_some() || self.stepping.is_some()
    }

    /// Shows the debugger's panel with the editor from now on, or puts it
    /// and the stopped line away until the next stop.
    pub fn show_panel(&mut self, shown: bool) {
        self.panel_pinned = shown;
        self.panel_closed = !shown;
    }

    /// Whether the debugger's panel shows with the editor.
    pub fn panel_open(&self) -> bool {
        self.panel_shown()
    }

    /// Where the debugger's panel starts on the frame [`Machine::render`]
    /// last returned, when it shows.
    pub fn panel_at(&self) -> Option<u32> {
        self.rt.panel_at.filter(|_| self.panel_shown())
    }

    /// The screens as the program left them, while the editor shows
    /// instead, for the debugger's panel.
    pub fn render_screens(&mut self) -> &Framebuffer {
        self.rt.render()
    }

    /// Gives each side of the editor the breakpoints and the stopped line
    /// of its slot.
    fn mark_editors(&mut self) {
        self.follow_edits();
        let at = self.paused_at().filter(|_| !self.panel_closed);
        for i in 0..2 {
            let slot = self.editors[i].slot();
            let stale = self.moved[slot];
            self.editors[i].marks = Marks {
                breakpoints: self.breakpoints[slot]
                    .keys()
                    .map(|&l| l as usize - 1)
                    .collect(),
                conditional: (self.breakpoints[slot].iter())
                    .filter(|(_, c)| c.is_some())
                    .map(|(&l, _)| l as usize - 1)
                    .collect(),
                stopped: at.filter(|at| at.0 == slot).map(|at| at.1 as usize - 1),
                stale,
            };
        }
    }

    /// Makes the stopped program go on from a line of its slot, in the
    /// same DEF, or outside DEFs as it is. False when it cannot.
    pub fn jump_to(&mut self, line: u32) -> bool {
        let slot = self.paused_at().map(|at| at.0);
        if self.rt.stopped != Stopped::Paused || slot.is_none_or(|slot| self.lines_moved(slot)) {
            return false;
        }
        let Some(vm) = &mut self.halted else {
            return false;
        };
        if !vm.jump_to(line) {
            return false;
        }
        // What stopped it is no longer where it is.
        self.stop_reason = None;
        if let Some(top) = self.rt.backtrace.first_mut() {
            top.1 = line;
        }
        true
    }

    /// Why the stopped program stopped, while it can go on.
    pub fn stop_reason(&self) -> Option<&StopReason> {
        let paused = self.halted.is_some() && self.rt.stopped == Stopped::Paused;
        self.stop_reason.as_ref().filter(|_| paused)
    }

    /// Where the stopped program goes on: its slot and the line about to
    /// run.
    pub fn paused_at(&self) -> Option<(usize, u32)> {
        let vm = self.halted.as_ref()?;
        Some((vm.slot()?, vm.line()))
    }

    /// The stopped or running program's top level and DEFs and GOSUBs
    /// under way, the innermost last.
    pub fn calls(&self) -> Vec<Call> {
        self.inspected().map(Vm::calls).unwrap_or_default()
    }

    /// The program the debugger's panel shows: the stopped one, else the
    /// one running.
    fn inspected(&self) -> Option<&Vm> {
        self.halted.as_ref().or(self.vm.as_ref())
    }

    /// The locals of one of the stopped or running program's DEF calls,
    /// by [`Call::frame`].
    pub fn frame_locals(&self, frame: usize) -> Vec<(String, Value)> {
        self.inspected()
            .map_or_else(Vec::new, |vm| vm.frame_locals(frame))
    }

    /// The globals of a slot of the stopped or running program, with
    /// what direct mode has changed since it stopped; without one, direct
    /// mode's.
    pub fn slot_globals(&self, slot: usize) -> Vec<(String, Value)> {
        let Some(vm) = self.inspected() else {
            // Without a program, direct mode's variables, as slot 0's.
            let mut vars: Vec<_> = match slot {
                0 => self
                    .vars
                    .iter()
                    .map(|(n, v)| (n.clone(), v.clone()))
                    .collect(),
                _ => Vec::new(),
            };
            vars.sort_by(|a, b| a.0.cmp(&b.0));
            return vars;
        };
        let mut globals = vm.slot_globals(slot);
        if self.halted.is_some() && vm.slot() == Some(slot) {
            for (name, value) in &mut globals {
                if let Some(v) = self.vars.get(name) {
                    *value = v.clone();
                }
            }
        }
        globals
    }

    /// Evaluates a watch expression with the stopped program's variables:
    /// a DEF call's locals, by [`Call::frame`], before a slot's globals.
    pub fn watch(&self, source: &str, frame: Option<usize>, slot: usize) -> Result<Value, Error> {
        let locals = frame.map(|f| self.frame_locals(f)).unwrap_or_default();
        let globals = self.slot_globals(slot);
        let var = |name: &str| {
            let mut all = locals.iter().chain(&globals);
            all.find(|(n, _)| n == name).map(|(_, v)| v.clone())
        };
        sb_core::watch::eval(source, &var)
    }

    /// Gives a variable of the stopped program a new value: a local of a
    /// DEF call if it has one by that name, else a global of the slot.
    /// False when there is no such variable.
    pub fn set_variable(
        &mut self,
        name: &str,
        value: Value,
        frame: Option<usize>,
        slot: usize,
    ) -> bool {
        let Some(vm) = &mut self.halted else {
            return false;
        };
        if frame.is_some_and(|f| vm.set_local(f, name, value.clone())) {
            return true;
        }
        if !vm.set_slot_global(slot, name, value.clone()) {
            return false;
        }
        // CONT gives the running slot direct mode's values.
        if vm.slot() == Some(slot) {
            self.vars.insert(name.to_string(), value);
        }
        true
    }

    /// Sets or clears a breakpoint on a slot's 1-based line; true when
    /// it is set now.
    pub fn toggle_breakpoint(&mut self, slot: usize, line: u32) -> bool {
        if slot >= SLOTS || line == 0 {
            return false;
        }
        self.follow_edits();
        let lines = &mut self.breakpoints[slot];
        let set = lines.remove(&line).is_none();
        if set {
            lines.insert(line, None);
        }
        self.arm_all();
        set
    }

    /// Sets a breakpoint on a slot's 1-based line that stops only when
    /// `condition` is true, or always when it is empty.
    pub fn set_condition(&mut self, slot: usize, line: u32, condition: &str) -> Result<(), Error> {
        if slot >= SLOTS || line == 0 {
            return Err(ErrorKind::OutOfRange.into());
        }
        let condition = condition.trim();
        let watch = match condition.is_empty() {
            true => None,
            false => Some(Rc::new(Watch::new(condition)?)),
        };
        self.follow_edits();
        self.breakpoints[slot].insert(line, watch);
        self.arm_all();
        Ok(())
    }

    /// The condition of the breakpoint on a slot's 1-based line, empty
    /// for one without, or `None` when there is none there.
    pub fn condition(&mut self, slot: usize, line: u32) -> Option<String> {
        self.follow_edits();
        let breakpoint = self.breakpoints.get(slot)?.get(&line)?;
        Some(
            breakpoint
                .as_ref()
                .map_or_else(String::new, |w| w.source().to_string()),
        )
    }

    /// Every breakpoint, by slot and line, with its condition.
    pub fn all_breakpoints(&mut self) -> Vec<(usize, u32, Option<String>)> {
        self.follow_edits();
        let mut all = Vec::new();
        for (slot, lines) in self.breakpoints.iter().enumerate() {
            for (&line, c) in lines {
                all.push((slot, line, c.as_ref().map(|w| w.source().to_string())));
            }
        }
        all
    }

    /// The lines of a slot with breakpoints, in order.
    pub fn breakpoints(&mut self, slot: usize) -> Vec<u32> {
        self.follow_edits();
        self.breakpoints[slot].keys().copied().collect()
    }

    /// Sets the watch expressions that stop programs, before the
    /// statement after the one that changed one of their values; those
    /// that do not parse are left out. Setting the same ones again keeps
    /// the values changes are told from.
    pub fn set_changes(&mut self, sources: &[&str]) {
        let same = sources.len() == self.changes.len()
            && sources
                .iter()
                .zip(&self.changes)
                .all(|(s, w)| *s == w.source());
        if same {
            return;
        }
        self.changes = (sources.iter())
            .filter_map(|s| Watch::new(s).ok())
            .map(Rc::new)
            .collect();
        self.arm_all();
    }

    /// Whether the editor moved a slot's lines since the program
    /// started, so that the stopped program's lines are not the text's.
    pub fn lines_moved(&mut self, slot: usize) -> bool {
        self.follow_edits();
        self.moved.get(slot).copied().unwrap_or(false)
    }

    /// Moves the breakpoints with the lines the editor, PRGINS and PRGDEL
    /// moved, and drops those of lines they took out. A program LOAD or
    /// NEW replaced keeps its own for its file, and a file loaded again
    /// gets back those it had.
    fn follow_edits(&mut self) {
        let edits: Vec<_> = (self.editors.iter_mut())
            .flat_map(|e| e.take_shifts())
            .map(|(slot, shift)| (slot, SlotEdit::Shift(shift)))
            .collect();
        let edits = edits
            .into_iter()
            .chain(std::mem::take(&mut self.rt.slot_edits));
        let (lines, kept, moved) = (&mut self.breakpoints, &mut self.kept, &mut self.moved);
        follow(lines, kept, moved, &self.rt.slots, edits.collect());
    }

    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.iter_mut().for_each(BTreeMap::clear);
        self.kept.clear();
        self.arm_all();
    }

    /// Gives a program the breakpoints, with the lines as the editor left
    /// them.
    fn arm(&mut self, vm: &mut Vm) {
        self.follow_edits();
        give_breakpoints(&self.breakpoints, &self.moved, vm);
        // Direct mode's lines stop at no breakpoint, and on no change.
        if vm.slot().is_some() {
            vm.set_changes(&self.changes);
        }
    }

    /// Gives the running and the stopped program the breakpoints.
    fn arm_all(&mut self) {
        let mut vms = [self.vm.take(), self.halted.take()];
        for vm in vms.iter_mut().flatten() {
            give_breakpoints(&self.breakpoints, &self.moved, vm);
            if vm.slot().is_some() {
                vm.set_changes(&self.changes);
            }
        }
        [self.vm, self.halted] = vms;
    }

    /// The editor, after it has taken the keys typed so far, so that its
    /// other commands come in order with them.
    /// Apart from the screens, it takes only the keys typed into its
    /// window.
    pub fn edit(&mut self) -> &mut Editor {
        let keys = self.editor_has_keys();
        let slot = self.editors[self.active].slot();
        let others = self.rt.other_slot_names(slot);
        let editor = &mut self.editors[self.active];
        editor.set_other_names(others);
        editor.tab_step = self.rt.settings.editor_tab_step as usize;
        editor.complete = Options::from_settings(&self.rt.settings, true);
        if !Rc::ptr_eq(&editor.library, &self.rt.library) {
            editor.library = self.rt.library.clone();
        }
        while let Some(key) = keys.then(|| self.rt.next_key()).flatten() {
            editor.key(key);
        }
        editor
    }

    /// Pasted text: into the editor, or typed as keys, with line breaks
    /// as Enter and letters as they are.
    pub fn paste(&mut self, text: &str) {
        if self.editor_has_keys() {
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

    /// Ctrl+F1 to F5: types what KEY set for function key `n`, 1 to 5,
    /// as it is.
    pub fn type_function_key(&mut self, n: usize) {
        let keys = self.rt.input.function_keys[n - 1].clone();
        for code in keys {
            self.rt.key_press(code);
        }
    }

    /// Runs one frame: the program, the editor, or direct-mode line
    /// editing, and then the subprogram, if one runs.
    pub fn frame(&mut self) {
        if let Some((_, age)) = &mut self.focus_flash {
            *age += 1;
        }
        self.move_focus();
        self.give_input();
        self.rt.env_status = self.env_status();
        // With a subprogram running, a busy main program leaves it a
        // quarter of the frame's time, and it gets what the main program
        // leaves.
        let limit = self.time_limit;
        if self.sub_running() {
            self.time_limit = limit.map(|l| l - l / SUB_SHARE);
        }
        let start = Instant::now();
        self.run_frame(None);
        self.time_limit = limit;
        if self.sub_running() {
            // A subprogram's HELPINFO OUT looks at the editor's cursor as
            // it is now.
            let (line, col) = self.editors[self.active].cursor_line();
            if self.rt.help_cursor.1 != col || self.rt.help_cursor.0 != line {
                self.rt.help_cursor = (line.to_vec(), col);
            }
        }
        self.run_sub(start.elapsed());
    }

    /// One frame of this interpreter; `main` is the main interpreter's
    /// runtime when this is the sub interpreter.
    fn run_frame(&mut self, mut main: Option<&mut Runtime>) {
        self.frame += 1;
        self.follow_edits();
        self.rt.begin_frame();
        self.rt.sound.render_frame();
        // Animations play in direct mode too, and go on while the editor
        // hides the screen (checked on hardware).
        self.rt.animate();
        if self.editing {
            self.edit();
            self.sync_slots();
            // A step from the editor runs with it shown.
            if !self.detached && (self.stepping.is_none() || self.vm.is_none()) {
                return;
            }
        }
        let mut vsynced = false;
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
            let status = match main.as_deref_mut() {
                Some(main) => {
                    let mut linked = Linked {
                        sub: &mut self.rt,
                        main,
                    };
                    vm.run(&mut linked, left)
                }
                None => vm.run(&mut self.rt, left),
            };
            match status {
                Ok(Status::Running) => {
                    // EXEC, PRGINS and the like changed slots: the program
                    // runs on with their breakpoints moved.
                    if !self.rt.slot_edits.is_empty() {
                        let edits = std::mem::take(&mut self.rt.slot_edits);
                        let (lines, kept, moved) =
                            (&mut self.breakpoints, &mut self.kept, &mut self.moved);
                        follow(lines, kept, moved, &self.rt.slots, edits);
                        give_breakpoints(&self.breakpoints, &self.moved, vm);
                    }
                    if has_time(vm) {
                        continue;
                    }
                }
                Ok(Status::Retry) => self.wait = 1,
                // WAIT, WAIT 0 too, starts VSYNC's count anew; INPUT does
                // not (checked on hardware).
                Ok(Status::Wait {
                    frames,
                    vsync: false,
                }) => {
                    vsynced = true;
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
                    vsynced = true;
                    self.vsync_at += u64::from(frames);
                    if self.vsync_at > self.frame {
                        self.wait = (self.vsync_at - self.frame) as u32;
                    } else {
                        self.vsync_at = self.frame;
                        self.rt.sound.synced();
                        if has_time(vm) {
                            continue;
                        }
                    }
                }
                Ok(Status::Ended) => self.finish(None),
                Ok(Status::Break { line }) => self.stop_on(Some(line), Halt::User),
                Ok(Status::Stop { line }) => self.stop_on(Some(line), Halt::Stop),
                Ok(Status::Traced { line }) => self.stop_on(Some(line), Halt::Trace),
                Ok(Status::Debug { line }) => self.stop_on(Some(line), Halt::Debug),
                Err(e) => self.finish(Some(e)),
            }
            break;
        }
        // A step that waits for input shows the screen until it stops: an
        // INPUT, or one going over lines that read the controls and waited
        // for a frame.
        // The editor apart stays open, and gives the keys to the screen.
        self.sync_slots();
        if let (Some(step), Some(vm)) = (self.stepping, &self.vm)
            && (vm.is_reading_input() || step != Step::Into && vsynced && self.rt.input.polled)
        {
            self.step_waited = true;
            if self.editing && !self.detached {
                self.close_editor();
            }
        }
    }

    /// Whether a debugger's step is running and has waited for input, so
    /// that the keys belong on the screen until it stops.
    pub fn step_wants_keys(&self) -> bool {
        self.stepping.is_some() && self.step_waited
    }

    /// How the frame that [`Machine::render`] returns is to be shown. The
    /// editor keeps the start-up resolution, sampled once.
    pub fn screen_mode(&self) -> ScreenMode {
        if self.editing && !self.detached {
            self.editor_mode()
        } else {
            self.rt.screen_mode()
        }
    }

    /// How the editor's frame is to be shown.
    pub fn editor_mode(&self) -> ScreenMode {
        let m = self.rt.startup_mode();
        ScreenMode::new(m.width, m.height)
    }

    /// The editor, or the screens while it is closed or shows apart.
    pub fn render(&mut self) -> &Framebuffer {
        if self.editing && !self.detached {
            self.render_editor()
        } else {
            self.rt.render()
        }
    }

    /// The editor's frame, which shows apart from the screens.
    pub fn render_editor(&mut self) -> &Framebuffer {
        self.mark_editors();
        let panel = self.panel_shown();
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
            panel,
        )
    }

    /// Counts the frames the editor's frame was drawn: while the count
    /// stays, [`Machine::render_editor`] gives the same pixels.
    pub fn editor_revision(&self) -> u64 {
        self.rt.editor_revision()
    }

    /// The editor's frame as [`Machine::render_editor`] last drew it.
    pub fn editor_frame(&self) -> &Framebuffer {
        self.rt.editor_frame()
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
            Some(slot) => sb_core::compile_in(source, &self.rt, slot),
            None => sb_core::compile_direct(source, &self.rt, self.last_run.unwrap_or(0)),
        };
        match compiled {
            Ok(program) => {
                if slot.is_some() {
                    self.follow_edits();
                    self.moved.fill(false);
                }
                let vm = Vm::with_globals(program, &self.vars);
                let mut vm = match slot {
                    Some(slot) => vm.in_slot(slot),
                    None => vm,
                };
                self.arm(&mut vm);
                self.rt.program = Some(vm.program());
                if let Some(slot) = slot {
                    self.rt.slot_programs = Default::default();
                    self.rt.slot_programs[slot] = Some(vm.program());
                }
                self.vm = Some(vm);
                self.wait = 0;
                // The VM first runs on the next frame.
                self.vsync_at = self.frame + 1;
            }
            Err(e) => {
                let e = e.in_slot(slot.map(|s| s as u32));
                if self.rt.env_type == 1 {
                    return self.end_sub(Some(e.to_string()));
                }
                self.last_error = slot.map(|slot| (slot, e.line, e.col)).or(self.last_error);
                // UNVERIFIED: that ENVSTAT tells of an error found before
                // the program runs too, at the token the error names.
                match e.slot {
                    Some(slot) => self.rt.env_error = self.error_place(slot as usize, &e),
                    None => self.direct_error(),
                }
                self.report(&e);
                self.ready();
            }
        }
    }

    /// With the setting, a program that ended or was stopped with the
    /// Break key leaves the screens as ACLS does and the sound stopped.
    /// STOP, errors and the debugger's stops leave everything to look at.
    fn tidy_after_run(&mut self) {
        if self.rt.settings.tidy_on_end && self.rt.env_type == 0 {
            self.rt.acls(false, false, false, false);
            self.rt.sound.stop_all();
        }
    }

    /// A line typed in direct mode failed: ENVSTAT keeps only the last
    /// error's line (checked on hardware, after a program's error).
    // UNVERIFIED: the same without one.
    fn direct_error(&mut self) {
        self.rt.env_error = [0, -1, -1, -1, self.rt.env_error[4]];
        self.direct_failed = true;
    }

    /// The program or line ended, by itself or with an error.
    fn finish(&mut self, error: Option<Error>) {
        if self.rt.env_type == 1 {
            return self.end_sub(error.map(|e| e.to_string()));
        }
        self.stepping = None;
        let vm = self.vm.take().expect("finish follows a run");
        self.vars.extend(vm.globals());
        let program = self.running_slot.is_some();
        if let Some(e) = &error {
            if let Some(slot) = e.slot {
                self.last_error = Some((slot as usize, e.line, e.col));
                self.rt.env_error = self.error_place(slot as usize, e);
            }
            if !program {
                self.direct_error();
            }
            self.report(e);
        }
        if program {
            self.rt.clear_input();
            match &error {
                Some(e) => self.halt(vm, Stopped::Error, e.line),
                None => {
                    self.forget_halted();
                    self.tidy_after_run();
                }
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
                Request::Subrun(file) => self.subrun(&file),
                Request::Substop => self.substop(),
                Request::Subshow(shown) => self.sub_shown = shown,
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

    /// Prints an error's message, and with localized error messages on,
    /// the Japanese line under it that explains it.
    fn report(&mut self, error: &Error) {
        let text: Vec<u16> = error.to_string().encode_utf16().collect();
        self.print_line(&text);
        if self.rt.settings.localized_errors
            && let Some(line) = crate::error_lines::japanese(error.kind.code())
        {
            self.print_line(&line.encode_utf16().collect::<Vec<_>>());
        }
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

/// Moves, keeps and brings back breakpoints as [`Machine::follow_edits`]
/// says; `slots` are the programs as they are now.
fn follow(
    breakpoints: &mut [BTreeMap<u32, Option<Rc<Watch>>>],
    kept: &mut HashMap<String, BTreeMap<u32, Option<Rc<Watch>>>>,
    moved: &mut [bool],
    slots: &[String],
    edits: Vec<(usize, SlotEdit)>,
) {
    for (slot, edit) in edits {
        let lines = &mut breakpoints[slot];
        match edit {
            SlotEdit::Shift(shift) => {
                *lines = std::mem::take(lines)
                    .into_iter()
                    .filter_map(|(line, c)| Some((shift.map(line as usize - 1)? as u32 + 1, c)))
                    .collect();
            }
            SlotEdit::Replaced { from, to } => {
                let old = std::mem::take(lines);
                if let Some(from) = from {
                    match old.is_empty() {
                        true => kept.remove(&from),
                        false => kept.insert(from, old),
                    };
                }
                // The file may have changed since: lines past its end go.
                let count = slots[slot].matches('\n').count() as u32 + 1;
                let back = to.and_then(|to| kept.remove(&to)).unwrap_or_default();
                *lines = back
                    .into_iter()
                    .filter(|&(line, _)| line <= count)
                    .collect();
            }
            // The program runs the code as it is now, so it can have
            // the breakpoints as they are.
            SlotEdit::Compiled => {
                moved[slot] = false;
                continue;
            }
        }
        // A running program goes on with the code it had.
        moved[slot] = true;
    }
}

/// Gives a program the breakpoints of the slots whose lines have not
/// moved since it started.
fn give_breakpoints(breakpoints: &[BTreeMap<u32, Option<Rc<Watch>>>], moved: &[bool], vm: &mut Vm) {
    for (slot, lines) in breakpoints.iter().enumerate() {
        if !moved[slot] {
            let lines: Vec<Breakpoint> = lines.iter().map(|(&l, c)| (l, c.clone())).collect();
            vm.set_breakpoints(slot, &lines);
        }
    }
}

/// A value as a debugger's note shows it: numbers as PRINT does, strings
/// quoted, and arrays by their size.
fn note_value(value: &Value) -> String {
    match value {
        Value::Default => "(none)".to_string(),
        Value::Int(_) | Value::Real(_) => String::from_utf16_lossy(&format_number(value)),
        Value::Str(s) => format!("\"{}\"", String::from_utf16_lossy(&s.borrow())),
        Value::Array(a) => format!("array of {}", a.borrow().len()),
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

/// The sub interpreter, as the main one runs it.
impl Machine {
    /// SUBRUN: loads `file`, as `"PROJECT/NAME"` or a name in the current
    /// project, into the sub interpreter's PRG0 and runs it there with the
    /// focus. The sub interpreter keeps its pages and screens from the
    /// last time.
    fn subrun(&mut self, file: &str) {
        if let Err(e) = self.start_sub(file) {
            let detail = format!("{file}: {e}");
            self.report(&Error::with_detail(ErrorKind::LoadFailed, detail));
        }
    }

    /// F9 to F11: starts tool `n`, 0 to 2, from the settings as SUBRUN
    /// does, also while a program runs. Gives false when the setting is
    /// empty, and the error when the file cannot be loaded, for the
    /// frontend to show rather than the console.
    pub fn run_tool(&mut self, n: usize) -> storage::Result<bool> {
        let file = self.rt.settings.tools[n].clone();
        if file.is_empty() {
            return Ok(false);
        }
        self.start_sub(&file)?;
        Ok(true)
    }

    fn start_sub(&mut self, file: &str) -> storage::Result<()> {
        let sub = self
            .sub
            .get_or_insert_with(|| Box::new(Machine::new(self.rt.new_sub())));
        let (project, name) = match file.split_once('/') {
            Some((project, name)) => (project, name),
            None => (self.rt.storage.project(), file),
        };
        // A running subprogram stops with User break first, whether the
        // file can be loaded or not.
        if sub.vm.is_some() {
            sub.stop();
        }
        let before = sub.rt.storage.project().to_string();
        let loaded = sub.rt.storage.set_project(project);
        if let Err(e) = loaded.and_then(|()| sub.rt.load_file(name, 0)) {
            let _ = sub.rt.storage.set_project(&before);
            return Err(e);
        }
        sub.end_sub(None);
        sub.ended = None;
        // Its variables, SPFUNC and TFUNC go; the rest stays.
        for sprite in &mut sub.rt.sprites.list {
            sprite.func = None;
        }
        for screen in &mut sub.rt.text {
            screen.func = None;
        }
        sub.run_slot(0);
        sub.rt.focus = true;
        self.rt.focus = false;
        self.focus_flash = Some((true, 0));
        self.focus_moving = None;
        (sub.rt.focus_busy, sub.rt.focus_wish) = (false, None);
        (self.rt.focus_busy, self.rt.focus_wish) = (false, None);
        self.rt.sub_running = true;
        self.sub_shown = true;
        Ok(())
    }

    /// SUBSTOP: stops the subprogram.
    fn substop(&mut self) {
        if let Some(sub) = &mut self.sub {
            sub.stop_on(None, Halt::User);
        }
        self.sub_ended(false);
    }

    /// When the subprogram has ended, the focus comes back, and the main
    /// interpreter prints why it ended, if not by itself, between lines
    /// of its own; `again` takes up the direct-mode line afresh after it.
    fn sub_ended(&mut self, again: bool) {
        let Some(sub) = &mut self.sub else {
            return;
        };
        let Some(message) = sub.ended.take() else {
            return;
        };
        sub.rt.focus = false;
        self.rt.focus = true;
        self.focus_moving = None;
        self.focus_flash = Some((false, 0));
        (sub.rt.focus_busy, sub.rt.focus_wish) = (false, None);
        (self.rt.focus_busy, self.rt.focus_wish) = (false, None);
        self.rt.sub_running = false;
        let Some(message) = message else {
            return;
        };
        let title = "=== From subinterpreter ===";
        let rule = "=".repeat(title.len());
        for line in [title, &message, &rule] {
            self.print_line(&line.encode_utf16().collect::<Vec<_>>());
        }
        if again && self.vm.is_none() && self.rt.direct {
            self.rt.direct_input_start();
        }
    }

    /// On the sub interpreter: the program is over, and the main
    /// interpreter is told.
    fn end_sub(&mut self, message: Option<String>) {
        self.stepping = None;
        self.vm = None;
        self.halted = None;
        self.running_slot = None;
        self.rt.running_slot = None;
        self.rt.take_requests();
        self.rt.cancel_input();
        self.rt.cancel_confirm();
        self.rt.clear_input();
        self.ended = Some(message);
    }

    /// What ENVSTAT gives for an error in a slot: its number, argument
    /// (-1 when not known), slot, character of the statement from the
    /// start of the program, counting from 0, and line.
    // UNVERIFIED: which errors know their argument.
    fn error_place(&self, slot: usize, e: &Error) -> [i32; 5] {
        let before: usize = (self.rt.slots[slot].split_inclusive('\n'))
            .take(e.line.saturating_sub(1) as usize)
            .map(|l| l.encode_utf16().count())
            .sum();
        let at = before + (e.col as usize).saturating_sub(1);
        [e.kind.code(), -1, slot as i32, at as i32, e.line as i32]
    }

    /// The main interpreter's state for ENVSTAT.
    fn env_status(&self) -> i32 {
        if self.editing && !self.detached {
            status::EDITING
        } else if self.running_slot.is_some() {
            status::RUNNING
        } else {
            match self.rt.stopped {
                _ if self.direct_failed => status::PAUSED,
                Stopped::No => status::DIRECT,
                Stopped::Paused => status::PAUSED,
                Stopped::Error => status::ERROR,
            }
        }
    }

    /// Hands the frame's controls and keys to the interpreter with the
    /// focus; the other sees nothing pressed.
    fn give_input(&mut self) {
        let Some(sub) = &mut self.sub else {
            return;
        };
        let live = self.rt.input.live;
        let idle = idle(&live);
        if !sub.rt.focus {
            sub.rt.input.live = idle;
            return;
        }
        sub.rt.keys.extend(self.rt.keys.drain(..));
        self.rt.input.live = idle;
        // The pointer, in the sub-screen's pixels.
        let mut mouse = live.mouse;
        let (fw, fh) = (sub.rt.mode.width as i32, sub.rt.mode.height as i32);
        let [x, y, w, h] = sub.rt.sub_screen.unwrap_or([0, 0, 1280, 720]);
        let (dx, dy) = (mouse.display.0 - x, mouse.display.1 - y);
        mouse.x = dx * fw / w.max(1);
        mouse.y = dy * fh / h.max(1);
        mouse.on_screen = (0..w).contains(&dx) && (0..h).contains(&dy) && self.sub_shown;
        sub.rt.input.live = crate::input::State { mouse, ..live };
    }

    /// Runs the subprogram's frame, if there is a sub interpreter, and
    /// passes the focus as ENVFOCUS asked.
    fn run_sub(&mut self, used: Duration) {
        let Some(sub) = &mut self.sub else {
            return;
        };
        sub.time_limit = (self.time_limit).map(|l| l.saturating_sub(used).max(l / SUB_SHARE));
        sub.run_frame(Some(&mut self.rt));
        // The main interpreter's wish first, as it ran first.
        let to_sub = match (self.rt.focus_wish.take(), sub.rt.focus_wish.take()) {
            (Some(main_takes), _) => Some(!main_takes),
            (None, sub_takes) => sub_takes,
        };
        if let Some(to_sub) = to_sub
            && self.focus_moving.is_none()
        {
            // UNVERIFIED: giving the focus away took one to three frames,
            // taking it one.
            self.focus_moving = Some((to_sub, if to_sub { 1 } else { 2 }));
            sub.rt.focus_busy = true;
            self.rt.focus_busy = true;
        }
        if std::mem::take(&mut self.rt.forget_stop) {
            self.forget_halted();
        }
        self.sub_ended(true);
        if let Some(sub) = &mut self.sub {
            let running = sub.vm.is_some();
            sub.rt.dimmed = running && !sub.rt.focus;
            self.rt.dimmed = running && sub.rt.focus;
        }
    }

    /// The subprogram's screen and where on the 1280x720 display it
    /// shows, as x, y, width and height, while it shows. Where its
    /// BACKCOLOR is clear, it is too, with its colors premultiplied.
    // SmileBASIC 4's own SMILETOOL ends without hiding its screen, so it
    // goes when the subprogram ends, as it does after an error or SUBSTOP.
    pub fn sub_screen(&mut self) -> Option<(&Framebuffer, [i32; 4])> {
        let sub = (self.sub.as_mut()).filter(|s| self.sub_shown && s.vm.is_some())?;
        let rect = sub.rt.sub_screen?;
        Some((sub.rt.render(), rect))
    }

    /// The yellow border that marks where the focus has just gone, also
    /// back to the main interpreter when a subprogram ends: the
    /// rectangle on the 1280x720 display (the subprogram's screen, or the
    /// whole display for the main interpreter) and its opacity. It shows
    /// whole for [`FLASH_HOLD`] frames, then fades out over
    /// [`FLASH_FADE`].
    pub fn focus_border(&self) -> Option<([i32; 4], u8)> {
        let (to_sub, age) = self.focus_flash?;
        let sub = self.sub.as_ref();
        if to_sub && !sub.is_some_and(|s| s.vm.is_some() && self.sub_shown) {
            return None;
        }
        let alpha = match age.checked_sub(FLASH_HOLD) {
            None => 255,
            Some(n) if n < FLASH_FADE => 255 - 255 * n / FLASH_FADE,
            Some(_) => return None,
        };
        let rect = match sub {
            Some(sub) if to_sub => sub.rt.sub_screen.unwrap_or([0, 0, 1280, 720]),
            _ => [0, 0, 1280, 720],
        };
        Some((rect, alpha as u8))
    }

    /// Whether the subprogram, rather than the main interpreter, has the
    /// focus.
    pub fn sub_has_focus(&self) -> bool {
        self.sub.as_ref().is_some_and(|s| s.rt.focus)
    }

    /// Gives the focus to the main interpreter (false) or the subprogram,
    /// as ENVFOCUS does, for the frontend's menu.
    pub fn focus_sub(&mut self, sub_focus: bool) {
        self.focus_moving = Some((sub_focus, 0));
        self.move_focus();
    }

    /// Counts down the frames before the focus gets where ENVFOCUS sent
    /// it, and moves it once they are over.
    fn move_focus(&mut self) {
        let Some((to_sub, left)) = self.focus_moving else {
            return;
        };
        if left > 1 {
            self.focus_moving = Some((to_sub, left - 1));
            return;
        }
        self.focus_moving = None;
        self.rt.focus_busy = false;
        if let Some(sub) = &mut self.sub {
            sub.rt.focus_busy = false;
            sub.rt.focus = to_sub && sub.vm.is_some();
            self.rt.focus = !sub.rt.focus;
            self.focus_flash = Some((sub.rt.focus, 0));
            // The keys follow the focus, also those not read yet: a
            // subprogram's PUSHKEY right after ENVFOCUS 0 types into the
            // main interpreter, as SmileBASIC 4's SMILETOOL has it.
            if self.rt.focus {
                self.rt.keys.extend(sub.rt.keys.drain(..));
            }
        }
    }

    /// Whether a subprogram is running.
    pub fn sub_running(&self) -> bool {
        self.sub.as_ref().is_some_and(|s| s.vm.is_some())
    }

    /// The running subprogram's runtime, for the debugger to look into,
    /// and where its screen shows on the 1280x720 display while it does,
    /// as [`Machine::sub_screen`] gives it but without rendering it.
    pub fn sub_runtime(&self) -> Option<(&Runtime, Option<[i32; 4]>)> {
        let sub = self.sub.as_ref().filter(|s| s.vm.is_some())?;
        let place = sub.rt.sub_screen.filter(|_| self.sub_shown);
        Some((&sub.rt, place))
    }

    /// SUBSTOP, for the frontend's menu.
    pub fn stop_sub(&mut self) {
        self.substop();
        if self.vm.is_none() && self.rt.direct {
            self.rt.direct_input_start();
        }
    }

    /// The sound mixed since the last call, the subprogram's with the
    /// main interpreter's as its SNDMSBAL balances them: left and right
    /// in turn.
    pub fn take_samples(&mut self) -> Vec<f32> {
        let mut out = self.rt.sound.take_samples();
        let Some(sub) = &mut self.sub else {
            return out;
        };
        let theirs = sub.rt.sound.take_samples();
        // 0 is the subprogram's sound only, 64 both in full and 127 the
        // main interpreter's only. UNVERIFIED: the curve between.
        let balance = sub.rt.sound.balance as f32;
        let (main_gain, sub_gain) = (
            (balance / 64.0).min(1.0),
            ((127.0 - balance) / 63.0).min(1.0),
        );
        if out.len() < theirs.len() {
            out.resize(theirs.len(), 0.0);
        }
        for (i, s) in out.iter_mut().enumerate() {
            let t = theirs.get(i).copied().unwrap_or(0.0);
            *s = (*s * main_gain + t * sub_gain).clamp(-1.0, 1.0);
        }
        out
    }
}

/// What an interpreter without the focus reads: controllers still
/// connected, but nothing pressed or touched.
fn idle(live: &crate::input::State) -> crate::input::State {
    let mut state = crate::input::State::default();
    for (pad, now) in state.pads.iter_mut().zip(&live.pads) {
        pad.connected = now.connected;
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::Font;
    use crate::palette::Part;
    use crate::runtime::keys;

    /// Draws the editor's frame as the frontend does, and again from
    /// scratch for the same frame; true if the two are alike.
    fn drawn_alike(m: &mut Machine) -> bool {
        let kept = m.render_editor().pixels().to_vec();
        m.rt.frame = m.rt.frame.wrapping_sub(1);
        m.rt.editor_key = None;
        kept == m.render_editor().pixels()
    }

    #[test]
    fn the_editor_is_drawn_again_only_when_it_changes() {
        let mut m = Machine::new(Runtime::new(400, 240, Font::fallback()));
        m.boot();
        m.set_slot(0, "?1\r\n?2\r\nFOR I=0 TO 9\r\n?I\r\nNEXT".to_string());
        m.set_slot(1, "?\"ONE\"".to_string());
        m.toggle_editor();
        type Step = fn(&mut Machine);
        let steps: [Step; 13] = [
            |_| {},
            |m| m.rt.key_press(keys::DOWN),
            |m| m.rt.key_press(keys::END),
            |m| m.type_text("+3"),
            |m| m.rt.key_press(keys::BACKSPACE),
            |m| m.rt.key_press(keys::ENTER),
            |m| m.edit().select_all(),
            |m| _ = m.toggle_breakpoint(0, 2),
            |m| m.show_panel(true),
            |m| m.rt.settings.editor_colors[Part::Background] = 0xFF10_2030,
            |m| m.toggle_split(),
            |m| m.switch_side(),
            |m| m.edit_slot(1),
        ];
        for (i, step) in steps.into_iter().enumerate() {
            step(&mut m);
            m.frame();
            // Through two blinks of the cursor.
            let drawn = m.editor_revision();
            for _ in 0..60 {
                assert!(drawn_alike(&mut m), "step {i}");
            }
            // Each frame above was also drawn from scratch. The ones kept
            // for the frontend are drawn again only after the step, as the
            // cursor blinks, and as a split's outline fades.
            let again = m.editor_revision() - drawn - 60;
            let most = if m.is_split() { 40 } else { 4 };
            assert!(again <= most, "step {i}: drawn again {again} times");
        }
    }
}

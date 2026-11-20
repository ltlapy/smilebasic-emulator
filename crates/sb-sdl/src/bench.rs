//! A timing log of the frontend's drawing, for measuring it without
//! anyone at the keys: `SB_UI_BENCH` sets the emulator up as it says,
//! times the parts of each trip round the loop for a while, prints what
//! they took and quits. It is meant for a scratch data directory (set
//! `XDG_DATA_HOME` on Linux), as it runs the project named there, and
//! with `SDL_VIDEO_DRIVER=offscreen` it needs no display.
//!
//! It counts trips round the frontend's loop. A trip runs one frame of the
//! program as a rule, but none while it is paused and several to catch up,
//! and it may leave the drawing out. The variable's parts and the report
//! call a trip a frame, as they did when each trip ran and drew one.
//!
//! The variable is a comma-separated list:
//!
//! - `frames=N`: trips timed (600 by default);
//! - `settle=N`: trips before the timing begins (60 by default), to get
//!   past what a program sets up;
//! - `run=PROJECT/PROGRAM`: runs a program of a project from the start;
//! - `load=PROJECT/PROGRAM`: loads a program of a project into PRG0;
//! - `end`: stops timing when the program ends, however many trips it was;
//! - `novsync`: presents without waiting for vsync;
//! - `stop`: stops it, as the Break key does, a second in;
//! - `type=TEXT`: half a second after that, types TEXT, with no commas;
//! - `editor`: opens the editor; `apart` in its own window;
//! - `panel`: shows the debugger's panel with it;
//! - `shot=PATH`: saves the frame of the last trip timed of each window shown, as
//!   `PATH-main.pam` and `PATH-editor.pam`, to compare drawings;
//! - `luma=PATH`: reads each frame timed as it goes to the window, writes
//!   its trip's number and mean brightness (0 to 255) to PATH, and reports
//!   the frames darker than both of their neighbours, as a flicker is;
//! - `log=PATH`: writes each trip timed to PATH, at the end of the run: its number,
//!   the window's size in pixels, the drawing's stages in microseconds, the bytes
//!   uploaded, the textures made, and in microseconds what the rest of the trip round
//!   the loop took: events, running the program, waiting for the next frame; then the
//!   frames the standby thread ran while the loop was held up, and its microseconds
//!   running them; then MAINCNT, and the frames so far that the program
//!   missed part of its trip in, by why, as the report's missed line says; then 1
//!   if the trip drew the window and 0 if it left the drawing out; then the
//!   samples of sound waiting in the device's buffer as the trip began, and
//!   the times they have run out so far;
//! - `menu`, `file-menu`, `view-menu`, `size-menu`, `help`, `about`, `colors`,
//!   `settings`, `toast`: shows the menu bar (as F8 does), the bar with its File menu
//!   open or its View menu at Window size or at Size, the help beside the editor, the About dialog, the editor colors window,
//!   the settings window at its Editor page, or a notice, once the rest is
//!   set up;
//! - `cpu-smart`: scales XSCREEN's mode 1 on the CPU, the fallback for a
//!   renderer that cannot make the texture the GPU route needs;
//! - `scale=N`: lays the editor's own window out at the UI scale N instead
//!   of the desktop's;
//! - `slow=MS`: from when the timing begins, makes each present of the
//!   main window that waits for vsync take MS milliseconds more, as a
//!   compositor that throttles a covered window does (SDL's Wayland
//!   backend waits up to 50 ms);
//! - `draw=US`: from then too, makes each drawing of the main window take US
//!   microseconds more, of work rather than of waiting, as a slow GPU does;
//! - `vblank`: from then too, makes each present of the main window that
//!   waits for vsync return at the next sixtieth of a second, as a display
//!   at 60 Hz does, and has the display count as one: the offscreen driver's
//!   presents do not wait;
//! - `occluded`: from then too, has the main window count as covered, as
//!   the system reports it when it can;
//! - `at=TRIP:WHAT`, any number of them: does something to the frontend in
//!   that trip, counted from the start: `targets` or `device` as if the renderer reset them (the
//!   first also spoils what the GPU path holds), `resize:W:H` (`editor-resize:W:H`
//!   for the editor's own window), `fullscreen`,
//!   `windowed`, `auto` and `cpu` for the renderer setting, `tool` to start
//!   the F9 tool as a subprogram and `substop` to end it, `lost` as if the
//!   device were gone, `fail` as if drawing failed, `refuse:N` to have the next N
//!   tries at a renderer refused, `panic`, `unsaved` to give PRG0 unsaved edits,
//!   `sleep:MS` to stop the loop for that long and count it as a suspend, `key:NAME`
//!   to press the key SDL calls NAME, such as `Escape`, with `ctrl+`, `shift+` or
//!   `alt+` before the name to hold those, `usb:USAGE` to have KEYBOARD read that
//!   key as down for a frame, which `key` does not, and `pad:NAME` and `padup:NAME`
//!   to press and let go the first controller's button SDL calls NAME, such as
//!   `b` for the right face button (SDL goes by Xbox labels), as both the event
//!   and what is held, the mouse's `click:X:Y` and the others [`pointer_events`]
//!   lists, with `editor-` before them for the editor's own window, `editor-focus` to give the editor's own window the keys,
//!   and the UI parts above, such as `settings`, to show that part then.
//!
//! Besides the stages, it reports the program frames run per second of
//! the time timed, those of them the standby thread ran, how far MAINCNT
//! went, the frames the program missed part of its trip in, the trips that
//! left the drawing out, the trip the
//! notice for a program slower than real time showed at, and what the
//! sound device's buffer went through and how much sound waited in it, so
//! that the frames keep time whatever happens to the drawing. For the
//! sound, `SDL_AUDIO_DRIVER=dummy` with `SDL_AUDIO_DEVICE_SAMPLE_FRAMES`
//! at 960 takes samples at very nearly the real rate without a device.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sb_runtime::machine::{Machine, Missed};
use sb_runtime::sound::RATE;
use sb_runtime::sound::output::Stats;
use sdl3::event::Event;
use sdl3::keyboard::Mod;
use sdl3::mouse::{MouseButton, MouseState, MouseWheelDirection};
use sdl3::render::Canvas;
use sdl3::video::Window;

use crate::standby::Kept;

/// Bytes given to textures since the trip began.
static UPLOADED: AtomicU64 = AtomicU64::new(0);
/// Textures made since the trip began.
static MADE: AtomicU64 = AtomicU64::new(0);

/// The modifiers before a key's name in `key:`, as in `ctrl+shift+F12`,
/// and the name.
pub fn key_mods(mut name: &str) -> (Mod, &str) {
    let mut keymod = Mod::NOMOD;
    loop {
        let (held, rest) = match name.split_once('+') {
            Some(("ctrl", rest)) => (Mod::LCTRLMOD, rest),
            Some(("shift", rest)) => (Mod::LSHIFTMOD, rest),
            Some(("alt", rest)) => (Mod::LALTMOD, rest),
            _ => return (keymod, name),
        };
        keymod |= held;
        name = rest;
    }
}

/// The mouse events an `at=` part stands for, in the window `window_id`,
/// or `None` if `what` is none of these. X and Y are window coordinates,
/// as SDL gives a pointer's:
///
/// - `click:X:Y`, `rclick:X:Y`, `dclick:X:Y`: moves the pointer there,
///   then presses and releases the left button, the right one, or the
///   left one as the second press of a double click;
/// - `press:X:Y`, `release:X:Y`: moves there and presses or releases the
///   left button, for a drag;
/// - `drag:X:Y`: moves there with the left button held;
/// - `move:X:Y`: moves there with no button held;
/// - `wheel:X:Y:N`: turns the wheel N notches up there, down if negative.
///
/// The events go the way the system's do. What reads the mouse's state
/// and not its events, as the program's touch does, sees none of it.
pub fn pointer_events(what: &str, window_id: u32) -> Option<Vec<Event>> {
    let mut parts = what.split(':');
    let kind = parts.next()?;
    let mut number = || parts.next()?.parse::<f32>().ok();
    let (x, y) = (number()?, number()?);
    let (timestamp, which) = (0, 0);
    let motion = |held: bool| Event::MouseMotion {
        timestamp,
        window_id,
        which,
        mousestate: MouseState::from_sdl_state(u32::from(held)),
        x,
        y,
        xrel: 0.0,
        yrel: 0.0,
    };
    let down = |mouse_btn, clicks| Event::MouseButtonDown {
        timestamp,
        window_id,
        which,
        mouse_btn,
        clicks,
        x,
        y,
    };
    let up = |mouse_btn| Event::MouseButtonUp {
        timestamp,
        window_id,
        which,
        mouse_btn,
        clicks: 1,
        x,
        y,
    };
    let (left, right) = (MouseButton::Left, MouseButton::Right);
    Some(match kind {
        "click" => vec![motion(false), down(left, 1), up(left)],
        "rclick" => vec![motion(false), down(right, 1), up(right)],
        "dclick" => vec![motion(false), down(left, 2), up(left)],
        "press" => vec![motion(false), down(left, 1)],
        "release" => vec![motion(true), up(left)],
        "drag" => vec![motion(true)],
        "move" => vec![motion(false)],
        "wheel" => {
            let notches = number()?;
            vec![
                motion(false),
                Event::MouseWheel {
                    timestamp,
                    window_id,
                    which,
                    x: 0.0,
                    y: notches,
                    direction: MouseWheelDirection::Normal,
                    integer_x: 0,
                    integer_y: notches as i32,
                    mouse_x: x,
                    mouse_y: y,
                },
            ]
        }
        _ => return None,
    })
}

/// Whether presents are to wait for vsync: not if `SB_UI_BENCH` says so.
pub fn wants_vsync() -> bool {
    !std::env::var("SB_UI_BENCH").is_ok_and(|s| s.split(',').any(|p| p.trim() == "novsync"))
}

/// Counts `bytes` given to a texture.
pub fn uploaded(bytes: usize) {
    UPLOADED.fetch_add(bytes as u64, Ordering::Relaxed);
}

/// Counts a texture made.
pub fn made() {
    MADE.fetch_add(1, Ordering::Relaxed);
}

/// The parts of a trip round the loop that are timed.
#[derive(Clone, Copy)]
pub enum Stage {
    /// The screens' or the inline editor's frame, made and uploaded.
    Screen,
    /// The editor's own window's frame.
    Editor,
    /// The debugger's panel: gathering what it shows, and the screens it
    /// shows.
    Gather,
    /// Copying the textures, the panel and the overlays drawn and
    /// uploaded.
    Overlays,
    Present,
    /// Outside the drawing: from the top of the loop through the system's events.
    Events,
    /// The program's frames, and their sound.
    Run,
    /// The wait for the next frame.
    Wait,
}

const STAGES: usize = 8;
/// Of the stages, those that draw.
const DRAWING: usize = Stage::Events as usize;
const NAMES: [&str; STAGES] = [
    "screen", "editor", "gather", "overlays", "present", "events", "run", "wait",
];
/// A trip round the loop this long is listed as a stall: sound starts to run out at
/// about a tenth of a second.
const STALL: Duration = Duration::from_millis(100);
/// Trips from the start before the program is stopped, and before the
/// timing begins after the last step of setting up.
const SETTLE: u32 = 60;

/// How much darker than both neighbours a frame is, out of 255, to count as a dip.
const LEAST_DIP: f32 = 0.5;

/// The frames at least `least` darker than the frames either side of them, with
/// how much.
fn single_frame_dips(lumas: &[f32], least: f32) -> Vec<(usize, f32)> {
    let around = lumas.windows(3).enumerate();
    around
        .map(|(i, w)| (i + 1, w[0].min(w[2]) - w[1]))
        .filter(|&(_, depth)| depth >= least)
        .collect()
}

/// The bench, there whether `SB_UI_BENCH` is set or not, so that the loop
/// calls it without asking. Without the variable it is off: it sets
/// nothing up, times nothing and stands for nothing, as its fields then
/// say, and [`Bench::step`] never ends the run.
#[derive(Default)]
pub struct Bench {
    /// `SB_UI_BENCH` is set.
    on: bool,
    /// Trips timed, and trips before the timing begins, as `frames=` and
    /// `settle=` say.
    trips: u32,
    settle: u32,
    run: Option<(String, String)>,
    load: Option<(String, String)>,
    stop: bool,
    typed: Option<String>,
    until_end: bool,
    editor: bool,
    apart: bool,
    panel: bool,
    shot: Option<String>,
    /// Where the frames' brightness goes, and what it was so far: the number
    /// of the trip that drew the frame, and its mean brightness.
    luma: Option<String>,
    lumas: Vec<(u32, f32)>,
    /// Where each trip's line goes, and the lines so far.
    log: Option<String>,
    lines: Vec<String>,
    /// The window's size in pixels this trip.
    size: (u32, u32),
    /// Added to each present that waits for vsync.
    slow: Duration,
    /// Added to each drawing of the main window, as work.
    draw: Duration,
    /// Presents that wait for vsync return at the next sixtieth of a second.
    vblank: bool,
    occluded: bool,
    /// Whether XSCREEN's mode 1 is scaled on the CPU.
    pub cpu_smart: bool,
    /// The editor's window's UI scale, instead of the desktop's.
    pub ui_scale: Option<u32>,
    /// Parts of the UI the frontend shows, as [`Bench::opens`] tells.
    extras: Vec<String>,
    /// What happens to the frontend at which trip.
    at: Vec<(u32, String)>,
    /// Trips since the start.
    trip: u32,
    /// Each timed trip's stages, and the bytes it uploaded.
    timed: Vec<([Duration; STAGES], u64)>,
    current: [Duration; STAGES],
    /// What the standby thread ran this trip, and in the trips timed.
    kept_now: Kept,
    kept: Kept,
    /// Trips timed in which the standby thread ran frames.
    kept_trips: u32,
    /// MAINCNT when the timing began, and now.
    maincnt: (u32, u32),
    /// The frames the program missed part of its trip in, when the timing
    /// began and now.
    missed: (Missed, Missed),
    /// The machine's count of deferred VSYNCs after the last trip, whether
    /// it rose in that trip, and the stretches of trips in a row in which
    /// it rose, of the trips timed. Once one VSYNC that was behind has
    /// waited for the next frame, the machine counts every VSYNC after it
    /// until two trips of the program fit one frame, though none of those
    /// loses a frame. Each stretch starts with a VSYNC that did wait.
    deferred_seen: u64,
    deferring: bool,
    deferrals: u32,
    /// Whether this trip left the drawing out, and the trips timed that did.
    undrawn_now: bool,
    undrawn: u32,
    last: Option<Instant>,
    /// When the timing began.
    began: Option<Instant>,
    /// Program frames run since then.
    ran: u32,
    /// The sound device's buffer's stats when the timing began, and now.
    sound: Option<(Stats, Stats)>,
    /// The samples waiting in that buffer as this trip began, and as each
    /// trip timed did.
    queued_now: usize,
    queued: Vec<usize>,
    /// The trip the notice for a program slower than real time first
    /// showed at.
    slow_hint: Option<u32>,
}

impl Bench {
    /// The bench `SB_UI_BENCH` asks for, or one that is off.
    pub fn from_env() -> Bench {
        let Ok(spec) = std::env::var("SB_UI_BENCH") else {
            return Bench::default();
        };
        let mut bench = Bench {
            on: true,
            trips: 600,
            settle: SETTLE,
            ..Bench::default()
        };
        for part in spec.split(',').map(str::trim) {
            match part.split_once('=') {
                Some(("frames", n)) => bench.trips = n.parse().unwrap_or(600),
                Some(("settle", n)) => bench.settle = n.parse().unwrap_or(SETTLE),
                Some(("shot", path)) => bench.shot = Some(path.to_string()),
                Some(("luma", path)) => bench.luma = Some(path.to_string()),
                Some(("log", path)) => bench.log = Some(path.to_string()),
                Some(("scale", n)) => bench.ui_scale = n.parse().ok(),
                Some(("type", text)) => bench.typed = Some(text.to_string()),
                Some(("at", what)) => {
                    if let Some((trip, what)) = what.split_once(':') {
                        let trip = trip.parse().unwrap_or(u32::MAX);
                        bench.at.push((trip, what.to_string()));
                    }
                }
                Some(("slow", ms)) => {
                    bench.slow = Duration::from_millis(ms.parse().unwrap_or(50));
                }
                Some(("draw", us)) => {
                    bench.draw = Duration::from_micros(us.parse().unwrap_or(0));
                }
                Some(("load", path)) => {
                    let (project, program) = path.split_once('/').unwrap_or((path, "MAIN.PRG"));
                    bench.load = Some((project.to_string(), program.to_string()));
                }
                Some(("run", path)) => {
                    let (project, program) = path.split_once('/').unwrap_or((path, "MAIN.PRG"));
                    bench.run = Some((project.to_string(), program.to_string()));
                }
                _ => match part {
                    "stop" => bench.stop = true,
                    "end" => bench.until_end = true,
                    "novsync" => {}
                    "editor" => bench.editor = true,
                    "apart" => bench.apart = true,
                    "panel" => bench.panel = true,
                    "vblank" => bench.vblank = true,
                    "occluded" => bench.occluded = true,
                    "cpu-smart" => bench.cpu_smart = true,
                    "menu" | "file-menu" | "view-menu" | "size-menu" | "help" | "about"
                    | "colors" | "settings" | "toast" => bench.extras.push(part.to_string()),
                    "" => {}
                    _ => eprintln!("SB_UI_BENCH: unknown part {part:?}"),
                },
            }
        }
        bench
    }

    /// Whether `SB_UI_BENCH` is set.
    pub fn on(&self) -> bool {
        self.on
    }

    /// Sets the emulator up a step at a time, at the start of each trip
    /// round the loop, and starts timing it. True once the trips are timed.
    pub fn step(&mut self, machine: &mut Machine) -> bool {
        if !self.on {
            return false;
        }
        let stop_at = if self.stop { SETTLE } else { 0 };
        let set_up = stop_at + 1;
        match self.trip {
            0 => {
                machine.rt.settings.editor_window = self.apart;
                if let Some((project, program)) = &self.load {
                    let loaded = machine.rt.storage.set_project(project).is_ok()
                        && machine.load_slot(0, program).is_ok();
                    if !loaded {
                        eprintln!("SB_UI_BENCH: could not load {project}/{program}");
                    }
                }
                if let Some((project, program)) = &self.run {
                    let started = machine.rt.storage.set_project(project).is_ok()
                        && machine.run_file(program).is_ok();
                    if !started {
                        eprintln!("SB_UI_BENCH: could not run {project}/{program}");
                    }
                }
            }
            t if t == stop_at && self.stop => machine.stop(),
            _ => {}
        }
        if self.trip == set_up {
            if self.editor && !machine.is_editing() {
                machine.toggle_editor();
            }
            if self.panel {
                machine.show_panel(true);
            }
        }
        if self.trip == set_up + SETTLE / 2
            && let Some(text) = &self.typed
        {
            machine.type_text(text);
        }
        if self.trip >= set_up + self.settle && self.last.is_some() {
            let uploaded = UPLOADED.load(Ordering::Relaxed);
            self.timed.push((self.current, uploaded));
            self.kept.add(self.kept_now);
            self.kept_trips += u32::from(self.kept_now.frames > 0);
            self.undrawn += u32::from(self.undrawn_now);
            self.queued.push(self.queued_now);
            if self.log.is_some() {
                let stages = self.current.map(|t| t.as_micros().to_string());
                let (drawing, rest) = stages.split_at(DRAWING);
                let (drawing, rest) = (drawing.join(" "), rest.join(" "));
                let made = MADE.load(Ordering::Relaxed);
                let (trip, (w, h)) = (self.trip, self.size);
                let (kept, busy) = (self.kept_now.frames, self.kept_now.busy.as_micros());
                let count = machine.rt.main_count();
                let Missed {
                    cut,
                    held,
                    deferred,
                } = machine.missed;
                let ran_out = self.sound.map_or(0, |(_, now)| now.ran_out);
                self.lines.push(format!(
                    "{trip} {w} {h} {drawing} {uploaded} {made} {rest} {kept} {busy} \
                     {count} {cut} {held} {deferred} {} {} {ran_out}",
                    u8::from(!self.undrawn_now),
                    self.queued_now
                ));
            }
        }
        self.trip += 1;
        self.current = [Duration::ZERO; STAGES];
        self.kept_now = Kept::default();
        self.undrawn_now = false;
        UPLOADED.store(0, Ordering::Relaxed);
        MADE.store(0, Ordering::Relaxed);
        self.last = (self.trip > set_up + self.settle).then(Instant::now);
        if self.last.is_some() && self.began.is_none() {
            self.began = self.last;
            self.ran = 0;
            self.maincnt.0 = machine.rt.main_count();
            self.missed.0 = machine.missed;
            (self.deferring, self.deferrals) = (false, 0);
        }
        let deferred = machine.missed.deferred > self.deferred_seen;
        self.deferrals += u32::from(deferred && !self.deferring);
        (self.deferring, self.deferred_seen) = (deferred, machine.missed.deferred);
        self.maincnt.1 = machine.rt.main_count();
        self.missed.1 = machine.missed;
        let ended = self.until_end && self.trip > set_up + 10 && !machine.is_running();
        ended || self.timed.len() >= self.trips as usize
    }

    /// Notes that the notice for a program slower than real time shows.
    pub fn slow_hint_shown(&mut self) {
        self.slow_hint.get_or_insert(self.trip);
    }

    /// Counts a program frame run.
    pub fn ran_frame(&mut self) {
        self.ran += 1;
    }

    /// Counts the frames the standby thread ran while the loop was held up.
    pub fn kept(&mut self, kept: Kept) {
        self.ran += kept.frames;
        self.kept_now.add(kept);
    }

    /// Notes the sound device's buffer's stats and the samples waiting in
    /// it, if there is a device.
    pub fn sound(&mut self, stats: Option<(Stats, usize)>) {
        let Some((stats, queued)) = stats else {
            return;
        };
        self.queued_now = queued;
        match &mut self.sound {
            Some((_, now)) if self.began.is_some() => *now = stats,
            _ => self.sound = Some((stats, stats)),
        }
    }

    /// Notes the window's size in pixels this trip.
    pub fn window_size(&mut self, size: (u32, u32)) {
        self.size = size;
    }

    /// Whether the main window is to count as covered.
    pub fn occluded(&self) -> bool {
        self.occluded && self.began.is_some()
    }

    /// Stands for a display's vsync and a compositor's throttling, after
    /// a present of the main window; `vsync` is whether it waits for vsync.
    pub fn present(&self, vsync: bool) {
        let Some(began) = self.began.filter(|_| vsync) else {
            return;
        };
        if self.vblank {
            let frame = crate::FRAME.as_nanos();
            let left = frame - began.elapsed().as_nanos() % frame;
            crate::pacing::delay(Duration::from_nanos(left as u64));
        }
        if !self.slow.is_zero() {
            std::thread::sleep(self.slow);
        }
    }

    /// Whether the display is to count as one at 60 Hz, whose vsync the
    /// presents wait for.
    pub fn vblank(&self) -> bool {
        self.vblank
    }

    /// Stands for a slow drawing of the main window, before it.
    pub fn burn(&self) {
        if self.draw.is_zero() || self.began.is_none() {
            return;
        }
        let start = Instant::now();
        while start.elapsed() < self.draw {
            std::hint::spin_loop();
        }
    }

    /// Counts this trip as one that left the drawing out.
    pub fn undrawn(&mut self) {
        self.undrawn_now = true;
    }

    /// Whether this trip is the last one timed, whose frame
    /// [`Bench::shoot`] saves and which must then draw.
    pub fn shoots(&self) -> bool {
        self.shot.is_some() && self.last.is_some() && self.timed.len() + 1 == self.trips as usize
    }

    /// Saves what `canvas` shows as `PATH-name.pam`, in the last trip
    /// timed, before it is presented.
    pub fn shoot(&self, canvas: &Canvas<Window>, name: &str) {
        let (Some(path), Some(_)) = (&self.shot, self.last) else {
            return;
        };
        if self.timed.len() + 1 != self.trips as usize {
            return;
        }
        let shot = canvas
            .read_pixels(None)
            .and_then(|s| s.convert_format(sdl3::pixels::PixelFormat::ABGR8888));
        let Ok(shot) = shot else {
            eprintln!("SB_UI_BENCH: could not read the {name} window");
            return;
        };
        let (w, h, pitch) = (shot.width(), shot.height(), shot.pitch() as usize);
        let mut out =
            format!("P7\nWIDTH {w}\nHEIGHT {h}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n")
                .into_bytes();
        shot.with_lock(|pixels| {
            for row in pixels.chunks(pitch).take(h as usize) {
                out.extend_from_slice(&row[..w as usize * 4]);
            }
        });
        if let Err(e) = std::fs::write(format!("{path}-{name}.pam"), out) {
            eprintln!("SB_UI_BENCH: {e}");
        }
    }

    /// Notes how bright the frame in `canvas` is, before it is presented.
    pub fn measure(&mut self, canvas: &Canvas<Window>) {
        if self.luma.is_none() || self.last.is_none() {
            return;
        }
        let shot = canvas
            .read_pixels(None)
            .and_then(|s| s.convert_format(sdl3::pixels::PixelFormat::ABGR8888));
        let Ok(shot) = shot else {
            eprintln!("SB_UI_BENCH: could not read the window");
            return;
        };
        let (w, h, pitch) = (shot.width() as usize, shot.height(), shot.pitch() as usize);
        let mut sum = 0u64;
        shot.with_lock(|pixels| {
            for row in pixels.chunks(pitch).take(h as usize) {
                for p in row[..w * 4].chunks_exact(4) {
                    // Rec. 601, in fixed point.
                    sum += 299 * u64::from(p[0]) + 587 * u64::from(p[1]) + 114 * u64::from(p[2]);
                }
            }
        });
        let luma = sum as f64 / 1000.0 / (w * h as usize).max(1) as f64;
        self.lumas.push((self.trip, luma as f32));
    }

    /// What is to happen to the frontend in this trip.
    pub fn due(&self) -> Vec<&str> {
        let due = self.at.iter().filter(|(trip, _)| *trip == self.trip);
        due.map(|(_, what)| what.as_str()).collect()
    }

    /// Whether the frontend is to show `part` of its UI now.
    pub fn opens(&self, part: &str) -> bool {
        let set_up = if self.stop { SETTLE } else { 0 } + 1;
        self.trip == set_up + 1 && self.extras.iter().any(|e| e == part)
            || self.due().contains(&part)
    }

    /// Ends a stage of the trip, which began where the last one ended.
    pub fn lap(&mut self, stage: Stage) {
        if let Some(last) = &mut self.last {
            let now = Instant::now();
            self.current[stage as usize] += now - *last;
            *last = now;
        }
    }

    /// Restarts the clock without counting what came since the last lap.
    pub fn skip(&mut self) {
        if let Some(last) = &mut self.last {
            *last = Instant::now();
        }
    }

    /// Writes the brightness of the frames and tells of their dips.
    fn report_luma(&self) {
        let Some(path) = &self.luma else {
            return;
        };
        let lines: Vec<String> = (self.lumas.iter())
            .map(|(trip, luma)| format!("{trip} {luma:.3}"))
            .collect();
        if let Err(e) = std::fs::write(path, lines.join("\n") + "\n") {
            eprintln!("SB_UI_BENCH: {e}");
        }
        let lumas: Vec<f32> = self.lumas.iter().map(|&(_, luma)| luma).collect();
        let dips = single_frame_dips(&lumas, LEAST_DIP);
        let deepest = dips.iter().map(|&(_, depth)| depth).fold(0.0, f32::max);
        let mut gaps: Vec<usize> = dips.windows(2).map(|d| d[1].0 - d[0].0).collect();
        gaps.sort_unstable();
        let least = lumas.iter().copied().fold(f32::MAX, f32::min);
        let most = lumas.iter().copied().fold(0.0, f32::max);
        println!(
            "luma      {} frames from {least:.2} to {most:.2}; {} dips of a frame, deepest {deepest:.2}, \
             frames apart: median {}",
            lumas.len(),
            dips.len(),
            gaps.get(gaps.len() / 2).copied().unwrap_or(0),
        );
    }

    /// Prints each stage's mean, median, 99th percentile and longest
    /// time, the same for their sum without presenting, and the bytes
    /// uploaded.
    pub fn report(&self, renderer: &str) {
        println!("{} frames timed, renderer {renderer}", self.timed.len());
        self.report_luma();
        if let Some(path) = &self.log {
            let (drawing, rest) = NAMES.split_at(DRAWING);
            let (drawing, rest) = (drawing.join(" "), rest.join(" "));
            let head = format!(
                "frame w h {drawing} uploaded made {rest} kept worker maincnt cut held deferred \
                 drawn queued ranout\n"
            );
            let text = head + &self.lines.join("\n") + "\n";
            if let Err(e) = std::fs::write(path, text) {
                eprintln!("SB_UI_BENCH: {e}");
            }
        }
        let line = |name: &str, mut times: Vec<Duration>| {
            if times.is_empty() {
                return;
            }
            let total: Duration = times.iter().sum();
            times.sort();
            let n = times.len();
            println!(
                "{name:<9} mean {:>9.3?}  median {:>9.3?}  99% {:>9.3?}  max {:>9.3?}",
                total / n as u32,
                times[n / 2],
                times[n * 99 / 100],
                times[n - 1],
            );
        };
        for (i, name) in NAMES.iter().enumerate() {
            line(name, self.timed.iter().map(|(t, _)| t[i]).collect());
        }
        // Trips the screens were drawn again in, as a heavy scene's are not
        // all of them.
        let screen = Stage::Screen as usize;
        let mut drawn: Vec<&[Duration; STAGES]> = self.timed.iter().map(|(t, _)| t).collect();
        let longest = drawn.iter().map(|t| t[screen]).max().unwrap_or_default();
        drawn.retain(|t| t[screen] * 4 >= longest);
        line("busy scr", drawn.iter().map(|t| t[screen]).collect());
        line(
            "busy pres",
            drawn.iter().map(|t| t[Stage::Present as usize]).collect(),
        );
        let drawing = |t: &[Duration; STAGES]| t[..Stage::Present as usize].iter().sum();
        let whole = |t: &[Duration; STAGES]| t.iter().sum::<Duration>();
        line("whole", self.timed.iter().map(|(t, _)| whole(t)).collect());
        let stalls: Vec<Duration> = (self.timed.iter().map(|(t, _)| whole(t)))
            .filter(|&t| t >= STALL)
            .collect();
        println!(
            "stalls    {} trips of {STALL:?} or more, {:.2?} in all",
            stalls.len(),
            stalls.iter().sum::<Duration>()
        );
        line(
            "drawing",
            self.timed.iter().map(|(t, _)| drawing(t)).collect(),
        );
        if let Some(began) = self.began {
            let secs = began.elapsed().as_secs_f64();
            println!(
                "frames    {:.2} run a second, {} in {secs:.2} s",
                self.ran as f64 / secs,
                self.ran
            );
            let counted = self.maincnt.1.wrapping_sub(self.maincnt.0);
            println!(
                "maincnt   {:.2} a second, {counted} in all",
                counted as f64 / secs
            );
            let (from, to) = self.missed;
            println!(
                "missed    {} frames out of time mid-trip, {} held at VSYNC, {} VSYNCs deferred \
                 in {} stretches",
                to.cut - from.cut,
                to.held - from.held,
                to.deferred - from.deferred,
                self.deferrals
            );
        }
        println!(
            "undrawn   {} of {} trips ran the program and left the drawing out",
            self.undrawn,
            self.timed.len()
        );
        println!(
            "kept      {} frames run while the loop was held up, in {} trips, {:.2?} running them",
            self.kept.frames, self.kept_trips, self.kept.busy
        );
        match self.slow_hint {
            Some(trip) => println!("slow hint shown at frame {trip}"),
            None => println!("slow hint not shown"),
        }
        if let Some((from, to)) = self.sound {
            println!(
                "sound     ran out {}, skipped {}, {:.1} ms silent",
                to.ran_out - from.ran_out,
                to.skips - from.skips,
                (to.silent - from.silent) as f64 * 1000.0 / RATE as f64
            );
            let mut queued = self.queued.clone();
            queued.sort_unstable();
            let ms = |samples: usize| samples as f64 * 1000.0 / RATE as f64;
            let (least, median) = (queued.first(), queued.get(queued.len() / 2));
            if let (Some(&least), Some(&median)) = (least, median) {
                println!(
                    "queued    {:.1} ms of sound waiting as a trip began at least, median {:.1} ms",
                    ms(least),
                    ms(median)
                );
            }
        }
        let bytes: u64 = self.timed.iter().map(|&(_, b)| b).sum();
        let trips = self.timed.len().max(1) as u64;
        let uploads = self.timed.iter().filter(|&&(_, b)| b > 0).count();
        println!(
            "uploaded  {:.1} KB a frame, in {uploads} of {} frames",
            bytes as f64 / trips as f64 / 1024.0,
            self.timed.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_take_modifiers_before_their_names() {
        assert_eq!(key_mods("Escape"), (Mod::NOMOD, "Escape"));
        let (keymod, name) = key_mods("ctrl+shift+F12");
        assert_eq!((keymod, name), (Mod::LCTRLMOD | Mod::LSHIFTMOD, "F12"));
        // The key itself may be a plus.
        assert_eq!(key_mods("shift++"), (Mod::LSHIFTMOD, "+"));
    }

    #[test]
    fn a_click_is_a_move_a_press_and_a_release() {
        let events = pointer_events("click:120:40.5", 7).expect("a click");
        let at = |e: &Event| match *e {
            Event::MouseMotion {
                window_id, x, y, ..
            }
            | Event::MouseButtonDown {
                window_id, x, y, ..
            }
            | Event::MouseButtonUp {
                window_id, x, y, ..
            } => (window_id, x, y),
            _ => panic!("{e:?}"),
        };
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|e| at(e) == (7, 120.0, 40.5)));
        assert!(matches!(
            events[1],
            Event::MouseButtonDown { clicks: 1, .. }
        ));
        let double = pointer_events("dclick:1:2", 7).expect("a double click");
        assert!(matches!(
            double[1],
            Event::MouseButtonDown { clicks: 2, .. }
        ));
        // A drag holds the left button from its press to its release.
        let held = |what| match pointer_events(what, 7).expect("a move")[0] {
            Event::MouseMotion { mousestate, .. } => mousestate.left(),
            ref e => panic!("{e:?}"),
        };
        assert!(!held("press:1:2") && held("drag:3:4") && held("release:5:6"));
        assert!(!held("move:1:2"));
        let wheel = pointer_events("wheel:10:20:-3", 7).expect("a wheel");
        assert!(matches!(
            wheel[1],
            Event::MouseWheel {
                integer_y: -3,
                mouse_x: 10.0,
                mouse_y: 20.0,
                ..
            }
        ));
        // Other parts, and ones short of a number, are not the mouse's.
        for what in [
            "key:Escape",
            "click:1",
            "click:a:b",
            "wheel:1:2",
            "resize:640:360",
        ] {
            assert!(pointer_events(what, 7).is_none(), "{what}");
        }
    }

    #[test]
    fn a_dip_is_one_frame_darker_than_both_sides() {
        let lumas = [10.0, 10.0, 8.0, 10.0, 9.0, 8.0, 7.0, 7.0, 10.0, 9.8, 10.0];
        // Not a slide down, a step, or what is too slight to see.
        assert_eq!(single_frame_dips(&lumas, 0.5), [(2, 2.0)]);
        assert_eq!(single_frame_dips(&lumas, 0.1).len(), 2);
    }
}

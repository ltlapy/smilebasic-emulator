//! A timing log of the frontend's drawing, for measuring it without
//! anyone at the keys: `SB_UI_BENCH` sets the emulator up as it says,
//! times the parts of each frame's drawing for a while, prints what they
//! took and quits. It is meant for a scratch data directory (set
//! `XDG_DATA_HOME` on Linux), as it runs the project named there, and
//! with `SDL_VIDEO_DRIVER=offscreen` it needs no display.
//!
//! The variable is a comma-separated list:
//!
//! - `frames=N`: frames timed (600 by default);
//! - `settle=N`: frames drawn before the timing begins (60 by default), to
//!   get past what a program sets up;
//! - `run=PROJECT/PROGRAM`: runs a program of a project from the start;
//! - `end`: stops timing when the program ends, however many frames it was;
//! - `novsync`: presents without waiting for vsync;
//! - `stop`: stops it, as the Break key does, a second in;
//! - `editor`: opens the editor; `apart` in its own window;
//! - `panel`: shows the debugger's panel with it;
//! - `shot=PATH`: saves the last frame timed of each window shown, as
//!   `PATH-main.pam` and `PATH-editor.pam`, to compare drawings;
//! - `menu`, `help`, `about`, `colors`, `toast`: shows the menu bar (as
//!   F8 does), the help beside the editor, the About dialog, the editor
//!   colors window, or a notice, once the rest is set up;
//! - `cpu-smart`: scales XSCREEN's mode 1 on the CPU, the fallback for a
//!   renderer that cannot make the texture the GPU route needs;
//! - `slow=MS`: from when the timing begins, makes each present of the
//!   main window that waits for vsync take MS milliseconds more, as a
//!   compositor that throttles a covered window does (SDL's Wayland
//!   backend waits up to 50 ms);
//! - `occluded`: from then too, has the main window count as covered, as
//!   the system reports it when it can.
//!
//! Besides the stages, it reports the program frames run per second of
//! the time timed and what the sound device's buffer went through, so
//! that the frames keep time whatever happens to the drawing. For the
//! sound, `SDL_AUDIO_DRIVER=dummy` with `SDL_AUDIO_DEVICE_SAMPLE_FRAMES`
//! at 960 takes samples at very nearly the real rate without a device.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sb_runtime::machine::Machine;
use sb_runtime::sound::RATE;
use sb_runtime::sound::output::Stats;
use sdl3::render::Canvas;
use sdl3::video::Window;

/// Bytes given to textures since the frame began.
static UPLOADED: AtomicU64 = AtomicU64::new(0);

/// Whether presents are to wait for vsync: not if `SB_UI_BENCH` says so.
pub fn wants_vsync() -> bool {
    !std::env::var("SB_UI_BENCH").is_ok_and(|s| s.split(',').any(|p| p.trim() == "novsync"))
}

/// Counts `bytes` given to a texture.
pub fn uploaded(bytes: usize) {
    UPLOADED.fetch_add(bytes as u64, Ordering::Relaxed);
}

/// The parts of a frame's drawing that are timed.
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
}

const STAGES: usize = 5;
const NAMES: [&str; STAGES] = ["screen", "editor", "gather", "overlays", "present"];
/// Frames from the start before the program is stopped, and before the
/// timing begins after the last step of setting up.
const SETTLE: u32 = 60;

#[derive(Default)]
pub struct Bench {
    frames: u32,
    settle: u32,
    run: Option<(String, String)>,
    stop: bool,
    until_end: bool,
    editor: bool,
    apart: bool,
    panel: bool,
    shot: Option<String>,
    /// Added to each present that waits for vsync.
    slow: Duration,
    occluded: bool,
    /// Whether XSCREEN's mode 1 is scaled on the CPU.
    pub cpu_smart: bool,
    /// Parts of the UI the frontend shows, as [`Bench::opens`] tells.
    extras: Vec<String>,
    /// Frames since the start.
    frame: u32,
    /// Each timed frame's stages, and the bytes it uploaded.
    timed: Vec<([Duration; STAGES], u64)>,
    current: [Duration; STAGES],
    last: Option<Instant>,
    /// When the timing began.
    began: Option<Instant>,
    /// Program frames run since then.
    ran: u32,
    /// The sound device's buffer's stats when the timing began, and now.
    sound: Option<(Stats, Stats)>,
}

impl Bench {
    /// The bench `SB_UI_BENCH` asks for, if any.
    pub fn from_env() -> Option<Bench> {
        let spec = std::env::var("SB_UI_BENCH").ok()?;
        let mut bench = Bench {
            frames: 600,
            settle: SETTLE,
            ..Bench::default()
        };
        for part in spec.split(',').map(str::trim) {
            match part.split_once('=') {
                Some(("frames", n)) => bench.frames = n.parse().unwrap_or(600),
                Some(("settle", n)) => bench.settle = n.parse().unwrap_or(SETTLE),
                Some(("shot", path)) => bench.shot = Some(path.to_string()),
                Some(("slow", ms)) => {
                    bench.slow = Duration::from_millis(ms.parse().unwrap_or(50));
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
                    "occluded" => bench.occluded = true,
                    "cpu-smart" => bench.cpu_smart = true,
                    "menu" | "help" | "about" | "colors" | "toast" => {
                        bench.extras.push(part.to_string())
                    }
                    "" => {}
                    _ => eprintln!("SB_UI_BENCH: unknown part {part:?}"),
                },
            }
        }
        Some(bench)
    }

    /// Sets the emulator up a step at a time, at the start of each frame,
    /// and starts timing it. True once the frames are timed.
    pub fn step(&mut self, machine: &mut Machine) -> bool {
        let stop_at = if self.stop { SETTLE } else { 0 };
        let set_up = stop_at + 1;
        match self.frame {
            0 => {
                machine.rt.settings.editor_window = self.apart;
                if let Some((project, program)) = &self.run {
                    let started = machine.rt.storage.set_project(project).is_ok()
                        && machine.run_file(program).is_ok();
                    if !started {
                        eprintln!("SB_UI_BENCH: could not run {project}/{program}");
                    }
                }
            }
            f if f == stop_at && self.stop => machine.stop(),
            _ => {}
        }
        if self.frame == set_up {
            if self.editor && !machine.is_editing() {
                machine.toggle_editor();
            }
            if self.panel {
                machine.show_panel(true);
            }
        }
        if self.frame >= set_up + self.settle && self.last.is_some() {
            self.timed
                .push((self.current, UPLOADED.load(Ordering::Relaxed)));
        }
        self.frame += 1;
        self.current = [Duration::ZERO; STAGES];
        UPLOADED.store(0, Ordering::Relaxed);
        self.last = (self.frame > set_up + self.settle).then(Instant::now);
        if self.last.is_some() && self.began.is_none() {
            self.began = self.last;
            self.ran = 0;
        }
        let ended = self.until_end && self.frame > set_up + 10 && !machine.is_running();
        ended || self.timed.len() >= self.frames as usize
    }

    /// Counts a program frame run.
    pub fn ran_frame(&mut self) {
        self.ran += 1;
    }

    /// Notes the sound device's buffer's stats, if there is a device.
    pub fn sound(&mut self, stats: Option<Stats>) {
        let Some(stats) = stats else {
            return;
        };
        match &mut self.sound {
            Some((_, now)) if self.began.is_some() => *now = stats,
            _ => self.sound = Some((stats, stats)),
        }
    }

    /// Whether the main window is to count as covered.
    pub fn occluded(&self) -> bool {
        self.occluded && self.began.is_some()
    }

    /// Stands for a compositor's throttling, after a present of the main
    /// window; `vsync` is whether it waits for vsync.
    pub fn present(&self, vsync: bool) {
        if vsync && !self.slow.is_zero() && self.began.is_some() {
            std::thread::sleep(self.slow);
        }
    }

    /// Saves what `canvas` shows as `PATH-name.pam`, in the last frame
    /// timed, before it is presented.
    pub fn shoot(&self, canvas: &Canvas<Window>, name: &str) {
        let (Some(path), Some(_)) = (&self.shot, self.last) else {
            return;
        };
        if self.timed.len() + 1 != self.frames as usize {
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

    /// Whether the frontend is to show `part` of its UI now.
    pub fn opens(&self, part: &str) -> bool {
        let set_up = if self.stop { SETTLE } else { 0 } + 1;
        self.frame == set_up + 1 && self.extras.iter().any(|e| e == part)
    }

    /// Ends a stage of the frame's drawing, which began where the last
    /// one ended.
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

    /// Prints each stage's mean, median, 99th percentile and longest
    /// time, the same for their sum without presenting, and the bytes
    /// uploaded.
    pub fn report(&self, renderer: &str) {
        println!("{} frames timed, renderer {renderer}", self.timed.len());
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
        // Frames the screens were drawn again in, as a heavy scene's are not
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
        }
        if let Some((from, to)) = self.sound {
            println!(
                "sound     ran out {}, skipped {}, {:.1} ms silent",
                to.ran_out - from.ran_out,
                to.skips - from.skips,
                (to.silent - from.silent) as f64 * 1000.0 / RATE as f64
            );
        }
        let bytes: u64 = self.timed.iter().map(|&(_, b)| b).sum();
        let frames = self.timed.len().max(1) as u64;
        let uploads = self.timed.iter().filter(|&&(_, b)| b > 0).count();
        println!(
            "uploaded  {:.1} KB a frame, in {uploads} of {} frames",
            bytes as f64 / frames as f64 / 1024.0,
            self.timed.len()
        );
    }
}

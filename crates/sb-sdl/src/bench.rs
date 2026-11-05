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
//! - `type=TEXT`: half a second after that, types TEXT, with no commas;
//! - `editor`: opens the editor; `apart` in its own window;
//! - `panel`: shows the debugger's panel with it;
//! - `shot=PATH`: saves the last frame timed of each window shown, as
//!   `PATH-main.pam` and `PATH-editor.pam`, to compare drawings;
//! - `luma=PATH`: reads each frame timed as it goes to the window, writes
//!   its frame number and mean brightness (0 to 255) to PATH, and reports
//!   the frames darker than both of their neighbours, as a flicker is;
//! - `log=PATH`: writes each frame timed to PATH, at the end of the run: its number,
//!   the window's size in pixels, the drawing's stages in microseconds, the bytes
//!   uploaded, the textures made, and in microseconds what the rest of the trip round
//!   the loop took: events, running the program, waiting for the next frame;
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
//!   the system reports it when it can;
//! - `at=FRAME:WHAT`, any number of them: does something to the frontend at
//!   that frame: `targets` or `device` as if the renderer reset them (the
//!   first also spoils what the GPU path holds), `resize:W:H`, `fullscreen`,
//!   `windowed`, `auto` and `cpu` for the renderer setting, `tool` to start
//!   the F9 tool as a subprogram and `substop` to end it, `lost` as if the
//!   device were gone, `fail` as if drawing failed, `refuse:N` to have the next N
//!   tries at a renderer refused, `panic`, `unsaved` to give PRG0 unsaved edits,
//!   `sleep:MS` to stop the loop, as a suspend does, `key:NAME` to press the key
//!   SDL calls NAME, such as `Escape`, `usb:USAGE` to have KEYBOARD read that
//!   key as down for a frame, which `key` does not, and `pad:NAME` and `padup:NAME`
//!   to press and let go the first controller's button SDL calls NAME, such as
//!   `b` for the right face button (SDL goes by Xbox labels), as both the event
//!   and what is held.
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
/// Textures made since the frame began.
static MADE: AtomicU64 = AtomicU64::new(0);

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
/// Frames from the start before the program is stopped, and before the
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

#[derive(Default)]
pub struct Bench {
    frames: u32,
    settle: u32,
    run: Option<(String, String)>,
    stop: bool,
    typed: Option<String>,
    until_end: bool,
    editor: bool,
    apart: bool,
    panel: bool,
    shot: Option<String>,
    /// Where the frames' brightness goes, and what it was so far: the frame's
    /// number and its mean brightness.
    luma: Option<String>,
    lumas: Vec<(u32, f32)>,
    /// Where each frame's line goes, and the lines so far.
    log: Option<String>,
    lines: Vec<String>,
    /// The window's size in pixels this frame.
    size: (u32, u32),
    /// Added to each present that waits for vsync.
    slow: Duration,
    occluded: bool,
    /// Whether XSCREEN's mode 1 is scaled on the CPU.
    pub cpu_smart: bool,
    /// Parts of the UI the frontend shows, as [`Bench::opens`] tells.
    extras: Vec<String>,
    /// What happens to the frontend at which frame.
    at: Vec<(u32, String)>,
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
                Some(("luma", path)) => bench.luma = Some(path.to_string()),
                Some(("log", path)) => bench.log = Some(path.to_string()),
                Some(("type", text)) => bench.typed = Some(text.to_string()),
                Some(("at", what)) => {
                    if let Some((frame, what)) = what.split_once(':') {
                        let frame = frame.parse().unwrap_or(u32::MAX);
                        bench.at.push((frame, what.to_string()));
                    }
                }
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
        if self.frame == set_up + SETTLE / 2 {
            if let Some(text) = &self.typed {
                machine.type_text(text);
            }
        }
        if self.frame >= set_up + self.settle && self.last.is_some() {
            let uploaded = UPLOADED.load(Ordering::Relaxed);
            self.timed.push((self.current, uploaded));
            if self.log.is_some() {
                let stages = self.current.map(|t| t.as_micros().to_string());
                let (drawing, rest) = stages.split_at(DRAWING);
                let (drawing, rest) = (drawing.join(" "), rest.join(" "));
                let made = MADE.load(Ordering::Relaxed);
                let (frame, (w, h)) = (self.frame, self.size);
                self.lines.push(format!(
                    "{frame} {w} {h} {drawing} {uploaded} {made} {rest}"
                ));
            }
        }
        self.frame += 1;
        self.current = [Duration::ZERO; STAGES];
        UPLOADED.store(0, Ordering::Relaxed);
        MADE.store(0, Ordering::Relaxed);
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

    /// Notes the window's size in pixels this frame.
    pub fn window_size(&mut self, size: (u32, u32)) {
        self.size = size;
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
        self.lumas.push((self.frame, luma as f32));
    }

    /// What is to happen to the frontend at this frame.
    pub fn due(&self) -> Vec<&str> {
        let due = self.at.iter().filter(|(frame, _)| *frame == self.frame);
        due.map(|(_, what)| what.as_str()).collect()
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

    /// Writes the brightness of the frames and tells of their dips.
    fn report_luma(&self) {
        let Some(path) = &self.luma else {
            return;
        };
        let lines: Vec<String> = (self.lumas.iter())
            .map(|(frame, luma)| format!("{frame} {luma:.3}"))
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
            let head = format!("frame w h {drawing} uploaded made {rest}\n");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dip_is_one_frame_darker_than_both_sides() {
        let lumas = [10.0, 10.0, 8.0, 10.0, 9.0, 8.0, 7.0, 7.0, 10.0, 9.8, 10.0];
        // Not a slide down, a step, or what is too slight to see.
        assert_eq!(single_frame_dips(&lumas, 0.5), [(2, 2.0)]);
        assert_eq!(single_frame_dips(&lumas, 0.1).len(), 2);
    }
}

//! `smilebasic-check`: runs a hardware check program on the emulator and
//! compares the screens it saved with the ones SmileBASIC 4 saved.
//!
//! Check programs end each part by saving the console's text with SAVEV
//! into a TXT file whose name starts with `RES` (see
//! docs/hardware-tests.md). Run on SmileBASIC 4, in Ryujinx, those files
//! land in the project's folder; this runs the same parts here, on a copy
//! of that folder and the projects beside it, and compares them row by
//! row.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sb_profiles::Profile;
use sb_runtime::confirm::FileConfirm;
use sb_runtime::font::Font;
use sb_runtime::framebuffer::Framebuffer;
use sb_runtime::input::State;
use sb_runtime::machine::Machine;
use sb_runtime::runtime::keys;
use sb_runtime::screen::ScreenMode;
use sb_runtime::sound::synth::Instruments;
use sb_runtime::sprites::Sprites;
use sb_runtime::storage::Storage;
use sb_runtime::text::CONSOLE;
use sb_runtime::{Runtime, image, resources, sbfile};

const USAGE: &str = "usage:
  smilebasic-check [--screen DIR] PROJECT PROGRAM [PARTS]
      run PROGRAM (its name in SmileBASIC 4, such as TEST12) from the
      SmileBASIC 4 project folder PROJECT on the emulator, once for each
      part in PARTS (such as 1-26 or 2,5,7), typing the part number at
      its INPUT. The RES* text files each run saves are compared with the
      ones SmileBASIC 4 saved into PROJECT; where there are none yet,
      ours are printed. The projects beside PROJECT are copied too, with
      ### as the workspace.
      With --screen, each part's screen is also saved into DIR as a PNG
      once it has run for two seconds, and then Enter is pressed, for
      parts that show a picture and wait for a key to end.
  smilebasic-check --bench FRAMES PROJECT PROGRAM [PNG]
      run PROGRAM for FRAMES frames, rendering each one, and print how
      long running and rendering took, the slowest frames, and the
      console at the end; with PNG, also save the last screen there.
  smilebasic-check --bench --fixed FRAMES PROJECT PROGRAM [PNG]
      the same on a fresh copy of PROJECT and the projects beside it, with
      time following the frame count and fixed random seeds, so that each
      run does the same work, for comparing builds with perf. It also
      prints a checksum of screens and console: only runs with the same
      checksum did the same work. FRAMES 0 shows what start-up costs.
      Options, after --bench and --fixed:
        --script FILE  give the program the input FILE asks for, in the
                       form --play takes; shots go beside PNG, or into
                       the current folder
        --log FILE     write each frame's times to FILE as CSV: frame,
                       run_us, draw_us, instructions, vsyncs (those the
                       program reached), redrawn, end_us (since the start)
        --window       run frames as the window does: to the clock, with
                       its time limits, catching up without drawing. Not
                       with --fixed, as the work then follows the clock
        --slower N     with --window, the clock and limits divided by N,
                       to stand for a machine N times slower than this
  smilebasic-check --play FRAMES PROJECT PROGRAM SCRIPT DIR
      run PROGRAM for at most FRAMES frames, giving it the input SCRIPT
      (a file, or - for none) asks for, and save the screens it asks for
      into DIR, the last one as end.png. Direct mode goes on after the
      program while the script has more, such as a SUBRUN line, and
      while a subprogram runs. Like --bench, the program runs
      in PROJECT itself, so give it a copy if it may save files. Each
      SCRIPT line is a frame number and what happens then:
        F key NAME        a key typed for INKEY$ and INPUT: ENTER, UP,
                          DOWN, LEFT, RIGHT, ESC, TAB, BACKSPACE,
                          DELETE, HOME, END, INSERT, SELECTLEFT,
                          SELECTRIGHT (Shift with the arrow keys), SPACE
                          or one character
        F type TEXT       the rest of the line typed
        F button NAMES N  buttons held for N frames, NAMES such as A or
                          A+UP: A B X Y UP DOWN LEFT RIGHT L R ZL ZR
                          LSTICK RSTICK
        F stick X Y N     the left stick tilted (-1 to 1) for N frames
        F usb ID N        the keyboard key with USB usage ID (such as
                          0x28 for Enter) held for N frames, for KEYBOARD
        F touch X Y N     the screen touched at X,Y for N frames; the
                          pointer stays there afterwards; the
                          pointer stays there afterwards
        F shot NAME       the screen saved as DIR/NAME.png
        F console         the console's text printed
        F end             the run stops
      Lines starting with # are comments.";

/// Result files start with this, after the TXT type letter.
const RESULT_PREFIX: &str = "TRES";
/// How long a part may run, in frames: five minutes at 60 a second.
/// Frames before the part's number is typed, as a person takes: VSYNC
/// counts from the VSYNC before INPUT, so `VSYNC 60` right after it
/// returns at once on hardware.
const TYPING_FRAMES: u32 = 180;
const FRAME_LIMIT: u32 = 5 * 60 * 60;
/// Frames after the part's number before --screen takes the picture.
const SCREEN_AFTER: u32 = 120;

type AnyError = Box<dyn std::error::Error>;

fn main() {
    if let Err(e) = run(std::env::args().skip(1).collect()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run(mut args: Vec<String>) -> Result<(), AnyError> {
    if args.first().is_some_and(|a| a == "--bench") {
        let fixed = args.get(1).is_some_and(|a| a == "--fixed");
        if fixed {
            args.remove(1);
        }
        let mut take = |name: &str| -> Result<Option<String>, AnyError> {
            match args.iter().position(|a| a == name) {
                Some(at) if at + 1 < args.len() => {
                    let value = args.remove(at + 1);
                    args.remove(at);
                    Ok(Some(value))
                }
                Some(_) => Err(USAGE.into()),
                None => Ok(None),
            }
        };
        let script = take("--script")?;
        let log = take("--log")?.map(PathBuf::from);
        let take_slower = take("--slower")?;
        let window = match args.iter().position(|a| a == "--window") {
            Some(at) => {
                args.remove(at);
                true
            }
            None => false,
        };
        if fixed && window {
            return Err(USAGE.into());
        }
        let speed = match take_slower {
            Some(n) => n.parse::<f64>().map_err(|_| USAGE)?,
            None => 1.0,
        };
        return match args.as_slice() {
            [_, frames, project, program, png @ ..] if png.len() <= 1 => {
                let options = BenchOptions {
                    frames: frames.parse().map_err(|_| USAGE)?,
                    png: png.first().map(Path::new),
                    steps: read_script(script.as_deref())?,
                    log,
                    fixed,
                    window: window.then_some(speed),
                };
                match fixed {
                    true => bench_fixed(&options, Path::new(project), program),
                    false => bench(&options, Path::new(project), program),
                }
            }
            _ => Err(USAGE.into()),
        };
    }
    if args.first().is_some_and(|a| a == "--play") {
        return match args.as_slice() {
            [_, frames, project, program, script, dir] => play(
                frames.parse().map_err(|_| USAGE)?,
                Path::new(project),
                program,
                script,
                Path::new(dir),
            ),
            _ => Err(USAGE.into()),
        };
    }
    let screens = match args.iter().position(|a| a == "--screen") {
        Some(at) if at + 1 < args.len() => {
            let dir = PathBuf::from(args.remove(at + 1));
            args.remove(at);
            std::fs::create_dir_all(&dir)?;
            Some(dir)
        }
        Some(_) => return Err(USAGE.into()),
        None => None,
    };
    let (project, program, parts) = match args.as_slice() {
        [project, program] => (project, program, vec![None]),
        [project, program, parts] => (project, program, parse_parts(parts)?),
        _ => return Err(USAGE.into()),
    };
    let project = Path::new(project);
    let name = project
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("PROJECT must be a project folder")?;
    let scratch = std::env::temp_dir().join(format!("smilebasic-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let outcome = copy_others(project, name, &scratch).and_then(|()| {
        parts.iter().try_for_each(|&part| {
            check_part(project, name, program, part, &scratch, screens.as_deref())
        })
    });
    let _ = std::fs::remove_dir_all(&scratch);
    outcome
}

/// Parts such as `1-26` or `2,5,7`.
fn parse_parts(spec: &str) -> Result<Vec<Option<u32>>, AnyError> {
    let mut parts = Vec::new();
    for item in spec.split(',') {
        let bad = || format!("bad part list: {spec}");
        let (from, to) = item.split_once('-').unwrap_or((item, item));
        let from: u32 = from.trim().parse().map_err(|_| bad())?;
        let to: u32 = to.trim().parse().map_err(|_| bad())?;
        parts.extend((from..=to).map(Some));
    }
    Ok(parts)
}

fn check_part(
    project: &Path,
    name: &str,
    program: &str,
    part: Option<u32>,
    scratch: &Path,
    screens: Option<&Path>,
) -> Result<(), AnyError> {
    // Each part starts from the project as SmileBASIC 4 had it, less the
    // results, so a part that saves nothing here shows as such.
    let copy = scratch.join(name);
    let _ = std::fs::remove_dir_all(&copy);
    std::fs::create_dir_all(&copy)?;
    for entry in std::fs::read_dir(project)? {
        let entry = entry?;
        let file = entry.file_name();
        if entry.file_type()?.is_file() && !is_result(&file.to_string_lossy()) {
            std::fs::copy(entry.path(), copy.join(file))?;
        }
    }

    let mut m = machine(scratch.to_path_buf())?;
    m.rt.storage
        .set_project(name)
        .map_err(|e| format!("{name}: {e}"))?;
    m.run_file(program).map_err(|e| format!("{program}: {e}"))?;
    let mut answer = part.map(|p| p.to_string());
    let mut frames = 0;
    // Frames since the part's number went in, for --screen.
    let mut shown = part.is_none().then_some(0);
    // Frames the program has waited at INPUT.
    let mut asked = 0;
    while m.is_running() && frames < FRAME_LIMIT {
        asked = if m.rt.wants_line() { asked + 1 } else { 0 };
        if asked > TYPING_FRAMES
            && let Some(text) = answer.take()
        {
            m.type_text(&text);
            m.rt.key_press(keys::ENTER);
            shown = Some(0);
        }
        m.frame();
        frames += 1;
        if let (Some(dir), Some(n)) = (screens, shown.as_mut()) {
            *n += 1;
            if *n == SCREEN_AFTER {
                let suffix = part.map_or(String::new(), |p| format!("-{p}"));
                let file = dir.join(format!("{program}{suffix}.png"));
                save_screen(&mut m, &file)?;
                println!("screen saved to {}", file.display());
                m.rt.key_press(keys::ENTER);
            }
        }
    }

    let title = part.map_or(String::new(), |p| format!(" part {p}"));
    println!("== {program}{title}");
    if m.is_running() {
        println!("still running after {} s; stopped", FRAME_LIMIT / 60);
    }
    let ours = results(&copy)?;
    if ours.is_empty() {
        println!("saved nothing here; the console:");
        print_rows(&console_rows(&m));
    }
    for (file, rows) in &ours {
        let theirs = project.join(file);
        if !theirs.is_file() {
            println!("{file}: no SmileBASIC 4 result yet; ours:");
            print_rows(rows);
            continue;
        }
        let theirs = screen_rows(&read_text(&theirs)?);
        compare(file, &theirs, rows);
    }
    Ok(())
}

/// Runs a program as the window would, timing each frame's two halves.
/// `--bench --fixed`: on a fresh copy of the project and those beside it,
/// so that nothing a run saves changes the next.
fn bench_fixed(options: &BenchOptions, project: &Path, program: &str) -> Result<(), AnyError> {
    let name = project
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("PROJECT must be a project folder")?;
    let scratch = std::env::temp_dir().join(format!("smilebasic-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let outcome = copy_others(project, name, &scratch)
        .and_then(|()| copy_dir(project, &scratch.join(name)))
        .and_then(|()| bench(options, &scratch.join(name), program));
    let _ = std::fs::remove_dir_all(&scratch);
    outcome
}

/// How `--bench` runs a program.
struct BenchOptions<'a> {
    frames: u32,
    png: Option<&'a Path>,
    /// The input to give it.
    steps: Vec<(u32, Action)>,
    /// Where to write each frame's times.
    log: Option<PathBuf>,
    fixed: bool,
    /// Whether frames run as the window runs them, on a machine this
    /// many times slower.
    window: Option<f64>,
}

/// One frame's row in a `--bench --log` file.
struct FrameLog {
    run: std::time::Duration,
    draw: std::time::Duration,
    instructions: u64,
    vsyncs: u64,
    redrawn: bool,
    end: std::time::Duration,
}

/// The window's frame and limits (see the frontend's main loop), for
/// `--bench --window`.
const WINDOW_FRAME: std::time::Duration = std::time::Duration::from_nanos(1_000_000_000 / 60);
const WINDOW_RUN: std::time::Duration = std::time::Duration::from_millis(10);
const WINDOW_CATCH_UP: std::time::Duration = std::time::Duration::from_millis(1);
const WINDOW_MOST_SKIPPED: u32 = 3;
const WINDOW_MOST_BEHIND: std::time::Duration =
    std::time::Duration::from_nanos(8 * 1_000_000_000 / 60);

/// Frames between the screens a fixed run's checksum takes in: few, as
/// reading a whole screen would otherwise outweigh what is measured.
const CHECKSUM_FRAMES: usize = 60;

/// Adds pixels to an FNV-1a checksum, a word at a time.
fn add_to_checksum(sum: u64, pixels: &[u32]) -> u64 {
    pixels.iter().fold(sum, |sum, &p| {
        (sum ^ u64::from(p)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

fn bench(options: &BenchOptions, project: &Path, program: &str) -> Result<(), AnyError> {
    use std::time::{Duration, Instant};
    let &BenchOptions {
        frames,
        png,
        fixed,
        window,
        ..
    } = options;
    let name = project
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("PROJECT must be a project folder")?;
    let parent = project.parent().ok_or("PROJECT must be a project folder")?;
    let mut m = machine(parent.to_path_buf())?;
    if fixed {
        m.rt.fix_time_and_chance();
    }
    m.rt.storage
        .set_project(name)
        .map_err(|e| format!("{name}: {e}"))?;
    let mut checksum = 0xCBF2_9CE4_8422_2325u64;
    let started = Instant::now();
    m.run_file(program).map_err(|e| format!("{program}: {e}"))?;
    println!("loaded and compiled in {:?}", started.elapsed());
    let shots = png.and_then(Path::parent).unwrap_or(Path::new("."));
    let mut player = Player::new(options.steps.clone());
    let mut frame_log = Vec::new();
    let (mut run, mut draw) = (Vec::new(), Vec::new());
    let slower = window.unwrap_or(1.0);
    let (frame_time, run_time, catch_up_time, most_behind) = (
        WINDOW_FRAME.div_f64(slower),
        WINDOW_RUN.div_f64(slower),
        WINDOW_CATCH_UP.div_f64(slower),
        WINDOW_MOST_BEHIND.div_f64(slower),
    );
    let window = window.is_some();
    let mut clock = Instant::now();
    let mut played = 0;
    'run: while played < frames && m.is_running() {
        // As the window's main loop does: a frame, then those that are
        // due at once, of which only the last is drawn.
        let mut skipped = 0;
        loop {
            if player.step(&mut m, played, shots)? {
                break 'run;
            }
            if window {
                m.time_limit = Some(if skipped == 0 {
                    run_time
                } else {
                    catch_up_time
                });
            }
            let (before, vsyncs) = (m.executed(), m.vsyncs());
            let t = Instant::now();
            m.frame();
            let ran = t.elapsed();
            frame_log.push(FrameLog {
                run: ran,
                draw: Duration::ZERO,
                instructions: m.executed() - before,
                vsyncs: m.vsyncs() - vsyncs,
                redrawn: false,
                end: started.elapsed(),
            });
            run.push(ran);
            played += 1;
            let due = window && Instant::now() >= clock + frame_time;
            if !due || skipped == WINDOW_MOST_SKIPPED || played == frames || !m.is_running() {
                break;
            }
            clock += frame_time;
            skipped += 1;
        }
        let revision = m.screen_revision();
        let t2 = Instant::now();
        let screen = m.render();
        let drawn = t2.elapsed();
        if fixed && run.len().is_multiple_of(CHECKSUM_FRAMES) {
            checksum = add_to_checksum(checksum, screen.pixels());
        }
        draw.push(drawn);
        if let Some(last) = frame_log.last_mut() {
            last.draw = drawn;
            last.redrawn = m.screen_revision() != revision;
            last.end = started.elapsed();
        }
        if window {
            clock += frame_time;
            let now = Instant::now();
            if clock > now {
                std::thread::sleep(clock - now);
            } else if now - clock > most_behind {
                clock = now;
            }
        }
    }
    if let Some(file) = &options.log {
        let mut text = String::from("frame,run_us,draw_us,instructions,vsyncs,redrawn,end_us\n");
        for (i, f) in frame_log.iter().enumerate() {
            text += &format!(
                "{i},{},{},{},{},{},{}\n",
                f.run.as_micros(),
                f.draw.as_micros(),
                f.instructions,
                f.vsyncs,
                u8::from(f.redrawn),
                f.end.as_micros()
            );
        }
        std::fs::write(file, text).map_err(|e| format!("{}: {e}", file.display()))?;
    }
    let report = |what: &str, times: &mut Vec<Duration>| {
        if times.is_empty() {
            return;
        }
        let total: Duration = times.iter().sum();
        times.sort();
        println!(
            "{what}: mean {:?}, median {:?}, 99% {:?}, max {:?}",
            total / times.len() as u32,
            times[times.len() / 2],
            times[times.len() * 99 / 100],
            times[times.len() - 1],
        );
    };
    println!("{} frames", run.len());
    if fixed {
        // What the program did: runs that differ here cannot be compared.
        let rows = console_rows(&m).join("\n").into_bytes();
        let rows: Vec<u32> = rows.iter().map(|&b| u32::from(b)).collect();
        checksum = add_to_checksum(checksum, m.render().pixels());
        println!("checksum {:016x}", add_to_checksum(checksum, &rows));
    }
    // Stutter shows as a few slow frames, so name the slowest.
    let mut slowest: Vec<(usize, Duration)> = (frame_log.iter().map(|f| f.run + f.draw))
        .enumerate()
        .collect();
    slowest.sort_by_key(|&(_, t)| std::cmp::Reverse(t));
    let slowest: Vec<String> = (slowest.iter().take(8))
        .map(|(i, t)| format!("{i}: {t:.1?}"))
        .collect();
    println!("slowest frames: {}", slowest.join(", "));
    report("run ", &mut run);
    report("draw", &mut draw);
    if m.is_running() {
        println!("still running");
    }
    if std::env::var_os("BENCH_DEBUG").is_some() {
        let order = m.rt.sprites.draw_order();
        let area: f64 = order
            .iter()
            .map(|(i, p)| {
                let s = &m.rt.sprites.list[*i];
                f64::from(s.w * s.h) * (p.scale.0 * p.scale.1).abs()
            })
            .sum();
        println!("{} sprites shown, {area} pixels", order.len());
        // The large ones, which take the time.
        for (i, p) in &order {
            let s = &m.rt.sprites.list[*i];
            let a = f64::from(s.w * s.h) * (p.scale.0 * p.scale.1).abs();
            if a > 20000.0 {
                println!(
                    "sprite {i}: page {} {}x{} attr {:x} color {:08x} layer {} scale {:?} area {a}",
                    s.page, s.w, s.h, s.attr, p.color, p.layer, p.scale
                );
            }
        }
        for (i, t) in m.rt.text.iter().enumerate() {
            println!("text {i}: visible {} size {:?}", t.visible, t.size());
        }
        for l in 0..8 {
            println!("layer {l}: {:?} {:?}", m.rt.layers[l], m.rt.filters[l]);
        }
        println!("mode {:?}", m.rt.screen_mode());
    }
    print_rows(&console_rows(&m));
    if let Some(file) = png {
        save_screen(&mut m, file)?;
    }
    Ok(())
}

/// What a --play script does at a frame.
#[derive(Clone, Debug, PartialEq)]
enum Action {
    Key(u16),
    Type(String),
    Hold(Hold, u32),
    Shot(String),
    Console,
    End,
}

/// Input held for some frames.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hold {
    Buttons(u32),
    Stick(f64, f64),
    Usb(u8),
    Touch(i32, i32),
}

/// BUTTON's ids by the names on a controller.
const BUTTON_NAMES: [(&str, u32); 14] = [
    ("X", 0),
    ("B", 1),
    ("Y", 2),
    ("A", 3),
    ("UP", 4),
    ("DOWN", 5),
    ("LEFT", 6),
    ("RIGHT", 7),
    ("L", 8),
    ("R", 9),
    ("ZL", 10),
    ("ZR", 11),
    ("LSTICK", 12),
    ("RSTICK", 13),
];

/// A --play script's lines, in frame order.
fn parse_script(text: &str) -> Result<Vec<(u32, Action)>, AnyError> {
    let mut steps = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let bad = || format!("script line {}: {line}", n + 1);
        let mut words = line.split_whitespace();
        let frame: u32 = words.next().and_then(|w| w.parse().ok()).ok_or_else(bad)?;
        let what = words.next().ok_or_else(bad)?.to_ascii_lowercase();
        let args: Vec<&str> = words.collect();
        let int = |w: &str| -> Option<i64> {
            match w.strip_prefix("0x") {
                Some(hex) => i64::from_str_radix(hex, 16).ok(),
                None => w.parse().ok(),
            }
        };
        let action = match (what.as_str(), args.as_slice()) {
            ("key", [name]) => Action::Key(key_code(name).ok_or_else(bad)?),
            ("type", _) => {
                let rest = line.split_once(char::is_whitespace).map_or("", |r| r.1);
                let rest = rest.trim_start().split_once(char::is_whitespace);
                Action::Type(rest.map_or("", |r| r.1).to_owned())
            }
            ("button", [names, frames]) => {
                let mut bits = 0;
                for name in names.split('+') {
                    let name = name.to_ascii_uppercase();
                    let (_, id) = BUTTON_NAMES
                        .iter()
                        .find(|(n, _)| *n == name)
                        .ok_or_else(bad)?;
                    bits |= 1 << id;
                }
                Action::Hold(Hold::Buttons(bits), frames.parse().map_err(|_| bad())?)
            }
            ("stick", [x, y, frames]) => Action::Hold(
                Hold::Stick(x.parse()?, y.parse()?),
                frames.parse().map_err(|_| bad())?,
            ),
            ("usb", [id, frames]) => Action::Hold(
                Hold::Usb(int(id).and_then(|i| u8::try_from(i).ok()).ok_or_else(bad)?),
                frames.parse().map_err(|_| bad())?,
            ),
            ("touch", [x, y, frames]) => Action::Hold(
                Hold::Touch(x.parse()?, y.parse()?),
                frames.parse().map_err(|_| bad())?,
            ),
            ("shot", [name]) => Action::Shot((*name).to_owned()),
            ("console", []) => Action::Console,
            ("end", []) => Action::End,
            _ => return Err(bad().into()),
        };
        steps.push((frame, action));
    }
    steps.sort_by_key(|(frame, _)| *frame);
    Ok(steps)
}

fn key_code(name: &str) -> Option<u16> {
    let code = match name.to_ascii_uppercase().as_str() {
        "ENTER" => keys::ENTER,
        "UP" => keys::UP,
        "DOWN" => keys::DOWN,
        "LEFT" => keys::LEFT,
        "RIGHT" => keys::RIGHT,
        "ESC" => keys::ESCAPE,
        "TAB" => keys::TAB,
        "BACKSPACE" => keys::BACKSPACE,
        "DELETE" => keys::DELETE,
        "HOME" => keys::HOME,
        "END" => keys::END,
        "INSERT" => keys::INSERT,
        "SELECTLEFT" => sb_runtime::line::commands::SELECT_LEFT,
        "SELECTRIGHT" => sb_runtime::line::commands::SELECT_RIGHT,
        "SPACE" => u16::from(b' '),
        _ => {
            let mut chars = name.chars();
            let c = chars.next()?;
            return chars
                .next()
                .is_none()
                .then(|| u16::try_from(u32::from(c)).ok())
                .flatten();
        }
    };
    Some(code)
}

/// The script a file holds, or none.
fn read_script(file: Option<&str>) -> Result<Vec<(u32, Action)>, AnyError> {
    match file {
        Some(file) => {
            parse_script(&std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?)
        }
        None => Ok(Vec::new()),
    }
}

/// Gives a program what a script asks for.
struct Player {
    steps: std::iter::Peekable<std::vec::IntoIter<(u32, Action)>>,
    /// Input held, and the frame it lets go at.
    held: Vec<(Hold, u32)>,
}

impl Player {
    fn new(steps: Vec<(u32, Action)>) -> Player {
        Player {
            steps: steps.into_iter().peekable(),
            held: Vec::new(),
        }
    }

    fn has_more(&mut self) -> bool {
        self.steps.peek().is_some()
    }

    /// Does what is due before `frame` runs, and sets the input it reads;
    /// shots go into `dir`. True if the script ends the run.
    fn step(&mut self, m: &mut Machine, frame: u32, dir: &Path) -> Result<bool, AnyError> {
        while let Some((_, action)) = self.steps.next_if(|(at, _)| *at <= frame) {
            match action {
                Action::Key(code) => m.rt.key_press(code),
                Action::Type(text) => m.type_text(&text),
                Action::Hold(hold, n) => self.held.push((hold, frame + n)),
                Action::Shot(shot) => {
                    let file = dir.join(format!("{shot}.png"));
                    save_screen(m, &file)?;
                    println!("frame {frame}: screen saved to {}", file.display());
                }
                Action::Console => {
                    println!("frame {frame}: the console:");
                    print_rows(&console_rows(m));
                    for notice in m.rt.take_notices() {
                        println!("notice: {notice}");
                    }
                }
                Action::End => return Ok(true),
            }
        }
        self.held.retain(|&(_, until)| until > frame);
        let mut state = State::default();
        // The pointer stays where the last touch or the program's MOUSE
        // put it.
        let last = m.rt.input.live.mouse;
        (state.mouse.x, state.mouse.y) = (last.x, last.y);
        (state.mouse.display, state.mouse.on_screen) = (last.display, last.on_screen);
        state.mouse.wheel = last.wheel;
        for &(hold, _) in &self.held {
            match hold {
                Hold::Buttons(bits) => state.pads[0].buttons |= bits,
                Hold::Stick(x, y) => state.pads[0].sticks[0] = (x, y),
                Hold::Usb(usage) => {
                    state.set_key(usage, true);
                    state.modifiers |= modifier(usage);
                }
                Hold::Touch(x, y) => {
                    set_pointer(&mut state, m, x, y);
                    state.mouse.buttons |= 1;
                }
            }
        }
        m.rt.input.live = state;
        Ok(false)
    }
}

/// Runs a program with scripted input, saving the screens it asks for.
fn play(
    frames: u32,
    project: &Path,
    program: &str,
    script: &str,
    dir: &Path,
) -> Result<(), AnyError> {
    let steps = read_script((script != "-").then_some(script))?;
    std::fs::create_dir_all(dir)?;
    let name = project
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("PROJECT must be a project folder")?;
    let parent = project.parent().ok_or("PROJECT must be a project folder")?;
    let mut m = machine(parent.to_path_buf())?;
    // The script's text is typed as written, not with its case swapped
    // as keys are when letters type upper case.
    m.rt.settings.upper_case_letters = false;
    m.rt.storage
        .set_project(name)
        .map_err(|e| format!("{name}: {e}"))?;
    m.run_file(program).map_err(|e| format!("{program}: {e}"))?;
    let mut player = Player::new(steps);
    let mut frame = 0;
    // Direct mode goes on while the script has more to type, as for
    // SUBRUN, and while a subprogram runs.
    while frame < frames && (m.is_running() || m.sub_running() || player.has_more()) {
        if player.step(&mut m, frame, dir)? {
            break;
        }
        m.frame();
        frame += 1;
    }
    if m.is_running() {
        println!("still running at frame {frame}");
    } else {
        println!("ended at frame {frame}");
    }
    print_rows(&console_rows(&m));
    let file = dir.join("end.png");
    save_screen(&mut m, &file)?;
    println!("screen saved to {}", file.display());
    Ok(())
}

/// KEYBOARD's bit for a modifier key's USB usage: Windows key, Shift,
/// Ctrl and Alt, left (0xE0-0xE3) or right (0xE4-0xE7).
fn modifier(usage: u8) -> u32 {
    match usage {
        0xE0 | 0xE4 => 1 << 2,
        0xE1 | 0xE5 => 1 << 1,
        0xE2 | 0xE6 => 1 << 3,
        0xE3 | 0xE7 => 1,
        _ => 0,
    }
}

/// Puts the mouse pointer at a screen pixel, and where that is on the
/// 1280x720 display, taking the screen as stretched over all of it.
fn set_pointer(state: &mut State, m: &Machine, x: i32, y: i32) {
    let mode = m.screen_mode();
    let scale = |v: i32, from: u32, to: i32| match i32::try_from(from) {
        Ok(from) if from > 0 => v * to / from,
        _ => v,
    };
    let mouse = &mut state.mouse;
    (mouse.x, mouse.y) = (x, y);
    mouse.display = (scale(x, mode.width, 1280), scale(y, mode.height, 720));
    mouse.on_screen = true;
}

fn machine(storage: PathBuf) -> Result<Machine, AnyError> {
    let screen = Profile::default().screen();
    let resources = resources::data_dir()?.join(resources::RESOURCE_DIR);
    // SmileBASIC 4's own font when it was imported, so screens match.
    let font = resources::load_font(&resources).unwrap_or_else(|_| Font::blank());
    let mut rt = Runtime::new(screen.width, screen.height, font);
    let (sprite_page, sprite_defs) = resources::load_sprites(&resources);
    if let Some(page) = sprite_page {
        rt.set_sprite_page(page);
    }
    if let Some(defs) = sprite_defs {
        rt.sprites = Sprites::new(defs);
    }
    rt.sound
        .set_sequences(resources::load_sequences(&resources).map(std::sync::Arc::new));
    // SmileBASIC 4's instruments, so that RECSTART records the songs.
    let bank = resources::load_instruments(&resources).map(std::sync::Arc::new);
    rt.sound.set_instruments(bank.map(Instruments::Bank));
    rt.set_startup_mode(ScreenMode {
        sample: screen.sample,
        ..ScreenMode::new(screen.width, screen.height)
    });
    rt.storage = Storage::new(storage);
    rt.settings.file_confirm = FileConfirm::Silent;
    let mut m = Machine::new(rt);
    m.boot();
    Ok(m)
}

/// The screen as a PNG, as large as XSCREEN made it.
/// With a subprogram's screen or the border marking the focus shown, the
/// whole 1280x720 display, the screen stretched to it and the
/// subprogram's put where XSUBSCREEN puts it.
fn save_screen(m: &mut Machine, file: &Path) -> Result<(), AnyError> {
    let fb = m.render();
    let mut image = image::Image {
        width: fb.width() as usize,
        height: fb.height() as usize,
        pixels: fb.pixels().to_vec(),
    };
    let border = m.focus_border();
    if let Some((sub, rect)) = m.sub_screen() {
        image = on_display(&image);
        put_sub_screen(&mut image, sub, rect);
    }
    if let Some((rect, alpha)) = border {
        if image.width != DISPLAY.0 {
            image = on_display(&image);
        }
        draw_focus_border(&mut image, rect, alpha);
    }
    std::fs::write(file, image::encode_png(&image)?)?;
    Ok(())
}

/// The border marking where the focus has just gone, as the frontend
/// draws it: yellow one pixel out and two in, then two dark pixels.
fn draw_focus_border(image: &mut image::Image, [x, y, w, h]: [i32; 4], alpha: u8) {
    let a = u32::from(alpha);
    for (inset, width, color) in [(-1, 3, 0x00FF_FF00u32), (2, 2, 0)] {
        let (x0, y0, x1, y1) = (x + inset, y + inset, x + w - inset, y + h - inset);
        for py in y0.max(0)..y1.min(image.height as i32) {
            for px in x0.max(0)..x1.min(image.width as i32) {
                let edge =
                    px < x0 + width || px >= x1 - width || py < y0 + width || py >= y1 - width;
                if !edge {
                    continue;
                }
                let d = &mut image.pixels[py as usize * image.width + px as usize];
                let mix = |shift: u32| {
                    let (c, under) = (color >> shift & 0xFF, *d >> shift & 0xFF);
                    ((c * a + under * (255 - a)) / 255) << shift
                };
                *d = 0xFF00_0000 | mix(16) | mix(8) | mix(0);
            }
        }
    }
}

/// SmileBASIC 4's display, which XSUBSCREEN and the focus's border are
/// placed on.
const DISPLAY: (usize, usize) = (1280, 720);

/// The screen stretched to the whole display.
fn on_display(main: &image::Image) -> image::Image {
    let (dw, dh) = DISPLAY;
    let mut pixels = Vec::with_capacity(dw * dh);
    for py in 0..dh {
        let sy = py * main.height / dh;
        pixels.extend((0..dw).map(|px| main.pixels[sy * main.width + px * main.width / dw]));
    }
    image::Image {
        width: dw,
        height: dh,
        pixels,
    }
}

/// Puts the subprogram's premultiplied screen over the display where
/// XSUBSCREEN puts it.
fn put_sub_screen(display: &mut image::Image, sub: &Framebuffer, [x, y, w, h]: [i32; 4]) {
    let (dw, dh) = DISPLAY;
    let pixels = &mut display.pixels;
    let (sw, sh) = (sub.width() as i64, sub.height() as i64);
    for py in y.max(0)..(y + h).min(dh as i32) {
        for px in x.max(0)..(x + w).min(dw as i32) {
            let sx = i64::from(px - x) * sw / i64::from(w);
            let sy = i64::from(py - y) * sh / i64::from(h);
            let s = sub.pixels()[(sy * sw + sx) as usize];
            let d = &mut pixels[py as usize * dw + px as usize];
            // Premultiplied over, each channel rounded to the nearest and
            // held to 255 (a clear BACKCOLOR's color can be brighter than
            // its alpha), as a GPU blends.
            let a = 255 - (s >> 24);
            let channel = |shift: u32| {
                let under = ((*d >> shift & 0xFF) * a + 127) / 255;
                ((s >> shift & 0xFF) + under).min(255) << shift
            };
            *d = channel(0) | channel(8) | channel(16) | channel(24);
        }
    }
}

/// Copies the projects beside PROJECT into `scratch` once, for parts that
/// read other projects or the workspace (SmileBASIC 4's `###` folder).
/// What a part changes in them stays for the next part. An emulator's
/// own workspace folder there gives way to `###`.
fn copy_others(project: &Path, name: &str, scratch: &Path) -> Result<(), AnyError> {
    std::fs::create_dir_all(scratch)?;
    let Some(parent) = project.parent().filter(|p| !p.as_os_str().is_empty()) else {
        return Ok(());
    };
    let sb4_workspace = parent.join("###").is_dir();
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        let file = entry.file_name();
        let other = file.to_string_lossy();
        if !entry.file_type()?.is_dir() || other == name || (sb4_workspace && other == "@WORKSPACE")
        {
            continue;
        }
        let to = match other.as_ref() {
            "###" => "@WORKSPACE",
            other => other,
        };
        copy_dir(&entry.path(), &scratch.join(to))?;
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), AnyError> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_dir(&entry.path(), &to.join(entry.file_name()))?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn is_result(file: &str) -> bool {
    file.starts_with(RESULT_PREFIX)
}

/// The result files in a project folder, as screen rows.
fn results(dir: &Path) -> Result<BTreeMap<String, Vec<String>>, AnyError> {
    let mut found = BTreeMap::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file = entry.file_name().to_string_lossy().into_owned();
        if is_result(&file) {
            found.insert(file, screen_rows(&read_text(&entry.path())?));
        }
    }
    Ok(found)
}

/// A TXT file's text, from SmileBASIC 4's format or plain UTF-8.
fn read_text(path: &Path) -> Result<String, AnyError> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = if bytes.starts_with(&[4, 0]) {
        sbfile::parse(&bytes)?.text()?.to_owned()
    } else {
        String::from_utf8(bytes).map_err(|_| format!("{}: not UTF-8", path.display()))?
    };
    Ok(text)
}

/// Rows without the spaces that end them or the empty rows at the end, so
/// cells never written (code 0) and spaces compare equal.
fn screen_rows(text: &str) -> Vec<String> {
    let mut rows: Vec<String> = text
        .split('\n')
        .map(|row| row.replace('\0', " ").trim_end().to_owned())
        .collect();
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

fn console_rows(m: &Machine) -> Vec<String> {
    let console = &m.rt.text[CONSOLE];
    let (w, h) = console.size();
    let text: Vec<String> = (0..h)
        .map(|y| {
            let codes: Vec<u16> = (0..w).map(|x| console.cell(x, y).code).collect();
            String::from_utf16_lossy(&codes)
        })
        .collect();
    screen_rows(&text.join("\n"))
}

fn print_rows(rows: &[String]) {
    for row in rows {
        println!("  |{row}");
    }
}

fn compare(file: &str, theirs: &[String], ours: &[String]) {
    let rows = theirs.len().max(ours.len());
    let row = |rows: &[String], y: usize| rows.get(y).cloned().unwrap_or_default();
    let differing: Vec<usize> = (0..rows)
        .filter(|&y| row(theirs, y) != row(ours, y))
        .collect();
    if differing.is_empty() {
        println!("{file}: same");
        return;
    }
    println!("{file}: {} of {rows} rows differ", differing.len());
    for y in differing {
        println!("  row {y:2} SB4 |{}", row(theirs, y));
        println!("         ours |{}", row(ours, y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_lists() {
        assert_eq!(parse_parts("3").unwrap(), [Some(3)]);
        assert_eq!(
            parse_parts("1-3,7").unwrap(),
            [Some(1), Some(2), Some(3), Some(7)]
        );
        assert!(parse_parts("1-x").is_err());
        assert!(parse_parts("").is_err());
    }

    #[test]
    fn scripts() {
        let steps = parse_script(
            "# menu\n30 button A+down 6\n10 key enter\n20 type LOAD \"X\"\n40 usb 0x28 2\n50 end\n",
        )
        .unwrap();
        assert_eq!(
            steps,
            [
                (10, Action::Key(keys::ENTER)),
                (20, Action::Type("LOAD \"X\"".into())),
                (30, Action::Hold(Hold::Buttons(1 << 3 | 1 << 5), 6)),
                (40, Action::Hold(Hold::Usb(0x28), 2)),
                (50, Action::End),
            ]
        );
        assert!(parse_script("5 button C 1").is_err());
        assert!(parse_script("x key A").is_err());
    }

    #[test]
    fn rows_ignore_trailing_blanks() {
        assert_eq!(screen_rows("A  \n\0B\0\n\n  \n"), ["A", " B"]);
        assert!(screen_rows("").is_empty());
    }
}

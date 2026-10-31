//! `smilebasic-check`: runs a hardware check program on the emulator and
//! compares the screens it saved with the ones SmileBASIC 4 saved.
//!
//! Check programs end each part by saving the console's text with SAVEV
//! into a TXT file whose name starts with `RES` (see
//! docs/hardware-tests.md). Run on SmileBASIC 4, in Ryujinx, those files
//! land in the project's folder; this runs the same parts here, on a copy
//! of that folder, and compares them row by row.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sb_profiles::Profile;
use sb_runtime::confirm::FileConfirm;
use sb_runtime::font::Font;
use sb_runtime::input::State;
use sb_runtime::machine::Machine;
use sb_runtime::runtime::keys;
use sb_runtime::screen::ScreenMode;
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
      ours are printed.
      With --screen, each part's screen is also saved into DIR as a PNG
      once it has run for two seconds, and then Enter is pressed, for
      parts that show a picture and wait for a key to end.
  smilebasic-check --bench FRAMES PROJECT PROGRAM [PNG]
      run PROGRAM for FRAMES frames, rendering each one, and print how
      long running and rendering took, and the console at the end; with
      PNG, also save the last screen there.
  smilebasic-check --play FRAMES PROJECT PROGRAM SCRIPT DIR
      run PROGRAM for at most FRAMES frames, giving it the input SCRIPT
      (a file, or - for none) asks for, and save the screens it asks for
      into DIR, the last one as end.png. Like --bench, the program runs
      in PROJECT itself, so give it a copy if it may save files. Each
      SCRIPT line is a frame number and what happens then:
        F key NAME        a key typed for INKEY$ and INPUT: ENTER, UP,
                          DOWN, LEFT, RIGHT, ESC, TAB, BACKSPACE,
                          DELETE, HOME, END, SPACE or one character
        F type TEXT       the rest of the line typed
        F button NAMES N  buttons held for N frames, NAMES such as A or
                          A+UP: A B X Y UP DOWN LEFT RIGHT L R ZL ZR
                          LSTICK RSTICK
        F stick X Y N     the left stick tilted (-1 to 1) for N frames
        F usb ID N        the keyboard key with USB usage ID (such as
                          0x28 for Enter) held for N frames, for KEYBOARD
        F touch X Y N     the screen touched at X,Y for N frames
        F shot NAME       the screen saved as DIR/NAME.png
        F console         the console's text printed
        F end             the run stops
      Lines starting with # are comments.";

/// Result files start with this, after the TXT type letter.
const RESULT_PREFIX: &str = "TRES";
/// How long a part may run, in frames: five minutes at 60 a second.
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
        return match args.as_slice() {
            [_, frames, project, program, png @ ..] if png.len() <= 1 => bench(
                frames.parse().map_err(|_| USAGE)?,
                Path::new(project),
                program,
                png.first().map(Path::new),
            ),
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
    let outcome = parts.iter().try_for_each(|&part| {
        check_part(project, name, program, part, &scratch, screens.as_deref())
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
    let _ = std::fs::remove_dir_all(scratch);
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
    while m.is_running() && frames < FRAME_LIMIT {
        if m.rt.wants_line()
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
fn bench(frames: u32, project: &Path, program: &str, png: Option<&Path>) -> Result<(), AnyError> {
    use std::time::{Duration, Instant};
    let name = project
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("PROJECT must be a project folder")?;
    let parent = project.parent().ok_or("PROJECT must be a project folder")?;
    let mut m = machine(parent.to_path_buf())?;
    m.rt.storage
        .set_project(name)
        .map_err(|e| format!("{name}: {e}"))?;
    let started = Instant::now();
    m.run_file(program).map_err(|e| format!("{program}: {e}"))?;
    println!("loaded and compiled in {:?}", started.elapsed());
    let (mut run, mut draw) = (Vec::new(), Vec::new());
    for _ in 0..frames {
        if !m.is_running() {
            break;
        }
        let t = Instant::now();
        m.frame();
        let t2 = Instant::now();
        m.render();
        run.push(t2 - t);
        draw.push(t2.elapsed());
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
#[derive(Debug, PartialEq)]
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

/// Runs a program with scripted input, saving the screens it asks for.
fn play(
    frames: u32,
    project: &Path,
    program: &str,
    script: &str,
    dir: &Path,
) -> Result<(), AnyError> {
    let steps = match script {
        "-" => Vec::new(),
        file => parse_script(&std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?)?,
    };
    std::fs::create_dir_all(dir)?;
    let name = project
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("PROJECT must be a project folder")?;
    let parent = project.parent().ok_or("PROJECT must be a project folder")?;
    let mut m = machine(parent.to_path_buf())?;
    m.rt.storage
        .set_project(name)
        .map_err(|e| format!("{name}: {e}"))?;
    m.run_file(program).map_err(|e| format!("{program}: {e}"))?;
    let mut steps = steps.into_iter().peekable();
    // Input held, and the frame it lets go at.
    let mut held: Vec<(Hold, u32)> = Vec::new();
    let mut frame = 0;
    'run: while frame < frames && m.is_running() {
        while let Some((_, action)) = steps.next_if(|(at, _)| *at <= frame) {
            match action {
                Action::Key(code) => m.rt.key_press(code),
                Action::Type(text) => m.type_text(&text),
                Action::Hold(hold, n) => held.push((hold, frame + n)),
                Action::Shot(shot) => {
                    let file = dir.join(format!("{shot}.png"));
                    save_screen(&mut m, &file)?;
                    println!("frame {frame}: screen saved to {}", file.display());
                }
                Action::Console => {
                    println!("frame {frame}: the console:");
                    print_rows(&console_rows(&m));
                }
                Action::End => break 'run,
            }
        }
        held.retain(|&(_, until)| until > frame);
        let mut state = State::default();
        for &(hold, _) in &held {
            match hold {
                Hold::Buttons(bits) => state.pads[0].buttons |= bits,
                Hold::Stick(x, y) => state.pads[0].sticks[0] = (x, y),
                Hold::Usb(usage) => state.set_key(usage, true),
                Hold::Touch(x, y) => {
                    state.mouse.x = x;
                    state.mouse.y = y;
                    state.mouse.on_screen = true;
                    state.mouse.buttons |= 1;
                }
            }
        }
        m.rt.input.live = state;
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
fn save_screen(m: &mut Machine, file: &Path) -> Result<(), AnyError> {
    let fb = m.render();
    let image = image::Image {
        width: fb.width() as usize,
        height: fb.height() as usize,
        pixels: fb.pixels().to_vec(),
    };
    std::fs::write(file, image::encode_png(&image))?;
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

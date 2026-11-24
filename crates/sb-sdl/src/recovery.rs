//! Recovery from a renderer failure with nothing lost. Programs and pages are kept
//! on the CPU, so a new renderer only has to be given them again. If the emulator
//! has to end because of a failure, the unsaved programs are saved to files first.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use sb_runtime::machine::Machine;
use sb_runtime::runtime::SLOTS;
use sb_runtime::storage::{FileKind, Storage};
use sdl3::render::{Canvas, TextureCreator};
use sdl3::video::{Window, WindowContext};

/// Failures in a row after which a driver is skipped, and after which the emulator
/// ends instead of trying again: enough to try every driver.
const AVOID_AFTER: u32 = 2;
const MOST_FAILURES: u32 = 8;
/// Frames shown in a row that make the drivers trusted again.
const GOOD_FRAMES: u32 = 120;
/// In the data directory: the message for the next start.
const NOTICE_FILE: &str = "recovery.txt";

/// The time between tries to make a renderer, while there is none to draw on.
pub const RETRY: Duration = Duration::from_secs(1);
/// With no renderer for this long, the user is told.
pub const TELL_AFTER: Duration = Duration::from_secs(5);
/// The lead over the monotonic clock that the wall clock needs to count as a suspend.
const SLEPT: Duration = Duration::from_secs(1);

/// Whether the computer was suspended, because a GL context may have silently lost
/// its textures then. On Linux and macOS the wall clock continues through a suspend
/// and `Instant` does not, so the difference between them shows a suspend. A main loop
/// that is only blocked (dragging a window blocks it for seconds) moves both equally.
///
/// On Windows `Instant` also continues through a suspend, so this never fires there:
/// Direct3D itself reports what it lost, and a GL context there keeps its textures.
/// A wall clock set forward looks the same as a suspend, which costs a tenth of a
/// second.
pub struct SuspendWatch {
    wall: SystemTime,
    monotonic: Instant,
}

impl SuspendWatch {
    pub fn new() -> SuspendWatch {
        SuspendWatch {
            wall: SystemTime::now(),
            monotonic: Instant::now(),
        }
    }

    /// Whether the computer was suspended since the last time this was asked.
    pub fn woke(&mut self) -> bool {
        self.woke_at(SystemTime::now(), Instant::now())
    }

    fn woke_at(&mut self, wall: SystemTime, monotonic: Instant) -> bool {
        let wall_gap = wall.duration_since(self.wall);
        let monotonic_gap = monotonic.duration_since(self.monotonic);
        (self.wall, self.monotonic) = (wall, monotonic);
        wall_gap.is_ok_and(|gap| gap.saturating_sub(monotonic_gap) >= SLEPT)
    }

    /// Simulates a suspend for the bench, as if the wall clock had moved on by `time`
    /// while the monotonic one stood still.
    pub fn pretend(&mut self, time: Duration) {
        self.wall -= time;
    }
}

/// A renderer's texture creator, with the lifetime the textures then have, so that
/// the renderer can be replaced while the variables holding textures stay.
///
/// The rule in place of the borrow checker: all the textures made through it are
/// dropped before it is. The frontend's variables are declared after it, for the
/// usual drop order, and emptied by hand before a renderer is replaced.
pub struct OwnedCreator(*mut TextureCreator<WindowContext>);

impl OwnedCreator {
    pub fn new(canvas: &Canvas<Window>) -> OwnedCreator {
        OwnedCreator(Box::into_raw(Box::new(canvas.texture_creator())))
    }

    pub fn get(&self) -> &'static TextureCreator<WindowContext> {
        // SAFETY: the box lives until `drop`, and by the rule above nothing made from
        // this reference outlives that.
        unsafe { &*self.0 }
    }
}

impl Drop for OwnedCreator {
    fn drop(&mut self) {
        // SAFETY: from Box::into_raw in `new`, and freed only here.
        drop(unsafe { Box::from_raw(self.0) });
    }
}

/// Whether the renderers can still be trusted, and which drivers cannot.
#[derive(Debug, Default)]
pub struct RendererWatch {
    failures: u32,
    good_frames: u32,
    avoid: Vec<String>,
}

impl RendererWatch {
    /// Notes that a renderer on `driver` failed. Returns false if no tries are left.
    pub fn failed(&mut self, driver: &str) -> bool {
        self.failures += 1;
        self.good_frames = 0;
        if self.failures >= AVOID_AFTER && !self.avoid.iter().any(|d| d == driver) {
            self.avoid.push(driver.to_string());
        }
        self.failures < MOST_FAILURES
    }

    /// Notes a frame shown in the window with no failure.
    pub fn frame_shown(&mut self) {
        if self.failures > 0 {
            self.good_frames += 1;
            if self.good_frames >= GOOD_FRAMES {
                *self = RendererWatch::default();
            }
        }
    }

    /// Drivers not to try again for now.
    pub fn avoid(&self) -> &[String] {
        &self.avoid
    }
}

/// The state that saves a failing emulator's unsaved programs to files.
pub struct Recovery {
    /// The data directory, for programs that a project cannot take and for the
    /// notice.
    data: PathBuf,
    /// The number in the file names of this session, so that files of an earlier
    /// failure that nobody has looked at are kept.
    session: Option<u32>,
}

impl Recovery {
    pub fn new(data: PathBuf) -> Recovery {
        Recovery {
            data,
            session: None,
        }
    }

    fn file_name(session: u32, slot: usize) -> String {
        format!("@RECOVER{session}_{slot}.PRG")
    }

    fn pick_session(storage: &Storage) -> u32 {
        let taken = |session| {
            (0..SLOTS)
                .any(|slot| storage.exists(FileKind::Txt, &Recovery::file_name(session, slot)))
        };
        (1..).find(|&session| !taken(session)).unwrap_or(1)
    }

    /// Saves every slot with unsaved edits beside the files of the current project,
    /// or in the data directory if the project takes none. Returns the message for
    /// the user as lines, or `None` if nothing was unsaved. The next start shows it
    /// too.
    pub fn save(&mut self, machine: &Machine) -> Option<Vec<String>> {
        let programs = machine.unsaved_programs();
        if programs.is_empty() {
            return None;
        }
        let storage = &machine.rt.storage;
        let session = *self
            .session
            .get_or_insert_with(|| Recovery::pick_session(storage));
        let project = storage.project();
        let (mut names, mut apart, mut lost) = (Vec::new(), Vec::new(), Vec::new());
        for (slot, text) in programs {
            let name = Recovery::file_name(session, slot);
            let bytes = sb_core::wtf8::encode(&text);
            if storage.write(FileKind::Txt, &name, &bytes, None).is_ok() {
                names.push(name);
                continue;
            }
            let path = self.data.join(format!("recover{session}_prg{slot}.txt"));
            match std::fs::write(&path, &bytes) {
                Ok(()) => apart.push(path),
                Err(e) => {
                    eprintln!("could not save PRG{slot} to {}: {e}", path.display());
                    lost.push(format!("PRG{slot}"));
                }
            }
        }
        let mut lines = Vec::new();
        if !names.is_empty() {
            let shown = if project.is_empty() {
                "(default)"
            } else {
                project
            };
            lines.push(format!("Unsaved programs were saved in project {shown}:"));
            lines.push(names.join(", "));
        }
        if !apart.is_empty() {
            lines.push("Unsaved programs were saved as:".to_string());
            lines.extend(apart.iter().map(|path| path.display().to_string()));
        }
        if !lost.is_empty() {
            lines.push(format!("Could not save: {}", lost.join(", ")));
        }
        write_notice(&self.data, &lines);
        Some(lines)
    }

    /// Returns the message that a failure left for this start. It is returned once.
    pub fn take_notice(&self) -> Option<Vec<String>> {
        let path = self.data.join(NOTICE_FILE);
        let text = std::fs::read_to_string(&path).ok()?;
        if let Err(e) = std::fs::remove_file(&path) {
            eprintln!("could not remove {}: {e}", path.display());
        }
        Some(text.lines().map(str::to_string).collect())
    }
}

fn write_notice(data: &Path, lines: &[String]) {
    let path = data.join(NOTICE_FILE);
    if let Err(e) = std::fs::write(&path, lines.join("\n") + "\n") {
        eprintln!("could not write {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns a watch and the readings of its clocks, which the tests move by hand.
    fn watch() -> (SuspendWatch, SystemTime, Instant) {
        let (wall, monotonic) = (SystemTime::now(), Instant::now());
        (SuspendWatch { wall, monotonic }, wall, monotonic)
    }

    #[test]
    fn a_held_up_loop_is_not_a_suspend() {
        let (mut watch, wall, monotonic) = watch();
        let held = Duration::from_secs(30);
        assert!(!watch.woke_at(wall + held, monotonic + held));
    }

    #[test]
    fn a_suspend_moves_the_wall_clock_alone() {
        let (mut watch, wall, monotonic) = watch();
        let (slept, ran) = (Duration::from_secs(60), Duration::from_millis(16));
        assert!(watch.woke_at(wall + slept + ran, monotonic + ran));
        // Reported once.
        let wall = wall + slept + ran + ran;
        assert!(!watch.woke_at(wall, monotonic + ran + ran));
    }

    #[test]
    fn a_short_wobble_or_the_wall_clock_going_back_is_not_a_suspend() {
        let (mut watch, wall, monotonic) = watch();
        assert!(!watch.woke_at(wall + Duration::from_millis(200), monotonic));
        let mut watch = SuspendWatch { wall, monotonic };
        assert!(!watch.woke_at(wall - Duration::from_secs(60), monotonic));
    }

    #[test]
    fn a_pretended_suspend_counts_once() {
        let (mut watch, wall, monotonic) = watch();
        watch.pretend(Duration::from_secs(4));
        assert!(watch.woke_at(wall, monotonic));
        assert!(!watch.woke_at(wall, monotonic));
    }
    use sb_runtime::Runtime;
    use sb_runtime::font::Font;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let pid = std::process::id();
            let dir = std::env::temp_dir().join(format!("sb-recovery-{name}-{pid}"));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn machine_with_changes(root: &Path, slots: &[(usize, &str)]) -> Machine {
        let mut rt = Runtime::for_tests(Font::blank());
        rt.storage = Storage::new(root.join("projects"));
        let mut m = Machine::new(rt);
        m.boot();
        for &(slot, text) in slots {
            m.set_slot(slot, text.to_string());
            m.rt.slot_modified[slot] = true;
        }
        m
    }

    #[test]
    fn a_driver_is_trusted_for_one_failure_and_then_passed_over() {
        let mut watch = RendererWatch::default();
        assert!(watch.failed("opengl"));
        assert!(watch.avoid().is_empty());
        assert!(watch.failed("opengl"));
        assert_eq!(watch.avoid(), ["opengl"]);
        assert!(watch.failed("vulkan"));
        assert_eq!(watch.avoid(), ["opengl", "vulkan"]);
    }

    #[test]
    fn failures_with_no_good_frames_end_the_trying() {
        let mut watch = RendererWatch::default();
        let tries = (0..100).take_while(|_| watch.failed("software")).count();
        assert_eq!(tries as u32, MOST_FAILURES - 1);
    }

    #[test]
    fn enough_good_frames_trust_the_drivers_again() {
        let mut watch = RendererWatch::default();
        watch.failed("opengl");
        watch.failed("opengl");
        for _ in 0..GOOD_FRAMES - 1 {
            watch.frame_shown();
        }
        assert_eq!(watch.avoid(), ["opengl"]);
        watch.frame_shown();
        assert!(watch.avoid().is_empty());
        // ...and the count starts again.
        assert!(watch.failed("opengl"));
        assert!(watch.avoid().is_empty());
    }

    #[test]
    fn nothing_unsaved_leaves_nothing_behind() {
        let dir = TempDir::new("none");
        let m = machine_with_changes(&dir.0, &[]);
        let mut recovery = Recovery::new(dir.0.clone());
        assert_eq!(recovery.save(&m), None);
        assert_eq!(recovery.take_notice(), None);
    }

    #[test]
    fn unsaved_slots_go_to_the_project_and_the_next_start_is_told_once() {
        let dir = TempDir::new("slots");
        let m = machine_with_changes(&dir.0, &[(0, "?\"A\"\n"), (2, "?\"C\"\n")]);
        let mut recovery = Recovery::new(dir.0.clone());
        let lines = recovery.save(&m).expect("something to save");
        assert!(lines[1].contains("@RECOVER1_0.PRG"), "{lines:?}");
        assert!(lines[1].contains("@RECOVER1_2.PRG"), "{lines:?}");
        let storage = &m.rt.storage;
        let read = |name| storage.read(FileKind::Txt, name).unwrap();
        assert_eq!(read("@RECOVER1_0.PRG"), b"?\"A\"\n");
        assert_eq!(read("@RECOVER1_2.PRG"), b"?\"C\"\n");
        assert_eq!(recovery.take_notice(), Some(lines));
        assert_eq!(recovery.take_notice(), None);
    }

    #[test]
    fn what_is_in_the_editor_is_what_is_saved() {
        let dir = TempDir::new("editor");
        let mut m = machine_with_changes(&dir.0, &[]);
        m.toggle_editor();
        m.type_text("12345");
        m.frame();
        let mut recovery = Recovery::new(dir.0.clone());
        recovery.save(&m).expect("something to save");
        let saved = m.rt.storage.read(FileKind::Txt, "@RECOVER1_0.PRG").unwrap();
        assert!(saved.windows(5).any(|w| w == b"12345"), "{saved:?}");
    }

    #[test]
    fn a_later_failure_keeps_what_an_earlier_one_saved() {
        let dir = TempDir::new("later");
        let m = machine_with_changes(&dir.0, &[(0, "FIRST\n")]);
        Recovery::new(dir.0.clone()).save(&m);
        let m = machine_with_changes(&dir.0, &[(0, "SECOND\n")]);
        let mut recovery = Recovery::new(dir.0.clone());
        recovery.save(&m);
        // Saved again in one session, for example when the failure continues: the file
        // is the same.
        let m = machine_with_changes(&dir.0, &[(0, "THIRD\n")]);
        recovery.save(&m);
        let storage = &m.rt.storage;
        let read = |name| storage.read(FileKind::Txt, name).unwrap();
        assert_eq!(read("@RECOVER1_0.PRG"), b"FIRST\n");
        assert_eq!(read("@RECOVER2_0.PRG"), b"THIRD\n");
    }

    #[test]
    fn a_project_that_takes_nothing_leaves_the_data_directory() {
        let dir = TempDir::new("apart");
        let mut m = machine_with_changes(&dir.0, &[(1, "TEXT\n")]);
        m.rt.storage = Storage::none();
        let mut recovery = Recovery::new(dir.0.clone());
        let lines = recovery.save(&m).expect("something to save");
        let path = dir.0.join("recover1_prg1.txt");
        assert_eq!(std::fs::read(&path).unwrap(), b"TEXT\n");
        assert!(lines.iter().any(|l| l.contains("recover1_prg1.txt")));
    }
}

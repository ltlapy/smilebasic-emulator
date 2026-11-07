//! A thread that runs the program's frames while the main thread is held up
//! in a call to the system, as `SDL_PumpEvents` is for seconds while a window
//! is dragged on GNOME. The machine stays the main thread's; the loop lends it
//! for such a call with [`Standby::hold`], and the standby thread runs a frame
//! only once one is a whole frame late. See `docs/frame-thread.md`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sb_runtime::machine::Machine;

use crate::audio::Sink;
use crate::pacing::Clock;
use crate::toast::Toasts;
use crate::{FrameGuard, RUN_TIME};

/// What the loop lends for a call.
pub struct Lend<'a> {
    pub machine: &'a mut Machine,
    pub guard: &'a mut FrameGuard,
    pub toasts: &'a mut Toasts,
    pub clock: &'a mut Clock,
    /// Whether frames may run at all: not while the program is paused.
    pub frames: bool,
}

/// The frames the standby thread ran during a call, and the time they took.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Kept {
    pub frames: u32,
    pub busy: Duration,
}

impl Kept {
    pub fn add(&mut self, other: Kept) {
        self.frames += other.frames;
        self.busy += other.busy;
    }
}

/// A lent machine, as the standby thread holds it.
struct Job {
    machine: *mut Machine,
    guard: *mut FrameGuard,
    toasts: *mut Toasts,
    clock: Clock,
    kept: Kept,
}

// SAFETY: the pointers come from `&mut` borrows that `Standby::hold` keeps for
// as long as the job exists, and only one thread uses them at a time: the
// standby thread between setting `Slot::busy` and clearing it, under the mutex,
// and the main thread not at all until `hold` has taken the job back. The
// `Rc`s and `RefCell`s inside the machine (why it is not `Send`) move with it.
// The calls lent for are SDL's and touch none of them, including those shared
// with something outside the machine, such as the help.
unsafe impl Send for Job {}

#[derive(Default)]
struct Slot {
    job: Option<Job>,
    /// The main thread wants the job back.
    back: bool,
    /// The standby thread is running a frame of the job.
    busy: bool,
    quit: bool,
}

#[derive(Default)]
struct Shared {
    slot: Mutex<Slot>,
    /// Signals any change to the slot.
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct Standby {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Standby {
    /// Starts the thread, which plays the frames' sound into `sink`. Without
    /// a thread, frames stop during a call as they used to.
    pub fn start(sink: Sink) -> Standby {
        let shared = Arc::new(Shared::default());
        let theirs = shared.clone();
        let thread = std::thread::Builder::new()
            .name("standby frames".to_string())
            .spawn(move || serve(&theirs, &sink))
            .map_err(|e| eprintln!("no standby thread for frames: {e}"))
            .ok();
        Standby { shared, thread }
    }

    /// Makes `call`, with frames run on the standby thread meanwhile if it
    /// holds the main thread up. Gives what `call` gave and the frames kept;
    /// the clock comes back as the frames left it.
    pub fn hold<R>(&self, lend: Lend<'_>, call: impl FnOnce() -> R) -> (R, Kept) {
        if !lend.frames || self.thread.is_none() {
            return (call(), Kept::default());
        }
        {
            let mut slot = self.shared.lock();
            slot.job = Some(Job {
                machine: lend.machine,
                guard: lend.guard,
                toasts: lend.toasts,
                clock: *lend.clock,
                kept: Kept::default(),
            });
            slot.back = false;
        }
        self.shared.changed.notify_all();
        // Takes the job back however `call` ends, a panic included.
        let mut back = Back {
            shared: &self.shared,
            done: false,
        };
        let result = call();
        let job = back.take();
        match job {
            Some(job) => {
                *lend.clock = job.clock;
                (result, job.kept)
            }
            None => (result, Kept::default()),
        }
    }
}

impl Drop for Standby {
    fn drop(&mut self) {
        self.shared.lock().quit = true;
        self.shared.changed.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Takes a lent job back.
struct Back<'a> {
    shared: &'a Shared,
    done: bool,
}

impl Back<'_> {
    /// Waits for the frame in hand, if any, and takes the job.
    fn take(&mut self) -> Option<Job> {
        self.done = true;
        let mut slot = self.shared.lock();
        slot.back = true;
        while slot.busy {
            slot = (self.shared.changed.wait(slot)).unwrap_or_else(|e| e.into_inner());
        }
        slot.job.take()
    }
}

impl Drop for Back<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.take();
        }
    }
}

/// The standby thread: runs a lent job's frames as they fall a frame late.
fn serve(shared: &Shared, sink: &Sink) {
    let mut slot = shared.lock();
    loop {
        if slot.quit {
            return;
        }
        let due = match &slot.job {
            Some(job) if !slot.back => job.clock.due(),
            _ => {
                slot = shared.changed.wait(slot).unwrap_or_else(|e| e.into_inner());
                continue;
            }
        };
        let now = Instant::now();
        if now < due {
            let waited = shared.changed.wait_timeout(slot, due - now);
            slot = waited.unwrap_or_else(|e| e.into_inner()).0;
            continue;
        }
        let job = slot.job.as_ref().expect("checked");
        let (machine, guard, toasts, mut clock) = (job.machine, job.guard, job.toasts, job.clock);
        slot.busy = true;
        drop(slot);
        let began = Instant::now();
        // SAFETY: `busy` is set, so the main thread waits in `Back::take` and
        // keeps the borrows the pointers came from (see `Job`).
        let (machine, guard, toasts) = unsafe { (&mut *machine, &mut *guard, &mut *toasts) };
        // FrameGuard catches a panic in the frame; this one anything else, so
        // that `busy` is always cleared.
        let _ = catch_unwind(AssertUnwindSafe(|| {
            machine.time_limit = Some(RUN_TIME);
            guard.run(machine, toasts, |m| {
                m.frame();
                sink.play(&m.take_samples());
            });
        }));
        clock.caught_up();
        let took = began.elapsed();
        slot = shared.lock();
        slot.busy = false;
        if let Some(job) = &mut slot.job {
            job.clock = clock;
            job.kept.frames += 1;
            job.kept.busy += took;
        }
        shared.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FRAME;
    use sb_runtime::Runtime;
    use sb_runtime::font::Font;
    use sb_runtime::sound::output::Output;

    fn machine(source: &str) -> Machine {
        let mut m = Machine::new(Runtime::new(400, 240, Font::fallback()));
        m.boot();
        m.set_slot(0, source.to_string());
        m.run_slot(0);
        m
    }

    fn count(m: &mut Machine) -> u32 {
        m.rt.main_count()
    }

    /// Lends `m` for a call that takes `held`, with the clock as the loop
    /// leaves it just after running a frame.
    fn hold_for(standby: &Standby, m: &mut Machine, held: Duration) -> (Kept, Clock) {
        let (mut guard, mut toasts) = (FrameGuard::default(), Toasts::default());
        let mut clock = Clock::new(Instant::now());
        let lend = Lend {
            machine: m,
            guard: &mut guard,
            toasts: &mut toasts,
            clock: &mut clock,
            frames: true,
        };
        let ((), kept) = standby.hold(lend, || std::thread::sleep(held));
        (kept, clock)
    }

    #[test]
    fn a_short_call_runs_no_frame() {
        let standby = Standby::start(Sink::to(None));
        let mut m = machine("WHILE 1:VSYNC:WEND");
        let before = count(&mut m);
        for _ in 0..20 {
            let (kept, _) = hold_for(&standby, &mut m, Duration::from_millis(1));
            assert_eq!(kept, Kept::default());
        }
        assert_eq!(count(&mut m), before);
    }

    #[test]
    fn a_held_up_call_keeps_frames_and_sound_going() {
        let output = Arc::new(Mutex::new(Output::default()));
        let standby = Standby::start(Sink::to(Some(output.clone())));
        let mut m = machine("BGMPLAY \"T120[CDEFG]\":WHILE 1:VSYNC:WEND");
        // Settle the program and drop what it has mixed so far.
        m.frame();
        let _ = m.take_samples();
        let before = count(&mut m);
        let held = Duration::from_millis(500);
        let (kept, clock) = hold_for(&standby, &mut m, held);
        // One frame late before the first, then one a frame.
        let expected = (held.as_nanos() / FRAME.as_nanos()) as u32 - 1;
        assert!(
            // A busy machine may wake the thread late, never early.
            (expected - 5..=expected + 1).contains(&kept.frames),
            "{} frames kept",
            kept.frames
        );
        assert_eq!(count(&mut m) - before, kept.frames);
        // The clock is where the frames left it, not further behind.
        assert!(!clock.behind(Instant::now() - FRAME));
        let queued = output.lock().unwrap().queued();
        let samples = kept.frames as usize * sb_runtime::sound::FRAME_SAMPLES;
        // The output holds back three samples for its interpolation, and starts
        // with one.
        assert_eq!(queued, samples - 2);
    }

    #[test]
    fn frames_kept_see_the_input_given_before_the_call() {
        let standby = Standby::start(Sink::to(None));
        let source = "CLS:WHILE 1:LOCATE 0,0:?FORMAT$(\"%04D\",BUTTON(0) AND 8);:VSYNC:WEND";
        let mut m = machine(source);
        let shown = |m: &Machine| {
            let console = &m.rt.text[4];
            let codes: Vec<u16> = (0..4).map(|x| console.cell(x, 0).code).collect();
            String::from_utf16_lossy(&codes)
        };
        m.frame();
        m.frame();
        assert_eq!(shown(&m), "0000");
        m.rt.input.live.pads[0].buttons = 1 << 3;
        let (kept, _) = hold_for(&standby, &mut m, Duration::from_millis(100));
        assert!(kept.frames > 0);
        assert_eq!(shown(&m), "0008");
    }

    #[test]
    fn a_paused_program_runs_no_frame() {
        let standby = Standby::start(Sink::to(None));
        let mut m = machine("WHILE 1:VSYNC:WEND");
        let before = count(&mut m);
        let (mut guard, mut toasts) = (FrameGuard::default(), Toasts::default());
        let mut clock = Clock::new(Instant::now());
        let lend = Lend {
            machine: &mut m,
            guard: &mut guard,
            toasts: &mut toasts,
            clock: &mut clock,
            frames: false,
        };
        let ((), kept) = standby.hold(lend, || std::thread::sleep(Duration::from_millis(100)));
        assert_eq!(kept.frames, 0);
        assert_eq!(count(&mut m), before);
    }

    #[test]
    fn the_machine_comes_back_from_a_call_that_panics() {
        let standby = Standby::start(Sink::to(None));
        let mut m = machine("WHILE 1:VSYNC:WEND");
        let (mut guard, mut toasts) = (FrameGuard::default(), Toasts::default());
        let mut clock = Clock::new(Instant::now());
        let lend = Lend {
            machine: &mut m,
            guard: &mut guard,
            toasts: &mut toasts,
            clock: &mut clock,
            frames: true,
        };
        let panicked = catch_unwind(AssertUnwindSafe(|| {
            standby.hold(lend, || {
                std::thread::sleep(Duration::from_millis(60));
                panic!("in the call")
            })
        }));
        assert!(panicked.is_err());
        // The standby thread has let go: the next lend works.
        let (kept, _) = hold_for(&standby, &mut m, Duration::from_millis(60));
        assert!(kept.frames > 0);
    }
}

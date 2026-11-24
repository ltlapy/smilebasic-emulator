# Frames while the main thread is blocked

The window runs the program's frames, mixes their sound and draws them on the
main thread. SDL needs its events, windows and renderers there too. When a call
into SDL blocks that thread, the program and its sound stop with it.

The common case is GNOME. While you drag or resize the window, libdecor's GTK
plugin redraws the decorations on the main thread inside `SDL_PumpEvents`. One
pump takes up to half a second on a busy machine, and 5 to 10 s has been seen.
Fedora ships libdecor 0.2.5; SDL skips that work with libdecor 0.3 and later.

## The design: a standby thread

`crates/sb-sdl/src/standby.rs` keeps one thread waiting. The machine lives on
the main thread, as before. Around each call that can block the main thread,
the loop lends the machine to the standby thread:

```rust
let (polled, kept) = standby.hold(parts, || drain(&mut events));
```

While the call runs, the standby thread watches the frames' clock. If a frame
is a whole frame late, counted the same way as the main loop's own catch-up,
and the call has taken 4 ms (draining the events otherwise takes less),
the standby thread runs it: `Machine::frame` under the `FrameGuard`, with the
time limit the loop's frames have, and gives its samples to the sound device.
It continues at 60 frames per second until the call returns. Then `hold` takes
the machine back, waiting at most for the frame in progress, and the loop
continues with the clock as the standby thread left it. It draws the latest
state once and does not replay the frames it missed drawing, the same as with
frames it skips to catch up.

The loop lends the machine for:

- draining SDL's events, which pumps them (`events.poll_event` until the
  queue is empty, into a buffer the loop then handles as before);
- the bench's `at=N:sleep:MS`, which imitates such a call.

The loop does not lend the machine for presents or for the wait for the next
frame: the pacing in `pacing.rs` already turns vsync off for a window whose
presents are held back, and the wait ends when the next frame is due.

### Why not run every frame on a separate thread

A thread that runs all frames, with the window only drawing, would also let
the program's work overlap the drawing. It needs much more than this:

- `Machine` is not `Send`. Values, programs, waves and the help hold `Rc` and
  `RefCell`. `Arc` in their place would put atomic counts on the VM's hot paths.
- The GPU path draws a scene through SDL's renderer, which stays on the main
  thread. That scene reads GRP pages, text cells and sprites that the next frame
  changes, so the frame thread would have to hand over copies, or the changes
  to them, every frame. Six GRP pages hold 96 MiB.
- The loop reads and changes the machine in a few hundred places per iteration:
  the editor, menus, dialogs, the debugger's panel, file confirmations, the
  pointer and the status line. Each would become a message or a copy.

On this project's x86-64 desktop the overlap has little to win. With 4000
sprites turning and a `GFILL` per frame, `SB_UI_BENCH` gives 1.38 ms of `run`
and 1.16 ms of `screen` per frame on the CPU path, and 1.28 ms and 0.22 ms on
the GPU path, well inside a frame either way. Overlap saves at most the smaller
of the two. A Cortex-A35 runs both several times slower, and there the CPU
path's sum could pass 16.7 ms where each part alone would not. That is the
argument for the larger change.

A blocked main thread is rare and has no input to give anyway, since the events
are what is stuck. Lending the machine only for those calls covers this case
and leaves everything else unchanged.

### What stays the same

Outside those calls the machine belongs to the main thread, so these work as
before:

- input timing and lag: a frame sees the input the loop gave it before the
  call, and no events come in during the call;
- the debugger, the editor, menus, dialogs, `DIALOG` and the file confirmations;
- renderer resets, device loss and `@RECOVERn_SLOT.PRG`: none of them happens
  inside a lend;
- the speed setting, `VSYNC` pacing, `MAINCNT` and the frames lost;
- `smilebasic-check`, which has no window and no standby thread.

While the menu bar pauses the program, or after `FrameGuard` has stopped frames,
the standby thread runs nothing.

### Sharing the machine safely

`hold` turns the machine, the `FrameGuard`, the toasts and the clock into raw
pointers for the standby thread and gives them back before it returns, also
when the call panics. The borrow checker keeps the loop off the machine for the
call. A mutex and condition variable carry the hand-over, so everything the
standby thread wrote is visible to the main thread afterwards. The lock is held
only to change hands, never across a frame.

The `Rc`s and `RefCell`s that make `Machine` not `Send` are safe this way
because one thread at a time touches them. Some are also shared outside the
machine, such as the help, which the help viewer uses. The calls the machine
is lent for are SDL calls and touch none of them.

The one thing in the machine that calls SDL is the clipboard. On the standby
thread, `CLIPBOARD` reads the text it last read or set, and a text set there
goes to the system clipboard when the main thread has the machine back
(`Clipboard::settle`).

### Cost

Lending the machine costs a lock and a wake-up of the standby thread each
iteration, a few microseconds; taking it back costs a lock when no frame
ran. Nothing is allocated for it.

## Measuring

`SB_UI_BENCH` reports `kept`, the frames the standby thread ran, and the time
it took running them. The `log=` lines end with the same two columns, and
`maincnt` gives how far `MAINCNT` advanced during the timed period.
[benchmarking.md](../performance/benchmarking.md) says how to drag a window with no screen.

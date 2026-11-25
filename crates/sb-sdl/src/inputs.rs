//! The state of the keys, the mouse and the controllers that the frontend
//! keeps between events, and the role of the pad in the emulator's own
//! screens.

use std::time::{Duration, Instant};

use sdl3::event::Event;
use sdl3::gamepad::{Axis, Button};

use crate::controls::{self, Controls};
use crate::pad_repeat::{Dir, PadRepeat};
use crate::simple_ui;

/// The `input_screen` number when none of the emulator's screens takes the
/// input.
pub const NO_SCREEN: usize = 0;
/// The `input_screen` number of a dialog.
pub const DIALOG_SCREEN: usize = 1;
/// The `input_screen` number of the project manager in mode 0. Each mode
/// adds its `mode_id`.
pub const MANAGER_SCREEN: usize = 2;
/// The `input_screen` number of the Simple UI's pause menu.
pub const PAUSE_SCREEN: usize = 20;
/// The `input_screen` number of the game list in mode 0. Each mode adds
/// its `mode_id`.
pub const GAME_LIST_SCREEN: usize = 21;
/// The lowest `input_screen` number whose screen repeats the pad. The
/// project manager in any mode, the pause menu and the game list repeat,
/// but a dialog does not.
pub const FIRST_REPEATING_SCREEN: usize = MANAGER_SCREEN;

/// The state of the keys, the mouse and the controllers that frontend()
/// keeps between events.
pub struct Inputs {
    pub controls: Controls,
    pub typing: controls::Typing,
    /// Select, for the Simple UI's menu and quitting.
    pub hotkeys: simple_ui::Hotkeys,
    /// Holding the pad's up or down repeats in the projects, and the left
    /// stick works as the pad there.
    pub pad_repeat: PadRepeat,
    /// The screen that `pad_repeat` was last ticked for.
    pub pad_screen: usize,
    pub pad_ticked_at: Instant,
    /// The left button was pressed on the editor and is still held.
    pub dragging: bool,
    /// The time the + button (F5, or the button on a controller) was
    /// pressed, while it is held.
    pub plus_down: Option<Instant>,
    /// The text of a key that something else took: the space some systems
    /// type for Ctrl+Space, or a digit that chose a slot in a window that
    /// then closed, which would otherwise reach the program or the editor.
    pub drop_next_text: bool,
    /// A USB key that reads as down for a frame, for the bench.
    pub bench_usb: Option<u8>,
}

impl Inputs {
    pub fn new(controls: Controls) -> Inputs {
        Inputs {
            controls,
            typing: controls::Typing::default(),
            hotkeys: simple_ui::Hotkeys::default(),
            pad_repeat: PadRepeat::default(),
            pad_screen: NO_SCREEN,
            pad_ticked_at: Instant::now(),
            dragging: false,
            plus_down: None,
            drop_next_text: false,
            bench_usb: None,
        }
    }

    /// Returns the pad's repeats for this iteration on the emulator's
    /// screen `screen` (see `input_screen`): presses of up or down for as
    /// long as it was held, once per frame regardless of the events that
    /// came. A new screen or mode starts without the old hold.
    pub fn repeats(&mut self, screen: usize) -> Vec<Event> {
        let dt = self.pad_ticked_at.elapsed().min(Duration::from_millis(100));
        self.pad_ticked_at = Instant::now();
        if screen != self.pad_screen {
            self.pad_screen = screen;
            self.pad_repeat.reset();
        }
        let mut repeats = Vec::new();
        if screen >= FIRST_REPEATING_SCREEN
            && let Some((dir, n)) = self.pad_repeat.tick(dt)
        {
            let (timestamp, which) = (0, 0.into());
            let button = dir.button();
            repeats.resize(
                n as usize,
                Event::GamepadButtonDown {
                    timestamp,
                    which,
                    button,
                },
            );
        }
        repeats
    }

    /// Notes the pad's up and down as held or released, unless the event
    /// is one of our repeats, and returns the event to handle. On the
    /// emulator's own screens, a push of the left stick is a press of the
    /// pad.
    pub fn pad_event(&mut self, event: Event, repeated: bool, screen: usize) -> Event {
        match event {
            Event::GamepadButtonDown { button, .. } if !repeated => {
                match button {
                    Button::DPadUp => self.pad_repeat.down(Dir::Up),
                    Button::DPadDown => self.pad_repeat.down(Dir::Down),
                    _ => {}
                }
                event
            }
            Event::GamepadButtonUp { button, .. } => {
                match button {
                    Button::DPadUp => self.pad_repeat.up(Dir::Up),
                    Button::DPadDown => self.pad_repeat.up(Dir::Down),
                    _ => {}
                }
                event
            }
            Event::GamepadAxisMotion {
                timestamp,
                which,
                axis,
                value,
            } => {
                let pushed = match axis {
                    Axis::LeftX => self.pad_repeat.stick_x(value),
                    Axis::LeftY => self.pad_repeat.stick_y(value),
                    _ => None,
                };
                match pushed {
                    Some(dir) if screen != NO_SCREEN => Event::GamepadButtonDown {
                        timestamp,
                        which,
                        button: dir.button(),
                    },
                    _ => event,
                }
            }
            event => event,
        }
    }
}

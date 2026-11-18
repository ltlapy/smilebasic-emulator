//! What the frontend keeps of the keys, the mouse and the controllers
//! between events, and the pad's part in the emulator's own screens.

use std::time::{Duration, Instant};

use sdl3::event::Event;
use sdl3::gamepad::{Axis, Button};

use crate::controls::{self, Controls};
use crate::pad_repeat::{Dir, PadRepeat};
use crate::simple_ui;

/// What frontend() keeps of the keys, the mouse and the controllers
/// between events.
pub struct Inputs {
    pub controls: Controls,
    pub typing: controls::Typing,
    /// Select, for the Simple UI's menu and quitting.
    pub hotkeys: simple_ui::Hotkeys,
    /// Holding the pad's up or down repeats in the projects, and the left
    /// stick works as the pad there. `pad_screen` is the screen it was
    /// last ticked for, `pad_tick` when.
    pub pad_repeat: PadRepeat,
    pub pad_screen: usize,
    pub pad_tick: Instant,
    /// The left button went down on the editor and is still held.
    pub dragging: bool,
    /// When the + button, F5 or a controller's, went down, while it is
    /// held.
    pub plus_down: Option<Instant>,
    /// The text of a key that something else took: the space some systems
    /// type for Ctrl+Space, or a digit that chose a slot in a window that
    /// then closed, which would otherwise reach the program or the editor.
    pub eat_text: bool,
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
            pad_screen: 0,
            pad_tick: Instant::now(),
            dragging: false,
            plus_down: None,
            eat_text: false,
            bench_usb: None,
        }
    }

    /// The pad's repeats for this trip on the emulator's screen `screen`
    /// (see `input_screen`): presses of up or down for as long as it was
    /// held, once a frame whatever events came. A new screen or mode
    /// starts without the old hold, and only the lists repeat.
    pub fn repeats(&mut self, screen: usize) -> Vec<Event> {
        let dt = self.pad_tick.elapsed().min(Duration::from_millis(100));
        self.pad_tick = Instant::now();
        if screen != self.pad_screen {
            self.pad_screen = screen;
            self.pad_repeat.reset();
        }
        let mut repeats = Vec::new();
        if screen >= 2
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
    /// is a repeat of ours, and gives the event to handle: the left stick
    /// pushed is the pad's press, on the emulator's own screens.
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
                    Some(dir) if screen != 0 => Event::GamepadButtonDown {
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

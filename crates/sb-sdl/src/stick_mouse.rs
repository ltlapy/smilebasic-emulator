//! SmileBASIC 4's right-stick mouse: the right stick moves the pointer
//! and R3 is the left mouse button, for the program or for the emulator's
//! own screens, and Select holds the stick for the pointer in any program.

use sb_runtime::input::{PADS, State, Tap};
use sdl3::event::Event;
use sdl3::gamepad::Button;
use sdl3::joystick::JoystickId;
use sdl3::mouse::{MouseButton, MouseState};

use crate::controls::{CARRIED_STICK, on_display};

/// #B_RSTICK's bit in BUTTON's value: R3, the right stick pressed.
const R3: u32 = 1 << 13;

/// How far the right stick tilted by `x`, `y` (Y down, as [`Pad::sticks`]
/// has it) moves the mouse pointer in a frame, in pixels of SmileBASIC
/// 4's 1280x720 display, as SmileBASIC 4 moves it: a speed by how far the
/// stick is tilted, nothing up to 0.3, and no speeding up while held.
///
/// [`Pad::sticks`]: sb_runtime::input::Pad::sticks
pub fn stick_step(x: f64, y: f64) -> (i32, i32) {
    let k = match x.hypot(y) {
        m if m > 0.85 => 16.0,
        m if m > 0.7 => 8.0,
        m if m > 0.4 => 4.0,
        m if m > 0.3 => 3.0,
        _ => return (0, 0),
    };
    let step = |a: f64| (a.abs() * k + 0.5) as i32;
    // An axis goes left or up only from a tenth of a tilt that way, and
    // right or down below that: a stick held right that drifts a little
    // up still goes down.
    let dx = if x <= -0.1 { -step(x) } else { step(x) };
    let dy = if -y >= 0.1 { -step(y) } else { step(y) };
    (dx, dy)
}

/// Select (SDL's Back, the − button) as the right-stick mouse and the
/// projects see it.
#[derive(Debug, Default)]
pub struct Select {
    held: bool,
    /// Another button went down while it was held, or the pointer moved.
    used: bool,
    /// Its press may open the projects at its release: see
    /// [`Select::arm`].
    armed: bool,
}

impl Select {
    pub(crate) fn event(&mut self, event: &Event) {
        match *event {
            Event::GamepadButtonDown {
                button: Button::Back,
                ..
            } => {
                *self = Select {
                    held: true,
                    ..Select::default()
                }
            }
            Event::GamepadButtonUp {
                button: Button::Back,
                ..
            } => self.held = false,
            Event::GamepadButtonDown { .. } => self.used |= self.held,
            _ => {}
        }
    }

    /// The press of Select now is one that opens the projects if it is
    /// released with nothing else done meanwhile.
    pub fn arm(&mut self) {
        self.armed = self.held;
    }

    /// At Select's release: whether it was pressed alone, after an
    /// [`Select::arm`] at its press. Only once for each press.
    pub fn alone(&mut self) -> bool {
        std::mem::take(&mut self.armed) && !self.used
    }
}

/// The right stick and R3 as the program does not see them: while Select
/// moves the pointer, and after that until they are released.
#[derive(Debug, Default)]
struct HeldBack {
    r3: [bool; PADS],
    stick: [bool; PADS],
}

impl HeldBack {
    /// Holds back from `state`, as the program will see it, what the
    /// pointer took this frame, `taken` while Select is held or one of
    /// the emulator's screens has the controls.
    fn apply(&mut self, state: &mut State, taken: bool) {
        for (i, pad) in state.pads.iter_mut().enumerate() {
            let r3 = pad.buttons & R3 != 0;
            let (x, y) = pad.sticks[1];
            let tilted = x.abs() > CARRIED_STICK || y.abs() > CARRIED_STICK;
            self.r3[i] = r3 && (taken || self.r3[i]);
            self.stick[i] = taken || tilted && self.stick[i];
            if self.r3[i] {
                pad.buttons &= !R3;
            }
            if self.stick[i] {
                pad.sticks[1] = (0.0, 0.0);
            }
        }
    }
}

/// The right stick as the right-stick mouse reads it: each axis from the
/// controller tilted furthest along it, and whether any has R3 pressed.
fn right_stick(state: &State) -> ((f64, f64), bool) {
    // UNVERIFIED: SmileBASIC 4 merges the controllers into one, by the
    // largest reading of each axis; taken here as the largest either way.
    let furthest = |a: f64, b: f64| if b.abs() > a.abs() { b } else { a };
    state
        .pads
        .iter()
        .fold(((0.0, 0.0), false), |((x, y), r3), pad| {
            let (px, py) = pad.sticks[1];
            (
                (furthest(x, px), furthest(y, py)),
                r3 || pad.buttons & R3 != 0,
            )
        })
}

/// Who the right stick and R3 are for, by what is open: see
/// [`Where::role`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StickRole {
    /// Neither the program nor the emulator's screens, as while the
    /// editor's own window has the keys: they are no mouse, and the
    /// program sees them as ever.
    Off,
    /// The program's, as SmileBASIC 4's option has them: the right stick
    /// moves the pointer and R3 is the left mouse button while `cursor`
    /// (the program may show the mouse cursor) or Select is held.
    Program { cursor: bool },
    /// One of the emulator's own screens on the desktop has them: the
    /// right stick moves the pointer, and R3 clicks with the left button.
    Screens,
    /// One of the Simple UI's own screens has them, which go by the pad
    /// alone: they are no mouse, and no cursor shows.
    Hidden,
}

impl StickRole {
    /// Whether the mouse cursor stays hidden, whatever else wants it.
    pub fn hides_cursor(self) -> bool {
        self == StickRole::Hidden
    }
}

/// What is open, for [`Where::role`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Where {
    pub simple_ui: bool,
    /// A program runs.
    pub running: bool,
    /// One of the emulator's screens has the controls: a window or dialog
    /// of its own, the menu bar paused on the program, an open menu, or
    /// the editor.
    pub screen: bool,
    /// The program's DIALOG is open.
    pub program_dialog: bool,
    /// The running program shows the mouse cursor (`MOUSECURSOR`).
    pub cursor: bool,
    /// The editor's own window has the keys, away from the main window's
    /// pointer.
    pub editor_window: bool,
}

impl Where {
    /// Who the right stick and R3 are for. On the desktop the emulator's
    /// screens and a program's DIALOG take them, and with no program
    /// running so does everything else there is: DIRECT mode, the menu
    /// bar, the editor. The Simple UI's screens and DIALOG are for the
    /// pad. Otherwise the program has them.
    pub fn role(self) -> StickRole {
        let ours = !self.running || self.screen || self.program_dialog;
        match self.simple_ui {
            _ if self.editor_window => StickRole::Off,
            true if ours => StickRole::Hidden,
            false if ours => StickRole::Screens,
            _ => StickRole::Program {
                cursor: self.cursor,
            },
        }
    }
}

/// The mouse that R3's clicks come from, in [`StickClick`]'s events: not
/// one SDL gives, nor its ids for touches and pens.
pub const STICK_MOUSE: u32 = u32::MAX - 2;

/// Two presses of R3 within this time, in SDL's nanoseconds, and within
/// [`DOUBLE_CLICK_RADIUS`] in window coordinates, make a double click, as
/// SDL counts a mouse's by default.
const DOUBLE_CLICK_TIME: u64 = 500_000_000;
const DOUBLE_CLICK_RADIUS: f32 = 32.0;

/// R3 as the left mouse button of the emulator's own screens: its press
/// and release become the mouse's, at the pointer, which go where a
/// mouse's go.
#[derive(Debug, Default)]
struct StickClick {
    /// The controller whose R3 holds the button down, and the window the
    /// press went to.
    down: Option<(JoystickId, u32)>,
    /// The last press: when, where, and its count of clicks.
    last: Option<(u64, (f32, f32), u8)>,
}

impl StickClick {
    /// The clicks a press at `timestamp` and `at` counts: 2 for a double
    /// click.
    fn count(&mut self, timestamp: u64, at: (f32, f32)) -> u8 {
        let near = |(x, y): (f32, f32)| {
            (at.0 - x).abs() <= DOUBLE_CLICK_RADIUS && (at.1 - y).abs() <= DOUBLE_CLICK_RADIUS
        };
        let clicks = match self.last {
            Some((then, place, n))
                if timestamp.saturating_sub(then) < DOUBLE_CLICK_TIME && near(place) =>
            {
                n.saturating_add(1)
            }
            _ => 1,
        };
        self.last = Some((timestamp, at, clicks));
        clicks
    }

    /// Makes `event` the left mouse button's: a press of R3 while the
    /// emulator's screens have it (`role`), at window coordinates `at` of
    /// window `window`, and the release of that R3 whatever has it by
    /// then. Meanwhile the mouse's moves have the left button held, so
    /// that tilting the stick drags.
    fn event(&mut self, event: &mut Event, role: StickRole, window: u32, at: (f32, f32)) {
        let mouse_btn = MouseButton::Left;
        let (x, y) = at;
        match *event {
            Event::GamepadButtonDown {
                timestamp,
                which,
                button: Button::RightStick,
            } if role == StickRole::Screens && self.down.is_none() => {
                let clicks = self.count(timestamp, at);
                self.down = Some((which, window));
                *event = Event::MouseButtonDown {
                    timestamp,
                    window_id: window,
                    which: STICK_MOUSE,
                    mouse_btn,
                    clicks,
                    x,
                    y,
                };
            }
            Event::GamepadButtonUp {
                timestamp,
                which,
                button: Button::RightStick,
            } if self.down.is_some_and(|(pad, _)| pad == which) => {
                let window_id = self.down.take().map_or(window, |(_, window)| window);
                let clicks = self.last.map_or(1, |(_, _, n)| n);
                *event = Event::MouseButtonUp {
                    timestamp,
                    window_id,
                    which: STICK_MOUSE,
                    mouse_btn,
                    clicks,
                    x,
                    y,
                };
            }
            Event::MouseMotion {
                ref mut mousestate, ..
            } if self.down.is_some() => {
                let left = MouseState::from_sdl_state(1).to_sdl_state();
                *mousestate = MouseState::from_sdl_state(mousestate.to_sdl_state() | left);
            }
            // Its release will not come.
            Event::GamepadRemoved { which, .. }
                if self.down.is_some_and(|(pad, _)| pad == which) =>
            {
                self.down = None;
            }
            _ => {}
        }
    }
}

/// SmileBASIC 4's right-stick mouse, and Select's use of it.
#[derive(Debug, Default)]
pub struct StickMouse {
    /// The setting Right stick moves the mouse, for the next event and
    /// frame.
    pub on: bool,
    pub select: Select,
    held_back: HeldBack,
    click: StickClick,
}

impl StickMouse {
    /// Whether controller `pad`'s `tap` is R3 while the program does not
    /// see it, as [`StickMouse::frame`] keeps it.
    pub(crate) fn holds_back(&self, pad: usize, tap: Tap) -> bool {
        self.on && tap == Tap::Buttons(pad, R3) && (self.select.held || self.held_back.r3[pad])
    }

    /// The right-stick mouse in a frame, on `state` as the program is to
    /// see it: the right stick moves the pointer from the pixel `from` of
    /// SmileBASIC 4's display to the one this returns, as `role` says. For the program R3 is the left mouse button, and with Select
    /// held this works whatever the program does, which then sees neither
    /// the right stick nor R3. Nor does it see them while the emulator's
    /// screens have them.
    pub fn frame(
        &mut self,
        state: &mut State,
        role: StickRole,
        from: (i32, i32),
    ) -> Option<(i32, i32)> {
        if !self.on {
            self.held_back = HeldBack::default();
            return None;
        }
        let select = self.select.held;
        let (moves, ours) = match role {
            StickRole::Off => return None,
            StickRole::Program { cursor } => (cursor || select, false),
            StickRole::Screens => (true, true),
            StickRole::Hidden => (false, true),
        };
        let ((x, y), r3) = right_stick(state);
        self.held_back.apply(state, select || ours);
        if !moves {
            return None;
        }
        // It takes the place of the mouse's buttons, as on SmileBASIC 4.
        // The emulator's screens have its clicks from `click` instead.
        if r3 && !ours {
            state.mouse.buttons = 1;
        }
        let step = stick_step(x, y);
        if step == (0, 0) {
            return None;
        }
        self.select.used |= select;
        Some(on_display((from.0 + step.0, from.1 + step.1)))
    }

    /// Whether Select has the right stick for the pointer now, which
    /// shows the cursor whatever the program says.
    pub fn select_points(&self) -> bool {
        self.on && self.select.held
    }

    /// Makes `event` the left mouse button's where it is R3 for the
    /// emulator's screens, as [`StickClick::event`] says.
    pub(crate) fn click(
        &mut self,
        event: &mut Event,
        role: StickRole,
        window: u32,
        at: (f32, f32),
    ) {
        // A press made before the setting went off still gets its release.
        if self.on || self.click.down.is_some() {
            self.click.event(event, role, window, at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn holding(buttons: u32) -> State {
        let mut state = State::default();
        state.pads[0].buttons = buttons;
        state
    }

    fn pad_event(button: Button, down: bool) -> Event {
        let (timestamp, which) = (0, 7.into());
        match down {
            true => Event::GamepadButtonDown {
                timestamp,
                which,
                button,
            },
            false => Event::GamepadButtonUp {
                timestamp,
                which,
                button,
            },
        }
    }

    #[test]
    fn the_right_stick_moves_the_pointer_by_how_far_it_is_tilted() {
        assert_eq!(stick_step(1.0, 0.0), (16, 0));
        assert_eq!(stick_step(0.86, 0.0), (14, 0));
        assert_eq!(stick_step(0.85, 0.0), (7, 0));
        assert_eq!(stick_step(0.8, 0.0), (6, 0));
        assert_eq!(stick_step(0.7, 0.0), (3, 0));
        assert_eq!(stick_step(0.5, 0.0), (2, 0));
        assert_eq!(stick_step(0.4, 0.0), (1, 0));
        assert_eq!(stick_step(0.35, 0.0), (1, 0));
        assert_eq!(stick_step(-1.0, 0.0), (-16, 0));
        assert_eq!(stick_step(0.0, 1.0), (0, 16));
        assert_eq!(stick_step(0.0, -1.0), (0, -16));
        // Each axis goes by its share of a full tilt.
        let d = std::f64::consts::FRAC_1_SQRT_2;
        assert_eq!(stick_step(d, -d), (11, -11));
        assert_eq!(stick_step(-d, d), (-11, 11));
    }

    #[test]
    fn the_right_stick_moves_nothing_up_to_three_tenths() {
        for (x, y) in [
            (0.3, 0.0),
            (0.25, 0.0),
            (0.2, -0.2),
            (0.0, 0.0),
            (-0.1, 0.28),
        ] {
            assert_eq!(stick_step(x, y), (0, 0), "{x}, {y}");
        }
    }

    #[test]
    fn a_reading_short_of_a_tenth_left_or_up_goes_right_or_down() {
        // Held right, drifting a little either way: down by a pixel.
        assert_eq!(stick_step(1.0, -0.05), (16, 1));
        assert_eq!(stick_step(1.0, 0.05), (16, 1));
        // Held up, drifting a little left: right by a pixel.
        assert_eq!(stick_step(-0.05, -1.0), (1, -16));
        // From a tenth, that way.
        assert_eq!(stick_step(-0.1, -1.0), (-2, -16));
        assert_eq!(stick_step(1.0, -0.1), (16, -2));
    }

    #[test]
    fn the_pointer_stays_on_the_display() {
        let mut stick = stick_mouse();
        let mut moved = |from, tilt| {
            let mut state = holding(0);
            state.pads[0].sticks[1] = tilt;
            stick.frame(&mut state, CURSOR, from)
        };
        // A full tilt goes 16 of SmileBASIC 4's pixels, whatever size the
        // UI's display is.
        assert_eq!(moved((100, 100), (1.0, 0.0)), Some((116, 100)));
        assert_eq!(moved((1275, 3), (1.0, -1.0)), Some((1279, 0)));
        assert_eq!(moved((1279, 0), (1.0, -1.0)), Some((1279, 0)));
        assert_eq!(moved((0, 715), (-1.0, 1.0)), Some((0, 719)));
        assert_eq!(moved((0, 719), (0.0, -0.35)), Some((0, 718)));
    }

    fn stick_mouse() -> StickMouse {
        StickMouse {
            on: true,
            ..StickMouse::default()
        }
    }

    /// A running program that shows the mouse cursor, and one that hides it.
    const CURSOR: StickRole = StickRole::Program { cursor: true };
    const NO_CURSOR: StickRole = StickRole::Program { cursor: false };

    #[test]
    fn r3_is_the_left_mouse_button_while_the_cursor_may_show() {
        let pressed = || {
            let mut state = holding(0);
            state.pads[1].buttons = R3;
            state.mouse.buttons = 0b110;
            state
        };
        let mut stick = stick_mouse();
        let mut state = pressed();
        assert_eq!(stick.frame(&mut state, CURSOR, (10, 10)), None);
        // In place of the mouse's own buttons, and BUTTON still sees it.
        assert_eq!(state.mouse.buttons, 1);
        assert_eq!(state.pads[1].buttons, R3);
        assert!(!stick.holds_back(1, Tap::Buttons(1, R3)));
        let mut state = pressed();
        stick.frame(&mut state, NO_CURSOR, (10, 10));
        assert_eq!(state.mouse.buttons, 0b110);
        let mut state = pressed();
        stick.on = false;
        stick.frame(&mut state, CURSOR, (10, 10));
        assert_eq!(state.mouse.buttons, 0b110);
    }

    #[test]
    fn the_program_sees_the_stick_that_moves_the_pointer() {
        let mut stick = stick_mouse();
        let mut state = holding(0);
        state.pads[2].sticks[1] = (1.0, 0.0);
        state.pads[0].sticks[1] = (0.2, -0.9);
        // Each axis from the controller tilted furthest along it.
        let moved = stick.frame(&mut state, CURSOR, (10, 100));
        assert_eq!(moved, Some((26, 86)));
        assert_eq!(state.pads[2].sticks[1], (1.0, 0.0));
        assert_eq!(stick.frame(&mut state, NO_CURSOR, (10, 100)), None);
        stick.on = false;
        assert_eq!(stick.frame(&mut state, CURSOR, (10, 100)), None);
    }

    #[test]
    fn select_points_in_any_program_and_keeps_the_stick_and_r3_from_it() {
        let mut stick = stick_mouse();
        let tilted = |buttons| {
            let mut state = holding(buttons | 1 << 1);
            state.pads[0].sticks = [(0.5, 0.0), (0.0, -1.0)];
            state
        };
        stick.select.event(&pad_event(Button::Back, true));
        assert!(stick.select_points());
        let mut state = tilted(R3);
        assert_eq!(
            stick.frame(&mut state, NO_CURSOR, (10, 100)),
            Some((10, 84))
        );
        assert_eq!(state.pads[0].sticks, [(0.5, 0.0), (0.0, 0.0)]);
        assert_eq!(state.pads[0].buttons, 1 << 1);
        assert_eq!(state.mouse.buttons, 1);
        assert!(stick.holds_back(0, Tap::Buttons(0, R3)));
        assert!(!stick.holds_back(0, Tap::Buttons(0, 1 << 1)));
        // Let go of Select first: the stick and R3 stay the pointer's until
        // they are released.
        stick.select.event(&pad_event(Button::Back, false));
        assert!(!stick.select_points());
        let mut state = tilted(R3);
        assert_eq!(stick.frame(&mut state, NO_CURSOR, (10, 84)), None);
        assert_eq!(state.pads[0].sticks[1], (0.0, 0.0));
        assert_eq!(state.pads[0].buttons, 1 << 1);
        assert!(stick.holds_back(0, Tap::Buttons(0, R3)));
        let mut state = holding(0);
        stick.frame(&mut state, NO_CURSOR, (10, 84));
        let mut state = tilted(R3);
        stick.frame(&mut state, NO_CURSOR, (10, 84));
        assert_eq!(state.pads[0].sticks[1], (0.0, -1.0));
        assert_eq!(state.pads[0].buttons, R3 | 1 << 1);
        assert!(!stick.holds_back(0, Tap::Buttons(0, R3)));
    }

    #[test]
    fn select_opens_the_projects_only_when_pressed_alone() {
        let mut stick = stick_mouse();
        let select =
            |stick: &mut StickMouse, down| stick.select.event(&pad_event(Button::Back, down));
        select(&mut stick, true);
        stick.select.arm();
        select(&mut stick, false);
        assert!(stick.select.alone());
        assert!(!stick.select.alone());
        // Not with another button pressed meanwhile...
        select(&mut stick, true);
        stick.select.arm();
        stick.select.event(&pad_event(Button::South, true));
        select(&mut stick, false);
        assert!(!stick.select.alone());
        // ...or the pointer moved...
        select(&mut stick, true);
        stick.select.arm();
        let mut state = holding(0);
        state.pads[0].sticks[1] = (1.0, 0.0);
        stick.frame(&mut state, NO_CURSOR, (0, 0));
        select(&mut stick, false);
        assert!(!stick.select.alone());
        // ...or a press that was not one to open them.
        select(&mut stick, true);
        select(&mut stick, false);
        assert!(!stick.select.alone());
        // A stick tilted too little to move the pointer does not count.
        select(&mut stick, true);
        stick.select.arm();
        let mut state = holding(0);
        state.pads[0].sticks[1] = (0.2, 0.0);
        stick.frame(&mut state, NO_CURSOR, (0, 0));
        select(&mut stick, false);
        assert!(stick.select.alone());
    }

    #[test]
    fn the_emulators_screens_take_the_stick_on_the_desktop_and_hide_it_in_the_simple_ui() {
        let program = Where {
            running: true,
            cursor: true,
            ..Where::default()
        };
        assert_eq!(program.role(), CURSOR);
        let hidden = Where {
            cursor: false,
            ..program
        };
        assert_eq!(hidden.role(), NO_CURSOR);
        // A window over the program, whatever MOUSECURSOR says, and with
        // no program running.
        let screen = Where {
            screen: true,
            ..hidden
        };
        assert_eq!(screen.role(), StickRole::Screens);
        let direct = Where::default();
        assert_eq!(direct.role(), StickRole::Screens);
        // A program's DIALOG takes them on the desktop, as our screens do,
        // whatever MOUSECURSOR says.
        let dialog = Where {
            program_dialog: true,
            cursor: false,
            ..program
        };
        assert_eq!(dialog.role(), StickRole::Screens);
        // The Simple UI's screens, and a program's DIALOG there, have no
        // mouse.
        let simple = |at: Where| Where {
            simple_ui: true,
            ..at
        };
        assert_eq!(simple(program).role(), CURSOR);
        for at in [screen, direct, dialog] {
            assert_eq!(simple(at).role(), StickRole::Hidden);
            assert!(simple(at).role().hides_cursor());
        }
        assert!(!screen.role().hides_cursor());
        // The editor's own window is away from the pointer.
        let editor = Where {
            editor_window: true,
            ..screen
        };
        assert_eq!(editor.role(), StickRole::Off);
    }

    #[test]
    fn the_emulators_screens_move_the_pointer_and_keep_the_stick_and_r3_from_the_program() {
        let mut stick = stick_mouse();
        let tilted = || {
            let mut state = holding(R3);
            state.pads[0].sticks[1] = (1.0, 0.0);
            state
        };
        let mut state = tilted();
        let moved = stick.frame(&mut state, StickRole::Screens, (10, 10));
        assert_eq!(moved, Some((26, 10)));
        // R3 clicks there by its events, not as the program's button.
        assert_eq!(state.mouse.buttons, 0);
        assert_eq!(
            (state.pads[0].buttons, state.pads[0].sticks[1]),
            (0, (0.0, 0.0))
        );
        assert!(stick.holds_back(0, Tap::Buttons(0, R3)));
        // The Simple UI's screens: no pointer, and nothing for the program.
        let mut stick = stick_mouse();
        let mut state = tilted();
        assert_eq!(stick.frame(&mut state, StickRole::Hidden, (10, 10)), None);
        assert_eq!(
            (state.pads[0].buttons, state.pads[0].sticks[1]),
            (0, (0.0, 0.0))
        );
        // Select does not make a mouse of them there.
        stick.select.event(&pad_event(Button::Back, true));
        let mut state = tilted();
        assert_eq!(stick.frame(&mut state, StickRole::Hidden, (10, 10)), None);
        // Off: as if there were no right-stick mouse.
        let mut stick = stick_mouse();
        let mut state = tilted();
        assert_eq!(stick.frame(&mut state, StickRole::Off, (10, 10)), None);
        assert_eq!(state.pads[0].buttons, R3);
    }

    const MAIN: u32 = 3;

    /// `event` as R3's clicks for `role` make it, with the pointer at
    /// `at` in window `MAIN`.
    fn clicked(stick: &mut StickMouse, mut event: Event, role: StickRole, at: (f32, f32)) -> Event {
        stick.click(&mut event, role, MAIN, at);
        event
    }

    fn r3_at(down: bool, timestamp: u64) -> Event {
        match pad_event(Button::RightStick, down) {
            Event::GamepadButtonDown { which, button, .. } => Event::GamepadButtonDown {
                timestamp,
                which,
                button,
            },
            Event::GamepadButtonUp { which, button, .. } => Event::GamepadButtonUp {
                timestamp,
                which,
                button,
            },
            _ => unreachable!(),
        }
    }

    /// A mouse button event's kind (down or up), button, clicks, window
    /// and place.
    type Click = (bool, MouseButton, u8, u32, (f32, f32));

    fn mouse_button(event: &Event) -> Option<Click> {
        match *event {
            Event::MouseButtonDown {
                window_id,
                mouse_btn,
                clicks,
                x,
                y,
                which: STICK_MOUSE,
                ..
            } => Some((true, mouse_btn, clicks, window_id, (x, y))),
            Event::MouseButtonUp {
                window_id,
                mouse_btn,
                clicks,
                x,
                y,
                which: STICK_MOUSE,
                ..
            } => Some((false, mouse_btn, clicks, window_id, (x, y))),
            _ => None,
        }
    }

    fn motion(left: bool) -> Event {
        Event::MouseMotion {
            timestamp: 0,
            window_id: MAIN,
            which: 1,
            mousestate: MouseState::from_sdl_state(u32::from(left)),
            x: 5.0,
            y: 5.0,
            xrel: 1.0,
            yrel: 0.0,
        }
    }

    fn left_held(event: &Event) -> bool {
        matches!(event, Event::MouseMotion { mousestate, .. } if mousestate.left())
    }

    #[test]
    fn r3_clicks_for_the_emulators_screens_and_drags() {
        let mut stick = stick_mouse();
        let left = MouseButton::Left;
        let down = clicked(&mut stick, r3_at(true, 0), StickRole::Screens, (10.0, 20.0));
        assert_eq!(
            mouse_button(&down),
            Some((true, left, 1, MAIN, (10.0, 20.0)))
        );
        // The pointer moves with the button held...
        assert!(left_held(&clicked(
            &mut stick,
            motion(false),
            StickRole::Screens,
            (0.0, 0.0)
        )));
        // ...and its release goes where the press went, wherever the
        // pointer is and whatever has the stick by then.
        let up = clicked(&mut stick, r3_at(false, 0), NO_CURSOR, (40.0, 20.0));
        assert_eq!(
            mouse_button(&up),
            Some((false, left, 1, MAIN, (40.0, 20.0)))
        );
        assert!(!left_held(&clicked(
            &mut stick,
            motion(false),
            StickRole::Screens,
            (0.0, 0.0)
        )));
        // The program's R3 stays the program's, and its release too.
        let mut stick = stick_mouse();
        for role in [CURSOR, NO_CURSOR, StickRole::Hidden, StickRole::Off] {
            let down = clicked(&mut stick, r3_at(true, 0), role, (0.0, 0.0));
            assert!(matches!(down, Event::GamepadButtonDown { .. }), "{role:?}");
            let up = clicked(&mut stick, r3_at(false, 0), StickRole::Screens, (0.0, 0.0));
            assert!(matches!(up, Event::GamepadButtonUp { .. }), "{role:?}");
        }
        // Other buttons are left alone.
        let a = clicked(
            &mut stick,
            pad_event(Button::South, true),
            StickRole::Screens,
            (0.0, 0.0),
        );
        assert!(matches!(
            a,
            Event::GamepadButtonDown {
                button: Button::South,
                ..
            }
        ));
        // With the setting off, R3 clicks nothing, but a press made
        // before still gets its release.
        stick.on = false;
        let down = clicked(&mut stick, r3_at(true, 0), StickRole::Screens, (0.0, 0.0));
        assert!(matches!(down, Event::GamepadButtonDown { .. }));
        let mut stick = stick_mouse();
        clicked(&mut stick, r3_at(true, 0), StickRole::Screens, (0.0, 0.0));
        stick.on = false;
        let up = clicked(&mut stick, r3_at(false, 0), StickRole::Screens, (0.0, 0.0));
        assert!(mouse_button(&up).is_some_and(|(down, ..)| !down));
    }

    #[test]
    fn two_quick_presses_of_r3_in_one_place_double_click() {
        let mut stick = stick_mouse();
        let ms = 1_000_000;
        let mut press = |t: u64, at: (f32, f32)| {
            let down = clicked(&mut stick, r3_at(true, t), StickRole::Screens, at);
            clicked(
                &mut stick,
                r3_at(false, t + 50 * ms),
                StickRole::Screens,
                at,
            );
            mouse_button(&down).map(|(_, _, clicks, ..)| clicks)
        };
        assert_eq!(press(1000 * ms, (100.0, 100.0)), Some(1));
        assert_eq!(press(1300 * ms, (110.0, 90.0)), Some(2));
        assert_eq!(press(1600 * ms, (110.0, 90.0)), Some(3));
        // Too late, or too far.
        assert_eq!(press(2200 * ms, (110.0, 90.0)), Some(1));
        assert_eq!(press(2300 * ms, (150.0, 90.0)), Some(1));
    }

    #[test]
    fn a_controller_gone_with_r3_down_lets_go_of_the_button() {
        let mut stick = stick_mouse();
        clicked(&mut stick, r3_at(true, 0), StickRole::Screens, (0.0, 0.0));
        let gone = Event::GamepadRemoved {
            timestamp: 0,
            which: 7.into(),
        };
        clicked(&mut stick, gone, StickRole::Screens, (0.0, 0.0));
        assert!(!left_held(&clicked(
            &mut stick,
            motion(false),
            StickRole::Screens,
            (0.0, 0.0)
        )));
        let down = clicked(&mut stick, r3_at(true, 0), StickRole::Screens, (0.0, 0.0));
        assert!(mouse_button(&down).is_some());
    }
}

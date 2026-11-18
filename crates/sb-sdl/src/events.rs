//! What each of the emulator's own screens does with an event. The
//! frontend's loop gives an event to the screen that is open, in the
//! order they lie over each other: a dialog first, then the welcome
//! screen, and so on down to the program. A screen that takes the event
//! ends its handling.

use sb_runtime::machine::Machine;
use sdl3::event::Event;
use sdl3::mouse::MouseButton;

use crate::dialog::Answer;
use crate::simple_ui::Welcomed;
use crate::toast::Toasts;
use crate::{Asking, Screens, load_edited, run_from_list, save_edited};

/// What the loop does once a screen has taken an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taken {
    /// Goes on to the next event.
    Next,
    /// Leaves the loop: the emulator quits.
    Quit,
}

/// The open dialog's: it takes all input. `quit` is set when the window
/// is closed under a dialog that does not ask about quitting, for the
/// loop to ask then. `to_display` gives the display's pixel under a
/// window position.
pub fn dialog(
    event: &Event,
    screens: &mut Screens,
    machine: &mut Machine,
    toasts: &mut Toasts,
    ran_from_list: &mut Option<String>,
    quit: &mut bool,
    to_display: impl Fn(f32, f32) -> (i32, i32),
) -> Taken {
    let Some((open, asking)) = &mut screens.dialog else {
        return Taken::Next;
    };
    let answer = match *event {
        Event::Quit { .. } if *asking == Asking::Quit => return Taken::Quit,
        Event::Quit { .. } => {
            *quit = true;
            None
        }
        Event::MouseMotion { x, y, .. } => {
            open.mouse_move(to_display(x, y));
            None
        }
        Event::MouseButtonDown {
            mouse_btn: MouseButton::Left,
            x,
            y,
            ..
        } => open.click(to_display(x, y)),
        Event::KeyDown {
            keycode: Some(key), ..
        } => open.key(key),
        Event::GamepadButtonDown { button, .. } => open.pad(button),
        Event::TextInput { ref text, .. } => {
            open.text(text);
            None
        }
        _ => None,
    };
    match (answer, asking.clone()) {
        (Some(Answer::Accept), Asking::Quit) => return Taken::Quit,
        (Some(answer), Asking::Command(_)) => {
            machine.answer(answer == Answer::Accept);
            screens.dialog = None;
        }
        (Some(Answer::Accept), Asking::Overwrite(name)) => {
            save_edited(machine, &name, toasts);
            screens.dialog = None;
        }
        (Some(Answer::Accept), Asking::Discard(name)) => {
            load_edited(machine, &name, toasts);
            screens.dialog = None;
        }
        (Some(Answer::Accept), Asking::Run(project, file)) => {
            screens.dialog = None;
            if run_from_list(machine, &project, &file, toasts) {
                (screens.game_list, *ran_from_list) = (None, Some(file));
            }
        }
        (Some(Answer::Accept), Asking::Condition(slot, line)) => {
            match machine.set_condition(slot, line, open.field()) {
                Ok(()) => screens.dialog = None,
                Err(e) => toasts.push(format!("Condition: {e}")),
            }
        }
        (Some(_), Asking::Notice) | (Some(Answer::Cancel), _) => screens.dialog = None,
        (None, _) => {}
    }
    Taken::Next
}

/// The welcome screen's. It takes the button pressed for A as A, by its
/// place, and keeps that and that it was shown.
pub fn welcome(event: &Event, screens: &mut Screens, machine: &mut Machine, quit: &mut bool) {
    let Some(open) = &mut screens.welcome else {
        return;
    };
    let swap = machine.rt.settings.swap_ab_xy;
    let welcomed = match *event {
        Event::Quit { .. } => {
            *quit = true;
            Welcomed::Stay
        }
        Event::KeyDown {
            keycode: Some(key),
            repeat: false,
            ..
        } => open.key(key),
        Event::GamepadButtonDown { button, .. } => {
            open.pad_down(button, swap);
            Welcomed::Stay
        }
        Event::GamepadButtonUp { button, .. } => open.pad_up(button, swap),
        _ => Welcomed::Stay,
    };
    if let Welcomed::Continue(chosen) = welcomed {
        screens.welcome = None;
        let settings = &mut machine.rt.settings;
        settings.simple_ui_welcome_shown = true;
        if let Some(swap) = chosen {
            settings.swap_ab_xy = swap;
        }
    }
}

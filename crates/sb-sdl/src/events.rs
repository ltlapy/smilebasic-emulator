//! What each of the emulator's own screens does with an event. The
//! frontend's loop gives an event to the screen that is open, in the
//! order they lie over each other: a dialog first, then the welcome
//! screen, and so on down to the program. A screen that takes the event
//! ends its handling.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use sb_runtime::machine::Machine;
use sdl3::event::Event;
use sdl3::mouse::MouseButton;
use sdl3::video::Window;

use crate::dialog::{Answer, Dialog};
use crate::projects::{Memory, Outcome, Transfer};
use crate::simple_ui::{self, Welcomed};
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

/// What every screen's handling is given besides its own state.
pub struct Ctx<'a> {
    pub machine: &'a mut Machine,
    pub toasts: &'a mut Toasts,
    /// Set when the window is closed while a screen has the input, for
    /// the loop to ask about quitting once the events are handled.
    pub quit: &'a mut bool,
    /// The display's size in the emulator's UI pixels.
    pub size: (u32, u32),
}

/// The open dialog's: it takes all input. `to_display` gives the
/// display's pixel under a window position.
pub fn dialog(
    event: &Event,
    screens: &mut Screens,
    ran_from_list: &mut Option<String>,
    cx: Ctx,
    to_display: impl Fn(f32, f32) -> (i32, i32),
) -> Taken {
    let Ctx {
        machine,
        toasts,
        quit,
        ..
    } = cx;
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
pub fn welcome(event: &Event, screens: &mut Screens, cx: Ctx) {
    let Ctx { machine, quit, .. } = cx;
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

/// The Simple UI's game list's: it takes all input but a dialog's.
/// `memory` keeps where the list was when a program is run from it, and
/// `ran` the program, for Restart.
pub fn game_list(
    event: &Event,
    screens: &mut Screens,
    memory: &mut simple_ui::Memory,
    ran: &mut Option<String>,
    cx: Ctx,
) {
    let Ctx {
        machine,
        toasts,
        quit,
        size,
    } = cx;
    let Some(open) = &mut screens.game_list else {
        return;
    };
    let storage = &machine.rt.storage;
    let outcome = match *event {
        Event::Quit { .. } => {
            *quit = true;
            simple_ui::Outcome::Stay
        }
        Event::KeyDown {
            keycode: Some(key), ..
        } => open.key(key, storage),
        Event::GamepadButtonDown { button, .. } => open.pad(button, storage),
        _ => simple_ui::Outcome::Stay,
    };
    match outcome {
        simple_ui::Outcome::Stay => {}
        simple_ui::Outcome::Notice(text) => toasts.push(text),
        simple_ui::Outcome::Run { project, file } => {
            *memory = open.memory();
            let unsaved = machine.unsaved_slots();
            if !unsaved.is_empty() {
                let label = sb_runtime::confirm::project_label(&project);
                let mut lines = vec![format!("Run {file} of {label}."), String::new()];
                lines.extend(sb_runtime::confirm::unsaved_lines(&unsaved));
                let ask = Dialog::new("Unsaved programs", lines, "Run", size);
                screens.dialog = Some((ask, Asking::Run(project, file)));
            } else if run_from_list(machine, &project, &file, toasts) {
                (screens.game_list, *ran) = (None, Some(file));
            }
        }
    }
}

/// The projects': they take all input but a dialog's. `memory` keeps
/// where they were when Run leaves them, for the return; a file or a
/// folder chosen in the system's dialog over `window` comes back through
/// `chosen`.
pub fn manager(
    event: &Event,
    screens: &mut Screens,
    memory: &mut Memory,
    chosen: &Sender<(PathBuf, Transfer)>,
    window: &Window,
    cx: Ctx,
    to_display: impl Fn(f32, f32) -> (i32, i32),
) {
    let Ctx {
        machine,
        toasts,
        quit,
        ..
    } = cx;
    let Some(open) = &mut screens.manager else {
        return;
    };
    // After a key, a click or a button, the lists show what other
    // programs changed meanwhile, for the next one to act on.
    let input = matches!(
        event,
        Event::KeyDown { .. } | Event::MouseButtonDown { .. } | Event::GamepadButtonDown { .. }
    );
    let unsaved = machine.unsaved_slots();
    let storage = &mut machine.rt.storage;
    let outcome = match *event {
        Event::Quit { .. } => {
            *quit = true;
            Outcome::Stay
        }
        Event::MouseMotion { x, y, .. } => {
            open.mouse_move(to_display(x, y));
            Outcome::Stay
        }
        Event::MouseButtonDown {
            mouse_btn: MouseButton::Left,
            clicks,
            x,
            y,
            ..
        } => match clicks {
            2.. => open.double_click(to_display(x, y), storage, &unsaved),
            _ => open.click(to_display(x, y), storage, &unsaved),
        },
        Event::MouseButtonDown {
            mouse_btn: MouseButton::Right,
            x,
            y,
            ..
        } => {
            open.right_click(to_display(x, y), storage);
            Outcome::Stay
        }
        Event::MouseButtonUp {
            mouse_btn: MouseButton::Left,
            x,
            y,
            ..
        } => {
            open.release(to_display(x, y), storage);
            Outcome::Stay
        }
        Event::MouseWheel { y, .. } => {
            open.wheel(-y.round() as i32);
            Outcome::Stay
        }
        Event::KeyDown {
            keycode: Some(key), ..
        } => open.key(key, storage, &unsaved),
        Event::GamepadButtonDown { button, .. } => open.pad(button, storage, &unsaved),
        Event::TextInput { ref text, .. } => {
            open.text(text);
            Outcome::Stay
        }
        _ => Outcome::Stay,
    };
    match outcome {
        Outcome::Stay => {}
        Outcome::Close => screens.manager = None,
        Outcome::Switched(name) => {
            toasts.push(format!("Switched to project {name}"));
            screens.manager = None;
        }
        Outcome::Open { switched, open } => {
            if let Some(name) = switched {
                toasts.push(format!("Switched to project {name}"));
            }
            if let (true, Some(m)) = (open.home, &screens.manager) {
                *memory = m.memory();
            }
            let done = match (open.run, open.home) {
                (true, true) => machine.run_home(&open.file),
                (true, false) => machine.run_file(&open.file),
                (false, _) => machine.load_slot(open.slot, &open.file),
            };
            match done {
                Ok(()) if !open.run => {
                    toasts.push(format!("Loaded {} into PRG{}", open.file, open.slot))
                }
                Ok(()) => {}
                Err(e) => toasts.push(format!("Could not load {}: {e}", open.file)),
            }
            screens.manager = None;
        }
        Outcome::ChooseFile(transfer, start) => {
            let send = chosen.clone();
            let filters = [
                sdl3::dialog::DialogFileFilter {
                    name: "Images",
                    pattern: "png;jpg;jpeg",
                },
                sdl3::dialog::DialogFileFilter {
                    name: "All files (GRP too)",
                    pattern: "*",
                },
            ];
            let shown = sdl3::dialog::show_open_file_dialog(
                &filters,
                start.as_deref(),
                false,
                window,
                Box::new(move |picked, _| {
                    if let Some(path) = picked.ok().and_then(|p| p.into_iter().next()) {
                        let _ = send.send((path, transfer.clone()));
                    }
                }),
            );
            if let Err(e) = shown {
                toasts.push(format!("Could not open a file dialog: {e}"));
            }
        }
        Outcome::ChooseFolder(transfer) => {
            let send = chosen.clone();
            sdl3::dialog::show_open_folder_dialog(
                None::<&Path>,
                false,
                window,
                Box::new(move |picked, _| {
                    if let Some(path) = picked.ok().and_then(|p| p.into_iter().next()) {
                        let _ = send.send((path, transfer.clone()));
                    }
                }),
            );
        }
    }
    if let (true, Some(open)) = (input, &mut screens.manager) {
        open.reread(&machine.rt.storage, false);
    }
}

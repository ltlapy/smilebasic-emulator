//! What each of the emulator's own screens does with an event. The
//! frontend's loop gives an event to the screen that is open, in the
//! order they lie over each other: a dialog first, then the welcome
//! screen, and so on down to the program. A screen that takes the event
//! ends its handling.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Sender;

use sb_runtime::machine::Machine;
use sb_runtime::sound::bank::Bank;
use sdl3::event::Event;
use sdl3::mouse::MouseButton;
use sdl3::video::Window;

use crate::dialog::{Answer, Dialog};
use crate::file_dialogs::FilePicker;
use crate::projects::{Memory, Outcome, Transfer};
use crate::simple_ui::{self, GameList, Welcomed};
use crate::toast::Toasts;
use crate::{
    Asking, FileDialog, Screens, choose_extra_font, choose_sound_font, colors, file_dialogs,
    load_edited, run_from_list, save_edited, settings_window, shortcuts, simple_ui_leave,
    use_instruments,
};

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

/// The system's file dialogs, for the screens that open them: the window
/// they open over, and where each one's choice comes back.
pub struct Choosers<'a> {
    pub window: &'a Window,
    /// A SoundFont, and the editor's extra font.
    pub font: &'a Sender<PathBuf>,
    pub extra_font: &'a Sender<PathBuf>,
    /// The editor colors' file, with whether to save there.
    pub colors: &'a Sender<(PathBuf, bool)>,
    /// The projects' file or folder, for importing and exporting.
    pub folder: &'a Sender<(PathBuf, Transfer)>,
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
/// where they were when Run leaves them, for the return.
pub fn manager(
    event: &Event,
    screens: &mut Screens,
    memory: &mut Memory,
    choosers: Choosers,
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
            let send = choosers.folder.clone();
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
                choosers.window,
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
            let send = choosers.folder.clone();
            sdl3::dialog::show_open_folder_dialog(
                None::<&Path>,
                false,
                choosers.window,
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

/// The windows of Ctrl+S and Ctrl+L: they take the event.
pub fn file_dialog(
    event: Event,
    screens: &mut Screens,
    cx: Ctx,
    to_display: impl Fn(f32, f32) -> (i32, i32),
) {
    let Ctx {
        machine,
        toasts,
        quit,
        size,
    } = cx;
    let Some(open) = &mut screens.file_dialog else {
        return;
    };
    // After a key, a click or a button, the list shows what other
    // programs changed meanwhile, for the next one to act on.
    let input = matches!(
        event,
        Event::KeyDown { .. } | Event::MouseButtonDown { .. } | Event::GamepadButtonDown { .. }
    );
    let storage = &machine.rt.storage;
    let outcome = match (open, event) {
        (_, Event::Quit { .. }) => {
            *quit = true;
            file_dialogs::Outcome::Stay
        }
        (FileDialog::Load(picker), Event::MouseMotion { x, y, .. }) => {
            picker.mouse_move(to_display(x, y));
            file_dialogs::Outcome::Stay
        }
        (FileDialog::Load(picker), Event::MouseWheel { y, .. }) => {
            picker.wheel(-y.round() as i32);
            file_dialogs::Outcome::Stay
        }
        (
            open,
            Event::MouseButtonDown {
                mouse_btn: MouseButton::Left,
                x,
                y,
                ..
            },
        ) => match open {
            FileDialog::Save(d) => d.click(to_display(x, y)),
            FileDialog::Load(p) => p.click(to_display(x, y), storage),
        },
        (
            open,
            Event::KeyDown {
                keycode: Some(key), ..
            },
        ) => match open {
            FileDialog::Save(d) => d.key(key),
            FileDialog::Load(p) => p.key(key, storage),
        },
        (FileDialog::Save(d), Event::TextInput { text, .. }) => {
            d.text(&text);
            file_dialogs::Outcome::Stay
        }
        (FileDialog::Load(p), Event::TextInput { text, .. }) => {
            p.text(&text);
            file_dialogs::Outcome::Stay
        }
        _ => file_dialogs::Outcome::Stay,
    };
    let slot = machine.edited_slot();
    match outcome {
        file_dialogs::Outcome::Stay => {}
        file_dialogs::Outcome::Close => screens.file_dialog = None,
        file_dialogs::Outcome::Save(name) => {
            screens.file_dialog = None;
            let own = machine.rt.slot_names[slot] == name;
            // A file of the folder the program was run from
            // asks the first time, even when it is the slot's.
            let originals = machine.rt.original_txt(&name);
            if (!own && machine.rt.txt_exists(&name)) || !originals.is_empty() {
                let mut lines = vec![
                    format!("Save slot {slot} as {name}."),
                    "The file there now will be replaced.".to_string(),
                ];
                if !originals.is_empty() {
                    lines.push(String::new());
                    lines.extend(sb_runtime::confirm::original_lines(&originals));
                }
                let open = Dialog::new("Save file", lines, "Save", size);
                screens.dialog = Some((open, Asking::Overwrite(name)));
            } else {
                save_edited(machine, &name, toasts);
            }
        }
        // Only the projects window copies.
        file_dialogs::Outcome::Copy { .. } => screens.file_dialog = None,
        file_dialogs::Outcome::Tool(n, file) => {
            screens.file_dialog = None;
            machine.rt.settings.tools[n] = file;
        }
        file_dialogs::Outcome::Load(spec) => {
            screens.file_dialog = None;
            if machine.unsaved_slots().contains(&slot) {
                let lines = vec![
                    format!("Load {spec} into slot {slot}."),
                    "Its unsaved changes will be lost.".to_string(),
                ];
                let open = Dialog::new("Load file", lines, "Load", size);
                screens.dialog = Some((open, Asking::Discard(spec)));
            } else {
                load_edited(machine, &spec, toasts);
            }
        }
    }
    if let (true, Some(FileDialog::Load(picker))) = (input, &mut screens.file_dialog) {
        picker.reread(&machine.rt.storage, false);
    }
}

/// The window of the keyboard shortcuts: it takes the event.
pub fn shortcuts_window(
    event: Event,
    screens: &mut Screens,
    cx: Ctx,
    to_display: impl Fn(f32, f32) -> (i32, i32),
) {
    let Some(open) = &mut screens.shortcuts_window else {
        return;
    };
    let quit = cx.quit;
    let outcome = match event {
        Event::Quit { .. } => {
            *quit = true;
            shortcuts::Outcome::Stay
        }
        Event::KeyDown {
            keycode: Some(key), ..
        } => open.key(key),
        Event::MouseWheel { y, .. } => {
            open.wheel(-y.round() as i32);
            shortcuts::Outcome::Stay
        }
        Event::MouseButtonDown {
            mouse_btn: MouseButton::Left,
            x,
            y,
            ..
        } => open.click(to_display(x, y)),
        _ => shortcuts::Outcome::Stay,
    };
    if outcome == shortcuts::Outcome::Close {
        screens.shortcuts_window = None;
    }
}

/// The settings window's: it takes the event. `bank` is SmileBASIC 4's
/// instruments, if they are imported.
pub fn settings_window(
    event: Event,
    screens: &mut Screens,
    bank: Option<&Arc<Bank>>,
    choosers: Choosers,
    cx: Ctx,
    to_display: impl Fn(f32, f32) -> (i32, i32),
) {
    let Ctx {
        machine,
        toasts,
        quit,
        size,
    } = cx;
    let Some(open) = &mut screens.settings_window else {
        return;
    };
    let settings = &mut machine.rt.settings;
    let outcome = match event {
        Event::Quit { .. } => {
            *quit = true;
            settings_window::Outcome::Stay
        }
        Event::MouseMotion { x, y, .. } => {
            open.mouse_move(to_display(x, y));
            settings_window::Outcome::Stay
        }
        Event::MouseButtonDown {
            mouse_btn: MouseButton::Left,
            x,
            y,
            ..
        } => open.click(to_display(x, y), settings),
        Event::KeyDown {
            keycode: Some(key), ..
        } => open.key(key, settings),
        _ => settings_window::Outcome::Stay,
    };
    match outcome {
        settings_window::Outcome::Stay => {}
        settings_window::Outcome::Close => screens.settings_window = None,
        settings_window::Outcome::Instruments => {
            if machine.rt.settings.smilebasic_instruments && bank.is_none() {
                toasts.push(
                    "SmileBASIC 4's instruments are not imported \
                     (see smilebasic-import)"
                        .into(),
                );
            }
            use_instruments(machine, bank, toasts);
        }
        settings_window::Outcome::ChooseSoundFont => {
            choose_sound_font(choosers.window, choosers.font.clone(), toasts);
        }
        settings_window::Outcome::ExtraFont => {
            if let Err(e) = machine.rt.load_extra_font() {
                toasts.push(e);
            }
        }
        settings_window::Outcome::ProgramFont => machine.rt.apply_program_font(),
        settings_window::Outcome::BundledGlyphs => machine.apply_bundled_glyphs(),
        settings_window::Outcome::ChooseExtraFont => {
            let send = choosers.extra_font.clone();
            choose_extra_font(choosers.window, send, toasts);
        }
        // The picker goes over the settings, which stay open.
        settings_window::Outcome::ChooseTool(n) => {
            let storage = &machine.rt.storage;
            let now = &machine.rt.settings.tools[n];
            let picker = FilePicker::tool(size, n, now, storage);
            screens.file_dialog = Some(FileDialog::Load(picker));
        }
    }
}

/// The editor colors window's: it takes the event.
pub fn color_window(
    event: Event,
    screens: &mut Screens,
    choosers: Choosers,
    cx: Ctx,
    to_display: impl Fn(f32, f32) -> (i32, i32),
) {
    let Ctx {
        machine,
        toasts,
        quit,
        ..
    } = cx;
    let Some(open) = &mut screens.color_window else {
        return;
    };
    let palette = &mut machine.rt.settings.editor_colors;
    let outcome = match event {
        Event::Quit { .. } => {
            *quit = true;
            colors::Outcome::Stay
        }
        Event::MouseMotion { x, y, .. } => {
            open.mouse_move(to_display(x, y));
            colors::Outcome::Stay
        }
        Event::MouseButtonDown {
            mouse_btn: MouseButton::Left,
            x,
            y,
            ..
        } => open.click(to_display(x, y), palette),
        Event::KeyDown {
            keycode: Some(key), ..
        } => open.key(key),
        Event::TextInput { text, .. } => {
            open.text(&text, palette);
            colors::Outcome::Stay
        }
        _ => colors::Outcome::Stay,
    };
    match outcome {
        colors::Outcome::Stay => {}
        colors::Outcome::Close => screens.color_window = None,
        colors::Outcome::Import | colors::Outcome::Export => {
            let save = outcome == colors::Outcome::Export;
            let send = choosers.colors.clone();
            let filters = [sdl3::dialog::DialogFileFilter {
                name: "Editor colors",
                pattern: "txt",
            }];
            let callback: sdl3::dialog::DialogCallback = Box::new(move |chosen, _| {
                if let Some(path) = chosen.ok().and_then(|p| p.into_iter().next()) {
                    let _ = send.send((path, save));
                }
            });
            let window = choosers.window;
            let shown = if save {
                sdl3::dialog::show_save_file_dialog(
                    &filters,
                    Some("editor-colors.txt"),
                    window,
                    callback,
                )
            } else {
                sdl3::dialog::show_open_file_dialog(
                    &filters,
                    None::<&Path>,
                    false,
                    window,
                    callback,
                )
            };
            if let Err(e) = shown {
                toasts.push(format!("Could not open a file dialog: {e}"));
            }
        }
    }
}

/// The Simple UI's pause menu's: it takes the event. `memory` is where
/// the game list was left, and `ran` the program it ran last, which
/// Restart runs again.
pub fn pause_menu(
    event: Event,
    screens: &mut Screens,
    memory: &simple_ui::Memory,
    ran: &Option<String>,
    cx: Ctx,
) {
    let Ctx {
        machine,
        toasts,
        quit,
        size,
    } = cx;
    let Some(open) = &mut screens.pause_menu else {
        return;
    };
    let chosen = match event {
        Event::Quit { .. } => Some(simple_ui::Choice::Quit),
        Event::KeyDown {
            keycode: Some(key), ..
        } => open.key(key),
        Event::GamepadButtonDown { button, .. } => open.pad(button),
        _ => None,
    };
    // Shown at once, behind the menu, and kept. A button held
    // as the swap changes reads as the other from the next
    // event on, and the program sees neither: the menu has
    // the controls.
    let settings = open.settings();
    match chosen {
        Some(simple_ui::Choice::Resume) => screens.pause_menu = None,
        Some(
            simple_ui::Choice::Filter
            | simple_ui::Choice::UiSize
            | simple_ui::Choice::Swap
            | simple_ui::Choice::FrameSkip
            | simple_ui::Choice::Vsync,
        ) => settings.apply(&mut machine.rt.settings),
        Some(simple_ui::Choice::Restart) => {
            screens.pause_menu = None;
            if let Some(file) = ran {
                simple_ui_leave(machine);
                if let Err(e) = machine.run_home(file) {
                    toasts.push(format!("Could not load {file}: {e}"));
                }
            }
        }
        Some(simple_ui::Choice::List) => {
            screens.pause_menu = None;
            simple_ui_leave(machine);
            screens.game_list = Some(GameList::new(&machine.rt.storage, size, memory));
        }
        Some(simple_ui::Choice::Quit) => {
            screens.pause_menu = None;
            *quit = true;
        }
        None => {}
    }
}

//! The handling of an event by each of the emulator's own screens. The
//! frontend's loop gives an event to the screen that is open, in the
//! order they are stacked: a dialog first, then the welcome screen, and
//! so on down to the program. A screen that takes the event ends its
//! handling.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Sender;

use sb_runtime::editor::Mode;
use sb_runtime::help::Help;
use sb_runtime::machine::Machine;
use sb_runtime::sound::bank::Bank;
use sdl3::clipboard::ClipboardUtil;
use sdl3::event::Event;
use sdl3::gamepad::Button;
use sdl3::keyboard::Keycode;
use sdl3::mouse::MouseButton;
use sdl3::video::Window;

use crate::colors::ColorWindow;
use crate::debug_panel::{DebugPanel, Hit, Typed};
use crate::dialog::{Answer, Dialog};
use crate::file_dialogs::FilePicker;
use crate::inputs::Inputs;
use crate::menu::{Action, MenuBar};
use crate::program_dialog::ProgramView;
use crate::projects::{Memory, Outcome, ProjectManager, Transfer};
use crate::settings_window::SettingsWindow;
use crate::shortcuts::ShortcutsWindow;
use crate::simple_ui::{self, GameList, MenuSettings, PauseMenu, Welcomed};
use crate::toast::Toasts;
use crate::window::{self, KeepShape};
use crate::{
    Asking, Beside, Choice, Debugger, FileDialog, Screens, Side, about_lines, choose,
    choose_extra_font, choose_sound_font, colors, condition_dialog, copy_selection, debug_action,
    debug_view, file_dialogs, help, index, load_edited, main_room, next_editor_size,
    open_file_dialog, open_folder, open_index, paste, run_from_list, save_edited, set_value,
    set_window_scale, settings_window, shape_free, shortcuts, side_outcome, simple_ui_leave,
    toggle_fullscreen, use_instruments,
};

/// The loop's next step once a screen has taken an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taken {
    /// Continues with the next event.
    Next,
    /// Leaves the loop: the emulator quits.
    Quit,
}

/// The values that every screen's handling gets besides its own state.
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

/// The open dialog: it takes all input. `to_display` returns the display
/// pixel under a window position.
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

/// The welcome screen. It takes the button pressed for A as A, by its
/// place, and keeps that choice and the fact that it was shown.
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

/// The Simple UI's game list: it takes all input except what goes to a
/// dialog. `memory` keeps where the list was when a program is run from
/// it, and `ran` keeps the program, for Restart.
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

/// The projects: they take all input except what goes to a dialog.
/// `memory` keeps where they were when Run leaves them, for the return.
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
            // A file in the folder the program was run from asks the
            // first time, even when it is the slot's own file.
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

/// The settings window: it takes the event. `bank` is SmileBASIC 4's
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

/// The editor colors window: it takes the event.
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

/// The Simple UI's hotkeys, ahead of every screen. Select+A, or F8, opens
/// and closes the pause menu, over a dialog too; that A goes nowhere else.
/// Select+Start quits the same as the menu's Quit does, and the release
/// of that Start does not count as a release of the + button. The welcome
/// screen opens no menu. `restart` says whether the menu offers Restart.
/// Returns true when the event went to one of these.
pub fn pause_hotkeys(
    event: &Event,
    inputs: &mut Inputs,
    screens: &mut Screens,
    (simple_ui, restart): (bool, bool),
    cx: Ctx,
) -> bool {
    let Ctx {
        machine,
        quit,
        size,
        ..
    } = cx;
    let select = inputs.controls.stick_mouse.select.held();
    if simple_ui {
        let toggled = match event {
            Event::GamepadButtonDown {
                button: Button::East,
                ..
            } => inputs.hotkeys.a_down(true, select) && screens.welcome.is_none(),
            Event::GamepadButtonDown {
                button: Button::Start,
                ..
            } if inputs.hotkeys.start_down(true, select) == simple_ui::Start::Quit => {
                (screens.pause_menu, inputs.plus_down, *quit) = (None, None, true);
                return true;
            }
            Event::KeyDown {
                keycode: Some(Keycode::F8),
                repeat: false,
                ..
            } => screens.welcome.is_none(),
            _ => false,
        };
        if toggled {
            screens.pause_menu = match screens.pause_menu {
                Some(_) => None,
                None => {
                    let running = machine.is_running();
                    let program = running || machine.home_waiting();
                    let settings = MenuSettings::of(&machine.rt.settings);
                    let open = PauseMenu::new(running, program, restart, settings, size);
                    Some(open)
                }
            };
            inputs.plus_down = None;
            return true;
        }
    }
    matches!(
        event,
        Event::GamepadButtonUp {
            button: Button::Start,
            ..
        }
    ) && !inputs.hotkeys.start_up()
}

/// The Simple UI's pause menu: it takes the event. `memory` is where
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
    // Shown immediately, behind the menu, and kept. A button held as the
    // swap changes reads as the other from the next event on, and the
    // program sees neither: the menu has the controls.
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

/// A program's DIALOG, in the main window. It takes clicks on it, Enter
/// and typing, and leaves the rest to the menu bar and the stop keys.
/// Returns true if it took the event.
pub fn program_dialog(
    event: &Event,
    open: &mut ProgramView,
    machine: &mut Machine,
    to_display: impl Fn(f32, f32) -> (i32, i32),
) -> bool {
    let (taken, answer) = match *event {
        Event::MouseMotion { x, y, .. } => {
            open.mouse_move(to_display(x, y));
            (false, None)
        }
        Event::MouseButtonDown {
            mouse_btn: MouseButton::Left,
            x,
            y,
            ..
        } if open.contains(to_display(x, y)) => (true, open.click(to_display(x, y))),
        Event::KeyDown {
            keycode: Some(key), ..
        } => match open.key(key) {
            Some(answer) => (true, answer),
            None => (false, None),
        },
        Event::TextInput { ref text, .. } => (open.text(text), None),
        _ => (false, None),
    };
    machine.set_dialog_focus(open.focus());
    if let Some(answer) = answer {
        machine.answer_dialog(answer);
    }
    taken
}

/// The pointer on the help or the index beside the editor, in the window
/// that panel is in. Returns true if the wheel turned over it.
pub fn side_pointer(
    event: &Event,
    side: &mut Beside,
    to_display: impl Fn(f32, f32) -> (i32, i32),
) -> bool {
    let Some(open) = &mut side.open else {
        return false;
    };
    match *event {
        Event::MouseWheel {
            y,
            mouse_x,
            mouse_y,
            ..
        } if open.frame().contains(to_display(mouse_x, mouse_y)) => {
            open.wheel(-y.round() as i32);
            return true;
        }
        Event::MouseMotion { x, y, .. } => open.mouse_move(to_display(x, y)),
        _ => {}
    }
    false
}

/// The debugger, for an event of the window the editor is in: the wheel
/// over its panel, its menu, and typing into the panel. `places` give the
/// display pixel and the menu pixel under a window position. Returns true
/// if it took the event.
pub fn debugger(
    event: &Event,
    debugger: &mut Debugger,
    screens: &mut Screens,
    clipboard: &ClipboardUtil,
    cx: Ctx,
    places: (
        impl Fn(f32, f32) -> (i32, i32),
        impl Fn(f32, f32) -> (i32, i32),
    ),
) -> bool {
    let Ctx {
        machine,
        toasts,
        size,
        ..
    } = cx;
    let (to_display, to_menu) = places;
    if let Event::MouseWheel {
        y,
        mouse_x,
        mouse_y,
        ..
    } = *event
        && debugger.panel.contains(to_display(mouse_x, mouse_y))
    {
        let shown = debug_view(machine, &debugger.panel).scrolled();
        debugger.panel.wheel(-y.round() as i32, shown);
        return true;
    }
    if let Some(open) = &mut debugger.menu {
        match *event {
            Event::MouseMotion { x, y, .. } => open.hover(to_menu(x, y)),
            Event::MouseButtonDown {
                mouse_btn, x, y, ..
            } => {
                let chosen = (mouse_btn == MouseButton::Left)
                    .then(|| open.click(to_menu(x, y)))
                    .flatten();
                debugger.menu = None;
                if let Some(choice) = chosen {
                    let panel = &mut debugger.panel;
                    let asks = choose(choice, machine, panel, clipboard, toasts, size);
                    if asks.is_some() {
                        screens.dialog = asks;
                    }
                }
                return true;
            }
            Event::KeyDown { .. } => {
                debugger.menu = None;
                return true;
            }
            _ => {}
        }
    }
    if debugger.panel.typing() {
        match *event {
            Event::KeyDown {
                keycode: Some(key), ..
            } => {
                if let Typed::Value(name, text) = debugger.panel.key(key) {
                    set_value(machine, &debugger.panel, &name, &text, toasts);
                }
                return true;
            }
            Event::TextInput { ref text, .. } => {
                debugger.panel.text(text);
                return true;
            }
            _ => {}
        }
    }
    false
}

/// The keys of the help or the index beside the editor, while it has
/// them. Returns true if it took the event.
pub fn side_keys(event: &Event, side: &mut Beside, machine: &mut Machine) -> bool {
    let Some(open) = &mut side.open else {
        return false;
    };
    let (taken, outcome) = match (open, event) {
        (
            Side::Help(help),
            Event::KeyDown {
                keycode: Some(Keycode::F2),
                ..
            },
        ) => {
            let size = &mut machine.rt.settings.help_text_size;
            *size = if *size <= 1 { 3 } else { *size - 1 };
            help.set_scale(*size);
            (true, index::Outcome::Stay)
        }
        (
            Side::Help(help),
            Event::KeyDown {
                keycode: Some(key), ..
            },
        ) => match help.key(*key) {
            help::Outcome::Close => (true, index::Outcome::Back),
            help::Outcome::Stay => (true, index::Outcome::Stay),
        },
        (
            Side::Index(index, _),
            Event::KeyDown {
                keycode: Some(key), ..
            },
        ) => (true, index.key(*key)),
        (Side::Index(index, _), Event::TextInput { text, .. }) => (true, index.text(text)),
        (_, Event::TextInput { .. }) => (true, index::Outcome::Stay),
        _ => (false, index::Outcome::Stay),
    };
    side_outcome(&mut side.open, &mut side.focus, machine, outcome);
    taken
}

/// The clipboard, the help and the renderer name, for the actions that
/// the menu bar starts.
pub struct Tools<'a> {
    pub clipboard: &'a ClipboardUtil,
    pub help: Option<&'a Help>,
    /// As About says it.
    pub drawn_by: &'a str,
}

/// The main window, for the items of the View menu: the profile's
/// display, the helper that keeps the window's shape, and whether the
/// window is the display of a console without a desktop.
pub struct MainWindow<'a> {
    pub window: &'a mut Window,
    pub display: (u32, u32),
    pub keep_shape: &'a mut KeepShape,
    pub kms: bool,
}

/// Does what an item of the menu bar says.
pub fn menu_action(
    action: Action,
    screens: &mut Screens,
    side: &mut Beside,
    bar: &mut MenuBar,
    tools: Tools,
    main: MainWindow,
    cx: Ctx,
) {
    let Ctx {
        machine,
        toasts,
        quit,
        size,
    } = cx;
    match action {
        Action::Quit => *quit = true,
        action @ (Action::SaveSlot | Action::LoadSlot) => {
            let save = action == Action::SaveSlot;
            match open_file_dialog(machine, save, size) {
                Ok(open) => screens.file_dialog = Some(open),
                Err(refused) => screens.dialog = Some((*refused, Asking::Notice)),
            }
        }
        Action::Undo => machine.edit().undo(),
        Action::Redo => machine.edit().redo(),
        action @ (Action::Cut | Action::Copy) => {
            copy_selection(machine, tools.clipboard, toasts, action == Action::Cut)
        }
        Action::Paste => paste(machine, tools.clipboard, toasts),
        Action::SelectAll => machine.edit().select_all(),
        Action::Comment => machine.edit().comment(),
        Action::Uncomment => machine.edit().uncomment(),
        Action::Run => machine.sysreq(),
        action @ (Action::Continue
        | Action::StepInto
        | Action::StepOver
        | Action::StepOut
        | Action::RunToCursor
        | Action::JumpToCursor
        | Action::ToggleBreakpoint
        | Action::BreakpointCondition
        | Action::ClearBreakpoints
        | Action::Restart
        | Action::DebugPanel) => {
            let asks = debug_action(machine, action, toasts, size);
            if asks.is_some() {
                screens.dialog = asks;
            }
        }
        Action::Help => {
            if !machine.is_editing() {
                machine.toggle_editor();
            }
            side.show_help(machine, tools.help, toasts, size, false);
        }
        Action::Shortcuts => {
            screens.shortcuts_window = Some(ShortcutsWindow::new(size));
        }
        Action::About => {
            let lines = about_lines(tools.drawn_by);
            let about = Dialog::notice("About", lines, size);
            screens.dialog = Some((about, Asking::Notice));
        }
        Action::Projects => {
            screens.manager = Some(ProjectManager::new(&machine.rt.storage, size));
        }
        Action::OpenFolder => open_folder(&machine.rt.storage, toasts),
        Action::Stop => machine.stop(),
        Action::SwitchFocus => {
            machine.focus_sub(!machine.sub_has_focus());
        }
        Action::StopSub => machine.stop_sub(),
        Action::Pause => bar.shown = !bar.shown,
        Action::ToggleEditor => machine.toggle_editor(),
        Action::EditSlot(slot) => machine.edit_slot(slot),
        Action::Split => machine.toggle_split(),
        Action::Index => {
            if !machine.is_editing() {
                machine.toggle_editor();
            }
            side.open = Some(open_index(machine, size));
            side.focus = true;
        }
        command @ (Action::GoToLine | Action::Find | Action::Replace) => {
            if !machine.is_editing() {
                machine.toggle_editor();
            }
            let editor = machine.edit();
            match command {
                Action::GoToLine => editor.go_to_line(),
                Action::Find => editor.start_find(false),
                _ => editor.start_find(true),
            }
        }
        Action::Settings => {
            screens.settings_window = Some(SettingsWindow::new(size));
        }
        Action::EditColors => {
            screens.color_window = Some(ColorWindow::new(size));
        }
        Action::ToggleFullscreen => {
            toggle_fullscreen(main.window, &mut machine.rt.settings);
        }
        Action::WindowSize(i) => {
            let settings = &mut machine.rt.settings;
            set_window_scale(main.window, settings, main.display, window::SCALES[i].1);
        }
        Action::ToggleKeepShape => {
            let on = !machine.rt.settings.keep_16_9;
            machine.rt.settings.keep_16_9 = on;
            // Corrected immediately where the window is not 16:9.
            let free = !on || main.kms || shape_free(main.window);
            let room = main_room(main.window);
            if let Some((w, h)) = main
                .keep_shape
                .now(main.window.size(), free, main.display, room)
            {
                let _ = main.window.set_size(w, h);
            }
        }
        toggle => toggle.toggle(&mut machine.rt.settings),
    }
}

/// A click at `at` on the debugger's panel, the second of a double click
/// if `double`.
pub fn panel_click(
    at: (i32, i32),
    double: bool,
    panel: &mut DebugPanel,
    screens: &mut Screens,
    clipboard: &ClipboardUtil,
    cx: Ctx,
) {
    let Ctx {
        machine,
        toasts,
        size,
        ..
    } = cx;
    match panel.click(at, double) {
        Some(Hit::Call(i)) => {
            if let Some(call) = machine.calls().get(i) {
                machine.show_line(call.slot, call.line);
            }
        }
        Some(Hit::Close) => machine.show_panel(false),
        // A breakpoint shows its line; a double click
        // asks for its condition.
        Some(Hit::Breakpoint(i)) => {
            if let Some(&(slot, line, _)) = machine.all_breakpoints().get(i) {
                machine.show_line(slot, line);
                if double {
                    screens.dialog = Some(condition_dialog(machine, slot, line, size));
                }
            }
        }
        Some(Hit::Variable(i)) if double => {
            let _ = choose(
                Choice::ChangeValue(i),
                machine,
                panel,
                clipboard,
                toasts,
                size,
            );
        }
        _ => {}
    }
}

/// Returns the menu bar item that a key with Ctrl in the editor stands
/// for, if any: the key does what the item does. `shift` is whether
/// Shift is held too.
pub fn editor_ctrl_action(key: Keycode, shift: bool) -> Option<Action> {
    Some(match key {
        Keycode::W => Action::ToggleWrap,
        Keycode::A => Action::SelectAll,
        Keycode::C => Action::Copy,
        Keycode::X => Action::Cut,
        Keycode::V => Action::Paste,
        Keycode::Z if shift => Action::Redo,
        Keycode::Z => Action::Undo,
        Keycode::Y => Action::Redo,
        Keycode::K if shift => Action::Uncomment,
        Keycode::K => Action::Comment,
        Keycode::G => Action::GoToLine,
        Keycode::Slash | Keycode::KpDivide => Action::Split,
        Keycode::_0 | Keycode::Kp0 => Action::ToggleEditor,
        Keycode::J => Action::Index,
        Keycode::F => Action::Find,
        Keycode::H => Action::Replace,
        _ => return None,
    })
}

/// A key with Ctrl in the editor that is not an item of the menu bar (see
/// [`editor_ctrl_action`]). `room` is the window the editor shows in, for
/// its sizes.
pub fn editor_ctrl_key(key: Keycode, room: (u32, f32), machine: &mut Machine) {
    match key {
        Keycode::B => next_editor_size(machine, room),
        Keycode::Home => machine.edit().file_top(),
        Keycode::End => machine.edit().file_end(),
        Keycode::Return | Keycode::KpEnter => {
            let editor = machine.edit();
            match editor.mode() {
                Mode::Replace => editor.replace_all(),
                _ => editor.insert_line(),
            }
        }
        Keycode::Tab => machine.switch_side(),
        Keycode::Backspace => machine.edit().delete_line(),
        Keycode::Delete => machine.edit().delete_to_end(),
        _ => {}
    }
}

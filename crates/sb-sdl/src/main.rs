#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod colors;
mod controls;
mod dialog;
mod keyhint;
mod menu;
mod program_dialog;
mod projects;
mod toast;
mod ui;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use audio::Audio;
use colors::ColorWindow;
use controls::Controls;
use dialog::{Answer, Dialog};
use keyhint::KeyHint;
use menu::{Action, Click, MenuBar};
use program_dialog::ProgramView;
use projects::{Outcome, ProjectManager, Transfer};
use sb_profiles::Profile;
use sb_runtime::editor::is_movement;
use sb_runtime::font::Font;
use sb_runtime::input::State;
use sb_runtime::machine::Machine;
use sb_runtime::palette::Palette;
use sb_runtime::runtime::keys;
use sb_runtime::screen::{Filter, ScreenMode};
use sb_runtime::settings::Settings;
use sb_runtime::sound::bank::Bank;
use sb_runtime::sound::synth::Instruments;
use sb_runtime::sprites::Sprites;
use sb_runtime::storage::Storage;
use sb_runtime::{Framebuffer, Runtime, resources, sbfile};
use sdl3::clipboard::ClipboardUtil;
use sdl3::event::Event;
use sdl3::event::WindowEvent;
use sdl3::keyboard::{Keycode, Mod};
use sdl3::messagebox::{MessageBoxFlag, show_simple_message_box};
use sdl3::mouse::MouseButton;
use sdl3::pixels::{Color, PixelFormat};
use sdl3::render::{BlendMode, Canvas, FRect, ScaleMode, Texture, TextureCreator};
use sdl3::sys::render::SDL_SetDefaultTextureScaleMode;
use sdl3::sys::surface::SDL_ScaleMode;
use sdl3::video::{Window, WindowContext};
use toast::Toasts;

const USAGE: &str = "usage:
  smilebasic                    start in direct mode
  smilebasic PROGRAM            load a SmileBASIC 4 program (TXT file or plain
                                text) into PRG0 and run it

SmileBASIC 4's own font and sprites come from smilebasic-import; without
them, a bundled font and blank sprites are used";

const TITLE: &str = "SmileBASIC emulator";
/// In the data directory.
const SETTINGS_FILE: &str = "settings.txt";
/// SmileBASIC runs at 60 frames a second.
const FRAME: Duration = Duration::from_nanos(1_000_000_000 / 60);
type AnyError = Box<dyn std::error::Error>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None => run(None),
        Some(program) if !program.starts_with('-') && args.len() == 1 => {
            run(Some(Path::new(program)))
        }
        _ => Err(USAGE.into()),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        // Release builds on Windows have no console to print to.
        let _ = show_simple_message_box(MessageBoxFlag::ERROR, TITLE, &e.to_string(), None);
        std::process::exit(1);
    }
}

fn data_dir() -> Result<PathBuf, AnyError> {
    Ok(sdl3::filesystem::get_pref_path("", resources::DATA_DIR)?)
}

fn resource_dir() -> Result<PathBuf, AnyError> {
    Ok(data_dir()?.join(resources::RESOURCE_DIR))
}

fn load_source(path: &Path) -> Result<String, AnyError> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.starts_with(&[4, 0]) {
        let file = sbfile::parse(&bytes)?;
        Ok(file.text()?.to_owned())
    } else {
        Ok(String::from_utf8(bytes).map_err(|_| "program is not UTF-8 text")?)
    }
}

fn run(program: Option<&Path>) -> Result<(), AnyError> {
    let source = program.map(load_source).transpose()?;
    let font = resources::load_font(&resource_dir()?).unwrap_or_else(|e| {
        eprintln!("using the fallback font: {e} (see `smilebasic-import`)");
        Font::fallback()
    });
    let screen = Profile::default().screen();
    let mut rt = Runtime::new(screen.width, screen.height, font);
    let (sprite_page, sprite_defs) = resources::load_sprites(&resource_dir()?);
    let bank = resources::load_instruments(&resource_dir()?).map(Arc::new);
    rt.sound
        .set_sequences(resources::load_sequences(&resource_dir()?).map(Arc::new));
    if let Some(page) = sprite_page {
        rt.set_sprite_page(page);
    }
    if let Some(defs) = sprite_defs {
        rt.sprites = Sprites::new(defs);
    }
    rt.set_startup_mode(ScreenMode {
        sample: screen.sample,
        ..ScreenMode::new(screen.width, screen.height)
    });
    rt.storage = Storage::new(data_dir()?.join("projects"));
    let settings_path = data_dir()?.join(SETTINGS_FILE);
    if let Ok(text) = std::fs::read_to_string(&settings_path) {
        rt.settings = Settings::from_text(&text);
    }
    let mut saved_settings = rt.settings.clone();
    let mut machine = Machine::new(rt);
    machine.boot();
    if let Some(source) = source {
        machine.set_slot(0, source);
        machine.run_slot(0);
    }

    let sdl = sdl3::init()?;
    let video = sdl.video()?;
    let (display_w, display_h) = screen.display;
    let window = video
        .window(TITLE, display_w, display_h)
        .position_centered()
        .resizable()
        .high_pixel_density()
        .build()?;

    video.text_input().start(&window);
    let clipboard = video.clipboard();
    machine.rt.clipboard = Box::new(SystemClipboard(video.clipboard()));
    let keyboard = sdl.keyboard();
    let mut controls = Controls::new(&sdl)?;
    let audio = Audio::open(&sdl);
    // The file dialog answers from its own thread.
    let (chosen_font, font_choice) = std::sync::mpsc::channel::<PathBuf>();
    // ...as do the editor colors window's, with whether to save there.
    let (chosen_colors, colors_choice) = std::sync::mpsc::channel::<(PathBuf, bool)>();
    // ...and the project manager's, for importing and exporting.
    let (chosen_folder, folder_choice) = std::sync::mpsc::channel::<(PathBuf, Transfer)>();
    let mut canvas = window.into_canvas();
    // Vsync paces frames on a 60 Hz display; on faster ones, or without
    // vsync, the loop waits for the next frame's time itself.
    // SAFETY: the renderer pointer is valid for the lifetime of `canvas`.
    if !unsafe { sdl3::sys::render::SDL_SetRenderVSync(canvas.raw(), 1) } {
        eprintln!("vsync unavailable: {}", sdl3::get_error());
    }
    let texture_creator = canvas.texture_creator();
    let mut mode = machine.screen_mode();
    let mut filter = machine.rt.settings.screen_filter.unwrap_or(mode.filter);
    // The frame's size and filter, which the texture is made for.
    let mut texture_kind = (mode.frame(), filter);
    let mut texture = frame_texture(&canvas, &texture_creator, texture_kind)?;
    // The pixels that mouse positions are given in: the screen's, or the
    // editor's frame.
    let mut pointer_space = (mode.width, mode.height);
    // The menu bar and other emulator UI, drawn at display resolution.
    let mut overlay = Framebuffer::new(display_w, display_h);
    let mut overlay_texture =
        texture_creator.create_texture_streaming(PixelFormat::ARGB8888, display_w, display_h)?;
    overlay_texture.set_scale_mode(ScaleMode::Nearest);
    overlay_texture.set_blend_mode(BlendMode::Blend);
    let mut menu = MenuBar::default();
    let mouse = sdl.mouse();
    let mut cursor_shown = true;
    // A question, which takes all input while it is open: before quitting
    // with unsaved programs, or for a command such as LOAD.
    let mut dialog: Option<(Dialog, Asking)> = None;
    // A program's DIALOG. It takes clicks on it, Enter and typing, but
    // leaves the menu bar and the stop keys working.
    let mut program_dialog: Option<ProgramView> = None;
    let mut blink = 0u32;
    let mut toasts = Toasts::default();
    let mut key_hint = KeyHint::default();
    use_instruments(&mut machine, bank.as_ref(), &mut toasts);
    // Takes all input while it is open, except for a dialog over it.
    let mut manager: Option<ProjectManager> = None;
    let mut color_window: Option<ColorWindow> = None;

    let mut events = sdl.event_pump()?;
    let mut next_frame = Instant::now();
    // The left button went down on the editor and is still held.
    let mut dragging = false;
    'running: loop {
        let (out_w, out_h) = canvas.output_size()?;
        let (win_w, _) = canvas.window().size();
        let display = display_rect(display_w, display_h, out_w, out_h);
        // Mouse positions come in window coordinates; the display is in pixels.
        let to_display = |x: f32, y: f32| {
            let px = out_w as f32 / win_w.max(1) as f32;
            let scale = display.w / display_w as f32;
            (
                ((x * px - display.x) / scale) as i32,
                ((y * px - display.y) / scale) as i32,
            )
        };
        // ...and the editor's pixels, which fill the screen's rectangle.
        let shown = screen_rect(&mode, filter, out_w, out_h);
        let to_screen = |x: f32, y: f32| {
            let px = out_w as f32 / win_w.max(1) as f32;
            (
                ((x * px - shown.x) * pointer_space.0 as f32 / shown.w).floor() as i32,
                ((y * px - shown.y) * pointer_space.1 as f32 / shown.h).floor() as i32,
            )
        };
        let shift = |keymod: Mod| keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
        let ctrl = |keymod: Mod| keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD);
        let mut quit = false;
        for event in events.poll_iter() {
            controls.event(&event);
            match event {
                Event::MouseMotion { x, y, .. } => {
                    let (sx, sy) = to_screen(x, y);
                    let m = &mut controls.mouse;
                    (m.x, m.y, m.display) = (sx, sy, to_display(x, y));
                    m.on_screen = (0..mode.width as i32).contains(&sx)
                        && (0..mode.height as i32).contains(&sy);
                }
                Event::MouseWheel { y, .. } if manager.is_none() => {
                    controls.mouse.wheel += y.round() as i32;
                }
                _ => {}
            }
            if let Some((open, asking)) = &mut dialog {
                let answer = match event {
                    Event::Quit { .. } if *asking == Asking::Quit => break 'running,
                    Event::Quit { .. } => {
                        quit = true;
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
                    _ => None,
                };
                match (answer, *asking) {
                    (Some(Answer::Accept), Asking::Quit) => break 'running,
                    (Some(answer), Asking::Command) => {
                        machine.rt.answer(answer == Answer::Accept);
                        dialog = None;
                    }
                    (Some(Answer::Cancel), Asking::Quit) => dialog = None,
                    (None, _) => {}
                }
                continue;
            }
            if let Some(open) = &mut program_dialog {
                let (taken, answer) = match event {
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
                if let Some(answer) = answer {
                    machine.rt.answer_dialog(answer);
                }
                if taken {
                    continue;
                }
            }
            if let Some(open) = &mut manager {
                let unsaved = machine.unsaved_slots();
                let storage = &mut machine.rt.storage;
                let outcome = match event {
                    Event::Quit { .. } => {
                        quit = true;
                        Outcome::Stay
                    }
                    Event::MouseMotion { x, y, .. } => {
                        open.mouse_move(to_display(x, y));
                        Outcome::Stay
                    }
                    Event::MouseButtonDown {
                        mouse_btn: MouseButton::Left,
                        x,
                        y,
                        ..
                    } => open.click(to_display(x, y), storage, &unsaved),
                    Event::MouseWheel { y, .. } => {
                        open.wheel(-y.round() as i32);
                        Outcome::Stay
                    }
                    Event::KeyDown {
                        keycode: Some(key), ..
                    } => open.key(key, storage, &unsaved),
                    Event::TextInput { text, .. } => {
                        open.text(&text);
                        Outcome::Stay
                    }
                    _ => Outcome::Stay,
                };
                match outcome {
                    Outcome::Stay => {}
                    Outcome::Close => manager = None,
                    Outcome::Switched(name) => {
                        toasts.push(format!("Switched to project {name}"));
                        manager = None;
                    }
                    Outcome::OpenFolder => open_folder(&machine.rt.storage, &mut toasts),
                    Outcome::Open { switched, open } => {
                        if let Some(name) = switched {
                            toasts.push(format!("Switched to project {name}"));
                        }
                        let done = match open.run {
                            true => machine.run_file(&open.file),
                            false => machine.load_slot(open.slot, &open.file),
                        };
                        match done {
                            Ok(()) if !open.run => {
                                toasts.push(format!("Loaded {} into PRG{}", open.file, open.slot))
                            }
                            Ok(()) => {}
                            Err(e) => toasts.push(format!("Could not load {}: {e}", open.file)),
                        }
                        manager = None;
                    }
                    Outcome::ChooseFolder(transfer) => {
                        let send = chosen_folder.clone();
                        sdl3::dialog::show_open_folder_dialog(
                            None::<&Path>,
                            false,
                            canvas.window(),
                            Box::new(move |chosen, _| {
                                if let Some(path) = chosen.ok().and_then(|p| p.into_iter().next()) {
                                    let _ = send.send((path, transfer.clone()));
                                }
                            }),
                        );
                    }
                }
                continue;
            }
            if let Some(open) = &mut color_window {
                let palette = &mut machine.rt.settings.editor_colors;
                let outcome = match event {
                    Event::Quit { .. } => {
                        quit = true;
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
                    colors::Outcome::Close => color_window = None,
                    colors::Outcome::Import | colors::Outcome::Export => {
                        let save = outcome == colors::Outcome::Export;
                        let send = chosen_colors.clone();
                        let filters = [sdl3::dialog::DialogFileFilter {
                            name: "Editor colors",
                            pattern: "txt",
                        }];
                        let callback: sdl3::dialog::DialogCallback = Box::new(move |chosen, _| {
                            if let Some(path) = chosen.ok().and_then(|p| p.into_iter().next()) {
                                let _ = send.send((path, save));
                            }
                        });
                        let window = canvas.window();
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
                continue;
            }
            match event {
                Event::Quit { .. } => quit = true,
                Event::MouseMotion { x, y, .. } => {
                    menu.mouse_move(Some(to_display(x, y)));
                    if dragging && machine.is_editing() {
                        let (x, y) = to_screen(x, y);
                        machine.edit().mouse_drag(x, y);
                    }
                }
                Event::MouseButtonUp {
                    mouse_btn: MouseButton::Left,
                    ..
                } => dragging = false,
                Event::Window {
                    win_event: WindowEvent::MouseLeave,
                    ..
                } => menu.mouse_move(None),
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } => match menu.click(to_display(x, y), machine.is_running()) {
                    Click::Chosen(Action::Quit) => quit = true,
                    Click::Chosen(Action::Projects) => {
                        let size = (display_w, display_h);
                        manager = Some(ProjectManager::new(&machine.rt.storage, size));
                    }
                    Click::Chosen(Action::OpenFolder) => {
                        open_folder(&machine.rt.storage, &mut toasts)
                    }
                    Click::Chosen(Action::Stop) => machine.stop(),
                    Click::Chosen(Action::Pause) => menu.shown = !menu.shown,
                    Click::Chosen(Action::ToggleEditor) => machine.toggle_editor(),
                    Click::Chosen(Action::EditSlot(slot)) => machine.edit_slot(slot),
                    Click::Chosen(Action::ChooseSoundFont) => {
                        let send = chosen_font.clone();
                        let filters = [sdl3::dialog::DialogFileFilter {
                            name: "SoundFont",
                            pattern: "sf2",
                        }];
                        let shown = sdl3::dialog::show_open_file_dialog(
                            &filters,
                            None::<&Path>,
                            false,
                            canvas.window(),
                            Box::new(move |chosen, _| {
                                if let Some(path) = chosen.ok().and_then(|p| p.into_iter().next()) {
                                    let _ = send.send(path);
                                }
                            }),
                        );
                        if let Err(e) = shown {
                            toasts.push(format!("Could not open a file dialog: {e}"));
                        }
                    }
                    Click::Chosen(Action::EditColors) => {
                        color_window = Some(ColorWindow::new((display_w, display_h)));
                    }
                    Click::Chosen(
                        choice @ (Action::SmileBasicInstruments | Action::BundledSoundFont),
                    ) => {
                        choice.toggle(&mut machine.rt.settings);
                        if choice == Action::SmileBasicInstruments && bank.is_none() {
                            toasts.push(
                                "SmileBASIC 4's instruments are not imported \
                                 (see smilebasic-import)"
                                    .into(),
                            );
                        }
                        use_instruments(&mut machine, bank.as_ref(), &mut toasts);
                    }
                    Click::Chosen(toggle) => toggle.toggle(&mut machine.rt.settings),
                    Click::Ignored if machine.is_editing() => {
                        let (x, y) = to_screen(x, y);
                        let extend = shift(keyboard.mod_state());
                        machine.edit().mouse_down(x, y, extend);
                        dragging = true;
                    }
                    Click::Consumed | Click::Ignored => {}
                },
                Event::KeyDown {
                    keycode: Some(Keycode::Pause),
                    ..
                } => machine.stop(),
                // SmileBASIC 4's F5 is the + button.
                Event::KeyDown {
                    keycode: Some(Keycode::F5),
                    keymod,
                    repeat: false,
                    ..
                } if !ctrl(keymod) => machine.sysreq(),
                // F8 leaves F9 to F11 for SmileBASIC 4's tools.
                Event::KeyDown {
                    keycode: Some(Keycode::F8),
                    repeat: false,
                    ..
                } => menu.shown = !menu.shown,
                // A paused program takes no keys.
                Event::KeyDown { .. } | Event::TextInput { .. }
                    if menu.shown && machine.is_running() => {}
                Event::KeyDown {
                    keycode: Some(Keycode::C),
                    keymod,
                    ..
                } if machine.rt.settings.ctrl_c_stops && ctrl(keymod) && !machine.is_editing() => {
                    machine.stop()
                }
                // Ctrl+Shift+V pastes everywhere, also as keys to programs.
                Event::KeyDown {
                    keycode: Some(Keycode::V),
                    keymod,
                    ..
                } if ctrl(keymod) && shift(keymod) => paste(&mut machine, &clipboard, &mut toasts),
                Event::KeyDown {
                    keycode: Some(Keycode::F4),
                    ..
                } => machine.toggle_editor(),
                // SmileBASIC 4's F12 returns to its menu, with the projects.
                Event::KeyDown {
                    keycode: Some(Keycode::F12),
                    ..
                } if !machine.is_running() => {
                    let size = (display_w, display_h);
                    manager = Some(ProjectManager::new(&machine.rt.storage, size));
                }
                Event::KeyDown {
                    keycode: Some(Keycode::F2),
                    ..
                } if machine.is_editing() => next_editor_size(&mut machine),
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    ..
                } if ctrl(keymod) && !machine.is_running() && slot_key(key).is_some() => {
                    machine.edit_slot(slot_key(key).expect("checked"));
                }
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    ..
                } if machine.is_editing() && ctrl(keymod) => match key {
                    Keycode::W => Action::ToggleWrap.toggle(&mut machine.rt.settings),
                    Keycode::B => next_editor_size(&mut machine),
                    Keycode::Home => machine.edit().file_top(),
                    Keycode::End => machine.edit().file_end(),
                    Keycode::A => machine.edit().select_all(),
                    Keycode::C | Keycode::X => {
                        let editor = machine.edit();
                        let text = if key == Keycode::X {
                            editor.cut()
                        } else {
                            editor.copy()
                        };
                        if let Some(Err(e)) = text.map(|t| clipboard.set_clipboard_text(&t)) {
                            toasts.push(format!("Could not copy: {e}"));
                        }
                    }
                    Keycode::V => paste(&mut machine, &clipboard, &mut toasts),
                    _ => {}
                },
                // Shift and the cursor keys select in the editor.
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    ..
                } if machine.is_editing()
                    && shift(keymod)
                    && special_key(key).is_some_and(is_movement) =>
                {
                    machine
                        .edit()
                        .key_select(special_key(key).expect("checked"));
                }
                Event::KeyDown {
                    keycode: Some(key), ..
                } => {
                    if let Some(code) = special_key(key) {
                        machine.rt.key_press(code);
                    }
                }
                Event::TextInput { text, .. } => machine.type_text(&text),
                _ => {}
            }
        }

        if quit {
            let unsaved = machine.unsaved_slots();
            if unsaved.is_empty() {
                break 'running;
            }
            dialog = Some((Dialog::quit(&unsaved, (display_w, display_h)), Asking::Quit));
        }

        // Programs see nothing pressed while the emulator's own screens
        // take the input.
        let modal = dialog.is_some() || manager.is_some() || color_window.is_some();
        let paused = menu.shown && machine.is_running();
        menu.paused = paused;
        machine.rt.input.live = if modal || paused || machine.is_editing() {
            State::default()
        } else {
            controls.state(
                &events.keyboard_state(),
                keyboard.mod_state(),
                events.mouse_state(),
                machine.rt.settings.keyboard_buttons,
            )
        };
        if let Ok(path) = font_choice.try_recv() {
            let settings = &mut machine.rt.settings;
            settings.smilebasic_instruments = false;
            settings.sound_font = Some(path.to_string_lossy().into_owned());
            use_instruments(&mut machine, bank.as_ref(), &mut toasts);
        }
        if let Ok((path, transfer)) = folder_choice.try_recv()
            && let Some(open) = &mut manager
        {
            open.folder_chosen(transfer, path, &machine.rt.storage);
        }
        if let Ok((path, save)) = colors_choice.try_recv() {
            let palette = &mut machine.rt.settings.editor_colors;
            toasts.push(if save {
                save_colors(palette, &path)
            } else {
                load_colors(palette, &path)
            });
        }
        if !paused {
            machine.frame();
        }
        audio.play(&machine.rt.sound.take_samples());
        // The editor leaves its top row for the menu bar.
        menu.pinned = machine.is_editing();
        // Settings change through the menu, keys and questions; keep each
        // change at once so that nothing is lost if the emulator crashes.
        if machine.rt.settings != saved_settings {
            saved_settings = machine.rt.settings.clone();
            if let Err(e) = std::fs::write(&settings_path, saved_settings.to_text()) {
                eprintln!(
                    "could not save settings to {}: {e}",
                    settings_path.display()
                );
            }
        }
        toasts.tick();
        if machine.rt.input.take_buttons_read() {
            let keys = machine.rt.settings.keyboard_buttons;
            key_hint.buttons_read(keys, controls.gamepads());
        }
        key_hint.tick();
        for notice in machine.rt.take_notices() {
            toasts.push(notice);
        }
        match (&dialog, machine.rt.confirmation()) {
            (None, Some(confirm)) => {
                let open = Dialog::ask(confirm, (display_w, display_h));
                dialog = Some((open, Asking::Command));
            }
            (Some((_, Asking::Command)), None) => dialog = None,
            _ => {}
        }
        match (&program_dialog, machine.rt.program_dialog()) {
            (Some(open), Some(shown)) if open.shows(shown) => {}
            (_, Some(shown)) => {
                program_dialog = Some(ProgramView::new(shown, (display_w, display_h)));
            }
            (_, None) => program_dialog = None,
        }
        blink = blink.wrapping_add(1);

        mode = machine.screen_mode();
        filter = machine.rt.settings.screen_filter.unwrap_or(mode.filter);
        let rect = screen_rect(&mode, filter, out_w, out_h);
        let editing = machine.is_editing();
        let fb = machine.render();
        let frame = (fb.width(), fb.height());
        // The editor's frame can be finer than the screen it is shown in.
        let kind = if editing {
            pointer_space = frame;
            (frame, editor_filter(rect, frame, filter))
        } else {
            pointer_space = (mode.width, mode.height);
            (frame, filter)
        };
        if kind != texture_kind {
            texture_kind = kind;
            texture = frame_texture(&canvas, &texture_creator, kind)?;
        }
        texture.update(None, as_bytes(fb.pixels()), (fb.width() * 4) as usize)?;
        canvas.set_draw_color(Color::BLACK);
        canvas.clear();
        canvas.copy(&texture, None, rect)?;
        // The colors window leaves the editor behind it bright, to show
        // its colors.
        let dims = dialog.is_some() || manager.is_some() || program_dialog.is_some();
        // A running program shows the mouse cursor only if it asks to,
        // unless the emulator's own screens need it.
        let show_cursor = !machine.is_running()
            || modal
            || dims
            || menu.visible()
            || machine.rt.settings.keep_mouse_cursor
            || machine.rt.mouse_cursor();
        if show_cursor != cursor_shown {
            cursor_shown = show_cursor;
            mouse.show_cursor(show_cursor);
        }
        if modal || dims || menu.visible() || toasts.visible() || key_hint.visible() {
            // A dialog or the project manager dims the screen behind it.
            overlay.clear(if dims { 0x8000_0000 } else { 0 });
            let font = machine.rt.font();
            if let Some(open) = &manager {
                open.draw(&mut overlay, font);
            }
            if let Some(open) = &program_dialog {
                open.draw(&mut overlay, font, (blink / 25).is_multiple_of(2));
            }
            if let Some(open) = &color_window {
                open.draw(&mut overlay, font, &machine.rt.settings.editor_colors);
            }
            if key_hint.visible() {
                key_hint.draw(&mut overlay, font);
            }
            toasts.draw(&mut overlay, font);
            if let Some((open, _)) = &dialog {
                open.draw(&mut overlay, font);
            } else if !modal && menu.visible() {
                let running = machine.is_running();
                menu.draw(&mut overlay, font, &machine.rt.settings, running);
            }
            overlay_texture.update(None, as_bytes(overlay.pixels()), (display_w * 4) as usize)?;
            canvas.copy(&overlay_texture, None, display)?;
        }
        canvas.present();
        next_frame += FRAME;
        let now = Instant::now();
        if next_frame > now {
            std::thread::sleep(next_frame - now);
        } else if now - next_frame > FRAME {
            // Too far behind to catch up; carry on from now.
            next_frame = now;
        }
    }
    Ok(())
}

/// Plays with SmileBASIC 4's instruments when they are imported and
/// chosen, or else with the chosen SoundFont or the bundled one. A
/// SoundFont that cannot be loaded leaves the bundled one in use.
fn use_instruments(machine: &mut Machine, bank: Option<&Arc<Bank>>, toasts: &mut Toasts) {
    if let Some(bank) = bank.filter(|_| machine.rt.settings.smilebasic_instruments) {
        machine
            .rt
            .sound
            .set_instruments(Some(Instruments::Bank(bank.clone())));
        return;
    }
    let path = machine.rt.settings.sound_font.clone();
    let font = match audio::load_font(path.as_deref().map(Path::new)) {
        Ok(font) => Some(font),
        Err(e) => {
            toasts.push(format!("Using the bundled sound font: {e}"));
            machine.rt.settings.sound_font = None;
            audio::load_font(None).map_err(|e| eprintln!("{e}")).ok()
        }
    };
    machine
        .rt
        .sound
        .set_instruments(font.map(Instruments::Font));
}

/// The computer's clipboard, for CLIPBOARD.
struct SystemClipboard(ClipboardUtil);

impl sb_runtime::env::Clipboard for SystemClipboard {
    // An empty or unreadable clipboard reads as an empty string.
    fn text(&mut self) -> String {
        self.0.clipboard_text().unwrap_or_default()
    }

    fn set_text(&mut self, text: &str) {
        let _ = self.0.set_clipboard_text(text);
    }
}

/// Pastes the clipboard's text into the editor, or types it.
fn paste(machine: &mut Machine, clipboard: &ClipboardUtil, toasts: &mut Toasts) {
    match clipboard.clipboard_text() {
        Ok(text) => machine.paste(&text),
        Err(e) => toasts.push(format!("Could not paste: {e}")),
    }
}

/// Opens the folder that holds the projects in the system's file manager.
fn open_folder(storage: &Storage, toasts: &mut Toasts) {
    let Some(root) = storage.root() else {
        toasts.push("There is no projects folder".to_string());
        return;
    };
    let opened = std::fs::create_dir_all(root)
        .map_err(|e| e.to_string())
        .and_then(|()| sdl3::url::open_url(&file_url(root)).map_err(|e| e.to_string()));
    if let Err(e) = opened {
        toasts.push(format!("Could not open the projects folder: {e}"));
    }
}

/// A `file:` URL for an absolute path, with anything but plain characters
/// percent-encoded.
fn file_url(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    // Windows paths start with a drive letter rather than a slash.
    let mut url = String::from(if path.starts_with('/') {
        "file://"
    } else {
        "file:///"
    });
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => {
                url.push(char::from(b))
            }
            _ => url.push_str(&format!("%{b:02X}")),
        }
    }
    url
}

/// Who asked the open dialog's question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asking {
    Quit,
    /// A command of the running program or direct mode.
    Command,
}

/// Where the display (emulator UI coordinates) goes in a window of
/// `out_w` x `out_h` pixels: scaled to fit and centered.
fn display_rect(width: u32, height: u32, out_w: u32, out_h: u32) -> FRect {
    let scale = (out_w as f32 / width as f32).min(out_h as f32 / height as f32);
    let (w, h) = (width as f32 * scale, height as f32 * scale);
    FRect::new((out_w as f32 - w) / 2.0, (out_h as f32 - h) / 2.0, w, h)
}

/// A texture for frames of `w` x `h` pixels, scaled with `filter`:
/// XSCREEN's, or the one chosen in the menu.
fn frame_texture<'a>(
    canvas: &Canvas<Window>,
    creator: &'a TextureCreator<WindowContext>,
    ((w, h), filter): ((u32, u32), Filter),
) -> Result<Texture<'a>, AnyError> {
    // The Rust bindings lack SDL 3.4's pixel art mode for a single
    // texture, but the renderer's default for new ones takes it.
    let scale = match filter {
        Filter::Bilinear => SDL_ScaleMode::LINEAR,
        Filter::SmartNearest => SDL_ScaleMode::PIXELART,
        Filter::Nearest => SDL_ScaleMode::NEAREST,
    };
    // SAFETY: the renderer pointer is valid for the lifetime of `canvas`.
    unsafe { SDL_SetDefaultTextureScaleMode(canvas.raw(), scale) };
    Ok(creator.create_texture_streaming(PixelFormat::ARGB8888, w, h)?)
}

/// Where the screen goes in a window of `out_w` x `out_h` pixels:
/// centered, at XSCREEN's aspect ratio, and scaled to fit, by a whole
/// factor when the filter is nearest neighbor.
fn screen_rect(mode: &ScreenMode, filter: Filter, out_w: u32, out_h: u32) -> FRect {
    let height = mode.height as f32;
    let width = height * mode.aspect() as f32;
    let fit = (out_w as f32 / width).min(out_h as f32 / height);
    let scale = match filter {
        Filter::Nearest => fit.floor().max(1.0),
        _ => fit,
    };
    let (w, h) = (width * scale, height * scale);
    FRect::new((out_w as f32 - w) / 2.0, (out_h as f32 - h) / 2.0, w, h)
}

/// How the editor's frame is scaled into `rect`: sharp when it fits a
/// whole number of times, and otherwise with the pixel art filter, unless
/// the screen is filtered smoothly.
fn editor_filter(rect: FRect, (_, h): (u32, u32), filter: Filter) -> Filter {
    let scale = rect.h / h as f32;
    if (scale - scale.round()).abs() < 0.001 {
        Filter::Nearest
    } else if filter == Filter::Bilinear {
        Filter::Bilinear
    } else {
        Filter::SmartNearest
    }
}

/// Makes the editor's characters a size smaller, back to the largest
/// after the smallest, as F2 and Ctrl+B do.
fn next_editor_size(machine: &mut Machine) {
    let size = &mut machine.rt.settings.editor_size;
    *size = size.next();
}

/// Reads editor colors from a file; returns what to tell the user.
fn load_colors(palette: &mut Palette, path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    match std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|text| Palette::from_text(&text))
    {
        Ok(read) => {
            *palette = read;
            format!("Loaded editor colors from {name}")
        }
        Err(e) => format!("Could not load {name}: {e}"),
    }
}

/// Writes the editor colors to a file; returns what to tell the user.
fn save_colors(palette: &Palette, path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    match std::fs::write(path, palette.to_text()) {
        Ok(()) => format!("Saved editor colors to {name}"),
        Err(e) => format!("Could not save {name}: {e}"),
    }
}

/// The slot that Ctrl and a digit key opens in the editor: Ctrl+1 for PRG0
/// up to Ctrl+6 for PRG5.
fn slot_key(key: Keycode) -> Option<usize> {
    [
        Keycode::_1,
        Keycode::_2,
        Keycode::_3,
        Keycode::_4,
        Keycode::_5,
        Keycode::_6,
    ]
    .iter()
    .position(|&k| k == key)
}

/// The INKEY$ code of a key that does not type text.
fn special_key(key: Keycode) -> Option<u16> {
    Some(match key {
        Keycode::Escape => keys::ESCAPE,
        Keycode::Tab => keys::TAB,
        Keycode::Backspace => keys::BACKSPACE,
        Keycode::Return | Keycode::KpEnter => keys::ENTER,
        Keycode::Insert => keys::INSERT,
        Keycode::Delete => keys::DELETE,
        Keycode::Home => keys::HOME,
        Keycode::End => keys::END,
        Keycode::PageUp => keys::PAGE_UP,
        Keycode::PageDown => keys::PAGE_DOWN,
        Keycode::Right => keys::RIGHT,
        Keycode::Left => keys::LEFT,
        Keycode::Up => keys::UP,
        Keycode::Down => keys::DOWN,
        _ => return None,
    })
}

fn as_bytes(pixels: &[u32]) -> &[u8] {
    // SAFETY: u32 has no padding and u8 has alignment 1.
    unsafe { std::slice::from_raw_parts(pixels.as_ptr().cast(), std::mem::size_of_val(pixels)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_encode_the_path() {
        assert_eq!(
            file_url(Path::new("/home/a b/데이터")),
            "file:///home/a%20b/%EB%8D%B0%EC%9D%B4%ED%84%B0"
        );
        assert_eq!(file_url(Path::new("C:\\Users\\x")), "file:///C:/Users/x");
    }
}

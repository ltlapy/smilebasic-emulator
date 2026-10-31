#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod colors;
mod controls;
mod dialog;
mod file_dialogs;
mod help;
mod index;
mod keyhint;
mod menu;
mod program_dialog;
mod projects;
mod settings_window;
mod shortcuts;
mod toast;
mod ui;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use audio::Audio;
use colors::ColorWindow;
use controls::Controls;
use dialog::{Answer, Dialog};
use file_dialogs::{FilePicker, SaveDialog};
use help::HelpViewer;
use index::IndexPanel;
use keyhint::KeyHint;
use menu::{Action, Click, Context, MenuBar};
use program_dialog::ProgramView;
use projects::{Outcome, ProjectManager, Transfer};
use sb_profiles::Profile;
use sb_runtime::editor::{Mode, Place, is_movement};
use sb_runtime::font::Font;
use sb_runtime::help::{Help, Lookup};
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
use settings_window::SettingsWindow;
use shortcuts::ShortcutsWindow;
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
/// Frames run without drawing them, at most, when drawing falls behind.
const MOST_SKIPPED: u32 = 3;
/// How long a frame runs a program that does not wait for the next one,
/// leaving the rest of the frame for drawing.
const RUN_TIME: Duration = Duration::from_millis(10);
/// The same for frames run to catch up.
const CATCH_UP_RUN_TIME: Duration = Duration::from_millis(1);
/// Holding F5, SmileBASIC 4's + button, this long stops a program even
/// past ON BREAK GOTO (checked on hardware).
const FORCED_STOP: Duration = Duration::from_secs(1);
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
    let help = resources::load_help(&resource_dir()?).map(std::rc::Rc::new);
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
    // Completion and argument hints take the forms of the help's pages.
    rt.library = std::rc::Rc::new(sb_runtime::complete::Library::new(help.as_deref()));
    rt.help = help.clone();
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
    let mut settings_window: Option<SettingsWindow> = None;
    let mut shortcuts_window: Option<ShortcutsWindow> = None;
    // Ctrl+S's and Ctrl+L's windows.
    let mut file_dialog: Option<FileDialog> = None;
    // A panel beside the editor, and whether it takes the keys.
    let mut side: Option<Side> = None;
    let mut side_focus = false;
    // The text of a key that something else took: the space some systems
    // type for Ctrl+Space, or a digit that chose a slot in a window that
    // then closed, which would otherwise reach the program or the editor.
    let mut eat_text = false;

    let mut events = sdl.event_pump()?;
    let mut next_frame = Instant::now();
    // The left button went down on the editor and is still held.
    let mut dragging = false;
    // When F5 went down, while it is held.
    let mut f5_down: Option<Instant> = None;
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
        let shown = screen_rect(&mode, filter, out_w, display.h);
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
            if matches!(event, Event::KeyDown { .. }) {
                eat_text = dialog.is_some()
                    || program_dialog.is_some()
                    || manager.is_some()
                    || file_dialog.is_some()
                    || settings_window.is_some()
                    || shortcuts_window.is_some()
                    || color_window.is_some()
                    || (side.is_some() && side_focus);
            }
            match event {
                Event::MouseMotion { x, y, .. } => {
                    let (sx, sy) = to_screen(x, y);
                    let m = &mut controls.mouse;
                    (m.x, m.y, m.display) = (sx, sy, to_display(x, y));
                    m.on_screen = (0..mode.width as i32).contains(&sx)
                        && (0..mode.height as i32).contains(&sy);
                }
                Event::MouseWheel { y, .. } if manager.is_none() && !machine.is_editing() => {
                    controls.mouse.wheel += y.round() as i32;
                }
                // The + button acts when let go, unless it was held until
                // the program stopped (checked on hardware).
                Event::KeyUp {
                    keycode: Some(Keycode::F5),
                    ..
                } if f5_down.take().is_some() => machine.sysreq(),
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
                match (answer, asking.clone()) {
                    (Some(Answer::Accept), Asking::Quit) => break 'running,
                    (Some(answer), Asking::Command) => {
                        machine.rt.answer(answer == Answer::Accept);
                        dialog = None;
                    }
                    (Some(Answer::Accept), Asking::Overwrite(name)) => {
                        save_edited(&mut machine, &name, &mut toasts);
                        dialog = None;
                    }
                    (Some(Answer::Accept), Asking::Discard(name)) => {
                        load_edited(&mut machine, &name, &mut toasts);
                        dialog = None;
                    }
                    (Some(_), Asking::Notice) | (Some(Answer::Cancel), _) => dialog = None,
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
            if let Some(open) = &mut side {
                match event {
                    Event::MouseWheel {
                        y,
                        mouse_x,
                        mouse_y,
                        ..
                    } if open.frame().contains(to_display(mouse_x, mouse_y)) => {
                        open.wheel(-y.round() as i32);
                        continue;
                    }
                    Event::MouseMotion { x, y, .. } => open.mouse_move(to_display(x, y)),
                    _ => {}
                }
            }
            if let (Some(open), true) = (&mut side, side_focus) {
                let (taken, outcome) = match (open, &event) {
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
                    (Side::Index(index, _), Event::TextInput { text, .. }) => {
                        (true, index.text(text))
                    }
                    (_, Event::TextInput { .. }) => (true, index::Outcome::Stay),
                    _ => (false, index::Outcome::Stay),
                };
                side_outcome(&mut side, &mut side_focus, &mut machine, outcome);
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
                        clicks,
                        x,
                        y,
                        ..
                    } => match clicks {
                        2 => open.double_click(to_display(x, y), storage, &unsaved),
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
            if let Some(open) = &mut file_dialog {
                let storage = &machine.rt.storage;
                let outcome = match (open, event) {
                    (_, Event::Quit { .. }) => {
                        quit = true;
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
                let size = (display_w, display_h);
                match outcome {
                    file_dialogs::Outcome::Stay => {}
                    file_dialogs::Outcome::Close => file_dialog = None,
                    file_dialogs::Outcome::Save(name) => {
                        file_dialog = None;
                        let own = machine.rt.slot_names[slot] == name;
                        if !own && machine.rt.txt_exists(&name) {
                            let lines = vec![
                                format!("Save slot {slot} as {name}."),
                                "The file there now will be replaced.".to_string(),
                            ];
                            let open = Dialog::new("Save file", lines, "Save", size);
                            dialog = Some((open, Asking::Overwrite(name)));
                        } else {
                            save_edited(&mut machine, &name, &mut toasts);
                        }
                    }
                    // Only the projects window copies.
                    file_dialogs::Outcome::Copy { .. } => file_dialog = None,
                    file_dialogs::Outcome::Load(spec) => {
                        file_dialog = None;
                        if machine.unsaved_slots().contains(&slot) {
                            let lines = vec![
                                format!("Load {spec} into slot {slot}."),
                                "Its unsaved changes will be lost.".to_string(),
                            ];
                            let open = Dialog::new("Load file", lines, "Load", size);
                            dialog = Some((open, Asking::Discard(spec)));
                        } else {
                            load_edited(&mut machine, &spec, &mut toasts);
                        }
                    }
                }
                continue;
            }
            if let Some(open) = &mut shortcuts_window {
                let outcome = match event {
                    Event::Quit { .. } => {
                        quit = true;
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
                    shortcuts_window = None;
                }
                continue;
            }
            if let Some(open) = &mut settings_window {
                let settings = &mut machine.rt.settings;
                let outcome = match event {
                    Event::Quit { .. } => {
                        quit = true;
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
                    settings_window::Outcome::Close => settings_window = None,
                    settings_window::Outcome::Instruments => {
                        if machine.rt.settings.smilebasic_instruments && bank.is_none() {
                            toasts.push(
                                "SmileBASIC 4's instruments are not imported \
                                 (see smilebasic-import)"
                                    .into(),
                            );
                        }
                        use_instruments(&mut machine, bank.as_ref(), &mut toasts);
                    }
                    settings_window::Outcome::ChooseSoundFont => {
                        choose_sound_font(canvas.window(), chosen_font.clone(), &mut toasts);
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
                        machine.editor_mouse_drag(x, y);
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
                } => match menu.click(to_display(x, y), menu_context(&machine)) {
                    Click::Chosen(Action::Quit) => quit = true,
                    Click::Chosen(action @ (Action::SaveSlot | Action::LoadSlot)) => {
                        let size = (display_w, display_h);
                        let save = action == Action::SaveSlot;
                        file_dialog = Some(open_file_dialog(&machine, save, size));
                    }
                    Click::Chosen(Action::Undo) => machine.edit().undo(),
                    Click::Chosen(Action::Redo) => machine.edit().redo(),
                    Click::Chosen(action @ (Action::Cut | Action::Copy)) => {
                        copy_selection(&mut machine, &clipboard, &mut toasts, action == Action::Cut)
                    }
                    Click::Chosen(Action::Paste) => paste(&mut machine, &clipboard, &mut toasts),
                    Click::Chosen(Action::SelectAll) => machine.edit().select_all(),
                    Click::Chosen(Action::Comment) => machine.edit().comment(),
                    Click::Chosen(Action::Uncomment) => machine.edit().uncomment(),
                    Click::Chosen(Action::Run) => machine.sysreq(),
                    Click::Chosen(Action::Help) => {
                        if !machine.is_editing() {
                            machine.toggle_editor();
                        }
                        let size = (display_w, display_h);
                        if let Some(panel) =
                            open_help(&mut machine, help.as_deref(), &mut toasts, size)
                        {
                            side = Some(panel);
                            side_focus = true;
                        }
                    }
                    Click::Chosen(Action::Shortcuts) => {
                        shortcuts_window = Some(ShortcutsWindow::new((display_w, display_h)));
                    }
                    Click::Chosen(Action::About) => {
                        let version = env!("CARGO_PKG_VERSION");
                        let lines = vec![
                            format!("SmileBASIC emulator {version}"),
                            "An emulator for SmileBASIC 4 programs.".to_string(),
                            String::new(),
                            "MIT OR Apache-2.0.".to_string(),
                            "Bundled fonts: SIL Open Font License 1.1.".to_string(),
                            "GeneralUser GS: its own license.".to_string(),
                        ];
                        let about = Dialog::notice("About", lines, (display_w, display_h));
                        dialog = Some((about, Asking::Notice));
                    }
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
                    Click::Chosen(Action::Split) => machine.toggle_split(),
                    Click::Chosen(Action::Index) => {
                        if !machine.is_editing() {
                            machine.toggle_editor();
                        }
                        side = Some(open_index(&mut machine, (display_w, display_h)));
                        side_focus = true;
                    }
                    Click::Chosen(
                        command @ (Action::GoToLine | Action::Find | Action::Replace),
                    ) => {
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
                    Click::Chosen(Action::Settings) => {
                        settings_window = Some(SettingsWindow::new((display_w, display_h)));
                    }
                    Click::Chosen(Action::EditColors) => {
                        color_window = Some(ColorWindow::new((display_w, display_h)));
                    }
                    Click::Chosen(toggle) => toggle.toggle(&mut machine.rt.settings),
                    Click::Ignored
                        if let Some(open) = &mut side
                            && open.frame().contains(to_display(x, y)) =>
                    {
                        side_focus = true;
                        if let Side::Index(index, _) = open {
                            let outcome = index.click(to_display(x, y));
                            side_outcome(&mut side, &mut side_focus, &mut machine, outcome);
                        }
                    }
                    Click::Ignored if machine.is_editing() => {
                        side_focus = false;
                        let (x, y) = to_screen(x, y);
                        let extend = shift(keyboard.mod_state());
                        machine.editor_mouse_down(x, y, extend);
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
                } if !ctrl(keymod) => f5_down = Some(Instant::now()),
                // F8 leaves F9 to F11 for SmileBASIC 4's tools.
                Event::KeyDown {
                    keycode: Some(Keycode::F8),
                    repeat: false,
                    ..
                } => menu.shown = !menu.shown,
                // A paused program takes no keys.
                Event::KeyDown { .. } | Event::TextInput { .. }
                    if menu.shown && machine.is_running() => {}
                // Ctrl+Space hides completion's list and the hints, or
                // shows them again.
                Event::KeyDown {
                    keycode: Some(Keycode::Space),
                    keymod,
                    ..
                } if ctrl(keymod) && (machine.is_editing() || machine.rt.wants_line()) => {
                    if machine.is_editing() {
                        machine.edit().toggle_popups();
                    } else {
                        machine.rt.toggle_line_popups();
                    }
                    eat_text = true;
                }
                // Editing the line typed in direct mode or for INPUT.
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    ..
                } if !machine.is_editing()
                    && machine.rt.wants_line()
                    && let Some(code) = line_command(key, shift(keymod), ctrl(keymod)) =>
                {
                    machine.rt.key_press(code)
                }
                Event::KeyDown {
                    keycode: Some(key @ (Keycode::C | Keycode::X)),
                    keymod,
                    ..
                } if ctrl(keymod)
                    && !machine.is_editing()
                    && let Some(text) = machine.rt.line_selection() =>
                {
                    if let Err(e) = clipboard.set_clipboard_text(&text) {
                        toasts.push(format!("Could not copy: {e}"));
                    } else if key == Keycode::X {
                        machine.rt.key_press(keys::BACKSPACE);
                    }
                }
                Event::KeyDown {
                    keycode: Some(Keycode::V),
                    keymod,
                    ..
                } if ctrl(keymod) && !machine.is_editing() && machine.rt.wants_line() => {
                    paste(&mut machine, &clipboard, &mut toasts)
                }
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
                // F1 shows the help for the command at the cursor.
                Event::KeyDown {
                    keycode: Some(Keycode::F1),
                    repeat: false,
                    ..
                } if machine.is_editing() => {
                    let size = (display_w, display_h);
                    if let Some(panel) = open_help(&mut machine, help.as_deref(), &mut toasts, size)
                    {
                        side = Some(panel);
                        side_focus = true;
                    }
                }
                // Simple save and load, as SmileBASIC 4 has them.
                Event::KeyDown {
                    keycode: Some(key @ (Keycode::S | Keycode::L)),
                    keymod,
                    ..
                } if ctrl(keymod) && !machine.is_running() => {
                    let size = (display_w, display_h);
                    file_dialog = Some(open_file_dialog(&machine, key == Keycode::S, size));
                }
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
                        copy_selection(&mut machine, &clipboard, &mut toasts, key == Keycode::X)
                    }
                    Keycode::V => paste(&mut machine, &clipboard, &mut toasts),
                    Keycode::Z if shift(keymod) => machine.edit().redo(),
                    Keycode::Z => machine.edit().undo(),
                    Keycode::Y => machine.edit().redo(),
                    Keycode::K if shift(keymod) => machine.edit().uncomment(),
                    Keycode::K => machine.edit().comment(),
                    Keycode::Return | Keycode::KpEnter => {
                        let editor = machine.edit();
                        match editor.mode() {
                            Mode::Replace => editor.replace_all(),
                            _ => editor.insert_line(),
                        }
                    }
                    Keycode::G => machine.edit().go_to_line(),
                    Keycode::Slash | Keycode::KpDivide => machine.toggle_split(),
                    Keycode::_0 | Keycode::Kp0 => machine.toggle_editor(),
                    Keycode::Tab => machine.switch_side(),
                    Keycode::J => {
                        side = Some(open_index(&mut machine, (display_w, display_h)));
                        side_focus = true;
                    }
                    Keycode::F => machine.edit().start_find(false),
                    Keycode::H => machine.edit().start_find(true),
                    Keycode::Backspace => machine.edit().delete_line(),
                    Keycode::Delete => machine.edit().delete_to_end(),
                    _ => {}
                },
                // Esc, which the editor's text does not use, closes a panel.
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } if side.is_some()
                    && machine.is_editing()
                    && machine.edit().mode() == Mode::Edit =>
                {
                    side = None;
                }
                Event::MouseWheel { y, .. } if machine.is_editing() => {
                    machine.edit().wheel(-3 * y.round() as i32);
                }
                // F3 finds, and in finding goes to the next match.
                Event::KeyDown {
                    keycode: Some(Keycode::F3),
                    keymod,
                    ..
                } if machine.is_editing() => {
                    let editor = machine.edit();
                    if shift(keymod) || matches!(editor.mode(), Mode::Find | Mode::Replace) {
                        editor.find_next(!shift(keymod));
                    } else {
                        editor.start_find(false);
                    }
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Tab),
                    keymod,
                    ..
                } if machine.is_editing()
                    && shift(keymod)
                    && machine.edit().mode() == Mode::Edit =>
                {
                    machine.edit().unindent()
                }
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
                Event::TextInput { .. } if eat_text => eat_text = false,
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
        let modal = dialog.is_some()
            || manager.is_some()
            || color_window.is_some()
            || settings_window.is_some()
            || shortcuts_window.is_some()
            || file_dialog.is_some();
        if !machine.is_editing() {
            side = None;
        }
        // Panels go on the side away from the code being edited.
        if let Some(open) = &mut side {
            let right_side = !machine.is_split_rows() && machine.active_side() == 1;
            open.set_left(machine.is_split() && right_side);
        }
        if f5_down.is_some_and(|t| t.elapsed() >= FORCED_STOP) {
            f5_down = None;
            machine.stop();
        }
        let paused = menu.shown && machine.is_running();
        menu.status = status_line(&machine, paused);
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
            machine.time_limit = Some(RUN_TIME);
            machine.frame();
        }
        audio.play(&machine.rt.sound.take_samples());
        // Behind by a frame or more, as when drawing takes too long: the
        // program and its sound catch up, and only the last of these
        // frames is drawn, so that the game and its music keep time.
        // Those frames run code briefly, so that catching up does not
        // fall further behind.
        let mut skipped = 0;
        while !paused && skipped < MOST_SKIPPED && Instant::now() >= next_frame + FRAME {
            machine.time_limit = Some(CATCH_UP_RUN_TIME);
            machine.frame();
            audio.play(&machine.rt.sound.take_samples());
            next_frame += FRAME;
            skipped += 1;
        }
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
        let rect = screen_rect(&mode, filter, out_w, display.h);
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
        if modal
            || dims
            || side.is_some()
            || menu.visible()
            || toasts.visible()
            || key_hint.visible()
        {
            // A dialog or the project manager dims the screen behind it.
            overlay.clear(if dims { 0x8000_0000 } else { 0 });
            let font = machine.rt.font();
            if let Some(open) = &manager {
                open.draw(&mut overlay, font, (blink / 25).is_multiple_of(2));
            }
            match &side {
                Some(Side::Help(help)) => help.draw(&mut overlay, font, side_focus),
                Some(Side::Index(index, _)) => index.draw(&mut overlay, font, side_focus),
                None => {}
            }
            if let Some(open) = &program_dialog {
                open.draw(&mut overlay, font, (blink / 25).is_multiple_of(2));
            }
            if let Some(open) = &color_window {
                open.draw(&mut overlay, font, &machine.rt.settings.editor_colors);
            }
            if let Some(open) = &settings_window {
                open.draw(&mut overlay, font, &machine.rt.settings);
            }
            if let Some(open) = &shortcuts_window {
                open.draw(&mut overlay, font);
            }
            match &file_dialog {
                Some(FileDialog::Save(d)) => {
                    d.draw(&mut overlay, font, (blink / 25).is_multiple_of(2))
                }
                Some(FileDialog::Load(p)) => {
                    p.draw(&mut overlay, font, (blink / 25).is_multiple_of(2))
                }
                None => {}
            }
            if key_hint.visible() {
                key_hint.draw(&mut overlay, font);
            }
            toasts.draw(&mut overlay, font);
            if let Some((open, _)) = &dialog {
                open.draw(&mut overlay, font);
            } else if !modal && menu.visible() {
                let context = menu_context(&machine);
                menu.draw(&mut overlay, font, &machine.rt.settings, context);
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

/// Ctrl+S's or Ctrl+L's window.
enum FileDialog {
    Save(SaveDialog),
    Load(FilePicker),
}

/// A panel beside the editor.
enum Side {
    Help(HelpViewer),
    /// With where the editor was when it opened, for Esc.
    Index(IndexPanel, Place),
}

impl Side {
    fn frame(&self) -> ui::Rect {
        match self {
            Side::Help(help) => help.frame(),
            Side::Index(index, _) => index.frame(),
        }
    }

    fn wheel(&mut self, rows: i32) {
        match self {
            Side::Help(help) => help.wheel(rows),
            Side::Index(index, _) => index.wheel(rows),
        }
    }

    fn set_left(&mut self, left: bool) {
        match self {
            Side::Help(help) => help.left = left,
            Side::Index(index, _) => index.left = left,
        }
    }

    fn mouse_move(&mut self, at: (i32, i32)) {
        if let Side::Index(index, _) = self {
            index.mouse_move(at);
        }
    }
}

/// Opens the index of the edited slot's labels and DEFs.
fn open_index(machine: &mut Machine, size: (u32, u32)) -> Side {
    let editor = machine.edit();
    let line = editor.cursor().0 - 1;
    let panel = IndexPanel::new(size, editor.slot(), editor.index(), line);
    Side::Index(panel, editor.place())
}

/// Acts on what a side panel asked for. The help closes with `Back`.
fn side_outcome(
    side: &mut Option<Side>,
    focus: &mut bool,
    machine: &mut Machine,
    outcome: index::Outcome,
) {
    match outcome {
        index::Outcome::Stay => return,
        index::Outcome::Show(line) => {
            machine.edit().jump_to(line);
            return;
        }
        index::Outcome::Go(line) => machine.edit().jump_to(line),
        index::Outcome::Back => {
            if let Some(Side::Index(_, place)) = side {
                machine.edit().go_back(*place);
            }
        }
    }
    *side = None;
    *focus = false;
}

/// Who asked the open dialog's question.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Asking {
    Quit,
    /// A command of the running program or direct mode.
    Command,
    /// Ctrl+S over another file of this name.
    Overwrite(String),
    /// Ctrl+L over unsaved changes, with the file to load.
    Discard(String),
    /// Something told, such as About, with OK alone.
    Notice,
}

/// Asks for a SoundFont file; the choice comes through `send`.
fn choose_sound_font(window: &Window, send: std::sync::mpsc::Sender<PathBuf>, toasts: &mut Toasts) {
    let filters = [sdl3::dialog::DialogFileFilter {
        name: "SoundFont",
        pattern: "sf2",
    }];
    let shown = sdl3::dialog::show_open_file_dialog(
        &filters,
        None::<&Path>,
        false,
        window,
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

/// What the menu bar's items depend on.
fn menu_context(machine: &Machine) -> Context {
    Context {
        running: machine.is_running(),
        editing: machine.is_editing(),
    }
}

/// The bar's right side: the project, the slot being edited (with `*`
/// for unsaved changes) or direct mode, and a running or paused program.
fn status_line(machine: &Machine, paused: bool) -> String {
    let project = sb_runtime::confirm::project_label(machine.rt.storage.project());
    let place = if machine.is_editing() {
        let slot = machine.edited_slot();
        let unsaved = if machine.unsaved_slots().contains(&slot) {
            "*"
        } else {
            ""
        };
        format!("EDIT PRG{slot}{unsaved}")
    } else {
        "DIRECT".to_string()
    };
    match (machine.is_running(), paused) {
        (true, true) => format!("{project} - Paused, F8 resumes"),
        (true, false) => format!("{project} - Running"),
        _ => format!("{project} - {place}"),
    }
}

/// Ctrl+S's window to save the edited slot, or Ctrl+L's to load into it.
fn open_file_dialog(machine: &Machine, save: bool, size: (u32, u32)) -> FileDialog {
    let slot = machine.edited_slot();
    let storage = &machine.rt.storage;
    if save {
        let name = &machine.rt.slot_names[slot];
        FileDialog::Save(SaveDialog::new(size, slot, name, storage.project()))
    } else {
        FileDialog::Load(FilePicker::new(size, slot, storage))
    }
}

/// F1: the help for the command at the editor's cursor, or its first
/// page away from any command.
fn open_help(
    machine: &mut Machine,
    help: Option<&Help>,
    toasts: &mut Toasts,
    size: (u32, u32),
) -> Option<Side> {
    let Some(help) = help else {
        toasts.push("SmileBASIC 4's help is not imported (see smilebasic-import)".into());
        return None;
    };
    let (line, col) = machine.edit().cursor_line();
    let topic = match help.lookup(line, col) {
        Lookup::Topic(topic) => topic,
        Lookup::Nothing => sb_runtime::help::INTRO.to_string(),
        Lookup::Unknown(word) => {
            toasts.push(format!("No help for {word}"));
            return None;
        }
    };
    let pages = help.pages(&topic);
    let scale = machine.rt.settings.help_text_size;
    Some(Side::Help(HelpViewer::new(size, &topic, &pages, scale)))
}

/// Ctrl+C and Ctrl+X in the editor.
fn copy_selection(
    machine: &mut Machine,
    clipboard: &ClipboardUtil,
    toasts: &mut Toasts,
    cut: bool,
) {
    let editor = machine.edit();
    let text = if cut { editor.cut() } else { editor.copy() };
    if let Some(Err(e)) = text.map(|t| clipboard.set_clipboard_text(&t)) {
        toasts.push(format!("Could not copy: {e}"));
    }
}

/// Saves the edited slot (Ctrl+S); says how it went.
fn save_edited(machine: &mut Machine, name: &str, toasts: &mut Toasts) {
    let slot = machine.edited_slot();
    match machine.save_edited(name) {
        Ok(()) if machine.is_editing() => {}
        Ok(()) => toasts.push(format!("Saved PRG{slot} as {name}")),
        Err(e) => toasts.push(format!("Could not save {name}: {e}")),
    }
}

/// Loads a file into the edited slot (Ctrl+L); says how it went.
fn load_edited(machine: &mut Machine, name: &str, toasts: &mut Toasts) {
    let slot = machine.edited_slot();
    match machine.load_edited(name) {
        Ok(()) => {
            let name = &machine.rt.slot_names[slot];
            toasts.push(format!("Loaded {name} into PRG{slot}"))
        }
        Err(e) => toasts.push(format!("Could not load {name}: {e}")),
    }
}

/// Where the display (emulator UI coordinates) goes in a window of
/// `out_w` x `out_h` pixels: scaled to fit, centered across and at the
/// top, so the menu bar stays at the window's top in a narrow window.
fn display_rect(width: u32, height: u32, out_w: u32, out_h: u32) -> FRect {
    let scale = (out_w as f32 / width as f32).min(out_h as f32 / height as f32);
    let (w, h) = (width as f32 * scale, height as f32 * scale);
    FRect::new((out_w as f32 - w) / 2.0, 0.0, w, h)
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

/// Where the screen goes in a window `out_w` pixels wide whose display
/// is `out_h` high: centered in the display, at XSCREEN's aspect ratio,
/// and scaled to fit, by a whole factor when the filter is nearest
/// neighbor.
fn screen_rect(mode: &ScreenMode, filter: Filter, out_w: u32, out_h: f32) -> FRect {
    let height = mode.height as f32;
    let width = height * mode.aspect() as f32;
    let fit = (out_w as f32 / width).min(out_h / height);
    let scale = match filter {
        Filter::Nearest => fit.floor().max(1.0),
        _ => fit,
    };
    let (w, h) = (width * scale, height * scale);
    FRect::new((out_w as f32 - w) / 2.0, (out_h - h) / 2.0, w, h)
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

/// The line editing command of a key with Shift or Ctrl, for the line
/// typed in direct mode or for INPUT.
fn line_command(key: Keycode, shift: bool, ctrl: bool) -> Option<u16> {
    use sb_runtime::line::commands::*;
    Some(match (key, shift, ctrl) {
        (Keycode::Left, true, false) => SELECT_LEFT,
        (Keycode::Right, true, false) => SELECT_RIGHT,
        (Keycode::Home, true, false) => SELECT_HOME,
        (Keycode::End, true, false) => SELECT_END,
        (Keycode::A, _, true) => SELECT_ALL,
        (Keycode::Backspace, _, true) => DELETE_LINE,
        (Keycode::Delete, _, true) => DELETE_TO_END,
        _ => return None,
    })
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

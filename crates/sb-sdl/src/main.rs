#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod colors;
mod controls;
mod debug_panel;
mod dialog;
mod file_dialogs;
mod help;
mod index;
mod keyhint;
mod menu;
mod popup;
mod program_dialog;
mod projects;
mod settings_window;
mod shortcuts;
mod toast;
mod ui;

use std::cell::{Cell, OnceCell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use audio::Audio;
use colors::ColorWindow;
use controls::Controls;
use debug_panel::{DebugPanel, Hit, Tab, Typed, View, value_text};
use dialog::{Answer, Dialog};
use file_dialogs::{FilePicker, SaveDialog};
use help::HelpViewer;
use index::IndexPanel;
use keyhint::KeyHint;
use menu::{Action, Click, Context, MenuBar};
use popup::Popup;
use program_dialog::ProgramView;
use projects::{Outcome, ProjectManager, Transfer};
use sb_profiles::Profile;
use sb_runtime::editor::{Mode, Place, is_movement};
use sb_runtime::font::Font;
use sb_runtime::help::{Help, Lookup};
use sb_runtime::input::State;
use sb_runtime::machine::{Machine, StopReason};
use sb_runtime::palette::Palette;
use sb_runtime::runtime::{SLOTS, keys};
use sb_runtime::screen::{Filter, ScreenMode};
use sb_runtime::settings::Settings;
use sb_runtime::sound::bank::Bank;
use sb_runtime::sound::synth::Instruments;
use sb_runtime::sprites::Sprites;
use sb_runtime::storage::Storage;
use sb_runtime::{Framebuffer, Runtime, resources, sbfile};
use sdl3::VideoSubsystem;
use sdl3::clipboard::ClipboardUtil;
use sdl3::event::Event;
use sdl3::event::WindowEvent;
use sdl3::gamepad::Button;
use sdl3::keyboard::{Keycode, Mod};
use sdl3::messagebox::{MessageBoxFlag, show_simple_message_box};
use sdl3::mouse::MouseButton;
use sdl3::pixels::{Color, PixelFormat};
use sdl3::render::{BlendMode, Canvas, FRect, ScaleMode, Texture, TextureCreator};
use sdl3::sys::blendmode::SDL_BLENDMODE_BLEND_PREMULTIPLIED;
use sdl3::sys::render::{SDL_SetDefaultTextureScaleMode, SDL_SetTextureBlendMode};
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
const EDITOR_TITLE: &str = "SmileBASIC editor";
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
/// Holding the + button, or F5 for it, this long stops a program even
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
    // The screens in the debugger's panel, drawn at the window's
    // resolution, with their size and filter.
    let mut panel_screens: Option<(Texture, TextureKind)> = None;
    // The subprogram's screen, over the main one.
    let mut sub_texture: Option<(Texture, TextureKind)> = None;
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
    // The editor's own window, made the first time the editor opens apart
    // from the screen and hidden while it is closed, with its frame's
    // texture, its own overlay, and the pixels its pointer positions are
    // given in.
    let editor_gfx: OnceCell<EditorWindow> = OnceCell::new();
    let mut editor_texture: Option<(Texture, TextureKind)> = None;
    let mut editor_overlay = Framebuffer::new(display_w, display_h);
    let mut editor_overlay_texture: Option<Texture> = None;
    let mut editor_space = (1, 1);
    let mut editor_shown = false;
    // Whether it has the keys, and the pointer.
    let mut editor_focused = false;
    let mut pointer_in_editor = false;
    // Whether the menu bar and the emulator's windows show in the editor's
    // window. They follow the keys, but a dialog or window stays where it
    // opened until it closes.
    let mut ui_on_editor = false;
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
    // The debugger's panel below the editor, and the stop it last showed.
    let mut debug_panel = DebugPanel::default();
    let mut shown_stop = None;
    // Whether the panel showed in the last frame.
    let mut panel_was_shown = false;
    // The menu a right click opens on the editor or the debugger's panel.
    let mut debug_menu: Option<Popup<Choice>> = None;
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
    // When the + button, F5 or a controller's, went down, while it is
    // held.
    let mut plus_down: Option<Instant> = None;
    'running: loop {
        // The editor shows in a window of its own while it is open, if
        // chosen.
        machine.set_detached(machine.rt.settings.editor_window);
        let apart = machine.is_detached() && machine.is_editing();
        if apart && editor_gfx.get().is_none() {
            let made = EditorWindow::open(&video, (display_w, display_h), canvas.window())?;
            let _ = editor_gfx.set(made);
            let gfx = editor_gfx.get().expect("set above");
            let mut made = gfx.creator.create_texture_streaming(
                PixelFormat::ARGB8888,
                display_w,
                display_h,
            )?;
            made.set_scale_mode(ScaleMode::Nearest);
            made.set_blend_mode(BlendMode::Blend);
            editor_overlay_texture = Some(made);
        }
        if let Some(gfx) = editor_gfx.get()
            && apart != editor_shown
        {
            editor_shown = apart;
            let mut editor_canvas = gfx.canvas.borrow_mut();
            let window = editor_canvas.window_mut();
            if apart {
                window.show();
                window.raise();
            } else {
                window.hide();
                editor_focused = false;
                pointer_in_editor = false;
            }
        }
        machine.focus_editor(apart && editor_focused);
        // The menu bar and the emulator's windows show in the window with
        // the keys.
        macro_rules! modal_shown {
            () => {
                dialog.is_some()
                    || manager.is_some()
                    || file_dialog.is_some()
                    || settings_window.is_some()
                    || shortcuts_window.is_some()
                    || color_window.is_some()
            };
        }
        if !modal_shown!() {
            ui_on_editor = editor_focused;
        }
        let mut ui_editor = apart && ui_on_editor;
        let (out_w, out_h) = canvas.output_size()?;
        let (win_w, _) = canvas.window().size();
        let display = display_rect(display_w, display_h, out_w, out_h);
        let shown = screen_rect(&mode, filter, out_w, display.h);
        let main_map = Mapping {
            px: out_w as f32 / win_w.max(1) as f32,
            display,
            display_w,
            shown,
            space: pointer_space,
        };
        let editor_map = match editor_gfx.get().filter(|_| apart) {
            Some(gfx) => {
                let editor_canvas = gfx.canvas.borrow();
                let (out_w, out_h) = editor_canvas.output_size()?;
                let (win_w, _) = editor_canvas.window().size();
                let display = display_rect(display_w, display_h, out_w, out_h);
                let mode = machine.editor_mode();
                let filter = machine.rt.settings.screen_filter.unwrap_or(mode.filter);
                Mapping {
                    px: out_w as f32 / win_w.max(1) as f32,
                    display,
                    display_w,
                    shown: screen_rect(&mode, filter, out_w, display.h),
                    space: editor_space,
                }
            }
            None => main_map,
        };
        // The window the event being handled came from: the main one, or
        // the editor's.
        let maps = [main_map, editor_map];
        let source = Cell::new(0);
        // Mouse positions come in window coordinates; the display is in
        // pixels...
        let to_display = |x: f32, y: f32| maps[source.get()].to_display(x, y);
        // ...and so are the screen's or the editor's, which fill their
        // rectangle.
        let to_screen = |x: f32, y: f32| maps[source.get()].to_screen(x, y);
        let shift = |keymod: Mod| keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
        let ctrl = |keymod: Mod| keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD);
        let mut quit = false;
        for event in events.poll_iter() {
            controls.event(&event);
            let editor_id = editor_gfx.get().map(|gfx| gfx.id);
            let from_editor = editor_id.is_some() && event.get_window_id() == editor_id;
            if let (true, Event::Window { win_event, .. }) = (from_editor, &event) {
                match win_event {
                    WindowEvent::FocusGained => editor_focused = true,
                    WindowEvent::FocusLost => editor_focused = false,
                    WindowEvent::MouseEnter => pointer_in_editor = true,
                    WindowEvent::MouseLeave => pointer_in_editor = false,
                    _ => {}
                }
                if !modal_shown!() && editor_focused != ui_on_editor {
                    ui_on_editor = editor_focused;
                    ui_editor = apart && ui_on_editor;
                    menu.leave();
                }
            }
            // Closing the editor's window closes the editor; closing the
            // main one quits even with the editor's shown.
            let event = match event {
                Event::Window {
                    win_event: WindowEvent::CloseRequested,
                    ..
                } if from_editor => {
                    if machine.is_editing() {
                        machine.toggle_editor();
                    }
                    continue;
                }
                Event::Window {
                    win_event: WindowEvent::CloseRequested,
                    timestamp,
                    ..
                } => Event::Quit { timestamp },
                event => event,
            };
            let from_editor = apart && from_editor;
            source.set(usize::from(from_editor));
            // The event is on the editor, wherever it shows...
            let on_editor = machine.is_editing() && (!apart || from_editor);
            // ...or on the window it shows in when open, or on the window
            // with the menu bar and the emulator's windows.
            let editor_window = !apart || from_editor;
            let ui_window = from_editor == ui_editor;
            if matches!(event, Event::KeyDown { .. }) {
                eat_text = dialog.is_some()
                    || program_dialog.is_some()
                    || manager.is_some()
                    || file_dialog.is_some()
                    || settings_window.is_some()
                    || shortcuts_window.is_some()
                    || color_window.is_some()
                    || debug_panel.typing()
                    || (side.is_some() && side_focus);
            }
            // Only the main window's pointer reaches the program, and only
            // the window with the emulator's windows takes theirs.
            let pointer = matches!(
                event,
                Event::MouseMotion { .. }
                    | Event::MouseButtonDown { .. }
                    | Event::MouseButtonUp { .. }
                    | Event::MouseWheel { .. }
            );
            if pointer && modal_shown!() && !ui_window {
                continue;
            }
            match event {
                Event::MouseMotion { x, y, .. } if !from_editor => {
                    let (sx, sy) = to_screen(x, y);
                    let m = &mut controls.mouse;
                    (m.x, m.y, m.display) = (sx, sy, to_display(x, y));
                    m.on_screen = (0..mode.width as i32).contains(&sx)
                        && (0..mode.height as i32).contains(&sy);
                }
                Event::MouseWheel { y, .. } if manager.is_none() && !on_editor && !from_editor => {
                    controls.mouse.wheel += y.round() as i32;
                }
                // The + button acts when let go, unless it was held until
                // the program stopped (checked on hardware).
                Event::KeyUp {
                    keycode: Some(Keycode::F5),
                    ..
                }
                | Event::GamepadButtonUp {
                    button: Button::Start,
                    ..
                } if plus_down.take().is_some() => machine.sysreq(),
                Event::GamepadButtonDown {
                    button: Button::Start,
                    ..
                } if !modal_shown!() => plus_down = Some(Instant::now()),
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
                    Event::TextInput { ref text, .. } => {
                        open.text(text);
                        None
                    }
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
                    (Some(Answer::Accept), Asking::Condition(slot, line)) => {
                        match machine.set_condition(slot, line, open.field()) {
                            Ok(()) => dialog = None,
                            Err(e) => toasts.push(format!("Condition: {e}")),
                        }
                    }
                    (Some(_), Asking::Notice) | (Some(Answer::Cancel), _) => dialog = None,
                    (None, _) => {}
                }
                continue;
            }
            if let (Some(open), false) = (&mut program_dialog, from_editor) {
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
            if let (Some(open), true) = (&mut side, editor_window) {
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
            // The help or the index beside the editor is over the panel.
            if let Event::MouseWheel {
                y,
                mouse_x,
                mouse_y,
                ..
            } = event
                && editor_window
                && debug_panel.contains(to_display(mouse_x, mouse_y))
            {
                let shown = debug_view(&mut machine, &debug_panel).variables.len();
                debug_panel.wheel(-y.round() as i32, shown);
                continue;
            }
            if let (Some(open), true) = (&mut debug_menu, editor_window) {
                match event {
                    Event::MouseMotion { x, y, .. } => open.hover(to_display(x, y)),
                    Event::MouseButtonDown {
                        mouse_btn, x, y, ..
                    } => {
                        let chosen = (mouse_btn == MouseButton::Left)
                            .then(|| open.click(to_display(x, y)))
                            .flatten();
                        debug_menu = None;
                        if let Some(choice) = chosen {
                            let panel = &mut debug_panel;
                            let size = (display_w, display_h);
                            let asks =
                                choose(choice, &mut machine, panel, &clipboard, &mut toasts, size);
                            dialog = asks.or(dialog);
                        }
                        continue;
                    }
                    Event::KeyDown { .. } => {
                        debug_menu = None;
                        continue;
                    }
                    _ => {}
                }
            }
            if debug_panel.typing() && editor_window {
                match event {
                    Event::KeyDown {
                        keycode: Some(key), ..
                    } => {
                        if let Typed::Value(name, text) = debug_panel.key(key) {
                            set_value(&mut machine, &debug_panel, &name, &text, &mut toasts);
                        }
                        continue;
                    }
                    Event::TextInput { text, .. } => {
                        debug_panel.text(&text);
                        continue;
                    }
                    _ => {}
                }
            }
            if let (Some(open), true) = (&mut side, side_focus && editor_window) {
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
                    if ui_window {
                        menu.mouse_move(Some(to_display(x, y)));
                    }
                    if editor_window {
                        debug_panel.pointer(Some(to_display(x, y)));
                    }
                    if dragging && on_editor {
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
                } => {
                    if ui_window {
                        menu.mouse_move(None);
                    }
                    if editor_window {
                        debug_panel.pointer(None);
                    }
                }
                // A right click on the editor or the debugger's panel opens a
                // menu of what can be done there.
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Right,
                    x,
                    y,
                    ..
                } if on_editor
                    && !side
                        .as_ref()
                        .is_some_and(|s| s.frame().contains(to_display(x, y))) =>
                {
                    let at = to_display(x, y);
                    let items = if debug_panel.contains(at) {
                        panel_menu(&mut machine, &debug_panel, at)
                    } else {
                        let (x, y) = to_screen(x, y);
                        if !machine.editor().has_selection() {
                            machine.editor_mouse_down(x, y, false);
                        }
                        editor_menu(&mut machine)
                    };
                    debug_menu = Some(Popup::new(at, items, (display_w, display_h)));
                }
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    clicks,
                    ..
                } => match if ui_window {
                    menu.click(to_display(x, y), menu_context(&machine))
                } else {
                    Click::Ignored
                } {
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
                    Click::Chosen(
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
                        | Action::DebugPanel),
                    ) => {
                        let size = (display_w, display_h);
                        dialog = debug_action(&mut machine, action, &mut toasts, size).or(dialog);
                    }
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
                    Click::Chosen(Action::SwitchFocus) => {
                        machine.focus_sub(!machine.sub_has_focus());
                    }
                    Click::Chosen(Action::StopSub) => machine.stop_sub(),
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
                            && editor_window
                            && open.frame().contains(to_display(x, y)) =>
                    {
                        side_focus = true;
                        if let Side::Index(index, _) = open {
                            let outcome = index.click(to_display(x, y));
                            side_outcome(&mut side, &mut side_focus, &mut machine, outcome);
                        }
                    }
                    Click::Ignored if on_editor && debug_panel.contains(to_display(x, y)) => {
                        match debug_panel.click(to_display(x, y), clicks == 2) {
                            Some(Hit::Call(i)) => {
                                if let Some(call) = machine.calls().get(i) {
                                    machine.show_line(call.slot, call.line);
                                }
                            }
                            Some(Hit::Close) => machine.show_panel(false),
                            Some(Hit::Variable(i)) if clicks == 2 => {
                                let panel = &mut debug_panel;
                                let _ = choose(
                                    Choice::ChangeValue(i),
                                    &mut machine,
                                    panel,
                                    &clipboard,
                                    &mut toasts,
                                    (display_w, display_h),
                                );
                            }
                            _ => {}
                        }
                    }
                    Click::Ignored if on_editor => {
                        side_focus = false;
                        let (x, y) = to_screen(x, y);
                        if !machine.editor_margin_click(x, y) {
                            let extend = shift(keyboard.mod_state());
                            machine.editor_mouse_down(x, y, extend);
                            dragging = true;
                        }
                    }
                    Click::Consumed | Click::Ignored => {}
                },
                Event::KeyDown {
                    keycode: Some(Keycode::Pause),
                    ..
                } => machine.stop(),
                // The debugger's keys, around F5's.
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    ..
                } if let Some(action) = debug_key(key, shift(keymod), ctrl(keymod)) => {
                    let size = (display_w, display_h);
                    dialog = debug_action(&mut machine, action, &mut toasts, size).or(dialog);
                }
                // SmileBASIC 4's F5 is the + button.
                Event::KeyDown {
                    keycode: Some(Keycode::F5),
                    keymod,
                    repeat: false,
                    ..
                } if !ctrl(keymod) => plus_down = Some(Instant::now()),
                // F8 leaves F9 to F11 for SmileBASIC 4's tools.
                Event::KeyDown {
                    keycode: Some(Keycode::F8),
                    repeat: false,
                    ..
                } => menu.shown = !menu.shown,
                // A paused program takes no keys.
                Event::KeyDown { .. } | Event::TextInput { .. }
                    if menu.shown && machine.is_running() && !on_editor => {}
                // Ctrl+Space hides completion's list and the hints, or
                // shows them again.
                Event::KeyDown {
                    keycode: Some(Keycode::Space),
                    keymod,
                    ..
                } if ctrl(keymod) && (on_editor || machine.rt.wants_line()) => {
                    if on_editor {
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
                } if !on_editor
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
                    && !on_editor
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
                } if ctrl(keymod) && !on_editor && machine.rt.wants_line() => {
                    paste(&mut machine, &clipboard, &mut toasts)
                }
                Event::KeyDown {
                    keycode: Some(Keycode::C),
                    keymod,
                    ..
                } if machine.rt.settings.ctrl_c_stops && ctrl(keymod) && !on_editor => {
                    machine.stop()
                }
                // Ctrl+Shift+V pastes everywhere, also as keys to programs.
                Event::KeyDown {
                    keycode: Some(Keycode::V),
                    keymod,
                    ..
                } if ctrl(keymod) && shift(keymod) => paste(&mut machine, &clipboard, &mut toasts),
                // From the screen, F4 brings the editor's own window up.
                Event::KeyDown {
                    keycode: Some(Keycode::F4),
                    ..
                } if apart && !from_editor => {
                    if let Some(gfx) = editor_gfx.get() {
                        gfx.canvas.borrow_mut().window_mut().raise();
                    }
                }
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
                } if on_editor => next_editor_size(&mut machine),
                // F1 shows the help for the command at the cursor.
                Event::KeyDown {
                    keycode: Some(Keycode::F1),
                    repeat: false,
                    ..
                } if on_editor => {
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
                } if ctrl(keymod) && (!machine.is_running() || on_editor && apart) => {
                    let size = (display_w, display_h);
                    file_dialog = Some(open_file_dialog(&machine, key == Keycode::S, size));
                }
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    ..
                } if ctrl(keymod)
                    && (!machine.is_running() || on_editor && apart)
                    && slot_key(key).is_some() =>
                {
                    machine.edit_slot(slot_key(key).expect("checked"));
                }
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    ..
                } if on_editor && ctrl(keymod) => match key {
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
                } if side.is_some() && on_editor && machine.edit().mode() == Mode::Edit => {
                    side = None;
                }
                Event::MouseWheel { y, .. } if on_editor => {
                    machine.edit().wheel(-3 * y.round() as i32);
                }
                // F3 finds, and in finding goes to the next match.
                Event::KeyDown {
                    keycode: Some(Keycode::F3),
                    keymod,
                    ..
                } if on_editor => {
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
                } if on_editor && shift(keymod) && machine.edit().mode() == Mode::Edit => {
                    machine.edit().unindent()
                }
                // Shift and the cursor keys select in the editor.
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    ..
                } if on_editor && shift(keymod) && special_key(key).is_some_and(is_movement) => {
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

        // Opened or closed since its window was shown or hidden above, the
        // editor shows where it did until the next frame.
        let apart = apart && machine.is_editing();
        let ui_editor = ui_editor && apart;
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
        if plus_down.is_some_and(|t| t.elapsed() >= FORCED_STOP) {
            plus_down = None;
            machine.stop();
        }
        let paused = menu.shown && machine.is_running();
        menu.status = status_line(&machine, paused);
        machine.focus_editor(editor_focused);
        machine.rt.input.live = if modal || paused || machine.editor_has_keys() {
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
        machine.set_changes(&debug_panel.change_watches());
        if !paused {
            machine.time_limit = Some(RUN_TIME);
            machine.frame();
        }
        audio.play(&machine.take_samples());
        // Behind by a frame or more, as when drawing takes too long: the
        // program and its sound catch up, and only the last of these
        // frames is drawn, so that the game and its music keep time.
        // Those frames run code briefly, so that catching up does not
        // fall further behind.
        let mut skipped = 0;
        while !paused && skipped < MOST_SKIPPED && Instant::now() >= next_frame + FRAME {
            machine.time_limit = Some(CATCH_UP_RUN_TIME);
            machine.frame();
            audio.play(&machine.take_samples());
            next_frame += FRAME;
            skipped += 1;
        }
        // A pointer the program moved with MOUSE stays there until the
        // real one moves, and the real one goes there too.
        let (sent, now) = (&mut controls.mouse, machine.rt.input.live.mouse);
        let fed = !modal && !paused && !machine.editor_has_keys();
        if fed && (sent.x, sent.y, sent.display) != (now.x, now.y, now.display) {
            let px = out_w as f32 / win_w.max(1) as f32;
            let (x, y) = if (sent.x, sent.y) != (now.x, now.y) {
                (
                    shown.x + (now.x as f32 + 0.5) * shown.w / pointer_space.0 as f32,
                    shown.y + (now.y as f32 + 0.5) * shown.h / pointer_space.1 as f32,
                )
            } else {
                let scale = display.w / display_w as f32;
                (
                    display.x + (now.display.0 as f32 + 0.5) * scale,
                    display.y + (now.display.1 as f32 + 0.5) * scale,
                )
            };
            mouse.warp_mouse_in_window(canvas.window(), x / px, y / px);
            (sent.x, sent.y, sent.display) = (now.x, now.y, now.display);
        }
        // The editor leaves its top row for the menu bar.
        menu.pinned = machine.is_editing() && (!machine.is_detached() || ui_editor);
        menu.running = machine.is_running();
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
        let editing = machine.is_editing() && !machine.is_detached();
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
        // The editor in its own window, where the debugger's panel goes
        // too: where its frame and display are there.
        let mut editor_place = (rect, frame, display);
        if let (true, Some(gfx)) = (apart, editor_gfx.get()) {
            let editor_canvas = gfx.canvas.borrow();
            let (out_w, out_h) = editor_canvas.output_size()?;
            let display = display_rect(display_w, display_h, out_w, out_h);
            let mode = machine.editor_mode();
            let filter = machine.rt.settings.screen_filter.unwrap_or(mode.filter);
            let rect = screen_rect(&mode, filter, out_w, display.h);
            let fb = machine.render_editor();
            let frame = (fb.width(), fb.height());
            editor_space = frame;
            let kind = (frame, editor_filter(rect, frame, filter));
            if editor_texture.as_ref().is_none_or(|(_, k)| *k != kind) {
                let made = frame_texture(&editor_canvas, &gfx.creator, kind)?;
                editor_texture = Some((made, kind));
            }
            if let Some((tex, _)) = &mut editor_texture {
                tex.update(None, as_bytes(fb.pixels()), (frame.0 * 4) as usize)?;
            }
            editor_place = (rect, frame, display);
        }
        // The debugger's panel fills the room the editor leaves below.
        let mut debug = None;
        if let Some(at) = machine.panel_at() {
            let (rect, frame, display) = editor_place;
            let top = (rect.y + at as f32 * rect.h / frame.1 as f32) * display_h as f32 / display.h;
            let top = (top.round() as u32).min(display_h);
            let place = ui::Rect {
                x: 0,
                y: top,
                w: display_w,
                h: display_h - top,
            };
            // A stop, though at the same place, shows the panel anew.
            let fresh = machine.paused_at() != shown_stop || !panel_was_shown;
            if fresh {
                shown_stop = machine.paused_at();
                debug_panel.reset();
            }
            // The screens change only while a step runs. Beside the
            // editor's own window, they show in the main one.
            if !apart && (fresh || machine.is_running() || panel_screens.is_none()) {
                let screens = machine.render_screens();
                let size = (screens.width(), screens.height());
                let shown = DebugPanel::screen_place(place, size);
                let filter = match shown.h as f32 * display.h / display_h as f32 >= size.1 as f32 {
                    true => Filter::Nearest,
                    false => Filter::Bilinear,
                };
                if panel_screens
                    .as_ref()
                    .is_none_or(|(_, kind)| *kind != (size, filter))
                {
                    let made = frame_texture(&canvas, &texture_creator, (size, filter))?;
                    panel_screens = Some((made, (size, filter)));
                }
                if let Some((tex, _)) = &mut panel_screens {
                    let pixels = as_bytes(screens.pixels());
                    tex.update(None, pixels, (size.0 * 4) as usize)?;
                }
            }
            let size = panel_screens
                .as_ref()
                .map_or((1, 1), |(_, (size, _))| *size);
            let view = debug_view(&mut machine, &debug_panel);
            debug = Some((place, view, (!apart).then_some(size)));
        }
        panel_was_shown = debug.is_some();
        canvas.set_draw_color(Color::BLACK);
        canvas.clear();
        canvas.copy(&texture, None, rect)?;
        // The subprogram's screen goes where XSUBSCREEN puts it on the
        // 1280x720 display.
        if !editing && let Some((fb, [x, y, w, h])) = machine.sub_screen() {
            let kind = ((fb.width(), fb.height()), filter);
            if sub_texture.as_ref().is_none_or(|(_, k)| *k != kind) {
                let made = frame_texture(&canvas, &texture_creator, kind)?;
                // SAFETY: the texture is valid while `made` lives.
                unsafe { SDL_SetTextureBlendMode(made.raw(), SDL_BLENDMODE_BLEND_PREMULTIPLIED) };
                sub_texture = Some((made, kind));
            }
            if let Some((tex, _)) = &mut sub_texture {
                tex.update(None, as_bytes(fb.pixels()), (fb.width() * 4) as usize)?;
                let (sx, sy) = (display.w / display_w as f32, display.h / display_h as f32);
                let dst = FRect::new(
                    display.x + x as f32 * sx,
                    display.y + y as f32 * sy,
                    w as f32 * sx,
                    h as f32 * sy,
                );
                canvas.copy(tex, None, dst)?;
            }
        }
        // Where the focus has just gone: a yellow line on the edge, one
        // pixel out and two in, then a dark band two pixels wide, fading
        // out together.
        if !editing && let Some(([x, y, w, h], alpha)) = machine.focus_border() {
            let (sx, sy) = (display.w / display_w as f32, display.h / display_h as f32);
            let place = |x: i32, y: i32, w: i32, h: i32| {
                let at = |v: i32, s: f32| v as f32 * s;
                FRect::new(
                    display.x + at(x, sx),
                    display.y + at(y, sy),
                    at(w, sx),
                    at(h, sy),
                )
            };
            canvas.set_blend_mode(BlendMode::Blend);
            for (inset, width, color) in [
                (-1, 3, Color::RGBA(255, 255, 0, alpha)),
                (2, 2, Color::RGBA(0, 0, 0, alpha)),
            ] {
                let (x, y, w, h) = (x + inset, y + inset, w - 2 * inset, h - 2 * inset);
                canvas.set_draw_color(color);
                canvas.fill_rects(&[
                    place(x, y, w, width),
                    place(x, y + h - width, w, width),
                    place(x, y + width, width, h - 2 * width),
                    place(x + w - width, y + width, width, h - 2 * width),
                ])?;
            }
            canvas.set_blend_mode(BlendMode::None);
        }
        // The debugger's panel leaves the screens' place clear.
        if let (Some((place, _, Some(size))), Some((tex, _))) = (&debug, &panel_screens) {
            let s = DebugPanel::screen_place(*place, *size);
            let scale = display.w / display_w as f32;
            let at = |v: u32| v as f32 * scale;
            let dst = FRect::new(display.x + at(s.x), display.y + at(s.y), at(s.w), at(s.h));
            canvas.copy(tex, None, dst)?;
        }
        // The colors window leaves the editor behind it bright, to show
        // its colors.
        let dims = dialog.is_some() || manager.is_some();
        // A running program shows the mouse cursor only if it asks to,
        // unless the emulator's own screens or the editor's window need
        // it.
        let show_cursor = !machine.is_running()
            || modal
            || dims
            || program_dialog.is_some()
            || menu.visible()
            || apart && pointer_in_editor
            || machine.rt.settings.keep_mouse_cursor
            || machine.rt.mouse_cursor();
        if show_cursor != cursor_shown {
            cursor_shown = show_cursor;
            mouse.show_cursor(show_cursor);
        }
        // Which overlay each part goes on: the main window's, or the
        // editor's window's. The menu bar and the emulator's windows go
        // with the keys, the editor's panels and menu with the editor.
        let ui = usize::from(ui_editor);
        let ed = usize::from(apart);
        let mut used = [false; 2];
        used[ui] |= modal || dims || menu.visible() || toasts.visible();
        used[ed] |= debug.is_some() || debug_menu.is_some() || side.is_some();
        used[0] |= program_dialog.is_some() || key_hint.visible();
        // The editor's window always shows the menu bar.
        used[1] |= apart;
        let mut overlays = [&mut overlay, &mut editor_overlay];
        for (i, fb) in overlays.iter_mut().enumerate() {
            // A dialog or the project manager dims the screen behind it,
            // as a program's DIALOG does.
            let dim = i == ui && dims || i == 0 && program_dialog.is_some();
            if used[i] {
                fb.clear(if dim { 0x8000_0000 } else { 0 });
            }
        }
        let font = machine.rt.font();
        ui::set_font(machine.rt.settings.ui_font);
        let cursor_on = (blink / 25).is_multiple_of(2);
        if let Some(open) = &manager {
            open.draw(overlays[ui], font, cursor_on);
        }
        match &debug {
            Some((place, view, size)) => {
                let scale = machine.rt.settings.debug_text_size;
                debug_panel.draw(overlays[ed], font, *place, *size, view, scale)
            }
            None => debug_panel.hide(),
        }
        match &side {
            Some(Side::Help(help)) => help.draw(overlays[ed], font, side_focus),
            Some(Side::Index(index, _)) => index.draw(overlays[ed], font, side_focus),
            None => {}
        }
        if let Some(open) = &program_dialog {
            open.draw(overlays[0], font, cursor_on);
        }
        if let Some(open) = &color_window {
            open.draw(overlays[ui], font, &machine.rt.settings.editor_colors);
        }
        if let Some(open) = &settings_window {
            open.draw(overlays[ui], font, &machine.rt.settings);
        }
        if let Some(open) = &shortcuts_window {
            open.draw(overlays[ui], font);
        }
        match &file_dialog {
            Some(FileDialog::Save(d)) => d.draw(overlays[ui], font, cursor_on),
            Some(FileDialog::Load(p)) => p.draw(overlays[ui], font, cursor_on),
            None => {}
        }
        if let Some(open) = &debug_menu {
            open.draw(overlays[ed], font);
        }
        if key_hint.visible() {
            key_hint.draw(overlays[0], font);
        }
        toasts.draw(overlays[ui], font);
        let context = menu_context(&machine);
        if let Some((open, _)) = &dialog {
            open.draw(overlays[ui], font);
        } else if !modal && menu.visible() {
            menu.draw(overlays[ui], font, &machine.rt.settings, context);
        }
        if apart && ui == 0 {
            let bar = menu.closed();
            bar.draw(overlays[1], font, &machine.rt.settings, context);
        }
        if used[0] {
            overlay_texture.update(None, as_bytes(overlay.pixels()), (display_w * 4) as usize)?;
            canvas.copy(&overlay_texture, None, display)?;
        }
        canvas.present();
        if let (true, Some(gfx)) = (apart, editor_gfx.get()) {
            let mut editor_canvas = gfx.canvas.borrow_mut();
            editor_canvas.set_draw_color(Color::BLACK);
            editor_canvas.clear();
            if let Some((tex, _)) = &editor_texture {
                editor_canvas.copy(tex, None, editor_place.0)?;
            }
            if let Some(tex) = &mut editor_overlay_texture {
                let pixels = as_bytes(editor_overlay.pixels());
                tex.update(None, pixels, (display_w * 4) as usize)?;
                editor_canvas.copy(tex, None, editor_place.2)?;
            }
            editor_canvas.present();
        }
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

/// The editor's own window, with its renderer.
struct EditorWindow {
    canvas: RefCell<Canvas<Window>>,
    creator: TextureCreator<WindowContext>,
    id: u32,
}

impl EditorWindow {
    /// Makes the window, hidden until the editor opens: beside the main
    /// window if the display has room on one of its sides, else where the
    /// system puts it.
    fn open(
        video: &VideoSubsystem,
        (w, h): (u32, u32),
        main: &Window,
    ) -> Result<EditorWindow, AnyError> {
        const GAP: i32 = 16;
        let mut builder = video.window(EDITOR_TITLE, w, h);
        let ((x, y), (main_w, _)) = (main.position(), main.size());
        if let Ok(room) = main.get_display().and_then(|d| d.get_usable_bounds()) {
            let right = x + main_w as i32 + GAP;
            let left = x - GAP - w as i32;
            if right + w as i32 <= room.x() + room.width() as i32 {
                builder.position(right, y);
            } else if left >= room.x() {
                builder.position(left, y);
            }
        }
        let window = builder.resizable().high_pixel_density().hidden().build()?;
        video.text_input().start(&window);
        let id = window.id();
        let canvas = window.into_canvas();
        let creator = canvas.texture_creator();
        Ok(EditorWindow {
            canvas: RefCell::new(canvas),
            creator,
            id,
        })
    }
}

/// How a window's pointer positions map to the display's pixels and to
/// those of the frame shown in `shown`, which has `space` pixels.
#[derive(Clone, Copy)]
struct Mapping {
    /// Pixels to a window coordinate.
    px: f32,
    display: FRect,
    display_w: u32,
    shown: FRect,
    space: (u32, u32),
}

impl Mapping {
    fn to_display(self, x: f32, y: f32) -> (i32, i32) {
        let scale = self.display.w / self.display_w as f32;
        (
            ((x * self.px - self.display.x) / scale) as i32,
            ((y * self.px - self.display.y) / scale) as i32,
        )
    }

    fn to_screen(self, x: f32, y: f32) -> (i32, i32) {
        let (shown, (w, h)) = (self.shown, self.space);
        (
            ((x * self.px - shown.x) * w as f32 / shown.w).floor() as i32,
            ((y * self.px - shown.y) * h as f32 / shown.h).floor() as i32,
        )
    }
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
    /// The condition of a breakpoint, by slot and 1-based line.
    Condition(usize, u32),
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
        paused: machine.paused_at().is_some() && !machine.is_running(),
        detached: machine.is_detached(),
        sub: machine.sub_running(),
    }
}

/// The debugger's keys: Shift+F5 continues and Ctrl+Shift+F5 restarts;
/// F6 steps into, F7 over and Shift+F6 out; Ctrl+F6 runs to the cursor
/// and Ctrl+Shift+F6 goes on from it; Shift+F7 sets a breakpoint.
fn debug_key(key: Keycode, shift: bool, ctrl: bool) -> Option<Action> {
    Some(match (key, shift, ctrl) {
        (Keycode::F5, true, false) => Action::Continue,
        (Keycode::F5, true, true) => Action::Restart,
        (Keycode::F6, false, false) => Action::StepInto,
        (Keycode::F6, true, false) => Action::StepOut,
        (Keycode::F6, false, true) => Action::RunToCursor,
        (Keycode::F6, true, true) => Action::JumpToCursor,
        (Keycode::F7, false, false) => Action::StepOver,
        (Keycode::F7, true, false) => Action::ToggleBreakpoint,
        _ => return None,
    })
}

/// Does one of the debugger's commands, or says why it cannot; a
/// command that asks something gives the dialog to show.
#[must_use]
fn debug_action(
    machine: &mut Machine,
    action: Action,
    toasts: &mut Toasts,
    size: (u32, u32),
) -> Option<(Dialog, Asking)> {
    use sb_core::vm::Step;
    use sb_runtime::machine::Resume;
    let cursor = machine
        .is_editing()
        .then(|| (machine.editor().slot(), machine.editor().cursor().0 as u32));
    let resume = match action {
        Action::Continue => Resume::Cont,
        Action::StepInto => Resume::Step(Step::Into),
        Action::StepOver => Resume::Step(Step::Over),
        Action::StepOut => Resume::Step(Step::Out),
        Action::RunToCursor => match cursor {
            Some((slot, line)) => Resume::RunTo(slot, line),
            None => return None,
        },
        Action::JumpToCursor => {
            let here = cursor.zip(machine.paused_at());
            if !here.is_some_and(|((slot, line), at)| slot == at.0 && machine.jump_to(line)) {
                toasts.push("Can only go on from a line of the stopped DEF".to_string());
            }
            return None;
        }
        Action::ToggleBreakpoint => {
            if let Some((slot, line)) = cursor {
                machine.toggle_breakpoint(slot, line);
            }
            return None;
        }
        Action::BreakpointCondition => {
            let (slot, line) = cursor?;
            let text = machine.condition(slot, line).unwrap_or_default();
            let lines = vec![
                format!("Stop at {slot}:{line} only when this is true."),
                "Leave it empty to stop always.".to_string(),
            ];
            let open = Dialog::prompt("Breakpoint condition", lines, &text, size);
            return Some((open, Asking::Condition(slot, line)));
        }
        Action::ClearBreakpoints => {
            machine.clear_breakpoints();
            return None;
        }
        Action::DebugPanel => {
            machine.show_panel(!machine.panel_open());
            return None;
        }
        Action::Restart => {
            if !machine.restart() {
                toasts.push("Nothing has run yet".to_string());
            }
            return None;
        }
        _ => return None,
    };
    if !machine.debug(resume) {
        let why = match resume {
            Resume::RunTo(slot, _) if machine.lines_moved(slot) => {
                "The stopped program's lines have moved: run it again"
            }
            _ if machine.is_running() => "The program is running",
            _ if machine.stopped_by_error() => "A program stopped by an error cannot go on",
            _ => "No stopped program: set a breakpoint or use STOP",
        };
        toasts.push(why.to_string());
    }
    None
}

/// What the debugger's panel shows of the stopped program.
fn debug_view(machine: &mut Machine, panel: &DebugPanel) -> View {
    let calls = machine.calls();
    let call = calls.get(panel.chosen_call(calls.len())).cloned();
    let home = call.as_ref().map_or(machine.edited_slot(), |c| c.slot);
    let mut tabs = Vec::new();
    if call.as_ref().is_some_and(|c| c.frame.is_some()) {
        tabs.push(Tab::Locals);
    }
    tabs.push(Tab::Globals(home));
    for slot in (0..SLOTS).filter(|&s| s != home) {
        if !machine.slot_globals(slot).is_empty() {
            tabs.push(Tab::Globals(slot));
        }
    }
    let tab = panel.tab.filter(|t| tabs.contains(t)).unwrap_or(tabs[0]);
    let call_frame = call.as_ref().and_then(|c| c.frame);
    let (frame, slot) = match tab {
        Tab::Locals => (call_frame, home),
        Tab::Globals(slot) => (None, slot),
    };
    let variables = match (tab, frame) {
        (Tab::Locals, Some(frame)) => machine.frame_locals(frame),
        (Tab::Globals(slot), _) => machine.slot_globals(slot),
        (Tab::Locals, None) => Vec::new(),
    };
    // Watches see the chosen call's locals and its slot's globals.
    let watches = (panel.watches.iter())
        .map(|w| {
            machine
                .watch(w, call_frame, home)
                .map_err(|e| e.to_string())
        })
        .collect();
    let (status, status_failed, keys) = match machine.paused_at() {
        _ if machine.is_running() => ("Running... Pause stops it".to_string(), false, ""),
        Some((slot, line)) => {
            let (mut status, failed) = stop_text(machine.stop_reason(), slot, line);
            if machine.lines_moved(slot) {
                status.push_str(" - edited since RUN, the old code goes on");
            }
            (status, failed, "F6/F7 step  Shift+F5 go on")
        }
        None => (String::new(), false, ""),
    };
    let changed = match machine.stop_reason() {
        Some(StopReason::Changed { watch, .. }) => Some(watch.clone()),
        _ => None,
    };
    View {
        calls,
        tabs,
        variables,
        frame,
        slot,
        watches,
        status,
        status_failed,
        keys: keys.to_string(),
        changed,
    }
}

/// Why and where the program stopped, as the debugger's panel says it,
/// and whether it was a failure.
fn stop_text(reason: Option<&StopReason>, slot: usize, line: u32) -> (String, bool) {
    let at = format!("{slot}:{line}");
    let text = match reason {
        Some(StopReason::Paused) => format!("Paused at {at}"),
        Some(StopReason::Stop) => format!("STOP at {at}"),
        Some(StopReason::Traced) => format!("TRACE at {at}"),
        Some(StopReason::Stepped) => format!("Stepped to {at}"),
        Some(StopReason::RanTo) => format!("Ran to {at}"),
        Some(StopReason::Breakpoint(None)) => format!("Breakpoint at {at}"),
        Some(StopReason::Breakpoint(Some(c))) => format!("Breakpoint at {at} when {c}"),
        Some(StopReason::Changed { watch, from, to }) => {
            format!("{watch} changed {from} -> {to}, at {at}")
        }
        Some(StopReason::ConditionFailed(e)) => {
            return (format!("Condition failed at {at}: {e}"), true);
        }
        None => format!("Stopped at {at}"),
    };
    (text, false)
}

/// What a right click's menu on the editor or the debugger's panel can do.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Choice {
    /// Type a new value for a variable shown, by its place in the list.
    ChangeValue(usize),
    CopyValue(String),
    Watch(String),
    EditWatch(usize),
    RemoveWatch(usize),
    /// Stop the program when a watch's value changes, or not.
    BreakOnChange(usize),
    AddWatch,
    Hex,
    /// Go to the label or DEF of a name.
    Definition(String),
    Debug(Action),
    Cut,
    Copy,
    Paste,
}

/// A right click's menu on the debugger's panel: for a variable, a
/// watch, or the panel.
fn panel_menu(
    machine: &mut Machine,
    panel: &DebugPanel,
    at: (i32, i32),
) -> Vec<(String, bool, Choice)> {
    let hex = (
        format!("{} Hexadecimal", if panel.hex { "[x]" } else { "[ ]" }),
        true,
        Choice::Hex,
    );
    let view = debug_view(machine, panel);
    let stopped = !machine.is_running();
    match panel.hit_at(at) {
        Some(Hit::Variable(i)) if i < view.variables.len() => {
            let (name, value) = view.variables[i].clone();
            let copy = value_text(&value, usize::MAX, panel.hex).0;
            vec![
                ("Change value".to_string(), stopped, Choice::ChangeValue(i)),
                ("Copy value".to_string(), true, Choice::CopyValue(copy)),
                (format!("Watch {name}"), true, Choice::Watch(name)),
                hex,
            ]
        }
        Some(Hit::Watch(i)) => {
            let copy = match view.watches.get(i) {
                Some(Ok(value)) => Some(value_text(value, usize::MAX, panel.hex).0),
                _ => None,
            };
            let breaks = if panel.breaks(i) { "[x]" } else { "[ ]" };
            vec![
                ("Edit watch".to_string(), true, Choice::EditWatch(i)),
                ("Remove watch".to_string(), true, Choice::RemoveWatch(i)),
                (
                    format!("{breaks} Break on change"),
                    true,
                    Choice::BreakOnChange(i),
                ),
                (
                    "Copy value".to_string(),
                    copy.is_some(),
                    Choice::CopyValue(copy.unwrap_or_default()),
                ),
                hex,
            ]
        }
        _ => vec![("Add watch".to_string(), true, Choice::AddWatch), hex],
    }
}

/// A right click's menu on the editor: the name at the cursor, the
/// debugger's commands for its line, and the clipboard.
fn editor_menu(machine: &mut Machine) -> Vec<(String, bool, Choice)> {
    let paused = menu_context(machine).paused;
    let mut items = Vec::new();
    if let Some(word) = machine.editor().word_or_selection() {
        let short: String = word.chars().take(20).collect();
        let defined = (machine.editor().index().iter()).any(|(_, d)| d.eq_ignore_ascii_case(&word));
        items.push((
            format!("Go to {short}"),
            defined,
            Choice::Definition(word.clone()),
        ));
        items.push((format!("Watch {short}"), true, Choice::Watch(word)));
    }
    items.extend([
        (
            "Breakpoint".to_string(),
            true,
            Choice::Debug(Action::ToggleBreakpoint),
        ),
        (
            "Breakpoint condition...".to_string(),
            true,
            Choice::Debug(Action::BreakpointCondition),
        ),
        (
            "Run to cursor".to_string(),
            paused,
            Choice::Debug(Action::RunToCursor),
        ),
        (
            "Go on from cursor".to_string(),
            paused,
            Choice::Debug(Action::JumpToCursor),
        ),
        (
            "Cut".to_string(),
            machine.editor().has_selection(),
            Choice::Cut,
        ),
        (
            "Copy".to_string(),
            machine.editor().has_selection(),
            Choice::Copy,
        ),
        ("Paste".to_string(), true, Choice::Paste),
    ]);
    items
}

/// Does what was chosen in a right click's menu, giving the dialog to
/// show for one that asks something.
#[must_use]
fn choose(
    choice: Choice,
    machine: &mut Machine,
    panel: &mut DebugPanel,
    clipboard: &ClipboardUtil,
    toasts: &mut Toasts,
    size: (u32, u32),
) -> Option<(Dialog, Asking)> {
    match choice {
        Choice::ChangeValue(i) => {
            let view = debug_view(machine, panel);
            if let Some((name, value)) = view.variables.get(i) {
                panel.edit_variable(name.clone(), value_text(value, usize::MAX, false).0);
            }
        }
        Choice::CopyValue(text) => {
            if let Err(e) = clipboard.set_clipboard_text(&text) {
                toasts.push(format!("Could not copy: {e}"));
            }
        }
        Choice::Watch(text) => panel.watches.push(text),
        Choice::EditWatch(i) => panel.edit_watch(i),
        Choice::RemoveWatch(i) => panel.remove_watch(i),
        Choice::BreakOnChange(i) => panel.toggle_break(i),
        Choice::AddWatch => panel.add_watch(String::new()),
        Choice::Hex => panel.hex = !panel.hex,
        Choice::Definition(name) => {
            if !machine.edit().go_to_definition(&name) {
                toasts.push(format!("No label or DEF named {name}"));
            }
        }
        Choice::Debug(action) => return debug_action(machine, action, toasts, size),
        Choice::Cut | Choice::Copy => {
            copy_selection(machine, clipboard, toasts, choice == Choice::Cut)
        }
        Choice::Paste => paste(machine, clipboard, toasts),
    }
    None
}

/// Gives a variable shown in the debugger's panel the value of what was
/// typed, worked out as a watch.
fn set_value(
    machine: &mut Machine,
    panel: &DebugPanel,
    name: &str,
    typed: &str,
    toasts: &mut Toasts,
) {
    let view = debug_view(machine, panel);
    match machine.watch(typed, view.frame, view.slot) {
        Ok(value) => {
            if !machine.set_variable(name, value, view.frame, view.slot) {
                toasts.push(format!("Could not change {name}"));
            }
        }
        Err(e) => toasts.push(format!("{name}: {e}")),
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

/// The size of a texture's frames and how they are scaled.
type TextureKind = ((u32, u32), Filter);

/// A texture for frames of `w` x `h` pixels, scaled with `filter`:
/// XSCREEN's, or the one chosen in the menu.
fn frame_texture<'a>(
    canvas: &Canvas<Window>,
    creator: &'a TextureCreator<WindowContext>,
    ((w, h), filter): TextureKind,
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

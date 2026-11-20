#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod bench;
mod colors;
mod controls;
mod corner;
mod debug_panel;
mod dialog;
mod draw;
mod editor_layout;
mod events;
mod file_dialogs;
mod file_view;
mod gpu;
mod gpu_golden;
mod gpu_project;
mod grp_view;
mod help;
mod index;
mod inputs;
mod keyhint;
mod layer;
mod menu;
mod overlays;
mod pacing;
mod pad_repeat;
mod placement;
mod pointer;
mod popup;
mod preview;
mod program_dialog;
mod projects;
mod recovery;
mod settings_window;
mod shortcuts;
mod simple_ui;
mod slow_hint;
mod standby;
mod stick_mouse;
mod text_field;
mod toast;
mod ui;
mod window;

use std::any::Any;
use std::cell::{Cell, OnceCell, RefCell};
use std::ffi::CStr;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use audio::Audio;
use bench::Stage;
use colors::ColorWindow;
use controls::Controls;
use debug_panel::{DebugPanel, Hit, Tab, View, value_text};
use dialog::Dialog;
use editor_layout::Layout;
use events::Taken;
use file_dialogs::{FilePicker, SaveDialog};
use grp_view::PageTexture;
use help::HelpViewer;
use index::IndexPanel;
use inputs::Inputs;
use keyhint::KeyHint;
use layer::Layer;
use menu::{Action, Click, Context, MenuBar};
use overlays::Overlays;
use placement::{Mapping, display_rect, from_corner, screen_place, windows_rect};
use popup::Popup;
use program_dialog::ProgramView;
use projects::{Memory, ProjectManager, Transfer};
use recovery::{OwnedCreator, Recovery, RendererWatch, SuspendWatch};
use sb_profiles::Profile;
use sb_runtime::confirm::Confirm;
use sb_runtime::editor::{self, Mode, Place};
use sb_runtime::font::Font;
use sb_runtime::help::{Help, Lookup};
use sb_runtime::machine::{Machine, StopReason};
use sb_runtime::palette::Palette;
use sb_runtime::raster::SceneBackend;
use sb_runtime::runtime::{SLOTS, keys};
use sb_runtime::screen::{Filter, ScreenMode};
use sb_runtime::settings::{Renderer, Settings};
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
use sdl3::keyboard::{Keycode, Mod, Scancode};
use sdl3::messagebox::{MessageBoxFlag, show_simple_message_box};
use sdl3::mouse::MouseButton;
use sdl3::pixels::{Color, PixelFormat};
use sdl3::rect::Rect;
use sdl3::render::{BlendMode, Canvas, FRect, ScaleMode, Texture, TextureCreator, create_renderer};
use sdl3::sys::blendmode::SDL_BLENDMODE_BLEND_PREMULTIPLIED;
use sdl3::sys::events::{
    SDL_EVENT_RENDER_DEVICE_LOST, SDL_EVENT_RENDER_DEVICE_RESET, SDL_EVENT_RENDER_TARGETS_RESET,
    SDL_Event, SDL_EventType, SDL_PushEvent,
};
use sdl3::sys::render::{
    SDL_GetNumRenderDrivers, SDL_GetRenderDriver, SDL_SetDefaultTextureScaleMode,
    SDL_SetRenderDrawBlendMode, SDL_SetTextureBlendMode,
};
use sdl3::sys::surface::SDL_ScaleMode;
use sdl3::video::{FullscreenType, Window, WindowBuilder, WindowContext, WindowPos};
use settings_window::SettingsWindow;
use shortcuts::ShortcutsWindow;
use simple_ui::{GameList, PauseMenu, Welcome};
use toast::Toasts;
use window::Which;

const USAGE: &str = "usage:
  smilebasic                    start in direct mode
  smilebasic PROGRAM            load a SmileBASIC 4 program (TXT file or plain
                                text) into PRG0 and run it; the folder that
                                holds it is the current project, so the
                                program loads and saves the files beside it
                                (HGLIB.PRG.txt, HGLIB.PRG or THGLIB.PRG are
                                all TXT:HGLIB.PRG)
  smilebasic --simple-ui [PROGRAM]
                                the same with the Simple UI, for this run only:
                                fullscreen, for a gamepad alone, starting in
                                a list of the projects

SmileBASIC 4's own font and sprites come from smilebasic-import; without
them, a bundled font and blank sprites are used";

const TITLE: &str = "SmileBASIC emulator";
const EDITOR_TITLE: &str = "SmileBASIC editor";
/// In the data directory.
const SETTINGS_FILE: &str = "settings.txt";
/// SmileBASIC runs at 60 frames a second.
const FRAME: Duration = Duration::from_nanos(1_000_000_000 / 60);
/// Holding the + button, or F5 for it, this long stops a program even
/// past ON BREAK GOTO (checked on hardware).
const FORCED_STOP: Duration = Duration::from_secs(1);
/// How long a window brought to the front for input in the other one has
/// to get the keys before it is flashed instead.
const RAISE_WAIT: Duration = Duration::from_millis(300);
type AnyError = Box<dyn std::error::Error>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match parse_args(&args) {
        Some((program, simple_ui)) => run(program.map(Path::new), simple_ui),
        None => Err(USAGE.into()),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        // Release builds on Windows have no console to print to.
        if !headless() {
            let _ = show_simple_message_box(MessageBoxFlag::ERROR, TITLE, &e.to_string(), None);
        }
        std::process::exit(1);
    }
}

/// The program to run and whether `--simple-ui` was given, from the
/// command line; `None` for anything else, which gets the usage.
fn parse_args(args: &[String]) -> Option<(Option<&str>, bool)> {
    let mut program = None;
    let mut simple_ui = false;
    for arg in args {
        match arg.as_str() {
            "--simple-ui" if !simple_ui => simple_ui = true,
            arg if arg.starts_with('-') => return None,
            arg if program.is_none() => program = Some(arg),
            _ => return None,
        }
    }
    Some((program, simple_ui))
}

/// Panics on frames in a row after which frames stop: the machine cannot be
/// put right, but the window can still save what is in the editor.
const MOST_PANICS: u32 = 3;

/// What keeps a panic in a program's frame from taking unsaved work with
/// it. A failed allocation aborts and cannot be caught, nor can a panic
/// outside of the frame, as when drawing.
#[derive(Debug, Default)]
struct FrameGuard {
    panics_in_a_row: u32,
    stopped: bool,
}

impl FrameGuard {
    /// Runs a frame. Costs nothing but a test and a store until a panic.
    #[inline]
    fn run(
        &mut self,
        machine: &mut Machine,
        toasts: &mut Toasts,
        frame: impl FnOnce(&mut Machine),
    ) {
        if self.stopped {
            return;
        }
        match catch_unwind(AssertUnwindSafe(|| frame(machine))) {
            Ok(()) => self.panics_in_a_row = 0,
            Err(panic) => self.panicked(machine, toasts, &*panic),
        }
    }

    #[cold]
    fn panicked(&mut self, machine: &mut Machine, toasts: &mut Toasts, panic: &(dyn Any + Send)) {
        let why = panic_text(panic);
        self.panics_in_a_row += 1;
        let recovered = catch_unwind(AssertUnwindSafe(|| machine.recover_from_panic(&why)));
        self.stopped = recovered.is_err() || self.panics_in_a_row >= MOST_PANICS;
        toasts.push(if self.stopped {
            "The emulator failed and has stopped: save your programs and restart it.".to_string()
        } else {
            format!("The emulator failed and stopped the program: {why}")
        });
    }
}

/// The version as Help > About and the Simple UI's pause menu show it:
/// [`sb_runtime::VERSION`], with the build's options after it, so that a
/// bug report shows a build with options, such as unchecked-vars.
fn version_with_options() -> String {
    match sb_runtime::BUILD_OPTIONS {
        "" => sb_runtime::VERSION.to_string(),
        options => format!("{} {options}", sb_runtime::VERSION),
    }
}

/// What Help > About says.
fn about_lines(drawn_by: &str) -> Vec<String> {
    vec![
        format!("SmileBASIC emulator {}", version_with_options()),
        "An emulator for SmileBASIC 4 programs.".to_string(),
        String::new(),
        "MIT OR Apache-2.0.".to_string(),
        "Bundled fonts: SIL Open Font License 1.1, or their authors' terms.".to_string(),
        "GeneralUser GS: its own license.".to_string(),
        String::new(),
        format!("Renderer: {drawn_by}"),
    ]
}

/// Whether there is nobody to show a message box to.
fn headless() -> bool {
    std::env::var("SDL_VIDEO_DRIVER").is_ok_and(|driver| driver == "offscreen")
}

/// What a panic said.
fn panic_text(panic: &(dyn Any + Send)) -> String {
    (panic.downcast_ref::<&str>().map(|s| s.to_string()))
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".to_string())
}

fn data_dir() -> Result<PathBuf, AnyError> {
    Ok(sdl3::filesystem::get_pref_path("", resources::DATA_DIR)?)
}

fn resource_dir() -> Result<PathBuf, AnyError> {
    Ok(data_dir()?.join(resources::RESOURCE_DIR))
}

fn load_source(path: &Path) -> Result<String, AnyError> {
    let bytes =
        (sb_runtime::storage::read_chosen(path)).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.starts_with(&[4, 0]) {
        let file = sbfile::parse(&bytes)?;
        Ok(file.text()?.to_owned())
    } else {
        Ok(String::from_utf8(bytes).map_err(|_| "program is not UTF-8 text")?)
    }
}

/// Makes the folder of the program given on the command line the current
/// project, and gives the name of the TXT file that the program is there,
/// if it maps to one.
fn open_program_folder(storage: &mut Storage, program: &Path) -> Option<String> {
    let dir = match program.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    if let Err(e) = storage.open_folder(dir) {
        eprintln!("{}: {e}", dir.display());
        return None;
    }
    let entry = program.file_name()?.to_str()?;
    match storage.folder_file(entry) {
        Some((sb_runtime::storage::FileKind::Txt, name)) => Some(name),
        _ => None,
    }
}

fn run(program: Option<&Path>, simple_ui_flag: bool) -> Result<(), AnyError> {
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
    let program_name = program.and_then(|p| open_program_folder(&mut rt.storage, p));
    let settings_path = data_dir()?.join(SETTINGS_FILE);
    if let Ok(text) = std::fs::read_to_string(&settings_path) {
        rt.settings = Settings::from_text(&text);
    }
    rt.apply_bundled_glyphs();
    // Completion and argument hints take the forms of the help's pages.
    rt.library = std::rc::Rc::new(sb_runtime::complete::Library::new(help.as_deref()));
    rt.help = help.clone();
    let saved_settings = rt.settings.clone();
    // The flag is for this run: the setting stays as it was saved.
    let simple_ui = simple_ui_flag || rt.settings.simple_ui;
    let mut machine = Machine::new(rt);
    machine.boot();
    let mut restart = None;
    if let Some(source) = source {
        // Loaded as LOAD would, it is named for Ctrl+S to save it again;
        // a program that maps to no name there runs all the same.
        let loaded = (program_name.as_ref()).is_some_and(|name| machine.load_slot(0, name).is_ok());
        if !loaded {
            machine.set_slot(0, source);
        }
        // With the Simple UI on it ends in the game list, after an error once
        // A is pressed, as a program run from there does.
        match program_name.filter(|_| loaded && simple_ui) {
            Some(name) if machine.run_home(&name).is_ok() => restart = Some(name),
            _ => machine.run_slot(0),
        }
    }
    let mut recovery = Recovery::new(data_dir()?);
    let start = Start {
        screen,
        bank,
        help,
        settings_path,
        saved_settings,
        simple_ui,
        restart,
    };
    let ended = catch_unwind(AssertUnwindSafe(|| {
        frontend(&mut machine, &mut recovery, start)
    }));
    // Whatever ended it, the machine can still be read, for its programs' text.
    let why = match ended {
        Ok(Ok(())) => return Ok(()),
        Ok(Err(e)) => e.to_string(),
        Err(panic) => format!("The emulator failed: {}", panic_text(&*panic)),
    };
    let saved = catch_unwind(AssertUnwindSafe(|| recovery.save(&machine)));
    Err(failure_message(&why, saved.ok().flatten()).into())
}

/// What to say when the emulator ends for a failure.
fn failure_message(why: &str, saved: Option<Vec<String>>) -> String {
    match saved {
        Some(lines) => format!("{why}\n\n{}", lines.join("\n")),
        None => why.to_string(),
    }
}

/// The emulator's own windows and dialogs. While one is open it takes the
/// input, and the program sees nothing pressed.
#[derive(Default)]
struct Screens {
    /// A question, which takes all input while it is open: before quitting
    /// with unsaved programs, or for a command such as LOAD.
    dialog: Option<(Dialog, Asking)>,
    /// Takes all input while it is open, except for a dialog over it.
    manager: Option<ProjectManager>,
    /// Ctrl+S's and Ctrl+L's windows.
    file_dialog: Option<FileDialog>,
    settings_window: Option<SettingsWindow>,
    shortcuts_window: Option<ShortcutsWindow>,
    color_window: Option<ColorWindow>,
    /// The Simple UI's game list, which takes all input but a dialog's
    /// while it is open, and its pause menu, which takes all input.
    game_list: Option<GameList>,
    pause_menu: Option<PauseMenu>,
    /// The screen shown the first time the Simple UI starts, over the
    /// list: it takes all input but a dialog's.
    welcome: Option<Welcome>,
}

impl Screens {
    /// Whether one of them is open now.
    fn any(&self) -> bool {
        self.dialog.is_some()
            || self.manager.is_some()
            || self.file_dialog.is_some()
            || self.settings_window.is_some()
            || self.shortcuts_window.is_some()
            || self.color_window.is_some()
            || self.game_list.is_some()
            || self.pause_menu.is_some()
            || self.welcome.is_some()
    }

    /// Shows the question a command of the program or of direct mode
    /// asks, on a display of `size`, and takes it away once the machine
    /// no longer asks it. Another dialog stays.
    fn follow_confirmation(&mut self, machine: &Machine, size: (u32, u32)) {
        match (&self.dialog, machine.confirmation()) {
            (Some((_, Asking::Command(shown))), Some(confirm)) if shown == confirm => {}
            (None | Some((_, Asking::Command(_))), Some(confirm)) => {
                let open = Dialog::ask(confirm, size);
                self.dialog = Some((open, Asking::Command(confirm.clone())));
            }
            (Some((_, Asking::Command(_))), None) => self.dialog = None,
            _ => {}
        }
    }

    /// Whether the menu bar, if `bar` is shown, the pause menu or the
    /// welcome screen holds the frames of a program that is `running`.
    fn pause(&self, bar: bool, running: bool) -> bool {
        (bar || self.pause_menu.is_some() || self.welcome.is_some()) && running
    }
}

/// What the frontend starts from.
struct Start {
    screen: sb_profiles::ScreenConfig,
    bank: Option<Arc<Bank>>,
    help: Option<std::rc::Rc<Help>>,
    settings_path: PathBuf,
    saved_settings: Settings,
    /// The Simple UI is on, by the setting or `--simple-ui`.
    simple_ui: bool,
    /// The program of the current project that runs from the command line
    /// with the Simple UI on, for the pause menu's Restart.
    restart: Option<String>,
}

/// The window and the main loop, on a machine that is already set up.
fn frontend(machine: &mut Machine, recovery: &mut Recovery, start: Start) -> Result<(), AnyError> {
    let Start {
        screen,
        bank,
        help,
        settings_path,
        mut saved_settings,
        mut simple_ui,
        restart,
    } = start;
    let sdl = sdl3::init()?;
    let video = sdl.video()?;
    // The UI's size: the profile's display on the desktop, or in
    // the Simple UI the output's at a whole scale.
    let profile_display = screen.display;
    let (mut display_w, mut display_h) = profile_display;
    // The offscreen driver of headless runs has a 1024x768 display, which
    // would change the size that runs are compared at.
    let usable = video
        .get_primary_display()
        .and_then(|d| d.get_usable_bounds());
    let usable = usable.ok().filter(|_| !headless());
    let usable = usable.map(|r| (r.width(), r.height()));
    // As large as when the emulator last quit, as View > Window size left it.
    let wanted = (machine.rt.settings.window_size).map_or(profile_display, |[w, h]| (w, h));
    let (window_w, window_h) = window::start_size(wanted, usable);
    let mut builder = video.window(TITLE, window_w, window_h);
    builder.position_centered().resizable().high_pixel_density();
    if simple_ui || machine.rt.settings.fullscreen {
        builder.fullscreen();
    }
    let window = builder.build()?;

    video.text_input().start(&window);
    let clipboard = video.clipboard();
    machine.rt.clipboard = Box::new(SystemClipboard::new(video.clipboard()));
    let keyboard = sdl.keyboard();
    let mut inputs = Inputs::new(Controls::new(&sdl)?);
    inputs.typing.upper_case = machine.rt.settings.upper_case_letters;
    let audio = Audio::open(&sdl);
    // Runs the frames while a call to SDL holds the loop up.
    let standby = standby::Standby::start(audio.sink());
    // The file dialog answers from its own thread.
    let (chosen_font, font_choice) = std::sync::mpsc::channel::<PathBuf>();
    // ...as does the one for the editor's extra font.
    let (chosen_extra_font, extra_font_choice) = std::sync::mpsc::channel::<PathBuf>();
    // ...as do the editor colors window's, with whether to save there.
    let (chosen_colors, colors_choice) = std::sync::mpsc::channel::<(PathBuf, bool)>();
    // ...and the project manager's, for importing and exporting.
    let (chosen_folder, folder_choice) = std::sync::mpsc::channel::<(PathBuf, Transfer)>();
    let mut canvas = make_canvas(&window, &[])?;
    // The window outlives each renderer made for it, for what is kept of
    // it at the end.
    let main_window = window;
    if simple_ui {
        let size = machine.rt.settings.simple_ui_size;
        (display_w, display_h) = simple_ui::ui_size(canvas.output_size()?, size);
    }
    // Vsync paces frames on a 60 Hz display; on others, without vsync,
    // or while the window is not shown, the loop goes by the clock. The
    // bench's novsync turns it off whatever the setting says.
    let bench_vsync = bench::wants_vsync();
    let wants_vsync = |settings: &Settings| bench_vsync && settings.vsync;
    let mut main_presents = pacing::Presents::new(&canvas, wants_vsync(&machine.rt.settings));
    let mut editor_presents = None;
    let mut iterations = 0u32;
    // Replaced with the renderer, as is everything that holds textures.
    let mut creator = OwnedCreator::new(&canvas);
    let mut texture_creator = creator.get();
    let mut mode = machine.screen_mode();
    let (out_w, out_h) = canvas.output_size()?;
    let settings = &machine.rt.settings;
    let (_, mut filter) = screen_place(&mode, settings, simple_ui, out_w, out_h as f32);
    // The frame's size and filter, which the texture is made for.
    let mut texture_kind = (mode.frame(), filter);
    let mut texture = frame_texture(&canvas, texture_creator, texture_kind)?;
    // The pixels that mouse positions are given in: the screen's, or the
    // editor's frame.
    let mut pointer_space = (mode.width, mode.height);
    // Where the editor's frame was drawn, if that frame was the
    // editor's laid out for the main window's room (see
    // `placement::editor_fill`), where the pointer maps through it: always
    // but in the Simple UI.
    let mut filled_editor: Option<FRect> = None;
    let mut menu = MenuBar::default();
    // Whether a program ran when the loop last looked, to see one start.
    let mut was_running = false;
    // The editor's own window, made the first time the editor opens apart
    // from the screen and hidden while it is closed, with its frame's
    // texture and the pixels its pointer positions are given in.
    let mut editor_gfx: OnceCell<EditorWindow> = OnceCell::new();
    // The other textures, and what the frames' textures hold. Some are
    // the editor's window's, so they come after it, to be dropped first.
    let mut textures = Held::default();
    // The emulator's UI over the screens or the editor, with the cursor
    // drawn where the system shows none.
    let cursor_var = std::env::var(pointer::CURSOR_VAR).ok();
    let draws_cursor = pointer::draws_own(video.current_video_driver(), cursor_var.as_deref());
    let mut overlays = Overlays::new(pointer::DrawnCursor::new(draws_cursor));
    // View > "Keep 16:9 when resizing": what puts the main window's shape
    // right where the system ignores the shape asked for, and whether
    // that shape is asked for now.
    let mut keep_shape = window::KeepShape::default();
    // SDL asks for no shape at first; asking sets the window's size again.
    let mut shape_asked = false;
    // The console without a desktop, whose one window is the display.
    let kms = video.current_video_driver() == "kmsdrm";
    let mut editor_space = (1, 1);
    let mut editor_shown = false;
    // Whether it has the keys, and the pointer.
    let mut editor_focused = false;
    // The main window took the keys from it for a step that waits for
    // input, to give them back when the step stops.
    let mut keys_lent = false;
    let mut pointer_in_editor = false;
    // Whether the menu bar and the emulator's windows show in the editor's
    // window. They follow the keys, but a dialog or window stays where it
    // opened until it closes.
    let mut ui_on_editor = false;
    // The window that input in the other one brought to the front, and
    // when, to mark it if the system would not raise it.
    let mut raised: Option<(Which, Instant)> = None;
    let mouse = sdl.mouse();
    let mut cursor_shown = true;
    // The emulator's own windows and dialogs.
    let mut screens = Screens::default();
    // A program's DIALOG. It takes clicks on it, Enter and typing, but
    // leaves the menu bar and the stop keys working.
    let mut program_dialog: Option<ProgramView> = None;
    let mut blink = 0u32;
    let mut toasts = Toasts::default();
    let mut frame_guard = FrameGuard::default();
    let mut key_hint = KeyHint::default();
    let mut slow_hint = slow_hint::SlowHint::default();
    use_instruments(machine, bank.as_ref(), &mut toasts);
    if let Err(e) = machine.rt.load_extra_font() {
        toasts.push(e);
    }
    if let Some(mut lines) = recovery.take_notice() {
        lines.splice(
            0..0,
            ["The emulator failed last time.".to_string(), String::new()],
        );
        let notice = Dialog::notice("Recovered programs", lines, (display_w, display_h));
        screens.dialog = Some((notice, Asking::Notice));
    }
    // Where the manager was when Run left it, for the return to it.
    let mut home_memory = Memory::default();
    screens.welcome = (simple_ui && !machine.rt.settings.simple_ui_welcome_shown)
        .then(|| Welcome::new((display_w, display_h)));
    // Where the list was left, and the program it ran last, for Restart.
    let mut list_memory = simple_ui::Memory::default();
    let mut ran_from_list = restart;
    // The debugger's panel below the editor, and the help or the index
    // beside it.
    let mut debugger = Debugger::default();
    let mut side_panel = Beside::default();

    let mut events = sdl.event_pump()?;
    let mut pacer = pacing::Pacer::new(Instant::now());
    let mut bench = bench::Bench::from_env();
    if std::env::var_os("SB_GPU_GOLDEN").is_some() {
        if canvas.renderer_name == "software" {
            println!("nothing to try: software renderer");
            return Ok(());
        }
        let passed = gpu_golden::report(canvas.raw());
        std::process::exit(if passed { 0 } else { 1 });
    }
    let mut paths = gpu::Paths::default();
    // Why the renderer has to be replaced, from where it was found out.
    let mut renderer_failed: Option<String> = None;
    let mut watch = RendererWatch::default();
    // Renderers to refuse, for the bench.
    let mut refusals = 0;
    let mut suspend = SuspendWatch::new();
    // The events of a trip round the loop, as they were drained.
    let mut polled: Vec<Event> = Vec::new();
    // Who the right stick and R3 are for now: the program or the
    // emulator's screens.
    macro_rules! stick_role {
        () => {
            stick_mouse::Where {
                simple_ui,
                running: machine.is_running(),
                screen: screens.any()
                    || screens.pause(menu.shown, machine.is_running())
                    || menu.is_open()
                    || machine.editor_has_keys() && machine.program_dialog().is_none(),
                program_dialog: machine.program_dialog().is_some(),
                cursor: machine.rt.mouse_cursor(),
                editor_window: machine.is_detached() && machine.is_editing() && editor_focused,
            }
            .role()
        };
    }
    // Lends the machine for a call that may hold the loop up, unless the
    // program is paused.
    macro_rules! lend {
        () => {
            standby::Lend {
                frames: !screens.pause(menu.shown, machine.is_running()),
                run_time: frame_time(machine, &pacer),
                machine: &mut *machine,
                guard: &mut frame_guard,
                toasts: &mut toasts,
                clock: pacer.clock(),
            }
        };
    }
    'running: loop {
        let trip_began = Instant::now();
        // A renderer that failed ends the frame here, to be replaced above it.
        macro_rules! drawing {
            ($result:expr) => {
                match $result {
                    Ok(value) => value,
                    Err(e) => {
                        renderer_failed = Some(e.to_string());
                        continue 'running;
                    }
                }
            };
        }
        if let Some(why) = renderer_failed.take() {
            eprintln!("the renderer failed: {why}");
            // All that lives on the renderers goes before they do.
            paths.forget();
            textures.forget_device();
            textures.forget_targets();
            overlays.forget();
            machine.rt.touch();
            machine.touch_sub();
            if !watch.failed(&canvas.renderer_name) {
                return Err(format!("Drawing keeps failing: {why}").into());
            }
            // The editor's window opens again when it is next wanted.
            if let Some(gfx) = editor_gfx.take()
                && machine.rt.settings.editor_window_keep_place
                && editor_shown
            {
                let place = window_place(gfx.canvas.borrow().window());
                machine.rt.settings.editor_window_place = Some(place);
            }
            (editor_shown, editor_focused, pointer_in_editor) = (false, false, false);
            editor_presents = None;
            let window = canvas.window().clone();
            drop(texture);
            drop(creator);
            drop(canvas);
            let mut waiting = Waiting {
                events: &mut events,
                controls: &mut inputs.controls,
                refusals: &mut refusals,
                quiet: headless(),
            };
            let made = new_renderer(
                &window,
                texture_kind,
                &watch,
                machine,
                recovery,
                &mut waiting,
            );
            let Some(made) = made? else {
                break 'running;
            };
            (canvas, creator, texture) = made;
            texture_creator = creator.get();
            main_presents = pacing::Presents::new(&canvas, wants_vsync(&machine.rt.settings));
            pacer.restart(Instant::now());
            suspend = SuspendWatch::new();
            toasts.push("The renderer failed and was restarted.".to_string());
        }
        // After a suspend a GL context may have lost its textures without a word.
        if suspend.woke() {
            if bench.on() {
                eprintln!("woke from a suspend: giving the renderer all again");
            }
            push_render_event(SDL_EVENT_RENDER_DEVICE_RESET);
        }
        if bench.on() {
            bench.sound(audio.stats());
        }
        // Frames the standby thread ran this trip.
        let mut kept = standby::Kept::default();
        if bench.step(machine) {
            bench.report(&canvas.renderer_name);
            println!("drawn by  {}", paths.drawn_by);
            if let Some(gpu) = &paths.main {
                gpu.report("gpu");
            }
            if let Some(gpu) = &paths.sub {
                gpu.report("sub");
            }
            break 'running;
        }
        for what in bench.due() {
            let pushed = match what {
                "targets" => {
                    if let Some(gpu) = paths.main.iter_mut().chain(&mut paths.sub).next() {
                        gpu.scribble();
                    }
                    push_render_event(SDL_EVENT_RENDER_TARGETS_RESET);
                    None
                }
                "device" => {
                    push_render_event(SDL_EVENT_RENDER_DEVICE_RESET);
                    None
                }
                "lost" => {
                    push_render_event(SDL_EVENT_RENDER_DEVICE_LOST);
                    None
                }
                "fail" => {
                    renderer_failed = Some("told to fail".to_string());
                    None
                }
                "panic" => panic!("told to panic"),
                "unsaved" => {
                    machine.set_slot(0, "?\"UNSAVED\"\n".to_string());
                    machine.rt.slot_modified[0] = true;
                    None
                }
                "editor-focus" => editor_gfx.get().map(|gfx| Event::Window {
                    timestamp: 0,
                    window_id: gfx.id,
                    win_event: WindowEvent::FocusGained,
                }),
                "main-focus" => editor_gfx.get().map(|gfx| Event::Window {
                    timestamp: 0,
                    window_id: gfx.id,
                    win_event: WindowEvent::FocusLost,
                }),
                "fullscreen" | "windowed" => {
                    let _ = canvas.window_mut().set_fullscreen(what == "fullscreen");
                    None
                }
                "tool" => {
                    let _ = machine.run_tool(0);
                    None
                }
                "substop" => {
                    machine.stop_sub();
                    None
                }
                "auto" => {
                    machine.rt.settings.renderer = Renderer::Auto;
                    None
                }
                "cpu" => {
                    machine.rt.settings.renderer = Renderer::Cpu;
                    None
                }
                _ if what.starts_with("window-size:") => {
                    let i = what[12..].parse().unwrap_or(1);
                    if let Some(&(_, halves)) = window::SCALES.get(i) {
                        let settings = &mut machine.rt.settings;
                        set_window_scale(canvas.window_mut(), settings, profile_display, halves);
                    }
                    None
                }
                _ if what.starts_with("refuse:") => {
                    refusals = what[7..].parse().unwrap_or(0);
                    None
                }
                _ if what.starts_with("usb:") => {
                    inputs.bench_usb = what[4..].parse().ok();
                    None
                }
                _ if what.starts_with("key:") || what.starts_with("editor-key:") => {
                    let (from, key) = what.split_once(':').expect("checked");
                    let (keymod, name) = bench::key_mods(key);
                    let window_id = match (from, editor_gfx.get()) {
                        ("editor-key", Some(gfx)) => gfx.id,
                        _ => canvas.window().id(),
                    };
                    Keycode::from_name(name).map(|key| Event::KeyDown {
                        timestamp: 0,
                        window_id,
                        keycode: Some(key),
                        // Any will do, but SDL's event must have one.
                        scancode: Some(Scancode::A),
                        keymod,
                        repeat: false,
                        which: controls::BENCH_KEYBOARD,
                        raw: 0,
                    })
                }
                _ if what.starts_with("pad:") || what.starts_with("padup:") => {
                    let (timestamp, which) = (0, 0.into());
                    let (state, name) = what.split_once(':').expect("checked");
                    Button::from_string(name).map(|button| {
                        if state == "pad" {
                            inputs.controls.bench |= controls::button_bit(button);
                            Event::GamepadButtonDown {
                                timestamp,
                                which,
                                button,
                            }
                        } else {
                            inputs.controls.bench &= !controls::button_bit(button);
                            Event::GamepadButtonUp {
                                timestamp,
                                which,
                                button,
                            }
                        }
                    })
                }
                _ if what.starts_with("sleep:") => {
                    let time = Duration::from_millis(what[6..].parse().unwrap_or(0));
                    let ((), held) = standby.hold(lend!(), || std::thread::sleep(time));
                    kept.add(held);
                    suspend.pretend(time);
                    None
                }
                _ if bench::pointer_events(what.trim_start_matches("editor-"), 0).is_some() => {
                    let window_id = match (what.starts_with("editor-"), editor_gfx.get()) {
                        (true, Some(gfx)) => gfx.id,
                        _ => canvas.window().id(),
                    };
                    let what = what.trim_start_matches("editor-");
                    for event in bench::pointer_events(what, window_id).into_iter().flatten() {
                        let _ = sdl.event().and_then(|e| e.push_event(event));
                    }
                    None
                }
                _ => {
                    let (editor, size) = match what.strip_prefix("editor-") {
                        Some(what) => (true, what),
                        None => (false, what),
                    };
                    let size = size.strip_prefix("resize:").and_then(|s| s.split_once(':'));
                    if let Some((w, h)) = size {
                        let (w, h) = (w.parse().unwrap_or(640), h.parse().unwrap_or(360));
                        match editor_gfx.get() {
                            Some(gfx) if editor => {
                                let _ = gfx.canvas.borrow_mut().window_mut().set_size(w, h);
                            }
                            _ if editor => {}
                            _ => {
                                let _ = canvas.window_mut().set_size(w, h);
                            }
                        }
                    }
                    None
                }
            };
            if let Some(event) = pushed {
                let _ = sdl.event().and_then(|e| e.push_event(event));
            }
        }
        // With the Simple UI on the UI follows the output's size.
        let wanted = match simple_ui {
            true => simple_ui::ui_size(
                drawing!(canvas.output_size()),
                machine.rt.settings.simple_ui_size,
            ),
            false => profile_display,
        };
        if wanted != (display_w, display_h) {
            (display_w, display_h) = wanted;
            if let Some((open, _)) = &mut screens.dialog {
                open.set_size(wanted);
            }
            if let Some(open) = &mut screens.game_list {
                open.set_size(wanted);
            }
            if let Some(open) = &mut screens.pause_menu {
                open.set_size(wanted);
            }
            if let Some(open) = &mut screens.welcome {
                open.set_size(wanted);
            }
            // Made again at the new size, from the machine's, below.
            program_dialog = None;
        }
        if bench.on() {
            let size = (display_w, display_h);
            menu.shown |= bench.opens("menu");
            if bench.opens("help") {
                side_panel.open = open_help(machine, help.as_deref(), &mut toasts, size);
            }
            if bench.opens("about") {
                let lines = about_lines(&paths.drawn_by);
                screens.dialog = Some((Dialog::notice("About", lines, size), Asking::Notice));
            }
            if bench.opens("colors") {
                screens.color_window = Some(ColorWindow::new(size));
            }
            if bench.opens("file-menu") {
                menu.open_menu(0, "");
            }
            if bench.opens("view-menu") {
                menu.open_menu(2, "Window size");
            }
            if bench.opens("size-menu") {
                menu.open_menu(2, "Size (F2)");
            }
            if bench.opens("settings") {
                let mut window = SettingsWindow::new(size);
                window.show_page("Editor");
                screens.settings_window = Some(window);
            }
            if bench.opens("toast") {
                toasts.push("A notice".to_string());
            }
        }
        // The program that Projects ran has ended: back to Projects. Opened
        // some other way, a wait for a button is over.
        if screens.manager.is_some() {
            machine.home_leave();
        } else if simple_ui {
            // The game list opens below, whenever nothing runs.
            simple_ui_ended(machine);
        } else if machine.home_done() {
            let size = (display_w, display_h);
            screens.manager = Some(ProjectManager::reopen(
                &machine.rt.storage,
                size,
                &home_memory,
            ));
        }
        // The Simple UI rests in the game list, never in DIRECT mode or
        // the editor, which a breakpoint may open.
        if simple_ui {
            if machine.is_editing() {
                machine.toggle_editor();
            }
            if screens.game_list.is_none()
                && screens.pause_menu.is_none()
                && !machine.is_running()
                && !machine.home_waiting()
            {
                let size = (display_w, display_h);
                screens.game_list = Some(GameList::new(&machine.rt.storage, size, &list_memory));
            }
        }
        // The editor shows in a window of its own while it is open, if
        // chosen.
        machine.set_detached(machine.rt.settings.editor_window);
        let apart = machine.is_detached() && machine.is_editing();
        if apart && editor_gfx.get().is_none() {
            let settings = &machine.rt.settings;
            let place = settings
                .editor_window_place
                .filter(|_| settings.editor_window_keep_place);
            let main_window = canvas.window();
            let made = EditorWindow::open(&video, (display_w, display_h), place, main_window);
            let made = drawing!(made);
            let _ = editor_gfx.set(made);
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
                if machine.rt.settings.editor_window_keep_place {
                    machine.rt.settings.editor_window_place = Some(window_place(window));
                }
                window.hide();
                editor_focused = false;
                pointer_in_editor = false;
            }
        }
        machine.focus_editor(apart && editor_focused);
        // The menu bar and the emulator's windows show in the window with
        // the keys.
        if !screens.any() {
            ui_on_editor = editor_focused;
        }
        let mut ui_editor = apart && ui_on_editor;
        let (out_w, out_h) = drawing!(canvas.output_size());
        bench.window_size((out_w, out_h));
        let (win_w, _) = canvas.window().size();
        let display = match simple_ui {
            true => simple_ui::display_rect(
                (display_w, display_h),
                (out_w, out_h),
                machine.rt.settings.simple_ui_size,
            ),
            false => display_rect(display_w, display_h, out_w, out_h),
        };
        let settings = &machine.rt.settings;
        // A UI at a scale that is not whole can go past the bottom edge;
        // the program's screen stays on the output. The desktop's goes
        // below its display too, in a window narrower than 16:9, and so
        // does the UI that goes with the screen.
        let screen_h = display.h.min(out_h as f32);
        let main_ui_h = match simple_ui {
            true => display_h,
            false => placement::ui_height(display, display_h, out_h),
        };
        let (shown, _) = match (filled_editor, simple_ui) {
            (Some(rect), _) => (rect, filter),
            (None, true) => screen_place(&mode, settings, true, out_w, screen_h),
            (None, false) => {
                placement::desktop_screen_place(&mode, settings, (out_w, out_h), display)
            }
        };
        // The main window's menus at 1x where its display is scaled below.
        let menus_apart = match simple_ui {
            true => None,
            false => placement::menu_place(display, display_w, (out_w, out_h)),
        };
        let main_map = Mapping {
            px: out_w as f32 / win_w.max(1) as f32,
            display,
            display_w,
            shown,
            space: pointer_space,
            menu: menus_apart.map(|place| place.at),
        };
        let ui_scale = bench.ui_scale;
        // The window the editor shows in, whose zooms F2 and View > Size
        // go by: its width and the height of SmileBASIC 4's display there.
        let mut editor_room = (out_w, display.h);
        // The editor's window has the UI in pixels of its own, and the
        // emulator's windows laid out on the display as on the main window.
        let (editor_map, windows_map, editor_ui) = match editor_gfx.get().filter(|_| apart) {
            Some(gfx) => {
                let settings = &machine.rt.settings;
                let layout = drawing!(gfx.layout(settings, (display_w, display_h), ui_scale));
                let editor_canvas = gfx.canvas.borrow();
                let (out_w, out_h) = drawing!(editor_canvas.output_size());
                let (win_w, _) = editor_canvas.window().size();
                editor_room = (out_w, (display_h * layout.ui) as f32);
                let map = Mapping {
                    px: out_w as f32 / win_w.max(1) as f32,
                    display: from_corner(layout.ui_pixels()),
                    display_w: layout.ui_size.0,
                    shown: from_corner(layout.frame_pixels()),
                    space: editor_space,
                    menu: None,
                };
                let display = (display_w, display_h);
                let windows = Mapping {
                    display: windows_rect(display, (out_w, out_h), layout.ui),
                    display_w,
                    ..map
                };
                (map, windows, layout.ui_size)
            }
            None => (main_map, main_map, (display_w, main_ui_h)),
        };
        // The room the menus open in on the window with the keys, and
        // that of the context menus on the main window's editor.
        let main_menus = menus_apart.map_or((display_w, main_ui_h), |place| place.size);
        menu.space = Some(if ui_editor { editor_ui } else { main_menus });
        let popup_room = [main_menus, editor_ui];
        // The window the event being handled came from: the main one, or
        // the editor's, and whether the emulator's windows are open on the
        // editor's.
        let maps = [main_map, editor_map, windows_map];
        let source = Cell::new(0);
        let windows_on_editor = Cell::new(false);
        let map = || match (source.get(), windows_on_editor.get()) {
            (1, true) => maps[2],
            (i, _) => maps[i],
        };
        // Mouse positions come in window coordinates; the display is in
        // pixels...
        let to_display = |x: f32, y: f32| map().to_display(x, y);
        // ...and the menus', which are the display's unless they go apart.
        let to_menu = |x: f32, y: f32| map().to_menu(x, y);
        // ...and so are the screen's or the editor's, which fill their
        // rectangle.
        let to_screen = |x: f32, y: f32| map().to_screen(x, y);
        let shift = |keymod: Mod| keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
        let ctrl = |keymod: Mod| keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD);
        let mut quit = false;
        // The pad repeats on the screen that has it, once a frame whatever
        // events came; a new screen or mode starts without the old hold.
        let screen = input_screen(
            screens.pause_menu.is_some(),
            screens.dialog.is_some(),
            screens.manager.as_ref().map(ProjectManager::mode_id),
            screens.game_list.as_ref().map(GameList::mode_id),
        );
        let repeats = inputs.repeats(screen);
        // Pumping the events is what can hold the loop up for seconds.
        let ((), held) = standby.hold(lend!(), || {
            while let Some(event) = events.poll_event() {
                polled.push(event);
            }
        });
        kept.add(held);
        if kept.frames > 0 {
            // Frames the standby thread ran went undrawn.
            pacer.kept(kept.frames);
            machine.rt.clipboard.settle();
            bench.kept(kept);
        }
        for (repeated, mut event) in
            (repeats.into_iter().map(|e| (true, e))).chain(polled.drain(..).map(|e| (false, e)))
        {
            // A repeat that changes the screen ends the rest of them.
            let now = input_screen(
                screens.pause_menu.is_some(),
                screens.dialog.is_some(),
                screens.manager.as_ref().map(ProjectManager::mode_id),
                screens.game_list.as_ref().map(GameList::mode_id),
            );
            if repeated && now != screen {
                continue;
            }
            if !repeated {
                // A, B, X and Y are what the setting makes them from here
                // on, for every screen and the program, even after a
                // change an event before made.
                inputs.controls.swap = machine.rt.settings.swap_ab_xy;
                inputs.controls.stick_mouse.on = machine.rt.settings.right_stick_mouse;
                inputs.controls.event(&mut event);
                // R3 clicks on the emulator's screens as the mouse does.
                let main_id = canvas.window().id();
                let lcd = main_map.lcd(simple_ui);
                inputs
                    .controls
                    .stick_click(&mut event, stick_role!(), main_id, lcd);
                // A press and its release can both come between two
                // frames, which what is held then would miss. One of the
                // emulator's screens takes them back below.
                let keys = machine.rt.settings.keyboard_buttons;
                for (tap, down) in inputs.controls.taps(&event, keys).into_iter().flatten() {
                    machine.rt.input.tap(tap, down);
                }
            }
            let event = inputs.pad_event(event, repeated, screen);
            // Whatever the event does may show on the screens.
            machine.rt.touch();
            // Some renderers lose what textures held, which then has to be
            // given again.
            if matches!(
                event,
                Event::RenderTargetsReset { .. } | Event::RenderDeviceReset { .. }
            ) {
                textures.forget_targets();
                overlays.forget();
                // A target comes back empty, and the frames are drawn again.
                machine.touch_sub();
            }
            // Direct3D 11 and 12 and Vulkan have let go of every texture, and fail
            // on those still held; a page sent to the GPU has to go again too.
            if let Event::RenderDeviceReset { .. } = event {
                paths.forget();
                textures.forget_device();
                texture = drawing!(frame_texture(&canvas, texture_creator, texture_kind));
            }
            // It cannot come back, and SDL says to make a new renderer.
            if let Event::Unknown { type_, .. } = event
                && type_ == SDL_EVENT_RENDER_DEVICE_LOST.0
            {
                renderer_failed = Some("the graphics device was lost".to_string());
            }
            let editor_id = editor_gfx.get().map(|gfx| gfx.id);
            let from_editor = editor_id.is_some() && event.get_window_id() == editor_id;
            // A window shown again tries vsync again.
            if let Event::Window {
                win_event:
                    WindowEvent::Exposed
                    | WindowEvent::Shown
                    | WindowEvent::Restored
                    | WindowEvent::FocusGained,
                ..
            } = event
                && !from_editor
            {
                main_presents.retry(&canvas);
            }
            if let Event::Window {
                win_event: WindowEvent::PixelSizeChanged(..) | WindowEvent::Resized(..),
                ..
            } = event
                && !from_editor
            {
                main_presents.resized();
            }
            if let Event::Window {
                win_event: WindowEvent::Resized(..),
                ..
            } = event
                && !from_editor
            {
                keep_shape.resized(Instant::now());
            }
            // Back from another program, which may have changed the files.
            if let Event::Window {
                win_event: WindowEvent::FocusGained,
                ..
            } = event
            {
                let storage = &machine.rt.storage;
                if let Some(open) = &mut screens.manager {
                    open.reread(storage, true);
                }
                if let Some(FileDialog::Load(picker)) = &mut screens.file_dialog {
                    picker.reread(storage, true);
                }
            }
            if let (true, Event::Window { win_event, .. }) = (from_editor, &event) {
                match win_event {
                    WindowEvent::FocusGained => editor_focused = true,
                    WindowEvent::FocusLost => editor_focused = false,
                    WindowEvent::MouseEnter => pointer_in_editor = true,
                    WindowEvent::MouseLeave => pointer_in_editor = false,
                    _ => {}
                }
                if !screens.any() && editor_focused != ui_on_editor {
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
            windows_on_editor.set(ui_editor && screens.any());
            // The event is on the editor, wherever it shows, unless a
            // subprogram beside it has the keys...
            let on_editor = machine.is_editing()
                && (!apart || from_editor)
                && !machine.sub_has_focus()
                && (from_editor || program_dialog.is_none());
            // ...or on the window it shows in when open, or on the window
            // with the menu bar and the emulator's windows.
            let editor_window = !apart || from_editor;
            let ui_window = from_editor == ui_editor;
            // Input in the window without the emulator's window that takes
            // it all brings that one forward instead.
            let modal_on = window::modal_window(apart, screens.any(), ui_editor);
            let from = if from_editor {
                Which::Editor
            } else {
                Which::Main
            };
            if let Some(target) = window::blocked_by(&event, modal_on, from) {
                match (target, editor_gfx.get()) {
                    (Which::Editor, Some(gfx)) => gfx.canvas.borrow_mut().window_mut().raise(),
                    _ => canvas.window_mut().raise(),
                };
                raised = Some((target, Instant::now()));
                continue;
            }
            if let Event::KeyDown {
                scancode, repeat, ..
            } = event
            {
                inputs
                    .typing
                    .any_key_down(&mut machine.rt, scancode, repeat);
            }
            if matches!(event, Event::KeyDown { .. }) {
                inputs.eat_text = screens.any()
                    || program_dialog.is_some()
                    || debugger.panel.typing()
                    || (side_panel.open.is_some() && side_panel.focus);
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
            if pointer && screens.any() && !ui_window {
                continue;
            }
            // Ctrl+Shift+F12 turns the Simple UI on or off, whatever shows.
            if let Event::KeyDown {
                keycode: Some(Keycode::F12),
                keymod,
                repeat: false,
                ..
            } = event
                && ctrl(keymod)
                && shift(keymod)
            {
                simple_ui = !simple_ui;
                machine.rt.settings.simple_ui = simple_ui;
                let window = canvas.window_mut();
                // The Simple UI is fullscreen; the desktop goes back to
                // what it was, as View > Fullscreen keeps it.
                if simple_ui {
                    let desktop = window.fullscreen_state() != FullscreenType::Off;
                    machine.rt.settings.fullscreen = desktop;
                    let _ = window.set_fullscreen(true);
                    // What needs a keyboard or a mouse closes.
                    (
                        screens.manager,
                        screens.file_dialog,
                        side_panel.open,
                        debugger.menu,
                    ) = (None, None, None, None);
                    (
                        screens.settings_window,
                        screens.shortcuts_window,
                        screens.color_window,
                    ) = (None, None, None);
                    menu.shown = false;
                    menu.leave();
                    if !machine.rt.settings.simple_ui_welcome_shown {
                        screens.welcome = Some(Welcome::new((display_w, display_h)));
                    }
                } else {
                    let _ = window.set_fullscreen(machine.rt.settings.fullscreen);
                    (screens.game_list, screens.pause_menu, screens.welcome) = (None, None, None);
                }
                inputs.plus_down = None;
                continue;
            }
            // Ctrl+Shift+F11 turns the desktop's fullscreen on or off,
            // whatever shows. The Simple UI is always fullscreen. Like
            // Ctrl+Shift+F12 it types nothing and starts no tool, though
            // KEYBOARD sees the keys held, as it sees every key.
            if let Event::KeyDown {
                keycode: Some(Keycode::F11),
                keymod,
                repeat,
                ..
            } = event
                && ctrl(keymod)
                && shift(keymod)
            {
                if !simple_ui && !repeat {
                    toggle_fullscreen(canvas.window_mut(), &mut machine.rt.settings);
                }
                continue;
            }
            // What each of the emulator's screens is given with the event.
            macro_rules! cx {
                () => {
                    events::Ctx {
                        machine: &mut *machine,
                        toasts: &mut toasts,
                        quit: &mut quit,
                        size: (display_w, display_h),
                    }
                };
            }
            // Does what an item of the menu bar says, for a click on it or
            // for the key that stands for it.
            macro_rules! act {
                ($action:expr) => {{
                    let tools = events::Tools {
                        clipboard: &clipboard,
                        help: help.as_deref(),
                        drawn_by: &paths.drawn_by,
                    };
                    let main = events::MainWindow {
                        window: canvas.window_mut(),
                        display: profile_display,
                        keep_shape: &mut keep_shape,
                        kms,
                    };
                    let (side, bar) = (&mut side_panel, &mut menu);
                    events::menu_action($action, &mut screens, side, bar, tools, main, cx!())
                }};
            }
            // The system's file dialogs, for the screens that open them.
            macro_rules! choosers {
                () => {
                    events::Choosers {
                        window: canvas.window(),
                        font: &chosen_font,
                        extra_font: &chosen_extra_font,
                        colors: &chosen_colors,
                        folder: &chosen_folder,
                    }
                };
            }
            if events::pause_hotkeys(
                &event,
                &mut inputs,
                &mut screens,
                (simple_ui, ran_from_list.is_some()),
                cx!(),
            ) {
                continue;
            }
            if screens.pause_menu.is_some() {
                let (memory, ran) = (&list_memory, &ran_from_list);
                events::pause_menu(event, &mut screens, memory, ran, cx!());
                continue;
            }
            // After an error of the program that Projects ran, only A or
            // Enter, freshly pressed, does anything on the keyboard and
            // the pad; the window and its menu bar work as ever.
            if machine.home_waiting() && !screens.any() && program_dialog.is_none() {
                match &event {
                    Event::KeyDown {
                        keycode: Some(Keycode::Return | Keycode::KpEnter),
                        repeat: false,
                        ..
                    }
                    | Event::GamepadButtonDown {
                        button: Button::East,
                        ..
                    } => {
                        if machine.home_press() {
                            if simple_ui {
                                machine.tidy();
                            }
                            let (storage, size) = (&machine.rt.storage, (display_w, display_h));
                            if simple_ui {
                                screens.game_list =
                                    Some(GameList::new(storage, size, &list_memory));
                            } else {
                                screens.manager =
                                    Some(ProjectManager::reopen(storage, size, &home_memory));
                            }
                        }
                        continue;
                    }
                    Event::KeyDown {
                        keycode: Some(key),
                        keymod,
                        ..
                    } if function_key(*key).is_none()
                        && !ctrl(*keymod)
                        && !keymod.intersects(
                            Mod::LALTMOD | Mod::RALTMOD | Mod::LGUIMOD | Mod::RGUIMOD,
                        ) =>
                    {
                        continue;
                    }
                    Event::TextInput { .. } => continue,
                    Event::GamepadButtonDown { button, .. } if *button != Button::Back => continue,
                    _ => {}
                }
            }
            match event {
                Event::MouseMotion { x, y, .. } if !from_editor => {
                    let size = (mode.width as i32, mode.height as i32);
                    let lcd = map().lcd(simple_ui).to_display(x, y);
                    controls::place_mouse(&mut inputs.controls.mouse, to_screen(x, y), size, lcd);
                }
                Event::MouseWheel { y, .. }
                    if screens.manager.is_none() && !on_editor && !from_editor =>
                {
                    inputs.controls.mouse.wheel += y.round() as i32;
                }
                // Here, before the dialogs and menus below take the event
                // and go on to the next, so that the page and the editor's
                // selection let go of the pointer whatever is shown.
                Event::MouseButtonUp {
                    mouse_btn: MouseButton::Left,
                    ..
                } => {
                    debugger.panel.release();
                    inputs.dragging = false;
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
                } if inputs.plus_down.take().is_some() => machine.sysreq(),
                Event::GamepadButtonDown {
                    button: Button::Start,
                    ..
                } if !screens.any() => inputs.plus_down = Some(Instant::now()),
                _ => {}
            }
            if screens.dialog.is_some() {
                let list = &mut ran_from_list;
                if events::dialog(&event, &mut screens, list, cx!(), to_display) == Taken::Quit {
                    break 'running;
                }
                continue;
            }
            if screens.welcome.is_some() {
                events::welcome(&event, &mut screens, cx!());
                continue;
            }
            if let (Some(open), false) = (&mut program_dialog, from_editor)
                && events::program_dialog(&event, open, machine, to_display)
            {
                continue;
            }
            let side_window = if side_panel.direct {
                !from_editor
            } else {
                editor_window
            };
            // An open menu too tall for the window scrolls.
            if let Event::MouseWheel {
                y,
                mouse_x,
                mouse_y,
                ..
            } = event
                && ui_window
                && menu.wheel(to_menu(mouse_x, mouse_y), y.round() as i32)
            {
                continue;
            }
            if side_window && events::side_pointer(&event, &mut side_panel, to_display) {
                continue;
            }
            // The help or the index beside the editor is over the panel.
            if editor_window {
                let (clip, places) = (&clipboard, (to_display, to_menu));
                if events::debugger(&event, &mut debugger, &mut screens, clip, cx!(), places) {
                    continue;
                }
            }
            if side_panel.focus
                && side_window
                && events::side_keys(&event, &mut side_panel, machine)
            {
                continue;
            }
            if screens.game_list.is_some() {
                let (memory, ran) = (&mut list_memory, &mut ran_from_list);
                events::game_list(&event, &mut screens, memory, ran, cx!());
                // Nothing else goes past it, as typing would reach the
                // prompt under it.
                continue;
            }
            if screens.manager.is_some() {
                let memory = &mut home_memory;
                events::manager(&event, &mut screens, memory, choosers!(), cx!(), to_display);
                continue;
            }
            if screens.file_dialog.is_some() {
                events::file_dialog(event, &mut screens, cx!(), to_display);
                continue;
            }
            if screens.shortcuts_window.is_some() {
                events::shortcuts_window(event, &mut screens, cx!(), to_display);
                continue;
            }
            if screens.settings_window.is_some() {
                let bank = bank.as_ref();
                events::settings_window(event, &mut screens, bank, choosers!(), cx!(), to_display);
                continue;
            }
            if screens.color_window.is_some() {
                events::color_window(event, &mut screens, choosers!(), cx!(), to_display);
                continue;
            }
            match event {
                Event::Quit { .. } => quit = true,
                // The desktop's windows and tools are not to be had with
                // the Simple UI on.
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    ..
                } if simple_ui && desktop_key(key, keymod) => {}
                Event::MouseMotion {
                    x, y, mousestate, ..
                } => {
                    if ui_window {
                        menu.mouse_move(Some(to_menu(x, y)));
                    }
                    if editor_window {
                        debugger.panel.motion(to_display(x, y), mousestate.left());
                    }
                    // A button let go where this loop did not see it ends
                    // the selection too.
                    inputs.dragging &= mousestate.left();
                    if inputs.dragging && on_editor {
                        let (x, y) = to_screen(x, y);
                        machine.editor_mouse_drag(x, y);
                    }
                }
                Event::Window {
                    win_event: WindowEvent::MouseLeave,
                    ..
                } => {
                    if ui_window {
                        menu.mouse_move(None);
                    }
                    if editor_window {
                        debugger.panel.leave();
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
                    && !side_panel
                        .open
                        .as_ref()
                        .is_some_and(|s| s.frame().contains(to_display(x, y))) =>
                {
                    let at = to_display(x, y);
                    let items = if debugger.panel.contains(at) {
                        panel_menu(machine, &debugger.panel, at)
                    } else {
                        let (x, y) = to_screen(x, y);
                        if !machine.editor().has_selection() {
                            machine.editor_mouse_down(x, y, false);
                        }
                        editor_menu(machine)
                    };
                    // In the menus' pixels, which may be apart from the
                    // display's.
                    let room = popup_room[usize::from(apart)];
                    debugger.menu = Some(Popup::new(to_menu(x, y), items, room));
                }
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    clicks,
                    ..
                } => match if ui_window && !simple_ui {
                    let open = menu.is_open().then_some(profile_display);
                    menu.click(
                        to_menu(x, y),
                        menu_context(machine, canvas.window(), open, editor_room),
                    )
                } else {
                    Click::Ignored
                } {
                    Click::Chosen(action) => act!(action),
                    Click::Ignored
                        if let Some(open) = &mut side_panel.open
                            && editor_window
                            && open.frame().contains(to_display(x, y)) =>
                    {
                        side_panel.focus = true;
                        if let Side::Index(index, _) = open {
                            let outcome = index.click(to_display(x, y));
                            side_outcome(
                                &mut side_panel.open,
                                &mut side_panel.focus,
                                machine,
                                outcome,
                            );
                        }
                    }
                    Click::Ignored if on_editor && debugger.panel.contains(to_display(x, y)) => {
                        let (at, double) = (to_display(x, y), clicks == 2);
                        let panel = &mut debugger.panel;
                        events::panel_click(at, double, panel, &mut screens, &clipboard, cx!());
                    }
                    Click::Ignored if on_editor => {
                        side_panel.focus = false;
                        let (x, y) = to_screen(x, y);
                        if !machine.editor_margin_click(x, y) {
                            let extend = shift(keyboard.mod_state());
                            machine.editor_mouse_down(x, y, extend);
                            inputs.dragging = true;
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
                } if let Some(action) = debug_key(key, shift(keymod), ctrl(keymod)) => act!(action),
                // SmileBASIC 4's F5 is the + button.
                Event::KeyDown {
                    keycode: Some(Keycode::F5),
                    keymod,
                    repeat: false,
                    ..
                } if !ctrl(keymod) => inputs.plus_down = Some(Instant::now()),
                // F8 leaves F9 to F11 for SmileBASIC 4's tools.
                Event::KeyDown {
                    keycode: Some(Keycode::F8),
                    repeat: false,
                    ..
                } => menu.shown = !menu.shown,
                // F9 to F11 start the tools set in the settings, also
                // while a program runs.
                Event::KeyDown {
                    keycode: Some(key @ (Keycode::F9 | Keycode::F10 | Keycode::F11)),
                    repeat: false,
                    ..
                } if !(menu.shown && machine.is_running()) => {
                    let n = match key {
                        Keycode::F9 => 0,
                        Keycode::F10 => 1,
                        _ => 2,
                    };
                    match machine.run_tool(n) {
                        Ok(true) => {}
                        Ok(false) => toasts.push(format!("No tool is set for F{}", n + 9)),
                        Err(e) => toasts.push(format!("Could not start tool {}: {e}", n + 1)),
                    }
                }
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
                    inputs.eat_text = true;
                }
                // Editing the line typed in direct mode or for INPUT.
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    repeat,
                    ..
                } if !on_editor
                    && machine.rt.wants_line()
                    && let Some(code) = line_command(key, shift(keymod), ctrl(keymod)) =>
                {
                    inputs
                        .typing
                        .key_down(&mut machine.rt, keymod, repeat, Some(code))
                }
                // Ctrl+C is a break, as the + button is, in a program and
                // on a DIRECT mode line that runs. It types nothing, also
                // as it repeats.
                Event::KeyDown {
                    keycode: Some(Keycode::C),
                    keymod,
                    repeat,
                    ..
                } if ctrl(keymod) && !shift(keymod) && !on_editor && machine.is_running() => {
                    if !repeat {
                        machine.keyboard_break();
                    }
                    inputs.eat_text = true;
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
                    } else {
                        machine.rt.line_copied();
                    }
                }
                Event::KeyDown {
                    keycode: Some(Keycode::V),
                    keymod,
                    ..
                } if ctrl(keymod) && !on_editor && machine.rt.wants_line() => {
                    paste(machine, &clipboard, &mut toasts)
                }
                // Ctrl+Shift+V pastes everywhere, also as keys to programs.
                Event::KeyDown {
                    keycode: Some(Keycode::V),
                    keymod,
                    ..
                } if ctrl(keymod) && shift(keymod) => paste(machine, &clipboard, &mut toasts),
                // Ctrl+F1 to F5 type what KEY set for them.
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    ..
                } if ctrl(keymod)
                    && !shift(keymod)
                    && (on_editor || !machine.is_running())
                    && let Some(n) = function_key(key) =>
                {
                    machine.type_function_key(n);
                    inputs.eat_text = true;
                }
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
                    act!(Action::Projects);
                }
                // A controller's Select (-) opens them in direct mode, so
                // that a game can be picked with the pad alone, when it is
                // released with nothing else pressed or moved meanwhile:
                // held, it is for the right-stick mouse. Programs never see
                // this button: BUTTON has no bit for it.
                Event::GamepadButtonDown {
                    button: Button::Back,
                    ..
                } if !machine.is_running() && !on_editor && !simple_ui => {
                    inputs.controls.stick_mouse.select.arm()
                }
                Event::GamepadButtonUp {
                    button: Button::Back,
                    ..
                } if inputs.controls.stick_mouse.select.alone()
                    && !machine.is_running()
                    && !on_editor
                    && !simple_ui =>
                {
                    act!(Action::Projects);
                }
                Event::KeyDown {
                    keycode: Some(Keycode::F2),
                    ..
                } if on_editor => next_editor_size(machine, editor_room),
                // F1 shows the help for the command at the cursor, in the
                // editor or on the line typed in direct mode.
                Event::KeyDown {
                    keycode: Some(Keycode::F1),
                    keymod,
                    repeat: false,
                    ..
                } if !ctrl(keymod)
                    && (on_editor
                        || !machine.is_editing() && !machine.is_running() && !from_editor) =>
                {
                    let size = (display_w, display_h);
                    side_panel.show_help(machine, help.as_deref(), &mut toasts, size, !on_editor);
                }
                // Simple save and load, as SmileBASIC 4 has them.
                Event::KeyDown {
                    keycode: Some(key @ (Keycode::S | Keycode::L)),
                    keymod,
                    ..
                } if ctrl(keymod) && (!machine.is_running() || on_editor && apart) => {
                    act!(match key {
                        Keycode::S => Action::SaveSlot,
                        _ => Action::LoadSlot,
                    });
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
                } if on_editor && ctrl(keymod) => {
                    match events::editor_ctrl_action(key, shift(keymod)) {
                        Some(action) => act!(action),
                        None => events::editor_ctrl_key(key, editor_room, machine),
                    }
                }
                // Esc, which the editor's text does not use, closes a panel.
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } if side_panel.open.is_some()
                    && on_editor
                    && machine.edit().mode() == Mode::Edit =>
                {
                    side_panel.open = None;
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
                // Shift+Tab takes the indent away, and Shift and the cursor
                // keys select in the editor. They go with the typed keys,
                // which repeat in frames.
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    repeat,
                    ..
                } if on_editor
                    && shift(keymod)
                    && let Some(code) = match key {
                        Keycode::Tab => Some(editor::UNINDENT),
                        key => special_key(key).and_then(editor::select_key),
                    } =>
                {
                    inputs
                        .typing
                        .key_down(&mut machine.rt, keymod, repeat, Some(code))
                }
                // What a program's dialog does not take is lost, or it
                // would reach the prompt or the program beside it.
                Event::KeyDown { .. } if program_dialog.is_some() && !from_editor => {}
                // Typing for a program, the prompt or the editor.
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    repeat,
                    ..
                } => inputs
                    .typing
                    .key_down(&mut machine.rt, keymod, repeat, special_key(key)),
                Event::TextInput { .. } if inputs.eat_text => inputs.eat_text = false,
                Event::TextInput { text, .. } => {
                    inputs.typing.text(&mut machine.rt, &text);
                }
                _ => {}
            }
        }

        // A window the system would not raise, as Wayland may refuse, is
        // flashed until it has the keys. Not at once: on Wayland a flash
        // takes the place of the raise's request.
        if let Some((target, at)) = raised
            && at.elapsed() >= RAISE_WAIT
        {
            raised = None;
            let flash = sdl3::video::FlashOperation::UntilFocused;
            match (target, editor_gfx.get()) {
                (Which::Editor, Some(gfx)) if !editor_focused => {
                    let _ = gfx.canvas.borrow_mut().window_mut().flash(flash);
                }
                (Which::Main, _) if !canvas.window().has_input_focus() => {
                    let _ = canvas.window_mut().flash(flash);
                }
                _ => {}
            }
        }
        // Opened or closed since its window was shown or hidden above, the
        // editor shows where it did until the next frame.
        let apart = apart && machine.is_editing();
        let ui_editor = ui_editor && apart;
        if !simple_ui {
            let main = canvas.window_mut();
            let on = machine.rt.settings.keep_16_9;
            keep_main_shape(
                main,
                &mut keep_shape,
                &mut shape_asked,
                on && !kms,
                profile_display,
            );
        }
        if quit {
            let unsaved = machine.unsaved_slots();
            if unsaved.is_empty() {
                break 'running;
            }
            screens.dialog = Some((Dialog::quit(&unsaved, (display_w, display_h)), Asking::Quit));
        }
        if renderer_failed.is_some() {
            continue 'running;
        }

        // Programs see nothing pressed while the emulator's own screens
        // take the input.
        let modal = screens.any();
        // The help opened in direct mode closes when the editor opens or
        // a program runs; the editor's panels close with the editor.
        if side_panel.direct && (machine.is_editing() || machine.is_running())
            || !side_panel.direct && !machine.is_editing()
        {
            side_panel.open = None;
        }
        if side_panel.open.is_none() {
            side_panel.direct = false;
        }
        // Panels go on the side away from the code being edited.
        if let Some(open) = &mut side_panel.open {
            let right_side = !machine.is_split_rows() && machine.active_side() == 1;
            open.set_left(machine.is_split() && right_side);
        }
        if inputs.plus_down.is_some_and(|t| t.elapsed() >= FORCED_STOP) {
            inputs.plus_down = None;
            machine.stop();
        }
        close_bar_on_start(&mut menu, &mut was_running, machine.is_running());
        let paused = screens.pause(menu.shown, machine.is_running());
        menu.status = status_line(machine, paused);
        machine.focus_editor(editor_focused);
        let editor_keys = machine.editor_has_keys() && machine.program_dialog().is_none();
        // The help beside direct mode has the keys and the prompt is left
        // alone, and after a Projects run's error only its A goes anywhere.
        let waiting = machine.home_waiting() && program_dialog.is_none();
        let beside = side_panel.direct && side_panel.focus && side_panel.open.is_some();
        inputs.controls.swap = machine.rt.settings.swap_ab_xy;
        inputs.controls.stick_mouse.on = machine.rt.settings.right_stick_mouse;
        let mut held = inputs.controls.state(
            &events.keyboard_state(),
            events.mouse_state(),
            machine.rt.settings.keyboard_buttons,
        );
        // The right stick moves the main window's pointer, but not while
        // the editor's own window has the keys.
        if let Some(at) = inputs.controls.stick_pointer(&mut held, stick_role!()) {
            let (x, y) = main_map.lcd(simple_ui).display_to_window(at);
            let size = (mode.width as i32, mode.height as i32);
            inputs
                .controls
                .place_pointer(&mut held, main_map.to_screen(x, y), size, at);
            mouse.warp_mouse_in_window(canvas.window(), x, y);
        }
        inputs.typing.hold(&held);
        inputs.typing.upper_case = machine.rt.settings.upper_case_letters;
        let screen = modal || paused || editor_keys || waiting || beside;
        // Programs see nothing pressed while the editor has the keys, but
        // a key held there repeats.
        let editor_held = editor_keys && !(modal || paused || waiting || beside);
        machine.rt.input.editor_held = if editor_held {
            held
        } else {
            Default::default()
        };
        machine.rt.input.live = inputs.controls.carried.program_sees(held, screen);
        // What a screen had the controls for, or closed with, is none of
        // the program's.
        if inputs.controls.carried.blanked() {
            machine.rt.input.drop_taps();
        }
        if let Some(usage) = inputs.bench_usb.take() {
            machine.rt.input.live.set_key(usage, true);
        }
        if let Ok(path) = font_choice.try_recv() {
            let settings = &mut machine.rt.settings;
            settings.smilebasic_instruments = false;
            settings.sound_font = Some(path.to_string_lossy().into_owned());
            use_instruments(machine, bank.as_ref(), &mut toasts);
        }
        if let Ok(path) = extra_font_choice.try_recv() {
            machine.rt.settings.editor_extra_font = Some(path.to_string_lossy().into_owned());
            if let Err(e) = machine.rt.load_extra_font() {
                toasts.push(e);
            }
        }
        if let Ok((path, transfer)) = folder_choice.try_recv()
            && let Some(open) = &mut screens.manager
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
        machine.set_changes(&debugger.panel.change_watches());
        bench.lap(Stage::Events);
        // Each frame reads the setting, whichever screen or reload changed it;
        // a subprogram, a tool among them, runs on the same machine.
        apply_frame_setting(machine);
        let frames = run_frames(
            machine,
            &mut frame_guard,
            &mut toasts,
            &mut pacer,
            &audio,
            &mut bench,
            paused,
        );
        let (run_began, ran) = (frames.began, frames.took);
        {
            let shown = !pacing::hidden(canvas.window()) && !bench.occluded();
            let trip = frames.trip(machine, !paused && shown);
            let project = machine.rt.storage.project();
            let program =
                (machine.is_running()).then(|| (project, machine.rt.slot_names[0].as_str()));
            slow_hint.trip(run_began, trip, program);
        }
        bench.lap(Stage::Run);
        // A step that waits for input needs the keys on the screen, and
        // the editor takes them back at the stop, unless the user has
        // gone to another window meanwhile.
        let step_waits = machine.step_wants_keys();
        if step_waits && editor_shown && editor_focused && !keys_lent {
            canvas.window_mut().raise();
            keys_lent = true;
        } else if !step_waits
            && std::mem::take(&mut keys_lent)
            && canvas.window().has_input_focus()
            && let Some(gfx) = editor_gfx.get().filter(|_| editor_shown)
        {
            gfx.canvas.borrow_mut().window_mut().raise();
        }
        // A pointer the program moved with MOUSE stays there until the
        // real one moves, and the real one goes there too, unless Settings
        // keep the computer's pointer where it is: then the program sees
        // the move only until the next frame, which reads the real one.
        let (sent, now) = (&mut inputs.controls.mouse, machine.rt.input.live.mouse);
        let fed = !modal
            && !paused
            && !machine.editor_has_keys()
            && !machine.rt.settings.keep_mouse_cursor;
        if fed && (sent.x, sent.y, sent.display) != (now.x, now.y, now.display) {
            let (x, y) = if (sent.x, sent.y) != (now.x, now.y) {
                main_map.screen_to_window(now.x, now.y)
            } else {
                main_map.lcd(simple_ui).display_to_window(now.display)
            };
            mouse.warp_mouse_in_window(canvas.window(), x, y);
            (sent.x, sent.y, sent.display) = (now.x, now.y, now.display);
        }
        // The editor leaves its top row for the menu bar.
        menu.pinned = machine.is_editing() && (!machine.is_detached() || ui_editor);
        menu.running = machine.is_running();
        // Settings change through the menu, keys and questions; keep each
        // change at once so that nothing is lost if the emulator crashes.
        if machine.rt.settings != saved_settings && !bench.on() {
            saved_settings = machine.rt.settings.clone();
            save_settings(&settings_path, &saved_settings);
        }
        toasts.tick();
        inputs
            .controls
            .key_buttons
            .update(machine.rt.input.take_keys_read());
        // A program that keeps every key to itself has no use for the
        // picture yet; it may still come to read only buttons.
        if machine.rt.input.take_buttons_read() && !inputs.controls.key_buttons.all_reserved() {
            let keys = machine.rt.settings.keyboard_buttons;
            key_hint.buttons_read(keys, inputs.controls.gamepads());
        }
        let settings = &machine.rt.settings;
        if corner::tick(&mut key_hint, &mut slow_hint, settings, paused || modal) {
            bench.slow_hint_shown();
        }
        for notice in machine.take_notices() {
            toasts.push(notice);
        }
        screens.follow_confirmation(machine, (display_w, display_h));
        // Centered on the screen, which goes below the desktop's display
        // in a window narrower than 16:9.
        follow_program_dialog(&mut program_dialog, machine, (display_w, main_ui_h));
        blink = blink.wrapping_add(1);

        // A drawing that would end after the frame's time is left out if
        // the program is still making the scene, or if it would cost the
        // program its next frame; see `Scene::drawing`. What the window
        // shows besides the program's screens is drawn every frame, and so
        // is the frame after it went.
        let must = paused
            || modal
            || !machine.is_running()
            || machine.is_editing()
            || machine.sub_running()
            || machine.is_stepping()
            || machine.panel_open()
            || machine.program_dialog().is_some()
            || main_presents.excused();
        let drawn_from = run_began + ran;
        let cost = pacer.draw_cost();
        let drawing = pacer.drawing(drawn_from, must, || pacing::Late {
            // A window that is not shown is not presented anyway, and the
            // bench's shot is of this frame.
            wanted: pacing::hidden(canvas.window()) || bench.occluded() || bench.shoots(),
            mid_loop: machine.mid_loop(),
            fits: fits_frame(machine, cost),
            run: machine.loop_time(),
        });
        if drawing != pacing::Drawing::Draw {
            bench.undrawn();
            let wait = pacer.skipped(Instant::now());
            if drawing == pacing::Drawing::Wait {
                pacing::delay(wait);
            }
            bench.lap(Stage::Wait);
            continue;
        }

        bench.skip();
        bench.burn();
        mode = machine.screen_mode();
        let settings = &machine.rt.settings;
        let rect;
        (rect, filter) = match simple_ui {
            true => screen_place(&mode, settings, true, out_w, screen_h),
            false => placement::desktop_screen_place(&mode, settings, (out_w, out_h), display),
        };
        let editing = machine.is_editing() && !machine.is_detached();
        // The main window's editor laid out for the room it has, under the
        // menu bar as the window shows it. Worked out only while it shows:
        // a trip that draws a program's screen does nothing for the editor.
        let fill = editing.then(|| {
            let scale = placement::menu_scale(display, display_w, (out_w, out_h));
            let bar = (MenuBar::HEIGHT as f32 * scale).ceil() as u32;
            let look = (settings.editor_font, settings.editor_size);
            placement::editor_fill(display, (out_w, out_h), look, bar)
        });
        if let Some(fill) = fill {
            machine.set_editor_area(Some(fill.area));
        }
        let renderer = machine.rt.settings.renderer;
        if paths.follow(&canvas, renderer) {
            machine.rt.touch();
        }
        // The editor's frame, and the screens', stay as they are while
        // their revision does.
        let revision = match editing {
            true => {
                machine.render_editor();
                (true, machine.editor_revision())
            }
            false => {
                match &mut paths.main {
                    // The debugger's panel wants the frame on the CPU.
                    Some(gpu) if !debugger.was_shown => machine.render_screens_with(gpu),
                    _ => {
                        machine.render();
                    }
                }
                (false, machine.screen_revision())
            }
        };
        let fb = match editing {
            true => machine.editor_frame(),
            false => machine.screen_frame(),
        };
        let frame = (fb.width(), fb.height());
        // The editor's frame, of the fill's area, fills the window at its
        // zoom.
        let rect = fill.map_or(rect, placement::Fill::rect);
        filled_editor = fill.map(|_| rect);
        // The frame the GPU path drew, which is not in `fb`.
        let on_gpu = !editing && machine.rt.frame_on_backend();
        // XSCREEN's mode 1 starts from a frame that stays sharp.
        let smart_frame = !editing && filter == Filter::SmartNearest;
        // The editor's frame can be finer than the screen it is shown in.
        let kind = if editing {
            pointer_space = frame;
            // Each of its pixels is a whole number of the window's.
            (frame, Filter::Nearest)
        } else {
            pointer_space = (mode.width, mode.height);
            (frame, if smart_frame { Filter::Nearest } else { filter })
        };
        let showing = Showing {
            revision,
            kind,
            rect,
            on_gpu,
            smart: smart_frame,
            cpu_smart: bench.cpu_smart,
        };
        let gpu_view = drawing!(upload_frame(
            (&mut texture, &mut texture_kind),
            &mut textures,
            &mut paths,
            (&mut canvas, texture_creator),
            fb,
            showing,
        ));
        bench.lap(Stage::Screen);
        // The editor in its own window, where the debugger's panel goes
        // too: where its frame and the UI are there, and the UI's size.
        let mut editor_place = (rect, frame, display, (display_w, display_h));
        // ...and where the emulator's windows go on it.
        let mut windows_place = display;
        if let (true, Some(gfx)) = (apart, editor_gfx.get()) {
            let settings = &machine.rt.settings;
            let layout = drawing!(gfx.layout(settings, (display_w, display_h), ui_scale));
            machine.set_editor_area(Some(layout.area));
            let editor_canvas = gfx.canvas.borrow();
            let pixels = drawing!(editor_canvas.output_size());
            windows_place = windows_rect((display_w, display_h), pixels, layout.ui);
            machine.render_editor();
            let revision = Some(machine.editor_revision());
            let fb = machine.editor_frame();
            let frame = (fb.width(), fb.height());
            editor_space = frame;
            // Each of its pixels is a whole number of the window's.
            let kind = (frame, Filter::Nearest);
            if textures.editor.as_ref().is_none_or(|(_, k)| *k != kind) {
                let made = drawing!(frame_texture(&editor_canvas, gfx.creator.get(), kind));
                textures.editor = Some((made, kind));
                textures.editor_holds = None;
            }
            if let Some((tex, _)) = &mut textures.editor
                && revision != textures.editor_holds
            {
                bench::uploaded(fb.pixels().len() * 4);
                drawing!(tex.update(None, as_bytes(fb.pixels()), (frame.0 * 4) as usize));
                textures.editor_holds = revision;
            }
            let (rect, ui) = (layout.frame_pixels(), layout.ui_pixels());
            editor_place = (from_corner(rect), frame, from_corner(ui), layout.ui_size);
        }
        bench.lap(Stage::Editor);
        // The debugger's panel fills the room the editor leaves below.
        let mut debug = None;
        if let Some(at) = machine.panel_at() {
            let (rect, frame, display, (ui_w, ui_h)) = editor_place;
            // To the main window's bottom, below its display too.
            let bottom = if apart { ui_h } else { main_ui_h };
            let y = rect.y + at as f32 * rect.h / frame.1 as f32 - display.y;
            let top = ((y * ui_h as f32 / display.h).round() as u32).min(bottom);
            let place = ui::Rect {
                x: 0,
                y: top,
                w: ui_w,
                h: bottom - top,
            };
            // A stop, though at the same place, shows the panel anew.
            let fresh = machine.paused_at() != debugger.shown_stop || !debugger.was_shown;
            if fresh {
                debugger.shown_stop = machine.paused_at();
                debugger.panel.reset();
            }
            // The screens change only while a step runs. Beside the
            // editor's own window, they show in the main one.
            let screens = !apart && machine.panel_screens();
            if screens && (fresh || machine.is_running() || textures.panel_screens.is_none()) {
                let screens = machine.render_screens();
                let size = (screens.width(), screens.height());
                let shown = DebugPanel::screen_place(place, size);
                let filter = match shown.h as f32 * display.h / display_h as f32 >= size.1 as f32 {
                    true => Filter::Nearest,
                    false => Filter::Bilinear,
                };
                if textures
                    .panel_screens
                    .as_ref()
                    .is_none_or(|(_, kind)| *kind != (size, filter))
                {
                    let made = drawing!(frame_texture(&canvas, texture_creator, (size, filter)));
                    textures.panel_screens = Some((made, (size, filter)));
                }
                if let Some((tex, _)) = &mut textures.panel_screens {
                    let pixels = as_bytes(screens.pixels());
                    bench::uploaded(pixels.len());
                    drawing!(tex.update(None, pixels, (size.0 * 4) as usize));
                }
            }
            let size = textures
                .panel_screens
                .as_ref()
                .map_or((1, 1), |(_, (size, _))| *size);
            let view = debug_view(machine, &debugger.panel);
            debug = Some((place, view, screens.then_some(size)));
        }
        debugger.was_shown = debug.is_some();
        bench.lap(Stage::Gather);
        canvas.set_draw_color(Color::BLACK);
        canvas.clear();
        let screen = textures
            .smart
            .texture()
            .or(gpu_view.as_deref())
            .unwrap_or(&texture);
        drawing!(canvas.copy(screen, None, rect));
        let lcd = main_map.lcd(simple_ui);
        drawing!(draw_sub_screen(
            machine,
            &mut paths,
            &mut textures.sub,
            (&mut canvas, texture_creator),
            lcd,
            !debugger.was_shown,
        ));
        // Where the focus has just gone: a yellow line on the edge, one
        // pixel out and two in, then a dark band two pixels wide, fading
        // out together.
        if let Some((rect, alpha)) = machine.focus_border() {
            let lcd = main_map.lcd(simple_ui);
            let place = placement::border_place(rect, main_map, lcd, display_h, main_ui_h);
            drawing!(draw::focus_border(&mut canvas, place, alpha));
        }
        // The debugger's panel leaves the screens' place clear.
        if let (Some((place, _, Some(size))), Some((tex, _))) = (&debug, &textures.panel_screens) {
            let s = DebugPanel::screen_place(*place, *size);
            let dst = main_map.on_window([s.x as f32, s.y as f32, s.w as f32, s.h as f32]);
            drawing!(canvas.copy(tex, None, dst));
        }
        let (grp, panel_grp) = (&machine.rt.grp, &debugger.panel.grp);
        match (&debug, apart) {
            (Some(_), false) => drawing!(textures.grp.draw(
                &mut canvas,
                texture_creator,
                grp,
                panel_grp,
                display,
                display_w,
            )),
            _ => textures.grp.release(),
        }
        if debug.is_none() || !apart {
            textures.editor_grp.release();
        }
        // The colors window leaves the editor behind it bright, to show
        // its colors.
        let dims =
            screens.dialog.is_some() || screens.manager.is_some() || screens.pause_menu.is_some();
        // A running program shows the mouse cursor only if it asks to,
        // unless the emulator's own screens or the editor's window need
        // it, or Select has the right stick for the pointer. The Simple
        // UI's own screens have none. Where the system shows no cursor,
        // the emulator draws its own.
        let stick = stick_role!();
        let show_cursor = !stick.hides_cursor()
            && (!machine.is_running()
                || modal
                || dims
                || program_dialog.is_some()
                || menu.visible()
                || stick == stick_mouse::StickRole::Screens
                || apart && pointer_in_editor
                || machine.rt.settings.keep_mouse_cursor
                || machine.rt.mouse_cursor()
                || inputs.controls.stick_mouse.select_points());
        let system_cursor = show_cursor && !overlays.cursor.on();
        if system_cursor != cursor_shown {
            cursor_shown = system_cursor;
            mouse.show_cursor(system_cursor);
        }
        // Which window each part goes on: the main one, or the editor's.
        // The menu bar and the emulator's windows go with the keys, the
        // editor's panels and menu with the editor.
        let ui = usize::from(ui_editor);
        let ed = usize::from(apart);
        let side_on = if side_panel.direct { 0 } else { ed };
        // Beside the editor's own window, the chosen sprite is outlined
        // on the main one's screen, and a subprogram's always on its own,
        // where the panel leaves it.
        let outline = (debug.as_ref()).and_then(|(place, view, _)| {
            let list = view.sprites.as_ref()?;
            let outline = list.outline?;
            match list.sub {
                Some(sub) => {
                    let top = if apart { display_h } else { place.y };
                    let (at, clip) =
                        debug_panel::sprites::sub_place(sub.place?, (display_w, display_h), top)?;
                    Some((outline, sub.frame, at, clip))
                }
                None if apart => {
                    let ui = (display_w, display_h);
                    let (at, clip) = placement::screen_on_ui(rect, display, ui, main_ui_h);
                    Some((outline, frame, at, clip))
                }
                None => None,
            }
        });
        // Each window's UI in its own pixels. On the editor's window, the
        // emulator's windows go on a layer of their own, laid out on the
        // display as on the main window. The main window's goes below its
        // display, where the screen goes too.
        let ui_sizes = [(display_w, main_ui_h), editor_place.3];
        let windows_apart = apart && ui == 1 && screens.any();
        if let Some(open) = &mut side_panel.open {
            open.set_size(ui_sizes[side_on]);
        }
        // The menu bar shows over everything in the window with the keys,
        // and the editor's window always shows it.
        let mut bar_shown = [false; 2];
        bar_shown[ui] = !simple_ui && screens.dialog.is_none() && !modal && menu.visible();
        bar_shown[1] |= apart && ui == 0;
        let closed_bar = menu.closed();
        let font = machine.rt.font();
        ui::set_font(machine.rt.settings.ui_font);
        ui::set_bundled_glyphs(machine.rt.settings.bundled_glyphs16);
        let cursor_on = (blink / 25).is_multiple_of(2);
        let open = menu.is_open().then_some(profile_display);
        let context = menu_context(machine, canvas.window(), open, editor_room);
        let settings = &machine.rt.settings;
        let editor_creator = editor_gfx
            .get()
            .filter(|_| apart)
            .map(|gfx| gfx.creator.get());
        let creators = [Some(texture_creator), editor_creator];
        let scale = settings.debug_text_size;
        match &debug {
            Some((_, view, _)) => {
                debugger.panel.advance(view);
                overlays.panel[1 - ed].release();
            }
            None => {
                debugger.panel.hide();
                overlays.panel.iter_mut().for_each(Layer::release);
            }
        }
        // Whether the panel is drawn with the rest of the window's UI
        // rather than on its own.
        let mut panel_inside = [false; 2];
        for (i, creator) in creators.into_iter().enumerate() {
            let Some(creator) = creator else {
                continue;
            };
            let size = ui_sizes[i];
            // The main window's menus at 1x, apart from its display.
            let menus_apart = menus_apart.filter(|_| i == 0);
            if bar_shown[i] {
                let bar = if i == ui { &menu } else { &closed_bar };
                let width = menus_apart.map_or(size.0, |place| place.size.0);
                drawing!(
                    overlays.bar[i].update(creator, (width, MenuBar::HEIGHT), |fb| {
                        bar.draw_bar(fb, font)
                    })
                );
            }
            // A dialog or the project manager dims the screen behind it,
            // as a program's DIALOG does, but not the panel and the bar
            // over it.
            let dim = i == ui && dims || i == 0 && program_dialog.is_some();
            let panel = debug.as_ref().filter(|_| i == ed).map(|(place, ..)| *place);
            // The bar's height on this UI, which is more than its own where
            // it goes apart at 1x from a display scaled below.
            let bar_h = match menus_apart {
                Some(_) => (MenuBar::HEIGHT as f32 * display_w as f32 / display.w).ceil() as u32,
                None => MenuBar::HEIGHT,
            };
            let bar = ui::Rect {
                x: 0,
                y: 0,
                w: size.0,
                h: bar_h,
            };
            let inside = Cell::new(false);
            // The UI in the window's pixels, and the emulator's windows,
            // which go on a layer of their own on the editor's window.
            let windows_on_layer = i == 1 && windows_apart;
            let shown = draw::Ui {
                window: i,
                ui,
                ed,
                side_on,
                windows_on_layer,
                dim,
                panel,
                debug: &debug,
                outline: &outline,
                notices: i == 0 && !paused && !modal && !simple_ui,
                key_hint: &key_hint,
                slow_hint: &slow_hint,
                side: &side_panel,
                screens: &screens,
                program_dialog: &program_dialog,
                popup: &debugger.menu,
                toasts: &toasts,
                menu: &menu,
                context,
                bar_shown: bar_shown[i],
                menus_apart: menus_apart.is_some(),
                bar,
                font,
                settings,
                scale,
                cursor_on,
            };
            let parts = |fb: &mut Framebuffer, debug_panel: &mut DebugPanel, windows: bool| {
                shown.draw(fb, debug_panel, inside.get(), windows)
            };
            if let (Some(place), Some((_, view, screens))) = (panel, &debug) {
                // What is partly see-through over the panel shows what is
                // under the panel, as it always did, if drawn with it.
                let probe =
                    overlays.overlay[i].dry_run(size, |fb| parts(fb, &mut debugger.panel, false));
                inside.set(probe.translucent.is_some_and(|t| t.overlaps(place)));
                panel_inside[i] = inside.get();
                if !inside.get() {
                    drawing!(overlays.panel[i].update(creator, (place.w, place.h), |fb| {
                        debugger.panel.draw(fb, font, place, *screens, view, scale)
                    }));
                }
            }
            let under = |fb: &mut Framebuffer| parts(fb, &mut debugger.panel, false);
            drawing!(overlays.overlay[i].update(creator, size, under));
            if windows_on_layer {
                let over = |fb: &mut Framebuffer| parts(fb, &mut debugger.panel, true);
                drawing!(
                    overlays
                        .windows
                        .update(creator, (display_w, display_h), over)
                );
            }
        }
        if !windows_apart {
            overlays.windows.release();
        }
        // The main window's open menus and context menu, apart at 1x.
        let lists = ui == 0 && bar_shown[0] && screens.dialog.is_none() && menu.is_open();
        let popup = (debugger.menu.as_ref()).filter(|_| ed == 0);
        match menus_apart {
            Some(place) if lists || popup.is_some() => {
                drawing!(overlays.menu.update(texture_creator, place.size, |fb| {
                    if lists {
                        menu.draw_lists(fb, font, settings, context);
                    }
                    if let Some(open) = popup {
                        open.draw(fb, font);
                    }
                }));
            }
            _ => overlays.menu.release(),
        }
        // The Simple UI's notices, under the rest of its UI as on the
        // desktop. Neither shows with the pause menu or the game list, so
        // they stay clear of the bar of hints at the bottom; the slow
        // program's notice waits for the keys' picture to go.
        let notice = corner::visible(&key_hint, &slow_hint);
        let notice = (simple_ui && notice && !paused && !modal).then(|| {
            let size = settings.simple_ui_size;
            simple_ui::notice_place((out_w, out_h), size)
        });
        match notice {
            Some((ui, _)) => {
                drawing!(overlays.notice.update(texture_creator, ui, |fb| {
                    corner::draw(fb, font, &key_hint, &slow_hint, true);
                }));
            }
            None => overlays.notice.release(),
        }
        let places = overlays::MainPlaces {
            display,
            display_w,
            panel: match (ed, &debug, panel_inside[0]) {
                (0, Some((place, ..)), false) => Some((place.x, place.y)),
                _ => None,
            },
            notice: notice.map(|(ui, place)| (place, ui.0)),
            bar: bar_shown[0],
            menus: menus_apart,
        };
        drawing!(overlays.copy_main(&mut canvas, places));
        let (at, lcd) = (inputs.controls.mouse.display, main_map.lcd(simple_ui));
        drawing!(overlays.cursor.draw(
            &mut canvas,
            texture_creator,
            show_cursor,
            at,
            lcd.display,
            lcd.display_w
        ));
        bench.lap(Stage::Overlays);
        bench.shoot(&canvas, "main");
        bench.measure(&canvas);
        main_presents.want(&canvas, wants_vsync(&machine.rt.settings));
        let vsync = main_presents.vsync();
        let present_began = Instant::now();
        let presented = main_presents.present(&mut canvas, Some(&bench));
        let present_took = present_began.elapsed();
        bench.lap(Stage::Present);
        if let (true, Some(gfx)) = (apart, editor_gfx.get()) {
            let mut editor_canvas = gfx.canvas.borrow_mut();
            editor_canvas.set_draw_color(Color::BLACK);
            editor_canvas.clear();
            if let Some((tex, _)) = &textures.editor {
                drawing!(editor_canvas.copy(tex, None, editor_place.0));
            }
            let (display, ui_w) = (editor_place.2, editor_place.3.0);
            let canvas = &mut *editor_canvas;
            if debug.is_some() {
                let (grp, panel_grp) = (&machine.rt.grp, &debugger.panel.grp);
                let creator = gfx.creator.get();
                drawing!(
                    textures
                        .editor_grp
                        .draw(canvas, creator, grp, panel_grp, display, ui_w)
                );
            }
            let places = overlays::EditorPlaces {
                display,
                ui_w,
                panel: match (&debug, panel_inside[1]) {
                    (Some((place, ..)), false) => Some((place.x, place.y)),
                    _ => None,
                },
                bar: bar_shown[1],
                windows: windows_apart.then_some((windows_place, display_w)),
            };
            drawing!(overlays.copy_editor(canvas, places));
            bench.lap(Stage::Overlays);
            bench.shoot(&editor_canvas, "editor");
            let presents =
                editor_presents.get_or_insert_with(|| pacing::Presents::new(&editor_canvas, false));
            presents.present(&mut editor_canvas, None);
            bench.lap(Stage::Present);
        }
        if iterations.is_multiple_of(60) {
            // The window can move to another display.
            pacer.display(pacing::at_60_hz(canvas.window()) || bench.vblank());
        }
        iterations = iterations.wrapping_add(1);
        pacer.drew(pacing::Drew {
            trip_began,
            trip: trip_began.elapsed(),
            ran,
            drawn_from,
            present_began,
            present_took,
            waited: presented && vsync,
            vsync: main_presents.vsync(),
        });
        pacing::delay(pacer.end(Instant::now()));
        bench.lap(Stage::Wait);
        watch.frame_shown();
    }
    // The editor's window, shown when quitting, opens there next time.
    let keep = machine.rt.settings.editor_window_keep_place;
    if let Some(gfx) = editor_gfx.get().filter(|_| editor_shown && keep) {
        let place = window_place(gfx.canvas.borrow().window());
        machine.rt.settings.editor_window_place = Some(place);
    }
    // The desktop starts fullscreen next time if it is now, whoever made
    // it so, and the window as large as it is now, unless fullscreen or
    // maximized, which keep the size it had before.
    if !simple_ui {
        let fullscreen = main_window.fullscreen_state() != FullscreenType::Off;
        machine.rt.settings.fullscreen = fullscreen;
        if !fullscreen && !main_window.is_maximized() {
            let (w, h) = main_window.size();
            machine.rt.settings.window_size = Some([w, h]);
        }
    }
    // A bench run leaves the settings as they were, as it does above, so
    // that runs in one data directory start alike.
    if machine.rt.settings != saved_settings && !bench.on() {
        save_settings(&settings_path, &machine.rt.settings);
    }
    Ok(())
}

/// Which of the emulator's screens takes the input, so that a change can
/// be told: the pause menu, a dialog, the manager in one of its modes, the
/// game list in one of its, or none.
fn input_screen(pause: bool, dialog: bool, manager: Option<usize>, list: Option<usize>) -> usize {
    match (pause, dialog, manager, list) {
        (true, ..) => 20,
        (false, true, ..) => 1,
        (false, false, Some(mode), _) => 2 + mode,
        (false, false, None, Some(mode)) => 21 + mode,
        (false, false, None, None) => 0,
    }
}

// The Simple UI goes back to its game list with the screens as ACLS
// leaves them and the sound stopped, as `tidy_on_end` has a program end,
// whatever that setting says: after END, STOP, the + button, an error
// once A is pressed, Return to Game list and Restart.

/// Whether the program the game list ran has ended by itself or by the +
/// button, tidied if so. An error waits for A instead.
fn simple_ui_ended(machine: &mut Machine) -> bool {
    let done = machine.home_done();
    if done {
        machine.tidy();
    }
    done
}

/// Stops the program the game list ran, or ends the wait after its error,
/// for the game list or a restart.
fn simple_ui_leave(machine: &mut Machine) {
    machine.stop();
    machine.home_leave();
    machine.home_done();
    machine.tidy();
}

/// The keys of the desktop's windows and tools, which the Simple UI leaves
/// alone: the function keys, but for F5 alone, which is the + button.
fn desktop_key(key: Keycode, keymod: Mod) -> bool {
    let held = Mod::LCTRLMOD | Mod::RCTRLMOD | Mod::LSHIFTMOD | Mod::RSHIFTMOD;
    match key {
        Keycode::F5 => keymod.intersects(held | Mod::LALTMOD | Mod::RALTMOD),
        Keycode::F1
        | Keycode::F2
        | Keycode::F3
        | Keycode::F4
        | Keycode::F6
        | Keycode::F7
        | Keycode::F8
        | Keycode::F9
        | Keycode::F10
        | Keycode::F11
        | Keycode::F12 => true,
        _ => false,
    }
}

/// Runs a program that the game list chose, in its project, to come back
/// to the list when it ends. False, with a notice, if it could not.
fn run_from_list(machine: &mut Machine, project: &str, file: &str, toasts: &mut Toasts) -> bool {
    let storage = &mut machine.rt.storage;
    if storage.project() != project
        && let Err(e) = storage.set_project(project)
    {
        let label = sb_runtime::confirm::project_label(project);
        toasts.push(format!("Could not switch to project {label}: {e}"));
        return false;
    }
    match machine.run_home(file) {
        Ok(()) => true,
        Err(e) => {
            toasts.push(format!("Could not load {file}: {e}"));
            false
        }
    }
}

fn save_settings(path: &Path, settings: &Settings) {
    if let Err(e) = std::fs::write(path, settings.to_text()) {
        eprintln!("could not save settings to {}: {e}", path.display());
    }
}

/// The editor's own window, with its renderer.
struct EditorWindow {
    canvas: RefCell<Canvas<Window>>,
    creator: OwnedCreator,
    id: u32,
    /// The smallest size it was last allowed, for its UI scale.
    least: Cell<(u32, u32)>,
}

impl EditorWindow {
    /// Makes the window, hidden until the editor opens: where it was last
    /// if that is still on a display, else beside the main window if the
    /// display has room on one of its sides, else where the system puts
    /// it.
    fn open(
        video: &VideoSubsystem,
        (w, h): (u32, u32),
        last: Option<[i32; 4]>,
        main: &Window,
    ) -> Result<EditorWindow, AnyError> {
        const GAP: i32 = 16;
        let displays: Vec<Rect> = (video.displays().unwrap_or_default().iter())
            .filter_map(|d| d.get_bounds().ok())
            .collect();
        if let Some([x, y, w, h]) = last.filter(|&place| on_a_display(place, &displays)) {
            let mut builder = video.window(EDITOR_TITLE, w as u32, h as u32);
            builder.position(x, y);
            return EditorWindow::build(video, builder);
        }
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
        EditorWindow::build(video, builder)
    }

    fn build(video: &VideoSubsystem, mut builder: WindowBuilder) -> Result<EditorWindow, AnyError> {
        let window = builder.resizable().high_pixel_density().hidden().build()?;
        video.text_input().start(&window);
        let id = window.id();
        let canvas = make_canvas(&window, &[])?;
        let creator = OwnedCreator::new(&canvas);
        Ok(EditorWindow {
            canvas: RefCell::new(canvas),
            creator,
            id,
            least: Cell::new((0, 0)),
        })
    }

    /// How the window is laid out at its size and the desktop's scale,
    /// or at `scale` if given, for the profile's `display`. It keeps its
    /// smallest size to what the UI needs at that scale.
    fn layout(
        &self,
        settings: &Settings,
        display: (u32, u32),
        scale: Option<u32>,
    ) -> Result<Layout, AnyError> {
        let mut canvas = self.canvas.borrow_mut();
        let pixels = canvas.output_size()?;
        let window = canvas.window_mut();
        let scale = scale.unwrap_or_else(|| editor_layout::ui_scale(window.display_scale()));
        let least = editor_layout::min_window(scale, window.pixel_density());
        if self.least.replace(least) != least {
            let _ = window.set_minimum_size(least.0, least.1);
        }
        let look = (settings.editor_font, settings.editor_size);
        Ok(Layout::new(pixels, scale, look, display, MenuBar::HEIGHT))
    }
}

/// A renderer for `window`: the one SDL picks or `SDL_RENDER_DRIVER` names, and if
/// that cannot start, any other. What sdl3's `into_canvas` does here is panic.
///
/// `avoid` names drivers that have just failed, which are not tried.
fn make_canvas(window: &Window, avoid: &[String]) -> Result<Canvas<Window>, AnyError> {
    let mut first = None;
    if avoid.is_empty() {
        match create_renderer(window.clone(), None) {
            Ok(canvas) => return Ok(canvas),
            Err(e) => {
                eprintln!("no renderer ({e}); trying the others");
                first = Some(e.to_string());
            }
        }
    }
    // SAFETY: the names are SDL's own, kept as long as it runs.
    let names = (0..unsafe { SDL_GetNumRenderDrivers() })
        .map(|i| unsafe { SDL_GetRenderDriver(i) })
        .filter(|name| !name.is_null())
        .map(|name| unsafe { CStr::from_ptr(name) })
        .filter(|name| !avoid.iter().any(|d| name.to_str() == Ok(d)));
    for name in names {
        match create_renderer(window.clone(), Some(name)) {
            Ok(canvas) => {
                eprintln!("using the renderer {}", canvas.renderer_name);
                return Ok(canvas);
            }
            Err(e) => {
                first.get_or_insert_with(|| e.to_string());
            }
        }
    }
    let first = first.unwrap_or_else(|| "every driver failed".to_string());
    Err(format!("No renderer could start: {first}").into())
}

/// What goes on while there is no renderer to draw with.
struct Waiting<'a> {
    events: &'a mut sdl3::EventPump,
    controls: &'a mut Controls,
    /// Tries to refuse, for the bench.
    refusals: &'a mut u32,
    /// No message box, with no display.
    quiet: bool,
}

/// A new renderer for `window`, with its creator and a frame's texture. With none to
/// be had, it keeps trying every second with the program paused and its sound running
/// out: a game nobody can see should not go on being played, and nobody can answer
/// it. Programs go to files at once, as the wait may never end. None if the user closes
/// the window with nothing unsaved.
fn new_renderer(
    window: &Window,
    kind: TextureKind,
    watch: &RendererWatch,
    machine: &Machine,
    recovery: &mut Recovery,
    waiting: &mut Waiting,
) -> Result<Option<(Canvas<Window>, OwnedCreator, Texture<'static>)>, AnyError> {
    let began = Instant::now();
    let mut told = false;
    let mut saved = None;
    let mut window = window.clone();
    let mut avoid = watch.avoid().to_vec();
    loop {
        let made = match *waiting.refusals {
            0 => make_canvas(&window, &avoid),
            _ => {
                *waiting.refusals -= 1;
                Err("refused by the bench".into())
            }
        };
        let why = match made {
            Ok(canvas) => {
                let creator = OwnedCreator::new(&canvas);
                match frame_texture(&canvas, creator.get(), kind) {
                    Ok(texture) => {
                        let _ = window.set_title(TITLE);
                        return Ok(Some((canvas, creator, texture)));
                    }
                    // It starts but cannot hold a frame: on to the next driver.
                    Err(e) => {
                        avoid.push(canvas.renderer_name.clone());
                        e.to_string()
                    }
                }
            }
            // With nothing else to be had, any may do by now.
            Err(e) => {
                avoid.clear();
                e.to_string()
            }
        };
        if saved.is_none() {
            eprintln!("no renderer, trying again every second: {why}");
            let _ = window.set_title(&format!("{TITLE}: no renderer, trying again"));
            saved = Some(recovery.save(machine));
        }
        if !told && !waiting.quiet && began.elapsed() >= recovery::TELL_AFTER {
            told = true;
            let text = "Nothing can be drawn at the moment, so the program is paused. \
                        The emulator will keep trying.";
            let lines = saved.clone().flatten();
            let text = failure_message(text, lines);
            let _ = show_simple_message_box(MessageBoxFlag::WARNING, TITLE, &text, None);
        }
        let until = Instant::now() + recovery::RETRY;
        while Instant::now() < until {
            for mut event in waiting.events.poll_iter() {
                waiting.controls.event(&mut event);
                let closed = matches!(
                    event,
                    Event::Quit { .. }
                        | Event::Window {
                            win_event: WindowEvent::CloseRequested,
                            ..
                        }
                );
                if closed {
                    return match machine.unsaved_slots().is_empty() {
                        true => Ok(None),
                        false => Err(format!("No renderer could start: {why}").into()),
                    };
                }
            }
            pacing::delay(Duration::from_millis(50));
        }
    }
}

/// Puts one of the renderer's events on SDL's queue: its targets or its
/// device reset, or its device gone. The sdl3 crate's `push_event`
/// refuses the first two and has no event of its own for the third.
fn push_render_event(kind: SDL_EventType) {
    // SAFETY: an all-zero event is valid, and the type is the only field read.
    unsafe {
        let mut event: SDL_Event = std::mem::zeroed();
        event.r#type = kind.0;
        SDL_PushEvent(&mut event);
    }
}

/// A window's position and size, as the settings keep them.
fn window_place(window: &Window) -> [i32; 4] {
    let ((x, y), (w, h)) = (window.position(), window.size());
    [x, y, w as i32, h as i32]
}

/// Whether a window placed so lies wholly on one of the displays, so that
/// it can go back there.
fn on_a_display([x, y, w, h]: [i32; 4], displays: &[Rect]) -> bool {
    let place = Rect::new(x, y, w.max(1) as u32, h.max(1) as u32);
    displays.iter().any(|d| d.contains_rect(place))
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

/// The computer's clipboard, for CLIPBOARD. SDL's clipboard belongs to the
/// main thread, so on the standby thread CLIPBOARD reads the text last read
/// or set, and a text set there waits for [`Clipboard::settle`].
///
/// [`Clipboard::settle`]: sb_runtime::env::Clipboard::settle
struct SystemClipboard {
    sdl: ClipboardUtil,
    main: std::thread::ThreadId,
    last: String,
    unsent: bool,
}

impl SystemClipboard {
    fn new(sdl: ClipboardUtil) -> SystemClipboard {
        SystemClipboard {
            sdl,
            main: std::thread::current().id(),
            last: String::new(),
            unsent: false,
        }
    }

    fn on_main(&self) -> bool {
        std::thread::current().id() == self.main
    }
}

impl sb_runtime::env::Clipboard for SystemClipboard {
    // An empty or unreadable clipboard reads as an empty string.
    fn text(&mut self) -> String {
        if self.on_main() {
            self.settle();
            self.last = self.sdl.clipboard_text().unwrap_or_default();
        }
        self.last.clone()
    }

    fn set_text(&mut self, text: &str) {
        self.last = text.to_string();
        self.unsent = true;
        self.settle();
    }

    fn settle(&mut self) {
        if self.unsent && self.on_main() {
            self.unsent = false;
            let _ = self.sdl.set_clipboard_text(&self.last);
        }
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

/// Ctrl+S's or Ctrl+L's window, or the settings' to pick a tool.
enum FileDialog {
    Save(SaveDialog),
    Load(FilePicker),
}

/// Has `view` show the program's DIALOG as the machine has it, on a UI
/// of `size`, with the focus where the controller moved it.
fn follow_program_dialog(view: &mut Option<ProgramView>, machine: &Machine, size: (u32, u32)) {
    match (&mut *view, machine.program_dialog()) {
        (Some(open), Some(shown)) if open.shows(shown) => open.set_size(size),
        (_, Some(shown)) => *view = Some(ProgramView::new(shown, size)),
        (_, None) => *view = None,
    }
    if let Some(open) = view {
        open.follow(machine.dialog_focus());
    }
}

/// The debugger's part of the window.
#[derive(Default)]
struct Debugger {
    /// The panel below the editor.
    panel: DebugPanel,
    /// The stop the panel last showed, by slot and line.
    shown_stop: Option<(usize, u32)>,
    /// Whether the panel showed in the last frame.
    was_shown: bool,
    /// The menu a right click opens on the editor or the panel.
    menu: Option<Popup<Choice>>,
}

/// The help or the index beside the editor.
#[derive(Default)]
struct Beside {
    open: Option<Side>,
    /// Whether it takes the keys.
    focus: bool,
    /// The help was opened in direct mode, over the screen.
    direct: bool,
}

impl Beside {
    /// Shows the help for the command at the cursor and gives it the
    /// keys, if there is help for it; `direct` is whether it was asked for
    /// in direct mode, to show over the screen. With no help to show, a
    /// notice says so and what was open stays.
    fn show_help(
        &mut self,
        machine: &mut Machine,
        help: Option<&Help>,
        toasts: &mut Toasts,
        size: (u32, u32),
        direct: bool,
    ) {
        if let Some(panel) = open_help(machine, help, toasts, size) {
            (self.open, self.focus, self.direct) = (Some(panel), true, direct);
        }
    }
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

    fn set_size(&mut self, size: (u32, u32)) {
        match self {
            Side::Help(help) => help.set_size(size),
            Side::Index(index, _) => index.set_size(size),
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
    /// A command of a running program or direct mode, with what it asks.
    Command(Confirm),
    /// Ctrl+S over another file of this name.
    Overwrite(String),
    /// Ctrl+L over unsaved changes, with the file to load.
    Discard(String),
    /// Something told, such as About, with OK alone.
    Notice,
    /// The condition of a breakpoint, by slot and 1-based line.
    Condition(usize, u32),
    /// The game list's Run with unsaved slots: the project and the file.
    Run(String, String),
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

/// Asks for a font file for the editor's extra characters; the choice
/// comes through `send`.
fn choose_extra_font(window: &Window, send: std::sync::mpsc::Sender<PathBuf>, toasts: &mut Toasts) {
    let filters = [sdl3::dialog::DialogFileFilter {
        name: "Fonts",
        pattern: "bdf;ttf;otf",
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

/// What the menu bar's items need of the emulator and the main window.
/// `display`, the profile's, is given while a menu is open: View > Window
/// size then asks the system for the window's display. `editor_room` is
/// the window the editor shows in, as [`next_editor_size`] takes it.
fn menu_context(
    machine: &Machine,
    main: &Window,
    display: Option<(u32, u32)>,
    (width, display_h): (u32, f32),
) -> Context {
    let fullscreen = main.fullscreen_state() != FullscreenType::Off;
    let (window_size, window_fits) = match display {
        Some(display) => {
            let current = window::current(display, main.size_in_pixels(), fullscreen);
            let usable = main.get_display().and_then(|d| d.get_usable_bounds());
            let usable = usable.map_or((0, 0), |r| (r.width(), r.height()));
            // Known on X11 and Windows; elsewhere the size alone is checked.
            let (top, left, bottom, right) = main.border_size().unwrap_or_default();
            let borders = (top.into(), left.into(), bottom.into(), right.into());
            let fits = window::fitting(display, main.pixel_density(), borders, usable);
            (current, fits)
        }
        None => (None, 0),
    };
    // Which of View > Size's steps look the same, which only an open menu
    // shows: a trip with the menus closed works nothing out for it.
    let same_sizes = match display {
        Some(_) => {
            let settings = &machine.rt.settings;
            let (font, size) = (settings.editor_font, settings.editor_size);
            let distinct = editor_layout::distinct_sizes(width, display_h, font, size);
            (0..distinct.len())
                .filter(|&i| !distinct[i])
                .fold(0, |bits, i| bits | 1 << i)
        }
        None => 0,
    };
    Context {
        running: machine.is_running(),
        editing: machine.is_editing(),
        paused: machine.paused_at().is_some() && !machine.is_running(),
        detached: machine.is_detached(),
        sub: machine.sub_running(),
        fullscreen,
        window_size,
        window_fits,
        same_sizes,
    }
}

/// Sets the main window to `halves` halves of the profile's `display` in
/// pixels, as View > Window size does: out of fullscreen first, and back
/// on the display if the larger window would go past its edge. The menu
/// bar goes over the screen, so the screen gets the whole multiple.
fn set_window_scale(main: &mut Window, settings: &mut Settings, display: (u32, u32), halves: u32) {
    if main.fullscreen_state() != FullscreenType::Off {
        let _ = main.set_fullscreen(false);
        settings.fullscreen = false;
    }
    if main.is_maximized() {
        main.restore();
    }
    let (w, h) = window::coordinates(window::scaled(display, halves), main.pixel_density());
    let _ = main.set_size(w, h);
    let (x, y) = main.position();
    let room = main.get_display().and_then(|d| d.get_usable_bounds());
    if let Ok(room) = room
        && (x + w as i32 > room.x() + room.width() as i32
            || y + h as i32 > room.y() + room.height() as i32)
    {
        main.set_position(WindowPos::Centered, WindowPos::Centered);
    }
}

/// Whether the main window's shape is left as it is: fullscreen or
/// maximized, where the system or the display decides its size.
fn shape_free(main: &Window) -> bool {
    main.fullscreen_state() != FullscreenType::Off || main.is_maximized()
}

/// The room the main window has on its display, in window coordinates:
/// the usable area less the window's borders, where the system says.
fn main_room(main: &Window) -> Option<(u32, u32)> {
    let usable = main
        .get_display()
        .and_then(|d| d.get_usable_bounds())
        .ok()?;
    let (top, left, bottom, right) = main.border_size().unwrap_or_default();
    let less = |v: u32, a: u16, b: u16| v.saturating_sub(u32::from(a) + u32::from(b));
    Some((
        less(usable.width(), left, right),
        less(usable.height(), top, bottom),
    ))
}

/// Keeps the main window at the shape of `display`, 16:9, while `on`:
/// asks the system for that shape, and where the system ignores it, sets
/// the nearest size of that shape once a resize has settled (see
/// [`window::KeepShape`]). `asked` is whether the shape is asked for now.
///
/// A fullscreen or maximized window is left as it is, and so is what it
/// asks for: SDL sets the window's size again when the shape asked for
/// changes, which would take it out of fullscreen on some systems.
fn keep_main_shape(
    main: &mut Window,
    keep: &mut window::KeepShape,
    asked: &mut bool,
    on: bool,
    display: (u32, u32),
) {
    let free = shape_free(main);
    if !free && *asked != on {
        *asked = on;
        let ratio = match on {
            true => display.0 as f32 / display.1.max(1) as f32,
            false => 0.0,
        };
        // SAFETY: the window is alive; SDL takes 0 to clear the ratio.
        unsafe { sdl3::sys::video::SDL_SetWindowAspectRatio(main.raw(), ratio, ratio) };
    }
    let room = main_room(main);
    if let Some((w, h)) = keep.tick(Instant::now(), main.size(), !on || free, display, room) {
        let _ = main.set_size(w, h);
    }
}

/// Turns the main window's fullscreen on or off, as View > Fullscreen and
/// Ctrl+Shift+F11 do, and keeps the choice. Desktop fullscreen: the
/// display's mode stays as it is.
fn toggle_fullscreen(main: &mut Window, settings: &mut Settings) {
    let on = main.fullscreen_state() == FullscreenType::Off;
    let _ = main.set_fullscreen(on);
    settings.fullscreen = on;
}

/// The debugger's keys: Shift+F5 continues and Ctrl+Shift+F5 restarts;
/// F6 steps into, F7 over and Shift+F6 out; Ctrl+F6 runs to the cursor
/// and Ctrl+Shift+F6 goes on from it; Shift+F7 sets a breakpoint and
/// Ctrl+F7 shows or hides the panel.
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
        (Keycode::F7, false, true) => Action::DebugPanel,
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
            return Some(condition_dialog(machine, slot, line, size));
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

/// Asks for the condition of the breakpoint on a slot's line.
fn condition_dialog(
    machine: &mut Machine,
    slot: usize,
    line: u32,
    size: (u32, u32),
) -> (Dialog, Asking) {
    let text = machine.condition(slot, line).unwrap_or_default();
    let lines = vec![
        format!("Stop at {slot}:{line} only when this is true."),
        "Leave it empty to stop always.".to_string(),
    ];
    let open = Dialog::prompt("Breakpoint condition", lines, &text, size);
    (open, Asking::Condition(slot, line))
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
    tabs.push(Tab::Perf);
    tabs.push(Tab::Sprites);
    if machine.sub_runtime().is_some() {
        tabs.push(Tab::SubSprites);
    }
    tabs.push(Tab::Grp);
    tabs.push(Tab::Bgm);
    let tab = panel.tab.filter(|t| tabs.contains(t)).unwrap_or(tabs[0]);
    let call_frame = call.as_ref().and_then(|c| c.frame);
    let (frame, slot) = match tab {
        Tab::Locals => (call_frame, home),
        Tab::Globals(slot) => (None, slot),
        Tab::Perf | Tab::Sprites | Tab::SubSprites | Tab::Grp | Tab::Bgm => (None, home),
    };
    let variables = match (tab, frame) {
        (Tab::Locals, Some(frame)) => machine.frame_locals(frame),
        (Tab::Globals(slot), _) => machine.slot_globals(slot),
        (Tab::Locals, None)
        | (Tab::Perf | Tab::Sprites | Tab::SubSprites | Tab::Grp | Tab::Bgm, _) => Vec::new(),
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
        _ if machine.is_running() => ("Running".to_string(), false, "Ctrl+F7: close"),
        Some((slot, line)) if machine.stop_reason().is_some() || !machine.stopped_by_error() => {
            let (mut status, failed) = stop_text(machine.stop_reason(), slot, line);
            if machine.lines_moved(slot) {
                status.push_str(" - edited since RUN, the old code goes on");
            }
            (status, failed, "F6/F7: step  Shift+F5: go on")
        }
        _ => ("Not running".to_string(), false, "F5: run  Ctrl+F7: close"),
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
        breakpoints: machine.all_breakpoints(),
        perf: *machine.rt.perf_stats(),
        // Only gathered while shown.
        sprites: match tab {
            Tab::Sprites => {
                let (first, rows) = panel.sprite_rows();
                Some(debug_panel::sprites::list(
                    &machine.rt,
                    first,
                    rows,
                    panel.sprite,
                ))
            }
            Tab::SubSprites => machine.sub_runtime().map(|(rt, place)| {
                let (first, rows) = panel.sprite_rows();
                let list = debug_panel::sprites::list(rt, first, rows, panel.sub_sprite);
                let frame = rt.screen_mode().frame();
                let sub = Some(debug_panel::sprites::SubScreen { place, frame });
                debug_panel::sprites::List { sub, ..list }
            }),
            _ => None,
        },
        grp: panel.grp_probe(&machine.rt.grp),
        bgm: (tab == Tab::Bgm).then(|| debug_panel::bgm::gather(&machine.rt.sound)),
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
    /// Type the condition of the breakpoint on a slot's line.
    Condition(usize, u32),
    RemoveBreakpoint(usize, u32),
    /// Forget what PERFEND has timed, as RUN does.
    ResetPerf,
    /// Outline a sprite on the screens, or none: one of the
    /// subprogram's, or the main interpreter's.
    Highlight(bool, Option<usize>),
    /// Zoom the GRP tab's page.
    Zoom(grp_view::Zoom),
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
        Some(Hit::Breakpoint(i)) if i < view.breakpoints.len() => {
            let (slot, line, _) = view.breakpoints[i];
            vec![
                (
                    "Condition...".to_string(),
                    true,
                    Choice::Condition(slot, line),
                ),
                (
                    "Remove breakpoint".to_string(),
                    true,
                    Choice::RemoveBreakpoint(slot, line),
                ),
            ]
        }
        Some(Hit::Perf | Hit::Tab(Tab::Perf)) => {
            vec![("Reset measurements".to_string(), true, Choice::ResetPerf)]
        }
        Some(Hit::Sprite(id)) => {
            let row = (view.sprites.iter())
                .flat_map(|l| &l.rows)
                .find(|r| r.id == id);
            let details = row.map(|r| debug_panel::sprites::details(r, panel.hex).join("\n"));
            let sub = panel.sub_tab();
            let chosen = panel.sprite_in(sub) == Some(id);
            vec![
                (
                    format!("{} Highlight", if chosen { "[x]" } else { "[ ]" }),
                    true,
                    Choice::Highlight(sub, (!chosen).then_some(id)),
                ),
                (
                    "Copy details".to_string(),
                    details.is_some(),
                    Choice::CopyValue(details.unwrap_or_default()),
                ),
                hex,
            ]
        }
        Some(hit @ (Hit::Sprites | Hit::Tab(Tab::Sprites | Tab::SubSprites))) => {
            // A tab's button is for its own sprites, shown or not.
            let sub = match hit {
                Hit::Tab(tab) => tab == Tab::SubSprites,
                _ => panel.sub_tab(),
            };
            vec![
                (
                    "Stop highlighting".to_string(),
                    panel.sprite_in(sub).is_some(),
                    Choice::Highlight(sub, None),
                ),
                hex,
            ]
        }
        Some(Hit::Grp | Hit::GrpPage(_) | Hit::Tab(Tab::Grp)) => {
            let mark = |z| if panel.grp.zoom == z { "[x]" } else { "[ ]" };
            let mut items: Vec<_> = (grp_view::ZOOMS.iter())
                .map(|&z| (format!("{} {}", mark(z), z.label()), true, Choice::Zoom(z)))
                .collect();
            if let Some((_, color)) = view.grp.pixel {
                let code = format!("&H{color:08X}");
                items.push((format!("Copy {code}"), true, Choice::CopyValue(code)));
            }
            items
        }
        Some(
            hit @ (Hit::BgmTrack(_)
            | Hit::BgmChannel(..)
            | Hit::Beep(_)
            | Hit::Bgm
            | Hit::Tab(Tab::Bgm)),
        ) => (view.bgm.unwrap_or_default().copies(hit).into_iter())
            .map(|(label, text)| {
                let some = text.is_some();
                (
                    label.to_string(),
                    some,
                    Choice::CopyValue(text.unwrap_or_default()),
                )
            })
            .collect(),
        _ => vec![("Add watch".to_string(), true, Choice::AddWatch), hex],
    }
}

/// A right click's menu on the editor: the name at the cursor, the
/// debugger's commands for its line, and the clipboard.
fn editor_menu(machine: &mut Machine) -> Vec<(String, bool, Choice)> {
    let paused = machine.paused_at().is_some() && !machine.is_running();
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
        Choice::Condition(slot, line) => return Some(condition_dialog(machine, slot, line, size)),
        Choice::RemoveBreakpoint(slot, line) => _ = machine.toggle_breakpoint(slot, line),
        Choice::ResetPerf => machine.rt.reset_perf_stats(),
        Choice::Highlight(sub, sprite) => panel.choose_sprite(sub, sprite),
        Choice::Zoom(zoom) => panel.grp.zoom_to(zoom, None),
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

/// Hides the menu bar when a program starts, however it was started, so
/// that a bar left shown by F8 does not start the program paused. F8
/// while the program runs still pauses it.
fn close_bar_on_start(menu: &mut MenuBar, was_running: &mut bool, running: bool) {
    if running && !*was_running {
        menu.shown = false;
    }
    *was_running = running;
}

/// What a trip round the loop ran of the program: see [`run_frames`].
struct Frames {
    /// When the frames began, and how long they took.
    began: Instant,
    took: Duration,
    /// Program frames that started: how far `MAINCNT` went.
    started: u32,
    /// Calls of `Machine::frame` that started none, because the program's
    /// trip under way kept its frame.
    kept: u32,
}

impl Frames {
    /// The trip as the notice for a program slower than real time takes
    /// it; `watched` is whether the program is not paused and the window
    /// is shown.
    fn trip(&self, machine: &Machine, watched: bool) -> slow_hint::Trip {
        let settings = &machine.rt.settings;
        let counts = watched
            && machine.keep_frames
            && settings.slow_hint
            && settings.speed == sb_runtime::speed::Speed::Unlimited
            && machine.program_dialog().is_none()
            && !machine.is_editing()
            && !machine.sub_running();
        slow_hint::Trip {
            started: self.started,
            kept: self.kept,
            counts,
            ..Default::default()
        }
    }
}

/// Runs a trip's frames of the program and plays their sound: one frame
/// unless `paused`, and more if the loop is behind.
///
/// Behind by a frame or more, as when drawing takes too long or the
/// compositor held a present back before vsync was dropped, the program
/// and its sound catch up, and only the last of these frames is drawn, so
/// that the game and its music keep time. Those frames run code briefly,
/// so that catching up does not fall further behind.
///
/// A program at its VSYNC runs a whole trip round its main loop if two
/// trips and a drawing fit in two frames, so that it keeps time, and
/// otherwise stays there: a part of a trip would only make the frame
/// drawn next show a half-made scene. That trip has the time a frame
/// gives, or a trip that a frame holds would be cut short here.
///
/// A program whose trips fit a frame catches up a trip round the loop at
/// a time instead, which leaves drawings out for it.
fn run_frames(
    machine: &mut Machine,
    guard: &mut FrameGuard,
    toasts: &mut Toasts,
    pacer: &mut pacing::Pacer,
    audio: &Audio,
    bench: &mut bench::Bench,
    paused: bool,
) -> Frames {
    let began = Instant::now();
    let (count_before, mut calls) = (machine.rt.main_count(), 0u32);
    if !paused {
        machine.time_limit = Some(frame_time(machine, pacer));
        guard.run(machine, toasts, Machine::frame);
        calls += 1;
        bench.ran_frame();
    }
    audio.play(&machine.take_samples());
    let mut skipped = 0;
    while !paused
        && let Some(more) = pacer.catch_up(Instant::now(), skipped, || pacing::Behind {
            fits: fits_frame(machine, pacer.draw_cost()),
            at_sync: machine.at_sync(),
            trip: machine.loop_time(),
        })
    {
        machine.time_limit = Some(more.time);
        machine.hold_at_sync = more.hold;
        guard.run(machine, toasts, Machine::frame);
        calls += 1;
        machine.hold_at_sync = false;
        bench.ran_frame();
        audio.play(&machine.take_samples());
        // A frame run to catch up goes undrawn too.
        pacer.caught_up();
        skipped += 1;
    }
    let took = began.elapsed();
    // A call that went on with a kept frame started none.
    let started = (machine.rt.main_count())
        .wrapping_sub(count_before)
        .min(calls);
    Frames {
        began,
        took,
        started,
        kept: calls - started,
    }
}

/// Whether the program waits at its VSYNC and can have a trip round its
/// main loop in every frame if drawings that cost `draw` are left out, as
/// many as may be: that many trips as long as its last one, and one
/// drawing, take no longer than their frames. The last trip's time is no
/// test by itself: one that was cut and went on after a wait reads longer
/// than the time a frame gives though it fits in one piece. Only at
/// unlimited speed, where a frame has a time limit, and for the main
/// program alone.
fn fits_frame(machine: &Machine, draw: Duration) -> bool {
    let trip = machine.loop_time();
    let frames = pacing::MOST_UNDRAWN + 1;
    machine.is_running()
        && machine.at_sync()
        && !trip.is_zero()
        && frames * trip + draw <= frames * FRAME
        && machine.rt.settings.speed == sb_runtime::speed::Speed::Unlimited
        && !machine.sub_running()
        && !machine.is_editing()
}

/// The time a frame gives a program that does not wait for the next one;
/// see [`pacing::Pacer::frame_time`].
fn frame_time(machine: &Machine, pacer: &pacing::Pacer) -> Duration {
    pacer.frame_time(fits_frame(machine, pacer.draw_cost()))
}

/// Gives the machine the setting for a program too slow for the computer:
/// it keeps its frames, and slows down, unless frames are to be skipped.
fn apply_frame_setting(machine: &mut Machine) {
    machine.keep_frames = !machine.rt.settings.skip_frames;
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
/// In a `#` project, which takes no files, Ctrl+S says so instead and
/// asks for no name, as SmileBASIC 4's editor does.
fn open_file_dialog(
    machine: &Machine,
    save: bool,
    size: (u32, u32),
) -> Result<FileDialog, Box<Dialog>> {
    let slot = machine.edited_slot();
    let storage = &machine.rt.storage;
    if save && machine.rt.current_project_read_only() {
        let lines = vec![
            "Failed to save the file.".to_string(),
            String::new(),
            format!("  Project:  {}", storage.project()),
            String::new(),
            "Files in system (#) projects are read-only.".to_string(),
            "File > Settings > Files can allow writing to them.".to_string(),
        ];
        return Err(Box::new(Dialog::notice("Save file", lines, size)));
    }
    Ok(if save {
        let name = &machine.rt.slot_names[slot];
        FileDialog::Save(SaveDialog::new(size, slot, name, storage.project()))
    } else {
        FileDialog::Load(FilePicker::new(size, slot, storage))
    })
}

/// F1: the help for the command at the editor's cursor, or at the cursor
/// of the line typed in direct mode, or its first page away from any
/// command.
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
    let lookup = match machine.rt.line_and_cursor() {
        Some((line, col)) if !machine.is_editing() => help.lookup(line, col),
        _ => {
            let (line, col) = machine.edit().cursor_line();
            help.lookup(line, col)
        }
    };
    let topic = match lookup {
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
    bench::made();
    gpu::texture_fits(canvas.raw(), w, h)?;
    Ok(creator.create_texture_streaming(PixelFormat::ARGB8888, w, h)?)
}

/// The frame the main window shows this trip, as [`upload_frame`] takes it.
struct Showing {
    /// Whether it is the editor's, and its revision.
    revision: (bool, u64),
    /// Its size and the filter it is scaled with.
    kind: TextureKind,
    /// Where it goes in the window.
    rect: FRect,
    /// The GPU path drew it, so its pixels are not in the framebuffer.
    on_gpu: bool,
    /// It is scaled as XSCREEN's mode 1 is: see [`SmartFrame`].
    smart: bool,
    /// The bench asks for that scaling on the CPU.
    cpu_smart: bool,
}

/// Brings the frame `fb` to where the main window draws it from: makes
/// the texture again when the frame's kind changed, sends the pixels when
/// the texture does not hold this revision, and gets the doubled frame of
/// XSCREEN's mode 1 ready. Gives the GPU path's own frame when that is the
/// one to show.
fn upload_frame<'a>(
    (texture, texture_kind): (&mut Texture<'a>, &mut TextureKind),
    textures: &mut Held<'a>,
    paths: &mut gpu::Paths,
    (canvas, creator): (&mut Canvas<Window>, &'a TextureCreator<WindowContext>),
    fb: &Framebuffer,
    showing: Showing,
) -> Result<Option<std::mem::ManuallyDrop<Texture<'static>>>, AnyError> {
    let Showing {
        revision,
        kind,
        rect,
        on_gpu,
        smart,
        cpu_smart,
    } = showing;
    if kind != *texture_kind {
        *texture_kind = kind;
        *texture = frame_texture(canvas, creator, kind)?;
        textures.main_holds = None;
    }
    let gpu_view = (paths.main.as_mut())
        .filter(|_| on_gpu)
        .and_then(|gpu| gpu.view(kind.1 == Filter::Bilinear));
    if Some(revision) != textures.main_holds && !on_gpu {
        bench::uploaded(fb.pixels().len() * 4);
        texture.update(None, as_bytes(fb.pixels()), (fb.width() * 4) as usize)?;
    }
    if smart {
        let changed = Some(revision) != textures.main_holds;
        textures.smart.prepare(
            canvas,
            creator,
            gpu_view.as_deref().unwrap_or(texture),
            fb,
            rect,
            changed,
            cpu_smart,
        )?;
        // Scaling on the CPU needs the frame on the CPU, from now on: this
        // one is drawn again next time round, once there is no GPU path.
        if on_gpu && textures.smart.on_cpu() {
            eprintln!("SB_GPU: the doubled frame was refused; staying on the CPU");
            paths.turn_off("CPU (doubled frame refused by the driver)");
        }
    } else {
        textures.smart.release();
    }
    textures.main_holds = Some(revision);
    Ok(gpu_view)
}

/// Draws the subprogram's screen where XSUBSCREEN puts it on the 1280x720
/// display, over the editor too. `lcd` maps that display to the window,
/// `held` is the texture its frame goes to when the GPU path did not draw
/// it, and `on_gpu` says whether that path may.
fn draw_sub_screen<'a>(
    machine: &mut Machine,
    paths: &mut gpu::Paths,
    held: &mut Option<(Texture<'a>, TextureKind)>,
    (canvas, creator): (&mut Canvas<Window>, &'a TextureCreator<WindowContext>),
    lcd: placement::Mapping,
    on_gpu: bool,
) -> Result<(), AnyError> {
    if paths.sub.is_none() && machine.sub_shows() {
        paths.sub = paths.main.as_ref().and_then(gpu::Gpu::for_sub);
    }
    let sub_backend = (paths.sub.as_mut())
        .filter(|_| on_gpu)
        .map(|g| g as &mut dyn SceneBackend);
    let Some(sub) = machine.sub_screen_with(sub_backend) else {
        return Ok(());
    };
    let fb = sub.frame;
    let on_window = |rect: [f32; 4]| lcd.on_window(rect);
    if let Some(bars) = sub.bars() {
        let c = |shift: u32| (sub.back >> shift & 0xFF) as u8;
        let before = canvas.blend_mode();
        // SAFETY: the renderer pointer is valid for the lifetime of `canvas`.
        unsafe { SDL_SetRenderDrawBlendMode(canvas.raw(), SDL_BLENDMODE_BLEND_PREMULTIPLIED) };
        canvas.set_draw_color(Color::RGBA(c(16), c(8), c(0), c(24)));
        for bar in bars {
            canvas.fill_rect(on_window(bar))?;
        }
        canvas.set_blend_mode(before);
    }
    // Whatever XSCREEN's mode, as a screen in mode 0 is.
    let kind = ((fb.width(), fb.height()), Filter::Bilinear);
    let sub_view = (paths.sub.as_mut())
        .filter(|_| sub.on_backend)
        .and_then(gpu::Gpu::premultiplied_view);
    if let Some(view) = &sub_view {
        canvas.copy(view, None, on_window(sub.quad))?;
    } else if held.as_ref().is_none_or(|(_, k)| *k != kind) {
        let made = frame_texture(canvas, creator, kind)?;
        // SAFETY: the texture is valid while `made` lives.
        unsafe { SDL_SetTextureBlendMode(made.raw(), SDL_BLENDMODE_BLEND_PREMULTIPLIED) };
        *held = Some((made, kind));
    }
    if let (None, Some((tex, _))) = (&sub_view, held) {
        bench::uploaded(fb.pixels().len() * 4);
        tex.update(None, as_bytes(fb.pixels()), (fb.width() * 4) as usize)?;
        canvas.copy(tex, None, on_window(sub.quad))?;
    }
    Ok(())
}

/// The textures frontend() keeps besides the main frame's, and what the
/// frames' textures hold.
#[derive(Default)]
struct Held<'a> {
    /// What the main window's texture holds, if anything: whether it is
    /// the editor's frame, and the frame's revision. For the editor's own
    /// window, the revision of the editor's frame its texture holds.
    main_holds: Option<(bool, u64)>,
    editor_holds: Option<u64>,
    smart: SmartFrame<'a>,
    /// The screens in the debugger's panel, drawn at the window's
    /// resolution, with their size and filter.
    panel_screens: Option<(Texture<'a>, TextureKind)>,
    /// The subprogram's screen, over the main one.
    sub: Option<(Texture<'a>, TextureKind)>,
    /// The editor's frame in its own window.
    editor: Option<(Texture<'a>, TextureKind)>,
    /// The page the panel's GRP tab shows, in the main window and the
    /// editor's.
    grp: PageTexture<'a>,
    editor_grp: PageTexture<'a>,
}

impl Held<'_> {
    /// The renderer's targets came back empty: the frames go to their
    /// textures again.
    fn forget_targets(&mut self) {
        (self.main_holds, self.editor_holds) = (None, None);
    }

    /// The renderer let go of every texture: these are made again when
    /// they are next wanted.
    fn forget_device(&mut self) {
        self.smart = SmartFrame::default();
        (self.panel_screens, self.sub, self.editor) = (None, None, None);
        (self.grp, self.editor_grp) = (PageTexture::default(), PageTexture::default());
    }
}

/// XSCREEN's composite mode 1 on its way to the window. Its mix between
/// texels is what bilinear sampling gives from a copy of the frame twice as
/// large made with nearest, which any renderer can do with no shader of
/// ours: a draw into a texture when the frame changes, and one out. Where
/// that texture cannot be made, as past an old GPU's largest size, the
/// frame is scaled on the CPU instead.
#[derive(Default)]
struct SmartFrame<'a> {
    /// The frame at twice its size, and that size.
    doubled: Option<(Texture<'a>, (u32, u32))>,
    /// The frame scaled to the window on the CPU, and that size.
    scaled: Option<(Texture<'a>, (u32, u32))>,
    scale: sb_runtime::smart_scale::SmartScale,
    pixels: Vec<u32>,
    /// Sizes the renderer has refused for the doubled frame.
    refused: Option<(u32, u32)>,
}

impl<'a> SmartFrame<'a> {
    /// Gets the frame `fb`, which `texture` holds, ready to show in `rect`.
    #[allow(clippy::too_many_arguments)]
    fn prepare(
        &mut self,
        canvas: &mut Canvas<Window>,
        creator: &'a TextureCreator<WindowContext>,
        texture: &Texture,
        fb: &Framebuffer,
        rect: FRect,
        changed: bool,
        cpu: bool,
    ) -> Result<(), AnyError> {
        let size = (fb.width() * 2, fb.height() * 2);
        if !cpu && self.refused != Some(size) {
            let mut fresh = false;
            if self.doubled.as_ref().is_none_or(|(_, s)| *s != size) {
                self.doubled = None;
                bench::made();
                let fits = gpu::texture_fits(canvas.raw(), size.0, size.1);
                let made = fits.map_err(|e| e.to_string()).and_then(|()| {
                    (creator.create_texture_target(PixelFormat::ARGB8888, size.0, size.1))
                        .map_err(|e| e.to_string())
                });
                match made {
                    Ok(mut made) => {
                        made.set_scale_mode(ScaleMode::Linear);
                        made.set_blend_mode(BlendMode::None);
                        self.doubled = Some((made, size));
                        fresh = true;
                    }
                    Err(_) => self.refused = Some(size),
                }
            }
            if let Some((target, _)) = &mut self.doubled {
                self.scaled = None;
                if changed || fresh {
                    let mut drawn = Ok(());
                    canvas.with_texture_canvas(target, |c| drawn = c.copy(texture, None, None))?;
                    drawn?;
                }
                return Ok(());
            }
        }
        // As SDL rounds the rectangle, so as to fill it with one texel a pixel.
        let shown = (rect.w.ceil().max(1.0) as u32, rect.h.ceil().max(1.0) as u32);
        let fresh = self.scaled.as_ref().is_none_or(|(_, s)| *s != shown);
        if fresh {
            bench::made();
            gpu::texture_fits(canvas.raw(), shown.0, shown.1)?;
            let mut made =
                creator.create_texture_streaming(PixelFormat::ARGB8888, shown.0, shown.1)?;
            made.set_scale_mode(ScaleMode::Nearest);
            self.scaled = Some((made, shown));
            self.pixels.resize((shown.0 * shown.1) as usize, 0);
        }
        if let Some((tex, _)) = &mut self.scaled
            && (changed || fresh)
        {
            let from = (fb.width(), fb.height());
            self.scale.scale(fb.pixels(), from, &mut self.pixels, shown);
            bench::uploaded(self.pixels.len() * 4);
            tex.update(None, as_bytes(&self.pixels), (shown.0 * 4) as usize)?;
        }
        Ok(())
    }

    /// The texture to show, once [`SmartFrame::prepare`] has run this frame.
    fn texture(&self) -> Option<&Texture<'a>> {
        let doubled = self.doubled.as_ref().map(|(t, _)| t);
        doubled.or(self.scaled.as_ref().map(|(t, _)| t))
    }

    /// Whether it scales on the CPU, from `fb` rather than the texture.
    fn on_cpu(&self) -> bool {
        self.scaled.is_some()
    }

    /// Frees what a frame shown another way has no use for.
    fn release(&mut self) {
        (self.doubled, self.scaled) = (None, None);
        self.pixels = Vec::new();
    }
}

/// Makes the editor's characters a size smaller, back to the largest
/// after the smallest, as F2 and Ctrl+B do. In the window `room` gives,
/// as [`editor_layout::next_size`] takes it, sizes that look the same as
/// the one chosen are skipped.
fn next_editor_size(machine: &mut Machine, (width, display_h): (u32, f32)) {
    let settings = &mut machine.rt.settings;
    let (font, size) = (settings.editor_font, settings.editor_size);
    settings.editor_size = editor_layout::next_size(width, display_h, font, size);
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

/// F1 to F5's number, for Ctrl+F1 to F5.
fn function_key(key: Keycode) -> Option<usize> {
    Some(match key {
        Keycode::F1 => 1,
        Keycode::F2 => 2,
        Keycode::F3 => 3,
        Keycode::F4 => 4,
        Keycode::F5 => 5,
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
    #[test]
    fn the_command_line_takes_simple_ui_and_a_program() {
        let args = |list: &[&str]| list.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(parse_args(&args(&[])), Some((None, false)));
        assert_eq!(parse_args(&args(&["A.TXT"])), Some((Some("A.TXT"), false)));
        assert_eq!(parse_args(&args(&["--simple-ui"])), Some((None, true)));
        for both in [["--simple-ui", "A.TXT"], ["A.TXT", "--simple-ui"]] {
            assert_eq!(parse_args(&args(&both)), Some((Some("A.TXT"), true)));
        }
        for bad in [&["-x"][..], &["A", "B"], &["--simple-ui", "--simple-ui"]] {
            assert_eq!(parse_args(&args(bad)), None, "{bad:?}");
        }
    }

    #[test]
    fn simple_ui_leaves_the_desktops_keys_alone() {
        assert!(!desktop_key(Keycode::F5, Mod::NOMOD), "the + button");
        assert!(desktop_key(Keycode::F5, Mod::LSHIFTMOD));
        assert!(desktop_key(Keycode::F5, Mod::RCTRLMOD));
        for key in [
            Keycode::F1,
            Keycode::F4,
            Keycode::F8,
            Keycode::F9,
            Keycode::F12,
        ] {
            assert!(desktop_key(key, Mod::NOMOD), "{key:?}");
        }
        for key in [Keycode::Escape, Keycode::Pause, Keycode::Return, Keycode::A] {
            assert!(!desktop_key(key, Mod::NOMOD), "{key:?}");
        }
    }

    #[test]
    fn the_pause_menu_takes_the_input_before_any_other_screen() {
        assert_eq!(input_screen(false, false, None, None), 0);
        assert_eq!(input_screen(false, true, None, Some(0)), 1);
        assert_eq!(input_screen(true, true, None, Some(0)), 20);
        assert_eq!(input_screen(false, false, None, Some(1)), 22);
        assert_eq!(input_screen(false, false, Some(3), None), 5);
        // Each tells the pad's repeat a new screen started.
        assert_ne!(input_screen(false, false, None, Some(0)), 2);
    }

    use super::*;

    fn console_rows(m: &Machine) -> Vec<String> {
        let console = &m.rt.text[sb_runtime::text::CONSOLE];
        let (w, h) = console.size();
        (0..h)
            .map(|y| {
                let codes: Vec<u16> = (0..w)
                    .map(|x| console.cell(x, y).code)
                    .take_while(|&c| c != 0)
                    .collect();
                String::from_utf16_lossy(&codes)
            })
            .collect()
    }

    fn running_machine() -> Machine {
        let rt = Runtime::for_tests(Font::blank());
        let mut m = Machine::new(rt);
        m.boot();
        m.set_slot(0, "WHILE 1:VSYNC:WEND\n".to_string());
        m.run_slot(0);
        m.frame();
        assert!(m.is_running());
        m
    }

    /// A program too slow for the computer slows down unless the setting
    /// says to skip frames.
    #[test]
    fn frames_are_kept_unless_skipped() {
        let mut m = running_machine();
        assert!(!m.keep_frames, "the machine's own default");
        apply_frame_setting(&mut m);
        assert!(m.keep_frames);
        m.rt.settings.skip_frames = true;
        apply_frame_setting(&mut m);
        assert!(!m.keep_frames);
    }

    /// Ctrl+S in a `#` project tells that it takes no files and asks for
    /// no name; Ctrl+L there, and Ctrl+S with the setting on, open as ever.
    #[test]
    fn ctrl_s_in_a_system_project_is_refused() {
        let dir = std::env::temp_dir().join(format!("sb-main-system-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut rt = Runtime::for_tests(Font::blank());
        rt.storage = Storage::new(dir.clone());
        rt.storage.create_project("#SYS").unwrap();
        rt.storage.set_project("#SYS").unwrap();
        let mut m = Machine::new(rt);
        let size = (1280, 720);
        assert!(open_file_dialog(&m, true, size).is_err());
        assert!(matches!(
            open_file_dialog(&m, false, size),
            Ok(FileDialog::Load(_))
        ));
        m.rt.settings.write_system_projects = true;
        assert!(matches!(
            open_file_dialog(&m, true, size),
            Ok(FileDialog::Save(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A machine with a project GAME whose programs are `programs`, as
    /// (name, source), with the desktop's `tidy_on_end` off.
    fn game_machine(tag: &str, programs: &[(&str, &str)]) -> (Machine, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("sb-main-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut rt = Runtime::for_tests(Font::blank());
        rt.settings.tidy_on_end = false;
        rt.storage = Storage::new(dir.clone());
        rt.storage.create_project("GAME").unwrap();
        for (name, source) in programs {
            std::fs::write(dir.join("GAME").join(format!("T{name}")), source).unwrap();
        }
        let mut m = Machine::new(rt);
        m.boot();
        (m, dir)
    }

    /// Whether the screens and sound are as ACLS and SNDSTOP leave them.
    fn tidy(m: &Machine) -> bool {
        m.rt.back_color == 0xFF00_0000 && m.rt.sound.inspect_tracks().is_empty()
    }

    const UNTIDY: &str = "BACKCOLOR #C_RED\nBGMPLAY 1\n";

    #[test]
    fn simple_ui_tidies_whenever_it_goes_back_to_the_list() {
        let ends = format!("{UNTIDY}END\n");
        let spins = format!("{UNTIDY}WHILE 1:VSYNC:WEND\n");
        let fails = format!("{UNTIDY}DIM A[2]\nA[5]=1\n");
        let programs = [
            ("MAIN.PRG", ends.as_str()),
            ("SPIN.PRG", spins.as_str()),
            ("FAIL.PRG", fails.as_str()),
        ];
        let (mut m, dir) = game_machine("tidy", &programs);
        let mut toasts = Toasts::default();
        let mut run = |m: &mut Machine, file: &str| {
            assert!(run_from_list(m, "GAME", file, &mut toasts));
            for _ in 0..3 {
                m.frame();
            }
        };
        // END.
        run(&mut m, "MAIN.PRG");
        assert!(!m.is_running() && !tidy(&m));
        assert!(simple_ui_ended(&mut m));
        assert!(tidy(&m));
        // The + button.
        run(&mut m, "SPIN.PRG");
        assert!(m.is_running() && !simple_ui_ended(&mut m) && !tidy(&m));
        m.sysreq();
        m.frame();
        assert!(simple_ui_ended(&mut m));
        assert!(tidy(&m));
        // Return to Game list, and Restart, which goes the same way.
        run(&mut m, "SPIN.PRG");
        simple_ui_leave(&mut m);
        assert!(!m.is_running() && tidy(&m));
        assert!(!simple_ui_ended(&mut m), "nothing more to do");
        // An error stays readable until A, and its press tidies.
        run(&mut m, "FAIL.PRG");
        assert!(m.home_waiting() && !simple_ui_ended(&mut m));
        assert!(!tidy(&m), "the message stays as it was");
        for _ in 0..30 {
            m.frame();
        }
        assert!(m.home_press());
        m.tidy();
        assert!(tidy(&m));
        // An error left for Return to Game list.
        run(&mut m, "FAIL.PRG");
        simple_ui_leave(&mut m);
        assert!(!m.home_waiting() && tidy(&m));
        // On the desktop the setting decides, as before.
        run(&mut m, "MAIN.PRG");
        assert!(m.home_done() && !tidy(&m));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_program_never_starts_paused() {
        let mut m = running_machine();
        let (mut menu, mut was_running) = (MenuBar::default(), true);
        // F8 while it runs pauses it, frame after frame.
        menu.shown = true;
        for _ in 0..2 {
            close_bar_on_start(&mut menu, &mut was_running, m.is_running());
            assert!(menu.shown);
        }
        // The + button stops it, and the bar stays in DIRECT mode.
        m.stop();
        m.frame();
        assert!(!m.is_running());
        close_bar_on_start(&mut menu, &mut was_running, m.is_running());
        assert!(menu.shown);
        // The next run hides it, wherever it was started from.
        m.run_slot(0);
        m.frame();
        close_bar_on_start(&mut menu, &mut was_running, m.is_running());
        assert!(!menu.shown);
    }

    #[test]
    fn a_panicking_frame_stops_the_program_and_keeps_its_text() {
        let mut m = running_machine();
        let (mut guard, mut toasts) = (FrameGuard::default(), Toasts::default());
        guard.run(&mut m, &mut toasts, |_| panic!("a bug in frame {}", 12));
        assert!(!m.is_running());
        assert!(!guard.stopped);
        assert!(toasts.visible());
        let console = console_rows(&m);
        assert!(
            console
                .iter()
                .any(|r| r.starts_with("Internal Error (a bug in frame 12)")),
            "{console:?}"
        );
        assert_eq!(m.rt.slots[0], "WHILE 1:VSYNC:WEND\n");
        // The machine works on, and a frame without a panic clears the count.
        guard.run(&mut m, &mut toasts, Machine::frame);
        assert_eq!(guard.panics_in_a_row, 0);
        m.run_slot(0);
        guard.run(&mut m, &mut toasts, Machine::frame);
        assert!(m.is_running());
    }

    #[test]
    fn frames_stop_when_panics_keep_coming() {
        let mut m = running_machine();
        let (mut guard, mut toasts) = (FrameGuard::default(), Toasts::default());
        for _ in 0..MOST_PANICS {
            assert!(!guard.stopped);
            guard.run(&mut m, &mut toasts, |_| panic!("again"));
        }
        assert!(guard.stopped);
        let mut ran = false;
        guard.run(&mut m, &mut toasts, |_| ran = true);
        assert!(!ran);
    }

    #[test]
    fn no_profile_turns_panics_into_aborts() {
        let manifest = include_str!("../../../Cargo.toml");
        assert!(
            !manifest.contains("panic ="),
            "catch_unwind needs unwinding"
        );
        assert!(!manifest.contains("panic="), "catch_unwind needs unwinding");
    }

    #[test]
    fn file_urls_encode_the_path() {
        assert_eq!(
            file_url(Path::new("/home/a b/데이터")),
            "file:///home/a%20b/%EB%8D%B0%EC%9D%B4%ED%84%B0"
        );
        assert_eq!(file_url(Path::new("C:\\Users\\x")), "file:///C:/Users/x");
    }

    #[test]
    fn windows_go_back_only_onto_a_display() {
        let displays = [Rect::new(0, 0, 1920, 1080), Rect::new(1920, 0, 1280, 1024)];
        assert!(on_a_display([100, 50, 1280, 720], &displays));
        assert!(on_a_display([1920, 0, 1280, 1024], &displays));
        // Off the edge, across two displays, or on one that is gone.
        assert!(!on_a_display([-10, 50, 640, 480], &displays));
        assert!(!on_a_display([1800, 50, 640, 480], &displays));
        assert!(!on_a_display([3300, 50, 640, 480], &displays));
        assert!(!on_a_display([100, 50, 640, 480], &[]));
    }
}

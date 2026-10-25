#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod menu;

use std::path::{Path, PathBuf};

use menu::{Action, Click, MenuBar};
use sb_profiles::Profile;
use sb_runtime::font::Font;
use sb_runtime::machine::Machine;
use sb_runtime::runtime::keys;
use sb_runtime::storage::Storage;
use sb_runtime::{Framebuffer, Runtime, resources, sbfile};
use sdl3::event::Event;
use sdl3::event::WindowEvent;
use sdl3::keyboard::{Keycode, Mod};
use sdl3::messagebox::{MessageBoxFlag, show_simple_message_box};
use sdl3::mouse::MouseButton;
use sdl3::pixels::{Color, PixelFormat};
use sdl3::rect::Rect;
use sdl3::render::{BlendMode, FRect, ScaleMode};

const USAGE: &str = "usage:
  smilebasic                    start in direct mode
  smilebasic PROGRAM            load a SmileBASIC 4 program (TXT file or plain
                                text) into PRG0 and run it
  smilebasic import ROMFS...    copy the font from your own extracted SmileBASIC 4
                                RomFS directories (list the update's last);
                                without it, a bundled font is used";

const TITLE: &str = "SmileBASIC emulator";

type AnyError = Box<dyn std::error::Error>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("import") if args.len() > 1 => import(&args[1..]),
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
    Ok(sdl3::filesystem::get_pref_path("", "smilebasic-emulator")?)
}

fn resource_dir() -> Result<PathBuf, AnyError> {
    Ok(data_dir()?.join("resources"))
}

fn import(romfs: &[String]) -> Result<(), AnyError> {
    let dest = resource_dir()?;
    let romfs: Vec<PathBuf> = romfs.iter().map(PathBuf::from).collect();
    for source in resources::import(&romfs, &dest)? {
        println!("imported {}", source.display());
    }
    println!("into {}", dest.display());
    Ok(())
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
        eprintln!("using the fallback font: {e} (see `smilebasic import`)");
        Font::fallback()
    });
    let screen = Profile::default().screen();
    let mut rt = Runtime::new(screen.width, screen.height, font);
    rt.storage = Storage::new(data_dir()?.join("projects"));
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
    let mut canvas = window.into_canvas();
    // SmileBASIC runs at 60 fps; until the VM has its own frame timer, pace with vsync.
    // SAFETY: the renderer pointer is valid for the lifetime of `canvas`.
    if !unsafe { sdl3::sys::render::SDL_SetRenderVSync(canvas.raw(), 1) } {
        eprintln!("vsync unavailable: {}", sdl3::get_error());
    }
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator.create_texture_streaming(
        PixelFormat::ARGB8888,
        screen.width,
        screen.height,
    )?;
    texture.set_scale_mode(ScaleMode::Nearest);
    // The menu bar and other emulator UI, drawn at display resolution.
    let mut overlay = Framebuffer::new(display_w, display_h);
    let mut overlay_texture =
        texture_creator.create_texture_streaming(PixelFormat::ARGB8888, display_w, display_h)?;
    overlay_texture.set_scale_mode(ScaleMode::Nearest);
    overlay_texture.set_blend_mode(BlendMode::Blend);
    let mut menu = MenuBar::default();

    let mut events = sdl.event_pump()?;
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
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. } => break 'running,
                Event::MouseMotion { x, y, .. } => menu.mouse_move(Some(to_display(x, y))),
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
                    Click::Chosen(Action::Quit) => break 'running,
                    Click::Chosen(Action::Stop) => machine.stop(),
                    Click::Chosen(toggle) => toggle.toggle(&mut machine.rt.settings),
                    Click::Consumed | Click::Ignored => {}
                },
                Event::KeyDown {
                    keycode: Some(Keycode::Pause),
                    ..
                } => machine.stop(),
                Event::KeyDown {
                    keycode: Some(Keycode::C),
                    keymod,
                    ..
                } if machine.rt.settings.ctrl_c_stops
                    && keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD) =>
                {
                    machine.stop()
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

        machine.frame();

        let fb = machine.render();
        texture.update(None, as_bytes(fb.pixels()), (fb.width() * 4) as usize)?;
        canvas.set_draw_color(Color::BLACK);
        canvas.clear();
        canvas.copy(
            &texture,
            None,
            screen_rect(fb.width(), fb.height(), out_w, out_h),
        )?;
        if menu.visible() {
            overlay.clear(0);
            let running = machine.is_running();
            menu.draw(
                &mut overlay,
                machine.rt.font(),
                &machine.rt.settings,
                running,
            );
            overlay_texture.update(None, as_bytes(overlay.pixels()), (display_w * 4) as usize)?;
            canvas.copy(&overlay_texture, None, display)?;
        }
        canvas.present();
    }
    Ok(())
}

/// Where the display (emulator UI coordinates) goes in a window of
/// `out_w` x `out_h` pixels: scaled to fit and centered.
fn display_rect(width: u32, height: u32, out_w: u32, out_h: u32) -> FRect {
    let scale = (out_w as f32 / width as f32).min(out_h as f32 / height as f32);
    let (w, h) = (width as f32 * scale, height as f32 * scale);
    FRect::new((out_w as f32 - w) / 2.0, (out_h as f32 - h) / 2.0, w, h)
}

/// Where the screen goes in a window of `out_w` x `out_h` pixels: scaled
/// by the largest whole factor that fits, and centered.
fn screen_rect(width: u32, height: u32, out_w: u32, out_h: u32) -> Rect {
    let scale = (out_w / width).min(out_h / height).max(1);
    let (w, h) = (width * scale, height * scale);
    Rect::new(
        (out_w as i32 - w as i32) / 2,
        (out_h as i32 - h as i32) / 2,
        w,
        h,
    )
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

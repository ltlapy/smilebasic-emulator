#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use sb_core::vm::{Status, Vm};
use sb_profiles::Profile;
use sb_runtime::{Runtime, resources, sbfile};
use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use sdl3::messagebox::{MessageBoxFlag, show_simple_message_box};
use sdl3::pixels::PixelFormat;
use sdl3::render::ScaleMode;
use sdl3::sys::render::SDL_RendererLogicalPresentation;

const USAGE: &str = "usage:
  smilebasic PROGRAM            run a SmileBASIC 4 program (TXT file or plain text)
  smilebasic import ROMFS...    copy the font from your own extracted SmileBASIC 4
                                RomFS directories (list the update's last)";

/// UNVERIFIED: instructions per frame for programs that do not VSYNC;
/// SmileBASIC 4's real speed has not been measured.
const FRAME_BUDGET: u32 = 200_000;

type AnyError = Box<dyn std::error::Error>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("import") if args.len() > 1 => import(&args[1..]),
        Some(program) if !program.starts_with('-') && args.len() == 1 => run(Path::new(program)),
        _ => Err(USAGE.into()),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        // Release builds on Windows have no console to print to.
        let _ = show_simple_message_box(MessageBoxFlag::ERROR, "SmileBASIC", &e.to_string(), None);
        std::process::exit(1);
    }
}

fn resource_dir() -> Result<PathBuf, AnyError> {
    Ok(sdl3::filesystem::get_pref_path("", "smilebasic-emulator")?.join("resources"))
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

fn run(program: &Path) -> Result<(), AnyError> {
    let source = load_source(program)?;
    let font = resources::load_font(&resource_dir()?).map_err(|e| {
        format!("cannot load the font ({e}); run `smilebasic import` first\n\n{USAGE}")
    })?;
    let screen = Profile::default().screen();
    let mut rt = Runtime::new(screen.width, screen.height, font);
    let mut vm = match sb_core::compile(&source, &rt) {
        Ok(compiled) => Some(Vm::new(compiled)),
        Err(e) => {
            report(&mut rt, &e);
            None
        }
    };

    let sdl = sdl3::init()?;
    let video = sdl.video()?;
    let window = video
        .window(
            "SmileBASIC",
            screen.width * screen.default_scale,
            screen.height * screen.default_scale,
        )
        .position_centered()
        .resizable()
        .high_pixel_density()
        .build()?;

    let mut canvas = window.into_canvas();
    // SmileBASIC runs at 60 fps; until the VM has its own frame timer, pace with vsync.
    // SAFETY: the renderer pointer is valid for the lifetime of `canvas`.
    if !unsafe { sdl3::sys::render::SDL_SetRenderVSync(canvas.raw(), 1) } {
        eprintln!("vsync unavailable: {}", sdl3::get_error());
    }
    canvas.set_logical_size(
        screen.width,
        screen.height,
        SDL_RendererLogicalPresentation::INTEGER_SCALE,
    )?;
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator.create_texture_streaming(
        PixelFormat::ARGB8888,
        screen.width,
        screen.height,
    )?;
    texture.set_scale_mode(ScaleMode::Nearest);

    let mut events = sdl.event_pump()?;
    let mut wait: u32 = 0;
    'running: loop {
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                _ => {}
            }
        }

        wait = wait.saturating_sub(1);
        if wait == 0
            && let Some(machine) = &mut vm
        {
            match machine.run(&mut rt, FRAME_BUDGET) {
                Ok(Status::Running) => {}
                Ok(Status::Wait { frames, .. }) => wait = frames,
                Ok(Status::Ended) => vm = None,
                Err(e) => {
                    report(&mut rt, &e);
                    vm = None;
                }
            }
        }

        let fb = rt.render();
        texture.update(None, as_bytes(fb.pixels()), (fb.width() * 4) as usize)?;
        canvas.clear();
        canvas.copy(&texture, None, None)?;
        canvas.present();
    }
    Ok(())
}

/// Shows an error on the console, as SmileBASIC does, and on stderr.
fn report(rt: &mut Runtime, error: &sb_core::error::Error) {
    eprintln!("{error}");
    let console = &mut rt.text[sb_runtime::text::CONSOLE];
    if console.cursor().0 != 0 {
        console.newline();
    }
    let text: Vec<u16> = error.to_string().encode_utf16().collect();
    console.print(&text);
    console.newline();
}

fn as_bytes(pixels: &[u32]) -> &[u8] {
    // SAFETY: u32 has no padding and u8 has alignment 1.
    unsafe { std::slice::from_raw_parts(pixels.as_ptr().cast(), std::mem::size_of_val(pixels)) }
}

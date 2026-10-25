#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use sb_profiles::Profile;
use sb_runtime::Framebuffer;
use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use sdl3::pixels::PixelFormat;
use sdl3::render::ScaleMode;
use sdl3::sys::render::SDL_RendererLogicalPresentation;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let screen = Profile::default().screen();
    let mut fb = Framebuffer::new(screen.width, screen.height);

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
    let mut frame: u32 = 0;
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

        draw_test_pattern(&mut fb, frame);
        let bytes: &[u8] = bytemuck_u32(fb.pixels());
        texture.update(None, bytes, (fb.width() * 4) as usize)?;

        canvas.clear();
        canvas.copy(&texture, None, None)?;
        canvas.present();
        frame = frame.wrapping_add(1);
    }
    Ok(())
}

/// Placeholder until the VM drives the frame.
fn draw_test_pattern(fb: &mut Framebuffer, frame: u32) {
    let (w, h) = (fb.width(), fb.height());
    for y in 0..h {
        for x in 0..w {
            let r = (x * 255 / w) as u8;
            let g = (y * 255 / h) as u8;
            let b = (frame % 256) as u8;
            fb.set(x, y, u32::from_be_bytes([0xFF, r, g, b]));
        }
    }
}

fn bytemuck_u32(pixels: &[u32]) -> &[u8] {
    // SAFETY: u32 has no padding and u8 has alignment 1.
    unsafe { std::slice::from_raw_parts(pixels.as_ptr().cast(), std::mem::size_of_val(pixels)) }
}

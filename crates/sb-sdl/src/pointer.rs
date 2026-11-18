//! The mouse cursor the emulator draws itself where the system shows
//! none, as on a handheld whose KMSDRM display has no cursor: an arrow
//! with its tip at the pointer, over everything else, that fades out when
//! the pointer stays still, as SmileBASIC 4's does.

use std::time::{Duration, Instant};

use sdl3::pixels::PixelFormat;
use sdl3::render::{BlendMode, Canvas, FRect, ScaleMode, Texture, TextureCreator};
use sdl3::video::{Window, WindowContext};

type AnyError = Box<dyn std::error::Error>;

/// The variable that picks the cursor: `drawn` for the emulator's own,
/// `system` for the system's, whatever the video driver.
pub const CURSOR_VAR: &str = "SB_CURSOR";

/// Whether the emulator draws the cursor itself, by `var` (the value of
/// [`CURSOR_VAR`]), or else by SDL's video `driver`: KMSDRM, which
/// handhelds without a desktop use, shows no cursor without a mouse
/// plugged in, and the emulator's cursor is the same with one or without.
pub fn draws_own(driver: &str, var: Option<&str>) -> bool {
    match var {
        Some("drawn") => true,
        Some("system") => false,
        _ => driver == "kmsdrm",
    }
}

/// The arrow's side, and its size on SmileBASIC 4's 1280x720 display.
const SIDE: u32 = 32;
/// The height of SmileBASIC 4's display, which the cursor is a
/// [`SIDE`]th of.
const SB4_DISPLAY_H: f32 = 720.0;

/// Our own arrow, not SmileBASIC 4's: `#` black, `o` white, its tip at the
/// top left corner. The tail starts under the head, by the notch in its
/// base, as a desktop's arrow does.
const ARROW: [&str; SIDE as usize] = [
    "#",
    "##",
    "###",
    "####",
    "##o##",
    "##oo##",
    "##ooo##",
    "##oooo##",
    "##ooooo##",
    "##oooooo##",
    "##ooooooo##",
    "##oooooooo##",
    "##ooooooooo##",
    "##oooooooooo##",
    "##ooooooooooo##",
    "##oooooooooooo##",
    "##ooooooooooooo##",
    "##oooooooooooooo##",
    "##ooooooooo########",
    "##ooooooooo#########",
    "##ooooo##ooo##",
    "##oooo###ooo##",
    "##ooo##.##ooo##",
    "##oo##..##ooo##",
    "##o##....##ooo##",
    "####.....##ooo##",
    "###.......##ooo##",
    "##........##ooo##",
    "#..........##ooo##",
    "...........##ooo##",
    "............#######",
    ".............#####",
];

/// [`ARROW`]'s pixels, ARGB, row by row.
fn arrow() -> Vec<u32> {
    let mut pixels = vec![0; (SIDE * SIDE) as usize];
    for (y, row) in ARROW.iter().enumerate() {
        for (x, c) in row.bytes().enumerate() {
            pixels[y * SIDE as usize + x] = match c {
                b'#' => 0xFF00_0000,
                b'o' => 0xFFFF_FFFF,
                _ => 0,
            };
        }
    }
    pixels
}

/// How long the pointer stays still before the cursor fades, and how long
/// it takes to go, as on SmileBASIC 4.
const IDLE: Duration = Duration::from_millis(5000);
const FADE: Duration = Duration::from_millis(700);

/// The cursor's opacity, 0 to 255, when the pointer has been still for
/// `still`.
fn opacity(still: Duration) -> u8 {
    let Some(fading) = still.checked_sub(IDLE) else {
        return 255;
    };
    let left = FADE.saturating_sub(fading).as_millis() as u32;
    (left * 255 / FADE.as_millis() as u32) as u8
}

/// When the cursor last moved or came, and where it was.
#[derive(Debug, Default)]
struct Fade {
    at: (i32, i32),
    /// `None` while the cursor is hidden, or gone after fading out.
    since: Option<Instant>,
    /// It faded out where it is.
    gone: bool,
}

impl Fade {
    /// The cursor's opacity now, `shown` and at `at`. `now` is called only
    /// while it shows or fades.
    fn opacity(&mut self, shown: bool, at: (i32, i32), now: impl FnOnce() -> Instant) -> u8 {
        if !shown {
            *self = Fade::default();
            return 0;
        }
        let moved = at != self.at;
        self.at = at;
        if self.gone && !moved {
            return 0;
        }
        let now = now();
        let since = match self.since {
            Some(since) if !moved => since,
            _ => *self.since.insert(now),
        };
        let alpha = opacity(now.saturating_duration_since(since));
        self.gone = alpha == 0;
        alpha
    }
}

/// The emulator's own cursor, where [`draws_own`] says so.
pub struct DrawnCursor<'a> {
    on: bool,
    texture: Option<Texture<'a>>,
    fade: Fade,
    /// The texture's opacity and sampling now, to change them only when
    /// they change.
    alpha: u8,
    scale_mode: ScaleMode,
}

impl<'a> DrawnCursor<'a> {
    pub fn new(on: bool) -> Self {
        DrawnCursor {
            on,
            texture: None,
            fade: Fade::default(),
            alpha: 255,
            scale_mode: ScaleMode::Nearest,
        }
    }

    /// Whether the emulator draws the cursor, rather than the system.
    pub fn on(&self) -> bool {
        self.on
    }

    /// Lets go of the texture, as when the renderer lost it.
    pub fn forget(&mut self) {
        self.texture = None;
    }

    /// Draws the cursor if `shown`, with its tip at the pixel `at` of
    /// SmileBASIC 4's display, which is `display` on the canvas and
    /// `display_w` pixels wide (see [`crate::placement::Mapping::lcd`]).
    /// It is as large there as SmileBASIC 4's is on its own.
    pub fn draw(
        &mut self,
        canvas: &mut Canvas<Window>,
        creator: &'a TextureCreator<WindowContext>,
        shown: bool,
        at: (i32, i32),
        display: FRect,
        display_w: u32,
    ) -> Result<(), AnyError> {
        let alpha = self.fade.opacity(self.on && shown, at, Instant::now);
        if alpha == 0 {
            return Ok(());
        }
        let texture = match &mut self.texture {
            Some(texture) => texture,
            None => {
                crate::bench::made();
                let mut made = creator.create_texture_static(PixelFormat::ARGB8888, SIDE, SIDE)?;
                made.set_blend_mode(BlendMode::Blend);
                let pixels = arrow();
                let bytes = crate::as_bytes(&pixels);
                crate::bench::uploaded(bytes.len());
                made.update(None, bytes, (SIDE * 4) as usize)?;
                (self.alpha, self.scale_mode) = (255, ScaleMode::Nearest);
                made.set_scale_mode(self.scale_mode);
                self.texture.insert(made)
            }
        };
        let scale = display.w / display_w.max(1) as f32;
        let side = (display.h * SIDE as f32 / SB4_DISPLAY_H).round().max(1.0);
        // Sharp at its own size, smooth at any other.
        let mode = match side as u32 == SIDE {
            true => ScaleMode::Nearest,
            false => ScaleMode::Linear,
        };
        if mode != self.scale_mode {
            self.scale_mode = mode;
            texture.set_scale_mode(mode);
        }
        if alpha != self.alpha {
            self.alpha = alpha;
            texture.set_alpha_mod(alpha);
        }
        let x = (display.x + at.0 as f32 * scale).floor();
        let y = (display.y + at.1 as f32 * scale).floor();
        canvas.copy(texture, None, FRect::new(x, y, side, side))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kmsdrm_gets_the_drawn_cursor_unless_the_variable_says_otherwise() {
        assert!(draws_own("kmsdrm", None));
        assert!(!draws_own("x11", None));
        assert!(!draws_own("wayland", Some("")));
        assert!(draws_own("offscreen", Some("drawn")));
        assert!(!draws_own("kmsdrm", Some("system")));
    }

    #[test]
    fn the_arrow_has_its_tip_at_the_corner_and_fits() {
        let pixels = arrow();
        assert_eq!(pixels[0], 0xFF00_0000);
        assert!(ARROW.iter().all(|row| row.len() <= SIDE as usize));
        // A white inside, outlined in black all round.
        let at = |x: usize, y: usize| pixels[y * SIDE as usize + x];
        assert_eq!(at(3, 10), 0xFFFF_FFFF);
        for (y, row) in ARROW.iter().enumerate() {
            for (x, c) in row.bytes().enumerate() {
                if c != b'o' {
                    continue;
                }
                let around = [(-1, 0), (1, 0), (0, -1), (0, 1)]
                    .map(|(dx, dy)| at((x as i32 + dx) as usize, (y as i32 + dy) as usize) >> 24);
                assert!(
                    around.iter().all(|&a| a == 0xFF),
                    "({x}, {y}) is on the edge"
                );
            }
        }
    }

    #[test]
    fn the_cursor_fades_after_five_seconds_still() {
        assert_eq!(opacity(Duration::ZERO), 255);
        assert_eq!(opacity(Duration::from_millis(5000)), 255);
        assert_eq!(opacity(Duration::from_millis(5350)), 127);
        assert_eq!(opacity(Duration::from_millis(5700)), 0);
        assert_eq!(opacity(Duration::from_secs(60)), 0);
    }

    #[test]
    fn moving_or_showing_the_cursor_brings_it_back() {
        let start = Instant::now();
        let at = |ms| move || start + Duration::from_millis(ms);
        let mut fade = Fade::default();
        assert_eq!(fade.opacity(true, (5, 5), at(0)), 255);
        assert_eq!(fade.opacity(true, (5, 5), at(5350)), 127);
        assert_eq!(fade.opacity(true, (5, 5), at(6000)), 0);
        // Gone: the clock is not even read while the pointer stays.
        let no_clock = || -> Instant { panic!("read the clock") };
        assert_eq!(fade.opacity(true, (5, 5), no_clock), 0);
        assert_eq!(fade.opacity(true, (6, 5), at(7000)), 255);
        assert_eq!(fade.opacity(true, (6, 5), at(11000)), 255);
        assert_eq!(fade.opacity(false, (6, 5), no_clock), 0);
        assert_eq!(fade.opacity(true, (6, 5), at(20000)), 255);
    }
}

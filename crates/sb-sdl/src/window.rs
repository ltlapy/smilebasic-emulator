//! The main window's size: what it starts at on a display.

/// The size the main window starts at, in the system's window
/// coordinates, when it asks for `wanted` on a display whose usable area
/// is `usable`: no larger on either side. A window larger than the
/// display shows only in part on most systems; on Linux's KMS/DRM, the
/// console without a desktop, SDL draws into a buffer the display's size
/// while the window keeps the size it asked for, so the renderer would
/// lay its frames out for the wrong size.
pub fn start_size((w, h): (u32, u32), usable: Option<(u32, u32)>) -> (u32, u32) {
    match usable {
        Some((uw, uh)) if uw > 0 && uh > 0 => (w.min(uw), h.min(uh)),
        _ => (w, h),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_starts_no_larger_than_the_display() {
        assert_eq!(start_size((1280, 720), Some((1920, 1040))), (1280, 720));
        // A handheld's 640x480 display: the window covers it, as the
        // buffer SDL draws into does.
        assert_eq!(start_size((1280, 720), Some((640, 480))), (640, 480));
        // A 1024x768 display with a panel at the bottom.
        assert_eq!(start_size((1280, 720), Some((1024, 738))), (1024, 720));
        // Unknown, it asks for what it wanted.
        assert_eq!(start_size((1280, 720), None), (1280, 720));
        assert_eq!(start_size((1280, 720), Some((0, 0))), (1280, 720));
    }
}

//! The emulator's windows: what size the main one starts at, and which
//! one comes to the front for input that another window's dialog keeps.

use sdl3::event::Event;

/// One of the emulator's windows: the main one, or the editor's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Main,
    Editor,
}

/// The window that shows the emulator's window taking all input, if one
/// is `modal` and the editor's own window is shown (`apart`): the window
/// that had the keys when it opened, `ui_editor` for the editor's. With
/// one window there is nothing to bring forward.
pub fn modal_window(apart: bool, modal: bool, ui_editor: bool) -> Option<Which> {
    match (apart && modal, ui_editor) {
        (false, _) => None,
        (true, false) => Some(Which::Main),
        (true, true) => Some(Which::Editor),
    }
}

/// The window to bring to the front for `event`, from window `from`,
/// while one of the emulator's windows that take all input (a dialog,
/// the projects, the settings, the editor colors, the keyboard
/// shortcuts, Ctrl+S's and Ctrl+L's windows) shows on `modal_on`. A
/// press, a click, the wheel or typing in the other window brings that
/// one forward, and goes no further: it would otherwise reach the window
/// the user cannot see, or nothing, which looks like a hang. The pointer's
/// motion and releases are left alone.
pub fn blocked_by(event: &Event, modal_on: Option<Which>, from: Which) -> Option<Which> {
    let input = matches!(
        event,
        Event::KeyDown { .. }
            | Event::TextInput { .. }
            | Event::TextEditing { .. }
            | Event::MouseButtonDown { .. }
            | Event::MouseWheel { .. }
    );
    modal_on.filter(|&on| input && on != from)
}

/// The size the main window starts at, in the system's window
/// coordinates, when it asks for `w` x `h` on a display whose usable area
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
    use sdl3::keyboard::{Keycode, Mod, Scancode};
    use sdl3::mouse::{MouseButton, MouseWheelDirection};

    fn key(window_id: u32) -> Event {
        Event::KeyDown {
            timestamp: 0,
            window_id,
            keycode: Some(Keycode::Escape),
            scancode: Some(Scancode::Escape),
            keymod: Mod::NOMOD,
            repeat: false,
            which: 0,
            raw: 0,
        }
    }

    fn inputs() -> Vec<Event> {
        let (timestamp, window_id, which) = (0, 1, 0);
        vec![
            key(window_id),
            Event::TextInput {
                timestamp,
                window_id,
                text: "a".to_string(),
            },
            Event::MouseButtonDown {
                timestamp,
                window_id,
                which,
                mouse_btn: MouseButton::Left,
                clicks: 1,
                x: 10.0,
                y: 10.0,
            },
            Event::MouseWheel {
                timestamp,
                window_id,
                which,
                x: 0.0,
                y: 1.0,
                direction: MouseWheelDirection::Normal,
                integer_x: 0,
                integer_y: 1,
                mouse_x: 10.0,
                mouse_y: 10.0,
            },
        ]
    }

    #[test]
    fn input_in_the_other_window_brings_the_dialogs_forward() {
        use Which::{Editor, Main};
        for event in inputs() {
            // The dialog on the main window, input in the editor's.
            assert_eq!(blocked_by(&event, Some(Main), Editor), Some(Main));
            // And the other way round.
            assert_eq!(blocked_by(&event, Some(Editor), Main), Some(Editor));
            // In the dialog's own window, it goes to the dialog.
            assert_eq!(blocked_by(&event, Some(Main), Main), None);
            assert_eq!(blocked_by(&event, Some(Editor), Editor), None);
            // With no dialog, or no editor's window, nothing is blocked.
            assert_eq!(blocked_by(&event, None, Editor), None);
            assert_eq!(blocked_by(&event, None, Main), None);
        }
    }

    #[test]
    fn the_dialog_is_on_the_window_that_had_the_keys() {
        assert_eq!(modal_window(true, true, false), Some(Which::Main));
        assert_eq!(modal_window(true, true, true), Some(Which::Editor));
        for ui_editor in [false, true] {
            assert_eq!(modal_window(false, true, ui_editor), None);
            assert_eq!(modal_window(true, false, ui_editor), None);
        }
        // A press in the editor's window with the settings on the main
        // one brings the main one forward.
        let modal_on = modal_window(true, true, false);
        assert_eq!(
            blocked_by(&key(2), modal_on, Which::Editor),
            Some(Which::Main)
        );
    }

    #[test]
    fn pointer_motion_and_window_events_bring_nothing_forward() {
        let (timestamp, window_id, which) = (0, 1, 0);
        let left_alone = [
            Event::MouseMotion {
                timestamp,
                window_id,
                which,
                mousestate: sdl3::mouse::MouseState::from_sdl_state(0),
                x: 1.0,
                y: 1.0,
                xrel: 1.0,
                yrel: 1.0,
            },
            Event::MouseButtonUp {
                timestamp,
                window_id,
                which,
                mouse_btn: MouseButton::Left,
                clicks: 1,
                x: 10.0,
                y: 10.0,
            },
            Event::Window {
                timestamp,
                window_id,
                win_event: sdl3::event::WindowEvent::FocusGained,
            },
            Event::Quit { timestamp },
        ];
        for event in left_alone {
            assert_eq!(blocked_by(&event, Some(Which::Main), Which::Editor), None);
        }
    }

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

//! The editor's own window, in runs with no window shown.

use std::process::Command;

/// Quitting with the editor open in its own window leaves no texture to
/// be destroyed after that window's renderer. SDL refuses such a texture
/// by default, so the run turns its checks of objects off, and a texture
/// left over is then read after it was freed. That crashes about half of
/// the runs, so a few of them are made.
#[test]
fn quitting_destroys_the_editor_windows_textures_before_its_renderer() {
    let dir = std::env::temp_dir().join("sb-editor-window");
    std::fs::create_dir_all(dir.join("data")).expect("a scratch folder");
    for _ in 0..4 {
        let out = Command::new(env!("CARGO_BIN_EXE_smilebasic"))
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env("XDG_DATA_HOME", dir.join("data"))
            .env("SDL_VIDEO_DRIVER", "offscreen")
            .env("SDL_AUDIO_DRIVER", "dummy")
            .env("SDL_INVALID_PARAM_CHECKS", "1")
            .env("SB_GPU", "0")
            .env("SB_UI_BENCH", "frames=3,settle=2,novsync,editor,apart")
            .output()
            .expect("the emulator runs");
        assert!(
            out.status.success(),
            "{:?}\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

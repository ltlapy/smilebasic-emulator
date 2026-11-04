//! The GPU path against the CPU, on whatever GPU this machine has, with no window
//! shown. Ignored, as CI has no GPU: `cargo test -p sb-sdl --test gpu_golden --
//! --ignored --nocapture`. The scenes are in `src/gpu_golden.rs`.

use std::process::Command;

#[test]
#[ignore = "needs a GPU"]
fn the_gpu_path_keeps_to_the_golden_scenes() {
    let out = Command::new(env!("CARGO_BIN_EXE_smilebasic"))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env(
            "XDG_DATA_HOME",
            std::env::temp_dir().join("sb-gpu-golden-data"),
        )
        .env("SDL_VIDEO_DRIVER", "offscreen")
        .env("SDL_AUDIO_DRIVER", "dummy")
        .env("SB_GPU_GOLDEN", "1")
        .output()
        .expect("the emulator runs");
    println!("{}", String::from_utf8_lossy(&out.stdout));
    eprintln!("{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "a scene differs too much");
}

/// The last line of a short run with no window, and whether it ended well.
fn drawn_by(envs: &[(&str, &str)]) -> (bool, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_smilebasic"));
    command
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("SB_GPU")
        .env(
            "XDG_DATA_HOME",
            std::env::temp_dir().join("sb-gpu-golden-data"),
        )
        .env("SDL_VIDEO_DRIVER", "offscreen")
        .env("SDL_AUDIO_DRIVER", "dummy")
        .env("SB_UI_BENCH", "frames=3,settle=1,novsync");
    for (key, value) in envs {
        command.env(key, value);
    }
    let out = command.output().expect("the emulator runs");
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find_map(|l| l.strip_prefix("drawn by  "));
    (out.status.success(), line.unwrap_or_default().to_string())
}

#[test]
fn a_driver_that_cannot_start_is_not_fatal() {
    let (ended_well, by) = drawn_by(&[("SDL_RENDER_DRIVER", "nosuch"), ("SB_GPU", "0")]);
    assert!(ended_well);
    assert_eq!(by, "CPU (SB_GPU)");
}

#[test]
fn a_failed_self_test_leaves_the_cpu() {
    let (ended_well, by) = drawn_by(&[("SB_GPU", "1"), ("SB_GPU_TEST", "fail")]);
    assert!(ended_well);
    assert!(by.starts_with("CPU ("), "{by}");
}

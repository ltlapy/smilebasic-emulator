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

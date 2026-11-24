//! MOUSE and TOUCH with their flag 1 use SmileBASIC 4's 1280x720 display
//! in the Simple UI too, in runs with no window shown. The Simple UI's own
//! display has the size of the output.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Puts the pointer in the middle of SmileBASIC 4's display, which is
/// the middle of the screen, and fills the screen green if the program
/// reads it there both ways, or red if not.
const PROGRAM: &str = "MOUSE 640,360,1\nFOR I=0 TO 9:VSYNC:NEXT\n\
                       MOUSE OUT X,Y:MOUSE 1 OUT A,B\n\
                       C=RGB(255,0,0)\n\
                       IF X==200 && Y==120 && A==640 && B==360 THEN C=RGB(0,255,0)\n\
                       GCLS C\nWHILE 1:VSYNC:WEND\n";

/// A separate folder for the test, with the program in it and the Simple
/// UI's welcome screen marked as seen.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("sb-mouse-display").join(name);
    let data = dir.join("data").join("smilebasic-emulator");
    std::fs::create_dir_all(&data).expect("a scratch folder");
    std::fs::write(dir.join("MIDDLE.txt"), PROGRAM).expect("the program is written");
    let settings = "simple_ui_welcome_shown=true\n";
    std::fs::write(data.join("settings.txt"), settings).expect("the settings are written");
    dir
}

/// The color in the middle of the main window once the program has
/// checked the pointer, with `args` before the program.
fn middle_color(name: &str, args: &[&str]) -> [u8; 3] {
    let dir = folder(name);
    let shot = dir.join("shot");
    let bench = format!("frames=1,settle=40,novsync,shot={}", shot.display());
    let out = Command::new(env!("CARGO_BIN_EXE_smilebasic"))
        .args(args)
        .arg(dir.join("MIDDLE.txt"))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("SDL_VIDEO_DRIVER", "offscreen")
        .env("SDL_AUDIO_DRIVER", "dummy")
        .env("SB_GPU", "0")
        .env("SB_CURSOR", "system")
        .env("SB_UI_BENCH", bench)
        .output()
        .expect("the emulator runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (width, height, pixels) = read_pam(&dir.join("shot-main.pam"));
    let at = (height / 2 * width + width / 2) * 4;
    [pixels[at], pixels[at + 1], pixels[at + 2]]
}

/// A PAM file's width, height and RGBA pixels.
fn read_pam(path: &Path) -> (usize, usize, Vec<u8>) {
    let data = std::fs::read(path).expect("the shot was saved");
    let end = (data.windows(7).position(|w| w == b"ENDHDR\n")).expect("a PAM header") + 7;
    let head = String::from_utf8_lossy(&data[..end]);
    let field = |name: &str| {
        let line = head.lines().find_map(|l| l.strip_prefix(name)).expect(name);
        line.trim().parse::<usize>().expect(name)
    };
    assert_eq!(field("DEPTH"), 4);
    (field("WIDTH"), field("HEIGHT"), data[end..].to_vec())
}

#[test]
fn the_middle_of_smilebasic_4s_display_is_the_middle_of_the_screen() {
    const GREEN: [u8; 3] = [0, 255, 0];
    assert_eq!(middle_color("desktop", &[]), GREEN);
    assert_eq!(middle_color("simple", &["--simple-ui"]), GREEN);
}

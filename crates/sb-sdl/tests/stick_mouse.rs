//! The right stick's mouse on the emulator's own screens, in runs with no
//! window shown: R3 clicks the desktop's menus, and the Simple UI's game
//! list shows no cursor.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Puts the pointer over File > Quit, with the mouse cursor hidden, and
/// waits: the screen's 400x240 pixels are the display's 1200x720 from 40
/// across, and Quit's row on the display goes from 162 to 186.
const PROGRAM: &str = "MOUSE 7,58\nWHILE 1:VSYNC:WEND\n";

/// Leaves the pointer in the middle of the screen and ends.
const MIDDLE: &str = "MOUSE 200,120\nEND\n";

/// What the bench prints when it ends by itself, and not by Quit.
const REPORT: &str = "drawn by";

/// A folder of the test's own, with the program in it and the Simple
/// UI's welcome screen seen.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("sb-stick-mouse").join(name);
    let data = dir.join("data").join("smilebasic-emulator");
    std::fs::create_dir_all(&data).expect("a scratch folder");
    std::fs::write(dir.join("QUIT.txt"), PROGRAM).expect("the program is written");
    std::fs::write(dir.join("MIDDLE.txt"), MIDDLE).expect("the program is written");
    let settings = "simple_ui_welcome_shown=true\n";
    std::fs::write(data.join("settings.txt"), settings).expect("the settings are written");
    dir
}

/// Runs the emulator in `dir` with `args` and the bench set to `bench`,
/// drawing the cursor as `cursor` says.
fn run(dir: &Path, args: &[&str], bench: &str, cursor: &str) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_smilebasic"))
        .args(args)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("SDL_VIDEO_DRIVER", "offscreen")
        .env("SDL_AUDIO_DRIVER", "dummy")
        .env("SB_GPU", "0")
        .env("SB_CURSOR", cursor)
        .env("SB_UI_BENCH", bench)
        .output()
        .expect("the emulator runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// Whether the run with the File menu open over the program, and R3
/// pressed and released as `r3` says, ended by itself.
fn ran_to_the_end(name: &str, r3: &str) -> bool {
    let dir = folder(name);
    let program = dir.join("QUIT.txt");
    let bench = format!("frames=20,settle=20,novsync,at=5:menu,at=6:file-menu{r3}");
    let out = run(&dir, &[program.to_str().unwrap()], &bench, "system");
    String::from_utf8_lossy(&out.stdout).contains(REPORT)
}

#[test]
fn r3_clicks_the_menu_item_under_the_pointer() {
    // Without R3, the run goes on to its end...
    assert!(ran_to_the_end("no-click", ""));
    // ...and with it, Quit ends it, though the program hides the cursor.
    assert!(!ran_to_the_end(
        "click",
        ",at=10:pad:rightstick,at=12:padup:rightstick"
    ));
}

/// A PAM file's RGBA pixels.
fn read_pam(path: &Path) -> Vec<u8> {
    let data = std::fs::read(path).expect("the shot was saved");
    let end = (data.windows(7).position(|w| w == b"ENDHDR\n")).expect("a PAM header") + 7;
    data[end..].to_vec()
}

/// The main window's frame once `MIDDLE` has ended, with the cursor
/// drawn by the emulator and by the system (which a shot leaves out),
/// with `args` before the program.
fn shots(name: &str, args: &[&str]) -> (Vec<u8>, Vec<u8>) {
    let dir = folder(name);
    let program = dir.join("MIDDLE.txt");
    let mut args = args.to_vec();
    args.push(program.to_str().unwrap());
    let shot = |cursor: &str| {
        let path = dir.join(format!("shot-{cursor}"));
        let bench = format!("frames=1,settle=30,novsync,shot={}", path.display());
        run(&dir, &args, &bench, cursor);
        read_pam(&dir.join(format!("shot-{cursor}-main.pam")))
    };
    (shot("drawn"), shot("system"))
}

#[test]
fn the_simple_uis_game_list_shows_no_cursor() {
    // DIRECT mode on the desktop shows it...
    let (drawn, system) = shots("desktop", &[]);
    assert_ne!(drawn, system);
    // ...and the Simple UI's game list, where the program ends, does not.
    let (drawn, system) = shots("simple", &["--simple-ui"]);
    assert_eq!(drawn, system);
}

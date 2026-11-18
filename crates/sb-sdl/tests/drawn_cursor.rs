//! The mouse cursor the emulator draws where the system shows none, in
//! runs with no window shown: it shows at the pointer, over the screen,
//! and fades out once the pointer has stayed still for 5 seconds.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Shows the cursor, puts the pointer in the middle of the screen and
/// waits on a dark blue screen.
const PROGRAM: &str = "SYSPARAM \"MOUSECURSOR\",1\nGCLS RGB(0,0,128)\nMOUSE 200,120\n\
                       WHILE 1:VSYNC:WEND\n";

/// The display's pixels around the middle of the screen, where the
/// pointer is.
const AROUND: (std::ops::Range<usize>, std::ops::Range<usize>) = (630..680, 350..400);

/// A folder of the test's own, with the program in it.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("sb-drawn-cursor").join(name);
    std::fs::create_dir_all(dir.join("data")).expect("a scratch folder");
    std::fs::write(dir.join("CURSOR.txt"), PROGRAM).expect("the program is written");
    dir
}

/// The main window's frame `settle` trips round the loop in, as RGBA rows.
fn frame_after(name: &str, settle: u32) -> (usize, Vec<u8>) {
    let dir = folder(name);
    let shot = dir.join("shot");
    let bench = format!("frames=1,settle={settle},novsync,shot={}", shot.display());
    let out = Command::new(env!("CARGO_BIN_EXE_smilebasic"))
        .arg(dir.join("CURSOR.txt"))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("SDL_VIDEO_DRIVER", "offscreen")
        .env("SDL_AUDIO_DRIVER", "dummy")
        .env("SB_GPU", "0")
        .env("SB_CURSOR", "drawn")
        .env("SB_UI_BENCH", bench)
        .output()
        .expect("the emulator runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    read_pam(&dir.join("shot-main.pam"))
}

/// A PAM file's width and RGBA pixels.
fn read_pam(path: &Path) -> (usize, Vec<u8>) {
    let data = std::fs::read(path).expect("the shot was saved");
    let end = (data.windows(7).position(|w| w == b"ENDHDR\n")).expect("a PAM header") + 7;
    let head = String::from_utf8_lossy(&data[..end]);
    let field = |name: &str| {
        let line = head.lines().find_map(|l| l.strip_prefix(name)).expect(name);
        line.trim().parse::<usize>().expect(name)
    };
    assert_eq!(field("DEPTH"), 4);
    (field("WIDTH"), data[end..].to_vec())
}

/// How many pixels around the pointer are white and how many black.
fn arrow_pixels((width, pixels): &(usize, Vec<u8>)) -> (usize, usize) {
    let (xs, ys) = AROUND;
    let colors = ys.flat_map(|y| xs.clone().map(move |x| (y * width + x) * 4));
    colors.fold((0, 0), |(white, black), i| match pixels[i..i + 3] {
        [255, 255, 255] => (white + 1, black),
        [0, 0, 0] => (white, black + 1),
        _ => (white, black),
    })
}

#[test]
fn the_cursor_shows_where_the_program_put_the_pointer() {
    let (white, black) = arrow_pixels(&frame_after("shown", 30));
    // The arrow's white inside and black outline, on the blue screen.
    assert!(white > 100, "{white} white pixels");
    assert!(black > 100, "{black} black pixels");
}

#[test]
fn the_cursor_fades_out_when_the_pointer_stays_still() {
    // About 6 seconds.
    let faded = frame_after("faded", 370);
    assert_eq!(arrow_pixels(&faded), (0, 0));
}

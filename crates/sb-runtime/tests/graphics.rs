//! Graphic commands run from programs.

use sb_runtime::Runtime;
use sb_runtime::font::Font;
use sb_runtime::machine::Machine;

/// Runs a program in PRG0 until it ends, and returns the console's last
/// non-empty row before OK.
fn run(source: &str) -> (Machine, String) {
    let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
    m.boot();
    m.set_slot(0, source.to_string());
    m.run_slot(0);
    for _ in 0..100 {
        m.frame();
        if !m.is_running() {
            break;
        }
    }
    assert!(!m.is_running(), "program did not end");
    let console = &m.rt.text[4];
    let (w, h) = console.size();
    let rows: Vec<String> = (0..h)
        .map(|y| {
            let codes: Vec<u16> = (0..w)
                .map(|x| console.cell(x, y).code)
                .take_while(|&c| c != 0)
                .collect();
            String::from_utf16(&codes).unwrap()
        })
        .filter(|r| !r.is_empty())
        .collect();
    let last = rows[rows.len() - 2].clone();
    (m, last)
}

#[test]
fn draw_and_read_back() {
    let (m, out) = run("GCOLOR #C_RED\n\
         GPSET 1,1\n\
         GLINE 0,5,9,5,#C_LIME\n\
         GFILL 20,20,29,29,&HFF0000FF\n\
         GCIRCLE 50,50,10\n\
         GTRI 100,100,110,100,100,110\n\
         GBOX 60,60,70,70\n\
         GPAINT 65,65,#C_WHITE\n\
         ?HEX$(GPGET(1,1));\" \";HEX$(GPGET(9,5));\" \";HEX$(GPGET(25,25));\" \";GPGET(-1,0)");
    assert_eq!(out, "FFFF0000 FF00FF00 FF0000FF 0");
    assert_eq!(m.rt.grp.pixel(0, 60, 50), 0xFFFF_0000);
    assert_eq!(m.rt.grp.pixel(0, 101, 101), 0xFFFF_0000);
    assert_eq!(m.rt.grp.pixel(0, 65, 65), 0xFFFF_FFFF);
}

#[test]
fn target_clip_and_copy() {
    let (m, out) = run("GTARGET 2:GFILL 0,0,3,3,#C_RED\n\
         GTARGET 0:GCLIP 0,0,1,1\n\
         GCOPY 2,0,0,3,3,0,0,0\n\
         GCLIP OUT A,B,C,D\n\
         GCLIP\n\
         ?GTARGET();A;B;C;D;\" \";HEX$(GCOLOR())");
    assert_eq!(out, "00011 FFFFFFFF");
    assert_eq!(m.rt.grp.pixel(0, 1, 1), 0xFFFF_0000);
    assert_eq!(m.rt.grp.pixel(0, 2, 2), 0, "outside the clip");
    assert_eq!(m.rt.grp.clip, sb_runtime::graphics::Clip::PAGE);
}

#[test]
fn gcls_and_screen() {
    let (mut m, _) = run("GCLS #C_BLUE:?1");
    let fb = m.render();
    assert_eq!(fb.get(399, 239), Some(0xFF00_00FF), "GRP0 is shown");
    let (m, _) = run("GCLS:?1");
    assert_eq!(m.rt.grp.pixel(0, 0, 0), 0);
}

#[test]
fn bad_arguments_are_errors() {
    for (source, error) in [
        ("GTARGET 6", "Out of range"),
        ("GPSET 0,0,0,5", "Out of range"),
        ("GPUTCHR 0,0,\"A\",12", "Out of range"),
        ("GCIRCLE 0,0,5,0,90,2", "Out of range"),
        ("GLINE 0,0", "Illegal function call"),
    ] {
        let (_, out) = run(source);
        assert!(out.starts_with(error), "{source}: {out}");
    }
}

#[test]
fn putchr_draws_glyphs() {
    let (m, _) = run("GPUTCHR 10,10,\"A\",8,2,2,#C_RED:?1");
    // The blank test font has no glyph pixels, so nothing is drawn.
    assert_eq!(m.rt.grp.pixel(0, 10, 10), 0);
    // With #G_NORMAL the glyph's transparent pixels clear what was there.
    let (m, _) = run("GFILL 0,0,15,15,#C_RED:GPUTCHR 0,0,\"A\",16,#C_WHITE,0:?1");
    assert_eq!(m.rt.grp.pixel(0, 0, 0), 0);
    let (m, _) = run("GFILL 0,0,15,15,#C_RED:GPUTCHR 0,0,\"A\",16,#C_WHITE,1:?1");
    assert_eq!(m.rt.grp.pixel(0, 0, 0), 0xFFFF_0000);
    let mut m = Machine::new(Runtime::new(400, 240, Font::fallback()));
    m.boot();
    m.set_slot(0, "GPUTCHR 0,0,\"A\",8,2,2,#C_RED".to_string());
    m.run_slot(0);
    m.frame();
    let red = (0..16)
        .flat_map(|y| (0..16).map(move |x| (x, y)))
        .filter(|&(x, y)| m.rt.grp.pixel(0, x, y) == 0xFFFF_0000)
        .count();
    assert!(red > 8 && red % 4 == 0, "scaled 2x2: {red}");
}

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
        ("GLINE 0,0", "Wrong number of arguments"),
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

#[test]
fn gsave_and_gload() {
    let (m, out) = run("GFILL 0,0,1,0,#C_RED:GPSET 0,1,#C_BLUE\n\
         DIM W%[0]\n\
         GSAVE 0,0,0,2,2,W%\n\
         ?LEN(W%);\" \";HEX$(W%[0]);\" \";HEX$(W%[2])\n\
         GLOAD 10,10,2,2,W%,0\n\
         GLOAD 20,20,2,2,W%,&HFF00FFFF,0\n\
         DIM I[4]:I[0]=1:I[3]=1\n\
         DIM P[2]:P[0]=0:P[1]=#C_WHITE\n\
         GLOAD 30,30,2,2,I,P,1\n\
         GSAMPLE 3,1:?GSAMPLE(3);GSAMPLE(0)");
    assert_eq!(out, "10");
    let g = &m.rt.grp;
    assert_eq!(g.pixel(0, 10, 11), 0xFF00_00FF);
    assert_eq!(g.pixel(0, 21, 20), 0xFF00_0000, "red tinted by cyan");
    assert_eq!((g.pixel(0, 30, 30), g.pixel(0, 31, 30)), (0xFFFF_FFFF, 0));
    assert_eq!(g.pixel(0, 31, 31), 0xFFFF_FFFF);
}

#[test]
fn gsave_needs_room_in_multi_dimensional_arrays() {
    let (_, out) = run("DIM W[1,1]:GSAVE 0,0,2,2,W");
    assert!(out.starts_with("Subscript out of range"), "{out}");
    // Off the page reads as 0.
    let (_, out) =
        run("GPSET 2047,0,#C_RED:DIM W%[0]:GSAVE 2047,0,2,1,W%:?LEN(W%);HEX$(W%[0]);W%[1]");
    assert_eq!(out, "2FFFF00000");
    // Reals take the colours unsigned, integers signed.
    let (_, out) = run(
        "GPSET 0,0,#C_RED:DIM R#[1],I%[1]:GSAVE 0,0,1,1,R#:GSAVE 0,0,1,1,I%:?R#[0];\" \";I%[0]",
    );
    assert_eq!(out, "4294901760 -65536");
    let (_, out) = run("DIM W[3]:GLOAD 0,0,2,2,W,0");
    assert!(out.starts_with("Subscript out of range"), "{out}");
}

#[test]
fn garray_follows_the_page() {
    let (m, out) = run("G=GARRAY(0)\n\
         GPSET 3,2,#C_RED\n\
         G[5,4]=#C_BLUE\n\
         ?HEX$(G[2,3]);\" \";HEX$(GPGET(4,5));\" \";\n\
         GUPDATE 0\n\
         H=GARRAY(0):H[0,0]=7\n\
         ?HEX$(GPGET(4,5));\" \";G[0,0];\" \";DIM(G,0)");
    // GPGET reads the array; the screen takes it on GUPDATE (TTEST21).
    assert_eq!(out, "FFFF0000 FF0000FF FF0000FF 7 2048");
    assert_eq!(m.rt.grp.pixel(0, 4, 5), 0xFF00_00FF);
    let (m, out) = run("G=GARRAY(0):G[20,20]=#C_LIME:GPSET 21,20,#C_RED:?HEX$(GPGET(20,20))");
    assert_eq!(out, "FF00FF00");
    assert_eq!(m.rt.grp.pixel(0, 20, 20), 0);
    let (_, out) = run("G=GARRAY(1):G[0,0]=-1:GUPDATE 1,0,0,0,0:GTARGET 1:?HEX$(GPGET(0,0))");
    assert_eq!(out, "FFFFFFFF");
    let (_, out) = run("GUPDATE 0,0,0,2048,0");
    assert!(out.starts_with("Out of range"), "{out}");
}

#[test]
fn proportional_widths() {
    // The blank test font has blank characters only, a quarter of the
    // font size wide; the spacing comes between them and scales.
    let (_, out) =
        run("?GPUTCHRP(\"AB\");\" \";GPUTCHRP(\"AB\",8,1,0);\" \";GPUTCHRP(\"AB\",16,2,3)");
    assert_eq!(out, "9 4 22");
}

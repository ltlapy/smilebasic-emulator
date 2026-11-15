//! Graphic commands run from programs.

use sb_runtime::Runtime;
use sb_runtime::font::Font;
use sb_runtime::machine::Machine;

/// Runs a program in PRG0 until it ends, and returns the console's last
/// non-empty row before OK, or the error that stopped it, which can take
/// more than a row.
fn run(source: &str) -> (Machine, String) {
    let mut m = Machine::new(Runtime::for_tests(Font::blank()));
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
    if let Some((e, _)) = m.take_program_error() {
        return (m, e.to_string());
    }
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
        ("GPUTCHR 0,0,\"A\",24", "Illegal font type"),
        ("GCIRCLE 0,0,5,0,90,2", "Out of range"),
        ("GLINE 0,0", "Wrong number of arguments"),
    ] {
        let (_, out) = run(source);
        assert!(out.starts_with(error), "{source}: {out}");
    }
}

/// Out of range errors name the argument that was wrong.
#[test]
fn ranges_are_numbered_by_argument() {
    for (source, arg) in [
        ("ATTR 4,128", "ATTR:2"),
        ("ATTR -1,0,3", "ATTR:1"),
        ("ATTR 0,2,0", "ATTR:2"),
        ("ATTR 0,0,4", "ATTR:3"),
        ("ATTR 4,2,0,0", "ATTR:2"),
        ("ATTR 4,0,5,0", "ATTR:3"),
        ("ATTR 4,0,0,4", "ATTR:4"),
        ("DIM A[4]:TLOAD 4,0,0,0,1,A", "TLOAD:4"),
        ("DIM A[4]:TLOAD 4,0,0,1,0,A", "TLOAD:5"),
        ("DIM A[4]:GLOAD 0,0,-1,1,A,0", "GLOAD:3"),
        ("GCLIP 2048,0,1,1", "GCLIP:1"),
        ("GCLIP 0,0,-1,1", "GCLIP:3"),
        ("GCLIP 0,0,1,2048", "GCLIP:4"),
        ("GUPDATE 0,0,2048,0,0", "GUPDATE:3"),
        ("GUPDATE 0,0,0,0,-1", "GUPDATE:5"),
    ] {
        let (_, out) = run(source);
        assert!(out.starts_with("Out of range"), "{source}: {out}");
        assert!(out.contains(&format!("({arg})")), "{source}: {out}");
    }
    let (_, out) = run("ATTR 0,1,3:ATTR 4 OUT H,V,R:?H;V;R;ATTR(4)");
    assert_eq!(out, "01311");
    let (_, out) = run("DIM A[4]:TLOAD 4,0,0,1,1,A:GLOAD 0,0,0,1,A,0:GLOAD 0,0,1,0,A,0:?1");
    assert_eq!(out, "1");
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
    let mut m = Machine::new(Runtime::for_tests(Font::fallback()));
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
fn gsave_takes_rows_wider_than_the_page() {
    // TEST47 part 23: hardware takes 2049 and 4096 pixels.
    let (_, out) = run("DIM A%[1]:GSAVE 0,0,0,2049,1,A%:GLOAD 0,0,2049,1,A%,0\n\
         GSAVE 0,0,0,4096,1,A%:?LEN(A%)");
    assert_eq!(out, "4096");
}

#[test]
fn gsave_and_gload_take_any_size() {
    // TEST48 parts 9-20: no limit but memory; GSAVE reads 0 off the
    // page, GLOAD drops what falls off it, and an empty rectangle is no
    // error.
    for (w, h, len) in [
        (4097, 1, 4097),
        (8192, 1, 8192),
        (2048, 2049, 4196352),
        (1, 4097, 4097),
    ] {
        let (_, out) = run(&format!(
            "GCLS &HFF102030:DIM A%[0]:GSAVE 0,0,0,{w},{h},A%:?LEN(A%);\" \";HEX$(A%[0],8);\" \";HEX$(A%[LEN(A%)-1],8)"
        ));
        assert_eq!(out, format!("{len} FF102030 00000000"), "{w},{h}");
        let (_, out) = run(&format!(
            "GCLS &HFF102030:DIM A%[{w}*{h}]:FILL A%,&HFF405060:GLOAD 0,0,{w},{h},A%,0\n\
             ?HEX$(GPGET(0,0),8);\" \";HEX$(GPGET(MIN({w},2048)-1,MIN({h},2048)-1),8)"
        ));
        assert_eq!(out, "FF405060 FF405060", "{w},{h}");
    }
    let (_, out) = run("DIM A%[0]:GSAVE 0,0,0,0,1,A%:GSAVE 0,0,0,1,0,A%:?LEN(A%)");
    assert_eq!(out, "0");
    let (_, out) =
        run("GCLS &HFF102030:DIM A%[1]:GLOAD 0,0,0,1,A%,0:GLOAD 0,0,1,0,A%,0:?HEX$(GPGET(0,0))");
    assert_eq!(out, "FF102030");
    // More pixels than memory holds.
    let (_, out) = run("DIM A%[0]:GSAVE 0,0,0,100000,100000,A%");
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

#[test]
fn proportional_widths_stay_when_grp5_is_drawn_over() {
    let (_, out) = run(
        "FONTINFO \"A\",16 OUT X,Y:GTARGET 5:GFILL X,Y,X+15,Y+15,-1\n\
         FONTINFO \"A\",8 OUT X,Y:GFILL X,Y,X+7,Y+7,-1:GTARGET 0\n\
         ?GPUTCHRP(\"A\",16);\" \";GPUTCHRP(\"A\",8)",
    );
    assert_eq!(out, "4 2");
}

#[test]
fn boxes_and_copies_at_extreme_coordinates_end() {
    for source in [
        "GBOX 0,-2147483648,5,-2147483648:?1",
        "GCOPY 0,0,0,5,5,-2147483648,-2147483648,0:?1",
        "GCOPY 0,0,0,5,5,2147483647,2147483647,0:?1",
    ] {
        assert_eq!(run(source).1, "1", "{source}");
    }
}

#[test]
fn a_box_on_the_smallest_row_draws_nothing_on_the_page() {
    let (_, out) = run("GCLS 0:GBOX 0,-2147483648,5,-2147483648,#C_WHITE:?GPGET(0,0);GPGET(5,5)");
    assert_eq!(out, "00");
}

#[test]
fn gputchr_at_extreme_coordinates_ends() {
    for source in [
        "GPUTCHR 2147483647,0,\"AB\",16,-1:?1",
        "GPUTCHR 0,2147483647,\"AB\",16,-1:?1",
        "GPUTCHR 2147483647,2147483647,\"AB\",16,32767,32767,-1:?1",
        "GPUTCHR -2147483648,-2147483648,\"AB\",16,32767,32767,-1:?1",
        "GPUTCHRP 2147483647,2147483647,\"AB\",16,32767,32767,-1:?1",
        "GPUTCHRP -2147483648,-2147483648,\"AB\",16,32767,32767,-1:?1",
        "GPUTCHR 0,0,\"A\"*100000,16,1024,1024,-1:?1",
        "GPUTCHRP 0,0,\"A\"*2000,16,32767,32767,-1:?1",
    ] {
        assert_eq!(run(source).1, "1", "{source}");
    }
}

/// A pattern of colors on GRP3: cell 0 of the 16-pixel grid at 0,0 and
/// cell 2210 at 544,544 (column 34, row 34), different from each other.
const USER_CELLS: &str = "GTARGET 3:GCLS\n\
     FOR Y=0 TO 15:FOR X=0 TO 15\n\
     IF (X*3+Y*5) MOD 7<3 THEN GPSET X,Y,RGB(255,X*16,Y*16)\n\
     IF (X*5+Y*3) MOD 4==1 THEN GPSET 544+X,544+Y,RGB(X*16,255,Y*16)\n\
     NEXT:NEXT:GTARGET 1:GCLS\n";

/// Whether GRP1's `size` square at 0,0 holds GRP3's at `from`, which has
/// something drawn.
fn drew_cell(m: &Machine, from: (i32, i32), size: i32) -> bool {
    let at = |page, x, y| m.rt.grp.pixel(page, x, y);
    let square = |f: &dyn Fn(i32, i32) -> bool| (0..size).all(|y| (0..size).all(|x| f(x, y)));
    !square(&|x, y| at(3, from.0 + x, from.1 + y) == 0)
        && square(&|x, y| at(1, x, y) == at(3, from.0 + x, from.1 + y))
}

#[test]
fn gputchr_draws_user_characters_from_the_consoles_tpage() {
    // Cell n of the console's TPAGE, in cells of the font type.
    let (m, _) = run(&format!(
        "{USER_CELLS}TPAGE 4,3,0,0:GPUTCHR 0,0,CHR$(&HE800),16,-1,0:?1"
    ));
    assert!(drew_cell(&m, (0, 0), 16));
    let (m, _) = run(&format!(
        "{USER_CELLS}TPAGE 4,3,0,0:GPUTCHR 0,0,CHR$(&HE800+2210),16,-1,0:?1"
    ));
    assert!(drew_cell(&m, (544, 544), 16));
    // At 8, the 8-pixel cell at the console's origin. Another screen's
    // TPAGE does not matter.
    let (m, _) = run(&format!(
        "{USER_CELLS}TPAGE 4,3,0,0:TPAGE 0,3,544,544:GPUTCHR 0,0,CHR$(&HE800),8,-1,0:?1"
    ));
    assert!(drew_cell(&m, (0, 0), 8));
    assert_eq!(m.rt.grp.pixel(1, 8, 0), 0, "only the 8-pixel cell");
    // The console's TPAGE at the start, GRP4 from 1024,0, is empty there:
    // nothing is drawn, as for a blank glyph.
    let (m, out) = run(&format!(
        "{USER_CELLS}GPUTCHR 0,0,CHR$(&HE800),16,-1,0:TPAGE 4 OUT P,X,Y:?P;X;Y"
    ));
    assert_eq!(out, "410240");
    assert_eq!(m.rt.grp.pixel(4, 1024, 0), 0);
    assert!((0..16).all(|y| (0..16).all(|x| m.rt.grp.pixel(1, x, y) == 0)));
}

#[test]
fn user_characters_under_tpage_minus_one_draw_nothing() {
    let (m, out) = run(&format!(
        "{USER_CELLS}TPAGE 4,-1,0,0:GPUTCHR 0,0,CHR$(&HE800),16,-1,0\n\
         GPUTCHRP 20,0,CHR$(&HE800),16,-1,0:?GPUTCHRP(CHR$(&HE800),16)"
    ));
    assert_eq!(out, "16");
    assert!((0..16).all(|y| (0..40).all(|x| m.rt.grp.pixel(1, x, y) == 0)));
}

#[test]
fn gputchr_multiplies_user_characters_by_the_color() {
    // As it multiplies the font's glyphs.
    let (m, _) = run("GTARGET 3:GCLS:GFILL 0,0,15,15,&HFF80FF40:GTARGET 1:GCLS\n\
         TPAGE 4,3,0,0:GPUTCHR 0,0,CHR$(&HE800),16,&HFFFF0000,0:?1");
    assert_eq!(m.rt.grp.pixel(1, 5, 5), 0xFF80_0000);
}

/// Under GSAMPLE 0, the part of a user-defined character past the page's
/// edge draws nothing, even with #G_NORMAL; under GSAMPLE 1 it wraps.
#[test]
fn user_characters_past_the_pages_edge() {
    let source = |sample| {
        format!(
            "GTARGET 3:GCLS:GFILL 2040,0,2047,15,&HFF8040C0:GFILL 0,0,7,15,&HFF00FF00\n\
             TPAGE 4,3,2040,0:GSAMPLE 3,{sample}:GTARGET 1:GFILL 0,0,31,31,&HFF112233\n\
             GPUTCHR 0,0,CHR$(&HE800),16,-1,0:?1"
        )
    };
    let (m, _) = run(&source(0));
    assert_eq!(m.rt.grp.pixel(1, 3, 3), 0xFF80_40C0);
    assert_eq!(m.rt.grp.pixel(1, 12, 3), 0xFF11_2233);
    let (m, _) = run(&source(1));
    assert_eq!(m.rt.grp.pixel(1, 12, 3), 0xFF00_FF00);
}

/// #G_NORMAL2 with a color of alpha 1: the glyph's own clear pixels are
/// left, the others are written, with the alpha the color gives them.
#[test]
fn gputchr_mode_1_writes_pixels_the_color_makes_clear() {
    let (m, _) = run(
        "GTARGET 3:GCLS:GFILL 0,0,7,15,&H80FFFFFF:GFILL 8,0,15,15,&HFFFFFFFF\n\
         TPAGE 4,3,0,0:GTARGET 1:GFILL 0,0,31,31,&HFF112233\n\
         GPUTCHR 0,0,CHR$(&HE800),16,&H01FFFFFF,1:GTARGET 3:GCLS 0\n\
         GTARGET 1:GPUTCHR 0,16,CHR$(&HE800),16,&H01FFFFFF,1:?1",
    );
    // Glyph alpha 128 comes out as 0, and is written.
    assert_eq!(m.rt.grp.pixel(1, 3, 3) >> 24, 0);
    assert_eq!(m.rt.grp.pixel(1, 12, 3), 0x01FF_FFFF);
    // A clear glyph is not written.
    assert_eq!(m.rt.grp.pixel(1, 3, 20), 0xFF11_2233);
    // A color of alpha 0 draws nothing at all.
    let (m, _) = run("GTARGET 3:GCLS:GFILL 0,0,15,15,&HFFFFFFFF:TPAGE 4,3,0,0\n\
         GTARGET 1:GFILL 0,0,31,31,&HFF112233:GPUTCHR 0,0,CHR$(&HE800),16,&H00FFFFFF,1:?1");
    assert_eq!(m.rt.grp.pixel(1, 3, 3), 0xFF11_2233);
}

#[test]
fn gputchrp_takes_user_characters_whole() {
    // A user-defined character is its whole cell wide, and the
    // ideographic space 14 pixels at 16.
    let (_, out) = run(&format!(
        "{USER_CELLS}TPAGE 4,3,0,0:U$=CHR$(&HE800):H$=CHR$(&HE800+2210)\n\
         ?GPUTCHRP(U$);\" \";GPUTCHRP(U$,8);\" \";GPUTCHRP(H$+H$,16,2,3);\" \";GPUTCHRP(CHR$(&H3000))"
    ));
    // 70 is 2*(16+16+3).
    assert_eq!(out, "16 8 70 14");
    let (m, _) = run(&format!(
        "{USER_CELLS}TPAGE 4,3,0,0:GPUTCHRP 0,0,CHR$(&HE800+2210),16,-1,0:?1"
    ));
    assert!(drew_cell(&m, (544, 544), 16));
}

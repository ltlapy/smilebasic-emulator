//! Commands on what text screens hold, checked through their cells.

use sb_core::error::ErrorKind;
use sb_core::vm::{Status, Vm};
use sb_runtime::Runtime;
use sb_runtime::font::Font;
use sb_runtime::machine::Machine;

fn run(source: &str) -> Result<Runtime, sb_core::error::Error> {
    let mut rt = Runtime::new(400, 240, Font::blank());
    let mut vm = Vm::new(sb_core::compile(source, &rt)?);
    while vm.run(&mut rt, 100_000)? != Status::Ended {}
    Ok(rt)
}

/// Runs a program frame by frame until it ends, and returns the console
/// rows it printed.
fn play(source: &str) -> Vec<String> {
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
    // The rows between the start-up OK and the one at the end.
    let start = rows.iter().position(|r| r == "OK").expect("OK") + 1;
    rows[start..rows.len() - 1].to_vec()
}

/// Row `y` of a screen, with `.` for empty cells.
fn row(rt: &Runtime, screen: usize, y: u32, width: u32) -> String {
    let s = &rt.text[screen];
    (0..width)
        .map(|x| match s.cell(x, y).code {
            0 => '.',
            c => char::from_u32(u32::from(c)).unwrap_or('?'),
        })
        .collect()
}

#[test]
fn tput_and_tfill() {
    let rt = run(concat!(
        "COLOR 0,#C_RED\n",
        "TPUT 0,1,0,\"AB\":TPUT 0,2,0,67,#A_REVV\n",
        "TFILL 0,0,1,4,2,\"XY\"\n",
        "TFILL 0,6,3,5,2,ASC(\"Q\")",
    ))
    .unwrap();
    assert_eq!(row(&rt, 0, 0, 7), ".AC....");
    assert_eq!(row(&rt, 0, 1, 7), "XYXYX..");
    assert_eq!(row(&rt, 0, 2, 7), "YXYXYQQ");
    assert_eq!(row(&rt, 0, 3, 7), ".....QQ");
    let c = rt.text[0].cell(2, 0);
    assert_eq!((c.color, c.attr), (0xFFFF_0000, 8));
    assert_eq!(rt.text[0].cell(1, 0).attr, 0);
}

#[test]
fn scroll_moves_the_contents() {
    let rt =
        run("TFILL 1,0,0,2,2,\"ABCDEFGHI\":SCROLL 1,1,1:SCROLL 2,-1,0:TPUT 2,0,0,\"Z\"").unwrap();
    assert_eq!(row(&rt, 1, 0, 3), "EF.");
    assert_eq!(row(&rt, 1, 1, 3), "HI.");
    assert_eq!(row(&rt, 1, 2, 3), "...");
    assert_eq!(row(&rt, 2, 0, 3), "Z..");
    let rt = run("?\"AB\":?\"CD\":SCROLL 0,-1").unwrap();
    assert_eq!(row(&rt, 4, 0, 3), "...");
    assert_eq!(row(&rt, 4, 1, 3), "AB.");
}

#[test]
fn chkchr_reads_cells_or_display_points() {
    let rt = run(concat!(
        "TPUT 0,2,1,\"K\",5\n",
        "TOFS 0,100,50\n",
        "CHKCHR 0,2,1 OUT C,A\n",
        "?C;A;CHKCHR(0,0,0);CHKCHR(0,99,0);CHKCHR(0,140,70,1);CHKCHR(0,100,50,1)\n",
        "TCOORD 0,32,16 OUT X,Y:?X;Y\n",
        "TCOORD 0,140,70,1 OUT X,Y:?X;Y\n",
        "TCOORD 0,140,70,2 OUT X,Y:?X;Y",
    ))
    .unwrap();
    let text = |y| row(&rt, 4, y, 20).trim_end_matches('.').to_string();
    assert_eq!(text(0), "75500750");
    assert_eq!(text(1), "13266");
    assert_eq!(text(2), "21");
    assert_eq!(text(3), "4020");
}

#[test]
fn tcoord_drops_mode_0_fractions_and_gives_home_on_a_flat_axis() {
    let rt = run(concat!(
        "TOFS 0,100,50\n",
        "TCOORD 0,3.9,2.9 OUT X,Y:?X;Y\n",
        "THOME 0,7,9:TSCALE 0,0,1:TOFS 0,0,0\n",
        "TCOORD 0,20,30,2 OUT X,Y:?X;Y",
    ))
    .unwrap();
    let text = |y| row(&rt, 4, y, 20).trim_end_matches('.').to_string();
    assert_eq!(text(0), "10352");
    assert_eq!(text(1), "739");
}

#[test]
fn tcopy_within_and_between_screens() {
    let rt = run(concat!(
        "TFILL 0,0,0,2,0,\"ABC\"\n",
        "TCOPY 0,0,0,1,0,1,0\n",
        "TCOPY 1,0,2,0,0,0,0,1",
    ))
    .unwrap();
    assert_eq!(row(&rt, 0, 0, 4), "AAB.");
    assert_eq!(row(&rt, 1, 0, 4), "....");
    assert_eq!(row(&rt, 1, 1, 4), "AAB.");
}

#[test]
fn tsave_and_tload() {
    let rt = run(concat!(
        "TPUT 0,1,1,\"A\",4\n",
        "DIM W%[0]:TSAVE 0,1,1,2,1,W%\n",
        "?LEN(W%);HEX$(W%[0]);\" \";HEX$(W%[1]);W%[2]\n",
        "TLOAD 0,3,2,2,1,W%\n",
        "DIM ALL[0]:TSAVE 0,ALL:?LEN(ALL)",
    ))
    .unwrap();
    assert_eq!(row(&rt, 4, 0, 20).trim_end_matches('.'), "440041 FFFFFFFF0");
    assert_eq!(row(&rt, 0, 2, 5), "...A.");
    assert_eq!(rt.text[0].cell(3, 2).attr, 4);
    assert_eq!(row(&rt, 4, 1, 20).trim_end_matches('.'), "750");
}

#[test]
fn tarray_follows_the_screen_until_tupdate() {
    let rt = run(concat!(
        "T=TARRAY(0)\n",
        "TPUT 0,1,0,\"A\"\n",
        "?HEX$(T[0,2]);LEN(T)\n",
        "T[0,0]=&H30042:T[0,1]=#C_RED\n",
        "?CHKCHR(0,0,0)\n",
        "TUPDATE 0,0,0,0,0",
    ))
    .unwrap();
    assert_eq!(row(&rt, 4, 0, 20).trim_end_matches('.'), "41750");
    assert_eq!(row(&rt, 4, 1, 3), "0..");
    let c = rt.text[0].cell(0, 0);
    assert_eq!((c.code, c.attr, c.color), (0x42, 3, 0xFFFF_0000));
}

#[test]
fn bad_arguments() {
    let kind = |src| run(src).err().map(|e| e.kind);
    assert_eq!(kind("TPUT 5,0,0,65"), Some(ErrorKind::OutOfRange));
    assert_eq!(kind("TPUT 0,0,0,65536"), Some(ErrorKind::OutOfRange));
    assert_eq!(kind("TPUT 0,0,0,65,16"), Some(ErrorKind::OutOfRange));
    assert_eq!(kind("TPUT 0,99,0,65"), Some(ErrorKind::OutOfRange));
    assert_eq!(
        kind("DIM A[0]:TSAVE 0,20,0,10,-1,A"),
        Some(ErrorKind::OutOfRange)
    );
    assert_eq!(
        kind("DIM A[0]:TSAVE 0,20,0,10,0,A"),
        Some(ErrorKind::OutOfRange)
    );
    assert_eq!(kind("DIM A[0]:TSAVE 0,20,0,5000,1,A"), None);
    assert_eq!(
        kind("DIM A[1]:TLOAD 0,A"),
        Some(ErrorKind::SubscriptOutOfRange)
    );
    assert_eq!(
        kind("DIM A[2,2]:TSAVE 0,A"),
        Some(ErrorKind::SubscriptOutOfRange)
    );
    assert_eq!(kind("?CHKCHR(0,0,0,2)"), Some(ErrorKind::OutOfRange));
}

#[test]
fn tanim_moves_text_screens() {
    let out = play(concat!(
        "TOFS 1,0,0,7\n",
        "TANIM 0,\"XY\",-2,10,20:TANIM 1,\"Z+\",1,5:TANIM 2,\"I\",1,5\n",
        "TANIM 3,\"XY\",-2,10,20:TSTOP 3\n",
        "?TCHK(0);TCHK(1);TCHK(2)\n",
        "WAIT 3\n",
        "TOFS 0 OUT X,Y:TOFS 1 OUT ,,Z:TOFS 3 OUT A,B\n",
        "?X;Y;Z;A;B;TCHK(0);TCHK(3)",
    ));
    // TCHK reports nothing for a stopped screen, as SPCHK does.
    assert_eq!(out, ["120", "1020120000"]);
}

#[test]
fn tvar_and_tanim_v() {
    let out = play(concat!(
        "TVAR 0,\"A\",5:TVAR 0,1,\"S\"\n",
        "?TVAR(0,\"A\");TVAR(0,1);TVAR(1,\"A\")\n",
        "TVAR 0,\"A\":?TVAR(0,\"A\");TVAR(0,1):TVAR 0:?TVAR(0,1)\n",
        "TANIM 2,\"V\",1,42:WAIT 2:?TVAR(2,7)",
    ));
    assert_eq!(out, ["5S0", "0S", "0", "42"]);
}

#[test]
fn call_text_runs_each_callback() {
    let out = play(concat!(
        "TFUNC 3,\"F\":TFUNC 0,\"@T\":TFUNC 1,\"@T\":TFUNC 1\n",
        "CALL TEXT\n",
        "?\"\"\n",
        "END\n",
        "@T:?\"L\";CALLIDX();:RETURN\n",
        "DEF F:?\"D\";CALLIDX();:END",
    ));
    assert_eq!(out, ["L0D3"]);
}

#[test]
fn tarray_follows_tscreen_and_acls_resets_gsample() {
    let rt = run("T=TARRAY(0):TSCREEN 0,8,8:TPUT 0,0,0,65\n\
         GSAMPLE 0,1:A=GSAMPLE(0):ACLS\n\
         ?T[0,0];LEN(T);A;GSAMPLE(0)")
    .unwrap();
    assert_eq!(row(&rt, 4, 0, 20).trim_end_matches('.'), "65300010");
}

/// A cell TSCREEN or CLS cleared has COLOR and ATTR in it; SCROLL's are empty.
#[test]
fn cleared_cells_keep_color_and_attr() {
    let rt = run("T=TARRAY(1):?HEX$(T[0,1]);\" \";T[0,0];\n\
         TSCREEN 1,8,8,16,16:?\" \";HEX$(T[0,1]);\" \";T[15,31];\n\
         COLOR 1,#C_RED:ATTR 1,1:CLS 1:?\" \";HEX$(T[3,1]);\" \";HEX$(T[3,0]);\n\
         COLOR 1,#C_LIME:ATTR 1,0:TSCREEN 1,8,8,16,16:?\" \";HEX$(T[3,1]);T[3,0];\n\
         SCROLL 1,0,1:?\" \";T[15,1];T[14,1]==T[3,1]")
    .unwrap();
    assert_eq!(
        row(&rt, 4, 0, 50).trim_end_matches('.'),
        "FFFFFFFF 0 FFFFFFFF -1 FFFF0000 10000 FF00FF000 01"
    );
}

#[test]
fn tscreen_undoes_the_screens_look() {
    let rt = run("DIM A[]=[-10,-1],B[]=[-10,45]\n\
         TANIM 0,\"C\",A,0:TANIM 0,\"R\",B,0\n\
         TCOLOR 0,0:THOME 0,8,4:TROT 0,30:TSCALE 0,2,2:TOFS 0,5,6,7\n\
         TSCREEN 0,16,16:TOFS 0 OUT X,Y,Z\n\
         ?HEX$(TCOLOR(0));TCHK(0);X;Y;Z")
    .unwrap();
    let t = &rt.text[0];
    assert_eq!((t.home, t.rot, t.scale), ((0.0, 0.0), 0.0, (1.0, 1.0)));
    assert_eq!(row(&rt, 4, 0, 16).trim_end_matches('.'), "FFFFFFFF0007");
    // TEST47 part 22: TBLEND goes back to 0; TLAYER, TPAGE and TVAR stay.
    let rt = run("TLAYER 1,3:TBLEND 1,1:TPAGE 1,3,16,32:TVAR 1,0,123\n\
         TSCREEN 1,8,8:TPAGE 1 OUT I,J,K\n\
         ?TLAYER(1);TBLEND(1);I;J;K;TVAR(1,0)")
    .unwrap();
    assert_eq!(row(&rt, 4, 0, 16).trim_end_matches('.'), "3031632123");
}

#[test]
fn xscreen_stops_text_animations_and_keeps_the_look() {
    let rt = run("DIM A[]=[-10,-1],B[]=[-10,45]\n\
         TANIM 0,\"C\",A,0:TANIM 0,\"R\",B,0:TCOLOR 0,RGB(255,0,0)\n\
         THOME 0,8,4:TROT 0,30:TSCALE 0,2,2:TOFS 0,5,6,7\n\
         XSCREEN 400,240:THOME 0 OUT HX,HY:TOFS 0 OUT X,Y,Z\n\
         ?HEX$(TCOLOR(0));TCHK(0);HX;HY;TROT(0);X;Y;Z")
    .unwrap();
    assert_eq!(row(&rt, 4, 0, 20).trim_end_matches('.'), "FFFF000008430567");
}

#[test]
fn extreme_scrolls_and_rectangles_end() {
    for source in [
        "SCROLL 0,2147483647",
        "SCROLL 2147483647,0",
        "SCROLL -2147483648,-2147483648",
        "TCOPY 0,-1000000000,-1000000000,1000000000,1000000000,0,0",
        "TCOPY 0,-2147483648,-2147483648,2147483647,2147483647,2147483647,2147483647",
        "TFILL 0,-1000000000,-1000000000,1000000000,1000000000,\"A\"",
        "TUPDATE 0,-2147483648,-2147483648,2147483647,2147483647",
        "TSCREEN 0,8,8,256,128:TPRINT 0,CHR$(10)*1000000",
    ] {
        run(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    }
}

#[test]
fn a_rectangle_is_clipped_before_tfills_string_starts() {
    let rt = run(concat!(
        "TSCREEN 0,8,8,6,3\n",
        "TFILL 0,-2147483648,-2147483648,2147483647,2147483647,\"ABC\"",
    ))
    .unwrap();
    for y in 0..3 {
        assert_eq!(row(&rt, 0, y, 6), "ABCABC");
    }
    let rt = run(concat!(
        "TSCREEN 0,8,8,6,3\n",
        "TFILL 0,-1,-1,4,1,\"ABCDEFG\"",
    ))
    .unwrap();
    assert_eq!(row(&rt, 0, 0, 6), "ABCDE.");
    assert_eq!(row(&rt, 0, 1, 6), "FGABC.");
    assert_eq!(row(&rt, 0, 2, 6), "......");
}

#[test]
fn tscreen_with_too_many_cells_is_out_of_range() {
    let e = run("TSCREEN 0,8,8,65536,65536").err().expect("an error");
    assert_eq!(e.kind, ErrorKind::OutOfRange);
    assert!(e.detail.is_some());
}

#[test]
fn scaled_text_settles_ties_as_sprites_do() {
    // TEST63: a user character's texels at TSCALE 1.5 and 0.5, on an 8-pixel
    // cell: the page's lower texel at 1.5, the higher at 0.5.
    let texels = |scale: &str, along: (u32, u32), step: (u32, u32), n: u32| -> Vec<u32> {
        let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
        m.boot();
        m.set_slot(
            0,
            format!(
                "GTARGET 3:FOR Y=0 TO 7:FOR X=0 TO 7:GPSET X,Y,RGB(X*7+8,Y*7+8,100):NEXT:NEXT\n\
                 GTARGET 0:TSCREEN 0,8,8:TPAGE 0,3,0,0:TOFS 0,40,40:TSCALE 0,{scale}\n\
                 LOCATE 0,0,0:TPRINT 0,CHR$(&HE800);"
            ),
        );
        m.run_slot(0);
        for _ in 0..100 {
            m.frame();
            if !m.is_running() {
                break;
            }
        }
        let fb = m.rt.render();
        (0..n)
            .map(|i| {
                let p = fb.get(along.0 + i * step.0, along.1 + i * step.1).unwrap();
                let v = if step.0 > 0 {
                    p >> 16 & 255
                } else {
                    p >> 8 & 255
                };
                (v - 8) / 7
            })
            .collect()
    };
    assert_eq!(
        texels("1.5,1.5", (40, 44), (1, 0), 7),
        [0, 0, 1, 2, 2, 3, 4]
    );
    assert_eq!(
        texels("1.5,1.5", (44, 40), (0, 1), 7),
        [0, 0, 1, 2, 2, 3, 4]
    );
    assert_eq!(texels("0.5,0.5", (40, 42), (1, 0), 4), [1, 3, 5, 7]);
    assert_eq!(texels("0.5,0.5", (42, 40), (0, 1), 4), [1, 3, 5, 7]);
}

/// TEST63: on a half pixel a cell's bottom edge reads the page's next row, off the
/// character, and at a side that is no power of two its left edge the column before.
#[test]
fn a_tie_can_read_the_page_past_a_cell() {
    let run = |source: &str| {
        let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
        m.boot();
        m.set_slot(0, source.to_string());
        m.run_slot(0);
        for _ in 0..200 {
            m.frame();
            if !m.is_running() {
                break;
            }
        }
        m.rt.render().clone()
    };
    let page = "GTARGET 3:FOR Y=0 TO 47:FOR X=0 TO 47:GPSET X,Y,RGB(X*5+8,Y*5+8,100):NEXT:NEXT\n\
                GTARGET 0:";
    let fb = run(&format!(
        "{page}TSCREEN 0,8,8:TPAGE 0,3,0,0:TOFS 0,40.5,40.5:LOCATE 0,0,0:TPRINT 0,CHR$(&HE800);"
    ));
    // The row under the cell's last, at the pixel its bottom edge runs through.
    let green = |p: u32| ((p >> 8 & 255) - 8) / 5;
    assert_eq!(green(fb.get(44, 48).unwrap()), 8);
    let fb = run(&format!(
        "{page}TSCREEN 0,24,24:TPAGE 0,3,0,0:TOFS 0,40.5,40.5:LOCATE 0,0,0:TPRINT 0,CHR$(&HE801);"
    ));
    // The character is the page's second of 24: its left edge reads the first's last.
    let red = |p: u32| ((p >> 16 & 255) - 8) / 5;
    assert_eq!(red(fb.get(40, 44).unwrap()), 23);
    assert_eq!(red(fb.get(41, 44).unwrap()), 24);
}

//! Text screens (placement, fonts, user-defined characters), display
//! order and layers.

use sb_runtime::Runtime;
use sb_runtime::font::{Font, FontSize};
use sb_runtime::framebuffer::Framebuffer;
use sb_runtime::machine::Machine;
use sb_runtime::raster::{RasterInput, SceneBackend};
use sb_runtime::scene::Scene;

const RED: u32 = 0xFFFF_0000;
const BLUE: u32 = 0xFF00_00FF;
const WHITE: u32 = 0xFFFF_FFFF;

/// Runs `source` with the bundled font for a few frames and returns the
/// machine, so the frame can be rendered.
fn run(source: &str) -> Machine {
    let mut m = Machine::new(Runtime::for_tests(Font::fallback()));
    m.boot();
    m.set_slot(0, source.to_string());
    m.run_slot(0);
    for _ in 0..5 {
        m.frame();
    }
    assert!(!m.is_running(), "program did not end");
    m
}

/// A program writes only the character codes through TARRAY, on a new screen
/// or a cleared one: the colours are there already, so the characters show.
#[test]
fn codes_alone_written_through_tarray_show() {
    for clear in ["", "CLS 1:"] {
        let mut m = run(&format!(
            "GTARGET 4:GFILL 1024,0,1031,7,#C_RED:GTARGET 0\n\
             TSCREEN 1,8,8,16,16:{clear}T=TARRAY(1):T[0,0]=&HE800:TUPDATE 1"
        ));
        assert_eq!(m.render().get(2, 2), Some(RED), "after {clear:?}");
    }
}

/// A pixel of "A" that the font draws fully opaque.
fn lit_pixel_of_a() -> (u32, u32) {
    let font = Font::fallback();
    (0..64)
        .map(|i| (i % 8, i / 8))
        .find(|&(x, y)| font.pixel(u16::from(b'A'), FontSize::Small, x, y) == WHITE)
        .expect("a lit pixel")
}

/// A red 16x16 sprite 0 at (0,0) in GRP4, made from template-free SPSET.
const RED_SPRITE: &str = "GTARGET 4:GFILL 0,0,15,15,#C_RED:GFILL 16,0,31,15,#C_BLUE:GTARGET 0\n";

/// A cell's attribute goes whole through TLOAD, TARRAY and TCOPY, and comes
/// back from TSAVE and CHKCHR; its bit 7 fills the cell around the glyph.
#[test]
fn tload_and_tarray_keep_the_whole_attribute_and_bit_7_inverts() {
    let (x, y) = lit_pixel_of_a();
    let m = run(concat!(
        "BACKCOLOR &HFF000000\n",
        "DIM T%[2]:T%[0]=&H12800041:T%[1]=&HFFFF0000:TLOAD 4,0,0,1,1,T%\n",
        "DIM S%[2]:TSAVE 4,0,0,1,1,S%\n",
        "CHKCHR 0,0 OUT C,A\n",
        "TCOPY 4,0,0,0,0,2,0\n",
        "A%=TARRAY(4):A%[0,8]=&H800041:A%[0,9]=&HFF00FF00:TUPDATE 4\n",
        "CHKCHR 4,0 OUT D,B\n",
        "LOCATE 0,5:?HEX$(S%[0]);\" \";C;\" \";A;\" \";D;\" \";B\n"
    ));
    let console = &m.rt.text[4];
    let line: String = (0..30)
        .map(|i| console.cell(i, 5).code)
        .take_while(|&c| c != 0)
        .map(|c| char::from_u32(c.into()).unwrap())
        .collect();
    assert_eq!(line, "12800041 65 4736 65 128");
    let mut m = m;
    let fb = m.rt.render();
    let at = |x: u32, y: u32| fb.get(x, y).unwrap();
    const BLACK: u32 = 0xFF00_0000;
    // The glyph is a hole in the color. TLOAD's cell, then TCOPY's copy.
    assert_eq!(at(x, y), BLACK);
    assert_eq!(at(16 + x, y), BLACK);
    let hole = (0..64)
        .map(|i| (i % 8, i / 8))
        .find(|&(a, b)| at(a, b) != BLACK)
        .expect("something filled");
    assert_eq!(at(hole.0, hole.1), RED);
    assert_eq!(at(16 + hole.0, hole.1), RED);
    // TARRAY's, with TUPDATE.
    assert_eq!(at(32 + x, y), BLACK);
    assert_eq!(at(32 + hole.0, hole.1), 0xFF00_FF00);
}

#[test]
fn the_console_starts_in_front_of_sprites() {
    let (x, y) = lit_pixel_of_a();
    let mut m = run(&format!(
        "{RED_SPRITE}CLS:SPSET 0,0,0,16,16:SPOFS 0,0,0,-4000:PRINT \"A\""
    ));
    assert_eq!(m.render().get(x, y), Some(WHITE));
}

#[test]
fn sprites_win_ties_with_text() {
    let (x, y) = lit_pixel_of_a();
    let mut m = run(&format!(
        "{RED_SPRITE}CLS:TOFS 4,0,0,0:SPSET 0,0,0,16,16:PRINT \"A\""
    ));
    assert_eq!(m.render().get(x, y), Some(RED));
}

#[test]
fn text_screens_move_and_turn() {
    let (x, y) = lit_pixel_of_a();
    let mut m = run("CLS:PRINT \"A\":TOFS 4,100,50:TOFS 4 OUT X,Y,Z:THOME 4,0,0:TSCALE 4,2,2");
    let fb = m.render();
    assert_eq!(fb.get(x, y), Some(0xFF00_0000), "moved away");
    assert_eq!(fb.get(100 + 2 * x, 50 + 2 * y), Some(WHITE));
    let mut m = run("CLS:PRINT \"A\":TROT 4,180:TOFS 4,8,8:TCOLOR 4,#C_RED");
    // Turned half around the top-left, which now sits at (8,8).
    assert_eq!(m.render().get(7 - x, 7 - y), Some(RED));
}

#[test]
fn lower_layers_are_in_front_whatever_the_z() {
    let mut m = run(&format!(
        "{RED_SPRITE}THIDE 4\n\
         SPSET 0,0,0,16,16:SPOFS 0,0,0,4000\n\
         SPSET 1,16,0,16,16:SPOFS 1,0,0,-4000:SPLAYER 1,1"
    ));
    assert_eq!(m.render().get(4, 4), Some(RED));
}

#[test]
fn layers_clip_move_and_compose() {
    let mut m = run(&format!(
        "{RED_SPRITE}THIDE 4:BACKCOLOR #C_LIME\n\
         SPSET 0,0,0,16,16:LCLIP 0,4,0,7,15"
    ));
    let fb = m.render();
    assert_eq!(
        fb.get(3, 0),
        Some(0xFF00_FF00),
        "clipped off, BACKCOLOR shows"
    );
    assert_eq!(fb.get(4, 0), Some(RED));
    let mut m = run(&format!(
        "{RED_SPRITE}THIDE 4:SPSET 0,0,0,16,16:LMATRIX 0,0,0,100,10"
    ));
    let fb = m.render();
    assert_eq!(fb.get(4, 4), Some(0xFF00_0000));
    assert_eq!(fb.get(104, 14), Some(RED));
    // Blue multiplied over white leaves blue; the color multiplies too.
    let mut m = run(&format!(
        "{RED_SPRITE}THIDE 4:BACKCOLOR #C_WHITE\n\
         SPSET 0,16,0,16,16:LAYER 0,3:SPSET 1,16,0,16,16:SPOFS 1,20,0:SPLAYER 1,1\n\
         LAYER 1,1,#C_BLACK"
    ));
    let fb = m.render();
    assert_eq!(fb.get(4, 4), Some(BLUE));
    assert_eq!(fb.get(30, 4), Some(0xFF00_0000));
}

#[test]
fn settings_read_back() {
    let mut m = Machine::new(Runtime::for_tests(Font::blank()));
    m.boot();
    m.set_slot(
        0,
        "TOFS 1,3,4,-5:TOFS 1 OUT X,Y,Z:THOME 1,6,7:THOME 1 OUT HX,HY\n\
         TROT 1,45:TSCALE 1,2,3:TSCALE 1 OUT SX,SY:TCOLOR 1,#C_RED:TBLEND 1,1\n\
         THIDE 1:TLAYER 1,6:LCLIP 2,9,8,1,2:LCLIP 2 OUT A,B,C,D:LCLIP 3 OUT E,F,G,H\n\
         CLS:?X;Y;Z;HX;HY;TROT(1);SX;SY;HEX$(TCOLOR(1));TBLEND(1);TSHOW(1);THIDE(1);\n\
         ?TLAYER(1);A;B;C;D;E;F;G;H;HEX$(BACKCOLOR())"
            .to_string(),
    );
    m.run_slot(0);
    for _ in 0..5 {
        m.frame();
    }
    let console = &m.rt.text[4];
    let codes: Vec<u16> = (0..console.size().0)
        .map(|x| console.cell(x, 0).code)
        .take_while(|&c| c != 0)
        .collect();
    assert_eq!(
        String::from_utf16(&codes).unwrap(),
        "34-5674523FFFF00001016129800399239FF000000"
    );
}

/// The console's first line after `source` runs, as text, joining the
/// rows it wraps onto.
fn first_row(source: &str) -> String {
    let mut m = Machine::new(Runtime::for_tests(Font::blank()));
    m.boot();
    m.set_slot(0, source.to_string());
    m.run_slot(0);
    for _ in 0..5 {
        m.frame();
    }
    let console = &m.rt.text[4];
    let (w, h) = console.size();
    let codes: Vec<u16> = (0..w * h)
        .map(|i| console.cell(i % w, i / w).code)
        .take_while(|&c| c != 0)
        .collect();
    String::from_utf16(&codes).unwrap()
}

#[test]
fn tscreen_sizes_and_attributes() {
    assert_eq!(
        first_row(
            "TSCREEN 16:TSCREEN 1,8,16,10,5:TSCREEN 2,32,32\n\
             TSCREEN OUT A,B,C,D:TSCREEN 1 OUT E,F,G,H:TSCREEN 2 OUT I,J,K,L\n\
             ATTR 1,1,3:ATTR OUT P,Q,R:ATTR 2,6:TPAGE 2,-1,8,9:TPAGE 2 OUT S,T,U\n\
             ?A;B;C;D;\" \";E;F;G;H;\" \";I;J;K;L;\" \";ATTR();P;Q;R;ATTR(2);S;T;U"
        ),
        "16162515 816105 3232127 151136-189"
    );
    // A font type SmileBASIC 4 has no font for, checked after the sizes.
    assert!(first_row("CLS:TSCREEN 12").starts_with("Illegal font type"));
    assert!(first_row("CLS:TSCREEN 1,12,8,0,3").starts_with("Out of range"));
    assert!(first_row("CLS:TSCREEN 1,12,8,3,3").starts_with("Illegal font type"));
    // A size given is 1 or more.
    assert!(first_row("CLS:TSCREEN 2,32,0,3").starts_with("Out of range"));
    assert!(first_row("CLS:TSCREEN 1,8,8,300,300").starts_with("Out of range"));
    // More cells than 32 bits count.
    assert!(first_row("CLS:TSCREEN 1,8,8,65536,65536").starts_with("Out of range"));
    assert!(first_row("CLS:TSCREEN 1,8,8,2147483647,2").starts_with("Out of range"));
}

#[test]
fn user_defined_characters_come_from_tpage() {
    let mut m = run("GTARGET 4:GPSET 1027,2,#C_RED:GTARGET 0\n\
         CLS:PRINT CHR$(&HE800);:ATTR 0,0,2:PRINT CHR$(&HE800)");
    let fb = m.render();
    assert_eq!(fb.get(3, 2), Some(RED));
    assert_eq!(fb.get(8 + 4, 5), Some(RED), "turned half around");
    let mut m = run("CLS:TPAGE -1,0,0:PRINT CHR$(&HE801)");
    assert_eq!(m.render().get(5, 5), Some(WHITE));
}

#[test]
fn drawing_on_grp5_changes_the_font() {
    let (x, y) = lit_pixel_of_a();
    // Find "A" on GRP5 by where the font put it, then clear it there.
    let font = Font::fallback();
    let (gx, gy) = font
        .origin(u16::from(b'A'), FontSize::Small)
        .expect("on the page");
    let mut m = run(&format!(
        "CLS:PRINT \"AA\":GTARGET 5:GFILL {gx},{gy},{},{},0:GTARGET 0",
        gx + 7,
        gy + 7
    ));
    let fb = m.render();
    assert_eq!(fb.get(x, y), Some(0xFF00_0000));
    assert_eq!(fb.get(8 + x, y), Some(0xFF00_0000));
    // The editor and other overlays keep the original font.
    assert_eq!(font.pixel(u16::from(b'A'), FontSize::Small, x, y), WHITE);
}

#[test]
fn xscreen_sets_the_resolution() {
    assert_eq!(
        first_row(
            "XSCREEN 320,200,2,1:XSCREEN OUT W,H,S,M,A:TSCREEN OUT F,C,TW,TH\n\
             ?W;H;S;M;FLOOR(A*100);\" \";F;C;TW;TH"
        ),
        "32020021160 884025"
    );
    assert!(first_row("CLS:XSCREEN 130,240").starts_with("Use multiples of 4"));
    assert!(first_row("CLS:XSCREEN 126,240").starts_with("Out of range"));
    assert!(first_row("CLS:XSCREEN 400,240,4").starts_with("Out of range"));
    assert!(first_row("CLS:XSCREEN 400,240,1,3").starts_with("Out of range"));
    assert!(first_row("CLS:XSCREEN 400,240,1,0,20").starts_with("Out of range"));
}

/// Errors name the argument that was wrong, as SmileBASIC 4's do.
#[test]
fn screen_errors_name_the_argument() {
    for (source, named) in [
        ("XSCREEN 16,240", "(XSCREEN:1)"),
        ("XSCREEN 400,721", "(XSCREEN:2)"),
        ("XSCREEN 400,240,11", "(XSCREEN:3)"),
        ("XSCREEN 400,240,1,3", "(XSCREEN:4)"),
        ("XSCREEN 400,240,1,0,20", "(XSCREEN:5)"),
        ("XSCREEN 400,242", "(XSCREEN:2)"),
        ("XSCREEN 400,240,4,0", "(XSCREEN:3)"),
        ("XSCREEN 400,240,4,0,1", "(XSCREEN:4)"),
        ("TSCREEN 0,8,8,32769,1", "(TSCREEN:4)"),
        ("TSCREEN 0,9,8,8", "(TSCREEN:2)"),
        ("TPAGE 0,6,0,0", "(TPAGE:2)"),
        ("TPAGE 0,0,0,32768", "(TPAGE:4)"),
        ("TPAGE 0,32768,0", "(TPAGE:2)"),
        ("TOFS 0,0,0,4096", "(TOFS:4)"),
        ("LFILTER 0,0,1", "(LFILTER:2)"),
        ("LFILTER 0,1,1281", "(LFILTER:3)"),
        ("DIM A%[240,4]:LFILTER 0,3,A%,1", "(LFILTER:3)"),
        ("DIM M#[15]:LMATRIX 0,M#", "(LMATRIX:2)"),
        ("DIM M#[16]:M#[0]=1E39:LMATRIX 0,M#", "(LMATRIX:2)"),
    ] {
        let said = first_row(&format!("CLS:{source}"));
        assert!(said.contains(named), "{source}: {said}");
    }
}

#[test]
fn sampling_makes_the_frame_finer() {
    let mut m = run(&format!(
        "{RED_SPRITE}THIDE 4:XSCREEN 256,128,2:SPSET 0,0,0,16,16:SPOFS 0,10,10\n\
         LCLIP 0,0,0,19,199"
    ));
    let fb = m.render();
    assert_eq!((fb.width(), fb.height()), (512, 256));
    assert_eq!(fb.get(20, 20), Some(RED));
    assert_eq!(fb.get(39, 51), Some(RED));
    assert_eq!(
        fb.get(40, 51),
        Some(0xFF00_0000),
        "clipped in coarse pixels"
    );
    assert_eq!(fb.get(19, 19), Some(0xFF00_0000));
}

#[test]
fn acls_goes_back_to_the_start() {
    let source = "GPSET 1,1,#C_RED:SPSET 0,0,0,16,16:BACKCOLOR #C_BLUE:LAYER 2,1\n\
         TOFS 1,5,5:TSCREEN 1,8,8:XSCREEN 256,128:ANIMDEF 1,\"XY\",1,1,1\n";
    assert_eq!(
        first_row(&format!(
            "{source}ACLS\n\
             XSCREEN OUT W,H:TOFS 1 OUT X,Y:TSCREEN 1 OUT F,C,TW,TH\n\
             ?W;H;X;Y;F;C;TW;TH;SPUSED(0);HEX$(BACKCOLOR());GPGET(1,1)"
        )),
        "40024000161625150FF0000000"
    );
    assert_eq!(
        first_row(&format!("{source}ACLS 1,0,0:?HEX$(GPGET(1,1))")),
        "FFFF0000"
    );
}

/// The steps TEST12 and TEST13 read back from FADE() on hardware.
#[test]
fn fade_steps_as_on_hardware() {
    let steps = |setup: &str, to: &str, n: u32| {
        let mut m = Machine::new(Runtime::for_tests(Font::blank()));
        m.boot();
        let src = format!(
            "{setup}:WAIT 1:FADE {to},{n}:R$=HEX$(FADE(),8)\nFOR I=1 TO {n}:VSYNC:R$=R$+\" \"+HEX$(FADE(),8):NEXT:?R$"
        );
        m.set_slot(0, src);
        m.run_slot(0);
        for _ in 0..30 {
            m.frame();
        }
        let console = &m.rt.text[4];
        (0..console.size().1)
            .map(|y| {
                (0..console.size().0)
                    .map(|x| console.cell(x, y).code)
                    .take_while(|&c| c != 0)
                    .map(|c| char::from_u32(c.into()).unwrap())
                    .collect::<String>()
            })
            .find(|r| r.len() > 20 && r.is_ascii())
            .unwrap_or_default()
    };
    assert_eq!(
        steps("FADE 0", "&HFF7F3F01", 3),
        "007F3F01 557F3E01 AA7F3F01 FF7F3F01"
    );
    assert_eq!(
        steps("FADE &H80FF0000", "&H800000FF", 4),
        "80FF0000 80BF003F 807F007F 803F00BF 800000FF"
    );
    assert_eq!(
        steps("FADE &HFF000000", "&H00FFFFFF", 3),
        "FF000000 A9000000 54000000 00000000"
    );
    assert_eq!(
        steps("FADE 0", "&H40FF8000", 3),
        "00FF8000 15FE8000 2AFF8000 40FF8000"
    );
}

#[test]
fn fade_covers_everything_and_moves_in_steps() {
    // FADE at once: half-covered red over the black back.
    let mut m = run("FADE &H80FF0000");
    let p = m.render().get(10, 10).unwrap();
    assert_eq!(p, 0xFF80_0000);
    // Over 4 frames from clear to opaque white; the program waits 2.
    let mut m = Machine::new(Runtime::for_tests(Font::fallback()));
    m.boot();
    m.set_slot(
        0,
        "VSYNC:FADE &HFFFFFFFF,4:A=FADECHK():VSYNC 2:B=FADE():VSYNC 3:?A;HEX$(B);FADECHK();HEX$(FADE())"
            .to_string(),
    );
    m.run_slot(0);
    for _ in 0..10 {
        m.frame();
    }
    assert!(!m.is_running());
    let console = &m.rt.text[4];
    let rows: Vec<String> = (0..console.size().1)
        .map(|y| {
            (0..console.size().0)
                .map(|x| console.cell(x, y).code)
                .take_while(|&c| c != 0)
                .map(|c| char::from_u32(c.into()).unwrap())
                .collect()
        })
        .collect();
    // From clear, only the alpha moves.
    assert!(rows.iter().any(|r| r == "17FFFFFFF0FFFFFFFF"), "{rows:?}");
}

#[test]
fn layer_filters() {
    // On layer 1, away from the console's OK.
    let sprite = format!("{RED_SPRITE}CLS:SPSET 0,0,0,32,16:SPLAYER 0,1\n");
    let mut m = run(&format!(
        "{sprite}LFILTER 1,1,16:IF LFILTER(1)!=1 THEN STOP"
    ));
    let fb = m.render();
    assert_eq!((fb.get(15, 15), fb.get(16, 0)), (Some(RED), Some(BLUE)));
    let mut m = run(&format!(
        "{sprite}LFILTER 1,1,32:LFILTER 1:IF LFILTER(1) THEN STOP"
    ));
    assert_eq!(m.render().get(0, 0), Some(RED));
    let mut m = run(&format!(
        "{sprite}DIM R#[240,2]:FOR I=0 TO 239:R#[I,0]=-0.1:R#[I,1]=1:NEXT\nLFILTER 1,3,R#,0"
    ));
    let fb = m.render();
    assert_eq!(
        (fb.get(0, 0), fb.get(40, 0)),
        (Some(0xFF00_0000), Some(RED))
    );
    let mut m = run(&format!("{sprite}LFILTER 1,5,120,0,0"));
    assert_eq!(m.render().get(0, 0), Some(0xFF00_FF00));
    // Not composed, the filter takes in what is behind the layer too; a
    // composed layer filters only itself (TTEST22).
    let mut m = run(&format!("{sprite}BACKCOLOR #C_RED:LFILTER 1,5,120,0,0"));
    assert_eq!(m.render().get(200, 200), Some(0xFF00_FF00));
    let mut m = run(&format!(
        "{sprite}BACKCOLOR #C_RED:LAYER 1,1:LFILTER 1,5,120,0,0"
    ));
    let fb = m.render();
    assert_eq!(
        (fb.get(0, 0), fb.get(200, 200)),
        (Some(0xFF00_FF00), Some(RED))
    );
}

/// LFILTER copies a raster array: what the program writes later does nothing
/// until the next LFILTER.
#[test]
fn raster_array_is_copied_at_lfilter() {
    let sprite =
        "GTARGET 2:GFILL 0,0,39,79,#C_RED:GTARGET 0:SPSET 0,0,0,400,80,0:SPPAGE 0,2:SPLAYER 0,1\n";
    let setup = "DIM R#[240,2]:FOR I=0 TO 239:R#[I,1]=1:NEXT\nLFILTER 1,3,R#,0\n";
    let write = "FOR I=0 TO 239:R#[I,0]=-0.05:NEXT\n";
    let mut m = run(&format!("{sprite}{setup}{write}"));
    assert_eq!(
        m.render().get(0, 0),
        Some(RED),
        "the write alone is not seen"
    );
    let mut m = run(&format!("{sprite}{setup}{write}LFILTER 1,3,R#,0\n"));
    assert_ne!(m.render().get(0, 0), Some(RED), "LFILTER takes it");
}

#[test]
fn raster_filter_forms() {
    let err = |source: &str| {
        let mut rt = Runtime::for_tests(Font::blank());
        let mut vm = sb_core::vm::Vm::new(sb_core::compile(source, &rt).unwrap());
        vm.run(&mut rt, 100_000).err().map(|e| e.kind)
    };
    use sb_core::error::ErrorKind;
    // As SmileBASIC 4.4.9 answers (TTEST22, TTEST25).
    assert_eq!(
        err("DIM Q[4,240]:LFILTER 1,3,Q"),
        Some(ErrorKind::OutOfRange)
    );
    assert_eq!(
        err("DIM Q[240,2]:LFILTER 1,3,Q"),
        Some(ErrorKind::OutOfRange)
    );
    assert_eq!(
        err("DIM Q[4,100]:LFILTER 1,4,Q,1"),
        Some(ErrorKind::TypeMismatch)
    );
    assert_eq!(err("DIM Q[240,4]:LFILTER 1,3,Q,#TRUE"), None);
    assert_eq!(err("DIM Q[240,2]:LFILTER 1,3,Q,#TRUE"), None);
    assert_eq!(
        err("DIM Q%[240,4]:LFILTER 1,3,Q%,#TRUE"),
        Some(ErrorKind::TypeMismatch)
    );
    assert_eq!(err("LFILTER 1,0,5"), Some(ErrorKind::OutOfRange));
}

#[test]
fn scaled_sprites_take_the_left_column_on_a_boundary() {
    // Columns of red, blue, red, blue at a scale of 1.5, as SmileBASIC 4
    // draws them (TTEST24): two, one, two and one pixels wide.
    let mut m = run(
        "GTARGET 4:GCLS:FOR X=0 TO 3:GLINE X,0,X,3,(X MOD 2)*&HFF0000FF+(1-X MOD 2)*&HFFFF0000:NEXT:GTARGET 0\nCLS:SPSET 0,0,0,4,4:SPSCALE 0,1.5,1.5:SPOFS 0,100,100",
    );
    let fb = m.render();
    let row: Vec<u32> = (100..106).map(|x| fb.get(x, 101).unwrap()).collect();
    assert_eq!(row, [RED, RED, BLUE, RED, RED, BLUE]);
}

#[test]
fn projected_sprites_keep_what_is_off_the_screen() {
    // The layer's origin goes to the middle of the screen, so a sprite at
    // negative coordinates still shows, left of and above it.
    let mut m = run(&format!(
        "{RED_SPRITE}SPSET 0,0,0,16,16:SPOFS 0,-20,-20\n\
         DIM M#[16]:M#[0]=1/200:M#[5]=-1/120:M#[10]=1:M#[15]=1:LMATRIX 0,M#"
    ));
    let fb = m.render();
    assert_eq!(fb.get(185, 105), Some(RED));
    assert_eq!(fb.get(205, 105), Some(0xFF00_0000));
    assert_eq!(fb.get(185, 125), Some(0xFF00_0000));
}

#[test]
fn projected_text_screens_keep_what_is_off_the_screen() {
    // As above, a text screen moved off the top left still shows.
    let (x, y) = lit_pixel_of_a();
    let mut m = run("CLS:PRINT \"A\":TOFS 4,-200,-120\n\
         DIM M#[16]:M#[0]=1/200:M#[5]=-1/120:M#[10]=1:M#[15]=1:LMATRIX 0,M#");
    assert_eq!(m.render().get(x, y), Some(WHITE));
}

/// Stands for a rasterizer outside the runtime.
#[derive(Default)]
struct Backend {
    takes: bool,
    compares: bool,
    drawn: u32,
    compared: Vec<Option<u32>>,
}

impl SceneBackend for Backend {
    fn draw(&mut self, _: &Scene, _: &RasterInput, _: (u32, u32)) -> bool {
        self.drawn += 1;
        self.takes
    }
    fn compares(&self) -> bool {
        self.compares
    }
    fn compare(&mut self, cpu: &Framebuffer) {
        self.compared.push(cpu.get(4, 4));
    }
}

/// A program still running with a red BACKCOLOR: in direct mode, the emulator's
/// own hints go over the frame on the CPU, so it is not offered to a backend.
fn running_red() -> Machine {
    let mut m = Machine::new(Runtime::for_tests(Font::fallback()));
    m.boot();
    m.set_slot(0, "BACKCOLOR &HFFFF0000:WHILE 1:VSYNC:WEND".to_string());
    m.run_slot(0);
    for _ in 0..5 {
        m.frame();
    }
    assert!(m.is_running());
    m
}

#[test]
fn a_backend_that_takes_a_scene_leaves_the_cpu_frame_alone() {
    let mut m = running_red();
    let mut backend = Backend {
        takes: true,
        ..Backend::default()
    };
    m.rt.touch();
    m.rt.render_with(Some(&mut backend));
    assert_eq!(backend.drawn, 1);
    assert!(m.rt.frame_on_backend());
    assert_ne!(m.rt.screen_frame().get(4, 4), Some(RED));
    // Nothing changed: not drawn again, and still the backend's.
    let revision = m.rt.revision();
    m.rt.render_with(Some(&mut backend));
    assert_eq!((backend.drawn, m.rt.revision()), (1, revision));
    assert!(m.rt.frame_on_backend());
}

#[test]
fn asking_for_the_frame_without_a_backend_draws_it_on_the_cpu() {
    let mut m = running_red();
    let mut backend = Backend {
        takes: true,
        ..Backend::default()
    };
    m.rt.touch();
    m.rt.render_with(Some(&mut backend));
    assert_eq!(m.rt.render().get(4, 4), Some(RED));
    assert!(!m.rt.frame_on_backend());
}

#[test]
fn a_backend_that_refuses_a_scene_leaves_it_to_the_cpu() {
    let mut m = running_red();
    let mut backend = Backend::default();
    m.rt.touch();
    m.rt.render_with(Some(&mut backend));
    assert_eq!(backend.drawn, 1);
    assert!(!m.rt.frame_on_backend());
    assert_eq!(m.rt.screen_frame().get(4, 4), Some(RED));
}

#[test]
fn a_comparing_backend_sees_the_cpu_frame_of_its_scene() {
    let mut m = running_red();
    let mut backend = Backend {
        takes: true,
        compares: true,
        ..Backend::default()
    };
    m.rt.touch();
    m.rt.render_with(Some(&mut backend));
    assert_eq!(backend.compared, [Some(RED)]);
    assert!(m.rt.frame_on_backend());
}

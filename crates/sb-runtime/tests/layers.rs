//! Text screens (placement, fonts, user-defined characters), display
//! order and layers.

use sb_runtime::Runtime;
use sb_runtime::font::{Font, FontSize};
use sb_runtime::machine::Machine;

const RED: u32 = 0xFFFF_0000;
const BLUE: u32 = 0xFF00_00FF;
const WHITE: u32 = 0xFFFF_FFFF;

/// Runs `source` with the bundled font for a few frames and returns the
/// machine, so the frame can be rendered.
fn run(source: &str) -> Machine {
    let mut m = Machine::new(Runtime::new(400, 240, Font::fallback()));
    m.boot();
    m.set_slot(0, source.to_string());
    m.run_slot(0);
    for _ in 0..5 {
        m.frame();
    }
    assert!(!m.is_running(), "program did not end");
    m
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
    let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
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
    let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
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
            "TSCREEN 16:TSCREEN 1,8,16,10,5:TSCREEN 2,32,0,3\n\
             TSCREEN OUT A,B,C,D:TSCREEN 1 OUT E,F,G,H:TSCREEN 2 OUT I,J,K,L\n\
             ATTR 1,1,3:ATTR OUT P,Q,R:ATTR 2,6:TPAGE 2,-1,8,9:TPAGE 2 OUT S,T,U\n\
             ?A;B;C;D;\" \";E;F;G;H;\" \";I;J;K;L;\" \";ATTR();P;Q;R;ATTR(2);S;T;U"
        ),
        "16162515 816105 3232123 151136-189"
    );
    assert!(first_row("CLS:TSCREEN 12").starts_with("Out of range"));
    assert!(first_row("CLS:TSCREEN 1,8,8,300,300").starts_with("Out of range"));
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
    assert!(first_row("CLS:XSCREEN 130,240").starts_with("Out of range"));
    assert!(first_row("CLS:XSCREEN 400,240,4").starts_with("Out of range"));
    assert!(first_row("CLS:XSCREEN 400,240,1,3").starts_with("Out of range"));
    assert!(first_row("CLS:XSCREEN 400,240,1,0,20").starts_with("Out of range"));
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
        let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
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
    let mut m = Machine::new(Runtime::new(400, 240, Font::fallback()));
    m.boot();
    m.set_slot(
        0,
        "FADE &HFFFFFFFF,4:A=FADECHK():VSYNC 2:B=FADE():VSYNC 3:?A;HEX$(B);FADECHK();HEX$(FADE())"
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
        "{sprite}LFILTER 1,1,32:IF LFILTER(1)!=1 THEN STOP"
    ));
    assert_eq!(m.render().get(5, 20), Some(0xFF40_0040));
    let mut m = run(&format!(
        "{sprite}LFILTER 1,1,32:LFILTER 1:IF LFILTER(1) THEN STOP"
    ));
    assert_eq!(m.render().get(0, 0), Some(RED));
    let mut m = run(&format!(
        "{sprite}DIM R#[240,2]:FOR I=0 TO 239:R#[I,0]=0.1:R#[I,1]=1:NEXT\nLFILTER 1,3,R#"
    ));
    let fb = m.render();
    assert_eq!(
        (fb.get(0, 0), fb.get(40, 0)),
        (Some(0xFF00_0000), Some(RED))
    );
    let mut m = run(&format!("{sprite}LFILTER 1,5,120,0,0"));
    assert_eq!(m.render().get(0, 0), Some(0xFF00_FF00));
}

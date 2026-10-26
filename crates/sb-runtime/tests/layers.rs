//! Text screen placement, display order and layers.

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

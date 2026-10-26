//! Sprite commands run from programs.

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
fn set_move_and_read_back() {
    let (m, out) = run("SPSET 5,10\n\
         SPOFS 5,100,50,-3\n\
         SPHOME 5,8,8:SPROT 5,45:SPSCALE 5,2,1.5:SPCOLOR 5,#C_RED\n\
         SPOFS 5 OUT X,Y,Z:SPHOME 5 OUT HX,HY:SPSCALE 5 OUT SX,SY\n\
         ?X;\",\";Y;\",\";Z;\" \";HX;HY;\" \";SPROT(5);\" \";SX;SY;\" \";HEX$(SPCOLOR(5));\" \";SPCHR(5);SPUSED(5);SPUSED(6)");
    assert_eq!(out, "100,50,-3 88 45 21.5 FFFF0000 1010");
    let s = &m.rt.sprites.list[5];
    assert_eq!((s.u, s.v, s.w, s.h), (160, 0, 16, 16), "the grid stand-in");
}

#[test]
fn image_forms_show_hide_and_clear() {
    let (m, out) = run("SPSET 1,32,48,24,8,4,0\n\
         SPCHR 1 OUT U,V,W,H,A\n\
         ?U;V;W;H;A;SPSHOW(1);SPHIDE(1);\" \";\n\
         SPSHOW 1:SPCHR 1,,,32:SPCHR 1 OUT U,V,W,H\n\
         ?U;V;W;H;SPSHOW(1);\" \";\n\
         ID=SPSET(100,98,7):?ID;SPSET(3);\" \";\n\
         SPCLR 2,99:?SPUSED(1);SPUSED(100);\n\
         SPCLR:?SPUSED(4095)");
    assert_eq!(out, "3248248401 324832161 1000 111");
    assert!(!m.rt.sprites.list[0].used);
}

#[test]
fn definitions() {
    let (_, out) = run("SPDEF 20,64,96,32,16,8,4,2\n\
         SPDEF 20 OUT U,V,W,H,X,Y,A:?U;V;W;H;X;Y;A;\" \";\n\
         SPDEF 21,20,,,,,1,,\n\
         SPDEF 21 OUT U,V,W,H,X,Y,A:?U;V;W;H;X;Y;A;\" \";\n\
         DIM D[14]:D[0]=1:D[7]=2\n\
         SPDEF D,30,100,0\n\
         SPDEF 31 OUT U,V:?U;V;\" \";\n\
         SPSET 0,20:SPHOME 0 OUT HX,HY:?HX;HY;\" \";\n\
         SPDEF:SPDEF 20 OUT U,V,W,H:?U;V;W;H");
    assert_eq!(out, "64963216842 64963216142 1020 84 32001616");
}

#[test]
fn variables() {
    let (_, out) = run("SPSET 0,0:SPVAR 0,7,1.5:SPVAR 0,\"NAME\",\"HI\"\n\
         ?SPVAR(0,7);SPVAR(0,\"NAME\");SPVAR(0,3);\n\
         SPVAR 0,7:?SPVAR(0,7);\n\
         SPSET 0,0:?SPVAR(0,\"NAME\")");
    assert_eq!(out, "1.5HI000");
}

#[test]
fn errors() {
    for (source, error) in [
        ("SPOFS 3,0,0", "Illegal function call"),
        ("SPSET 4096,0", "Out of range"),
        ("SPSET 0,8192", "Out of range"),
        ("SPSET 0,0:SPOFS 0,0,0,5000", "Out of range"),
        ("SPPAGE 6", "Out of range"),
        ("DIM D[6]:SPDEF D", "Illegal function call"),
    ] {
        let (_, out) = run(source);
        assert!(out.starts_with(error), "{source}: {out}");
    }
}

#[test]
fn sprites_are_drawn_by_z_around_text() {
    let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
    m.boot();
    m.set_slot(
        0,
        "GTARGET 4:GFILL 0,0,15,15,#C_RED:GFILL 16,0,31,15,#C_BLUE\n\
         SPSET 0,0,0,16,16:SPOFS 0,0,0,1\n\
         SPSET 1,16,0,16,16:SPOFS 1,8,0,0\n\
         GTARGET 0:GCLS #C_LIME"
            .to_string(),
    );
    m.run_slot(0);
    for _ in 0..5 {
        m.frame();
    }
    let fb = m.render();
    assert_eq!(fb.get(2, 2), Some(0xFFFF_0000), "sprite 0 over GRP0");
    assert_eq!(fb.get(10, 2), Some(0xFF00_00FF), "sprite 1 is nearer");
    assert_eq!(fb.get(30, 30), Some(0xFF00_FF00), "GRP0 through #GSPRITE");
}

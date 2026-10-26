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
        ("SPOFS 3,0,0", "Sprite not set"),
        ("SPSET 4096,0", "Out of range"),
        ("SPSET 0,8192", "Out of range"),
        ("SPSET 0,0:SPOFS 0,0,0,5000", "Out of range"),
        ("SPPAGE 6", "Out of range"),
        ("DIM D[6]:SPDEF D", "Illegal function call"),
        (
            "SPSET 0,0:SPSET 1,0:SPLINK 1,0:SPLINK 0,1",
            "Illegal function call",
        ),
        ("SPSET 0,0:SPLINK 0,0", "Illegal function call"),
        ("SPSET 0,0:SPANIM 0,\"XY\",1,2", "Illegal function call"),
        ("SPSET 0,0:SPANIM 0,\"Q\",1,2", "Out of range"),
        ("SPSET 0,0:SPANIM 0,5", "Illegal function call"),
        ("SPSET 0,0:SPANIM 0,\"Z\",\"@NOPE\"", "Undefined label"),
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

#[test]
fn animations_slide_and_end() {
    // Each frame after SPANIM plays one step; VSYNC 2 lets two pass.
    let (_, out) = run("SPSET 0,0:SPOFS 0,0,0\n\
         DIM A[3]:A[0]=-4:A[1]=40:A[2]=20\n\
         SPANIM 0,\"XY\",A\n\
         VSYNC 2:SPOFS 0 OUT X,Y:?X;\",\";Y;\" \";SPCHK(0);\" \";\n\
         VSYNC 2:SPOFS 0 OUT X,Y:?X;\",\";Y;\" \";SPCHK(0)");
    assert_eq!(out, "20,10 1 40,20 0");
}

#[test]
fn animations_from_data_can_clear_the_sprite() {
    let (_, out) = run("SPSET 0,0:SPOFS 0,100,0\n\
         SPANIM 0,\"XY+.\",\"@MV\"\n\
         VSYNC 1:SPOFS 0 OUT X,Y:?X;\" \";\n\
         VSYNC 2:?SPUSED(0)\n\
         END\n\
         @MV\n\
         DATA 2\n\
         DATA 1,5,0\n\
         DATA -1,10,0");
    assert_eq!(out, "105 0");
}

#[test]
fn animation_definitions_stop_and_start() {
    let (_, out) = run("ANIMDEF 3,\"Z\",1,1,1,2,0\n\
         SPSET 0,0:SPANIM 0,3\n\
         VSYNC 1:SPOFS 0 OUT X,Y,Z:?Z;\" \";\n\
         SPSTOP 0:VSYNC 2:SPOFS 0 OUT X,Y,Z:?Z;\" \";\n\
         SPSTART:VSYNC 1:SPOFS 0 OUT X,Y,Z:?Z;\" \";SPCHK(0);\" \";\n\
         SPANIM 0:?SPCHK(0)");
    assert_eq!(out, "1 1 2 2 0");
}

#[test]
fn spdef_from_data() {
    let (_, out) = run("SPDEF \"@D\",10,5,6\n\
         SPDEF 10 OUT U,V,W,H,X,Y,A:?U;\",\";V;\",\";W;\",\";H;\",\";X;\",\";Y;\",\";A\n\
         @D\n\
         DATA 1, 1,2,3,4,5,6,7");
    assert_eq!(out, "6,8,3,4,5,6,7");
}

#[test]
fn sprites_hit_each_other() {
    let (_, out) = run("FOR I=0 TO 3:SPSET I,0:SPCOL I:NEXT\n\
         SPOFS 0,0,0:SPOFS 1,8,8:SPOFS 2,100,0:SPOFS 3,4,4\n\
         H=SPHITSP(0):?H;:H=SPHITSP():?H;:H=SPHITSP():?H;\" \";\n\
         ?SPHITSP(0,2);SPHITSP(0,3);\" \";\n\
         SPCOL 3,,2:SPCOL 0,,1:?SPHITSP(0,3);\" \";\n\
         SPCOL 1 OUT X,Y,W,H,SC,M:?X;Y;W;H;SC;HEX$(M)");
    assert_eq!(out, "13-1 01 0 0016160FFFFFFFF");
}

#[test]
fn moving_rectangles_hit_sprites() {
    let (_, out) = run("SPSET 5,0:SPOFS 5,100,0:SPCOL 5\n\
         H=SPHITRC(0,0,10,10,,200,0)\n\
         SPHITINFO OUT T,X1,Y1,X2,Y2\n\
         ?H;\" \";T;\" \";X1;\",\";X2;\" \";SPHITRC(5,0,0,10,10);SPHITRC(0,4,0,0,10,10,,200,0)");
    assert_eq!(out, "5 0.45 90,100 0-1");
}

#[test]
fn sliding_sprites_collide_with_their_motion() {
    let (_, out) = run("SPSET 0,0:SPOFS 0,0,0:SPCOL 0:SPANIM 0,\"XY\",-10,100,0\n\
         SPSET 1,0:SPOFS 1,30,0:SPCOL 1\n\
         VSYNC 1\n\
         ?SPHITSP(0,1);\" \";\n\
         SPHITINFO OUT T,X1,Y1,VX1,VY1,X2,Y2,VX2,VY2:?T;\" \";X1;\" \";VX1;\" \";\n\
         SPCOLVEC 0,0,0:?SPHITSP(0,1)");
    assert_eq!(out, "1 0.4 14 10 0");
}

#[test]
fn linked_sprites_follow_their_parent() {
    let (_, out) = run("FOR I=0 TO 3:SPSET I,0:NEXT\n\
         SPOFS 0,100,50,10:SPROT 0,90:SPSCALE 0,2,2\n\
         SPOFS 1,10,0,-2:SPLINK 1,0\n\
         SPOFS 2,10,0:SPLINK 2,0,6\n\
         SPOFS 3,5,5:SPLINK 3,1,1\n\
         SPOFS 1,1 OUT X,Y,Z:?X;\",\";Y;\",\";Z;\" \";\n\
         SPOFS 2,1 OUT X,Y:?ROUND(X);\",\";ROUND(Y);\" \";\n\
         SPOFS 3,1 OUT X,Y:SPOFS 3 OUT RX,RY:?X;\",\";Y;\",\";RX;\" \";\n\
         ?SPLINK(1);SPLINK(0,1);SPLINK(1,2);SPLINK(3,3);SPLINK(2,2)");
    // Offsets turn and scale with the parent whatever the flags; the flags
    // pass on the parent's own rotation and scale to the child's image.
    assert_eq!(out, "100,70,8 100,70 105,75,5 0121-1");
}

#[test]
fn clearing_a_parent_clears_or_frees_its_children() {
    let (_, out) = run("SPSET 0,0:SPSET 1,0:SPSET 2,0:SPLINK 1,0,1:SPLINK 2,0\n\
         SPCLR 0:?SPUSED(1);SPLINK(2);\" \";:VSYNC 1:?SPUSED(1);SPUSED(2)");
    assert_eq!(out, "1-1 01");
}

#[test]
fn hidden_parents_hide_children_that_follow_them() {
    let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
    m.boot();
    m.set_slot(
        0,
        "SPSET 0,0:SPSET 1,0:SPSET 2,0:SPLINK 1,0,1:SPLINK 2,0:SPHIDE 0".to_string(),
    );
    m.run_slot(0);
    m.frame();
    let shown: Vec<usize> = m.rt.sprites.draw_order().iter().map(|&(i, _)| i).collect();
    assert_eq!(shown, [4095, 2]);
}

#[test]
fn call_sprite_runs_each_callback() {
    let (_, out) = run("SPSET 3,0:SPSET 1,0:SPSET 2,0\n\
         SPFUNC 3,\"@CB\":SPFUNC 1,\"MOVE\":SPFUNC 2,\"@CB\":SPFUNC 2\n\
         CALL SPRITE\n\
         ?\" \";CALLIDX()\n\
         END\n\
         @CB\n\
         ?CALLIDX();\n\
         RETURN\n\
         DEF MOVE\n\
         ?\"M\";CALLIDX();\n\
         END");
    assert_eq!(out, "M13 4096");
}

#[test]
fn label_data_moves_reads_place() {
    let (_, out) = run("RESTORE @A:SPSET 0,0:SPANIM 0,\"XY\",\"@B\":READ Q\n\
         SPDEF \"@C\":READ R\n\
         ?Q;R\n\
         END\n\
         @A:DATA 111\n\
         @B:DATA 1,1,5,5\n\
         DATA 222\n\
         @C:DATA 1,0,0,16,16,0,0,0\n\
         DATA 333");
    assert_eq!(out, "222333");
}

#[test]
fn relative_colors_multiply() {
    let (_, out) = run(
        "SPSET 0,0:SPCOLOR 0,&H80808080:SPANIM 0,\"C+\",1,&H10101010\n\
         VSYNC 1:?HEX$(SPCOLOR(0))",
    );
    assert_eq!(out, "8080808");
}

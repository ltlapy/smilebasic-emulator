//! Sprite commands run from programs.

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
fn set_move_and_read_back() {
    let (m, out) = run("SPSET 5,10\n\
         SPOFS 5,100,50,-3\n\
         SPHOME 5,8,8:SPROT 5,45:SPSCALE 5,2,1.5:SPCOLOR 5,#C_RED\n\
         SPOFS 5 OUT X,Y,Z:SPHOME 5 OUT HX,HY:SPSCALE 5 OUT SX,SY\n\
         ?X;\",\";Y;\",\";Z;\" \";HX;HY;\" \";SPROT(5);\" \";SX;SY;\" \";HEX$(SPCOLOR(5));\" \";SPCHR(5);SPUSED(5);SPUSED(6)");
    assert_eq!(out, "100,50,-3 88 45 21.5 FFFF0000 1010");
    let s = &m.rt.sprites.list()[5];
    assert_eq!((s.u, s.v, s.w, s.h), (160, 0, 16, 16), "the grid stand-in");
}

#[test]
fn image_forms_show_hide_and_clear() {
    let (m, out) = run("SPSET 1,32,48,24,8,4,0\n\
         SPCHR 1 OUT U,V,W,H,A\n\
         ?U;V;W;H;A;SPSHOW(1);SPHIDE(1);\" \";\n\
         SPSHOW 1:SPCHR 1,,,3:SPCHR 1 OUT U,V,W,H\n\
         ?U;V;W;H;SPSHOW(1);\" \";\n\
         ID=SPSET(100,98,7):?ID;SPSET(3);\" \";\n\
         SPCLR 2,99:?SPUSED(1);SPUSED(100);\n\
         SPCLR:?SPUSED(4095)");
    assert_eq!(out, "3248248401 324816161 1000 111");
    assert!(!m.rt.sprites.list()[0].used);
}

#[test]
fn image_forms_by_argument_count() {
    // From SPSET 1,32,48,24,32,1 (turned 90 degrees), each form reads
    // back U,V,W,H,A as SB4 gave them.
    let forms = [
        ("1,,,5", "32 48 16 16 5"),
        ("1,8,8,4", "8 8 16 16 4"),
        ("1,0,0", "0 0 16 16 0"),
        ("1,,", "32 48 16 16 0"),
        ("1,,,40,50", "32 48 40 50 0"),
        ("1,,,40,", "32 48 40 24 0"),
        ("1,,,,40", "32 48 32 40 0"),
        ("1,8,8,40,50,5", "8 8 50 40 5"),
        ("1,,,40,50,", "32 48 50 40 1"),
        ("1,,,40,,", "32 48 24 40 1"),
        ("1,,,40,,5", "32 48 24 40 5"),
        ("1,,,,40,5", "32 48 40 32 5"),
        ("1,,,,,", "32 48 24 32 1"),
        ("1,,,,,9", "32 48 24 32 9"),
    ];
    for (form, want) in forms {
        let (_, out) = run(&format!(
            "SPSET 1,32,48,24,32,1:SPCHR {form}:SPCHR 1 OUT U,V,W,H,A\n\
             ?U;\" \";V;\" \";W;\" \";H;\" \";A"
        ));
        assert_eq!(out, want, "SPCHR {form}");
    }
    // Unturned, the six-argument form keeps W and H in order.
    for (form, want) in [("0,0,64,16,0", "64 16"), ("0,0,16,64,0", "16 64")] {
        let (_, out) = run(&format!(
            "SPSET 1,0,0,16,16,0:SPCHR 1,{form}:SPCHR 1 OUT U,V,W,H:?W;\" \";H"
        ));
        assert_eq!(out, want, "SPCHR 1,{form}");
    }
    let (_, out) = run("SPSET 1,0:SPCHR 1,,,40");
    assert!(out.contains("Out of range"), "{out}");
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
fn variables_of_unset_sprites() {
    let (_, out) = run("SPVAR 5,\"A\",3:?SPVAR(5,\"A\");SPVAR(6,\"A\");\n\
         SPVAR 5:?SPVAR(5,\"A\");\n\
         SPVAR 5,\"A\",4:SPCLR 5:?SPVAR(5,\"A\")");
    assert_eq!(out, "3000");
}

#[test]
fn what_sets_or_needs_no_sprite() {
    // An animation number never defined starts nothing; a sprite that is
    // not set hits nothing and still takes SPFUNC.
    let (_, out) = run(
        "SPSET 0,0:SPANIM 0,5:SPCOL 0:SPCLR 1:SPFUNC 1,\"@X\":?SPCHK(0);SPHITSP(1);SPHITSP(0,1)\n@X",
    );
    assert_eq!(out, "0-10");
    // Setting what an animation changes stops it.
    let (_, out) = run("SPSET 0,0:SPANIM 0,\"C \",-4,-1,240,-1:SPCOLOR 0,0:?SPCHK(0)");
    assert_eq!(out, "0");
}

#[test]
fn functions_called_with_one_out() {
    let (_, out) = run("SPSET 0,5:SPCHR 0 OUT D:SPSET 242 OUT I:?D;I");
    assert_eq!(out, "51");
}

#[test]
fn errors() {
    for (source, error) in [
        ("SPOFS 3,0,0", "Sprite not set"),
        ("SPSET 4096,0", "Out of range"),
        ("SPSET 0,8192", "Out of range"),
        ("SPSET 0,0:SPOFS 0,0,0,5000", "Out of range"),
        ("SPPAGE 6", "Out of range"),
        ("DIM D[6]:SPDEF D", "Illegal array size in 0:1(SPDEF:1)"),
        (
            "SPSET 0,0:SPSET 1,0:SPLINK 1,0:SPLINK 0,1",
            "Can't link to decendent sprite in 0:1(SPLINK:2)",
        ),
        (
            "SPSET 0,0:SPLINK 0,0",
            "Illegal function call in 0:1(SPLINK:2)",
        ),
        ("SPLINK 0,1", "Sprite not set in 0:1(SPLINK:1)"),
        ("SPSET 0,0:SPLINK 0,1,32", "Out of range in 0:1(SPLINK:3)"),
        ("SPSET 0,0:SPOFS 0,0,0,5000", "Out of range in 0:1(SPOFS:4)"),
        ("SPOFS 4096,\"X\"", "Out of range in 0:1(SPOFS:1)"),
        ("SPSET 1,0:SPCHR 1,,,40", "Out of range in 0:1(SPCHR:4)"),
        (
            "SPSET 1,0:SPCHR 1,0,0,16,16,32",
            "Out of range in 0:1(SPCHR:6)",
        ),
        (
            "SPSET 1,0:SPCHR 1,0,0,2049,16",
            "Out of range in 0:1(SPCHR:4)",
        ),
        (
            "SPDEF 0,0,0,0,2049,16,0,0,1",
            "Out of range in 0:1(SPDEF:5)",
        ),
        ("SPDEF 0,0,0,0,16,16,0,0,32", "Out of range in 0:1(SPDEF:9)"),
        ("SPDEF 0,0,2040,0,16,,,,", "Out of range in 0:1(SPDEF)"),
        ("SPDEF 0,0,0,2049,16", "Out of range in 0:1(SPDEF:4)"),
        ("SPDEF 0,0,0,16,16,32768,0", "Out of range in 0:1(SPDEF:6)"),
        ("SPDEF 0,0,0,16,16,0,0,32", "Out of range in 0:1(SPDEF:8)"),
        ("SPDEF 0,-1,0", "Out of range in 0:1(SPDEF:2)"),
        ("SPDEF 0 OUT A,B,C,D,E,F,G,H", "Wrong number of"),
        ("DIM D[7]:D[2]=2049:SPDEF D", "Out of range in 0:1(SPDEF)"),
        ("DIM D[7]:D[6]=-1:SPDEF D", "Out of range in 0:1(SPDEF)"),
        ("DIM D[7]:SPDEF D,0,0", "Wrong number of arguments"),
        ("SPDEF \"@D\"\n@D\nDATA 8193", "Out of range in 0:1(SPDEF)"),
        ("SPFUNC 0,0", "Type mismatch in 0:1(SPFUNC:2)"),
        ("SPFUNC 4096,0", "Out of range in 0:1(SPFUNC:1)"),
        ("TFUNC 0,0", "Type mismatch in 0:1(TFUNC:2)"),
        // The name must be a DEF, or else a label, as it is set.
        (
            "SPFUNC 0,\"SPSET\"",
            "Function must be user defined in 0:1(SPFUNC:2)",
        ),
        (
            "TFUNC 0,\"ABS\"",
            "Function must be user defined in 0:1(TFUNC:2)",
        ),
        ("SPFUNC 0,\"NOPE\"", "Illegal label string in 0:1(SPFUNC)"),
        ("SPFUNC 0,\"@\"", "Illegal label string in 0:1(SPFUNC)"),
        ("TFUNC 0,\"3:F\"", "Illegal label string in 0:1(TFUNC)"),
        ("SPFUNC 0,\"@NOPE\"", "Undefined label in 0:1(SPFUNC:2)"),
        ("SPFUNC 4096,\"NOPE\"", "Out of range in 0:1(SPFUNC:1)"),
        ("SPSET 0,0,0,16,16,32", "Out of range in 0:1(SPSET:6)"),
        ("SPSET 0,0,0,16,16,-1,1", "Out of range in 0:1(SPSET:6)"),
        ("Q=SPSET(0,0,16,16,32)", "Out of range in 0:1(SPSET:5)"),
        ("Q=SPSET(0,0,16,16,32,1)", "Out of range in 0:1(SPSET:5)"),
        ("Q=SPSET(5,9,0,0,16,16,32)", "Out of range in 0:1(SPSET:7)"),
        (
            "Q=SPSET(5,9,0,0,16,16,-1,1)",
            "Out of range in 0:1(SPSET:7)",
        ),
        // With no free sprite in the range, too.
        (
            "SPSET 0,0:Q=SPSET(0,0,0,0,-1,16,1,0)",
            "Out of range in 0:1(SPSET:5)",
        ),
        (
            "SPSET 0,0:Q=SPSET(0,0,0,0,16,2049,1,0)",
            "Out of range in 0:1(SPSET:6)",
        ),
        (
            "SPSET 0,0:Q=SPSET(0,0,0,0,16,16,32,0)",
            "Out of range in 0:1(SPSET:7)",
        ),
        (
            "SPSET 0,0:Q=SPSET(0,0,8192)",
            "Out of range in 0:1(SPSET:3)",
        ),
        ("SPSET 0,0:SPCOL 0,0,0,16", "Wrong number of arguments"),
        (
            "SPSET 0,0:SPCOL 0,0,0,16,16,1,0,0",
            "Wrong number of arguments",
        ),
        ("SPSET 0,0:SPANIM 0,\"XY\",1,2", "Illegal function call"),
        ("SPSET 0,0:SPANIM 0,\"Q\",1,2", "Out of range"),
        ("SPSET 0,0:SPANIM 0,\"Z\",\"@NOPE\"", "Undefined label"),
        (
            "SPSET 0,0:DIM A[4]:SPANIM 0,A,1,2",
            "Animation type mismatch in 0:1(SPANIM:2)",
        ),
        (
            "DIM K[4]:SPANIM 0,\"XY\",K",
            "Sprite not set in 0:1(SPANIM:1)",
        ),
        ("SPANIM 0,1024", "Sprite not set in 0:1(SPANIM:1)"),
        (
            "DIM A[2]:TANIM 0,A,1,2",
            "Animation type mismatch in 0:1(TANIM:2)",
        ),
        (
            "DIM K[66]:SPSET 0,0:SPANIM 0,\"Z\",K",
            "Animation is too long in 0:1(SPANIM)",
        ),
        (
            "ANIMDEF 0,\"Z\",\"@K\"\n@K\nDATA 33",
            "Animation is too long in 0:1(ANIMDEF)",
        ),
        (
            "ANIMDEF 0,\"Z\",\"@K\"\n@K\nDATA 0",
            "Illegal animation data in 0:1(ANIMDEF)",
        ),
    ] {
        let (_, out) = run(source);
        assert!(out.starts_with(error), "{source}: {out}");
    }
}

/// SPDEF from an array or DATA sets the templates up to the last and
/// leaves the rest out, and a bad template stops it after those before.
#[test]
fn spdef_from_a_list_stops_at_the_last_template() {
    let (_, out) = run("DIM D[14]:D[0]=5:D[2]=16:D[3]=16:D[9]=16:D[10]=16\n\
         SPDEF D,8191:SPDEF 8191 OUT U,V,W,H:?U;W");
    assert_eq!(out, "516");
    let (_, out) = run(
        "SPDEF \"@D\",8191:SPDEF 8191 OUT U,V:?U;V;:SPDEF \"@N\":?\"N\"\n\
         @D\nDATA 2,5,6,16,16,0,0,1,7,8,16,16,0,0,1\n@N\nDATA -1",
    );
    assert_eq!(out, "56N");
    let (_, out) = run("DIM D[14]:D[0]=3:D[2]=16:D[3]=16:D[9]=4096\n\
         SPDEF D,9 OUT X\n?X");
    assert!(out.starts_with("Wrong number of"), "{out}");
    // The second template's width is out of range: the first is set.
    let (m, out) = run("DIM D[14]:D[0]=3:D[2]=16:D[3]=16:D[9]=4096:SPDEF D,9");
    assert!(out.starts_with("Out of range"), "{out}");
    assert_eq!((m.rt.sprites.defs[9].u, m.rt.sprites.defs[9].w), (3, 16));
}

/// A DEF, a label in the program or in a DEF, and another slot's
/// COMMON DEF may be set.
#[test]
fn callbacks_name_what_the_program_has() {
    let (_, out) = run("SPFUNC 0,\"F\":SPFUNC 1,\"@L\":TFUNC 0,\"@M\":?\"OK\"\n\
         @L\nDEF F\n@M\nEND");
    assert_eq!(out, "OK");
}

#[test]
fn sprites_are_drawn_by_z_around_text() {
    let mut m = Machine::new(Runtime::for_tests(Font::blank()));
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
    let (_, out) = run("SPSET 0,0:SPOFS 0,0,0:VSYNC 1\n\
         DIM A[3]:A[0]=-4:A[1]=40:A[2]=20\n\
         SPANIM 0,\"XY\",A\n\
         VSYNC 2:SPOFS 0 OUT X,Y:?X;\",\";Y;\" \";SPCHK(0);\" \";\n\
         VSYNC 2:SPOFS 0 OUT X,Y:?X;\",\";Y;\" \";SPCHK(0)");
    assert_eq!(out, "20,10 1 40,20 0");
}

/// TEST48 part 7: set in the frame of SPSET, an animation's first step
/// shows a frame late; set a frame later, after one VSYNC. Rotation and
/// scale are single precision, set or slid.
#[test]
fn animations_wait_a_frame_after_spset() {
    let (_, out) = run("SPSET 0,0:SPANIM 0,\"R\",-3,-10\n\
         VSYNC 1:?SPROT(0);\" \";:VSYNC 1:?SPROT(0);\" \";\n\
         SPSET 1,0:VSYNC 1:SPANIM 1,\"S\",-3,3,-0.5\n\
         VSYNC 1:SPSCALE 1 OUT X,Y:?X;\",\";Y;\" \";\n\
         SPROT 1,-3.3:?SPROT(1)");
    assert_eq!(out, "0 -3.33333302 1.66666663,0.5 -3.29999995");
}

/// TEST60: XY, Z, UV and I are kept in 16 bits and refuse more.
#[test]
fn animation_values_outside_16_bits_are_out_of_range() {
    let refused = |target: &str, value: i32| {
        let (m, _) = run(&format!(
            "SPSET 0,0:DIM K[3]:K[0]=1:K[1]={value}:K[2]=0:DIM L[2]:L[0]=1:L[1]={value}\n\
             IF LEN(\"{target}\")==2 THEN SPANIM 0,\"{target}\",K ELSE SPANIM 0,\"{target}\",L\n\
             ?\"FINE\""
        ));
        !(0..10).any(|y| m.rt.text[4].cell(0, y).code == u16::from(b'F'))
    };
    for target in ["XY", "UV", "Z", "I"] {
        assert!(!refused(target, 32767), "{target}");
        assert!(!refused(target, -32768), "{target}");
        assert!(refused(target, 32768), "{target}");
        assert!(refused(target, -32769), "{target}");
    }
    assert!(!refused("R", 40000));
}

#[test]
fn animations_from_data_can_clear_the_sprite() {
    let (_, out) = run("SPSET 0,0:SPOFS 0,100,0:VSYNC 1\n\
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
         SPSET 0,0:VSYNC 1:SPANIM 0,3\n\
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
    let (_, out) = run("SPSET 0,0:SPOFS 0,0,0:SPCOL 0\n\
         SPSET 1,0:SPOFS 1,30,0:SPCOL 1\n\
         VSYNC 1:SPANIM 0,\"XY\",-10,100,0:VSYNC 1\n\
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
    let (_, out) = run(
        "VSYNC:SPSET 0,0:SPSET 1,0:SPSET 2,0:SPLINK 1,0,1:SPLINK 2,0\n\
         SPCLR 0:?SPUSED(1);SPLINK(2);\" \";:VSYNC 1:?SPUSED(1);SPUSED(2)",
    );
    assert_eq!(out, "1-1 01");
}

#[test]
fn hidden_parents_hide_children_that_follow_them() {
    let mut m = Machine::new(Runtime::for_tests(Font::blank()));
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

/// A command reading DATA from `"@L"` looks at the labels of the DEF it is
/// called in first, then the top level's, as RESTORE does (4.4.8).
#[test]
fn label_data_finds_the_labels_of_the_def_running() {
    let (_, out) = run("F:G\n\
         END\n\
         @D:DATA 1,9,9,16,16,0,0,1\n\
         DEF F\n\
         SPDEF \"@D\",10:SPDEF 10 OUT U,V:?U;\n\
         SPSET 0,0:SPANIM 0,\"XY\",\"@A\":READ Q:?Q;\n\
         @D:DATA 1,5,5,16,16,0,0,1\n\
         @A:DATA 1,1,5,5:DATA 7\n\
         END\n\
         DEF G\n\
         SPDEF \"@D\",11:SPDEF 11 OUT U,V:?U;\n\
         SPDEF \"0:@D\",12:SPDEF 12 OUT U,V:?U\n\
         END");
    assert_eq!(out, "5799");
    let (_, out) = run("SPDEF \"@D\"\nDEF F\n@D:DATA 1,5,5,16,16,0,0,1\nEND");
    assert!(out.starts_with("Undefined label"), "{out}");
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
        "SPSET 0,0:SPCOLOR 0,&H80808080:VSYNC 1:SPANIM 0,\"C+\",1,&H10101010\n\
         VSYNC 1:?HEX$(SPCOLOR(0))",
    );
    assert_eq!(out, "8080808");
    // Divided by 256, as TEST12 showed.
    let (_, out) = run(
        "SPSET 0,0:SPCOLOR 0,&HFFFF80C8:VSYNC 1:SPANIM 0,\"C+\",1,&HFF80FF80\n\
         VSYNC 1:?HEX$(SPCOLOR(0))",
    );
    assert_eq!(out, "FE7F7F64");
}

#[test]
fn callbacks_see_sprites_set_and_cleared_by_earlier_ones() {
    let (_, out) = run("FOR I=20 TO 22:SPSET I,0:SPFUNC I,\"@CB\":NEXT\n\
         CALL SPRITE:?\n\
         END\n\
         @CB:?CALLIDX();\" \";\n\
         IF CALLIDX()==20 THEN SPCLR 21:SPSET 23,0:SPFUNC 23,\"@CB\"\n\
         RETURN");
    assert_eq!(out, "20 22 23 ");
}

#[test]
fn corners_for_the_debugger() {
    let (m, _) = run("SPSET 0,0:SPOFS 0,100,50:SPHOME 0,8,8:SPSCALE 0,2,1\n\
         SPSET 1,0,0,16,8,1:SPOFS 1,10,20\n\
         SPSET 2,0:SPOFS 2,10,20:SPROT 2,90\n\
         SPSET 3,0:SPCLR 3:?1");
    assert_eq!(
        m.rt.sprite_corners(0),
        Some([(84.0, 42.0), (116.0, 42.0), (116.0, 58.0), (84.0, 58.0)])
    );
    // A quarter turn swaps the sides.
    assert_eq!(
        m.rt.sprite_corners(1),
        Some([(10.0, 20.0), (18.0, 20.0), (18.0, 36.0), (10.0, 36.0)])
    );
    let round = |c: [(f64, f64); 4]| c.map(|(x, y)| (x.round() as i32, y.round() as i32));
    let turned = m.rt.sprite_corners(2).map(round);
    assert_eq!(turned, Some([(10, 20), (10, 36), (-6, 36), (-6, 20)]));
    assert_eq!(m.rt.sprite_corners(3), None);
    assert_eq!(m.rt.sprite_corners(4096), None);
    // In the frame's finer pixels.
    let (m, _) = run("XSCREEN 400,240,2:SPSET 0,0:SPOFS 0,10,20:?1");
    let corners = m.rt.sprite_corners(0).unwrap();
    assert_eq!((corners[0], corners[2]), ((20.0, 40.0), (52.0, 72.0)));
}

#[test]
fn xscreen_puts_the_graphics_sprite_on_new_sprites_layer() {
    let (_, out) = run("SPLAYER 6:XSCREEN 320,240:?SPLAYER(4095)");
    assert_eq!(out, "6");
    let (_, out) = run("SPLAYER 6:ACLS:?SPLAYER(4095)");
    assert_eq!(out, "0");
}

#[test]
fn spdef_from_an_array_wraps_coordinates() {
    let (_, out) = run("DIM D[7]:D[0]=2147483647:D[1]=2147483647:SPDEF D,30,1,2\n\
         SPDEF 30 OUT U,V:?U;\",\";V");
    assert_eq!(out, "-2147483648,-2147483647");
}

/// The columns (along a row) or rows (along a column) of a page of unique
/// colors that a program's sprite shows, past the first `skip` pixels.
fn texels(setup: &str, along: (u32, u32), step: (u32, u32), n: u32) -> Vec<Option<u32>> {
    let source = format!(
        "GTARGET 3:FOR Y=0 TO 32:FOR X=0 TO 32:GPSET X,Y,RGB(X*7+8,Y*7+8,100):NEXT:NEXT\n\
         GTARGET 0\n{setup}"
    );
    let (mut m, _) = run(&source);
    let fb = m.rt.render();
    (0..n)
        .map(|i| {
            let p = fb.get(along.0 + i * step.0, along.1 + i * step.1)?;
            let (x, y) = ((p >> 16 & 255) as i32 - 8, (p >> 8 & 255) as i32 - 8);
            if p & 255 != 100 {
                return None;
            }
            Some(if step.0 > 0 { x / 7 } else { y / 7 } as u32)
        })
        .collect()
}

fn shown(list: &[u32]) -> Vec<Option<u32>> {
    list.iter().map(|&v| Some(v)).collect()
}

/// TEST63 on a Switch 2: a side and a scale that are powers of two read the
/// page's higher texel, anything else the lower, at any flip.
#[test]
fn scaled_sprites_settle_ties_by_size_and_scale() {
    let sp = "SPSET 0,0,0,16,16:SPPAGE 0,3:SPOFS 0,40,40:SPSCALE 0,1.5,1.5";
    assert_eq!(
        texels(sp, (40, 44), (1, 0), 7),
        shown(&[0, 0, 1, 2, 2, 3, 4])
    );
    assert_eq!(
        texels(sp, (44, 40), (0, 1), 7),
        shown(&[0, 0, 1, 2, 2, 3, 4])
    );
    // At 0.5 the higher texel each way...
    let sp = "SPSET 0,0,0,16,16:SPPAGE 0,3:SPOFS 0,40,40:SPSCALE 0,0.5,0.5";
    assert_eq!(texels(sp, (40, 44), (1, 0), 4), shown(&[1, 3, 5, 7]));
    assert_eq!(texels(sp, (44, 40), (0, 1), 4), shown(&[1, 3, 5, 7]));
    // ...and 15 wide the lower each way.
    let sp = "SPSET 0,0,0,15,15:SPPAGE 0,3:SPOFS 0,40,40:SPSCALE 0,1.5,1.5";
    assert_eq!(texels(sp, (44, 40), (0, 1), 4), shown(&[0, 0, 1, 2]));
    let sp = "SPSET 0,0,0,15,15:SPPAGE 0,3:SPOFS 0,40,40:SPSCALE 0,0.5,0.5";
    assert_eq!(texels(sp, (40, 44), (1, 0), 4), shown(&[0, 2, 4, 6]));
}

#[test]
fn a_flip_keeps_a_tie_on_the_same_side_of_the_page() {
    // 16 flipped at a scale of 1: still the higher texel, so on the frame the
    // texel before the plain one, and 15 the lower.
    let sp = "SPSET 0,0,0,16,16,&H4:SPPAGE 0,3:SPOFS 0,40.5,40.5";
    assert_eq!(texels(sp, (41, 44), (1, 0), 4), shown(&[15, 14, 13, 12]));
    let sp = "SPSET 0,0,0,15,15,&H4:SPPAGE 0,3:SPOFS 0,40.5,40.5";
    assert_eq!(texels(sp, (41, 44), (1, 0), 4), shown(&[13, 12, 11, 10]));
    let sp = "SPSET 0,0,0,15,15:SPPAGE 0,3:SPOFS 0,40.5,40.5";
    assert_eq!(texels(sp, (41, 44), (1, 0), 4), shown(&[0, 1, 2, 3]));
}

/// TEST63: a tie before the page's first texel reads that one.
#[test]
fn a_tie_before_the_page_reads_its_first_texel() {
    let sp = "SPSET 0,0,0,15,15:SPPAGE 0,3:SPOFS 0,40.5,40.5";
    assert_eq!(texels(sp, (40, 44), (1, 0), 2), shown(&[0, 0]));
}

/// TEST62 on a Switch 2: at a scale of 1 a tie goes to the higher texel, and on
/// the image's far edge that is the page's next one.
#[test]
fn a_tie_can_read_the_page_past_the_image() {
    let sp = "SPSET 0,0,0,16,16,1:SPPAGE 0,3:SPROT 0,90:SPOFS 0,100.5,40";
    assert_eq!(texels(sp, (84, 44), (1, 0), 4), shown(&[16, 15, 14, 13]));
}

#[test]
fn half_pixel_sprites_cover_by_left_and_bottom_edges() {
    let sp = "SPSET 0,0,0,16,16:SPPAGE 0,3:SPOFS 0,40.5,40.5";
    let mut across = vec![None];
    across.extend((0..16).map(Some));
    across.push(None);
    // From the pixel the left edge runs through to the last before the
    // right edge's; the last texel shows twice nowhere.
    assert_eq!(
        texels(sp, (39, 44), (1, 0), 17)[1..],
        shown(&(0..16).collect::<Vec<_>>())
    );
    // Not the pixel the top edge runs through, but the bottom edge's, which
    // reads the page past the image.
    let down = texels(sp, (44, 40), (0, 1), 18);
    assert_eq!(down[0], None);
    assert_eq!(down[1..=16], shown(&(1..=16).collect::<Vec<_>>()));
    assert_eq!(down[17], None);
}

#[test]
fn a_search_finds_a_sprite_where_its_animation_and_parent_took_it() {
    let (_, out) = run("SPSET 0,0:SPCOL 0:?SPHITRC(0,0,8,8);\" \";\n\
         SPANIM 0,\"XY\",-1,100,100,1\n\
         VSYNC 1:VSYNC 3\n\
         ?SPHITRC(0,0,8,8);SPHITRC(101,101,8,8);\" \";\n\
         SPSET 1,0:SPCOL 1:SPOFS 1,5,5:SPSET 2,0:SPOFS 2,300,0:SPLINK 1,2\n\
         ?SPHITRC(306,6,4,4);\" \";\n\
         SPOFS 2,0,200\n\
         ?SPHITRC(306,6,4,4);SPHITRC(6,206,4,4)");
    assert_eq!(out, "0 -10 1 -11");
}

/// SPHIDE's flag shows on 0 and hides on any other value; page -1 reads
/// back as -1 for sprites and text screens.
#[test]
fn sphide_flags_and_page_minus_one() {
    let (_, out) = run("SPSET 1,0\n\
         SPHIDE 1,0:A=SPSHOW(1):SPHIDE 1,2:B=SPSHOW(1):SPHIDE 1,0:C=SPSHOW(1)\n\
         SPHIDE 1,-1:D=SPSHOW(1):SPSHOW 1:SPHIDE 1:E=SPSHOW(1)\n\
         SPPAGE 1,-1:SPPAGE -1:TPAGE 2,-1,0,0:TPAGE 2 OUT P,X,Y\n\
         ?A;B;C;D;E;\" \";SPPAGE(1);SPPAGE();P");
    assert_eq!(out, "10100 -1-1-1");
}

//! Commands on what text screens hold, checked through their cells.

use sb_core::error::ErrorKind;
use sb_core::vm::{Status, Vm};
use sb_runtime::Runtime;
use sb_runtime::font::Font;

fn run(source: &str) -> Result<Runtime, sb_core::error::Error> {
    let mut rt = Runtime::new(400, 240, Font::blank());
    let mut vm = Vm::new(sb_core::compile(source, &rt)?);
    while vm.run(&mut rt, 100_000)? != Status::Ended {}
    Ok(rt)
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
        "TPUT 0,1,0,\"AB\":TPUT 0,2,0,67,#A_REVV:TPUT 0,99,0,\"Z\"\n",
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
    assert_eq!(
        kind("DIM A[0]:TSAVE 0,20,0,10,1,A"),
        Some(ErrorKind::OutOfRange)
    );
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

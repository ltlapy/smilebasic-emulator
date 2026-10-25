//! Programs run against the runtime, checked through the console contents.

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

fn row(rt: &Runtime, screen: usize, y: u32) -> String {
    let s = &rt.text[screen];
    let codes: Vec<u16> = (0..s.size().0)
        .map(|x| s.cell(x, y).code)
        .take_while(|&c| c != 0)
        .collect();
    String::from_utf16(&codes).unwrap()
}

#[test]
fn print_locate_color() {
    let rt = run("PRINT \"HI\"\nLOCATE 3,2:COLOR #C_RED:?\"X\";\nLOCATE OUT X,Y\n?X;Y").unwrap();
    assert_eq!(row(&rt, 4, 0), "HI");
    assert_eq!(rt.text[4].cell(3, 2).code, u16::from(b'X'));
    assert_eq!(rt.text[4].cell(3, 2).color, 0xFFFF_0000);
    assert_eq!(row(&rt, 4, 2), "");
    assert_eq!(rt.text[4].cell(4, 2).code, u16::from(b'4'));
}

#[test]
fn omitted_coordinates_and_screens() {
    let rt =
        run("LOCATE 5,7:LOCATE ,1:TPRINT 1,\"A\";:COLOR 1,&HFF00FF00\nC=COLOR(1)\n?C==&HFF00FF00")
            .unwrap();
    assert_eq!(rt.text[4].cell(5, 1).code, u16::from(b'1'));
    assert_eq!(row(&rt, 1, 0), "A");
    let rt = run("?\"A\":CLS:?\"B\"").unwrap();
    assert_eq!(row(&rt, 4, 0), "B");
}

#[test]
fn errors() {
    let kind = |src| run(src).err().map(|e| e.kind);
    assert_eq!(kind("LOCATE 50,0"), Some(ErrorKind::OutOfRange));
    assert_eq!(kind("CLS 5"), Some(ErrorKind::OutOfRange));
    assert_eq!(kind("COLOR 1,2,3"), Some(ErrorKind::IllegalFunctionCall));
    assert_eq!(kind("COLOR \"A\""), Some(ErrorKind::TypeMismatch));
}

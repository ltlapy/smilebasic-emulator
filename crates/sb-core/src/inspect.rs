//! INSPECT (`??`): a value's type and contents, one line each, for
//! direct mode and, later, the debugger's variable views.

use crate::value::{Array, ArrayData, Str, Value, format_number};

/// Characters of a string shown before the rest is left out.
// UNVERIFIED: SmileBASIC 4's limit (more than 300), and how it marks what
// it leaves out.
pub const STRING_CHARS: usize = 1024;

/// The lines INSPECT prints for `value`: `INT: 1`, `REAL: 1.50000000` or
/// `STRING: (2)"AB"`; for an array, `INT[2,2]:`, a line ` [0,1]: 0` for
/// each element and an empty line.
pub fn lines(value: &Value) -> Vec<Str> {
    match value {
        Value::Array(a) => array_lines(&a.borrow()),
        v => vec![scalar(v)],
    }
}

fn array_lines(array: &Array) -> Vec<Str> {
    let kind = match array.data {
        ArrayData::Int(_) => "INT",
        ArrayData::Real(_) => "REAL",
        ArrayData::Str(_) => "STRING",
    };
    let dims: Vec<String> = array.dims.iter().map(usize::to_string).collect();
    let header = format!("{kind}[{}]:", dims.join(","));
    let mut out = vec![header.encode_utf16().collect()];
    for i in 0..array.len() {
        let mut line: Str = format!(" [{}]: ", index_text(&array.dims, i))
            .encode_utf16()
            .collect();
        line.extend(element(&array.get(i)));
        out.push(line);
    }
    out.push(Str::new());
    out
}

/// A flat position as one index per dimension.
fn index_text(dims: &[usize], mut flat: usize) -> String {
    let mut indices = vec![0; dims.len()];
    for (index, &n) in indices.iter_mut().zip(dims).rev() {
        *index = flat % n;
        flat /= n;
    }
    let indices: Vec<String> = indices.iter().map(usize::to_string).collect();
    indices.join(",")
}

fn scalar(value: &Value) -> Str {
    let kind = match value {
        Value::Int(_) => "INT: ",
        Value::Real(_) => "REAL: ",
        Value::Str(_) => "STRING: ",
        Value::Default | Value::Array(_) => "",
    };
    let mut line: Str = kind.encode_utf16().collect();
    line.extend(element(value));
    line
}

/// An array element: a number, or a string's length and its text.
fn element(value: &Value) -> Str {
    let s = match value {
        Value::Real(x) => return format!("{x:.8}").encode_utf16().collect(),
        Value::Str(s) => s,
        v => return format_number(v),
    };
    let s = s.borrow();
    let mut out: Str = format!("({})\"", s.len()).encode_utf16().collect();
    out.extend(s.iter().take(STRING_CHARS));
    out.push(u16::from(b'"'));
    if s.len() > STRING_CHARS {
        out.extend("...".encode_utf16());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{ElemType, str_from};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn text(value: &Value) -> Vec<String> {
        lines(value)
            .iter()
            .map(|l| String::from_utf16_lossy(l))
            .collect()
    }

    #[test]
    fn scalars() {
        assert_eq!(text(&Value::Int(12)), ["INT: 12"]);
        assert_eq!(text(&Value::Real(1.5)), ["REAL: 1.50000000"]);
        assert_eq!(text(&Value::Str(str_from("AB"))), ["STRING: (2)\"AB\""]);
        let long = "1234567890".repeat(110);
        let shown = format!("STRING: (1100)\"{}\"...", &long[..STRING_CHARS]);
        assert_eq!(text(&Value::Str(str_from(&long))), [shown]);
    }

    #[test]
    fn arrays() {
        let mut a = Array::new(ElemType::Int, vec![2, 2]);
        a.set(3, Value::Int(7)).unwrap();
        let a = Value::Array(Rc::new(RefCell::new(a)));
        let expected = [
            "INT[2,2]:",
            " [0,0]: 0",
            " [0,1]: 0",
            " [1,0]: 0",
            " [1,1]: 7",
            "",
        ];
        assert_eq!(text(&a), expected);
        let s = Array::new(ElemType::Str, vec![1]);
        let s = Value::Array(Rc::new(RefCell::new(s)));
        assert_eq!(text(&s), ["STRING[1]:", " [0]: (0)\"\"", ""]);
    }
}

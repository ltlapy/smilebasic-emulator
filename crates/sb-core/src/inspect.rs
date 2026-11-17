//! INSPECT (`??`): a value's type and contents, one line each, for
//! direct mode and, later, the debugger's variable views.

use crate::value::{Array, ArrayData, ArrayRef, Str, Value, format_number};

/// Characters of a string shown; a longer one is cut off there, without
/// its closing quote.
pub const STRING_CHARS: usize = 256;

/// The lines INSPECT prints for `value`: `INT: 1`, `REAL: 1.50000000` or
/// `STRING: (2)"AB"`; for an array, `INT[2,2]:`, a line ` [0,1]: 0` for
/// each element and an empty line.
pub fn lines(value: &Value) -> Vec<Str> {
    Lines::new(value).collect()
}

/// [`lines`] one at a time. An array's lines are made from the array as
/// they are reached, so a page of a huge array costs a page of lines.
///
/// The array is read as each line is made: an element changed after the
/// first line shows its new value, and lines past an array made shorter
/// are left out, the empty line too.
#[derive(Clone, Debug)]
pub struct Lines {
    what: What,
    /// The number of the next line, 0 being the first.
    next: usize,
}

#[derive(Clone, Debug)]
enum What {
    One(Str),
    Array(ArrayRef),
}

impl Lines {
    pub fn new(value: &Value) -> Lines {
        let what = match value {
            Value::Array(a) => What::Array(a.clone()),
            v => What::One(scalar(v)),
        };
        Lines { what, next: 0 }
    }

    /// How many lines have been made.
    pub fn made(&self) -> usize {
        self.next
    }

    /// Whether a line is left.
    pub fn has_more(&self) -> bool {
        match &self.what {
            What::One(_) => self.next == 0,
            // The header, the elements and the empty line.
            What::Array(a) => self.next < a.borrow().len() + 2,
        }
    }
}

impl Iterator for Lines {
    type Item = Str;

    fn next(&mut self) -> Option<Str> {
        if !self.has_more() {
            return None;
        }
        let n = self.next;
        self.next += 1;
        match &mut self.what {
            What::One(line) => Some(std::mem::take(line)),
            What::Array(a) => {
                let array = a.borrow();
                Some(match n {
                    0 => header(&array),
                    n if n <= array.len() => element_line(&array, n - 1),
                    _ => Str::new(),
                })
            }
        }
    }
}

fn header(array: &Array) -> Str {
    let kind = match array.data {
        ArrayData::Int(_) => "INT",
        ArrayData::Real(_) => "REAL",
        ArrayData::Str(_) => "STRING",
    };
    let dims: Vec<String> = array.dims.iter().map(usize::to_string).collect();
    format!("{kind}[{}]:", dims.join(","))
        .encode_utf16()
        .collect()
}

/// ` [0,1]: 0` for element `i`.
fn element_line(array: &Array, i: usize) -> Str {
    let mut line: Str = format!(" [{}]: ", index_text(&array.dims, i))
        .encode_utf16()
        .collect();
    line.extend(element(&array.get(i)));
    line
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
    // A string of exactly 256 characters is cut too.
    if s.len() < STRING_CHARS {
        out.push(u16::from(b'"'));
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
        let exact: Value = Value::Str(str_from(&"A".repeat(STRING_CHARS)));
        assert!(!text(&exact)[0].ends_with("A\""));
        let shown = format!("STRING: (1100)\"{}", &long[..STRING_CHARS]);
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

    /// The lines as INSPECT made them before [`Lines`], all at once.
    fn eager(value: &Value) -> Vec<Str> {
        let Value::Array(a) = value else {
            return vec![scalar(value)];
        };
        let array = a.borrow();
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

    fn array(elem: ElemType, dims: &[usize]) -> Value {
        let mut a = Array::new(elem, dims.to_vec());
        for i in 0..a.len() {
            let v = match elem {
                ElemType::Int => Value::Int(i as i32 * 7919 - 30000),
                ElemType::Real => Value::Real(i as f64 / 7.0 - 3.0),
                ElemType::Str => {
                    let len = [0, 1, 24, 255, 256, 257, 1000][i % 7];
                    let text: String = (0..len).map(|k| (b'A' + (k % 26) as u8) as char).collect();
                    Value::Str(str_from(&text))
                }
            };
            a.set(i, v).unwrap();
        }
        Value::Array(Rc::new(RefCell::new(a)))
    }

    #[test]
    fn lines_one_at_a_time_are_the_lines_made_at_once() {
        let shapes: [&[usize]; 9] = [
            &[0],
            &[1],
            &[37],
            &[3, 0],
            &[5, 7],
            &[3, 4, 5],
            &[2, 3, 2, 3],
            &[1, 1, 1, 1],
            &[0, 2, 2, 2],
        ];
        for elem in [ElemType::Int, ElemType::Real, ElemType::Str] {
            for dims in shapes {
                let v = array(elem, dims);
                let mut one = Lines::new(&v);
                let mut made = Vec::new();
                while one.has_more() {
                    made.push(one.next().unwrap());
                    assert_eq!(one.made(), made.len());
                }
                assert_eq!(one.next(), None);
                assert_eq!(made, eager(&v), "{elem:?} {dims:?}");
            }
        }
        let long = "X".repeat(5000);
        for v in [
            Value::Int(-5),
            Value::Real(1e300),
            Value::Str(str_from(&long)),
            Value::Str(str_from("")),
            Value::Default,
        ] {
            assert_eq!(lines(&v), eager(&v));
        }
    }

    #[test]
    fn lines_read_the_array_as_they_go() {
        let v = array(ElemType::Int, &[1000]);
        let mut one = Lines::new(&v);
        let first: Vec<Str> = one.by_ref().take(3).collect();
        assert_eq!(first, eager(&v)[..3]);
        assert_eq!(one.made(), 3);
        let Value::Array(a) = &v else { unreachable!() };
        a.borrow_mut().set(2, Value::Int(-1)).unwrap();
        assert_eq!(String::from_utf16_lossy(&one.next().unwrap()), " [2]: -1");
        // Made shorter: the lines past its end, and the empty one, go.
        *a.borrow_mut() = Array::new(ElemType::Int, vec![2]);
        assert!(!one.has_more());
        assert_eq!(one.next(), None);
    }
}

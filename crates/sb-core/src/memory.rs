//! What FREEMEM reports: SmileBASIC 4's memory for strings and arrays, less
//! what the program's values take.
//!
//! The sizes were fitted to hardware (TTEST21, TTEST23): a block takes its
//! contents plus a header, rounded up to 64 bytes, and small contents take
//! nothing. Values are counted as they are now, so memory a program lets
//! go of is free at once; SmileBASIC 4 frees it only later.
// Checked on hardware: strings of up to 24 characters (48 bytes) and
// arrays of up to 64 bytes of elements take nothing.
// UNVERIFIED: arrays of 65 to 79 bytes.

use std::collections::HashSet;

use crate::value::{ArrayData, Value};

/// FREEMEM right after start-up, which the start-up screen shows too.
pub const TOTAL: u32 = 134_216_720;
/// Contents up to these sizes take no memory of their own.
const SMALL_STRING: usize = 48;
const SMALL_ARRAY: usize = 64;
/// A string's header and terminator: 1000 characters take 2112 bytes.
const STRING_HEADER: usize = 98;
/// An array's header: 100 Reals take 896 bytes, 100000 take 800064.
const ARRAY_HEADER: usize = 64;

fn block(contents: usize, small: usize, header: usize) -> usize {
    match contents {
        n if n <= small => 0,
        n => (n + header).div_ceil(64) * 64,
    }
}

/// Counts each string and array once, however many values refer to it.
#[derive(Default)]
pub struct Tally {
    seen: HashSet<usize>,
    pub bytes: usize,
}

impl Tally {
    /// Leaves an array out, as if counted already.
    pub fn skip(&mut self, array: &crate::value::ArrayRef) {
        self.seen.insert(array.as_ptr() as usize);
    }

    pub fn add(&mut self, value: &Value) {
        match value {
            Value::Str(s) => {
                if self.seen.insert(s.as_ptr() as usize) {
                    self.bytes += block(s.borrow().len() * 2, SMALL_STRING, STRING_HEADER);
                }
            }
            Value::Array(a) => {
                if !self.seen.insert(a.as_ptr() as usize) {
                    return;
                }
                let a = a.borrow();
                // Strings are held by pointers of 8 bytes.
                let contents = match &a.data {
                    ArrayData::Int(v) => v.len() * 4,
                    ArrayData::Real(v) => v.len() * 8,
                    ArrayData::Str(v) => v.len() * 8,
                };
                self.bytes += block(contents, SMALL_ARRAY, ARRAY_HEADER);
                if let ArrayData::Str(v) = &a.data {
                    for s in v {
                        self.add(&Value::Str(s.clone()));
                    }
                }
            }
            _ => {}
        }
    }

    /// FREEMEM: what is left.
    pub fn free(&self) -> i32 {
        (TOTAL as usize).saturating_sub(self.bytes) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{Array, ElemType, new_str};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn used(values: &[Value]) -> usize {
        let mut t = Tally::default();
        values.iter().for_each(|v| t.add(v));
        t.bytes
    }

    fn string(len: usize) -> Value {
        Value::Str(new_str(vec![u16::from(b'X'); len]))
    }

    fn array(elem: ElemType, len: usize) -> Value {
        Value::Array(Rc::new(RefCell::new(Array::new(elem, vec![len]))))
    }

    #[test]
    fn sizes_seen_on_hardware() {
        for (len, bytes) in [
            (17, 0),
            (24, 0),
            (25, 192),
            (31, 192),
            (32, 192),
            (100, 320),
            (1000, 2112),
            (10000, 20160),
        ] {
            assert_eq!(used(&[string(len)]), bytes, "{len} characters");
        }
        for (elem, len, bytes) in [
            (ElemType::Int, 10, 0),
            (ElemType::Int, 16, 0),
            (ElemType::Real, 8, 0),
            (ElemType::Int, 100, 512),
            (ElemType::Real, 1, 0),
            (ElemType::Real, 100, 896),
            (ElemType::Real, 100_000, 800_064),
            (ElemType::Str, 10, 192),
            (ElemType::Str, 100, 896),
        ] {
            assert_eq!(used(&[array(elem, len)]), bytes, "{elem:?} x {len}");
        }
    }

    #[test]
    fn shared_values_count_once() {
        let s = string(1000);
        assert_eq!(used(&[s.clone(), s]), 2112);
    }
}

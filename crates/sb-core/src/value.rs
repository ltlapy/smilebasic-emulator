//! Run-time values.
//!
//! Strings and arrays are reference types: assigning one copies the
//! reference, so `B$=A$:A$[1]="Z"` changes what both variables show.

use std::cell::RefCell;
use std::rc::Rc;

use crate::error::{Error, ErrorKind, Result};

pub type Str = Vec<u16>;
pub type StrRef = Rc<RefCell<Str>>;
pub type ArrayRef = Rc<RefCell<Array>>;

pub fn new_str(s: Str) -> StrRef {
    Rc::new(RefCell::new(s))
}

pub fn str_from(s: &str) -> StrRef {
    new_str(crate::wtf8::to_utf16(s))
}

#[derive(Clone, Debug)]
pub enum Value {
    /// The value of an omitted argument. Any operation on it is an error.
    Default,
    Int(i32),
    Real(f64),
    Str(StrRef),
    Array(ArrayRef),
}

/// Element type of an array, which is fixed when the array is created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElemType {
    Int,
    Real,
    Str,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArrayData {
    Int(Vec<i32>),
    Real(Vec<f64>),
    Str(Vec<StrRef>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Array {
    /// One to four dimensions. Elements are stored row-major, so any array
    /// can also be indexed as one-dimensional.
    pub dims: Vec<usize>,
    pub data: ArrayData,
}

pub const MAX_DIMS: usize = 4;

impl Array {
    pub fn new(elem: ElemType, dims: Vec<usize>) -> Self {
        let len = dims.iter().product();
        let data = match elem {
            ElemType::Int => ArrayData::Int(vec![0; len]),
            ElemType::Real => ArrayData::Real(vec![0.0; len]),
            ElemType::Str => ArrayData::Str((0..len).map(|_| new_str(Vec::new())).collect()),
        };
        Array { dims, data }
    }

    pub fn elem_type(&self) -> ElemType {
        match self.data {
            ArrayData::Int(_) => ElemType::Int,
            ArrayData::Real(_) => ElemType::Real,
            ArrayData::Str(_) => ElemType::Str,
        }
    }

    pub fn len(&self) -> usize {
        match &self.data {
            ArrayData::Int(v) => v.len(),
            ArrayData::Real(v) => v.len(),
            ArrayData::Str(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Flat position of `indices`: either one index into the flat storage, or
    /// exactly one per dimension.
    pub fn offset(&self, indices: &[i32]) -> Result<usize> {
        let in_range = |i: i32, n: usize| usize::try_from(i).ok().filter(|&i| i < n);
        if indices.len() == 1 {
            return in_range(indices[0], self.len()).ok_or(ErrorKind::SubscriptOutOfRange.into());
        }
        if indices.len() != self.dims.len() {
            return Err(ErrorKind::SubscriptOutOfRange.into());
        }
        let mut offset = 0;
        for (&i, &n) in indices.iter().zip(&self.dims) {
            offset = offset * n + in_range(i, n).ok_or(ErrorKind::SubscriptOutOfRange)?;
        }
        Ok(offset)
    }

    pub fn get(&self, offset: usize) -> Value {
        match &self.data {
            ArrayData::Int(v) => Value::Int(v[offset]),
            ArrayData::Real(v) => Value::Real(v[offset]),
            ArrayData::Str(v) => Value::Str(v[offset].clone()),
        }
    }

    pub fn set(&mut self, offset: usize, value: Value) -> Result<()> {
        match &mut self.data {
            ArrayData::Int(v) => v[offset] = value.to_int()?,
            ArrayData::Real(v) => v[offset] = value.to_real()?,
            ArrayData::Str(v) => v[offset] = value.into_str()?,
        }
        Ok(())
    }
}

/// Real to integer, dropping the fraction as INT() does.
/// Values outside the 32-bit range raise Overflow.
pub fn real_to_int(x: f64) -> Result<i32> {
    let t = x.trunc();
    if t.is_nan() || t < i32::MIN as f64 || t > i32::MAX as f64 {
        return Err(ErrorKind::Overflow.into());
    }
    Ok(t as i32)
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(str_from(s))
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Real(_))
    }

    pub fn to_int(&self) -> Result<i32> {
        match *self {
            Value::Int(n) => Ok(n),
            Value::Real(x) => real_to_int(x).map_err(|e| e.of_value(self)),
            _ => Err(Error::new(ErrorKind::TypeMismatch).of_value(self)),
        }
    }

    pub fn to_real(&self) -> Result<f64> {
        match *self {
            Value::Int(n) => Ok(f64::from(n)),
            Value::Real(x) => Ok(x),
            _ => Err(Error::new(ErrorKind::TypeMismatch).of_value(self)),
        }
    }

    pub fn into_str(self) -> Result<StrRef> {
        match self {
            Value::Str(s) => Ok(s),
            _ => Err(ErrorKind::TypeMismatch.into()),
        }
    }

    pub fn into_array(self) -> Result<ArrayRef> {
        match self {
            Value::Array(a) => Ok(a),
            _ => Err(ErrorKind::TypeMismatch.into()),
        }
    }

    /// Conditions treat any non-zero number as true.
    pub fn is_true(&self) -> Result<bool> {
        match *self {
            Value::Int(n) => Ok(n != 0),
            Value::Real(x) => Ok(x != 0.0),
            _ => Err(ErrorKind::TypeMismatch.into()),
        }
    }

    /// TYPEOF() codes.
    pub fn type_code(&self) -> i32 {
        match self {
            Value::Default => 0,
            Value::Int(_) => 1,
            Value::Real(_) => 2,
            Value::Str(_) => 3,
            Value::Array(a) => match a.borrow().elem_type() {
                ElemType::Int => 5,
                ElemType::Real => 6,
                ElemType::Str => 7,
            },
        }
    }
}

/// Number to text as PRINT and STR$ show it: integers in decimal, reals
/// with eight decimals and the trailing zeros removed, never in exponent
/// form (checked on hardware: 1E20 prints in full, 1/3 as 0.33333333).
/// NaN shows as `nan` and infinity as `inf`.
pub fn format_number(value: &Value) -> Str {
    let text = match *value {
        Value::Int(n) => n.to_string(),
        Value::Real(x) => format_real(x),
        _ => String::new(),
    };
    text.encode_utf16().collect()
}

/// A real as C's `%g` gives it, which is STR$'s form and not PRINT's: six
/// significant digits, with an exponent when it is -5 or under, or 6 or over.
pub fn format_g(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.into();
    }
    let e = format!("{x:.5e}");
    let (mantissa, exponent) = e.split_once('e').expect("{:e} has an exponent");
    let exponent: i32 = exponent.parse().expect("{:e} gives a whole exponent");
    let bare = |s: &str| match s.contains('.') {
        true => s.trim_end_matches('0').trim_end_matches('.').to_string(),
        false => s.to_string(),
    };
    if (-4..6).contains(&exponent) {
        bare(&format!("{x:.*}", (5 - exponent) as usize))
    } else {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{}e{sign}{:02}", bare(mantissa), exponent.abs())
    }
}

fn format_real(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.into();
    }
    // A small negative number shows as -0 (checked on hardware).
    let fixed = format!("{x:.8}");
    fixed
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(v: Value) -> String {
        String::from_utf16(&format_number(&v)).unwrap()
    }

    #[test]
    fn g_formatting() {
        assert_eq!(format_g(2.0 / 3.0), "0.666667");
        assert_eq!(format_g(1.0), "1");
        assert_eq!(format_g(-2.5), "-2.5");
        assert_eq!(format_g(100000.0), "100000");
        assert_eq!(format_g(999999.5), "1e+06");
        assert_eq!(format_g(1234567.0), "1.23457e+06");
        assert_eq!(format_g(0.0001), "0.0001");
        assert_eq!(format_g(0.00001), "1e-05");
        assert_eq!(format_g(1e100), "1e+100");
        assert_eq!(format_g(0.0), "0");
    }

    #[test]
    fn number_formatting() {
        assert_eq!(fmt(Value::Int(-42)), "-42");
        assert_eq!(fmt(Value::Real(3.0)), "3");
        assert_eq!(fmt(Value::Real(1.0 / 3.0)), "0.33333333");
        assert_eq!(fmt(Value::Real(-2.5)), "-2.5");
        assert_eq!(fmt(Value::Real(1e20)), "100000000000000000000");
        assert_eq!(fmt(Value::Real(1e-7)), "0.0000001");
        assert_eq!(fmt(Value::Real(123456.789)), "123456.789");
        assert_eq!(fmt(Value::Real(-1e-9)), "-0");
    }

    #[test]
    fn real_to_int_truncates_toward_zero() {
        assert_eq!(real_to_int(12.9), Ok(12));
        assert_eq!(real_to_int(-12.9), Ok(-12));
        assert_eq!(real_to_int(3e9).unwrap_err().kind, ErrorKind::Overflow);
    }

    #[test]
    fn array_offsets() {
        let a = Array::new(ElemType::Int, vec![2, 3]);
        assert_eq!(a.offset(&[1, 2]), Ok(5));
        assert_eq!(a.offset(&[5]), Ok(5));
        assert!(a.offset(&[6]).is_err());
        assert!(a.offset(&[0, 3]).is_err());
        assert!(a.offset(&[-1]).is_err());
        assert!(a.offset(&[0, 0, 0]).is_err());
    }

    #[test]
    fn array_set_converts_to_element_type() {
        let mut a = Array::new(ElemType::Int, vec![1]);
        a.set(0, Value::Real(2.7)).unwrap();
        assert_eq!(a.data, ArrayData::Int(vec![2]));
        assert_eq!(
            a.set(0, Value::str("x")).unwrap_err().kind,
            ErrorKind::TypeMismatch
        );
    }
}

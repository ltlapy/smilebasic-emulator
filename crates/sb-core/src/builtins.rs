//! Built-in functions and commands that need nothing from the host:
//! numbers, strings and arrays.

use crate::error::{Error, ErrorKind, Result};
use crate::value::{Array, ArrayData, ElemType, Str, Value, format_number, new_str};

#[derive(Clone, Copy)]
pub enum Kind {
    Function(fn(&[Value]) -> Result<Value>),
    /// Called as a statement; gets the number of OUT variables and returns
    /// that many values.
    Command(fn(&[Value], usize) -> Result<Vec<Value>>),
}

/// How a builtin is being called. A name can have both forms (`RGB(...)`
/// and `RGB C OUT R,G,B`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Function,
    Command,
}

#[derive(Clone, Copy)]
pub struct Builtin {
    /// Upper case, suffix included.
    pub name: &'static str,
    pub min_args: u8,
    pub max_args: u8,
    pub kind: Kind,
}

const VARIADIC: u8 = u8::MAX;

macro_rules! table {
    ($($name:literal $min:literal..=$max:tt $kind:ident $f:expr;)*) => {
        static BUILTINS: &[Builtin] = &[
            $(Builtin { name: $name, min_args: $min, max_args: table!(@max $max), kind: Kind::$kind($f) },)*
        ];
    };
    (@max N) => { VARIADIC };
    (@max $n:literal) => { $n };
}

table! {
    "LEN" 1..=1 Function len;
    "LAST" 1..=1 Function |a| Ok(Value::Int(length(&a[0])? - 1));
    "STR$" 1..=2 Function str;
    "VAL" 1..=1 Function val;
    "CHR$" 1..=1 Function chr;
    "ASC" 1..=1 Function asc;
    "MID$" 2..=3 Function mid;
    "LEFT$" 2..=2 Function left;
    "RIGHT$" 2..=2 Function right;
    "INSTR" 2..=3 Function instr;
    "SUBST$" 3..=4 Function subst;
    "HEX$" 1..=2 Function |a| radix_str(a, 16, 8);
    "BIN$" 1..=2 Function |a| radix_str(a, 2, 32);
    "INT" 1..=1 Function |a| Ok(Value::Int(a[0].to_int()?));
    "FLOAT" 1..=1 Function |a| Ok(Value::Real(a[0].to_real()?));
    "FLOOR" 1..=1 Function |a| real_fn(a, f64::floor);
    // UNVERIFIED: halves round away from zero.
    "ROUND" 1..=1 Function |a| real_fn(a, f64::round);
    "CEIL" 1..=1 Function |a| real_fn(a, f64::ceil);
    "ABS" 1..=1 Function abs;
    "SGN" 1..=1 Function |a| Ok(Value::Int(sign(a[0].to_real()?)));
    "SQR" 1..=1 Function |a| checked_real(a, |x| (x >= 0.0).then(|| x.sqrt()));
    "POW" 2..=2 Function |a| Ok(Value::Real(a[0].to_real()?.powf(a[1].to_real()?)));
    "EXP" 0..=1 Function exp;
    "LOG" 1..=2 Function log;
    "SIN" 1..=1 Function |a| real_fn(a, f64::sin);
    "COS" 1..=1 Function |a| real_fn(a, f64::cos);
    "TAN" 1..=1 Function |a| real_fn(a, f64::tan);
    "ASIN" 1..=1 Function |a| checked_real(a, |x| (-1.0..=1.0).contains(&x).then(|| x.asin()));
    "ACOS" 1..=1 Function |a| checked_real(a, |x| (-1.0..=1.0).contains(&x).then(|| x.acos()));
    "ATAN" 1..=2 Function atan;
    "SINH" 1..=1 Function |a| real_fn(a, f64::sinh);
    "COSH" 1..=1 Function |a| real_fn(a, f64::cosh);
    "TANH" 1..=1 Function |a| real_fn(a, f64::tanh);
    "DEG" 1..=1 Function |a| real_fn(a, f64::to_degrees);
    "RAD" 1..=1 Function |a| real_fn(a, f64::to_radians);
    "MIN" 1..=N Function |a| extreme(a, std::cmp::Ordering::Less);
    "MAX" 1..=N Function |a| extreme(a, std::cmp::Ordering::Greater);
    "TYPEOF" 1..=1 Function |a| Ok(Value::Int(a[0].type_code()));
    "ARRAY%" 1..=4 Function |a| new_array(a, ElemType::Int);
    "ARRAY#" 1..=4 Function |a| new_array(a, ElemType::Real);
    "ARRAY$" 1..=4 Function |a| new_array(a, ElemType::Str);
    "DIM" 1..=2 Function dim;
    "POP" 1..=1 Function |a| take(&a[0], End::Back);
    "SHIFT" 1..=1 Function |a| take(&a[0], End::Front);
    "PUSH" 2..=2 Command |a, _| put(&a[0], a[1].clone(), End::Back).map(|()| Vec::new());
    "UNSHIFT" 2..=2 Command |a, _| put(&a[0], a[1].clone(), End::Front).map(|()| Vec::new());
    "FILL" 2..=4 Command |a, _| fill(a).map(|()| Vec::new());
    "RGB" 3..=5 Function rgb;
    "RGB" 1..=1 Command rgb_out;
}

pub fn lookup(name: &str, form: Form) -> Option<u16> {
    BUILTINS
        .iter()
        .position(|b| b.name == name && b.form() == form)
        .map(|i| i as u16)
}

pub fn get(id: u16) -> &'static Builtin {
    &BUILTINS[usize::from(id)]
}

impl Builtin {
    pub fn form(&self) -> Form {
        match self.kind {
            Kind::Function(_) => Form::Function,
            Kind::Command(_) => Form::Command,
        }
    }

    pub fn accepts(&self, argc: usize) -> bool {
        argc >= self.min_args.into() && (self.max_args == VARIADIC || argc <= self.max_args.into())
    }
}

// ---- argument helpers ----

fn text(v: &Value) -> Result<Str> {
    match v {
        Value::Str(s) => Ok(s.borrow().clone()),
        _ => Err(ErrorKind::TypeMismatch.into()),
    }
}

fn string(s: Str) -> Value {
    Value::Str(new_str(s))
}

/// An optional argument: missing or omitted (`F(A,,B)`) reads as `None`.
fn opt(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !matches!(v, Value::Default))
}

fn index_arg(v: &Value) -> Result<usize> {
    usize::try_from(v.to_int()?).map_err(|_| ErrorKind::OutOfRange.into())
}

fn real_fn(args: &[Value], f: fn(f64) -> f64) -> Result<Value> {
    Ok(Value::Real(f(args[0].to_real()?)))
}

/// UNVERIFIED: arguments outside a function's domain raise Out of range.
fn checked_real(args: &[Value], f: fn(f64) -> Option<f64>) -> Result<Value> {
    f(args[0].to_real()?)
        .map(Value::Real)
        .ok_or(ErrorKind::OutOfRange.into())
}

fn sign(x: f64) -> i32 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

// ---- numbers ----

fn abs(args: &[Value]) -> Result<Value> {
    match args[0] {
        Value::Int(n) => Ok(n
            .checked_abs()
            .map_or(Value::Real(-f64::from(n)), Value::Int)),
        ref v => Ok(Value::Real(v.to_real()?.abs())),
    }
}

fn exp(args: &[Value]) -> Result<Value> {
    Ok(Value::Real(
        opt(args, 0).map_or(Ok(1.0), Value::to_real)?.exp(),
    ))
}

fn log(args: &[Value]) -> Result<Value> {
    let x = args[0].to_real()?;
    if x <= 0.0 {
        return Err(ErrorKind::OutOfRange.into());
    }
    match opt(args, 1) {
        None => Ok(Value::Real(x.ln())),
        Some(base) => {
            let base = base.to_real()?;
            if base <= 0.0 || base == 1.0 {
                return Err(ErrorKind::OutOfRange.into());
            }
            Ok(Value::Real(x.log(base)))
        }
    }
}

fn atan(args: &[Value]) -> Result<Value> {
    let y = args[0].to_real()?;
    Ok(Value::Real(match opt(args, 1) {
        Some(x) => y.atan2(x.to_real()?),
        None => y.atan(),
    }))
}

/// MIN/MAX over the arguments, or over the elements of a single array.
/// The winning value keeps its type.
fn extreme(args: &[Value], want: std::cmp::Ordering) -> Result<Value> {
    let values: Vec<Value> = match args {
        [Value::Array(a)] => {
            let a = a.borrow();
            if a.elem_type() == ElemType::Str || a.is_empty() {
                return Err(ErrorKind::TypeMismatch.into());
            }
            (0..a.len()).map(|i| a.get(i)).collect()
        }
        _ => args.to_vec(),
    };
    let mut best = values[0].clone();
    for v in &values[1..] {
        if v.to_real()?.partial_cmp(&best.to_real()?) == Some(want) {
            best = v.clone();
        }
    }
    best.to_real()?;
    Ok(best)
}

// ---- strings ----

fn length(v: &Value) -> Result<i32> {
    let n = match v {
        Value::Str(s) => s.borrow().len(),
        Value::Array(a) => a.borrow().len(),
        _ => return Err(ErrorKind::TypeMismatch.into()),
    };
    Ok(n as i32)
}

fn len(args: &[Value]) -> Result<Value> {
    Ok(Value::Int(length(&args[0])?))
}

fn str(args: &[Value]) -> Result<Value> {
    if !args[0].is_number() {
        return Err(ErrorKind::TypeMismatch.into());
    }
    let digits = opt(args, 1).map_or(Ok(0), index_arg)?;
    if digits > 63 {
        return Err(ErrorKind::OutOfRange.into());
    }
    Ok(string(pad_left(format_number(&args[0]), digits, ' ')))
}

fn pad_left(mut s: Str, width: usize, fill: char) -> Str {
    if s.len() < width {
        let mut padded = vec![fill as u16; width - s.len()];
        padded.append(&mut s);
        return padded;
    }
    s
}

/// UNVERIFIED: text that is not a number reads as 0, and surrounding spaces
/// are ignored.
fn val(args: &[Value]) -> Result<Value> {
    Ok(parse_number(&text(&args[0])?).unwrap_or(Value::Int(0)))
}

/// Reads a number as VAL and INPUT do: decimal, `&H` hex or `&B` binary,
/// with surrounding spaces. `None` if the text is not a number.
pub fn parse_number(text: &[u16]) -> Option<Value> {
    let s = String::from_utf16_lossy(text);
    let s = s.trim();
    let upper = s.to_ascii_uppercase();
    let radix = |digits: &str, radix| u32::from_str_radix(digits, radix).ok();
    if let Some(hex) = upper.strip_prefix("&H") {
        radix(hex, 16).map(|n| Value::Int(n as i32))
    } else if let Some(bin) = upper.strip_prefix("&B") {
        radix(bin, 2).map(|n| Value::Int(n as i32))
    } else if let Ok(n) = s.parse::<i32>() {
        Some(Value::Int(n))
    } else {
        s.parse::<f64>()
            .ok()
            .filter(|x| x.is_finite())
            .map(Value::Real)
    }
}

fn chr(args: &[Value]) -> Result<Value> {
    let code = u16::try_from(args[0].to_int()?).map_err(|_| ErrorKind::OutOfRange)?;
    Ok(string(vec![code]))
}

fn asc(args: &[Value]) -> Result<Value> {
    let s = text(&args[0])?;
    let first = s.first().ok_or(ErrorKind::OutOfRange)?;
    Ok(Value::Int(i32::from(*first)))
}

/// UNVERIFIED: a start past the end gives "" and counts are clamped,
/// while negative positions or counts are errors.
fn substring(s: &[u16], start: usize, count: usize) -> Str {
    let start = start.min(s.len());
    let end = start.saturating_add(count).min(s.len());
    s[start..end].to_vec()
}

fn mid(args: &[Value]) -> Result<Value> {
    let s = text(&args[0])?;
    let start = index_arg(&args[1])?;
    let count = opt(args, 2).map_or(Ok(usize::MAX), index_arg)?;
    Ok(string(substring(&s, start, count)))
}

fn left(args: &[Value]) -> Result<Value> {
    let s = text(&args[0])?;
    Ok(string(substring(&s, 0, index_arg(&args[1])?)))
}

fn right(args: &[Value]) -> Result<Value> {
    let s = text(&args[0])?;
    let count = index_arg(&args[1])?.min(s.len());
    Ok(string(s[s.len() - count..].to_vec()))
}

fn instr(args: &[Value]) -> Result<Value> {
    let (start, haystack, needle) = match args {
        [s, n] => (0, text(s)?, text(n)?),
        [start, s, n] => (index_arg(start)?, text(s)?, text(n)?),
        _ => unreachable!("arity checked by the caller"),
    };
    if start > haystack.len() {
        return Ok(Value::Int(-1));
    }
    let found = if needle.is_empty() {
        Some(0)
    } else {
        haystack[start..]
            .windows(needle.len())
            .position(|w| w == needle.as_slice())
    };
    Ok(Value::Int(found.map_or(-1, |i| (start + i) as i32)))
}

fn subst(args: &[Value]) -> Result<Value> {
    let s = text(&args[0])?;
    let start = index_arg(&args[1])?.min(s.len());
    let (count, replacement) = match args {
        [_, _, r] => (usize::MAX, text(r)?),
        [_, _, n, r] => (index_arg(n)?, text(r)?),
        _ => unreachable!("arity checked by the caller"),
    };
    let end = start.saturating_add(count).min(s.len());
    let mut out = s[..start].to_vec();
    out.extend_from_slice(&replacement);
    out.extend_from_slice(&s[end..]);
    Ok(string(out))
}

/// HEX$ and BIN$. Negative numbers show their 32-bit two's complement.
fn radix_str(args: &[Value], radix: u32, max_digits: usize) -> Result<Value> {
    let n = args[0].to_int()? as u32;
    let digits = opt(args, 1).map_or(Ok(0), index_arg)?;
    if digits > max_digits {
        return Err(ErrorKind::OutOfRange.into());
    }
    let text = match radix {
        16 => format!("{n:X}"),
        _ => format!("{n:b}"),
    };
    Ok(string(pad_left(text.encode_utf16().collect(), digits, '0')))
}

// ---- colors ----

fn component(v: &Value) -> Result<u32> {
    let n = v.to_int()?;
    u32::try_from(n)
        .ok()
        .filter(|&n| n <= 255)
        .ok_or(ErrorKind::OutOfRange.into())
}

fn argb(a: u32, r: u32, g: u32, b: u32) -> Value {
    Value::Int((a << 24 | r << 16 | g << 8 | b) as i32)
}

/// `RGB(R,G,B)`, `RGB(A,R,G,B)`, or `RGB(color,[A],[R],[G],[B])` which
/// replaces the given components of `color`.
fn rgb(args: &[Value]) -> Result<Value> {
    match args {
        [r, g, b] => Ok(argb(255, component(r)?, component(g)?, component(b)?)),
        [a, r, g, b] => Ok(argb(
            component(a)?,
            component(r)?,
            component(g)?,
            component(b)?,
        )),
        [color, parts @ ..] => {
            let mut c = color.to_int()? as u32;
            for (i, part) in parts.iter().enumerate() {
                if !matches!(part, Value::Default) {
                    let shift = 24 - 8 * i as u32;
                    c = c & !(0xFF << shift) | component(part)? << shift;
                }
            }
            Ok(Value::Int(c as i32))
        }
        [] => unreachable!("arity checked by the caller"),
    }
}

/// `RGB color OUT R,G,B` or `RGB color OUT A,R,G,B`.
fn rgb_out(args: &[Value], outs: usize) -> Result<Vec<Value>> {
    let c = args[0].to_int()? as u32;
    let parts = |shifts: &[u32]| {
        shifts
            .iter()
            .map(|s| Value::Int((c >> s & 0xFF) as i32))
            .collect()
    };
    match outs {
        3 => Ok(parts(&[16, 8, 0])),
        4 => Ok(parts(&[24, 16, 8, 0])),
        _ => Err(ErrorKind::IllegalFunctionCall.into()),
    }
}

// ---- arrays ----

fn new_array(args: &[Value], elem: ElemType) -> Result<Value> {
    let dims = args.iter().map(index_arg).collect::<Result<Vec<_>>>()?;
    let total = dims.iter().try_fold(1usize, |acc, &d| acc.checked_mul(d));
    // UNVERIFIED: the cap stands in for "out of memory".
    if total.is_none_or(|t| t > i32::MAX as usize) {
        return Err(ErrorKind::OutOfRange.into());
    }
    Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
        Array::new(elem, dims),
    ))))
}

fn dim(args: &[Value]) -> Result<Value> {
    let Value::Array(a) = &args[0] else {
        return Err(ErrorKind::TypeMismatch.into());
    };
    let a = a.borrow();
    match opt(args, 1) {
        None => Ok(Value::Int(a.dims.len() as i32)),
        Some(d) => {
            let size = a.dims.get(index_arg(d)?).ok_or(ErrorKind::OutOfRange)?;
            Ok(Value::Int(*size as i32))
        }
    }
}

#[derive(Clone, Copy)]
enum End {
    Front,
    Back,
}

/// PUSH and friends only work on one-dimensional arrays and on strings.
fn check_1d(a: &Array) -> Result<()> {
    if a.dims.len() > 1 {
        return Err(Error::with_detail(
            ErrorKind::IllegalFunctionCall,
            "multi-dimensional array",
        ));
    }
    Ok(())
}

fn put(target: &Value, value: Value, end: End) -> Result<()> {
    match target {
        Value::Str(s) => {
            let piece = text(&value)?;
            let mut s = s.borrow_mut();
            match end {
                End::Back => s.extend_from_slice(&piece),
                End::Front => {
                    s.splice(0..0, piece);
                }
            }
        }
        Value::Array(a) => {
            let mut a = a.borrow_mut();
            check_1d(&a)?;
            let at = match end {
                End::Back => a.len(),
                End::Front => 0,
            };
            match &mut a.data {
                ArrayData::Int(v) => v.insert(at, value.to_int()?),
                ArrayData::Real(v) => v.insert(at, value.to_real()?),
                ArrayData::Str(v) => v.insert(at, value.into_str()?),
            }
            let len = a.len();
            a.dims = vec![len];
        }
        _ => return Err(ErrorKind::TypeMismatch.into()),
    }
    Ok(())
}

fn take(target: &Value, end: End) -> Result<Value> {
    match target {
        Value::Str(s) => {
            let mut s = s.borrow_mut();
            let c = match end {
                End::Back => s.pop(),
                End::Front => (!s.is_empty()).then(|| s.remove(0)),
            };
            Ok(string(vec![c.ok_or(ErrorKind::SubscriptOutOfRange)?]))
        }
        Value::Array(a) => {
            let mut a = a.borrow_mut();
            check_1d(&a)?;
            if a.is_empty() {
                return Err(ErrorKind::SubscriptOutOfRange.into());
            }
            let at = match end {
                End::Back => a.len() - 1,
                End::Front => 0,
            };
            let value = match &mut a.data {
                ArrayData::Int(v) => Value::Int(v.remove(at)),
                ArrayData::Real(v) => Value::Real(v.remove(at)),
                ArrayData::Str(v) => Value::Str(v.remove(at)),
            };
            let len = a.len();
            a.dims = vec![len];
            Ok(value)
        }
        _ => Err(ErrorKind::TypeMismatch.into()),
    }
}

fn fill(args: &[Value]) -> Result<()> {
    let a = args[0].clone().into_array()?;
    let mut a = a.borrow_mut();
    let start = opt(args, 2).map_or(Ok(0), index_arg)?;
    let count = opt(args, 3).map_or(Ok(usize::MAX), index_arg)?;
    if start > a.len() {
        return Err(ErrorKind::OutOfRange.into());
    }
    let end = start.saturating_add(count).min(a.len());
    for i in start..end {
        a.set(i, args[1].clone())?;
    }
    Ok(())
}

/// Used by the compiler to fold constant expressions.
pub fn is_constant_function(name: &str) -> bool {
    // The reference allows these in CONST and DATA. RGB/HSV live in the
    // host's graphics code and are not folded yet.
    matches!(name, "CHR$" | "DEG" | "RAD" | "RGB")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: &[Value]) -> Result<Value> {
        let id = lookup(name, Form::Function).or_else(|| lookup(name, Form::Command));
        let b = get(id.unwrap_or_else(|| panic!("{name} missing")));
        assert!(b.accepts(args.len()));
        match b.kind {
            Kind::Function(f) => f(args),
            Kind::Command(f) => f(args, 0).map(|_| Value::Default),
        }
    }

    fn s(text: &str) -> Value {
        Value::str(text)
    }

    fn as_string(v: Value) -> String {
        String::from_utf16(&v.into_str().unwrap().borrow()).unwrap()
    }

    fn as_int(v: Value) -> i32 {
        match v {
            Value::Int(n) => n,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn string_functions_use_zero_based_positions() {
        assert_eq!(
            as_string(call("MID$", &[s("ABC"), Value::Int(0), Value::Int(2)]).unwrap()),
            "AB"
        );
        assert_eq!(
            as_string(call("MID$", &[s("ABC"), Value::Int(1)]).unwrap()),
            "BC"
        );
        assert_eq!(
            as_string(call("LEFT$", &[s("ABC"), Value::Int(2)]).unwrap()),
            "AB"
        );
        assert_eq!(
            as_string(call("RIGHT$", &[s("ABC"), Value::Int(2)]).unwrap()),
            "BC"
        );
        assert_eq!(
            as_int(call("INSTR", &[Value::Int(0), s("ABC"), s("B")]).unwrap()),
            1
        );
        assert_eq!(as_int(call("INSTR", &[s("ABC"), s("Z")]).unwrap()), -1);
        assert_eq!(
            as_string(call("SUBST$", &[s("ABC"), Value::Int(0), Value::Int(2), s("XY")]).unwrap()),
            "XYC"
        );
        assert_eq!(
            as_string(call("SUBST$", &[s("ABC"), Value::Int(1), s("Z")]).unwrap()),
            "AZ"
        );
    }

    #[test]
    fn conversions() {
        assert_eq!(as_string(call("STR$", &[Value::Int(123)]).unwrap()), "123");
        assert_eq!(
            as_string(call("STR$", &[Value::Int(7), Value::Int(3)]).unwrap()),
            "  7"
        );
        assert_eq!(
            as_string(call("HEX$", &[Value::Int(65535), Value::Int(6)]).unwrap()),
            "00FFFF"
        );
        assert_eq!(
            as_string(call("HEX$", &[Value::Int(-1)]).unwrap()),
            "FFFFFFFF"
        );
        assert_eq!(
            as_string(call("BIN$", &[Value::Int(5), Value::Int(4)]).unwrap()),
            "0101"
        );
        assert_eq!(as_int(call("VAL", &[s("123")]).unwrap()), 123);
        assert!(matches!(call("VAL", &[s("-12.56")]).unwrap(), Value::Real(x) if x == -12.56));
        assert_eq!(as_int(call("VAL", &[s("&HFF")]).unwrap()), 255);
        assert_eq!(as_int(call("ASC", &[s("XYZ")]).unwrap()), 88);
        assert_eq!(as_string(call("CHR$", &[Value::Int(65)]).unwrap()), "A");
    }

    #[test]
    fn math() {
        assert_eq!(as_int(call("INT", &[Value::Real(-12.5)]).unwrap()), -12);
        assert!(
            matches!(call("FLOOR", &[Value::Real(-12.5)]).unwrap(), Value::Real(x) if x == -13.0)
        );
        assert_eq!(
            as_int(call("MAX", &[Value::Int(3), Value::Int(9), Value::Int(4)]).unwrap()),
            9
        );
        assert_eq!(as_int(call("SGN", &[Value::Real(-0.5)]).unwrap()), -1);
        assert_eq!(
            call("SQR", &[Value::Int(-1)]).unwrap_err().kind,
            ErrorKind::OutOfRange
        );
    }

    #[test]
    fn colors() {
        let int = |v: Value| as_int(v) as u32;
        assert_eq!(
            int(call("RGB", &[Value::Int(0), Value::Int(100), Value::Int(200)]).unwrap()),
            0xFF0064C8
        );
        let four = [
            Value::Int(128),
            Value::Int(0),
            Value::Int(100),
            Value::Int(200),
        ];
        assert_eq!(int(call("RGB", &four).unwrap()), 0x800064C8);
        // RGB(#C_RED,127,,,) is a half-transparent red.
        let red = Value::Int(0xFFFF0000u32 as i32);
        let half = [
            red,
            Value::Int(127),
            Value::Default,
            Value::Default,
            Value::Default,
        ];
        assert_eq!(int(call("RGB", &half).unwrap()), 0x7FFF0000);
        let too_big = [Value::Int(256), Value::Int(0), Value::Int(0)];
        assert_eq!(
            call("RGB", &too_big).unwrap_err().kind,
            ErrorKind::OutOfRange
        );
        let b = get(lookup("RGB", Form::Command).unwrap());
        let Kind::Command(f) = b.kind else { panic!() };
        let out = f(&[Value::Int(0x80112233u32 as i32)], 4).unwrap();
        assert_eq!(
            out.iter().map(|v| as_int(v.clone())).collect::<Vec<_>>(),
            [0x80, 0x11, 0x22, 0x33]
        );
    }

    #[test]
    fn arrays() {
        let a = call("ARRAY%", &[Value::Int(2)]).unwrap();
        call("PUSH", &[a.clone(), Value::Real(5.9)]).unwrap();
        call("UNSHIFT", &[a.clone(), Value::Int(7)]).unwrap();
        assert_eq!(as_int(call("LEN", std::slice::from_ref(&a)).unwrap()), 4);
        assert_eq!(as_int(call("POP", std::slice::from_ref(&a)).unwrap()), 5);
        assert_eq!(as_int(call("SHIFT", std::slice::from_ref(&a)).unwrap()), 7);
        call("FILL", &[a.clone(), Value::Int(3)]).unwrap();
        assert_eq!(as_int(call("MIN", std::slice::from_ref(&a)).unwrap()), 3);

        let grid = call("ARRAY#", &[Value::Int(2), Value::Int(3)]).unwrap();
        assert_eq!(as_int(call("DIM", std::slice::from_ref(&grid)).unwrap()), 2);
        assert_eq!(
            as_int(call("DIM", &[grid.clone(), Value::Int(1)]).unwrap()),
            3
        );
        assert_eq!(
            call("POP", &[grid]).unwrap_err().kind,
            ErrorKind::IllegalFunctionCall
        );

        let text = s("AB");
        call("PUSH", &[text.clone(), s("C")]).unwrap();
        assert_eq!(as_string(text), "ABC");
    }
}

//! Built-in functions and commands that need nothing from the host:
//! numbers, strings and arrays.

mod arrays;
mod signal;

use crate::bytecode::{Compiled, Literal};
use crate::error::{Error, ErrorKind, Result};
use crate::value::{Array, ArrayData, ElemType, Str, Value, format_number, new_str};

#[derive(Clone, Copy)]
pub enum Kind {
    Function(fn(&[Value]) -> Result<Value>),
    /// Called as a statement; gets the number of OUT variables and returns
    /// that many values.
    Command(fn(&[Value], usize) -> Result<Vec<Value>>),
    /// A command that also reads DATA, moving READ's place.
    Data(fn(&[Value], usize, &mut Data) -> Result<Vec<Value>>),
}

/// The program's DATA and READ's place in it, for [`Kind::Data`].
pub struct Data<'a> {
    pub(crate) program: &'a Compiled,
    pub(crate) pos: &'a mut usize,
}

impl Data<'_> {
    /// RESTORE to a `"@label"`.
    pub fn restore(&mut self, label: &Value) -> Result<()> {
        let name = crate::vm::label_name(label)?;
        let pos = self
            .program
            .data_labels
            .get(&name)
            .ok_or_else(|| Error::with_detail(ErrorKind::UndefinedLabel, format!("@{name}")))?;
        *self.pos = *pos as usize;
        Ok(())
    }

    /// READ.
    pub fn read(&mut self) -> Result<Value> {
        let item = self
            .program
            .data
            .get(*self.pos)
            .ok_or(ErrorKind::OutOfData)?;
        *self.pos += 1;
        Ok(match item {
            Literal::Int(n) => Value::Int(*n),
            Literal::Real(x) => Value::Real(*x),
            Literal::Str(s) => Value::Str(new_str(s.clone())),
        })
    }
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
    "RGBF" 3..=5 Function rgbf;
    "RGBF" 1..=1 Command rgbf_out;
    "HSV" 3..=4 Function |a| hsv(a, false);
    "HSV" 1..=1 Command |a, outs| hsv_out(a, outs, false);
    "HSVF" 3..=4 Function |a| hsv(a, true);
    "HSVF" 1..=1 Command |a, outs| hsv_out(a, outs, true);
    "FORMAT$" 1..=N Function format;
    "COPY" 1..=3 Function arrays::copy_fn;
    "COPY" 2..=5 Data arrays::copy;
    "SORT" 1..=10 Command arrays::sort_up;
    "RSORT" 1..=10 Command arrays::sort_down;
    "INSERT" 2..=4 Command arrays::insert;
    "REMOVE" 2..=3 Command arrays::remove;
    "FIND" 2..=2 Function arrays::find_fn;
    "FIND" 2..=2 Command arrays::find;
    "RESIZE" 2..=5 Command arrays::resize;
    "ARYOP" 4..=5 Command arrays::aryop;
    "RINGCOPY" 3..=5 Command arrays::ringcopy;
    "CLASSIFY" 1..=1 Function signal::classify;
    "FFT" 4..=5 Command signal::fft;
    "IFFT" 4..=4 Command signal::ifft;
    "FFTWFN" 2..=2 Command signal::fftwfn;
    "BIQUAD" 3..=3 Command signal::biquad;
    "BQPARAM" 5..=6 Command signal::bqparam;
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
            Kind::Command(_) | Kind::Data(_) => Form::Command,
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

/// Text that is not a number reads as 0, and so does a number with spaces
/// around it (checked on hardware: `VAL(" 12 ")` is 0).
fn val(args: &[Value]) -> Result<Value> {
    Ok(parse_number(&text(&args[0])?).unwrap_or(Value::Int(0)))
}

/// Reads a number as VAL and INPUT do: decimal, `&H` hex or `&B` binary.
/// `None` if the text is not a number.
pub fn parse_number(text: &[u16]) -> Option<Value> {
    let s = String::from_utf16_lossy(text);
    let s = s.as_str();
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

/// FORMAT$: `%S`, `%D`, `%X`, `%F`, `%B` and `%%`, each with C's flags
/// (`-`, `+`, space, `0`), width and precision. Letters in either case
/// are conversions and values left over are ignored (checked on
/// hardware).
fn format(args: &[Value]) -> Result<Value> {
    let fmt = text(&args[0])?;
    let mut values = args[1..].iter();
    let mut out: Str = Vec::new();
    let at = |i: usize| fmt.get(i).and_then(|&c| char::from_u32(c.into()));
    let mut i = 0;
    while i < fmt.len() {
        if at(i) != Some('%') {
            out.push(fmt[i]);
            i += 1;
            continue;
        }
        i += 1;
        let (mut left, mut plus, mut space, mut zero) = (false, false, false, false);
        loop {
            match at(i) {
                Some('-') => left = true,
                Some('+') => plus = true,
                Some(' ') => space = true,
                Some('0') => zero = true,
                _ => break,
            }
            i += 1;
        }
        let number = |i: &mut usize| {
            let start = *i;
            while at(*i).is_some_and(|c| c.is_ascii_digit()) {
                *i += 1;
            }
            String::from_utf16_lossy(&fmt[start..*i])
                .parse::<usize>()
                .ok()
        };
        let width = number(&mut i).unwrap_or(0);
        let precision = if at(i) == Some('.') {
            i += 1;
            Some(number(&mut i).unwrap_or(0))
        } else {
            None
        };
        let conversion = at(i).map(|c| c.to_ascii_uppercase());
        i += 1;
        if conversion == Some('%') {
            out.push(u16::from(b'%'));
            continue;
        }
        let value = values.next().ok_or_else(|| {
            Error::with_detail(ErrorKind::WrongNumberOfArguments, "too few values")
        })?;
        let (sign, body): (&str, String) = match conversion {
            Some('S') => {
                let s = String::from_utf16_lossy(&text(value)?);
                let s = match precision {
                    Some(p) => s.chars().take(p).collect(),
                    None => s,
                };
                zero = false;
                ("", s)
            }
            Some(c @ ('D' | 'X' | 'B' | 'F')) => {
                if !value.is_number() {
                    return Err(ErrorKind::TypeMismatch.into());
                }
                let negative = value.to_real()? < 0.0;
                let body = match c {
                    'D' => i64::from(value.to_int()?).unsigned_abs().to_string(),
                    'X' => format!("{:X}", value.to_int()? as u32),
                    'B' => format!("{:b}", value.to_int()? as u32),
                    _ => fixed(value.to_real()?.abs(), precision.unwrap_or(6)),
                };
                let sign = match (c, negative) {
                    ('X' | 'B', _) => "",
                    (_, true) => "-",
                    _ if plus => "+",
                    _ if space => " ",
                    _ => "",
                };
                (sign, body)
            }
            _ => {
                return Err(ErrorKind::IllegalFormat.into());
            }
        };
        let pad = width.saturating_sub(sign.len() + body.chars().count());
        let field = if left {
            format!("{sign}{body}{}", " ".repeat(pad))
        } else if zero {
            format!("{sign}{}{body}", "0".repeat(pad))
        } else {
            format!("{}{sign}{body}", " ".repeat(pad))
        };
        out.extend(field.encode_utf16());
    }
    Ok(string(out))
}

/// `x` with `digits` decimals, rounded to the nearest; a value exactly
/// half way goes toward zero, as on hardware (`%.0F` of 2.5 and 3.5 gave
/// 2 and 3, of 2.7 gave 3).
fn fixed(x: f64, digits: usize) -> String {
    // Every double has a finite decimal expansion of at most 1074 digits.
    let exact = format!("{x:.1100}");
    let point = exact.find('.').unwrap_or(exact.len());
    let rest = exact.get(point + 1 + digits..).unwrap_or("");
    if rest.starts_with('5') && rest[1..].bytes().all(|b| b == b'0') {
        let end = if digits == 0 {
            point
        } else {
            point + 1 + digits
        };
        return exact[..end].to_string();
    }
    format!("{x:.digits$}")
}

// ---- colors ----

/// A component, clamped to 0 to 255.
fn component(v: &Value) -> Result<u32> {
    Ok(v.to_int()?.clamp(0, 255) as u32)
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

/// A component from 0 to 1.0, scaled to 0-255 and rounded down (checked
/// on hardware: 0.5 gives &H7F).
fn unit_component(v: &Value) -> Result<u32> {
    let x = v.to_real()?;
    if !(0.0..=1.0).contains(&x) {
        return Err(ErrorKind::OutOfRange.into());
    }
    Ok((x * 255.0) as u32)
}

/// `RGBF`: `RGB` with components from 0 to 1.0.
fn rgbf(args: &[Value]) -> Result<Value> {
    match args {
        [r, g, b] => Ok(argb(
            255,
            unit_component(r)?,
            unit_component(g)?,
            unit_component(b)?,
        )),
        [a, r, g, b] => Ok(argb(
            unit_component(a)?,
            unit_component(r)?,
            unit_component(g)?,
            unit_component(b)?,
        )),
        [color, parts @ ..] => {
            let mut c = color.to_int()? as u32;
            for (i, part) in parts.iter().enumerate() {
                if !matches!(part, Value::Default) {
                    let shift = 24 - 8 * i as u32;
                    c = c & !(0xFF << shift) | unit_component(part)? << shift;
                }
            }
            Ok(Value::Int(c as i32))
        }
        [] => unreachable!("arity checked by the caller"),
    }
}

/// `RGBF color OUT R,G,B` or `RGBF color OUT A,R,G,B`, from 0 to 1.0.
fn rgbf_out(args: &[Value], outs: usize) -> Result<Vec<Value>> {
    Ok(rgb_out(args, outs)?
        .into_iter()
        .map(|v| match v {
            Value::Int(n) => Value::Real(f64::from(n) / 255.0),
            v => v,
        })
        .collect())
}

/// `HSV(hue,saturation,value[,alpha])`, with the hue in degrees (wrapping
/// at 360) and the rest from 0 to 255; `HSVF` takes everything from 0 to
/// 1.0, the hue wrapping at 1.
// UNVERIFIED: how SmileBASIC 4 rounds the resulting components.
fn hsv(args: &[Value], unit: bool) -> Result<Value> {
    let part = |v: &Value| -> Result<f64> {
        let x = v.to_real()?;
        let max = if unit { 1.0 } else { 255.0 };
        if (0.0..=max).contains(&x) {
            Ok(x / max)
        } else {
            Err(ErrorKind::OutOfRange.into())
        }
    };
    let hue = args[0].to_real()?;
    let hue = if unit { hue * 360.0 } else { hue }.rem_euclid(360.0);
    let (s, v) = (part(&args[1])?, part(&args[2])?);
    let alpha = match args.get(3) {
        Some(a) => part(a)?,
        None => 1.0,
    };
    let h6 = hue / 60.0;
    let f = h6 - h6.floor();
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match h6.floor() as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let byte = |x: f64| (x * 255.0).round().clamp(0.0, 255.0) as u32;
    Ok(argb(byte(alpha), byte(r), byte(g), byte(b)))
}

/// `HSV color OUT H,S,V[,A]`: the hue in degrees from 0 to 360 and the
/// rest from 0 to 255; `HSVF` gives everything from 0 to 1.0.
fn hsv_out(args: &[Value], outs: usize, unit: bool) -> Result<Vec<Value>> {
    let c = args[0].to_int()? as u32;
    let channel = |shift: u32| f64::from(c >> shift & 0xFF) / 255.0;
    let (a, r, g, b) = (channel(24), channel(16), channel(8), channel(0));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let hue = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    let mut values = if unit {
        [hue / 360.0, s, max, a].map(Value::Real).to_vec()
    } else {
        [hue, s * 255.0, max * 255.0, a * 255.0]
            .map(|x| Value::Int(x.round() as i32))
            .to_vec()
    };
    match outs {
        3 => {
            values.truncate(3);
            Ok(values)
        }
        4 => Ok(values),
        // Fewer results than H, S and V, as on hardware.
        _ => Err(ErrorKind::WrongNumberOfOutArguments.into()),
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
            Kind::Data(_) => panic!("{name} reads DATA"),
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
        let too_big = [Value::Int(256), Value::Int(0), Value::Int(-1)];
        assert_eq!(int(call("RGB", &too_big).unwrap()), 0xFFFF0000);
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

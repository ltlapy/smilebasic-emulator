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

/// A new string for a function to give; what it takes goes on the memory
/// estimate.
fn string(s: Str) -> Value {
    crate::memory::charge(crate::memory::string_bytes(s.len()));
    Value::Str(new_str(s))
}

/// An optional argument: missing or omitted (`F(A,,B)`) reads as `None`.
fn opt(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !matches!(v, Value::Default))
}

fn index_arg(v: &Value) -> Result<usize> {
    usize::try_from(v.to_int()?).map_err(|_| Error::new(ErrorKind::OutOfRange).of_value(v))
}

fn real_fn(args: &[Value], f: fn(f64) -> f64) -> Result<Value> {
    Ok(Value::Real(f(args[0].to_real()?)))
}

/// Arguments outside a function's domain raise Out of range: SQR below 0,
/// LOG at 0 or below, ASIN and ACOS past 1.
fn checked_real(args: &[Value], f: fn(f64) -> Option<f64>) -> Result<Value> {
    f(args[0].to_real()?)
        .map(Value::Real)
        .ok_or_else(|| Error::new(ErrorKind::OutOfRange).of_value(&args[0]))
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
        return Err(Error::new(ErrorKind::OutOfRange).of_arg(0));
    }
    match opt(args, 1) {
        None => Ok(Value::Real(x.ln())),
        Some(base) => {
            let base = base.to_real()?;
            if base <= 0.0 || base == 1.0 {
                return Err(Error::new(ErrorKind::OutOfRange).of_arg(1));
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
        return Err(Error::new(ErrorKind::OutOfRange).of_arg(1));
    }
    let text = match &args[0] {
        Value::Real(x) => crate::value::format_g(*x).encode_utf16().collect(),
        number => format_number(number),
    };
    Ok(string(pad_left(text, digits, ' ')))
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

/// Reads a number as VAL and INPUT do: decimal, `&H` hex or `&B` binary,
/// whose digits take underscores as in a program (`&H_FF_`), though at
/// least one digit is needed. `None` if the text is not a number.
pub fn parse_number(text: &[u16]) -> Option<Value> {
    let s = String::from_utf16_lossy(text);
    let s = s.as_str();
    let upper = s.to_ascii_uppercase();
    let radix = |digits: &str, radix| u32::from_str_radix(&digits.replace('_', ""), radix).ok();
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
    let code = u16::try_from(args[0].to_int()?)
        .map_err(|_| Error::new(ErrorKind::OutOfRange).of_arg(0))?;
    Ok(string(vec![code]))
}

fn asc(args: &[Value]) -> Result<Value> {
    let s = text(&args[0])?;
    let first = (s.first()).ok_or_else(|| Error::new(ErrorKind::OutOfRange).of_arg(0))?;
    Ok(Value::Int(i32::from(*first)))
}

/// A start past the end gives "" and counts are clamped, while negative
/// positions or counts are errors.
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
    let found = find_text(&haystack[start..], &needle);
    Ok(Value::Int(found.map_or(-1, |i| (start + i) as i32)))
}

/// The first place `needle` starts in `haystack`, in time linear in their
/// lengths (Knuth-Morris-Pratt): comparing at each place in turn takes
/// their product for text such as "AAAA...AB".
fn find_text(haystack: &[u16], needle: &[u16]) -> Option<usize> {
    let Some(&first) = needle.first() else {
        return Some(0);
    };
    if needle.len() > haystack.len() {
        return None;
    }
    if needle.len() == 1 {
        return haystack.iter().position(|&c| c == first);
    }
    // For each length of `needle` matched so far, the longest shorter
    // start of it that the match also ends with.
    let mut short = [0usize; 16];
    let mut long = Vec::new();
    let border = match short.get_mut(..needle.len()) {
        Some(border) => border,
        None => {
            long.resize(needle.len(), 0);
            &mut long[..]
        }
    };
    let mut k = 0;
    for i in 1..needle.len() {
        while k > 0 && needle[i] != needle[k] {
            k = border[k - 1];
        }
        if needle[i] == needle[k] {
            k += 1;
        }
        border[i] = k;
    }
    let mut k = 0;
    for (i, &c) in haystack.iter().enumerate() {
        while k > 0 && c != needle[k] {
            k = border[k - 1];
        }
        if c == needle[k] {
            k += 1;
            if k == needle.len() {
                return Some(i + 1 - k);
            }
        }
    }
    None
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
    let chars = s.len() - (end - start) + replacement.len();
    crate::memory::request(crate::memory::string_bytes(chars))?;
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
/// hardware). The whole result has to fit in [`MOST_FORMATTED`].
fn format(args: &[Value]) -> Result<Value> {
    let fmt = text(&args[0])?;
    let mut value_at = 0;
    let mut out: Str = Vec::new();
    let at = |i: usize| fmt.get(i).and_then(|&c| char::from_u32(c.into()));
    let mut i = 0;
    while i < fmt.len() {
        if at(i) != Some('%') {
            push_formatted(&mut out, fmt[i])?;
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
            // Digits past any size are over the limit, not absent.
            (*i > start).then(|| {
                String::from_utf16_lossy(&fmt[start..*i])
                    .parse::<usize>()
                    .unwrap_or(usize::MAX)
            })
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
            push_formatted(&mut out, u16::from(b'%'))?;
            continue;
        }
        value_at += 1;
        let Some(value) = args.get(value_at) else {
            return Err(Error::with_detail(
                ErrorKind::WrongNumberOfArguments,
                "too few values",
            ));
        };
        let room = MOST_FORMATTED - out.len();
        let pad = Padding {
            width,
            left,
            zero,
            room,
        };
        let done = match conversion {
            // The precision only limits the string: it is no size of its own.
            Some('S') => text(value).and_then(|s| {
                let shown = &s[..precision.map_or(s.len(), |p| p.min(s.len()))];
                pad.apply(&mut out, "", shown)
            }),
            Some(c @ ('D' | 'X' | 'B' | 'F')) => {
                format_numeric(c, value, (plus, space), precision, pad, &mut out)
            }
            _ => return Err(ErrorKind::IllegalFormat.into()),
        };
        done.map_err(|e| e.of_arg(value_at))?;
    }
    Ok(string(out))
}

/// The most characters FORMAT$ can give: it writes into a buffer of this
/// size.
const MOST_FORMATTED: usize = 1024;

/// One character of the format string itself, whose overflow has no
/// argument to name.
#[inline]
fn push_formatted(out: &mut Str, c: u16) -> Result<()> {
    if out.len() >= MOST_FORMATTED {
        return Err(Error::with_detail(
            ErrorKind::StringTooLong,
            format!("over {MOST_FORMATTED} characters"),
        ));
    }
    out.push(c);
    Ok(())
}

/// How one conversion of FORMAT$ fills its width, and the room left.
#[derive(Clone, Copy)]
struct Padding {
    width: usize,
    left: bool,
    zero: bool,
    room: usize,
}

impl Padding {
    /// `sign` and `body` padded to the width, if that fits. A width too
    /// large to fit is refused here, before anything is made of it.
    fn apply(self, out: &mut Str, sign: &str, body: &[u16]) -> Result<()> {
        let filled = sign.len() + body.len();
        let total = self.width.max(filled);
        if total > self.room {
            return Err(field_too_long(total));
        }
        let pad = total - filled;
        let sign = sign.encode_utf16();
        if self.left {
            out.extend(sign);
            out.extend_from_slice(body);
            out.resize(out.len() + pad, u16::from(b' '));
        } else if self.zero {
            out.extend(sign);
            out.resize(out.len() + pad, u16::from(b'0'));
            out.extend_from_slice(body);
        } else {
            out.resize(out.len() + pad, u16::from(b' '));
            out.extend(sign);
            out.extend_from_slice(body);
        }
        Ok(())
    }
}

/// FORMAT$'s `%D`, `%X`, `%B` and `%F`.
fn format_numeric(
    conversion: char,
    value: &Value,
    (plus, space): (bool, bool),
    precision: Option<usize>,
    pad: Padding,
    out: &mut Str,
) -> Result<()> {
    if !value.is_number() {
        return Err(ErrorKind::TypeMismatch.into());
    }
    if conversion == 'F' {
        let negative = value.to_real()? < 0.0;
        let digits = precision.unwrap_or(6);
        // At least a digit and the point before them: no need to make what
        // will be refused.
        if digits.saturating_add(2) >= pad.room {
            return Err(field_too_long(digits.saturating_add(2)));
        }
        let body = fixed(value.to_real()?.abs(), digits);
        let sign = match negative {
            true => "-",
            false if plus => "+",
            false if space => " ",
            false => "",
        };
        // It is written by a routine that wants room for a character more.
        let pad = Padding {
            room: pad.room.saturating_sub(1),
            ..pad
        };
        let body: Str = body.encode_utf16().collect();
        return pad.apply(out, sign, &body);
    }
    let n = value.to_int()?;
    let (sign, digits) = match conversion {
        'D' => (
            match n {
                ..0 => "-",
                _ if plus => "+",
                _ if space => " ",
                _ => "",
            },
            i64::from(n).unsigned_abs().to_string(),
        ),
        'X' => ("", format!("{:X}", n as u32)),
        _ => ("", format!("{:b}", n as u32)),
    };
    // Zero has no digits of its own, only the one the least precision of 1
    // asks for; `%D` shows no sign for it then either.
    let digits = if n == 0 { "" } else { digits.as_str() };
    let sign = if n == 0 && precision == Some(0) {
        ""
    } else {
        sign
    };
    let least = precision.unwrap_or(1);
    if least > pad.room {
        return Err(field_too_long(least));
    }
    let mut body: Str = vec![u16::from(b'0'); least.saturating_sub(digits.len())];
    body.extend(digits.encode_utf16());
    // A precision says how many zeros there are.
    let pad = Padding {
        zero: pad.zero && precision.is_none(),
        ..pad
    };
    pad.apply(out, sign, &body)
}

fn field_too_long(chars: usize) -> Error {
    Error::with_detail(ErrorKind::StringTooLong, format!("{chars} characters"))
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
/// on hardware: 0.5 gives &H7F). Those outside are clamped.
fn unit_component(v: &Value) -> Result<u32> {
    let x = v.to_real()?;
    Ok((x.clamp(0.0, 1.0) * 255.0) as u32)
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
    let total = dims.iter().fold(1usize, |acc, &d| acc.saturating_mul(d));
    crate::memory::request(crate::memory::array_bytes(elem, total))?;
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

/// Before an element goes onto `len` others of `size` bytes. An array's
/// block changes size each 64 bytes, and the new one has to fit beside the
/// old. The element itself is looked at when the VM next counts memory.
#[inline]
fn room_for_one(len: usize, size: usize, elem: ElemType) -> Result<()> {
    if len.wrapping_mul(size) & 63 == 0 {
        return next_block(elem, len);
    }
    Ok(())
}

#[inline(never)]
fn next_block(elem: ElemType, len: usize) -> Result<()> {
    crate::memory::grow(
        crate::memory::array_bytes(elem, len),
        crate::memory::array_bytes(elem, len + 1),
    )
}

fn put(target: &Value, value: Value, end: End) -> Result<()> {
    match target {
        Value::Str(s) => {
            let piece = text(&value)?;
            let mut s = s.borrow_mut();
            crate::memory::grow(
                crate::memory::string_bytes(s.len()),
                crate::memory::string_bytes(s.len() + piece.len()),
            )?;
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
                ArrayData::Int(v) => {
                    let n = value.to_stored_int()?;
                    room_for_one(v.len(), 4, ElemType::Int)?;
                    v.insert(at, n);
                }
                ArrayData::Real(v) => {
                    let x = value.to_real()?;
                    room_for_one(v.len(), 8, ElemType::Real)?;
                    v.insert(at, x);
                }
                ArrayData::Str(v) => {
                    let s = value.into_str()?;
                    room_for_one(v.len(), 8, ElemType::Str)?;
                    v.insert(at, s);
                }
            }
            a.dims[0] = a.len();
        }
        _ => return Err(ErrorKind::TypeMismatch.into()),
    }
    Ok(())
}

/// Before an element comes off `len` others of `size` bytes: whether the
/// array may shrink. Its block changes size each 64 bytes, and a smaller
/// block is a new one that has to fit beside the old.
#[inline]
fn can_shrink(len: usize, size: usize, elem: ElemType) -> Result<bool> {
    if (len - 1).wrapping_mul(size) & 63 == 0 {
        return smaller_block(elem, len);
    }
    Ok(true)
}

#[inline(never)]
fn smaller_block(elem: ElemType, len: usize) -> Result<bool> {
    crate::memory::shrink(
        crate::memory::array_bytes(elem, len),
        crate::memory::array_bytes(elem, len - 1),
    )
}

/// POP or SHIFT of what cannot shrink: the length stays and there is no
/// error, but the element is still given, and SHIFT has still moved the
/// rest down, leaving the last one twice.
#[cold]
fn take_keeping_length<T: Clone>(v: &mut [T], end: End) -> T {
    let last = v.len() - 1;
    match end {
        End::Back => v[last].clone(),
        End::Front => {
            let first = v[0].clone();
            v.rotate_left(1);
            if last > 0 {
                v[last] = v[last - 1].clone();
            }
            first
        }
    }
}

fn take(target: &Value, end: End) -> Result<Value> {
    match target {
        Value::Str(s) => {
            let mut s = s.borrow_mut();
            let len = s.len();
            if len == 0 {
                return Err(ErrorKind::SubscriptOutOfRange.into());
            }
            // A string's block changes size each 64 characters.
            if (len - 1) & 63 == 0
                && !crate::memory::shrink(
                    crate::memory::string_bytes(len),
                    crate::memory::string_bytes(len - 1),
                )?
            {
                return Ok(string(vec![take_keeping_length(&mut s, end)]));
            }
            let c = match end {
                End::Back => s[len - 1],
                End::Front => s.remove(0),
            };
            if let End::Back = end {
                s.truncate(len - 1);
            }
            Ok(string(vec![c]))
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
                ArrayData::Int(v) => {
                    if !can_shrink(v.len(), 4, ElemType::Int)? {
                        return Ok(Value::Int(take_keeping_length(v, end)));
                    }
                    Value::Int(v.remove(at))
                }
                ArrayData::Real(v) => {
                    if !can_shrink(v.len(), 8, ElemType::Real)? {
                        return Ok(Value::Real(take_keeping_length(v, end)));
                    }
                    Value::Real(v.remove(at))
                }
                ArrayData::Str(v) => {
                    if !can_shrink(v.len(), 8, ElemType::Str)? {
                        return Ok(Value::Str(take_keeping_length(v, end)));
                    }
                    Value::Str(v.remove(at))
                }
            };
            a.dims[0] = a.len();
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
        return Err(Error::new(ErrorKind::OutOfRange).of_arg(2));
    }
    let end = start.saturating_add(count).min(a.len());
    for i in start..end {
        a.set(i, args[1].clone()).map_err(|e| e.of_arg(1))?;
    }
    Ok(())
}

/// Used by the compiler to fold constant expressions.
pub fn is_constant_function(name: &str) -> bool {
    // The reference allows these in CONST and DATA.
    matches!(
        name,
        "CHR$" | "DEG" | "RAD" | "RGB" | "RGBF" | "HSV" | "HSVF"
    )
}

/// Constant arguments a function rejects when the program is compiled,
/// though it takes the same values at run time: `RGBF(1.1,0,0)` and
/// `RGBF(1,-1,0)` stop the program before it runs, while `RGBF(R,0,0)`
/// clamps R (checked on hardware).
pub fn check_constant_args(name: &str, args: &[Value]) -> Result<()> {
    let over = |v: &Value| v.to_real().is_ok_and(|x| !(0.0..=1.0).contains(&x));
    if name == "RGBF" && matches!(args.len(), 3 | 4) && args.iter().any(over) {
        return Err(ErrorKind::OutOfRange.into());
    }
    Ok(())
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
            as_string(call("STR$", &[Value::Real(2.0 / 3.0)]).unwrap()),
            "0.666667"
        );
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
        // TEST56.
        assert_eq!(as_int(call("VAL", &[s("&H_FF")]).unwrap()), 255);
        assert_eq!(as_int(call("VAL", &[s("&HFF_")]).unwrap()), 255);
        assert_eq!(as_int(call("VAL", &[s("1_000")]).unwrap()), 0);
        assert_eq!(as_int(call("VAL", &[s("&H_")]).unwrap()), 0);
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

    /// With little memory free, by a count just made.
    fn little_free(free: usize) {
        crate::memory::counted_bytes(crate::memory::TOTAL as usize - free, true);
    }

    /// With plenty free, by a count made a while ago.
    fn plenty_free() {
        crate::memory::counted_bytes(0, false);
    }

    fn int_array(len: usize) -> Value {
        let a = call("ARRAY%", &[Value::Int(len as i32)]).unwrap();
        let Value::Array(array) = &a else { panic!() };
        let mut array = array.borrow_mut();
        for i in 0..len {
            array.set(i, Value::Int(i as i32 + 1)).unwrap();
        }
        drop(array);
        a
    }

    fn one(name: &str, a: &Value) -> Value {
        call(name, std::slice::from_ref(a)).unwrap()
    }

    #[test]
    fn pop_with_no_room_for_a_smaller_block_keeps_the_length() {
        // 40001 ints, whose next size down is another block.
        let a = int_array(40001);
        little_free(1000);
        assert_eq!(as_int(one("POP", &a)), 40001);
        assert_eq!(as_int(one("LEN", &a)), 40001);
        little_free(1000);
        assert_eq!(as_int(one("POP", &a)), 40001);
        assert_eq!(as_int(one("LEN", &a)), 40001);
        // Within a block, an element comes off with no new block.
        little_free(1000);
        call("PUSH", &[a.clone(), Value::Int(9)]).unwrap();
        assert_eq!(as_int(one("POP", &a)), 9);
        assert_eq!(as_int(one("LEN", &a)), 40001);
        // With room it does shrink.
        plenty_free();
        assert_eq!(as_int(one("POP", &a)), 40001);
        assert_eq!(as_int(one("LEN", &a)), 40000);
    }

    #[test]
    fn taking_asks_for_a_count_before_changing_anything() {
        let a = int_array(40001);
        // What is known says it may not fit, with no count behind it.
        crate::memory::counted_bytes(crate::memory::TOTAL as usize, false);
        for name in ["POP", "SHIFT"] {
            let e = call(name, std::slice::from_ref(&a)).unwrap_err();
            assert_eq!(e.kind, ErrorKind::MemoryCheck);
        }
        let Value::Array(array) = &a else { panic!() };
        let array = array.borrow();
        assert_eq!(array.len(), 40001);
        assert_eq!(as_int(array.get(0)), 1);
        assert_eq!(as_int(array.get(40000)), 40001);
    }

    #[test]
    fn shift_with_no_room_moves_the_rest_down_and_keeps_the_length() {
        let a = int_array(40001);
        little_free(1000);
        assert_eq!(as_int(one("SHIFT", &a)), 1);
        let Value::Array(array) = &a else { panic!() };
        let array = array.borrow();
        assert_eq!(array.len(), 40001);
        assert_eq!(as_int(array.get(0)), 2);
        assert_eq!(as_int(array.get(39999)), 40001);
        assert_eq!(as_int(array.get(40000)), 40001);
    }

    #[test]
    fn string_pop_and_shift_with_no_room_keep_the_length() {
        let chars = 64 * 800 + 1;
        let mut text = vec![u16::from(b'A'); chars];
        text[0] = u16::from(b'F');
        text[chars - 1] = u16::from(b'L');
        let v = Value::Str(crate::value::new_str(text));
        little_free(1000);
        assert_eq!(as_string(one("POP", &v)), "L");
        little_free(1000);
        assert_eq!(as_string(one("SHIFT", &v)), "F");
        let Value::Str(s) = &v else { panic!() };
        let s = s.borrow();
        assert_eq!(s.len(), chars);
        assert_eq!(s[0], u16::from(b'A'));
        assert_eq!(s[chars - 2], u16::from(b'L'));
        assert_eq!(s[chars - 1], u16::from(b'L'));
    }

    #[test]
    fn small_blocks_always_shrink() {
        let a = int_array(1601);
        little_free(0);
        assert_eq!(as_int(one("POP", &a)), 1601);
        assert_eq!(as_int(one("LEN", &a)), 1600);
    }

    #[test]
    fn remove_with_no_room_moves_the_rest_down_then_is_out_of_memory() {
        let a = int_array(40001);
        little_free(1000);
        let e = call("REMOVE", &[a.clone(), Value::Int(0), Value::Int(1)]).unwrap_err();
        assert_eq!(e.kind, ErrorKind::OutOfMemory);
        let Value::Array(array) = &a else { panic!() };
        assert_eq!(array.borrow().len(), 40001);
        assert_eq!(as_int(array.borrow().get(0)), 2);
        assert_eq!(as_int(array.borrow().get(39999)), 40001);
        assert_eq!(as_int(array.borrow().get(40000)), 0);
        // Asked for a count, it has changed nothing.
        plenty_free();
        let b = int_array(40001);
        crate::memory::counted_bytes(crate::memory::TOTAL as usize, false);
        let e = call("REMOVE", &[b.clone(), Value::Int(0)]).unwrap_err();
        assert_eq!(e.kind, ErrorKind::MemoryCheck);
        assert_eq!(as_int(one("LEN", &b)), 40001);
        plenty_free();
        assert_eq!(as_int(one("SHIFT", &b)), 1);
    }

    #[test]
    fn resize_with_no_room_for_a_smaller_block_is_out_of_memory() {
        let a = int_array(40001);
        little_free(1000);
        let e = call("RESIZE", &[a.clone(), Value::Int(40000)]).unwrap_err();
        assert_eq!(e.kind, ErrorKind::OutOfMemory);
        assert_eq!(as_int(one("LEN", &a)), 40001);
        // A block small enough is not looked at.
        little_free(1000);
        call("RESIZE", &[a.clone(), Value::Int(100)]).unwrap();
        assert_eq!(as_int(one("LEN", &a)), 100);
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

    #[test]
    fn instr_finds_what_comparing_at_each_place_finds() {
        let naive = |h: &[u16], n: &[u16]| {
            if n.is_empty() {
                Some(0)
            } else {
                h.windows(n.len()).position(|w| w == n)
            }
        };
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        for _ in 0..200_000 {
            // Few letters, for many partial matches. The lengths go past
            // the needle's table on the stack.
            let letters = 1 + next(3);
            let hay_len = next(60);
            let needle_len = if next(4) == 0 { next(40) } else { next(6) };
            let hay: Vec<u16> = (0..hay_len).map(|_| 65 + next(letters) as u16).collect();
            let needle: Vec<u16> = match next(3) {
                // A piece of the text, so that some are found.
                0 if hay_len > 0 => {
                    let from = next(hay_len) as usize;
                    let to = (from + needle_len as usize).min(hay.len());
                    hay[from..to].to_vec()
                }
                _ => (0..needle_len).map(|_| 65 + next(letters) as u16).collect(),
            };
            assert_eq!(
                find_text(&hay, &needle),
                naive(&hay, &needle),
                "{hay:?} {needle:?}"
            );
        }
    }

    #[test]
    fn instr_start_offsets_and_empty_needles() {
        let instr = |args: &[Value]| as_int(call("INSTR", args).unwrap());
        let (i, abab) = (Value::Int, s("ABAB"));
        assert_eq!(instr(&[abab.clone(), s("B")]), 1);
        assert_eq!(instr(&[i(2), abab.clone(), s("B")]), 3);
        assert_eq!(instr(&[i(2), abab.clone(), s("BA")]), -1);
        assert_eq!(instr(&[i(4), abab.clone(), s("B")]), -1);
        assert_eq!(instr(&[i(5), abab.clone(), s("B")]), -1);
        assert_eq!(instr(&[abab.clone(), s("")]), 0);
        assert_eq!(instr(&[i(3), abab.clone(), s("")]), 3);
        assert_eq!(instr(&[i(4), abab.clone(), s("")]), 4);
        assert_eq!(instr(&[i(5), abab.clone(), s("")]), -1);
        assert_eq!(instr(&[abab.clone(), s("ABABA")]), -1);
        assert_eq!(instr(&[s(""), s("")]), 0);
    }

    #[test]
    fn instr_of_many_partial_matches_finishes() {
        // Comparing at each place would take some 10^11 steps.
        let mut needle = vec![65; 100_000];
        let hay = vec![65; 1_000_000];
        needle.push(66);
        assert_eq!(find_text(&hay, &needle), None);
        let mut hay = hay;
        hay.extend_from_slice(&needle);
        assert_eq!(find_text(&hay, &needle), Some(1_000_000));
    }

    #[test]
    fn format_with_too_long_a_field_is_an_error() {
        let format = |f: &str| call("FORMAT$", &[s(f), Value::Real(1.5)]);
        let digits = |f: &str| as_string(format(f).unwrap()).len();
        assert_eq!(digits("%.1000F"), 1002);
        assert_eq!(digits("%.1021F"), 1023);
        assert_eq!(digits("%1000D"), 1000);
        assert_eq!(digits("%1024D"), 1024);
        for f in [
            "%.1022F",
            "%1024F",
            "%.1023F",
            "%1025D",
            "%.10000F",
            "%.99999F",
            "%.18446744073709551615F",
            "%.99999999999999999999999F",
            "%1048576D",
            "%1048576F",
            "%99999999999999999999999D",
        ] {
            let e = format(f).unwrap_err();
            assert_eq!(e.kind, ErrorKind::StringTooLong, "{f}");
            assert!(e.detail.is_some(), "{f}");
            assert_eq!(e.arg, Some(1), "{f}");
        }
    }

    /// The limit is on the result as a whole (TEST57).
    #[test]
    fn format_limits_the_whole_result() {
        let chars = |n: usize| s(&"A".repeat(n));
        let len = |args: &[Value]| as_string(call("FORMAT$", args).unwrap()).len();
        let refused = |args: &[Value]| {
            let e = call("FORMAT$", args).unwrap_err();
            assert_eq!(e.kind, ErrorKind::StringTooLong);
            e.arg
        };
        let (one, two) = (Value::Int(1), Value::Int(2));
        assert_eq!(len(&[s("%1000D%24D"), one.clone(), two.clone()]), 1024);
        assert_eq!(refused(&[s("%1000D%25D"), one.clone(), two]), Some(2));
        assert_eq!(len(&[s("%S"), chars(1024)]), 1024);
        assert_eq!(refused(&[s("%S"), chars(1025)]), Some(1));
        // The format's own text has no argument to blame.
        assert_eq!(len(&[chars(1024)]), 1024);
        assert_eq!(refused(&[chars(1025)]), None);
        let percents = s(&"%%".repeat(1025));
        assert_eq!(refused(&[percents]), None);
        assert_eq!(refused(&[s(&format!("{}%%", "A".repeat(1024)))]), None);
        assert_eq!(refused(&[s("%1024D%%"), one]), None);
    }

    /// A precision is the least number of digits,
    /// and takes the place of the `0` flag.
    #[test]
    fn format_takes_a_precision_on_integers() {
        let show = |f: &str, v: i32| as_string(call("FORMAT$", &[s(f), Value::Int(v)]).unwrap());
        for (f, v, shown) in [
            ("[%.3D]", 5, "[005]"),
            ("[%5.3D]", 5, "[  005]"),
            ("[%-5.3D]", 5, "[005  ]"),
            ("[%.3D]", -5, "[-005]"),
            ("[%+.3D]", 5, "[+005]"),
            ("[% .3D]", 5, "[ 005]"),
            ("[%.1D]", 1234, "[1234]"),
            ("[%.0D]", 5, "[5]"),
            // Zero at a precision of 0 is nothing at all.
            ("[%.0D]", 0, "[]"),
            ("[%+.0D]", 0, "[]"),
            ("[%3.0D]", 0, "[   ]"),
            ("[%.D]", 0, "[]"),
            ("[%D]", 0, "[0]"),
            ("[%+D]", 0, "[+0]"),
            ("[%.1D]", 0, "[0]"),
            ("[%.2D]", 0, "[00]"),
            // A precision takes the place of the 0 flag.
            ("[%05D]", 5, "[00005]"),
            ("[%05.3D]", 5, "[  005]"),
            ("[%05.0D]", 0, "[     ]"),
            ("[%-05D]", 5, "[5    ]"),
            ("[%.4X]", 255, "[00FF]"),
            ("[%6.4X]", 255, "[  00FF]"),
            ("[%06.4X]", 255, "[  00FF]"),
            ("[%06X]", 255, "[0000FF]"),
            ("[%.0X]", 0, "[]"),
            ("[%X]", 0, "[0]"),
            ("[%.2X]", 0, "[00]"),
            ("[%.10X]", -1, "[00FFFFFFFF]"),
            ("[%.4B]", 5, "[0101]"),
            ("[%6.4B]", 5, "[  0101]"),
            ("[%06.4B]", 5, "[  0101]"),
            ("[%.0B]", 0, "[]"),
            ("[%B]", 0, "[0]"),
        ] {
            assert_eq!(show(f, v), shown, "{f} of {v}");
        }
        // It counts towards the limit, and is refused before it is made.
        assert_eq!(show("%.1024D", 5).len(), 1024);
        for f in [
            "%.1025D",
            "%.99999999999D",
            "%+.1024D",
            "%.1025X",
            "%.1025B",
        ] {
            let e = call("FORMAT$", &[s(f), Value::Int(5)]).unwrap_err();
            assert_eq!(e.kind, ErrorKind::StringTooLong, "{f}");
            assert_eq!(e.arg, Some(1), "{f}");
        }
    }

    #[test]
    fn format_pads_strings() {
        let abc = s("ABC");
        let show = |f: &str, v: &Value| as_string(call("FORMAT$", &[s(f), v.clone()]).unwrap());
        assert_eq!(show("[%5S]", &abc), "[  ABC]");
        assert_eq!(show("[%-5S]", &abc), "[ABC  ]");
        assert_eq!(show("[%05S]", &abc), "[00ABC]");
        assert_eq!(show("[%-05S]", &abc), "[ABC  ]");
        assert_eq!(show("[%.2S]", &abc), "[AB]");
        assert_eq!(show("[%5.2S]", &abc), "[   AB]");
        assert_eq!(show("[%.0S]", &abc), "[]");
        // The precision only limits the string: it can be any size.
        assert_eq!(show("[%.2000S]", &abc), "[ABC]");
        assert_eq!(show("[%.99999999S]", &abc), "[ABC]");
        // Not every code is a character of its own in Rust's strings.
        let lone = Value::Str(crate::value::new_str(vec![0xD800, 0x41]));
        let got = call("FORMAT$", &[s("%3S"), lone]).unwrap();
        let Value::Str(got) = got else { panic!() };
        assert_eq!(got.borrow().as_slice(), [0x20, 0xD800, 0x41]);
    }
}

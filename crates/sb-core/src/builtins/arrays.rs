//! Array commands: COPY, SORT, RSORT, INSERT, REMOVE, FIND, RESIZE, ARYOP
//! and RINGCOPY.

use std::cmp::Ordering;

use super::{Data, check_1d, index_arg, opt, string};
use crate::error::{Error, ErrorKind, Result};
use crate::value::{Array, ArrayData, ArrayRef, ElemType, Value};

fn out_of_range(what: &str) -> Error {
    Error::with_detail(ErrorKind::OutOfRange, what)
}

fn array(v: &Value) -> Result<ArrayRef> {
    v.clone().into_array()
}

fn is_source(v: &Value) -> bool {
    matches!(v, Value::Array(_) | Value::Str(_))
}

/// A count of elements: zero or more.
fn count(v: &Value) -> Result<usize> {
    index_arg(v)
}

/// `offset` and `count` within `len`: the count defaults to the rest.
fn span(len: usize, offset: Option<&Value>, n: Option<&Value>) -> Result<(usize, usize)> {
    let offset = offset.map_or(Ok(0), count)?;
    if offset > len {
        return Err(out_of_range("offset"));
    }
    let n = n.map_or(Ok(len - offset), count)?;
    if offset + n > len {
        return Err(out_of_range("count"));
    }
    Ok((offset, n))
}

/// Grows a one-dimensional array to `len` elements; others must already
/// be long enough.
fn grow(a: &mut Array, len: usize) -> Result<()> {
    if a.len() >= len {
        return Ok(());
    }
    if a.dims.len() > 1 {
        return Err(ErrorKind::SubscriptOutOfRange.into());
    }
    match &mut a.data {
        ArrayData::Int(v) => v.resize(len, 0),
        ArrayData::Real(v) => v.resize(len, 0.0),
        ArrayData::Str(v) => v.resize_with(len, || crate::value::new_str(Vec::new())),
    }
    a.dims = vec![len];
    Ok(())
}

/// Copies an element, so that a string array's copy has strings of its own.
fn element(a: &Array, i: usize) -> Value {
    match a.get(i) {
        Value::Str(s) => string(s.borrow().clone()),
        v => v,
    }
}

/// `COPY(array or string [,offset [,count]])`.
pub(super) fn copy_fn(args: &[Value]) -> Result<Value> {
    match &args[0] {
        Value::Str(s) => {
            let s = s.borrow();
            let (offset, n) = span(s.len(), opt(args, 1), opt(args, 2))?;
            Ok(string(s[offset..offset + n].to_vec()))
        }
        Value::Array(a) => {
            let a = a.borrow();
            check_1d(&a)?;
            let (offset, n) = span(a.len(), opt(args, 1), opt(args, 2))?;
            let mut out = Array::new(a.elem_type(), vec![n]);
            for i in 0..n {
                out.set(i, element(&a, offset + i))?;
            }
            Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(out))))
        }
        _ => Err(ErrorKind::TypeMismatch.into()),
    }
}

/// `COPY dest [,offset], source [[,offset], count]`, where the source is
/// an array, a string, or a `"@label"` of DATA to read into an array.
pub(super) fn copy(args: &[Value], _outs: usize, data: &mut Data) -> Result<Vec<Value>> {
    let at = if is_source(&args[1]) { 1 } else { 2 };
    let dest_offset = match at {
        2 => count(&args[1])?,
        _ => 0,
    };
    let source = args.get(at).ok_or(ErrorKind::IllegalFunctionCall)?;
    let rest = &args[at + 1..];
    if rest.len() > 2 {
        return Err(ErrorKind::IllegalFunctionCall.into());
    }
    let (offset, n) = match rest {
        [] => (None, None),
        [n] => (None, Some(n)),
        [offset, n] => (Some(offset), Some(n)),
        _ => unreachable!("checked above"),
    };
    match (&args[0], source) {
        (Value::Str(dest), Value::Str(source)) => {
            let piece = {
                let source = source.borrow();
                let (offset, n) = span(source.len(), offset, n)?;
                source[offset..offset + n].to_vec()
            };
            let mut dest = dest.borrow_mut();
            if dest_offset > dest.len() {
                return Err(out_of_range("offset"));
            }
            let end = (dest_offset + piece.len()).min(dest.len());
            dest.splice(dest_offset..end, piece);
            Ok(vec![])
        }
        (Value::Array(dest), Value::Str(label))
            if label.borrow().first() == Some(&u16::from(b'@')) =>
        {
            if offset.is_some() {
                return Err(ErrorKind::IllegalFunctionCall.into());
            }
            data.restore(source)?;
            let mut dest = dest.borrow_mut();
            let n = n.map_or(Ok(dest.len()), count)?;
            let values = (0..n).map(|_| data.read()).collect::<Result<Vec<_>>>()?;
            grow(&mut dest, dest_offset + n)?;
            for (i, v) in values.into_iter().enumerate() {
                dest.set(dest_offset + i, v)?;
            }
            Ok(vec![])
        }
        (Value::Array(dest), Value::Array(source)) => {
            let values: Vec<Value> = {
                let source = source.borrow();
                let (offset, n) = span(source.len(), offset, n)?;
                (offset..offset + n).map(|i| element(&source, i)).collect()
            };
            let mut dest = dest.borrow_mut();
            grow(&mut dest, dest_offset + values.len())?;
            for (i, v) in values.into_iter().enumerate() {
                dest.set(dest_offset + i, v)?;
            }
            Ok(vec![])
        }
        _ => Err(ErrorKind::TypeMismatch.into()),
    }
}

fn compare(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Str(a), Value::Str(b)) => a.borrow().cmp(&b.borrow()),
        _ => {
            let (a, b) = (a.to_real().unwrap_or(0.0), b.to_real().unwrap_or(0.0));
            a.total_cmp(&b)
        }
    }
}

/// SORT and RSORT: `[start, count,] key [, arrays...]`. The other arrays
/// are put in the key's new order. SORT keeps equal keys in order; RSORT
/// is SORT turned round, so it reverses them (checked on hardware).
fn sort(args: &[Value], descending: bool) -> Result<Vec<Value>> {
    let (start, n, arrays) = if matches!(args[0], Value::Array(_)) {
        (None, None, args)
    } else if args.len() >= 3 {
        (Some(&args[0]), Some(&args[1]), &args[2..])
    } else {
        return Err(ErrorKind::IllegalFunctionCall.into());
    };
    if arrays.len() > 8 {
        return Err(ErrorKind::IllegalFunctionCall.into());
    }
    let arrays: Vec<ArrayRef> = arrays.iter().map(array).collect::<Result<_>>()?;
    let len = arrays[0].borrow().len();
    let (start, n) = span(len, start, n)?;
    if arrays.iter().any(|a| a.borrow().len() < start + n) {
        return Err(ErrorKind::SubscriptOutOfRange.into());
    }
    let keys: Vec<Value> = {
        let key = arrays[0].borrow();
        (start..start + n).map(|i| key.get(i)).collect()
    };
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| compare(&keys[i], &keys[j]));
    if descending {
        order.reverse();
    }
    for a in &arrays {
        let mut a = a.borrow_mut();
        let values: Vec<Value> = order.iter().map(|&i| a.get(start + i)).collect();
        for (i, v) in values.into_iter().enumerate() {
            a.set(start + i, v)?;
        }
    }
    Ok(vec![])
}

pub(super) fn sort_up(args: &[Value], _outs: usize) -> Result<Vec<Value>> {
    sort(args, false)
}

pub(super) fn sort_down(args: &[Value], _outs: usize) -> Result<Vec<Value>> {
    sort(args, true)
}

/// A count that must be 1 or more, 1 when omitted.
fn positive(v: Option<&Value>) -> Result<usize> {
    match v {
        None => Ok(1),
        Some(v) => match count(v)? {
            0 => Err(out_of_range("count")),
            n => Ok(n),
        },
    }
}

pub(super) fn insert(args: &[Value], _outs: usize) -> Result<Vec<Value>> {
    let a = array(&args[0])?;
    let mut a = a.borrow_mut();
    check_1d(&a)?;
    let at = count(&args[1])?;
    if at > a.len() {
        return Err(out_of_range("position"));
    }
    let n = positive(opt(args, 2))?;
    let len = a.len();
    grow(&mut a, len + n)?;
    for i in (at..len).rev() {
        let v = a.get(i);
        a.set(i + n, v)?;
    }
    let fill = match (opt(args, 3), a.elem_type()) {
        (Some(v), _) => v.clone(),
        (None, ElemType::Str) => string(Vec::new()),
        (None, _) => Value::Int(0),
    };
    for i in 0..n {
        let v = match &fill {
            Value::Str(s) => string(s.borrow().clone()),
            v => v.clone(),
        };
        a.set(at + i, v)?;
    }
    Ok(vec![])
}

pub(super) fn remove(args: &[Value], _outs: usize) -> Result<Vec<Value>> {
    let a = array(&args[0])?;
    let mut a = a.borrow_mut();
    check_1d(&a)?;
    let at = count(&args[1])?;
    // A count of 0 removes nothing, even from an empty array.
    let n = opt(args, 2).map_or(Ok(1), count)?;
    if at + n > a.len() {
        return Err(out_of_range("position"));
    }
    match &mut a.data {
        ArrayData::Int(v) => drop(v.drain(at..at + n)),
        ArrayData::Real(v) => drop(v.drain(at..at + n)),
        ArrayData::Str(v) => drop(v.drain(at..at + n)),
    }
    let len = a.len();
    a.dims = vec![len];
    Ok(vec![])
}

/// Where `value` first is in the array, by element order.
fn find_in(args: &[Value]) -> Result<(Vec<usize>, Option<usize>)> {
    let a = array(&args[0])?;
    let a = a.borrow();
    let want = &args[1];
    let numeric = a.elem_type() != ElemType::Str;
    if numeric != want.is_number() {
        return Err(ErrorKind::TypeMismatch.into());
    }
    let found = (0..a.len()).find(|&i| compare(&a.get(i), want) == Ordering::Equal);
    Ok((a.dims.clone(), found))
}

pub(super) fn find_fn(args: &[Value]) -> Result<Value> {
    Ok(Value::Int(find_in(args)?.1.map_or(-1, |i| i as i32)))
}

pub(super) fn find(args: &[Value], outs: usize) -> Result<Vec<Value>> {
    let (dims, found) = find_in(args)?;
    if outs != dims.len() {
        return Err(ErrorKind::IllegalFunctionCall.into());
    }
    let Some(mut i) = found else {
        return Ok(vec![Value::Int(-1); outs]);
    };
    let mut indices = vec![0; dims.len()];
    for (d, &size) in dims.iter().enumerate().rev() {
        indices[d] = (i % size) as i32;
        i /= size;
    }
    Ok(indices.into_iter().map(Value::Int).collect())
}

/// `RESIZE array, n1 [, n2...]`: only the first size can change, and the
/// others are given as they are (checked on hardware).
pub(super) fn resize(args: &[Value], _outs: usize) -> Result<Vec<Value>> {
    let a = array(&args[0])?;
    let mut a = a.borrow_mut();
    let sizes: Vec<usize> = args[1..].iter().map(count).collect::<Result<_>>()?;
    if sizes.len() != a.dims.len() {
        return Err(ErrorKind::WrongNumberOfArguments.into());
    }
    if sizes[1..] != a.dims[1..] {
        return Err(ErrorKind::IllegalFunctionCall.into());
    }
    let len = sizes.iter().product();
    match &mut a.data {
        ArrayData::Int(v) => v.resize(len, 0),
        ArrayData::Real(v) => v.resize(len, 0.0),
        ArrayData::Str(v) => v.resize_with(len, || crate::value::new_str(Vec::new())),
    }
    a.dims = sizes;
    Ok(vec![])
}

/// An ARYOP parameter: a number, or an array read over and over.
enum Param {
    Number(f64),
    Array(Vec<f64>),
}

impl Param {
    fn new(v: &Value) -> Result<Param> {
        match v {
            Value::Array(a) => {
                let a = a.borrow();
                if a.elem_type() == ElemType::Str {
                    return Err(ErrorKind::TypeMismatch.into());
                }
                if a.is_empty() {
                    return Err(out_of_range("empty array"));
                }
                (0..a.len())
                    .map(|i| a.get(i).to_real())
                    .collect::<Result<_>>()
                    .map(Param::Array)
            }
            v => v.to_real().map(Param::Number),
        }
    }

    fn at(&self, i: usize) -> f64 {
        match self {
            Param::Number(x) => *x,
            Param::Array(v) => v[i % v.len()],
        }
    }
}

pub(super) fn aryop(args: &[Value], _outs: usize) -> Result<Vec<Value>> {
    let op = args[0].to_int()?;
    let wants_three = matches!(op, 4..=6);
    if !(0..=6).contains(&op) {
        return Err(out_of_range("operation"));
    }
    if (args.len() == 5) != wants_three {
        return Err(ErrorKind::IllegalFunctionCall.into());
    }
    let params: Vec<Param> = args[2..].iter().map(Param::new).collect::<Result<_>>()?;
    let result = array(&args[1])?;
    let mut result = result.borrow_mut();
    if result.elem_type() == ElemType::Str {
        return Err(ErrorKind::TypeMismatch.into());
    }
    for i in 0..result.len() {
        let p = |n: usize| params[n].at(i);
        let x = match op {
            0 => p(0) + p(1),
            1 => p(0) - p(1),
            2 => p(0) * p(1),
            3 => p(0) / p(1),
            4 => p(0) * p(1) + p(2),
            5 => p(0) * p(2) + p(1) * (1.0 - p(2)),
            // A plain comparison, the lower bound first: FINITI gives a
            // bound above the other to mean "at least" and "at most".
            // UNVERIFIED: that the lower bound wins when both apply, and
            // that NaN goes through.
            _ => {
                let (x, lo, hi) = (p(0), p(1), p(2));
                if x < lo {
                    lo
                } else if x > hi {
                    hi
                } else {
                    x
                }
            }
        };
        result.set(i, Value::Real(x))?;
    }
    Ok(vec![])
}

/// An array as channels of samples: a 2D array's first size counts the
/// channels.
fn channels(a: &Array) -> Result<(usize, usize)> {
    match a.dims[..] {
        [len] => Ok((1, len)),
        [ch, len] => Ok((ch, len)),
        _ => Err(ErrorKind::IllegalFunctionCall.into()),
    }
}

/// `RINGCOPY dest, offset, source [[,offset], count] [OUT end]`: copies
/// into `dest` going round to its start, and gives where to copy next.
pub(super) fn ringcopy(args: &[Value], outs: usize) -> Result<Vec<Value>> {
    if outs > 1 || args.len() > 5 {
        return Err(ErrorKind::IllegalFunctionCall.into());
    }
    let dest = array(&args[0])?;
    let source = array(&args[2])?;
    let (offset, n) = match &args[3..] {
        [] => (None, None),
        [n] => (None, Some(n)),
        [offset, n] => (Some(offset), Some(n)),
        _ => unreachable!("checked above"),
    };
    let samples: Vec<Vec<Value>> = {
        let source = source.borrow();
        let (ch, len) = channels(&source)?;
        // Reading past the source's end is an Illegal function call on
        // hardware, not Out of range as for COPY.
        let (offset, n) =
            span(len, offset, n).map_err(|_| Error::from(ErrorKind::IllegalFunctionCall))?;
        (0..ch)
            .map(|c| {
                (offset..offset + n)
                    .map(|i| element(&source, c * len + i))
                    .collect()
            })
            .collect()
    };
    let mut dest = dest.borrow_mut();
    let (ch, len) = channels(&dest)?;
    if len == 0 {
        return Err(out_of_range("empty array"));
    }
    if samples.len() != 1 && samples.len() != ch {
        return Err(ErrorKind::IllegalFunctionCall.into());
    }
    let start = count(&args[1])? % len;
    let n = samples[0].len();
    for c in 0..ch {
        let from = &samples[c.min(samples.len() - 1)];
        for (k, v) in from.iter().enumerate() {
            dest.set(c * len + (start + k) % len, v.clone())?;
        }
    }
    Ok(if outs == 1 {
        vec![Value::Int(((start + n) % len) as i32)]
    } else {
        vec![]
    })
}

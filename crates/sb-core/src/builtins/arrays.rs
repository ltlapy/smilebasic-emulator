//! Array commands: COPY, SORT, RSORT, INSERT, REMOVE, FIND, RESIZE, ARYOP
//! and RINGCOPY.

use std::cmp::Ordering;

use super::{Data, check_1d, index_arg, mismatch, opt, string};
use crate::error::{Error, ErrorKind, Result};
use crate::memory;
use crate::value::{Array, ArrayData, ArrayRef, ElemType, Value};

fn out_of_range(what: &str) -> Error {
    Error::with_detail(ErrorKind::OutOfRange, what)
}

/// An array argument; anything else is a `Type mismatch` naming it.
fn array(v: &Value) -> Result<ArrayRef> {
    match v {
        Value::Array(a) => Ok(a.clone()),
        _ => Err(mismatch(v)),
    }
}

fn is_source(v: &Value) -> bool {
    matches!(v, Value::Array(_) | Value::Str(_))
}

/// A count of elements: zero or more.
fn count(v: &Value) -> Result<usize> {
    index_arg(v)
}

/// COPY's source `offset` and `count` within `len`. Past the end is
/// Subscript out of range; the function form names the argument, the
/// command form does not. An offset at the end of a source that is not
/// empty, with no count, is past it too, while one with a count of 0
/// copies nothing. Offset 0 of an empty source copies nothing.
fn copy_span(
    len: usize,
    offset: Option<&Value>,
    n: Option<&Value>,
    named: bool,
) -> Result<(usize, usize)> {
    let past_end = |what, v: Option<&Value>| {
        let e = Error::with_detail(ErrorKind::SubscriptOutOfRange, what);
        match v {
            Some(v) if named => e.of_value(v),
            _ => e,
        }
    };
    let at = offset.map_or(Ok(0), count)?;
    if at > len || (at == len && len != 0 && offset.is_some() && n.is_none()) {
        return Err(past_end("offset", offset));
    }
    let size = n.map_or(Ok(len - at), count)?;
    if at + size > len {
        return Err(past_end("count", n));
    }
    Ok((at, size))
}

/// What growing an array to `len` elements needs of memory.
fn make_room(a: &Array, len: usize) -> Result<()> {
    let elem = a.elem_type();
    memory::grow(
        memory::array_bytes(elem, a.len()),
        memory::array_bytes(elem, len),
    )
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
            let (offset, n) = copy_span(s.len(), opt(args, 1), opt(args, 2), true)?;
            Ok(string(s[offset..offset + n].to_vec()))
        }
        Value::Array(a) => {
            let a = a.borrow();
            check_1d(&a)?;
            let (offset, n) = copy_span(a.len(), opt(args, 1), opt(args, 2), true)?;
            memory::request(memory::array_bytes(a.elem_type(), n))?;
            let mut out = Array::new(a.elem_type(), vec![n]);
            for i in 0..n {
                out.set(i, element(&a, offset + i))?;
            }
            Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(out))))
        }
        // SmileBASIC 4 names the first argument.
        v => Err(mismatch(v)),
    }
}

/// `COPY dest [,offset], source [[,offset], count]`, where the source is
/// an array, a string, or a `"@label"` of DATA to read into an array.
pub(super) fn copy(args: &[Value], _outs: usize, data: &mut Data) -> Result<Vec<Value>> {
    // SmileBASIC 4 checks the destination's type before the
    // rest, and names it.
    if !is_source(&args[0]) {
        return Err(mismatch(&args[0]));
    }
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
                let (offset, n) = copy_span(source.len(), offset, n, false)?;
                source[offset..offset + n].to_vec()
            };
            let mut dest = dest.borrow_mut();
            // Past the destination's end, spaces fill the gap.
            let len = dest.len().max(dest_offset);
            let end = (dest_offset + piece.len()).min(len);
            memory::take(memory::string_bytes(
                len - (end - dest_offset) + piece.len(),
            ))?;
            dest.resize(len, u16::from(b' '));
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
            if dest.dims.len() == 1 {
                // Before anything is read: this may run again.
                make_room(&dest, dest_offset + n)?;
            }
            let values = (0..n).map(|_| data.read()).collect::<Result<Vec<_>>>()?;
            grow(&mut dest, dest_offset + n)?;
            for (i, v) in values.into_iter().enumerate() {
                dest.set(dest_offset + i, v)?;
            }
            Ok(vec![])
        }
        (Value::Array(dest), Value::Array(source)) => {
            let len = source.borrow().len();
            let (offset, n) = copy_span(len, offset, n, false)?;
            let same = {
                let dest = dest.borrow();
                if dest.dims.len() == 1 && dest.len() < dest_offset + n {
                    make_room(&dest, dest_offset + n)?;
                }
                let elem = dest.elem_type();
                elem == source.borrow().elem_type() && elem != ElemType::Str
            };
            // Numbers into an array of their type go across as they are,
            // rather than as a value each: what the elements are set to
            // is the same, and an array copied into itself is read before
            // it is written, as `copy_within` does.
            if same {
                let to = dest_offset..dest_offset + n;
                if std::rc::Rc::ptr_eq(dest, source) {
                    let mut dest = dest.borrow_mut();
                    grow(&mut dest, to.end)?;
                    match &mut dest.data {
                        ArrayData::Int(v) => v.copy_within(offset..offset + n, to.start),
                        ArrayData::Real(v) => v.copy_within(offset..offset + n, to.start),
                        ArrayData::Str(_) => unreachable!("numbers only"),
                    }
                } else {
                    let source = source.borrow();
                    let mut dest = dest.borrow_mut();
                    grow(&mut dest, to.end)?;
                    match (&mut dest.data, &source.data) {
                        (ArrayData::Int(d), ArrayData::Int(s)) => {
                            d[to].copy_from_slice(&s[offset..offset + n])
                        }
                        (ArrayData::Real(d), ArrayData::Real(s)) => {
                            d[to].copy_from_slice(&s[offset..offset + n])
                        }
                        _ => unreachable!("numbers of one type"),
                    }
                }
                return Ok(vec![]);
            }
            let values: Vec<Value> = {
                let source = source.borrow();
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
// SmileBASIC 4 takes the form from the first
// argument: a number needs a count and an array after it, else Wrong
// number of arguments, as do more than eight arrays. Each array in turn
// must be one-dimensional (else Type mismatch) and hold the start and the
// count (else Out of range), both naming it; the start must be inside the
// array, so an empty one is Out of range even with a count of 0.
fn sort(args: &[Value], descending: bool) -> Result<Vec<Value>> {
    let (start, mut n, values) = if args[0].is_number() {
        if args.len() < 3 {
            return Err(ErrorKind::WrongNumberOfArguments.into());
        }
        (count(&args[0])?, Some(count(&args[1])?), &args[2..])
    } else {
        if args.len() > 8 {
            return Err(ErrorKind::WrongNumberOfArguments.into());
        }
        (0, None, args)
    };
    let mut arrays = Vec::with_capacity(values.len());
    for v in values {
        let a = array(v)?;
        let len = {
            let a = a.borrow();
            if a.dims.len() != 1 {
                return Err(mismatch(v));
            }
            a.len()
        };
        // The whole of the first array, when no count is given.
        let n = *n.get_or_insert(len);
        if len <= start || len < start + n {
            return Err(Error::new(ErrorKind::OutOfRange).of_value(v));
        }
        arrays.push(a);
    }
    let n = n.unwrap_or(0);
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
    make_room(&a, len + n)?;
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
    let (elem, len) = (a.elem_type(), a.len());
    if !memory::shrink(
        memory::array_bytes(elem, len),
        memory::array_bytes(elem, len - n),
    )? {
        // The elements have moved down before the block is asked for, and
        // the length stays. What is left at the end is what follows the
        // array in SmileBASIC 4's memory.
        // UNVERIFIED: that it reads as the type's zero value whatever the
        // array and however many were removed.
        let tail = len - n;
        match &mut a.data {
            ArrayData::Int(v) => {
                v[at..].rotate_left(n);
                v[tail..].fill(0);
            }
            ArrayData::Real(v) => {
                v[at..].rotate_left(n);
                v[tail..].fill(0.0);
            }
            ArrayData::Str(v) => {
                v[at..].rotate_left(n);
                v[tail..].fill_with(|| crate::value::new_str(Vec::new()));
            }
        }
        return Err(ErrorKind::OutOfMemory.into());
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

/// `FIND array, value OUT I` gives the place by element order, as the
/// function does; more OUT variables need one for each of the array's
/// sizes. The count is looked at after the search.
pub(super) fn find(args: &[Value], outs: usize) -> Result<Vec<Value>> {
    let (dims, found) = find_in(args)?;
    if outs == 1 {
        return Ok(vec![Value::Int(found.map_or(-1, |i| i as i32))]);
    }
    if outs != dims.len() {
        return Err(ErrorKind::WrongNumberOfOutArguments.into());
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

/// Gives memory back to the system after shrinking a lot, as that counts as
/// free from now on.
fn hand_back<T>(v: &mut Vec<T>) {
    if v.len() < v.capacity() / 4 {
        v.shrink_to_fit();
    }
}

/// RESIZE keeps an array below this many elements.
const RESIZE_MOST: u64 = 1 << 30;

/// `RESIZE array, n1 [, n2...]`: only the first size can change, and the
/// others are given as they are (checked on hardware).
// SmileBASIC 4 refuses an array of the system's, naming it,
// before it looks at the number of sizes, then at each size in turn: its
// type, the elements so far, past 2^30 Array is too large naming it, and a
// later size other than the array's an Illegal function call naming it. The elements are multiplied as 64-bit unsigned
// numbers, so a negative size is too large unless a 0 came before it.
// Exactly 2^30 is too large too, naming nothing.
pub(super) fn resize(args: &[Value], _outs: usize) -> Result<Vec<Value>> {
    let a = array(&args[0])?;
    let mut a = a.borrow_mut();
    if a.system {
        return Err(Error::new(ErrorKind::CantResizeThisArray).of_arg(0));
    }
    if args.len() != a.dims.len() + 1 {
        return Err(ErrorKind::WrongNumberOfArguments.into());
    }
    let mut sizes = Vec::with_capacity(a.dims.len());
    let mut elements: u64 = 1;
    for (k, v) in args[1..].iter().enumerate() {
        let n = v.to_int()?;
        elements = elements.wrapping_mul(i64::from(n) as u64);
        if elements > RESIZE_MOST {
            return Err(Error::new(ErrorKind::ArrayTooLarge).of_value(v));
        }
        if k > 0 && i64::from(n) != a.dims[k] as i64 {
            return Err(Error::new(ErrorKind::IllegalFunctionCall).of_value(v));
        }
        sizes.push(n.max(0) as usize);
    }
    if elements == RESIZE_MOST {
        return Err(ErrorKind::ArrayTooLarge.into());
    }
    let len = elements as usize;
    make_room(&a, len)?;
    if !memory::shrink(
        memory::array_bytes(a.elem_type(), a.len()),
        memory::array_bytes(a.elem_type(), len),
    )? {
        return Err(ErrorKind::OutOfMemory.into());
    }
    match &mut a.data {
        ArrayData::Int(v) => {
            v.resize(len, 0);
            hand_back(v);
        }
        ArrayData::Real(v) => {
            v.resize(len, 0.0);
            hand_back(v);
        }
        ArrayData::Str(v) => {
            v.resize_with(len, || crate::value::new_str(Vec::new()));
            hand_back(v);
        }
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
                    return Err(mismatch(v));
                }
                if a.is_empty() {
                    return Err(Error::new(ErrorKind::ArrayTooSmall).of_value(v));
                }
                Ok(Param::Array(match &a.data {
                    ArrayData::Real(v) => v.clone(),
                    ArrayData::Int(v) => v.iter().map(|n| f64::from(*n)).collect(),
                    ArrayData::Str(_) => unreachable!("checked above"),
                }))
            }
            v => v.to_real().map(Param::Number),
        }
    }

    fn at(&self, i: usize) -> f64 {
        match self {
            Param::Number(x) => *x,
            // Going round is rare, and a division for each element is not cheap.
            Param::Array(v) => match v.get(i) {
                Some(x) => *x,
                None => v[i % v.len()],
            },
        }
    }
}

/// ARYOP looks at the operation, the number of arguments it takes, the
/// result array and then each parameter in turn, naming the one that is
/// wrong.
pub(super) fn aryop(args: &[Value], _outs: usize) -> Result<Vec<Value>> {
    let op = args[0].to_int()?;
    let wants_three = matches!(op, 4..=6);
    if !(0..=6).contains(&op) {
        return Err(out_of_range("operation").of_value(&args[0]));
    }
    if (args.len() == 5) != wants_three {
        return Err(ErrorKind::WrongNumberOfArguments.into());
    }
    let result = array(&args[1])?;
    if result.borrow().elem_type() == ElemType::Str {
        return Err(mismatch(&args[1]));
    }
    // A parameter may be the result array itself.
    let params: Vec<Param> = args[2..].iter().map(Param::new).collect::<Result<_>>()?;
    let mut result = result.borrow_mut();
    let value = |i: usize| {
        let p = |n: usize| params[n].at(i);
        match op {
            0 => p(0) + p(1),
            1 => p(0) - p(1),
            2 => p(0) * p(1),
            3 => p(0) / p(1),
            4 => p(0) * p(1) + p(2),
            5 => p(0) * p(2) + p(1) * (1.0 - p(2)),
            // A plain comparison, the lower bound first: FINITI gives a
            // bound above the other to mean "at least" and "at most". The
            // lower bound wins when both apply, and NaN goes through.
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
        }
    };
    if let ArrayData::Real(elements) = &mut result.data {
        for (i, element) in elements.iter_mut().enumerate() {
            *element = value(i);
        }
    } else {
        for i in 0..result.len() {
            result.set(i, Value::Real(value(i)))?;
        }
    }
    Ok(vec![])
}

/// An array as channels of samples: a 2D array's first size counts the
/// channels.
fn channels(a: &Array) -> Result<(usize, usize)> {
    match a.dims[..] {
        [len] => Ok((1, len)),
        [ch, len] => Ok((ch, len)),
        _ => Err(ErrorKind::TypeMismatch.into()),
    }
}

/// An integer argument of RINGCOPY's, by its number.
fn ring_int(args: &[Value], i: usize) -> Result<i64> {
    args[i].to_int().map(i64::from).map_err(|e| e.of_arg(i))
}

/// `RINGCOPY dest, offset, source [[,offset], count] [OUT end]`: copies
/// into `dest` going round to its start, and gives where to copy next. Only
/// numbers can be copied, but string arrays are refused only when there is
/// something to copy.
pub(super) fn ringcopy(args: &[Value], outs: usize) -> Result<Vec<Value>> {
    let mismatch = |i| Error::from(ErrorKind::TypeMismatch).of_arg(i);
    let Value::Array(dest) = &args[0] else {
        return Err(mismatch(0));
    };
    let Value::Array(source) = &args[2] else {
        return Err(mismatch(2));
    };
    let elem = {
        let (a, b) = (dest.borrow().elem_type(), source.borrow().elem_type());
        if a != b && (a == ElemType::Str || b == ElemType::Str) {
            return Err(mismatch(2));
        }
        a
    };
    let (ch, len) = channels(&dest.borrow())?;
    let (source_ch, source_len) = channels(&source.borrow())?;
    let wrap = ring_int(args, 1)?;
    let most = source_len as i64;
    let illegal = |i| Error::from(ErrorKind::IllegalFunctionCall).of_arg(i);
    let (from, n) = match args.len() {
        3 => (0, most),
        4 => {
            let n = ring_int(args, 3)?;
            if !(0..=most).contains(&n) {
                return Err(illegal(3));
            }
            (0, n)
        }
        _ => {
            let from = ring_int(args, 3)?;
            if from < 0 {
                return Err(illegal(3));
            }
            let n = ring_int(args, 4)?;
            if n < 0 || from + n > most {
                return Err(illegal(4));
            }
            (from, n)
        }
    };
    let (from, n) = (from as usize, n as usize);
    if n == 0 || len == 0 {
        return Ok(if outs == 1 {
            vec![Value::Int(0)]
        } else {
            vec![]
        });
    }
    if source_ch != 1 && source_ch != ch || elem == ElemType::Str {
        return Err(ErrorKind::TypeMismatch.into());
    }
    let start = wrap.rem_euclid(len as i64) as usize;
    // Channel after channel. A file can give a million channels of no
    // samples, which take nothing here.
    let samples: Vec<Value> = {
        let source = source.borrow();
        let mut samples = Vec::with_capacity(source_ch * n);
        for c in 0..source_ch {
            samples.extend((from..from + n).map(|i| element(&source, c * source_len + i)));
        }
        samples
    };
    let mut dest = dest.borrow_mut();
    for c in 0..ch {
        let at = c.min(source_ch - 1) * n;
        for (k, v) in samples[at..at + n].iter().enumerate() {
            dest.set(c * len + (start + k) % len, v.clone())?;
        }
    }
    Ok(if outs == 1 {
        vec![Value::Int(((start + n) % len) as i32)]
    } else {
        vec![]
    })
}

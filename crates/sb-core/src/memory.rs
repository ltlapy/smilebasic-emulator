//! The value FREEMEM reports: SmileBASIC 4's memory for strings and arrays,
//! minus what the program's values take.
//!
//! The sizes follow hardware (TEST57): a block takes its contents rounded
//! up (64 bytes for an array, 128 for a string) plus a header of 64, and
//! small contents take nothing. Only the values that exist now are counted,
//! so memory that a program no longer holds is free immediately. SmileBASIC
//! 4 frees it only later.
//!
//! The same count decides `Out of memory`. Going through every value for
//! each allocation would be far too slow, so the sites that allocate data
//! work with a running estimate: the bytes live at the last count plus the
//! bytes allocated since. The estimate has no knowledge of what has been
//! dropped, so it runs high. It is never taken as final: when it says a
//! request may not fit, the site returns [`ErrorKind::MemoryCheck`] before
//! doing anything, and the VM counts exactly and runs the instruction again.
//! Only a request that does not fit that count, or that could not fit in
//! all of memory, is refused. Small requests do not stop to check. The VM
//! counts at the start of a slice once enough has been allocated since the
//! last count, and refuses there if the program holds more than all of
//! memory.
//!
//! A block that gets smaller is also a new block; see [`shrink`].

use std::cell::Cell;
use std::collections::HashSet;

use crate::error::{ErrorKind, Result};
use crate::value::{ArrayData, ElemType, StrRef, Value};

/// FREEMEM right after start-up, which the start-up screen shows too.
pub const TOTAL: u32 = 134_216_720;
/// Contents up to these sizes take no separate memory: characters of a
/// string, bytes of an array.
const SMALL_STRING: usize = 24;
const SMALL_ARRAY: usize = 64;
/// The bytes a block takes besides its contents.
const BLOCK_HEADER: usize = 64;

/// Requests below this size only add to the estimate. Stopping to count
/// for them could take longer than making them many times over.
const BIG_REQUEST: usize = 64 * 1024;
/// The bytes allocated between counts. It is at least `MIN_BETWEEN_COUNTS`,
/// and the part that grows with the number of values a count has to go
/// through is at most `MOST_BETWEEN_COUNTS_FOR_VALUES`.
const MIN_BETWEEN_COUNTS: usize = 16 * 1024 * 1024;
const MOST_BETWEEN_COUNTS_FOR_VALUES: usize = 256 * 1024 * 1024;
const BETWEEN_COUNTS_PER_VALUE: usize = 256;

/// The bytes that a string of `chars` characters takes.
#[inline]
pub fn string_bytes(chars: usize) -> usize {
    if chars <= SMALL_STRING {
        return 0;
    }
    chars
        .saturating_mul(2)
        .div_ceil(128)
        .saturating_mul(128)
        .saturating_add(BLOCK_HEADER)
}

/// The bytes that an array of `len` elements takes, without the strings of
/// a string array.
#[inline]
pub fn array_bytes(elem: ElemType, len: usize) -> usize {
    // Strings are held by pointers of 8 bytes.
    let size = match elem {
        ElemType::Int => 4,
        ElemType::Real | ElemType::Str => 8,
    };
    match len.saturating_mul(size) {
        n if n <= SMALL_ARRAY => 0,
        n => n
            .saturating_add(BLOCK_HEADER)
            .div_ceil(64)
            .saturating_mul(64),
    }
}

/// The estimate for one interpreter. FREEMEM counts each interpreter
/// separately.
#[derive(Clone, Copy, Debug, Default)]
pub struct Estimate {
    live: usize,
    made: usize,
    /// Count again once `made` reaches this.
    due: usize,
}

thread_local! {
    // The interpreters take turns on one thread, each setting its own
    // estimate here as it starts a slice.
    static LIVE: Cell<usize> = const { Cell::new(0) };
    static MADE: Cell<usize> = const { Cell::new(0) };
    static DUE: Cell<usize> = const { Cell::new(0) };
    // A count has just been made for the instruction about to run again,
    // so the estimate is now exact.
    static EXACT: Cell<bool> = const { Cell::new(false) };
}

/// Starts a slice of the interpreter whose estimate this is.
pub fn enter(estimate: Estimate) {
    LIVE.set(estimate.live);
    MADE.set(estimate.made);
    DUE.set(estimate.due);
}

/// Ends a slice and returns the estimate to keep.
pub fn leave() -> Estimate {
    Estimate {
        live: LIVE.get(),
        made: MADE.get(),
        due: DUE.get(),
    }
}

/// Whether enough has been made to count again.
pub fn count_due() -> bool {
    MADE.get() >= DUE.get()
}

/// Records `bytes` as allocated, where nothing can be refused any more or
/// the size is small.
#[inline]
pub fn charge(bytes: usize) {
    if bytes != 0 {
        MADE.set(MADE.get().saturating_add(bytes));
    }
}

/// For data about to be made, or already made, by something that cannot run
/// again: refused only if it could never fit.
#[inline]
pub fn take(bytes: usize) -> Result<()> {
    if bytes > TOTAL as usize {
        return Err(ErrorKind::OutOfMemory.into());
    }
    charge(bytes);
    Ok(())
}

/// A new string or array of `bytes`, by something that has done nothing yet
/// and can run again.
#[inline]
pub fn request(bytes: usize) -> Result<()> {
    if bytes < BIG_REQUEST {
        charge(bytes);
        return Ok(());
    }
    big_request(bytes, bytes)
}

/// An array growing from `old` bytes to `new`, by something that has done
/// nothing yet and can run again. The new block has to fit while the old
/// one is still held.
#[inline]
pub fn grow(old: usize, new: usize) -> Result<()> {
    if new <= old {
        return Ok(());
    }
    if new < BIG_REQUEST {
        charge(new - old);
        return Ok(());
    }
    big_request(new, new - old)
}

/// A block going from `old` bytes to `new`, by something that has done
/// nothing yet and can run again. Returns whether it may. A smaller block
/// is also a new block, which has to fit while the old one is still held.
/// Each caller decides what happens when it does not fit. Blocks under
/// [`BIG_REQUEST`] are always allowed.
#[inline]
pub fn shrink(old: usize, new: usize) -> Result<bool> {
    if new >= old || new < BIG_REQUEST {
        return Ok(true);
    }
    big_shrink(new)
}

#[inline(never)]
fn big_shrink(new: usize) -> Result<bool> {
    match big_request(new, 0) {
        Ok(()) => Ok(true),
        Err(e) if e.kind == ErrorKind::OutOfMemory => Ok(false),
        Err(e) => Err(e),
    }
}

/// `needs` has to fit in the free memory, and then `adds` more bytes are
/// live.
#[inline(never)]
fn big_request(needs: usize, adds: usize) -> Result<()> {
    let exact = EXACT.replace(false);
    if needs > TOTAL as usize {
        return Err(ErrorKind::OutOfMemory.into());
    }
    if LIVE.get().saturating_add(MADE.get()).saturating_add(needs) <= TOTAL as usize {
        charge(adds);
        return Ok(());
    }
    Err(match exact {
        true => ErrorKind::OutOfMemory,
        false => ErrorKind::MemoryCheck,
    }
    .into())
}

/// Takes an exact count as the estimate. `then` means that the instruction
/// that asked for the count runs again next.
pub fn counted(tally: &Tally, then: bool) {
    let between = tally
        .values
        .saturating_mul(BETWEEN_COUNTS_PER_VALUE)
        .min(MOST_BETWEEN_COUNTS_FOR_VALUES);
    LIVE.set(tally.bytes);
    MADE.set(0);
    DUE.set(MIN_BETWEEN_COUNTS.max(tally.bytes / 4).max(between));
    EXACT.set(then);
}

/// [`counted`], for tests elsewhere, of a count that came to `bytes`.
#[cfg(test)]
pub(crate) fn counted_bytes(bytes: usize, then: bool) {
    let tally = Tally {
        bytes,
        ..Tally::default()
    };
    counted(&tally, then);
}

/// Counts each string and array once, however many values refer to it.
#[derive(Default)]
pub struct Tally {
    seen: HashSet<usize>,
    pub bytes: usize,
    /// Strings and arrays gone through.
    values: usize,
}

impl Tally {
    /// Skips an array, as if it were already counted.
    pub fn skip(&mut self, array: &crate::value::ArrayRef) {
        self.seen.insert(array.as_ptr() as usize);
    }

    pub fn add(&mut self, value: &Value) {
        match value {
            Value::Str(s) => {
                self.values += 1;
                self.add_string_bytes(s);
            }
            Value::Array(a) => {
                if !self.seen.insert(a.as_ptr() as usize) {
                    return;
                }
                let a = a.borrow();
                self.values += 1;
                self.bytes += array_bytes(a.elem_type(), a.len());
                if let ArrayData::Str(v) = &a.data {
                    self.values += v.len();
                    for s in v {
                        self.add_string_bytes(s);
                    }
                }
            }
            _ => {}
        }
    }

    /// Adds the bytes of string `s`, unless they were counted already.
    fn add_string_bytes(&mut self, s: &StrRef) {
        // Nothing else holds it, which is true of most strings. This
        // test is much cheaper than the set.
        if std::rc::Rc::strong_count(s) == 1 || self.seen.insert(s.as_ptr() as usize) {
            self.bytes += string_bytes(s.borrow().len());
        }
    }

    /// FREEMEM: the bytes left.
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
            (64, 192),
            (65, 320),
            (100, 320),
            (128, 320),
            (129, 448),
            (256, 576),
            (257, 704),
            (1000, 2112),
            (10000, 20160),
        ] {
            assert_eq!(used(&[string(len)]), bytes, "{len} characters");
        }
        for (elem, len, bytes) in [
            (ElemType::Int, 10, 0),
            (ElemType::Int, 16, 0),
            (ElemType::Real, 8, 0),
            (ElemType::Int, 17, 192),
            (ElemType::Int, 33, 256),
            (ElemType::Int, 49, 320),
            (ElemType::Int, 100, 512),
            (ElemType::Int, 1000, 4096),
            (ElemType::Real, 9, 192),
            (ElemType::Real, 17, 256),
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

    #[test]
    fn shared_strings_in_arrays_count_once() {
        let s = new_str(vec![0; 1000]);
        let mut a = Array::new(ElemType::Str, vec![2]);
        a.set(0, Value::Str(s.clone())).unwrap();
        a.set(1, Value::Str(s)).unwrap();
        let a = Value::Array(Rc::new(RefCell::new(a)));
        assert_eq!(used(&[a]), 2112);
    }

    fn fresh(live: usize) {
        enter(Estimate {
            live,
            made: 0,
            due: usize::MAX,
        });
        EXACT.set(false);
    }

    #[test]
    fn a_request_that_could_never_fit_is_refused_at_once() {
        fresh(0);
        let e = request(TOTAL as usize + 1).unwrap_err();
        assert_eq!(e.kind, ErrorKind::OutOfMemory);
        let e = grow(0, TOTAL as usize + 1).unwrap_err();
        assert_eq!(e.kind, ErrorKind::OutOfMemory);
        let e = take(TOTAL as usize + 1).unwrap_err();
        assert_eq!(e.kind, ErrorKind::OutOfMemory);
    }

    #[test]
    fn the_estimate_alone_never_refuses() {
        fresh(TOTAL as usize);
        let e = request(1 << 20).unwrap_err();
        assert_eq!(e.kind, ErrorKind::MemoryCheck);
        // The count that follows finds it is all garbage.
        let tally = Tally::default();
        counted(&tally, true);
        request(1 << 20).unwrap();
    }

    #[test]
    fn a_request_that_does_not_fit_the_count_is_refused() {
        fresh(0);
        let tally = Tally {
            bytes: TOTAL as usize - 1000,
            ..Tally::default()
        };
        counted(&tally, true);
        let e = request(1 << 20).unwrap_err();
        assert_eq!(e.kind, ErrorKind::OutOfMemory);
        // That was for one instruction: the next asks again.
        let e = request(1 << 20).unwrap_err();
        assert_eq!(e.kind, ErrorKind::MemoryCheck);
    }

    #[test]
    fn growing_needs_the_new_block_beside_the_old() {
        let mb = 1 << 20;
        fresh(50 * mb);
        grow(50 * mb, 70 * mb).unwrap();
        fresh(62 * mb);
        let e = grow(62 * mb, 124 * mb).unwrap_err();
        assert_eq!(e.kind, ErrorKind::MemoryCheck);
        // Growing adds only the difference to what is live.
        fresh(0);
        grow(mb, 2 * mb).unwrap();
        assert_eq!(leave().made, mb);
    }

    #[test]
    fn growing_a_big_block_makes_only_the_growth() {
        fresh(16 * (1 << 20));
        let (old, new) = (
            array_bytes(ElemType::Int, 4_000_000),
            array_bytes(ElemType::Int, 4_000_001),
        );
        grow(old, new).unwrap();
        assert_eq!(leave().made, 64);
    }

    #[test]
    fn shrinking_needs_the_smaller_block_beside_the_old() {
        let mb = 1 << 20;
        // The same block, or a small one, is never checked.
        fresh(TOTAL as usize);
        assert!(shrink(4 * mb, 4 * mb).unwrap());
        assert!(shrink(4 * mb, BIG_REQUEST - 64).unwrap());
        // Not known yet: nothing is decided until the count.
        let e = shrink(4 * mb, 4 * mb - 64).unwrap_err();
        assert_eq!(e.kind, ErrorKind::MemoryCheck);
        // The count says it does not fit: no error, but the answer is no.
        let tally = Tally {
            bytes: TOTAL as usize - mb,
            ..Tally::default()
        };
        counted(&tally, true);
        assert!(!shrink(4 * mb, 4 * mb - 64).unwrap());
        // The count says it fits, and nothing more is live for that.
        let tally = Tally {
            bytes: 8 * mb,
            ..Tally::default()
        };
        counted(&tally, true);
        assert!(shrink(4 * mb, 4 * mb - 64).unwrap());
        assert_eq!(leave().made, 0);
    }

    #[test]
    fn small_requests_only_add_up() {
        fresh(TOTAL as usize);
        request(100).unwrap();
        assert_eq!(leave().made, 100);
    }
}

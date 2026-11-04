//! Operator semantics, following the operator page of the reference.
//!
//! `+ - *` stay integer when both sides are integers and the result fits;
//! otherwise they produce a real. `/` is always real. DIV, MOD, shifts and
//! the bitwise operators are integer operators: real operands are truncated.

use std::cmp::Ordering;

use crate::ast::{BinaryOp, UnaryOp};
use crate::error::{ErrorKind, Result};
use crate::memory;
use crate::value::{StrRef, Value, new_str};

fn bool_value(b: bool) -> Value {
    Value::Int(i32::from(b))
}

fn arith(
    lhs: &Value,
    rhs: &Value,
    int: fn(i32, i32) -> Option<i32>,
    real: fn(f64, f64) -> f64,
) -> Result<Value> {
    if let (Value::Int(a), Value::Int(b)) = (lhs, rhs) {
        if let Some(n) = int(*a, *b) {
            return Ok(Value::Int(n));
        }
    }
    Ok(Value::Real(real(lhs.to_real()?, rhs.to_real()?)))
}

fn int_op(lhs: &Value, rhs: &Value, f: impl Fn(i32, i32) -> Result<i32>) -> Result<Value> {
    Ok(Value::Int(f(lhs.to_int()?, rhs.to_int()?)?))
}

/// The shift count is taken modulo 32, as the AArch64 shift instructions on
/// the Switch do.
fn shift_count(n: i32) -> u32 {
    (n & 31) as u32
}

/// `None` when a NaN is involved: every comparison except `!=` is then false.
fn compare(lhs: &Value, rhs: &Value) -> Result<Option<Ordering>> {
    match (lhs, rhs) {
        (Value::Int(a), Value::Int(b)) => Ok(Some(a.cmp(b))),
        (Value::Str(a), Value::Str(b)) => {
            Ok(Some(a.borrow().as_slice().cmp(b.borrow().as_slice())))
        }
        _ if lhs.is_number() && rhs.is_number() => Ok(lhs.to_real()?.partial_cmp(&rhs.to_real()?)),
        _ => Err(ErrorKind::TypeMismatch.into()),
    }
}

fn compare_with(lhs: &Value, rhs: &Value, accept: fn(Ordering) -> bool) -> Result<Value> {
    Ok(bool_value(compare(lhs, rhs)?.is_some_and(accept)))
}

/// `a+b` for strings; out of line, for arithmetic to stay as short as it is.
#[inline(never)]
fn joined(a: &StrRef, b: &StrRef) -> Result<Value> {
    let (a, b) = (a.borrow(), b.borrow());
    let chars = a.len() + b.len();
    memory::request(memory::string_bytes(chars))?;
    let mut s = Vec::with_capacity(chars);
    s.extend_from_slice(&a);
    s.extend_from_slice(&b);
    Ok(Value::Str(new_str(s)))
}

/// `s*count`.
#[inline(never)]
fn repeated(s: &StrRef, count: usize) -> Result<Value> {
    let s = s.borrow();
    memory::request(memory::string_bytes(s.len().saturating_mul(count)))?;
    Ok(Value::Str(new_str(s.repeat(count))))
}

/// What [`binary`] gives for the operators programs mostly use on two
/// integers, when that is an integer again. `None` leaves it to [`binary`].
#[inline]
pub fn binary_ints(op: BinaryOp, a: i32, b: i32) -> Option<i32> {
    use BinaryOp as B;
    match op {
        B::Add => a.checked_add(b),
        B::Sub => a.checked_sub(b),
        B::Mul => a.checked_mul(b),
        B::Eq => Some(i32::from(a == b)),
        B::NotEq => Some(i32::from(a != b)),
        B::Less => Some(i32::from(a < b)),
        B::LessEq => Some(i32::from(a <= b)),
        B::Greater => Some(i32::from(a > b)),
        B::GreaterEq => Some(i32::from(a >= b)),
        B::And => Some(a & b),
        B::Or => Some(a | b),
        B::Xor => Some(a ^ b),
        _ => None,
    }
}

pub fn binary(op: BinaryOp, lhs: &Value, rhs: &Value) -> Result<Value> {
    use BinaryOp as B;
    match op {
        B::Add => {
            if let (Value::Str(a), Value::Str(b)) = (lhs, rhs) {
                return joined(a, b);
            }
            arith(lhs, rhs, i32::checked_add, |a, b| a + b)
        }
        B::Sub => arith(lhs, rhs, i32::checked_sub, |a, b| a - b),
        B::Mul => {
            if let Value::Str(s) = lhs {
                // A negative count overflows (checked on hardware).
                let count = usize::try_from(rhs.to_int()?).map_err(|_| ErrorKind::Overflow)?;
                return repeated(s, count);
            }
            arith(lhs, rhs, i32::checked_mul, |a, b| a * b)
        }
        B::Div => {
            let divisor = rhs.to_real()?;
            let dividend = lhs.to_real()?;
            if divisor == 0.0 {
                return Err(ErrorKind::DivideByZero.into());
            }
            Ok(Value::Real(dividend / divisor))
        }
        // MOD takes the sign of the dividend, and the one overflowing case
        // (-2147483648 DIV -1) wraps.
        B::IntDiv => int_op(lhs, rhs, |a, b| match b {
            0 => Err(ErrorKind::DivideByZero.into()),
            _ => Ok(a.wrapping_div(b)),
        }),
        B::Mod => int_op(lhs, rhs, |a, b| match b {
            0 => Err(ErrorKind::DivideByZero.into()),
            _ => Ok(a.wrapping_rem(b)),
        }),
        B::Shl => int_op(lhs, rhs, |a, b| Ok(a.wrapping_shl(shift_count(b)))),
        B::Shr => int_op(lhs, rhs, |a, b| Ok(a.wrapping_shr(shift_count(b)))),
        B::UShr => int_op(lhs, rhs, |a, b| {
            Ok((a as u32).wrapping_shr(shift_count(b)) as i32)
        }),
        B::RotL => int_op(lhs, rhs, |a, b| Ok(a.rotate_left(shift_count(b)))),
        B::RotR => int_op(lhs, rhs, |a, b| Ok(a.rotate_right(shift_count(b)))),
        B::And => int_op(lhs, rhs, |a, b| Ok(a & b)),
        B::Or => int_op(lhs, rhs, |a, b| Ok(a | b)),
        B::Xor => int_op(lhs, rhs, |a, b| Ok(a ^ b)),
        B::Eq => compare_with(lhs, rhs, Ordering::is_eq),
        B::NotEq => Ok(bool_value(!compare(lhs, rhs)?.is_some_and(Ordering::is_eq))),
        B::Less => compare_with(lhs, rhs, Ordering::is_lt),
        B::LessEq => compare_with(lhs, rhs, Ordering::is_le),
        B::Greater => compare_with(lhs, rhs, Ordering::is_gt),
        B::GreaterEq => compare_with(lhs, rhs, Ordering::is_ge),
        // Short-circuiting forms are compiled to jumps; this is the eager fallback.
        B::LogicalAnd => Ok(bool_value(lhs.is_true()? && rhs.is_true()?)),
        B::LogicalOr => Ok(bool_value(lhs.is_true()? || rhs.is_true()?)),
    }
}

pub fn unary(op: UnaryOp, value: &Value) -> Result<Value> {
    match op {
        UnaryOp::Neg => match *value {
            Value::Int(n) => Ok(n
                .checked_neg()
                .map_or(Value::Real(-f64::from(n)), Value::Int)),
            Value::Real(x) => Ok(Value::Real(-x)),
            _ => Err(ErrorKind::TypeMismatch.into()),
        },
        UnaryOp::Not => Ok(Value::Int(!value.to_int()?)),
        UnaryOp::LogicalNot => Ok(bool_value(!value.is_true()?)),
    }
}

#[cfg(test)]
mod tests {
    /// The integer path must answer as the full one does, or leave it to it.
    #[test]
    fn binary_ints_agrees_with_binary() {
        use BinaryOp as B;
        let ops = [
            B::Add,
            B::Sub,
            B::Mul,
            B::Div,
            B::IntDiv,
            B::Mod,
            B::Shl,
            B::Eq,
            B::NotEq,
            B::Less,
            B::LessEq,
            B::Greater,
            B::GreaterEq,
            B::And,
            B::Or,
            B::Xor,
        ];
        let values = [
            i32::MIN,
            i32::MIN + 1,
            -65536,
            -2,
            -1,
            0,
            1,
            2,
            3,
            46341,
            65536,
            i32::MAX - 1,
            i32::MAX,
        ];
        for op in ops {
            for a in values {
                for b in values {
                    if let Some(n) = binary_ints(op, a, b) {
                        match binary(op, &Value::Int(a), &Value::Int(b)) {
                            Ok(Value::Int(m)) => assert_eq!(n, m, "{op:?} {a} {b}"),
                            other => panic!("{op:?} {a} {b}: {n} and {other:?}"),
                        }
                    }
                }
            }
        }
    }

    use super::*;
    use BinaryOp as B;

    fn int(n: i32) -> Value {
        Value::Int(n)
    }

    fn real(x: f64) -> Value {
        Value::Real(x)
    }

    fn eval(op: BinaryOp, a: Value, b: Value) -> Value {
        binary(op, &a, &b).unwrap()
    }

    fn kind(op: BinaryOp, a: Value, b: Value) -> ErrorKind {
        binary(op, &a, &b).unwrap_err().kind
    }

    fn as_int(v: Value) -> i32 {
        match v {
            Value::Int(n) => n,
            other => panic!("expected int, got {other:?}"),
        }
    }

    fn as_real(v: Value) -> f64 {
        match v {
            Value::Real(x) => x,
            other => panic!("expected real, got {other:?}"),
        }
    }

    fn as_str(v: Value) -> String {
        String::from_utf16(&v.into_str().unwrap().borrow()).unwrap()
    }

    #[test]
    fn integer_arithmetic_overflows_into_real() {
        assert_eq!(as_int(eval(B::Add, int(2), int(3))), 5);
        assert_eq!(as_real(eval(B::Add, int(i32::MAX), int(1))), 2147483648.0);
        assert_eq!(as_real(eval(B::Mul, int(65536), int(65536))), 4294967296.0);
        assert_eq!(as_real(eval(B::Add, int(1), real(0.5))), 1.5);
        assert_eq!(
            as_real(unary(UnaryOp::Neg, &int(i32::MIN)).unwrap()),
            2147483648.0
        );
    }

    #[test]
    fn division() {
        assert_eq!(as_real(eval(B::Div, int(7), int(2))), 3.5);
        assert_eq!(as_int(eval(B::IntDiv, int(201), int(5))), 40);
        assert_eq!(as_int(eval(B::Mod, int(201), int(5))), 1);
        assert_eq!(as_int(eval(B::IntDiv, real(7.9), real(2.1))), 3);
        assert_eq!(kind(B::Div, int(1), int(0)), ErrorKind::DivideByZero);
        assert_eq!(kind(B::Mod, int(1), real(0.5)), ErrorKind::DivideByZero);
    }

    #[test]
    fn shifts_and_rotates() {
        assert_eq!(as_int(eval(B::Shl, int(100), int(2))), 400);
        assert_eq!(as_int(eval(B::Shr, int(-10), int(1))), -5);
        assert_eq!(as_int(eval(B::UShr, int(-10), int(1))), 0x7FFF_FFFB);
        assert_eq!(
            as_int(eval(B::RotL, int(0xFF00FF00_u32 as i32), int(8))),
            0x00FF00FF
        );
        assert_eq!(
            as_int(eval(B::RotR, int(0x00FF00FF), int(8))),
            0xFF00FF00_u32 as i32
        );
    }

    #[test]
    fn bitwise() {
        assert_eq!(as_int(eval(B::And, int(200), int(0xE7))), 192);
        assert_eq!(as_int(eval(B::Or, int(128), int(0xA3))), 0xA3);
        assert_eq!(as_int(eval(B::Xor, int(100), int(0x4C))), 40);
        assert_eq!(as_int(unary(UnaryOp::Not, &int(0)).unwrap()), -1);
    }

    #[test]
    fn strings() {
        assert_eq!(
            as_str(eval(B::Add, Value::str("AB"), Value::str("C"))),
            "ABC"
        );
        assert_eq!(as_str(eval(B::Mul, Value::str("AB"), int(3))), "ABABAB");
        assert_eq!(
            kind(B::Add, Value::str("A"), int(1)),
            ErrorKind::TypeMismatch
        );
        assert_eq!(
            as_int(eval(B::Less, Value::str("ABC"), Value::str("ABD"))),
            1
        );
        assert_eq!(as_int(eval(B::Eq, Value::str("A"), Value::str("A"))), 1);
    }

    #[test]
    fn comparisons_return_int() {
        assert_eq!(as_int(eval(B::Eq, int(1), real(1.0))), 1);
        assert_eq!(as_int(eval(B::GreaterEq, int(1), int(2))), 0);
        let nan = real(f64::NAN);
        assert_eq!(as_int(eval(B::Eq, nan.clone(), nan.clone())), 0);
        assert_eq!(as_int(eval(B::LessEq, nan.clone(), int(1))), 0);
        assert_eq!(as_int(eval(B::NotEq, nan, int(1))), 1);
    }

    #[test]
    fn default_type_is_an_error() {
        assert_eq!(
            kind(B::Add, Value::Default, int(1)),
            ErrorKind::TypeMismatch
        );
        assert!(unary(UnaryOp::Neg, &Value::Default).is_err());
    }
}

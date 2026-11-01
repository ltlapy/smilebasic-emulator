//! Watch expressions for a debugger: an expression evaluated against a
//! stopped program's variables without changing anything. It may use
//! variables, array elements, constants, operators and the builtin
//! functions that only compute a value; DEFs, commands and functions
//! that touch the program or the machine are refused.

use crate::ast::{BinaryOp, Expr};
use crate::builtins::{self, Form, Kind};
use crate::bytecode::full_name;
use crate::constants::{self, ConstValue};
use crate::error::{Error, ErrorKind, Result};
use crate::ops;
use crate::parser;
use crate::value::{Value, new_str};

/// Builtin functions that change their argument.
const CHANGES: &[&str] = &["POP", "SHIFT"];

/// Evaluates `source`, finding variables by full name (`A%`) with `var`.
pub fn eval(source: &str, var: &dyn Fn(&str) -> Option<Value>) -> Result<Value> {
    Watch::new(source)?.eval(var)
}

/// A watch expression parsed once, to be worked out again and again, as
/// a breakpoint's condition is.
#[derive(Debug)]
pub struct Watch {
    source: String,
    expr: Expr,
}

impl Watch {
    pub fn new(source: &str) -> Result<Watch> {
        let expr = parser::parse_expression(source).map_err(|e| {
            Error::with_detail(ErrorKind::SyntaxError, e.detail().unwrap_or_default())
        })?;
        Ok(Watch {
            source: source.to_string(),
            expr,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// Its value, finding variables by full name (`A%`) with `var`.
    pub fn eval(&self, var: &dyn Fn(&str) -> Option<Value>) -> Result<Value> {
        value(&self.expr, var)
    }
}

fn refused(what: &str) -> Error {
    Error::with_detail(ErrorKind::IllegalFunctionCall, format!("{what} in a watch"))
}

/// Works out `expr` from the first operand of its chain up (see
/// [`Expr::chain_operand`]), the left operand before the right.
fn value(expr: &Expr, var: &dyn Fn(&str) -> Option<Value>) -> Result<Value> {
    let mut links = Vec::new();
    let mut first = expr;
    while let Some(operand) = first.chain_operand() {
        links.push(first);
        first = operand;
    }
    let mut value = operand(first, var)?;
    for link in links.iter().rev() {
        value = match link {
            Expr::Index { indices, .. } => {
                let Value::Array(array) = value else {
                    return Err(ErrorKind::TypeMismatch.into());
                };
                let indices = (indices.iter())
                    .map(|i| self::value(i, var)?.to_int())
                    .collect::<Result<Vec<_>>>()?;
                let array = array.borrow();
                array.get(array.offset(&indices)?)
            }
            Expr::Unary { op, .. } => ops::unary(*op, &value)?,
            Expr::Binary {
                op: op @ (BinaryOp::LogicalAnd | BinaryOp::LogicalOr),
                rhs,
                ..
            } => {
                let left = value.is_true()?;
                let wants = *op == BinaryOp::LogicalAnd;
                let truth = if left == wants {
                    self::value(rhs, var)?.is_true()?
                } else {
                    left
                };
                Value::Int(i32::from(truth))
            }
            Expr::Binary { op, rhs, .. } => ops::binary(*op, &value, &self::value(rhs, var)?)?,
            _ => unreachable!("chain links are operators and indexing"),
        };
    }
    Ok(value)
}

/// [`value`] of anything but an operator or indexing.
fn operand(expr: &Expr, var: &dyn Fn(&str) -> Option<Value>) -> Result<Value> {
    Ok(match expr {
        Expr::Int(n) => Value::Int(*n),
        Expr::Real(x) => Value::Real(*x),
        Expr::Str(s) => Value::Str(new_str(s.clone())),
        Expr::Label(name) => Value::str(&format!("@{name}")),
        Expr::Constant(name) => match constants::lookup(name) {
            Some(ConstValue::Int(n)) => Value::Int(n),
            Some(ConstValue::Real(x)) => Value::Real(x),
            None => {
                return Err(Error::with_detail(
                    ErrorKind::UndefinedConstant,
                    format!("#{name}"),
                ));
            }
        },
        Expr::Var(v) => {
            let name = full_name(&v.name, v.suffix);
            var(&name).ok_or_else(|| Error::with_detail(ErrorKind::UndefinedVariable, name))?
        }
        Expr::Index { .. } | Expr::Unary { .. } | Expr::Binary { .. } => {
            unreachable!("value() follows operators and indexing")
        }
        Expr::Call { name, args } => {
            let full = full_name(&name.name, name.suffix);
            let builtin = builtins::lookup(&full, Form::Function)
                .map(builtins::get)
                .filter(|_| !CHANGES.contains(&full.as_str()));
            let Some(builtin) = builtin else {
                return Err(refused(&full));
            };
            let Kind::Function(f) = builtin.kind else {
                return Err(refused(&full));
            };
            let args = (args.iter())
                .map(|a| a.as_ref().map_or(Ok(Value::Default), |a| value(a, var)))
                .collect::<Result<Vec<_>>>()?;
            if !builtin.accepts(args.len()) {
                return Err(ErrorKind::WrongNumberOfArguments.into());
            }
            f(&args)?
        }
        Expr::DynamicCall { .. } => return Err(refused("CALL")),
        Expr::DynamicVar(name) => {
            let name = value(name, var)?.into_str()?;
            let name = String::from_utf16_lossy(&name.borrow()).to_ascii_uppercase();
            var(&name).ok_or_else(|| Error::with_detail(ErrorKind::UndefinedVariable, name))?
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watch(source: &str) -> Result<String> {
        let var = |name: &str| match name {
            "A" => Some(Value::Int(6)),
            "S$" => Some(Value::str("HELLO")),
            "N%" => Some(Value::Int(-2)),
            _ => None,
        };
        let v = eval(source, &var)?;
        Ok(match v {
            Value::Str(s) => String::from_utf16_lossy(&s.borrow()),
            v => format!("{:?}", v),
        })
    }

    #[test]
    fn evaluates_without_side_effects() {
        assert_eq!(watch("A*2+1").unwrap(), "Int(13)");
        assert_eq!(watch("LEN(S$)").unwrap(), "Int(5)");
        assert_eq!(watch("MID$(S$,1,3)").unwrap(), "ELL");
        assert_eq!(watch("ABS(N%) && A>3").unwrap(), "Int(1)");
        assert_eq!(
            watch("#C_RED").unwrap(),
            format!("{:?}", Value::Int(-65536))
        );
        assert_eq!(watch("VAR(\"a\")").unwrap(), "Int(6)");
        assert_eq!(watch("B").unwrap_err().kind, ErrorKind::UndefinedVariable);
        assert_eq!(
            watch("F(1)").unwrap_err().kind,
            ErrorKind::IllegalFunctionCall
        );
        assert_eq!(watch("A+").unwrap_err().kind, ErrorKind::SyntaxError);
    }

    #[test]
    fn long_chains_fit_a_small_stack() {
        let source = format!("S$+{}", vec!["\"B\""; 20_000].join("+"));
        let run = std::thread::Builder::new()
            .stack_size(256 << 10)
            .spawn(move || watch(&source).unwrap().len());
        assert_eq!(run.unwrap().join().unwrap(), 5 + 20_000);
        assert_eq!(watch("-A*-2+1>0&&LEN(S$)==5").unwrap(), "Int(1)");
    }
}

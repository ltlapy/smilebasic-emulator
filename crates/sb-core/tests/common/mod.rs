//! A host with a few commands of its own, standing in for the runtime's, so
//! that tests see host calls made fused and one instruction at a time alike.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::host::{Host, TextHost};
use sb_core::value::Value;

/// The commands, by id:
///
/// - `PUT a,b,...` prints its numbers; a string is a Type mismatch, a
///   negative number Out of range.
/// - `GET()` gives a count that goes up by one with each call, and `GET N`
///   adds N to it; as `GET OUT X` it gives the count too.
/// - `PAIR A OUT B,C` gives A+1 and A*2.
/// - `HOLD` waits a frame on every other call, as a confirmation does.
/// - `BAD()` gives no value, which the VM refuses.
/// - `NOTE$()` gives a new string each call.
#[derive(Debug, Default)]
pub struct CmdHost {
    pub text: TextHost,
    pub count: i32,
    pub held: bool,
    retry: bool,
}

const NAMES: &[&str] = &["PUT", "GET", "PAIR", "HOLD", "BAD", "NOTE$"];

fn number(v: &Value) -> Result<f64> {
    match v {
        Value::Int(n) => Ok(f64::from(*n)),
        Value::Real(x) => Ok(*x),
        _ => Err(Error::new(ErrorKind::TypeMismatch).of_value(v)),
    }
}

impl Host for CmdHost {
    fn lookup(&self, name: &str) -> Option<u32> {
        NAMES.iter().position(|n| *n == name).map(|i| i as u32)
    }

    fn call(&mut self, id: u32, args: &[Value], want: Want) -> Result<Vec<Value>> {
        let wrong = || Error::new(ErrorKind::WrongNumberOfArguments).of_command(NAMES[id as usize]);
        match (id, want, args.len()) {
            (0, Want::Outs(0), _) => {
                let mut line = String::new();
                for a in args {
                    let n = number(a)?;
                    if n < 0.0 {
                        return Err(Error::new(ErrorKind::OutOfRange).of_value(a));
                    }
                    line += &format!("{n} ");
                }
                self.text.output += &format!("PUT {line}\n");
                Ok(vec![])
            }
            (1, Want::Value | Want::Outs(1), 0) => {
                self.count += 1;
                Ok(vec![Value::Int(self.count)])
            }
            (1, Want::Outs(0), 1) => {
                self.count += number(&args[0])? as i32;
                Ok(vec![])
            }
            (2, Want::Outs(2), 1) => {
                let a = number(&args[0])?;
                Ok(vec![Value::Real(a + 1.0), Value::Real(a * 2.0)])
            }
            (3, Want::Outs(0), 0) => {
                self.held = !self.held;
                self.retry = self.held;
                if !self.held {
                    self.text.output += "HELD\n";
                }
                Ok(vec![])
            }
            (4, Want::Value, 0) => Ok(vec![]),
            (5, Want::Value, 0) => {
                self.count += 1;
                Ok(vec![Value::Str(sb_core::value::new_str(
                    format!("N{}", self.count).encode_utf16().collect(),
                ))])
            }
            _ => Err(wrong()),
        }
    }

    fn retry(&mut self) -> bool {
        std::mem::take(&mut self.retry)
    }

    fn print(&mut self, screen: Option<i32>, text: &[u16]) -> Result<()> {
        self.text.print(screen, text)
    }

    fn print_tab(&mut self, screen: Option<i32>) -> Result<()> {
        self.text.print_tab(screen)
    }

    fn print_newline(&mut self, screen: Option<i32>) -> Result<()> {
        self.text.print_newline(screen)
    }

    fn input_start(&mut self) -> Result<()> {
        self.text.input_start()
    }

    fn input_poll(&mut self) -> Result<Option<Vec<u16>>> {
        self.text.input_poll()
    }
}

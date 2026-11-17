//! Interface to everything outside the language: screens, input, sound,
//! files. The frontend's runtime implements it.

use std::collections::VecDeque;

use std::rc::Rc;

use crate::bytecode::{Callbacks, Compiled, Want};
use crate::error::{Error, ErrorKind, Result};
use crate::value::Value;
use crate::vm::Work;

/// What EXEC goes on with, from [`Host::exec_source`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exec {
    /// Run this source in the slot.
    Run(String),
    /// Waiting for the user, such as a confirmation: ask again on the
    /// next frame.
    Wait,
}

pub trait Host {
    /// Resolves a command or function name, suffix included (`"GPSET"`,
    /// `"INKEY$"`), to an id passed back to [`Host::call`].
    fn lookup(&self, name: &str) -> Option<u32>;

    /// Runs a host command or function. Omitted arguments are
    /// [`Value::Default`]. Must return exactly `want.count()` values.
    fn call(&mut self, id: u32, args: &[Value], want: Want) -> Result<Vec<Value>>;

    /// [`Host::call`], with the values put in `out`, which is empty and
    /// kept by the VM from call to call, so that a host can give a result
    /// without allocating a list for it. What is in `out` after an error
    /// is dropped.
    fn call_into(
        &mut self,
        id: u32,
        args: &[Value],
        want: Want,
        out: &mut Vec<Value>,
    ) -> Result<()> {
        *out = self.call(id, args, want)?;
        Ok(())
    }

    /// The work the VM has done so far, given before each [`Host::call`]
    /// when [`crate::vm::Vm::report_work`] asks for it.
    fn work(&mut self, _work: Work) {}

    /// Whether the time the host gives the VM has run out, asked after
    /// each [`Host::call`] and PRINT when [`crate::vm::Vm::report_work`]
    /// is on. The VM then stops with [`crate::vm::Status::Running`], as
    /// when its budget is spent.
    fn out_of_time(&self) -> bool {
        false
    }

    /// PRINT output. `screen` is `None` for the console, or TPRINT's screen.
    fn print(&mut self, screen: Option<i32>, text: &[u16]) -> Result<()>;
    /// PRINT's `,` separator.
    fn print_tab(&mut self, screen: Option<i32>) -> Result<()>;
    fn print_newline(&mut self, screen: Option<i32>) -> Result<()>;

    /// Starts reading a line from the keyboard on the console, for INPUT
    /// and LINPUT. The prompt has already been printed.
    fn input_start(&mut self) -> Result<()>;
    /// The line once Enter has been pressed, or `None` while still typing.
    /// The VM polls once per frame.
    fn input_poll(&mut self) -> Result<Option<Vec<u16>>>;

    /// Whether the last [`Host::call`] did nothing because it waits for
    /// the user, such as a confirmation. The VM then makes the same call
    /// on the next frame, and ignores what this one returned. Reading it
    /// clears it.
    fn retry(&mut self) -> bool {
        false
    }

    /// EXEC: loads the program file `file`, if given, into program slot
    /// `slot`, and gives the slot's source to run there.
    fn exec_source(&mut self, _file: Option<&[u16]>, _slot: i32) -> Result<Exec> {
        Err(ErrorKind::IllegalFunctionCall.into())
    }

    /// The name of the file program slot `slot` was loaded from or saved
    /// to, as PRGNAME$ gives it; `#_FILENAME` in that slot's program.
    fn program_name(&self, _slot: usize) -> Vec<u16> {
        Vec::new()
    }

    /// The VM went on in program slot `slot`, which runs `program`.
    fn entered_slot(&mut self, _slot: usize, _program: &Rc<Compiled>) {}

    /// The callback CALL SPRITE or CALL TEXT runs after number `after`
    /// (-1 to start): its number, which CALLIDX() reports, and a
    /// `"@label"` to GOSUB or the name of a DEF command to call. Asked
    /// anew each time, so callbacks set or cleared by earlier ones count.
    fn next_callback(&mut self, _kind: Callbacks, _after: i32) -> Option<(i32, Vec<u16>)> {
        None
    }

    /// What CALLIDX() reports once CALL SPRITE or CALL TEXT has ended:
    /// one past the last number it went through.
    fn callbacks_end(&self, _kind: Callbacks) -> i32 {
        0
    }

    /// READ's place in the DATA, handed over before each [`Host::call`],
    /// and the DEF of the running program the call is made in, whose
    /// DATA labels a `"@label"` finds first.
    fn set_data_pos(&mut self, _pos: usize, _def: Option<u32>) {}

    /// READ's place after a [`Host::call`], which commands that read DATA
    /// from a `"@label"` move; `None` leaves it.
    fn data_pos(&self) -> Option<usize> {
        None
    }

    /// Arrays the host keeps in memory of its own, such as a graphic
    /// page's, which FREEMEM leaves out.
    fn own_arrays(&self) -> Vec<crate::value::ArrayRef> {
        Vec::new()
    }

    /// Adds to a count of FREEMEM what the host holds for the program,
    /// such as the values of sprite variables.
    fn count_held(&self, _tally: &mut crate::memory::Tally) {}
}

/// A host with no commands that records PRINT output and answers INPUT
/// from a list of lines. Useful for tests and for running programs headless.
#[derive(Debug, Default)]
pub struct TextHost {
    pub output: String,
    /// Lines typed for INPUT and LINPUT, in order.
    pub input: VecDeque<String>,
}

impl Host for TextHost {
    fn lookup(&self, _name: &str) -> Option<u32> {
        None
    }

    fn call(&mut self, _id: u32, _args: &[Value], _want: Want) -> Result<Vec<Value>> {
        unreachable!("lookup() resolves nothing")
    }

    fn print(&mut self, _screen: Option<i32>, text: &[u16]) -> Result<()> {
        self.output.push_str(&String::from_utf16_lossy(text));
        Ok(())
    }

    fn print_tab(&mut self, _screen: Option<i32>) -> Result<()> {
        let column = self
            .output
            .rsplit('\n')
            .next()
            .map_or(0, |line| line.chars().count());
        let pad = 4 - column % 4;
        self.output.extend(std::iter::repeat_n(' ', pad));
        Ok(())
    }

    fn print_newline(&mut self, _screen: Option<i32>) -> Result<()> {
        self.output.push('\n');
        Ok(())
    }

    fn input_start(&mut self) -> Result<()> {
        Ok(())
    }

    /// Echoes the next line like a terminal would.
    fn input_poll(&mut self) -> Result<Option<Vec<u16>>> {
        let line = self
            .input
            .pop_front()
            .ok_or_else(|| Error::with_detail(ErrorKind::IllegalStatement, "no more input"))?;
        self.output.push_str(&line);
        self.output.push('\n');
        Ok(Some(line.encode_utf16().collect()))
    }
}

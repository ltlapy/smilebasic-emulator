//! Interface to everything outside the language: screens, input, sound,
//! files. The frontend's runtime implements it.

use crate::bytecode::Want;
use crate::error::Result;
use crate::value::Value;

pub trait Host {
    /// Resolves a command or function name, suffix included (`"GPSET"`,
    /// `"INKEY$"`), to an id passed back to [`Host::call`].
    fn lookup(&self, name: &str) -> Option<u32>;

    /// Runs a host command or function. Omitted arguments are
    /// [`Value::Default`]. Must return exactly `want.count()` values.
    fn call(&mut self, id: u32, args: &[Value], want: Want) -> Result<Vec<Value>>;

    /// PRINT output. `screen` is `None` for the console, or TPRINT's screen.
    fn print(&mut self, screen: Option<i32>, text: &[u16]) -> Result<()>;
    /// PRINT's `,` separator.
    fn print_tab(&mut self, screen: Option<i32>) -> Result<()>;
    fn print_newline(&mut self, screen: Option<i32>) -> Result<()>;
}

/// A host with no commands that records PRINT output. Useful for tests and
/// for running programs headless.
#[derive(Debug, Default)]
pub struct TextHost {
    pub output: String,
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
        // UNVERIFIED: the console's tab width; TABSTEP defaults to 4 in SB3.
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
}

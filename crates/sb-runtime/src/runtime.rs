//! The [`Host`] that programs run against: text screens now, graphics,
//! sprites and sound later.

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::host::Host;
use sb_core::value::Value;

use crate::font::Font;
use crate::framebuffer::Framebuffer;
use crate::text::{CONSOLE, SCREENS, TextScreen};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Cls,
    Color,
    Locate,
}

const COMMANDS: &[(&str, Command)] = &[
    ("CLS", Command::Cls),
    ("COLOR", Command::Color),
    ("LOCATE", Command::Locate),
];

pub struct Runtime {
    pub text: Vec<TextScreen>,
    font: Font,
    fb: Framebuffer,
}

fn illegal() -> Error {
    ErrorKind::IllegalFunctionCall.into()
}

fn screen_id(v: &Value) -> Result<usize> {
    let id = v.to_int()?;
    usize::try_from(id)
        .ok()
        .filter(|&id| id < SCREENS)
        .ok_or_else(|| Error::with_detail(ErrorKind::OutOfRange, format!("screen {id}")))
}

/// An omittable integer argument.
fn opt_int(v: &Value) -> Result<Option<i32>> {
    match v {
        Value::Default => Ok(None),
        v => v.to_int().map(Some),
    }
}

/// Splits an optional leading screen id off the arguments: commands take
/// either `rest` arguments or a screen id plus `rest`.
fn with_screen(args: &[Value], rest: usize) -> Result<(usize, &[Value])> {
    match args.len() {
        n if n == rest => Ok((CONSOLE, args)),
        n if n == rest + 1 => Ok((screen_id(&args[0])?, &args[1..])),
        _ => Err(illegal()),
    }
}

impl Runtime {
    pub fn new(width: u32, height: u32, font: Font) -> Runtime {
        Runtime {
            text: (0..SCREENS)
                .map(|_| TextScreen::new(width, height))
                .collect(),
            font,
            fb: Framebuffer::new(width, height),
        }
    }

    /// Composes the current frame.
    pub fn render(&mut self) -> &Framebuffer {
        self.fb.clear(0xFF00_0000);
        // UNVERIFIED: screens are drawn in id order until layers exist.
        for screen in &self.text {
            screen.draw(&mut self.fb, &self.font);
        }
        &self.fb
    }

    fn screen(&mut self, id: Option<i32>) -> Result<&mut TextScreen> {
        let id = match id {
            None => CONSOLE,
            Some(id) => screen_id(&Value::Int(id))?,
        };
        Ok(&mut self.text[id])
    }

    fn run(&mut self, command: Command, args: &[Value], want: Want) -> Result<Vec<Value>> {
        match (command, want) {
            (Command::Cls, Want::Outs(0)) => {
                let (id, _) = with_screen(args, 0)?;
                self.text[id].cls();
                Ok(vec![])
            }
            (Command::Color, Want::Value) => {
                let (id, _) = with_screen(args, 0)?;
                Ok(vec![Value::Int(self.text[id].color() as i32)])
            }
            (Command::Color, Want::Outs(0)) => {
                let (id, rest) = with_screen(args, 1)?;
                let color = rest[0].to_int()? as u32;
                self.text[id].set_color(color);
                Ok(vec![])
            }
            (Command::Locate, Want::Outs(0)) => {
                let (id, rest) = with_screen(args, 2)?;
                let (x, y) = (opt_int(&rest[0])?, opt_int(&rest[1])?);
                if self.text[id].locate(x, y) {
                    Ok(vec![])
                } else {
                    Err(ErrorKind::OutOfRange.into())
                }
            }
            (Command::Locate, Want::Outs(2)) => {
                let (id, _) = with_screen(args, 0)?;
                let (x, y) = self.text[id].cursor();
                Ok(vec![Value::Int(x as i32), Value::Int(y as i32)])
            }
            _ => Err(illegal()),
        }
    }
}

impl Host for Runtime {
    fn lookup(&self, name: &str) -> Option<u32> {
        COMMANDS
            .iter()
            .position(|(n, _)| *n == name)
            .map(|i| i as u32)
    }

    fn call(&mut self, id: u32, args: &[Value], want: Want) -> Result<Vec<Value>> {
        let command = COMMANDS[id as usize].1;
        self.run(command, args, want)
    }

    fn print(&mut self, screen: Option<i32>, text: &[u16]) -> Result<()> {
        self.screen(screen)?.print(text);
        Ok(())
    }

    fn print_tab(&mut self, screen: Option<i32>) -> Result<()> {
        self.screen(screen)?.tab();
        Ok(())
    }

    fn print_newline(&mut self, screen: Option<i32>) -> Result<()> {
        self.screen(screen)?.newline();
        Ok(())
    }
}

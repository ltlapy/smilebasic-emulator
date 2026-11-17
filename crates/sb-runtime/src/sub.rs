//! The sub interpreter, which runs SUBRUN's program beside the main one
//! with screens, sprites, sounds and program slots of its own. Its screen
//! shows over the main one where XSUBSCREEN puts it, and whichever of the
//! two has the focus gets the controls and the keyboard.
//!
//! The commands that reach across, such as ENVLOAD and ENVSTAT, run on
//! [`Linked`], which hands the sub interpreter's calls to its runtime
//! with the main one at hand.

use std::rc::Rc;

use sb_core::bytecode::{Callbacks, Compiled, Want};
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::host::{Exec, Host};
use sb_core::value::{ArrayRef, Value};

use crate::graphics::PAGES;
use crate::runtime::{COMMANDS, Command, Runtime, SLOTS};
use crate::sprites::{DEFS, Sprites};

/// What the main interpreter is doing, as ENVSTAT gives it to the sub
/// interpreter.
pub mod status {
    pub const DIRECT: i32 = 0;
    pub const PAUSED: i32 = 1;
    pub const ERROR: i32 = 2;
    pub const RUNNING: i32 = 3;
    pub const EDITING: i32 = 4;
}

impl Runtime {
    /// A runtime for the sub interpreter, with this one's font, preset
    /// sprites, sounds, settings and storage, and nothing else of its.
    pub(crate) fn new_sub(&self) -> Runtime {
        let mode = self.startup_mode();
        let mut rt = Runtime::new(mode.width, mode.height, self.font.clone());
        rt.set_startup_mode(mode);
        if self.system.is_fixed() {
            rt.system.fix();
        }
        if let Some(page) = self.sprite_page() {
            rt.put_sprite_page(page.clone());
        }
        rt.sprites = Sprites::new(self.sprites.default_defs().to_vec());
        rt.sound.set_instruments(self.sound.instruments().cloned());
        rt.sound.set_sequences(self.sound.sequences());
        rt.settings = self.settings.clone();
        rt.storage = self.storage.clone();
        rt.library = Rc::clone(&self.library);
        rt.help = self.help.clone();
        rt.share_fonts(self);
        rt.env_type = 1;
        rt
    }

    /// ENVFOCUS, ENVLOAD, ENVSAVE, ENVSTAT, ENVINPUT$, ENVPROJECT, PUSHKEY,
    /// CLIPBOARD and HELPINFO OUT on the sub interpreter, which reach the main one, or
    /// `None` for a call that is the sub interpreter's own.
    fn linked_command(
        &mut self,
        main: &mut Runtime,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Option<Result<Vec<Value>>> {
        let int = Value::Int;
        let result = match (command, want, args) {
            (Command::Envstat, Want::Value, []) => Ok(vec![int(main.env_status)]),
            // A stopped program, by STOP or an error, can no longer go
            // on.
            (Command::Envstat, Want::Outs(0), [s]) => s.to_int().and_then(|s| match s {
                0 => {
                    if matches!(main.env_status, status::PAUSED | status::ERROR) {
                        main.env_status = status::DIRECT;
                        main.forget_stop = true;
                    }
                    Ok(vec![])
                }
                s => Err(Error::with_detail(ErrorKind::OutOfRange, format!("{s}"))),
            }),
            (Command::Envstat, Want::Outs(3), []) => Ok(vec![
                int(main.env_status),
                int(main.running_slot.unwrap_or(0) as i32),
                int(main.edit_slot as i32),
            ]),
            // While the editor shows, nothing of the last error.
            (Command::Envstat, Want::Outs(6), []) => {
                let mut out = vec![int(main.env_status)];
                match main.env_status {
                    status::EDITING => out.extend([0; 5].map(int)),
                    _ => out.extend(main.env_error.map(int)),
                }
                Ok(out)
            }
            (Command::Envload | Command::Envsave, Want::Outs(0), [kind, from, to]) => {
                let load = command == Command::Envload;
                copy_across(main, self, load, kind, from, to).map(|()| vec![])
            }
            (Command::Envinput | Command::Envlocate | Command::Clipboard, _, _) => {
                main.run_counted(command, args, want)
            }
            // The main interpreter's cursor, if this program may see it.
            (Command::Helpinfo, Want::Outs(2), []) => {
                let system = self.storage.project().eq_ignore_ascii_case("#SYS");
                Ok(main.helpinfo_out(system))
            }
            (Command::Envproject, Want::Outs(0), []) => {
                // The main interpreter's project was set before, so it
                // can be set again.
                let project = main.storage.project().to_string();
                let _ = self.storage.set_project(&project);
                Ok(vec![])
            }
            // Calls from subprograms are ignored (the manual says so).
            (
                Command::Irsprite
                | Command::Irstart
                | Command::Irstop
                | Command::Vibrate
                | Command::Xctrlstyle,
                Want::Outs(0),
                _,
            ) => Ok(vec![]),
            // The subprogram's SNDMVOL checks its arguments, then does
            // nothing: the main program's sets the level of both.
            (Command::Sndmvol, Want::Outs(0), [_] | [_, _]) => {
                crate::snd::sndmvol_args(args).map(|_| vec![])
            }
            // The keys go to the interpreter with the focus, so that a
            // subprogram can type into the main one's direct mode.
            (Command::Pushkey, Want::Outs(0), [_]) if !self.focus => {
                if !self.frame_wait_over() {
                    if let Err(e) = main.pushkey(&args[0]) {
                        return Some(Err(e));
                    }
                    self.hold_for_a_frame();
                }
                Ok(vec![])
            }
            _ => return None,
        };
        Some(result)
    }
}

/// ENVLOAD (`load`) or ENVSAVE of a program slot (kind 0, ENVLOAD only),
/// a graphic page (1) or a sprite definition (2). ENVSAVE of kind 3 or 4
/// copies nothing.
fn copy_across(
    main: &mut Runtime,
    sub: &mut Runtime,
    load: bool,
    kind: &Value,
    from: &Value,
    to: &Value,
) -> Result<()> {
    if !load && matches!(crate::env::envsave_kind(kind)?, 3 | 4) {
        return Ok(());
    }
    let (kind, from, to) = (kind.to_int()?, from.to_int()?, to.to_int()?);
    let (source, dest) = if load { (main, sub) } else { (sub, main) };
    let index = |n: i32, count: usize| {
        usize::try_from(n)
            .ok()
            .filter(|&n| n < count)
            .ok_or_else(|| Error::with_detail(ErrorKind::OutOfRange, format!("{n}")))
    };
    match kind {
        0 if load => {
            let (from, to) = (index(from, SLOTS)?, index(to, SLOTS)?);
            dest.slots[to] = source.slots[from].clone();
            dest.slot_names[to] = source.slot_names[from].clone();
            dest.slot_changes[to] += 1;
            dest.replaced_lines(to, source.slot_file(from));
        }
        1 => {
            let (from, to) = (index(from, PAGES)?, index(to, PAGES)?);
            dest.grp.set_page(to, source.grp.page(from).clone());
        }
        2 => {
            let (from, to) = (index(from, DEFS)?, index(to, DEFS)?);
            dest.sprites.defs[to] = source.sprites.defs[from];
        }
        n => return Err(Error::with_detail(ErrorKind::OutOfRange, format!("{n}")).of_arg(0)),
    }
    Ok(())
}

/// The sub interpreter's runtime as its program's host, with the main
/// interpreter's runtime for the commands that reach it.
pub(crate) struct Linked<'a> {
    pub sub: &'a mut Runtime,
    pub main: &'a mut Runtime,
}

impl Host for Linked<'_> {
    fn lookup(&self, name: &str) -> Option<u32> {
        self.sub.lookup(name)
    }

    fn work(&mut self, work: sb_core::vm::Work) {
        self.sub.work(work);
    }

    fn out_of_time(&self) -> bool {
        self.sub.out_of_time()
    }

    fn call(&mut self, id: u32, args: &[Value], want: Want) -> Result<Vec<Value>> {
        let command = COMMANDS[id as usize].1;
        match self.sub.linked_command(self.main, command, args, want) {
            Some(result) => {
                result.map_err(|e| crate::runtime::command_error(e, COMMANDS[id as usize].0, args))
            }
            None => self.sub.call(id, args, want),
        }
    }

    fn print(&mut self, screen: Option<i32>, text: &[u16]) -> Result<()> {
        self.sub.print(screen, text)
    }

    fn print_tab(&mut self, screen: Option<i32>) -> Result<()> {
        self.sub.print_tab(screen)
    }

    fn print_newline(&mut self, screen: Option<i32>) -> Result<()> {
        self.sub.print_newline(screen)
    }

    fn input_start(&mut self) -> Result<()> {
        self.sub.input_start()
    }

    fn input_poll(&mut self) -> Result<Option<Vec<u16>>> {
        self.sub.input_poll()
    }

    fn retry(&mut self) -> bool {
        self.sub.retry()
    }

    fn exec_source(&mut self, file: Option<&[u16]>, slot: i32) -> Result<Exec> {
        self.sub.exec_source(file, slot)
    }

    fn program_name(&self, slot: usize) -> Vec<u16> {
        self.sub.program_name(slot)
    }

    fn entered_slot(&mut self, slot: usize, program: &Rc<Compiled>) {
        self.sub.entered_slot(slot, program)
    }

    fn next_callback(&mut self, kind: Callbacks, after: i32) -> Option<(i32, Vec<u16>)> {
        self.sub.next_callback(kind, after)
    }

    fn callbacks_end(&self, kind: Callbacks) -> i32 {
        self.sub.callbacks_end(kind)
    }

    fn set_data_pos(&mut self, pos: usize, def: Option<u32>) {
        self.sub.set_data_pos(pos, def)
    }

    fn data_pos(&self) -> Option<usize> {
        self.sub.data_pos()
    }

    fn own_arrays(&self) -> Vec<ArrayRef> {
        self.sub.own_arrays()
    }

    fn count_held(&self, tally: &mut sb_core::memory::Tally) {
        self.sub.count_held(tally);
    }
}

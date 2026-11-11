//! Questions that commands wait on, and the notices shown instead of them.
//! SmileBASIC 4 asks before every file read and write, except a LOAD in a
//! running program; the emulator lets that be turned into a notice or silenced, but
//! always asks before unsaved edits are lost.

use crate::runtime::Runtime;

/// How file commands typed in direct mode confirm.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileConfirm {
    /// Ask first, as SmileBASIC 4 does.
    #[default]
    Ask,
    /// Go ahead and show a notice.
    Notify,
    Silent,
}

/// A question a command waits on; the frontend shows it and calls
/// [`Runtime::answer`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirm {
    pub title: String,
    pub lines: Vec<String>,
    /// The label of the button that goes ahead.
    pub accept: &'static str,
    /// Whether the button that goes ahead is chosen at first, as on
    /// SmileBASIC 4's file questions. Otherwise Cancel is, so that a
    /// quick press does not throw unsaved edits away.
    pub accept_first: bool,
}

/// How a question stands.
pub(crate) enum Ask {
    /// No question was needed, or the user agreed.
    Go,
    Refused,
    /// No answer yet; the command runs again next frame.
    Wait,
}

/// What a file command is about to do, for its question or notice.
pub(crate) struct FileAction {
    pub title: String,
    /// One sentence saying what happens.
    pub summary: String,
    /// The project and file lines.
    pub details: Vec<String>,
    pub accept: &'static str,
    /// The one line shown when notifying instead of asking.
    pub notice: String,
    /// Whether it also asks in a running program. On SmileBASIC 4, LOAD
    /// there asks only when it would replace unsaved edits.
    pub in_programs: bool,
    /// Files of the folder a program was run from that it would replace,
    /// delete or rename, by their names on disk, of those there from the
    /// start and not yet changed (see [`crate::storage::Storage::original`]).
    /// Any makes it ask, whatever the setting.
    pub originals: Vec<String>,
}

const HINT: [&str; 2] = ["To stop these questions, go to", "File > Settings > Files."];

/// Warns that `slots` have unsaved changes that will be lost.
pub fn unsaved_lines(slots: &[usize]) -> Vec<String> {
    let names: Vec<String> = slots.iter().map(usize::to_string).collect();
    let first = match slots {
        [slot] => format!("Slot {slot} has unsaved changes,"),
        _ => format!("Slots {} have unsaved changes,", names.join(", ")),
    };
    vec![first, "which will be lost.".to_string()]
}

/// Says that `files`, there before the emulator started, will change.
pub fn original_lines(files: &[String]) -> Vec<String> {
    let first = match files {
        [file] => format!("This changes the original file {file}"),
        _ => format!("This changes the original files {}", files.join(", ")),
    };
    vec![first, "in the folder the program was run from.".to_string()]
}

/// Before switching from project `old` to `new` with `unsaved` slots.
pub fn switch_lines(new: &str, old: &str, unsaved: &[usize]) -> Vec<String> {
    let names: Vec<String> = unsaved.iter().map(usize::to_string).collect();
    vec![
        format!("Switch to project {new}."),
        String::new(),
        format!("Unsaved changes in slot {}", names.join(", ")),
        format!("are not saved in project {old}."),
    ]
}

/// A project's name as shown to the user: the workspace is `[WORKSPACE]`,
/// as on SmileBASIC 4.
pub fn project_label(name: &str) -> &str {
    match name {
        "" => "[WORKSPACE]",
        name => name,
    }
}

impl Runtime {
    /// The question waiting for an answer, if any.
    pub fn confirmation(&self) -> Option<&Confirm> {
        self.confirm.as_ref().filter(|_| self.answer.is_none())
    }

    /// Answers the waiting question: `true` goes ahead.
    pub fn answer(&mut self, yes: bool) {
        if self.confirm.is_some() {
            self.answer = Some(yes);
        }
    }

    /// Drops a question or DIALOG whose command was stopped.
    pub fn cancel_confirm(&mut self) {
        self.confirm = None;
        self.answer = None;
        self.dialog = None;
        self.dialog_answer = None;
        self.listing_left = None;
        self.inspect_left = None;
    }

    /// Notices for the frontend to show briefly, since the last call.
    pub fn take_notices(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notices)
    }

    /// Those of `slots` that have unsaved edits.
    pub(crate) fn unsaved_among(&self, slots: impl IntoIterator<Item = usize>) -> Vec<usize> {
        slots
            .into_iter()
            .filter(|&s| self.slot_modified[s])
            .collect()
    }

    /// The project's name as shown to the user.
    pub(crate) fn project_label(&self) -> String {
        project_label(self.storage.project()).to_string()
    }

    /// Asks `question` (built once) and waits for the answer.
    pub(crate) fn ask(&mut self, question: impl FnOnce(&mut Runtime) -> Confirm) -> Ask {
        match self.answer.take() {
            Some(yes) => {
                self.confirm = None;
                if yes { Ask::Go } else { Ask::Refused }
            }
            None => {
                if self.confirm.is_none() {
                    self.confirm = Some(question(self));
                }
                self.retry = true;
                Ask::Wait
            }
        }
    }

    /// Confirms a file command as the settings say. Replacing slots in
    /// `replaces` that have unsaved edits always asks, as does changing
    /// one of the [`FileAction::originals`].
    pub(crate) fn confirm_file(&mut self, action: FileAction, replaces: &[usize]) -> Ask {
        let unsaved = self.unsaved_among(replaces.iter().copied());
        let originals = !action.originals.is_empty();
        let mode = if !unsaved.is_empty() || originals {
            FileConfirm::Ask
        } else if self.direct || action.in_programs {
            self.settings.file_confirm
        } else {
            FileConfirm::Silent
        };
        match mode {
            FileConfirm::Silent => Ask::Go,
            FileConfirm::Notify => {
                self.notices.push(action.notice);
                Ask::Go
            }
            FileConfirm::Ask => self.ask(|rt| {
                let mut lines = vec![action.summary, String::new()];
                lines.extend(action.details);
                if originals {
                    lines.push(String::new());
                    lines.extend(original_lines(&action.originals));
                }
                if !unsaved.is_empty() {
                    lines.push(String::new());
                    lines.extend(unsaved_lines(&unsaved));
                } else if !originals && !rt.settings.file_hint_shown {
                    rt.settings.file_hint_shown = true;
                    lines.push(String::new());
                    lines.extend(HINT.map(String::from));
                }
                // SmileBASIC 4 starts on the button that goes ahead for
                // DELETE and RENAME.
                // UNVERIFIED: that it does for the other file commands but
                // saving, and which button it starts on when unsaved edits
                // would be lost.
                Confirm {
                    title: action.title,
                    lines,
                    accept: action.accept,
                    accept_first: unsaved.is_empty() && !originals,
                }
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsaved_lines_name_the_slots() {
        assert_eq!(
            unsaved_lines(&[0]),
            ["Slot 0 has unsaved changes,", "which will be lost."]
        );
        assert_eq!(
            unsaved_lines(&[1, 3])[0],
            "Slots 1, 3 have unsaved changes,"
        );
    }

    #[test]
    fn original_lines_name_the_files() {
        let one = original_lines(&["HGLIB.PRG.txt".to_string()]);
        assert_eq!(one[0], "This changes the original file HGLIB.PRG.txt");
        let two = original_lines(&["A".to_string(), "B".to_string()]);
        assert_eq!(two[0], "This changes the original files A, B");
    }
}

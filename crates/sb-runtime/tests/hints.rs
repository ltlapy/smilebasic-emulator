//! Checks the argument hints against every form of SmileBASIC 4's help,
//! read from `$SB_HELP`, the resource directory the importer wrote
//! `HelpData.idx` and `HelpData.dat` to from your own copy. Skipped when
//! the variable is unset.
//!
//! For each form, this types the command and its arguments one by one,
//! as the form writes their separators, and checks that the hint marks
//! the argument being typed: right after the separator, inside a string
//! being typed, and after the argument.
//!
//! Run with `SB_HELP=/path/resources cargo test -p sb-runtime --test
//! hints -- --nocapture`.

use sb_runtime::complete::Library;
use sb_runtime::hint::{self, Spans};

/// Forms the help writes in a way no split fits.
const UNCLEAN: &[&str] = &[
    // The quote is never closed.
    "FILES \"[FileType:]ProjectName,StringArray",
    // One `]` too many.
    "DEF CommandName[Parameter[,Parameter...]] OUT ReturnValue [,ReturnValue...]]",
];

/// Whether a marked argument is whole: quotes, brackets and parentheses
/// closed, and no separator outside them.
fn is_clean(text: &str) -> bool {
    let (mut quoted, mut brackets, mut parens) = (false, 0i32, 0i32);
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            _ if quoted => {}
            '[' => brackets += 1,
            ']' => brackets -= 1,
            '(' => parens += 1,
            ')' => parens -= 1,
            ',' | ';' if brackets == 0 => return false,
            _ => {}
        }
        if brackets < 0 || parens < 0 {
            return false;
        }
    }
    !text.trim().is_empty() && !quoted && brackets == 0 && parens == 0
}

fn slice(chars: &[char], (a, b): (usize, usize)) -> String {
    chars[a..b].iter().collect()
}

/// The text to type for an argument.
fn sample(text: &str) -> &'static str {
    match text.chars().next() {
        Some('"') => "\"A\"",
        Some('@') => "@A",
        _ => "1",
    }
}

/// The text to type between two arguments, from the form's text between
/// them: `None` where a new statement starts.
fn separator(between: &str) -> Option<String> {
    let rest = between.trim_matches(|c: char| "[]. …".contains(c));
    match rest.chars().next() {
        Some(c @ (',' | ';')) => Some(c.to_string()),
        _ => {
            let word: String = rest.chars().take_while(|c| c.is_alphanumeric()).collect();
            match word.as_str() {
                "THEN" | "ELSE" | "ENDIF" => None,
                "" => Some(",".into()),
                _ => Some(format!(" {word} ")),
            }
        }
    }
}

/// Types the arguments of `list` after `prefix`, checking the hint for
/// each. Returns the line with all of them typed, or `None` if a new
/// statement started.
fn type_list(
    (topic, form, chars): (&str, &str, &[char]),
    library: &Library,
    (list, out): (&Spans, bool),
    prefix: String,
    failures: &mut Vec<String>,
) -> Option<String> {
    let mut line = prefix;
    for (k, &span) in list.iter().enumerate() {
        if k > 0 {
            let between = slice(chars, (list[k - 1].1, span.0));
            line += &separator(&between)?;
        }
        let text = slice(chars, span);
        let mut check = |typed: &str| {
            let units: Vec<u16> = typed.encode_utf16().collect();
            let got = hint::find(&units, units.len(), library, &[]);
            let ok = got.as_ref().is_some_and(|h| {
                h.name == topic && h.arg == k && h.out == out && h.mark(form) == Some(span)
            });
            if !ok {
                let marked = got.and_then(|h| h.mark(form)).map(|s| slice(chars, s));
                failures.push(format!(
                    "{form:?}: typed {typed:?}, want {text:?}, marked {marked:?}"
                ));
            }
        };
        check(&line);
        if text.starts_with('"') {
            check(&format!("{line}\"AB"));
        }
        line += sample(&text);
        check(&line);
    }
    Some(line)
}

#[test]
fn every_form_of_the_help() {
    let Some(dir) = std::env::var_os("SB_HELP") else {
        return;
    };
    let help = sb_runtime::resources::load_help(dir.as_ref()).expect("SB_HELP holds the help");
    let library = Library::new(Some(&help));
    let mut topics: Vec<&str> = help.topics().collect();
    topics.sort_unstable();
    let (mut forms, mut arguments) = (0, 0);
    let mut failures = Vec::new();
    for topic in topics {
        // The start page and the pages of MML belong to no command.
        if matches!(topic, "DEFAULTHELPKEY" | "MML") {
            continue;
        }
        for form in library.forms(topic) {
            if form.starts_with("About ") {
                continue;
            }
            forms += 1;
            let chars: Vec<char> = form.chars().collect();
            let args = hint::arguments(form, topic);
            for &span in args.ins.iter().chain(&args.outs) {
                arguments += 1;
                let text = slice(&chars, span);
                if !is_clean(&text) && !UNCLEAN.iter().any(|u| form.starts_with(u)) {
                    failures.push(format!("{form:?}: {text:?} is not a whole argument"));
                }
            }
            let prefix = match args.call {
                true => format!("X={topic}("),
                false => format!("{topic} "),
            };
            let at = (topic, form.as_str(), chars.as_slice());
            let Some(line) = type_list(at, &library, (&args.ins, false), prefix, &mut failures)
            else {
                continue;
            };
            if !args.outs.is_empty() {
                let prefix = format!("{} OUT ", line.trim_end());
                type_list(at, &library, (&args.outs, true), prefix, &mut failures);
            }
        }
    }
    println!("{forms} forms, {arguments} arguments");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

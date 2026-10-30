//! Colors for the editor: which part of the language each character of a
//! line belongs to. Lines are read one at a time, as SmileBASIC 4's editor
//! colors them; nothing in the language spans lines except `\` joins,
//! which are left alone.

use sb_core::lexer::Keyword;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Names of variables, operators and anything else.
    Character,
    Numeric,
    String,
    /// `#NAME`.
    Constant,
    /// `@NAME`.
    Label,
    /// The names of SmileBASIC's instructions and functions.
    Command,
    Reserved,
    Comment,
}

/// Lines that start something, drawn on their own background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Plain,
    /// A line starting with DEF or COMMON DEF.
    Def,
    /// A line starting with a label.
    Label,
}

/// The instructions and functions of SmileBASIC 4's reference that are not
/// reserved words, sorted for binary search.
const COMMANDS: &[&str] = &[
    "ABS",
    "ACCEL",
    "ACLS",
    "ACOS",
    "ANIMDEF",
    "ARRAY#",
    "ARRAY$",
    "ARRAY%",
    "ARYOP",
    "ASC",
    "ASIN",
    "ATAN",
    "ATTR",
    "BACKCOLOR",
    "BACKTRACE",
    "BEEP",
    "BEEPPAN",
    "BEEPPIT",
    "BEEPSTOP",
    "BEEPVOL",
    "BGMCHK",
    "BGMCLEAR",
    "BGMCONT",
    "BGMPAUSE",
    "BGMPITCH",
    "BGMPLAY",
    "BGMSET",
    "BGMSETD",
    "BGMSTOP",
    "BGMVAR",
    "BGMVOL",
    "BGMWET",
    "BIN$",
    "BIQUAD",
    "BQPARAM",
    "BREPEAT",
    "BUTTON",
    "CALLIDX",
    "CEIL",
    "CHKCALL",
    "CHKCHR",
    "CHKFILE",
    "CHKLABEL",
    "CHKMML",
    "CHKVAR",
    "CHR$",
    "CLASSIFY",
    "CLEAR",
    "CLIPBOARD",
    "CLS",
    "COLOR",
    "CONT",
    "CONTROLLER",
    "COPY",
    "COS",
    "COSH",
    "DATE$",
    "DEC",
    "DEFARG",
    "DEFARGC",
    "DEFOUTC",
    "DEG",
    "DELETE",
    "DIALOG",
    "DTREAD",
    "EFCEN",
    "EFCSET",
    "EFCWET",
    "ENVFOCUS",
    "ENVINPUT$",
    "ENVLOAD",
    "ENVPROJECT",
    "ENVSAVE",
    "ENVSTAT",
    "ENVTYPE",
    "EXP",
    "FADE",
    "FADECHK",
    "FFT",
    "FFTWFN",
    "FILES",
    "FILL",
    "FIND",
    "FLOAT",
    "FLOOR",
    "FONTINFO",
    "FORMAT$",
    "FREEMEM",
    "GARRAY",
    "GBOX",
    "GCIRCLE",
    "GCLIP",
    "GCLS",
    "GCOLOR",
    "GCOPY",
    "GFILL",
    "GLINE",
    "GLOAD",
    "GPAINT",
    "GPGET",
    "GPSET",
    "GPUTCHR",
    "GSAMPLE",
    "GSAVE",
    "GTARGET",
    "GTRI",
    "GUPDATE",
    "GYROA",
    "GYROSYNC",
    "GYROV",
    "HELPGET",
    "HELPINFO",
    "HEX$",
    "HSV",
    "HSVF",
    "IFFT",
    "INC",
    "INKEY$",
    "INSERT",
    "INSPECT",
    "INSTR",
    "INT",
    "IRREAD",
    "IRSPRITE",
    "IRSTART",
    "IRSTATE",
    "IRSTOP",
    "KEY",
    "KEYBOARD",
    "LAST",
    "LAYER",
    "LCLIP",
    "LEFT$",
    "LEN",
    "LFILTER",
    "LIST",
    "LMATRIX",
    "LOAD",
    "LOADG",
    "LOADV",
    "LOCATE",
    "LOG",
    "MAINCNT",
    "MAX",
    "MBUTTON",
    "METAEDIT",
    "METALOAD",
    "METASAVE",
    "MID$",
    "MILLISEC",
    "MIN",
    "MML",
    "MOUSE",
    "NEW",
    "OPTION",
    "PCMCONT",
    "PCMPOS",
    "PCMSTOP",
    "PCMSTREAM",
    "PCMVOL",
    "PERFBEGIN",
    "PERFEND",
    "POP",
    "POW",
    "PRGDEL",
    "PRGEDIT",
    "PRGGET$",
    "PRGINS",
    "PRGNAME$",
    "PRGSEEK",
    "PRGSET",
    "PRGSIZE",
    "PROJECT",
    "PUSH",
    "PUSHKEY",
    "RAD",
    "RANDOMIZE",
    "RECCHK",
    "RECDATA",
    "RECLEN",
    "RECPOS",
    "RECSAVE",
    "RECSTART",
    "RECSTOP",
    "REMOVE",
    "RENAME",
    "RESIZE",
    "RESULT",
    "RGB",
    "RGBF",
    "RIGHT$",
    "RINGCOPY",
    "RND",
    "RNDF",
    "ROUND",
    "RSORT",
    "RUN",
    "SAVE",
    "SAVEG",
    "SAVEV",
    "SCROLL",
    "SGN",
    "SHIFT",
    "SIN",
    "SINH",
    "SNDMSBAL",
    "SNDMVOL",
    "SNDSTOP",
    "SORT",
    "SPANIM",
    "SPCHK",
    "SPCHR",
    "SPCLR",
    "SPCOL",
    "SPCOLOR",
    "SPCOLVEC",
    "SPDEF",
    "SPFUNC",
    "SPHIDE",
    "SPHITINFO",
    "SPHITRC",
    "SPHITSP",
    "SPHOME",
    "SPLAYER",
    "SPLINK",
    "SPOFS",
    "SPPAGE",
    "SPROT",
    "SPSCALE",
    "SPSET",
    "SPSHOW",
    "SPSTART",
    "SPSTOP",
    "SPUNLINK",
    "SPUSED",
    "SPVAR",
    "SQR",
    "STICK",
    "STOP",
    "STR$",
    "SUBHIDE",
    "SUBRUN",
    "SUBSHOW",
    "SUBST$",
    "SUBSTOP",
    "SYSPARAM",
    "TALK",
    "TALKCHK",
    "TALKSTOP",
    "TAN",
    "TANH",
    "TANIM",
    "TARRAY",
    "TBLEND",
    "TCBIKE",
    "TCCAR",
    "TCFISHING",
    "TCHK",
    "TCHOUSE",
    "TCOLOR",
    "TCOORD",
    "TCOPY",
    "TCPIANO",
    "TCPLANE",
    "TCROBOT",
    "TCSUBM",
    "TCVEHICLE",
    "TCVISOR",
    "TFILL",
    "TFUNC",
    "THIDE",
    "THOME",
    "TIME$",
    "TLAYER",
    "TLOAD",
    "TMREAD",
    "TOFS",
    "TOUCH",
    "TPAGE",
    "TPUT",
    "TRACE",
    "TROT",
    "TSAVE",
    "TSCALE",
    "TSCREEN",
    "TSHOW",
    "TSTART",
    "TSTOP",
    "TUPDATE",
    "TVAR",
    "TYPEOF",
    "UNSHIFT",
    "VAL",
    "VIBRATE",
    "VSYNC",
    "WAIT",
    "WAVSET",
    "WAVSETA",
    "XCTRLSTYLE",
    "XSCREEN",
    "XSUBSCREEN",
];

/// Words SmileBASIC 4's editor colors as reserved (checked on hardware),
/// though the language specification does not reserve them. OPTION is
/// colored as a command.
const ALSO_RESERVED: &[&str] = &["TO", "STEP", "CONST", "ENUM"];

fn is_ident_start(c: u16) -> bool {
    c == u16::from(b'_') || (c < 0x80 && (c as u8).is_ascii_alphabetic())
}

fn is_ident_char(c: u16) -> bool {
    c == u16::from(b'_') || (c < 0x80 && (c as u8).is_ascii_alphanumeric())
}

fn is_digit(c: Option<&u16>) -> bool {
    c.is_some_and(|&c| c < 0x80 && (c as u8).is_ascii_digit())
}

fn is(c: Option<&u16>, ascii: u8) -> bool {
    c.is_some_and(|&c| c == u16::from(ascii))
}

/// The role of each character of `line`, and what the line starts with.
pub fn highlight(line: &[u16]) -> (Vec<Role>, LineKind) {
    let mut roles = vec![Role::Character; line.len()];
    // The words read so far, for telling what the line starts with.
    let mut first: Vec<String> = Vec::new();
    let mut starts_with_label = false;
    let mut i = 0;
    while i < line.len() {
        let c = line[i];
        let start = i;
        let role = match c {
            0x27 => {
                i = line.len();
                Role::Comment
            }
            0x22 => {
                i += 1;
                while i < line.len() && line[i] != 0x22 {
                    i += 1;
                }
                i = (i + 1).min(line.len());
                Role::String
            }
            _ if is_digit(Some(&c)) || (c == u16::from(b'.') && is_digit(line.get(i + 1))) => {
                i = number_end(line, i);
                Role::Numeric
            }
            // `&H` and `&B` literals; `&&` is an operator.
            0x26 if matches!(line.get(i + 1), Some(&c) if b"HhBb".contains(&(c as u8)) && c < 0x80) =>
            {
                i += 2;
                while i < line.len() && line[i] < 0x80 && (line[i] as u8).is_ascii_hexdigit() {
                    i += 1;
                }
                Role::Numeric
            }
            // Labels may also start with a digit.
            0x40 if line.get(i + 1).is_some_and(|&c| is_ident_char(c)) => {
                i += 1;
                while i < line.len() && is_ident_char(line[i]) {
                    i += 1;
                }
                starts_with_label |= first.is_empty();
                Role::Label
            }
            0x23 if line.get(i + 1).is_some_and(|&c| is_ident_start(c)) => {
                i += 1;
                while i < line.len() && is_ident_char(line[i]) {
                    i += 1;
                }
                // Constants can carry a type suffix too (`#LF$`).
                if line
                    .get(i)
                    .is_some_and(|&c| b"$%#".contains(&(c as u8)) && c < 0x80)
                {
                    i += 1;
                }
                Role::Constant
            }
            _ if is_ident_start(c) => {
                while i < line.len() && is_ident_char(line[i]) {
                    i += 1;
                }
                let mut word = String::from_utf16_lossy(&line[start..i]).to_ascii_uppercase();
                let suffix = line
                    .get(i)
                    .filter(|&&c| b"$%#".contains(&(c as u8)) && c < 0x80);
                if let Some(&s) = suffix {
                    word.push(char::from(s as u8));
                    i += 1;
                }
                if first.len() < 2 && !starts_with_label {
                    first.push(word.clone());
                }
                if suffix.is_none() && word == "REM" {
                    i = line.len();
                    Role::Comment
                } else if suffix.is_none()
                    && (Keyword::from_word(&word).is_some() || ALSO_RESERVED.contains(&&*word))
                {
                    Role::Reserved
                } else if COMMANDS.binary_search(&&*word).is_ok() {
                    Role::Command
                } else {
                    Role::Character
                }
            }
            _ => {
                i += 1;
                Role::Character
            }
        };
        roles[start..i].fill(role);
    }
    let kind = if starts_with_label {
        LineKind::Label
    } else {
        match first.iter().map(String::as_str).collect::<Vec<_>>()[..] {
            ["DEF", ..] | ["COMMON", "DEF"] => LineKind::Def,
            _ => LineKind::Plain,
        }
    };
    (roles, kind)
}

/// Where a decimal number starting at `i` ends, read as the lexer does:
/// digits, a fraction, and an exponent only when digits follow it.
/// What a line defines, for the editor's index: the label it starts
/// with, or the function its DEF names, as written.
pub fn definition(line: &[u16]) -> Option<String> {
    let (roles, kind) = highlight(line);
    let word = |from: usize, to: usize| String::from_utf16_lossy(&line[from..to]);
    match kind {
        LineKind::Plain => None,
        LineKind::Label => {
            let start = roles.iter().position(|&r| r == Role::Label)?;
            let len = roles[start..]
                .iter()
                .take_while(|&&r| r == Role::Label)
                .count();
            Some(word(start, start + len))
        }
        LineKind::Def => {
            let upper: Vec<u16> = line
                .iter()
                .map(|&c| {
                    if (0x61..=0x7A).contains(&c) {
                        c - 0x20
                    } else {
                        c
                    }
                })
                .collect();
            let def: Vec<u16> = "DEF".encode_utf16().collect();
            let at = upper.windows(3).position(|w| w == def.as_slice())? + 3;
            let start = at + line[at..].iter().take_while(|&&c| c == 32).count();
            let mut end = start
                + line[start..]
                    .iter()
                    .take_while(|&&c| is_ident_char(c))
                    .count();
            if line
                .get(end)
                .is_some_and(|&c| c < 0x80 && b"$%#".contains(&(c as u8)))
            {
                end += 1;
            }
            (end > start).then(|| format!("DEF {}", word(start, end)))
        }
    }
}

fn number_end(line: &[u16], mut i: usize) -> usize {
    while is_digit(line.get(i)) {
        i += 1;
    }
    if is(line.get(i), b'.') {
        i += 1;
        while is_digit(line.get(i)) {
            i += 1;
        }
    }
    if is(line.get(i), b'E') || is(line.get(i), b'e') {
        let digits = if is(line.get(i + 1), b'+') || is(line.get(i + 1), b'-') {
            i + 2
        } else {
            i + 1
        };
        if is_digit(line.get(digits)) {
            i = digits;
            while is_digit(line.get(i)) {
                i += 1;
            }
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_for_the_index() {
        let def = |text: &str| definition(&text.encode_utf16().collect::<Vec<_>>());
        assert_eq!(def("@MAIN 'start").as_deref(), Some("@MAIN"));
        assert_eq!(def("  @loop:GOTO @X").as_deref(), Some("@loop"));
        assert_eq!(def("DEF Add(A,B)").as_deref(), Some("DEF Add"));
        assert_eq!(def("COMMON DEF NAME$ X").as_deref(), Some("DEF NAME$"));
        assert_eq!(def("PRINT 1"), None);
        assert_eq!(def("GOTO @MAIN"), None);
    }

    /// One letter per character: `.` character, `n` numeric, `s` string,
    /// `k` constant, `l` label, `c` command, `r` reserved, `'` comment.
    fn roles(text: &str) -> (String, LineKind) {
        let line: Vec<u16> = text.encode_utf16().collect();
        let (roles, kind) = highlight(&line);
        let letters = roles
            .iter()
            .map(|r| match r {
                Role::Character => '.',
                Role::Numeric => 'n',
                Role::String => 's',
                Role::Constant => 'k',
                Role::Label => 'l',
                Role::Command => 'c',
                Role::Reserved => 'r',
                Role::Comment => '\'',
            })
            .collect();
        (letters, kind)
    }

    #[test]
    fn colors_each_part() {
        assert_eq!(
            roles("PRINT \"HI\";A$:LOCATE 1,2.5E3").0,
            "rrrrr.ssss....cccccc.n.nnnnn"
        );
        assert_eq!(roles("X=&HFF+&B10&&#C_RED").0, "..nnnn.nnnn..kkkkkk");
        assert_eq!(roles("GOTO @LOOP 'back").0, "rrrr.lllll.'''''");
        assert_eq!(roles("rem note").0, "''''''''");
        // Names with a suffix, and names that only start like a command.
        assert_eq!(roles("A=LEFT$(B$,1)+PRINTX").0, "..ccccc....n........");
        assert_eq!(roles("FOR I=0 TO 9 STEP 2").0, "rrr...n.rr.n.rrrr.n");
        // An unclosed string runs to the end of the line.
        assert_eq!(roles("?\"AB:'C").0, ".ssssss");
        assert_eq!(roles("A=1ELSE").0, "..nrrrr");
        assert_eq!(roles("OPTION STRICT").0, "cccccc.......");
    }

    #[test]
    fn def_and_label_lines() {
        assert_eq!(roles("DEF F(X)").1, LineKind::Def);
        assert_eq!(roles("  common def F").1, LineKind::Def);
        assert_eq!(roles("@MAIN").1, LineKind::Label);
        assert_eq!(roles("  @MAIN:CLS").1, LineKind::Label);
        assert_eq!(roles("@1ST").1, LineKind::Label);
        assert_eq!(roles("GOTO @MAIN").1, LineKind::Plain);
        assert_eq!(roles("X=1:DEF F").1, LineKind::Plain);
        assert_eq!(roles("'DEF").1, LineKind::Plain);
    }

    #[test]
    fn commands_are_sorted() {
        assert!(COMMANDS.windows(2).all(|w| w[0] < w[1]));
    }
}

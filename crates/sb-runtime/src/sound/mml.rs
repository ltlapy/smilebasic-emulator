//! MML, the music language of BGMPLAY, BGMSET and BGMSETD, parsed into a
//! list of commands for each channel. Octaves, lengths and variables are
//! left to the player, since repeats and BGMVAR change them as it plays.

use std::collections::HashMap;
use std::rc::Rc;

pub const CHANNELS: usize = 16;
/// Ticks in a whole note: every length divides it.
pub const WHOLE: u32 = 192;
/// How deep macros may use other macros.
const MACRO_DEPTH: usize = 16;

/// A number in MML: written out, or one of the variables $0 to $7.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Num {
    Lit(i32),
    Var(u8),
}

/// A length: a divisor of a whole note (4 for a quarter) and dots, or the
/// L command's when there is no divisor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Len {
    pub div: Option<Num>,
    pub dots: u8,
}

/// What joins a note to the next: a tie or slur (`&`) or a portamento
/// (`_`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Link {
    #[default]
    None,
    Tie,
    Porta,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pitch {
    /// Semitones from C in the current octave, accidentals included.
    Name(i32),
    /// N's note number.
    Number(Num),
    Rest,
}

/// Within a chord: notes, and octave changes between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChordPart {
    Note(i32),
    Octave(Num),
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    Note {
        pitch: Pitch,
        len: Len,
        link: Link,
    },
    Chord {
        parts: Vec<ChordPart>,
        len: Len,
        link: Link,
    },
    Length(Len),
    Octave(Num),
    Up,
    Down,
    Tempo(Num),
    Program(Num),
    Volume(Num),
    Louder(Num),
    Softer(Num),
    Velocity(Num),
    Pan(Num),
    Gate(Num),
    Key(Num),
    /// K before the first channel: the whole song's key.
    SongKey(Num),
    Detune(Num),
    Envelope([Num; 4]),
    EnvelopeOff,
    Vibrato([Num; 4]),
    VibratoOff,
    Tremolo([Num; 4]),
    TremoloOff,
    PanWave([Num; 4]),
    PanWaveOff,
    ModulationOff,
    ModulationOn,
    RepeatStart,
    /// Back to the matching RepeatStart; 0 repeats for ever.
    RepeatEnd(Num),
    SetVar(u8, i32),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Song {
    pub channels: Vec<Vec<Cmd>>,
    /// `!`: < and > swapped in every channel.
    pub reversed: bool,
    /// The MML as given, for the debugger to show.
    pub source: Rc<str>,
}

impl Song {
    /// Channels with something in them.
    pub fn used(&self) -> impl Iterator<Item = (usize, &Vec<Cmd>)> {
        self.channels
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.is_empty())
    }
}

/// Where the MML could not be read, as a character index.
pub type Error = usize;

/// Takes out `/comments/`, and puts macros in where they are used.
fn expand(text: &[char]) -> Result<Vec<char>, Error> {
    let mut plain = Vec::with_capacity(text.len());
    let mut comment = false;
    for &c in text {
        if c == '/' {
            comment = !comment;
        } else if !comment {
            plain.push(c.to_ascii_uppercase());
        }
    }
    let mut macros: HashMap<String, Vec<char>> = HashMap::new();
    let mut out = Vec::with_capacity(plain.len());
    expand_into(&plain, &mut macros, &mut out, 0)?;
    Ok(out)
}

fn expand_into(
    text: &[char],
    macros: &mut HashMap<String, Vec<char>>,
    out: &mut Vec<char>,
    depth: usize,
) -> Result<(), Error> {
    let mut i = 0;
    while i < text.len() {
        if text[i] != '{' {
            out.push(text[i]);
            i += 1;
            continue;
        }
        let at = out.len();
        // A definition may use other macros: `{Y={X}F}`.
        let mut open = 0;
        let close = (i..text.len())
            .find(|&j| {
                match text[j] {
                    '{' => open += 1,
                    '}' => open -= 1,
                    _ => {}
                }
                open == 0
            })
            .ok_or(at)?;
        let inside = &text[i + 1..close];
        let (name, body) = match inside.iter().position(|&c| c == '=') {
            Some(eq) => (&inside[..eq], Some(&inside[eq + 1..])),
            None => (inside, None),
        };
        if name.is_empty() || !name.iter().all(|&c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(at);
        }
        let name: String = name.iter().collect();
        match body {
            Some(body) => {
                macros.insert(name, body.to_vec());
            }
            None => {
                let body = macros.get(&name).ok_or(at)?.clone();
                if depth == MACRO_DEPTH {
                    return Err(at);
                }
                expand_into(&body, macros, out, depth + 1)?;
            }
        }
        i = close + 1;
    }
    Ok(())
}

struct Parser {
    text: Vec<char>,
    at: usize,
    song: Song,
    channel: usize,
    /// A `:` has been seen; before it, K is the song's key.
    in_channel: bool,
}

/// Values a literal may take, checked as it is read.
type Range = std::ops::RangeInclusive<i32>;

impl Parser {
    fn peek(&self) -> Option<char> {
        self.text.get(self.at).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        self.skip_space();
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    fn starts(&self, word: &str) -> bool {
        word.chars()
            .enumerate()
            .all(|(i, c)| self.text.get(self.at + i) == Some(&c))
    }

    fn int(&mut self) -> Option<i32> {
        self.skip_space();
        let start = self.at;
        if self.peek() == Some('-') {
            self.at += 1;
        }
        let digits = self.at;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
        if self.at == digits {
            self.at = start;
            return None;
        }
        let s: String = self.text[start..self.at].iter().collect();
        s.parse().ok()
    }

    /// A number or a variable, if there is one.
    fn opt_num(&mut self, range: Range) -> Result<Option<Num>, Error> {
        self.skip_space();
        let start = self.at;
        if self.peek() == Some('$') {
            // `$n=` sets a variable rather than giving a value.
            let mut end = self.at + 1;
            while self.text.get(end).is_some_and(char::is_ascii_digit) {
                end += 1;
            }
            if self.text.get(end) == Some(&'=') {
                return Ok(None);
            }
            self.at += 1;
            return match self.int() {
                Some(n @ 0..=7) => Ok(Some(Num::Var(n as u8))),
                _ => Err(start),
            };
        }
        match self.int() {
            None => Ok(None),
            Some(n) if range.contains(&n) => Ok(Some(Num::Lit(n))),
            Some(_) => Err(start),
        }
    }

    fn num(&mut self, range: Range) -> Result<Num, Error> {
        let at = self.at;
        self.opt_num(range)?.ok_or(at)
    }

    fn four(&mut self) -> Result<[Num; 4], Error> {
        let mut v = [Num::Lit(0); 4];
        for (i, n) in v.iter_mut().enumerate() {
            if i > 0 && !self.eat(',') {
                return Err(self.at);
            }
            *n = self.num(0..=128)?;
        }
        Ok(v)
    }

    fn len(&mut self) -> Result<Len, Error> {
        let at = self.at;
        let div = self.opt_num(1..=WHOLE as i32)?;
        if let Some(Num::Lit(n)) = div
            && !WHOLE.is_multiple_of(n as u32)
        {
            return Err(at);
        }
        let mut dots = 0;
        while self.eat('.') {
            dots += 1;
        }
        Ok(Len { div, dots })
    }

    fn link(&mut self) -> Link {
        if self.eat('&') {
            Link::Tie
        } else if self.eat('_') {
            Link::Porta
        } else {
            Link::None
        }
    }

    fn accidentals(&mut self, mut semitone: i32) -> i32 {
        loop {
            match self.peek() {
                Some('+' | '#') => semitone += 1,
                Some('-') => semitone -= 1,
                _ => return semitone,
            }
            self.at += 1;
        }
    }

    fn push(&mut self, cmd: Cmd) {
        self.song.channels[self.channel].push(cmd);
    }

    /// The note before, if a tie follows it.
    fn tied_pitch(&self) -> Option<Pitch> {
        match self.song.channels[self.channel].last() {
            Some(Cmd::Note {
                pitch,
                link: Link::Tie,
                ..
            }) => Some(*pitch),
            _ => None,
        }
    }

    /// `&` or `_` written apart from the note before it.
    fn link_last(&mut self, link: Link, at: usize) -> Result<(), Error> {
        match self.song.channels[self.channel].last_mut() {
            Some(Cmd::Note { link: l, .. } | Cmd::Chord { link: l, .. }) => {
                *l = link;
                Ok(())
            }
            _ => Err(at),
        }
    }

    fn chord(&mut self) -> Result<Cmd, Error> {
        let mut parts = Vec::new();
        loop {
            self.skip_space();
            let at = self.at;
            let Some(c) = self.peek() else {
                return Err(at);
            };
            self.at += 1;
            match c {
                '|' => break,
                '<' => parts.push(ChordPart::Up),
                '>' => parts.push(ChordPart::Down),
                'O' => parts.push(ChordPart::Octave(self.num(-1..=9)?)),
                c => match note_name(c) {
                    Some(s) => {
                        let s = self.accidentals(s);
                        parts.push(ChordPart::Note(s));
                    }
                    None => return Err(at),
                },
            }
        }
        if !parts.iter().any(|p| matches!(p, ChordPart::Note(_))) {
            return Err(self.at);
        }
        let len = self.len()?;
        let link = self.link();
        Ok(Cmd::Chord { parts, len, link })
    }

    fn at_command(&mut self, at: usize) -> Result<Cmd, Error> {
        if self.peek().is_some_and(|c| c.is_ascii_digit() || c == '$') {
            return Ok(Cmd::Program(self.num(0..=432)?));
        }
        for (word, off) in [
            ("MPOF", Cmd::VibratoOff),
            ("MAOF", Cmd::TremoloOff),
            ("MLOF", Cmd::PanWaveOff),
            ("MOF", Cmd::ModulationOff),
            ("MON", Cmd::ModulationOn),
            ("ER", Cmd::EnvelopeOff),
        ] {
            if self.starts(word) {
                self.at += word.len();
                return Ok(off);
            }
        }
        let c = self.peek().ok_or(at)?;
        self.at += 1;
        Ok(match c {
            'V' => Cmd::Velocity(self.num(0..=127)?),
            'D' => Cmd::Detune(self.num(-128..=127)?),
            'E' => Cmd::Envelope(self.four()?),
            'M' => {
                let which = self.peek().ok_or(at)?;
                self.at += 1;
                let v = self.four()?;
                match which {
                    'P' => Cmd::Vibrato(v),
                    'A' => Cmd::Tremolo(v),
                    'L' => Cmd::PanWave(v),
                    _ => return Err(at),
                }
            }
            _ => return Err(at),
        })
    }

    fn command(&mut self) -> Result<(), Error> {
        self.skip_space();
        let at = self.at;
        let Some(c) = self.peek() else {
            return Ok(());
        };
        self.at += 1;
        let cmd = match c {
            ':' => {
                match self.int() {
                    Some(n @ 0..=15) => self.channel = n as usize,
                    _ => return Err(at),
                }
                self.in_channel = true;
                return Ok(());
            }
            'R' => Cmd::Note {
                pitch: Pitch::Rest,
                len: self.len()?,
                link: Link::None,
            },
            'N' => {
                let key = self.num(0..=127)?;
                // Without `,length`, the note takes L's length.
                let len = match self.eat(',') {
                    true => self.len()?,
                    false => Len { div: None, dots: 0 },
                };
                let link = self.link();
                Cmd::Note {
                    pitch: Pitch::Number(key),
                    len,
                    link,
                }
            }
            '|' => self.chord()?,
            'L' => Cmd::Length(self.len()?),
            'O' => Cmd::Octave(self.num(-1..=9)?),
            '<' => Cmd::Up,
            '>' => Cmd::Down,
            '!' => {
                self.song.reversed = true;
                return Ok(());
            }
            'T' => Cmd::Tempo(self.num(1..=1023)?),
            '@' => self.at_command(at)?,
            'V' => Cmd::Volume(self.num(0..=127)?),
            '(' => Cmd::Louder(self.opt_num(0..=127)?.unwrap_or(Num::Lit(1))),
            ')' => Cmd::Softer(self.opt_num(0..=127)?.unwrap_or(Num::Lit(1))),
            'P' => Cmd::Pan(self.num(0..=127)?),
            'Q' => Cmd::Gate(self.num(0..=32)?),
            'K' if self.in_channel => Cmd::Key(self.num(-115..=115)?),
            'K' => Cmd::SongKey(self.num(-115..=115)?),
            '[' => Cmd::RepeatStart,
            ']' => Cmd::RepeatEnd(self.opt_num(0..=255)?.unwrap_or(Num::Lit(0))),
            '$' => {
                let n = match self.int() {
                    Some(n @ 0..=7) => n as u8,
                    _ => return Err(at),
                };
                if !self.eat('=') {
                    return Err(self.at);
                }
                match self.int() {
                    Some(v @ -32768..=32767) => Cmd::SetVar(n, v),
                    _ => return Err(self.at),
                }
            }
            // Singing: the phoneme is not spoken, the note plays.
            // UNVERIFIED: what SmileBASIC plays for it without speech.
            '*' => {
                while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                    self.at += 1;
                }
                if !self.eat(',') {
                    return Err(self.at);
                }
                return Ok(());
            }
            '&' => return self.link_last(Link::Tie, at),
            // `C4&2`: a length after a tie goes on with the same note.
            c if c.is_ascii_digit() => {
                let Some(pitch) = self.tied_pitch() else {
                    return Err(at);
                };
                self.at = at;
                let len = self.len()?;
                let link = self.link();
                Cmd::Note { pitch, len, link }
            }
            '_' => return self.link_last(Link::Porta, at),
            c => match note_name(c) {
                Some(s) => {
                    let s = self.accidentals(s);
                    let len = self.len()?;
                    let link = self.link();
                    Cmd::Note {
                        pitch: Pitch::Name(s),
                        len,
                        link,
                    }
                }
                None => return Err(at),
            },
        };
        self.push(cmd);
        Ok(())
    }
}

fn note_name(c: char) -> Option<i32> {
    Some(match c {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    })
}

/// Reads MML, or gives where it could not be read.
pub fn parse(text: &str) -> Result<Song, Error> {
    let chars: Vec<char> = text.chars().collect();
    let mut p = Parser {
        text: expand(&chars)?,
        at: 0,
        song: Song {
            channels: vec![Vec::new(); CHANNELS],
            reversed: false,
            source: text.into(),
        },
        channel: 0,
        in_channel: false,
    };
    while p.at < p.text.len() {
        p.command()?;
        p.skip_space();
    }
    // Repeats must close within their channel.
    for channel in &p.song.channels {
        let mut depth = 0i32;
        for cmd in channel {
            match cmd {
                Cmd::RepeatStart => depth += 1,
                Cmd::RepeatEnd(_) => depth -= 1,
                _ => {}
            }
            if depth < 0 {
                return Err(p.text.len());
            }
        }
        if depth != 0 {
            return Err(p.text.len());
        }
    }
    Ok(p.song)
}

/// Ticks for a length, with `default` for L's; `None` if a variable gives
/// a length that is not one.
pub fn ticks(len: Len, default: u32, var: impl Fn(u8) -> i32) -> Option<u32> {
    let mut t = match len.div {
        None => default,
        Some(n) => {
            let div = match n {
                Num::Lit(n) => n,
                Num::Var(v) => var(v),
            };
            if div < 1 || div > WHOLE as i32 || !WHOLE.is_multiple_of(div as u32) {
                return None;
            }
            WHOLE / div as u32
        }
    };
    let mut add = t;
    for _ in 0..len.dots {
        add /= 2;
        t += add;
    }
    Some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(s: i32, div: Option<i32>) -> Cmd {
        Cmd::Note {
            pitch: Pitch::Name(s),
            len: Len {
                div: div.map(Num::Lit),
                dots: 0,
            },
            link: Link::None,
        }
    }

    #[test]
    fn notes_and_channels() {
        let song = parse("T120 C D+4 E-8 :1 o3 c").unwrap();
        assert_eq!(
            song.channels[0],
            [
                Cmd::Tempo(Num::Lit(120)),
                note(0, None),
                note(3, Some(4)),
                note(3, Some(8))
            ]
        );
        assert_eq!(song.channels[1], [Cmd::Octave(Num::Lit(3)), note(0, None)]);
    }

    #[test]
    fn links_chords_and_lengths() {
        let song = parse("C4.&C8 |CE<G|2 N60,4_D").unwrap();
        let c = &song.channels[0];
        assert!(matches!(
            c[0],
            Cmd::Note {
                len: Len { dots: 1, .. },
                link: Link::Tie,
                ..
            }
        ));
        assert!(matches!(&c[2], Cmd::Chord { parts, .. } if parts.len() == 4));
        assert!(matches!(
            c[3],
            Cmd::Note {
                pitch: Pitch::Number(Num::Lit(60)),
                link: Link::Porta,
                ..
            }
        ));
        assert_eq!(
            ticks(
                Len {
                    div: Some(Num::Lit(4)),
                    dots: 2
                },
                48,
                |_| 0
            ),
            Some(84)
        );
        assert_eq!(
            ticks(
                Len {
                    div: Some(Num::Var(0)),
                    dots: 0
                },
                48,
                |_| 5
            ),
            None
        );
    }

    #[test]
    fn macros_comments_and_variables() {
        let song = parse("{A=CD}/skip me/{A}$1=-3 N$1,4 @MP1,2,3,4 @MPOF @ER").unwrap();
        let c = &song.channels[0];
        assert_eq!(c[..2], [note(0, None), note(2, None)]);
        assert_eq!(c[2], Cmd::SetVar(1, -3));
        assert!(matches!(
            c[3],
            Cmd::Note {
                pitch: Pitch::Number(Num::Var(1)),
                ..
            }
        ));
        assert_eq!(c[5], Cmd::VibratoOff);
        assert_eq!(c[6], Cmd::EnvelopeOff);
    }

    #[test]
    fn song_key_comes_before_channels() {
        let song = parse("K2 :0 K-1").unwrap();
        assert_eq!(
            song.channels[0],
            [Cmd::SongKey(Num::Lit(2)), Cmd::Key(Num::Lit(-1))]
        );
    }

    #[test]
    fn errors_say_where() {
        assert_eq!(parse("CDX"), Err(2));
        assert_eq!(parse("C5"), Err(1));
        assert_eq!(parse("V128"), Err(1));
        assert_eq!(parse("[CD"), Err(3));
        assert_eq!(parse("{NOPE}"), Err(0));
        assert_eq!(parse(":16C"), Err(0));
        assert!(parse("[[O4CEG]3<C2.]").is_ok());
    }

    #[test]
    fn forms_bundled_and_shared_songs_use() {
        // A tie followed by a bare length goes on with the same note.
        let c = &parse("C4&2").unwrap().channels[0];
        assert!(matches!(
            c[1],
            Cmd::Note {
                pitch: Pitch::Name(0),
                ..
            }
        ));
        assert_eq!(parse("&2"), Err(0));
        let c = &parse("{X=CD}{Y_1={X}E}{Y_1}").unwrap().channels[0];
        assert_eq!(c.len(), 3);
        assert_eq!(
            parse("@MP19,19,19,0@MOF@MON N60").unwrap().channels[0].len(),
            4
        );
    }
}

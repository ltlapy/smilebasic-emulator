//! MML, the music language of BGMPLAY, BGMSET and BGMSETD, parsed into a
//! list of commands for each channel. Octaves, lengths and variables are
//! left to the player, since repeats and BGMVAR change them as it plays.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub const CHANNELS: usize = 16;
/// Ticks in a whole note: every length divides it.
pub const WHOLE: u32 = 192;
/// How deep macros may use other macros.
const MACRO_DEPTH: usize = 16;
/// Characters macros may copy for one song, counting a macro's whole body
/// each time it is defined or used. Macros using macros multiply, so a few
/// hundred bytes could otherwise ask for gigabytes. MML with no macros
/// costs nothing against this.
// Ours, far above what any song seen needs (see docs/differences.md).
const MACRO_CHARS: usize = 1 << 22;

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
    /// @MPON: vibrato on again with the last @MP settings.
    VibratoOn,
    Tremolo([Num; 4]),
    TremoloOff,
    TremoloOn,
    PanWave([Num; 4]),
    PanWaveOff,
    PanWaveOn,
    ModulationOff,
    ModulationOn,
    RepeatStart,
    /// Back to the matching RepeatStart; 0 repeats for ever.
    RepeatEnd(Num),
    SetVar(u8, i32),
    /// `*WA,`: the next note sings these phoneme letters.
    Sing(Rc<str>),
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

/// Where the MML could not be read, as a character index in the text as
/// given, comments and macro definitions counted.
pub type Error = usize;

/// Why macros could not be put in, with where.
enum Stop {
    Bad(Error),
    OverLimit(Error),
}

/// MML with comments taken out and macros put in, and where each of its
/// characters stands in the text as given. A macro's characters all stand
/// just past where it was used: `{X=@MP128,1,1,1}C{X}` fails at 20.
// UNVERIFIED: a macro used inside another stands past the outer use.
struct Plain {
    text: Vec<char>,
    /// Indexes into the text as given, or, marked [`USED`], the index just
    /// past a macro's use.
    at: Vec<u32>,
}

/// Marks a place in [`Plain::at`] as just past a macro's use: an error
/// there stands at that index itself, not one after it.
const USED: u32 = 1 << 31;

/// Takes out `/comments/`, and puts macros in where they are used. With
/// `seen`, a macro goes in only where it is first used, which is enough to
/// read every body and cannot multiply.
fn expand(text: &[char], mut seen: Option<HashSet<String>>) -> Result<Plain, Stop> {
    let mut plain = Vec::with_capacity(text.len());
    let mut comment = false;
    for (i, &c) in text.iter().enumerate() {
        if c == '/' {
            comment = !comment;
        } else if !comment {
            plain.push((c.to_ascii_uppercase(), i as u32));
        }
    }
    let mut macros: HashMap<String, Vec<(char, u32)>> = HashMap::new();
    let mut out = Plain {
        text: Vec::with_capacity(plain.len()),
        at: Vec::with_capacity(plain.len()),
    };
    let mut left = if seen.is_some() {
        usize::MAX
    } else {
        MACRO_CHARS
    };
    expand_into(&plain, &mut macros, &mut out, 0, None, &mut left, &mut seen)?;
    Ok(out)
}

/// Puts `text` in `out`. `used` is where the outermost macro being put in
/// was used: [`USED`] and the index just past its `}`.
#[allow(clippy::too_many_arguments)]
fn expand_into(
    text: &[(char, u32)],
    macros: &mut HashMap<String, Vec<(char, u32)>>,
    out: &mut Plain,
    depth: usize,
    used: Option<u32>,
    left: &mut usize,
    seen: &mut Option<HashSet<String>>,
) -> Result<(), Stop> {
    let mut i = 0;
    while i < text.len() {
        let (c, source) = text[i];
        if c != '{' {
            out.text.push(c);
            out.at.push(used.unwrap_or(source));
            i += 1;
            continue;
        }
        let at = used.map_or(source, |u| u & !USED) as usize;
        // A definition may use other macros: `{Y={X}F}`.
        let mut open = 0;
        let close = (i..text.len())
            .find(|&j| {
                match text[j].0 {
                    '{' => open += 1,
                    '}' => open -= 1,
                    _ => {}
                }
                open == 0
            })
            .ok_or(Stop::Bad(at))?;
        let inside = &text[i + 1..close];
        let (name, body) = match inside.iter().position(|&(c, _)| c == '=') {
            Some(eq) => (&inside[..eq], Some(&inside[eq + 1..])),
            None => (inside, None),
        };
        let is_name = |&(c, _): &(char, u32)| c.is_ascii_alphanumeric() || c == '_';
        if name.is_empty() || !name.iter().all(is_name) {
            return Err(Stop::Bad(at));
        }
        let name: String = name.iter().map(|&(c, _)| c).collect();
        match body {
            Some(body) => {
                // Counted too: a macro that only defines others puts out
                // nothing, however many times it runs.
                *left = (left.checked_sub(body.len() + 1)).ok_or(Stop::OverLimit(at))?;
                macros.insert(name, body.to_vec());
            }
            None => {
                let body = macros.get(&name).ok_or(Stop::Bad(at))?;
                if depth == MACRO_DEPTH {
                    return Err(Stop::Bad(at));
                }
                // The 1 is for empty macros, which cost time all the same.
                *left = (left.checked_sub(body.len() + 1)).ok_or(Stop::OverLimit(at))?;
                let body = body.clone();
                let used = used.or(Some(USED | (text[close].1 + 1)));
                if seen.as_mut().is_none_or(|seen| seen.insert(name)) {
                    expand_into(&body, macros, out, depth + 1, used, left, seen)?;
                }
            }
        }
        i = close + 1;
    }
    Ok(())
}

struct Parser {
    text: Vec<char>,
    /// [`Plain::at`]: where each character of `text` was written.
    source: Vec<u32>,
    at: usize,
    song: Song,
    channel: usize,
    /// A `:` has been seen; before it, K is the song's key.
    in_channel: bool,
    /// Each channel's octave as the text sets it, for `<` and `>` past -1
    /// to 9.
    octaves: [i32; CHANNELS],
    /// Each channel's `L` in ticks as the text sets it, for the dots after
    /// a note without a length; `None` after `L` with a variable.
    lengths: [Option<u32>; CHANNELS],
    /// Each channel's `[` not yet closed.
    repeats: [u32; CHANNELS],
    /// A bit for each channel used so far; channel 0 from the start.
    chosen: u16,
}

/// Values a literal may take, checked as it is read.
type Range = std::ops::RangeInclusive<i32>;

impl Parser {
    /// An error at the character at `p`, where it was written.
    fn start(&self, p: usize) -> Error {
        match self.source.get(p) {
            Some(&i) => (i & !USED) as usize,
            None => self.end_of(p),
        }
    }

    /// An error just after what comes before `p`, where that was written:
    /// SmileBASIC 4 gives the end of what it stopped on.
    fn end_of(&self, p: usize) -> Error {
        match p.checked_sub(1).and_then(|i| self.source.get(i)) {
            Some(&i) if i & USED != 0 => (i & !USED) as usize,
            Some(&i) => i as usize + 1,
            None => self.source.first().map_or(0, |&i| (i & !USED) as usize),
        }
    }

    /// An error just after what has been read.
    fn end(&self) -> Error {
        self.end_of(self.at)
    }

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
                _ => Err(self.start(start)),
            };
        }
        match self.int() {
            None => Ok(None),
            Some(n) if range.contains(&n) => Ok(Some(Num::Lit(n))),
            Some(_) => Err(self.start(start)),
        }
    }

    fn num(&mut self, range: Range) -> Result<Num, Error> {
        let at = self.at;
        self.opt_num(range)?.ok_or_else(|| self.start(at))
    }

    /// `O`'s octave, which `<` and `>` count from. It takes a number only,
    /// so a variable is refused just after the `O`, before any space.
    fn set_octave(&mut self) -> Result<Num, Error> {
        let after_o = self.at;
        self.skip_space();
        if self.peek() == Some('$') {
            return Err(self.end_of(after_o));
        }
        let n = self.num(-1..=9)?;
        if let Num::Lit(n) = n {
            self.octaves[self.channel] = n;
        }
        Ok(n)
    }

    /// `<` (up) or `>` (down), which fail past the octaves `O` takes,
    /// where the text stands after them. `>` from octave 0 reaches -1
    /// (`O0>C` passes) as `O-1` does, and a second `>` fails.
    fn shift_octave(&mut self, up: bool) -> Result<(), Error> {
        let by = if up != self.song.reversed { 1 } else { -1 };
        let octave = &mut self.octaves[self.channel];
        *octave += by;
        if (-1..=9).contains(octave) {
            Ok(())
        } else {
            Err(self.end())
        }
    }

    /// The four values of `@E`, `@MP`, `@MA` and `@ML`, 0 to 127, each
    /// refused where the text stands after it. A negative one too: `-1` is
    /// read as one number, and refused after its 1.
    fn four(&mut self) -> Result<[Num; 4], Error> {
        let mut v = [Num::Lit(0); 4];
        for (i, n) in v.iter_mut().enumerate() {
            if i > 0 && !self.eat(',') {
                return Err(self.end());
            }
            *n = self.num(i32::MIN..=i32::MAX)?;
            if matches!(*n, Num::Lit(n) if !(0..=127).contains(&n)) {
                return Err(self.end());
            }
        }
        Ok(v)
    }

    /// A length and its dots. Each dot adds half of what the one before
    /// added, so it is refused, after the last dot, on an odd number of
    /// ticks: `C4...` passes and `C192.` does not.
    fn len(&mut self) -> Result<Len, Error> {
        let at = self.at;
        let div = self.opt_num(1..=WHOLE as i32)?;
        let mut ticks = match div {
            Some(Num::Lit(n)) if !WHOLE.is_multiple_of(n as u32) => return Err(self.start(at)),
            Some(Num::Lit(n)) => Some(WHOLE / n as u32),
            Some(Num::Var(_)) => None,
            None => self.lengths[self.channel],
        };
        let mut dots = 0;
        let mut odd = false;
        while self.eat('.') {
            dots += 1;
            if let Some(t) = ticks {
                odd |= t % 2 == 1;
                ticks = Some(t / 2);
            }
        }
        if odd {
            return Err(self.end());
        }
        Ok(Len { div, dots })
    }

    /// What `len` gives in ticks, if the text says without variables.
    fn len_ticks(&self, len: Len) -> Option<u32> {
        match len.div {
            Some(Num::Var(_)) => None,
            Some(Num::Lit(_)) => ticks(len, 0, |_| 0),
            None => ticks(len, self.lengths[self.channel]?, |_| 0),
        }
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

    /// The note or chord before, if a tie follows it.
    fn tied(&self) -> Option<Cmd> {
        match self.song.channels[self.channel].last()? {
            cmd @ (Cmd::Note {
                link: Link::Tie, ..
            }
            | Cmd::Chord {
                link: Link::Tie, ..
            }) => Some(cmd.clone()),
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
            _ => Err(self.start(at)),
        }
    }

    /// `|...|`, two to six notes. A chord that cannot be read fails just
    /// after its first `|`, before any octave in it is checked: one note is
    /// no chord, so `O8|<C|<C` gives 3. An octave set inside a chord lasts
    /// to its end only: the note after `|A<CE|` is in the octave before the
    /// chord. Something else between the bars fails at the end of what
    /// comes before it: `|CR|` gives 2.
    // UNVERIFIED: that more than six notes, an octave change after the
    // last note, or no closing bar fail just after the first bar too, and
    // that anything other than R fails where R does.
    fn chord(&mut self) -> Result<Cmd, Error> {
        const NOTES: std::ops::RangeInclusive<usize> = 2..=6;
        let octave = self.octaves[self.channel];
        let bar = self.end();
        let mut parts = Vec::new();
        let mut notes = 0;
        // Where a `<` or `>` first left the octaves, if one did.
        let mut past = None;
        // The end of the last part read.
        let mut last = bar;
        loop {
            if self.at > 0 && self.text[self.at - 1] != '|' {
                last = self.end();
            }
            self.skip_space();
            let Some(c) = self.peek() else {
                return Err(bar);
            };
            self.at += 1;
            match c {
                '|' => break,
                '<' | '>' => {
                    if let Err(e) = self.shift_octave(c == '<') {
                        past.get_or_insert(e);
                    }
                    parts.push(match c {
                        '<' => ChordPart::Up,
                        _ => ChordPart::Down,
                    });
                }
                'O' => parts.push(ChordPart::Octave(self.set_octave()?)),
                c => match note_name(c) {
                    Some(s) => {
                        let s = self.accidentals(s);
                        parts.push(ChordPart::Note(s));
                        notes += 1;
                    }
                    None => return Err(last),
                },
            }
        }
        if !NOTES.contains(&notes) || !matches!(parts.last(), Some(ChordPart::Note(_))) {
            return Err(bar);
        }
        if let Some(e) = past {
            return Err(e);
        }
        self.octaves[self.channel] = octave;
        let len = self.len()?;
        let link = self.link();
        Ok(Cmd::Chord { parts, len, link })
    }

    /// An `@` command. Its word runs on through `@` and letters, and stops
    /// once it is three to five characters long and only one word of
    /// [`AT_WORDS`] starts with it, or at six: `@MPONC` is `@MPON` and C,
    /// and `@MPOXC` is all one word. A word that is not a command fails
    /// at its end: `@MPOXC` gives 6, `@MPO C` 4.
    fn at_command(&mut self, at: usize) -> Result<Cmd, Error> {
        if self.peek().is_some_and(|c| c.is_ascii_digit() || c == '$') {
            return Ok(Cmd::Program(self.num(0..=432)?));
        }
        let mut end = self.at;
        while self
            .text
            .get(end)
            .is_some_and(|&c| c == '@' || c.is_ascii_uppercase())
        {
            let n = end - at;
            if n > 5 || (n >= 3 && unique_at_word(&self.text[at..end])) {
                break;
            }
            end += 1;
        }
        let word: String = self.text[at + 1..end].iter().collect();
        self.at = end;
        Ok(match word.as_str() {
            "V" => Cmd::Velocity(self.num(0..=127)?),
            "D" => Cmd::Detune(self.num(-128..=127)?),
            "E" => Cmd::Envelope(self.four()?),
            "ER" => Cmd::EnvelopeOff,
            "MP" => Cmd::Vibrato(self.four()?),
            "MA" => Cmd::Tremolo(self.four()?),
            "ML" => Cmd::PanWave(self.four()?),
            "MPOF" => Cmd::VibratoOff,
            "MAOF" => Cmd::TremoloOff,
            "MLOF" => Cmd::PanWaveOff,
            "MPON" => Cmd::VibratoOn,
            "MAON" => Cmd::TremoloOn,
            "MLON" => Cmd::PanWaveOn,
            "MOF" => Cmd::ModulationOff,
            "MON" => Cmd::ModulationOn,
            _ => return Err(self.end()),
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
            // A channel's MML stands in one piece: going back to a channel
            // used earlier fails at the channel's number, as `:0C:1D:0E`
            // does at 7 and `:1C:2D:1E` at 7. Channel 0 counts as used from
            // the start, so `:1C:0D` fails at 4 and `C:1D:0E` at 5.
            // `:0C:0D` goes on in channel 0, and `:2C:1D` passes. Leaving a
            // channel with a `[` still open fails there too: `[:1C]2` at 2,
            // `[C:1D]` at 3. A number past 15 fails at its start: `:16C`
            // at 1. A `:` naming the channel it is in is no switch, so it
            // passes inside an open `[` too (`[:0C]`).
            ':' => {
                self.skip_space();
                let number = self.start(self.at);
                match self.int() {
                    Some(n @ 0..=15) => {
                        let n = n as usize;
                        if n != self.channel
                            && (self.chosen & 1 << n != 0 || self.repeats[self.channel] > 0)
                        {
                            return Err(number);
                        }
                        self.chosen |= 1 << n;
                        self.channel = n;
                    }
                    Some(_) => return Err(number),
                    None => return Err(self.start(at)),
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
            'L' => {
                let len = self.len()?;
                self.lengths[self.channel] = self.len_ticks(len);
                Cmd::Length(len)
            }
            'O' => Cmd::Octave(self.set_octave()?),
            '<' => {
                self.shift_octave(true)?;
                Cmd::Up
            }
            '>' => {
                self.shift_octave(false)?;
                Cmd::Down
            }
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
            '[' => {
                self.repeats[self.channel] += 1;
                Cmd::RepeatStart
            }
            // A `]` with no `[` fails just after it, before its count:
            // `C]0` gives 2.
            ']' => {
                let open = &mut self.repeats[self.channel];
                if *open == 0 {
                    return Err(self.end());
                }
                *open -= 1;
                Cmd::RepeatEnd(self.opt_num(0..=255)?.unwrap_or(Num::Lit(0)))
            }
            '$' => {
                let n = match self.int() {
                    Some(n @ 0..=7) => n as u8,
                    _ => return Err(self.start(at)),
                };
                if !self.eat('=') {
                    return Err(self.end());
                }
                match self.int() {
                    Some(v @ -32768..=32767) => Cmd::SetVar(n, v),
                    _ => return Err(self.end()),
                }
            }
            // Singing: the next note sings the phonemes. It takes three
            // letters at most, which are not checked (`*XA,C` passes), and
            // then a comma: `*KONNICHIWA,C1` fails after `KON`.
            '*' => {
                let start = self.at;
                while self.at - start < 3 && self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                    self.at += 1;
                }
                let letters: String = self.text[start..self.at].iter().collect();
                let after = self.end();
                if !self.eat(',') {
                    return Err(after);
                }
                if letters.is_empty() {
                    return Ok(());
                }
                Cmd::Sing(letters.into())
            }
            '&' => return self.link_last(Link::Tie, at),
            // `C4&2`: a length after a tie goes on with the same note, and
            // `|CEG|4&2` with the same chord.
            // UNVERIFIED: the bare length holds the whole chord.
            c if c.is_ascii_digit() => {
                let Some(mut cmd) = self.tied() else {
                    return Err(self.start(at));
                };
                self.at = at;
                if let Cmd::Note { len, link, .. } | Cmd::Chord { len, link, .. } = &mut cmd {
                    *len = self.len()?;
                    *link = self.link();
                }
                cmd
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
                None => return Err(self.start(at)),
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

/// The `@` words that end an `@` command early, in the order SmileBASIC 4
/// lists them.
const AT_WORDS: [&str; 12] = [
    "@MPOF", "@MAOF", "@MLOF", "@MPON", "@MAON", "@MLON", "@MOF", "@MON", "@MP", "@MA", "@ML",
    "@ER",
];

/// Whether exactly one of [`AT_WORDS`] starts with `head`.
fn unique_at_word(head: &[char]) -> bool {
    let starts = |w: &&&str| w.len() >= head.len() && w.chars().zip(head).all(|(a, &b)| a == b);
    AT_WORDS.iter().filter(starts).count() == 1
}

/// Reads MML, or gives where it could not be read.
pub fn parse(text: &str) -> Result<Song, Error> {
    let chars: Vec<char> = text.chars().collect();
    match expand(&chars, None) {
        Ok(plain) => parse_plain(plain, text),
        Err(Stop::Bad(at) | Stop::OverLimit(at)) => Err(at),
    }
}

/// CHKMML: whether the MML can be read. Macros beyond what BGMPLAY takes
/// are read one body each rather than refused, so where it gives for an
/// error in such MML counts from that shorter text.
pub fn check(text: &str) -> Result<(), Error> {
    let chars: Vec<char> = text.chars().collect();
    let plain = match expand(&chars, None) {
        Err(Stop::OverLimit(_)) => expand(&chars, Some(HashSet::new())),
        expanded => expanded,
    };
    match plain {
        Ok(plain) => parse_plain(plain, text).map(drop),
        Err(Stop::Bad(at) | Stop::OverLimit(at)) => Err(at),
    }
}

fn parse_plain(plain: Plain, text: &str) -> Result<Song, Error> {
    let mut p = Parser {
        text: plain.text,
        source: plain.at,
        at: 0,
        song: Song {
            channels: vec![Vec::new(); CHANNELS],
            reversed: false,
            source: text.into(),
        },
        channel: 0,
        in_channel: false,
        octaves: [4; CHANNELS],
        lengths: [Some(WHOLE / 4); CHANNELS],
        repeats: [0; CHANNELS],
        chosen: 1,
    };
    while p.at < p.text.len() {
        p.command()?;
        p.skip_space();
    }
    // Repeats must close within their channel.
    if p.repeats.iter().any(|&open| open > 0) {
        return Err(p.end_of(p.text.len()));
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
        // A bare length after a tied chord goes on with the chord.
        let song = parse("|CEG|4&2&8.C").unwrap();
        let chord = |div, dots, link| Cmd::Chord {
            parts: vec![ChordPart::Note(0), ChordPart::Note(4), ChordPart::Note(7)],
            len: Len {
                div: Some(Num::Lit(div)),
                dots,
            },
            link,
        };
        assert_eq!(
            song.channels[0][..3],
            [
                chord(4, 0, Link::Tie),
                chord(2, 0, Link::Tie),
                chord(8, 1, Link::None)
            ]
        );
        assert_eq!(parse("|CEG|4 2").err(), Some(7));
        // Two to six notes; what a chord cannot hold fails after its first
        // bar, and something else between the bars after the part before
        // it.
        for (mml, at) in [
            ("|CE|", None),
            ("|C+E-G#|", None),
            ("|O3CO5E|", None),
            ("|CDEFGA|", None),
            ("|CDEFGAB|", Some(1)),
            ("C|E|G", Some(2)),
            ("||", Some(1)),
            ("|CE<|", Some(1)),
            ("|CR|", Some(2)),
            ("|C R|", Some(2)),
            ("|R|", Some(1)),
            ("|C+R|", Some(3)),
            ("|C4E|", Some(2)),
            ("|CE", Some(1)),
            ("O9|<CE", Some(3)),
        ] {
            assert_eq!(parse(mml).err(), at, "{mml}");
        }
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
    fn lfo_on_words() {
        let song = parse("@MP64,1,16,18@MPON{A=C}@MAONC@mlon@MPOFF").unwrap();
        let c = &song.channels[0];
        assert_eq!(c[1], Cmd::VibratoOn);
        assert_eq!(c[2], Cmd::TremoloOn);
        assert_eq!(c[4], Cmd::PanWaveOn);
        assert_eq!(c[5], Cmd::VibratoOff);
        // @MPOFF is @MPOF and the note F.
        assert!(matches!(c[6], Cmd::Note { .. }));
        // Other letters after @MPO, @MAO, @MLO or @MO are not words, and
        // fail at the end of the word.
        for (bad, at) in [
            ("@MPOXC", 6),
            ("@MPO C", 4),
            ("@MOX", 4),
            ("@MAOC", 5),
            ("@MLOC", 5),
            ("@MLOX", 5),
            ("@MO", 3),
            ("@MPOXYZW", 6),
            ("@X", 2),
        ] {
            assert_eq!(check(bad), Err(at), "{bad}");
        }
        assert!(parse("@MOFC@MONF@ERC@V100@D-5@E1,2,3,4").is_ok());
    }

    /// `first`, then macros each using the last ten times.
    fn macro_bomb(first: &str, levels: usize) -> String {
        let mut s = format!("{{M0={first}}}");
        for i in 1..=levels {
            let last = format!("{{M{}}}", i - 1);
            s += &format!("{{M{i}={}}}", last.repeat(10));
        }
        s + &format!("{{M{levels}}}")
    }

    #[test]
    fn macros_that_multiply_are_an_error() {
        // 10^10 notes from a few hundred bytes.
        let bomb = macro_bomb("CCCCCCCCCC", 9);
        assert!(bomb.len() < 500);
        assert!(parse(&bomb).is_err());
        // Macros that put out nothing would take as long, and so would
        // macros that only define others.
        assert!(parse(&macro_bomb("", 15)).is_err());
        assert!(parse(&macro_bomb("{X=C}", 15)).is_err());
        // Short of the limit they still work: 10^5 notes.
        let song = parse(&macro_bomb("CCCCCCCCCC", 4)).unwrap();
        assert_eq!(song.channels[0].len(), 100_000);
    }

    #[test]
    fn checking_reads_macros_beyond_the_limit_once_each() {
        for levels in [6, 7, 9, 15] {
            let bomb = macro_bomb("CCCCCCCCCC", levels);
            assert_eq!(check(&bomb), Ok(()), "{levels}");
        }
        // Each body is still read.
        assert!(check(&macro_bomb("Q", 15)).is_err());
        assert!(check(&(macro_bomb("CCCCCCCCCC", 9) + "Q")).is_err());
        // Going on being refused: too deep, and not defined.
        assert!(check(&macro_bomb("CCCCCCCCCC", 17)).is_err());
        assert!(check("{X}").is_err());
    }

    /// Where CHKMML stops on octaves and four-value commands out of range
    /// (TEST67, TEST69).
    #[test]
    fn octaves_and_envelopes_stop_where_sb4_does() {
        for (mml, at) in [
            ("@E127,127,127,127C", None),
            ("@E128,0,0,0C", Some(5)),
            ("@E0,0,0,128C", Some(11)),
            ("O9C<C", Some(4)),
            ("O9C<<<C", Some(4)),
            ("O-1C>C", Some(5)),
            ("O9<<<<<<<<<<C", Some(3)),
            ("O10C", Some(1)),
            ("O9C>C<C", None),
            ("O-1C<C", None),
            // Each channel keeps its own octave.
            (":0O9C:1<C", None),
            ("O9|C<E|", Some(5)),
            // TEST69.
            ("O8[<C]3", None),
            ("O9!>C", Some(4)),
            ("O9|<CE|", Some(4)),
            // One note is no chord: it fails after the first bar, before
            // its octave is checked.
            ("O8|<C|<C", Some(3)),
            ("O9|<C|", Some(3)),
            // A chord's octave ends with it.
            ("O8|<CE|<C", None),
            ("O8|<CE|<C<C", Some(10)),
            ("O1|>>CE|>C", None),
            ("$0=9O$0<C", Some(5)),
            ("$0=1O$0>>C", Some(5)),
            ("@MP127,1,1,1C", None),
            ("@MP128,1,1,1C", Some(6)),
            ("@MP1,128,1,1C", Some(8)),
            ("@MP1,1,128,1C", Some(10)),
            ("@MP1,1,1,128C", Some(12)),
            ("@MA128,1,1,1C", Some(6)),
            ("@ML128,1,1,1C", Some(6)),
            ("@MA1,1,1,128C", Some(12)),
            ("@ML1,1,1,128C", Some(12)),
        ] {
            assert_eq!(check(mml).err(), at, "{mml}");
        }
    }

    /// Negative four-value numbers, `O` before a space, octaves down to -1,
    /// lower case and dots.
    #[test]
    fn numbers_octaves_and_dots_stop_at_the_end_of_what_failed() {
        for (mml, at) in [
            ("@MP129,1,1,1C", Some(6)),
            ("@MP200,1,1,1C", Some(6)),
            ("@MP-1,1,1,1C", Some(5)),
            ("@E0,-1,0,0C", Some(6)),
            ("$0=9O $0<C", Some(5)),
            ("$0=9O  $0<C", Some(5)),
            ("O0>C", None),
            ("O-1C", None),
            ("O0>>C", Some(4)),
            ("o4c", None),
            ("C4..", None),
            ("C4...D", None),
            ("C1......", None),
            ("C1.......", Some(9)),
            ("C96.", None),
            ("C96..", Some(5)),
            ("C192.", Some(5)),
            ("N60,192.", Some(8)),
            ("|CE|192.", Some(8)),
            // Without a length the dots count from L's.
            ("L192C.", Some(6)),
            ("L192:1C.", None),
            ("L96.C.", Some(6)),
            ("$0=4L$0C.", None),
        ] {
            assert_eq!(check(mml).err(), at, "{mml}");
        }
    }

    /// Positions count comments, and an error inside a macro stands just
    /// past where the macro was used.
    #[test]
    fn errors_stand_in_the_text_as_given() {
        assert_eq!(check("{X=5}O{X}C"), Ok(()));
        assert_eq!(check("/skip/C5"), Err(7));
        assert_eq!(check("/A/@MP128,1,1,1C"), Err(9));
        assert_eq!(check("{X=@MP128,1,1,1}C{X}"), Err(20));
        assert_eq!(check("{X=C}/A/@MP128,1,1,1C"), Err(14));
        assert_eq!(check("@MP129/,/,1,1,1C"), Err(6));
        assert_eq!(check("{X=C5}CDE{X}"), Err(12));
        assert_eq!(check("{X=C5}CDE{X}D"), Err(12));
        assert_eq!(check("{X=@MP129,1,1,1}C{X}"), Err(20));
        // A macro used in another stands past the outer use.
        assert_eq!(check("{X=C}{Y=D{X}Q}{Y}"), Err(17));
        assert_eq!(check("{X=C5}{Y=D{X}}{Y}E"), Err(17));
        assert_eq!(check("{X=C}{X}Z"), Err(8));
        assert_eq!(check("CD{NOPE}"), Err(2));
        assert_eq!(check("[CD/x/"), Err(3));
    }

    #[test]
    fn long_mml_without_macros_has_no_limit() {
        let song = parse(&"C".repeat(MACRO_CHARS + 1)).unwrap();
        assert_eq!(song.channels[0].len(), MACRO_CHARS + 1);
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
        assert_eq!(parse(":16C"), Err(1));
        assert!(parse("[[O4CEG]3<C2.]").is_ok());
        // A `]` with no `[` fails just after it.
        assert_eq!(check("C]0"), Err(2));
        assert_eq!(check("C]-"), Err(2));
        assert_eq!(check("C]0D5"), Err(2));
        assert_eq!(check("[C]0"), Ok(()));
        assert_eq!(check(":1[C:1D"), Err(7));
    }

    #[test]
    fn channels_stand_in_one_piece() {
        assert_eq!(check(":0C:1D:0E"), Err(7));
        assert_eq!(check(":0@218T120O4L1@MP64,1,16,0:1R1:0@MPONC"), Err(31));
        assert_eq!(check(":1C:2D:1E"), Err(7));
        assert_eq!(check(":0C:0D:1E"), Ok(()));
        // Channel 0 is used from the start.
        assert_eq!(check("C:1D:0E"), Err(5));
        assert_eq!(check(":1C:0D"), Err(4));
        assert_eq!(check(":0C:1D:0E"), Err(7));
        for passes in [":2C:1D", ":1C:1D", ":0C:0D", "C:0D", ":1[C]2:2D"] {
            assert_eq!(check(passes), Ok(()), "{passes}");
        }
        // Leaving a channel with an open `[`.
        assert_eq!(check("[:1C]2"), Err(2));
        assert_eq!(check("[C:1D]2"), Err(3));
        assert_eq!(check(":0[C:1D]2"), Err(5));
        assert_eq!(check("[C:1D]"), Err(3));
    }

    #[test]
    fn sung_notes_keep_their_phonemes() {
        let c = &parse("*wa,O3C1*K,C16").unwrap().channels[0];
        assert_eq!(c[0], Cmd::Sing("WA".into()));
        assert_eq!(c[3], Cmd::Sing("K".into()));
        assert_eq!(c.len(), 5);
        assert!(parse("*WA C4").is_err());
    }

    /// A sung note takes three letters at most, any letters, and then a
    /// comma.
    #[test]
    fn sung_notes_take_three_letters() {
        assert_eq!(parse("T60@0O4*KONNICHIWA,C1").err(), Some(11));
        assert_eq!(parse("T60@0O4*KOQNICHIWA,C1").err(), Some(11));
        assert!(parse("T60@0O4*KO,C*Q,C*NI,C*CHI,C*WA,C1").is_ok());
        for ok in [
            "*RYA,C", "*CA,C", "*CHA,C", "*XA,C", "*KXA,C", "*CLA,C", "*L,C", "*KK,C", "*VA,C",
            "*,C",
        ] {
            assert!(parse(ok).is_ok(), "{ok}");
        }
        for (bad, at) in [("*SHYA,C", 4), ("*AIUEO,C", 4), ("*WA C4", 3)] {
            assert_eq!(parse(bad).err(), Some(at), "{bad}");
        }
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

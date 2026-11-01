//! TALK: Japanese speech from kana or SmileBASIC's phoneme letters, on a
//! Klatt formant synthesizer (klattsch-core). Kana become phoneme letters,
//! the letters become morae, and each mora a few targets for the
//! synthesizer to glide between. MML's sung notes (`*WA,C4`) use the
//! same phonemes on a [`Singer`] of each channel's own.
//!
//! None of this is SmileBASIC 4's own voice; it only says the same words.

use std::collections::HashMap;
use std::sync::OnceLock;

use klattsch_core::{
    BankRegistry, FormantSynth, ParamUpdate, Params, PhonemeParams, PhonemeTable, Schedule,
    ScheduleEvent,
};

use super::RATE;

/// The bank with Japanese vowels (Mokhtari & Tanaka 2000) over Klatt's
/// English consonants.
const BANK: &str = "ja-mokhtari-2000";
const PHONEMES: [&str; 26] = [
    "A", "I", "U", "E", "O", "K", "G", "S", "SH", "Z", "ZH", "JH", "T", "CH", "D", "N", "NG", "HH",
    "F", "B", "P", "M", "Y", "DX", "W", "_",
];

/// Milliseconds at the standard speed, `<S128>`.
const MORA: f64 = 130.0;
const CLOSURE: f64 = 45.0;
const BURST: f64 = 15.0;
const FRICATIVE: f64 = 70.0;
/// ち, じ and つ: a shorter closure and burst than a stop's, then the
/// fricative they are released into.
const AFFRICATE_CLOSURE: f64 = 30.0;
const AFFRICATE_BURST: f64 = 8.0;
const AFFRICATE_FRICATION: f64 = 45.0;
const SONORANT: f64 = 45.0;
const GLIDE: f64 = 40.0;
/// 、 and 。, as long as the help's example says: it writes
/// "こんにちわ。わたしは、" as "koqnichiwa######watashiwa##".
const SHORT_PAUSE: f64 = 200.0;
const LONG_PAUSE: f64 = 600.0;
/// `#`: 0.1 seconds without sound.
const BREAK: f64 = 100.0;
/// Glides between targets.
const TRANSITION: f64 = 25.0;
const QUICK: f64 = 8.0;

/// The voice's pitch at `<P128>`.
// UNVERIFIED: SmileBASIC's voice is higher or lower than this.
const BASE_PITCH: f32 = 150.0;
/// The synthesizer's gain at `<V128>`.
const BASE_GAIN: f32 = 3.0;
/// Pitch at a phrase's start and end, as a part of the base: speech falls
/// as it goes.
const PITCH_START: f32 = 1.06;
const PITCH_END: f32 = 0.88;
/// Where a falling syllable's pitch ends, as a part of where it starts.
// UNVERIFIED: how far は falls before 、.
const FALL: f32 = 0.8;

/// One beat of speech.
#[derive(Clone, Debug, PartialEq)]
enum Mora {
    /// A consonant (if any) with a `y` glide (if any) and a vowel.
    Syllable {
        consonant: Option<&'static str>,
        glide: bool,
        vowel: char,
    },
    /// ん.
    Nasal,
    /// っ: a held silence before the next consonant.
    Hold,
    Pause(f64),
    /// The syllable before falls in pitch, as は before 、 does.
    Fall,
    /// `<S…>`, `<P…>` and `<V…>`.
    Control(char, i32),
}

/// Kana as SmileBASIC's phoneme letters: `q` for ん, `cl` for っ.
fn kana_letters(text: &str) -> String {
    const TABLE: &[(char, &str)] = &[
        ('あ', "a"),
        ('い', "i"),
        ('う', "u"),
        ('え', "e"),
        ('お', "o"),
        ('か', "ka"),
        ('き', "ki"),
        ('く', "ku"),
        ('け', "ke"),
        ('こ', "ko"),
        ('が', "ga"),
        ('ぎ', "gi"),
        ('ぐ', "gu"),
        ('げ', "ge"),
        ('ご', "go"),
        ('さ', "sa"),
        ('し', "shi"),
        ('す', "su"),
        ('せ', "se"),
        ('そ', "so"),
        ('ざ', "za"),
        ('じ', "ji"),
        ('ず', "zu"),
        ('ぜ', "ze"),
        ('ぞ', "zo"),
        ('た', "ta"),
        ('ち', "chi"),
        ('つ', "tsu"),
        ('て', "te"),
        ('と', "to"),
        ('だ', "da"),
        ('ぢ', "ji"),
        ('づ', "zu"),
        ('で', "de"),
        ('ど', "do"),
        ('な', "na"),
        ('に', "ni"),
        ('ぬ', "nu"),
        ('ね', "ne"),
        ('の', "no"),
        ('は', "ha"),
        ('ひ', "hi"),
        ('ふ', "fu"),
        ('へ', "he"),
        ('ほ', "ho"),
        ('ば', "ba"),
        ('び', "bi"),
        ('ぶ', "bu"),
        ('べ', "be"),
        ('ぼ', "bo"),
        ('ぱ', "pa"),
        ('ぴ', "pi"),
        ('ぷ', "pu"),
        ('ぺ', "pe"),
        ('ぽ', "po"),
        ('ま', "ma"),
        ('み', "mi"),
        ('む', "mu"),
        ('め', "me"),
        ('も', "mo"),
        ('や', "ya"),
        ('ゆ', "yu"),
        ('よ', "yo"),
        ('ら', "ra"),
        ('り', "ri"),
        ('る', "ru"),
        ('れ', "re"),
        ('ろ', "ro"),
        ('わ', "wa"),
        ('ゐ', "i"),
        ('ゑ', "e"),
        ('を', "o"),
        ('ん', "q"),
        ('ゔ', "bu"),
        ('っ', "cl"),
        ('ゎ', "wa"),
        ('、', ","),
        ('，', ","),
        (',', ","),
        ('.', "."),
        ('　', ","),
        (' ', ","),
        ('。', "."),
        ('．', "."),
        ('！', "."),
        ('？', "."),
        ('!', "."),
        ('?', "."),
    ];
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        // Katakana sit 0x60 above hiragana.
        let c = match c {
            'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            c => c,
        };
        let last_vowel = out.chars().rev().find(|c| "aiueo".contains(*c));
        match c {
            // Small ゃゅょ: きゃ is kya, しゃ is sha.
            'ゃ' | 'ゅ' | 'ょ' if out.ends_with('i') => {
                out.pop();
                if !(out.ends_with("sh") || out.ends_with("ch") || out.ends_with('j')) {
                    out.push('y');
                }
                out.push(match c {
                    'ゃ' => 'a',
                    'ゅ' => 'u',
                    _ => 'o',
                });
            }
            // Small vowels: ふぁ is fa, てぃ is ti.
            'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' => {
                let v = match c {
                    'ぁ' => 'a',
                    'ぃ' => 'i',
                    'ぅ' => 'u',
                    'ぇ' => 'e',
                    _ => 'o',
                };
                if out.ends_with(['a', 'i', 'u', 'e', 'o']) {
                    out.pop();
                }
                out.push(v);
            }
            'ー' | '～' | '〜' => {
                if let Some(v) = last_vowel {
                    out.push(v);
                }
            }
            // Checked on hardware: は before 、 is read "wa", falling in
            // pitch, as a particle; before 。 and other punctuation, or
            // anywhere else, it is "ha".
            'は' if chars.get(i + 1) == Some(&'、') => out.push_str("wa↓"),
            c => match TABLE.iter().find(|(k, _)| *k == c) {
                Some((_, letters)) => out.push_str(letters),
                // UNVERIFIED: anything else is skipped.
                None if "<>0123456789SPVspv#".contains(c) => out.push(c),
                None => {}
            },
        }
    }
    out
}

/// A consonant's phoneme in the bank.
fn consonant(letters: &str) -> Option<&'static str> {
    Some(match letters {
        "k" => "K",
        "g" => "G",
        "s" => "S",
        "sh" => "SH",
        "z" => "Z",
        "j" => "JH",
        "t" => "T",
        "ch" => "CH",
        "ts" => "TS",
        "d" => "D",
        "n" => "N",
        "h" => "HH",
        "f" => "F",
        "b" => "B",
        "p" => "P",
        "m" => "M",
        "r" => "DX",
        "w" => "W",
        "y" => "Y",
        _ => return None,
    })
}

/// Where a sung note's phoneme letters stop making syllables, as an index
/// into `letters`, or None when they all do. The help's letters are the
/// vowels, `q` (ん), `cl` (っ) and consonants (`b d f g h j k m n p r s t
/// w y z`, `by ch dy gy hy ky my ny py sh ts`); a consonant goes before a
/// vowel or ends the letters (`*K,C16`). The first letter that cannot go
/// on is the wrong one: in `KONNICHIWA`, the second N.
// UNVERIFIED: `ry`, which the help leaves out though りゃ needs it; and
// that a `c` with no `h` or `l` after it is wrong at the next letter.
pub(crate) fn bad_letter(letters: &[char]) -> Option<usize> {
    let at = |i: usize| letters.get(i).map(char::to_ascii_lowercase);
    let vowel = |c: Option<char>| c.is_some_and(|c| "aiueo".contains(c));
    let mut i = 0;
    while let Some(c) = at(i) {
        let next = at(i + 1);
        // Where the consonant's letters end.
        let end = match (c, next) {
            _ if vowel(Some(c)) || c == 'q' => {
                i += 1;
                continue;
            }
            ('c', Some('l')) => {
                i += 2;
                continue;
            }
            ('c', Some('h')) | ('s', Some('h')) | ('t', Some('s')) => i + 2,
            ('b' | 'd' | 'g' | 'h' | 'k' | 'm' | 'n' | 'p' | 'r', Some('y')) => i + 2,
            ('c', _) => return Some(i + 1),
            ('b' | 'd' | 'f' | 'g' | 'h' | 'j' | 'k' | 'm' | 'n' | 'p' | 'r' | 's' | 't', _)
            | ('w' | 'y' | 'z', _) => i + 1,
            _ => return Some(i),
        };
        match at(end) {
            None => return None,
            v if vowel(v) => i = end + 1,
            _ => return Some(end),
        }
    }
    None
}

/// SmileBASIC's phoneme letters, `<S…>` and the like as morae.
fn morae(text: &str) -> Vec<Mora> {
    let chars: Vec<char> = text.to_ascii_lowercase().chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let rest: String = chars[i..].iter().take(2).collect();
        if c == '<' {
            let end = chars[i..]
                .iter()
                .position(|&c| c == '>')
                .map_or(chars.len(), |p| i + p);
            let inside: String = chars[i + 1..end].iter().collect();
            let mut it = inside.chars();
            if let Some(kind) = it.next().map(|k| k.to_ascii_uppercase())
                && let Ok(n) = it.as_str().trim().parse()
            {
                out.push(Mora::Control(kind, n));
            }
            i = end + 1;
            continue;
        }
        if "aiueo".contains(c) {
            out.push(Mora::Syllable {
                consonant: None,
                glide: false,
                vowel: c,
            });
            i += 1;
            continue;
        }
        match c {
            'q' => {
                out.push(Mora::Nasal);
                i += 1;
                continue;
            }
            '↓' => {
                out.push(Mora::Fall);
                i += 1;
                continue;
            }
            '#' => {
                out.push(Mora::Pause(BREAK));
                i += 1;
                continue;
            }
            ',' => {
                out.push(Mora::Pause(SHORT_PAUSE));
                i += 1;
                continue;
            }
            '.' => {
                out.push(Mora::Pause(LONG_PAUSE));
                i += 1;
                continue;
            }
            _ => {}
        }
        if rest == "cl" {
            out.push(Mora::Hold);
            i += 2;
            continue;
        }
        // A consonant, perhaps two letters, perhaps with y, then a vowel.
        let (letters, used) = match rest.as_str() {
            "sh" | "ch" | "ts" => (rest.clone(), 2),
            _ => (c.to_string(), 1),
        };
        // UNVERIFIED: that TALK skips letters that are no phoneme, where a
        // sung note's are Illegal MML (see `bad_letter`).
        let Some(cons) = consonant(&letters) else {
            i += 1;
            continue;
        };
        let mut j = i + used;
        let glide = cons != "Y" && chars.get(j) == Some(&'y');
        if glide {
            j += 1;
        }
        match chars.get(j) {
            Some(&v) if "aiueo".contains(v) => {
                out.push(Mora::Syllable {
                    consonant: Some(cons),
                    glide,
                    vowel: v,
                });
                i = j + 1;
            }
            // A lone n is ん.
            _ if cons == "N" => {
                out.push(Mora::Nasal);
                i = j;
            }
            // A consonant on its own, with a silent u.
            _ => {
                out.push(Mora::Syllable {
                    consonant: Some(cons),
                    glide,
                    vowel: 'U',
                });
                i = j;
            }
        }
    }
    out
}

fn voiceless(consonant: Option<&str>) -> bool {
    matches!(
        consonant,
        Some("K" | "S" | "SH" | "T" | "CH" | "TS" | "HH" | "F" | "P")
    )
}

/// Builds the synthesizer's schedule for some morae.
struct Planner<'a> {
    table: &'a HashMap<&'static str, PhonemeParams>,
    events: Vec<ScheduleEvent>,
    /// Where the next event goes, in samples.
    at: f64,
    speed: f64,
    pitch: f32,
    gain: f32,
}

fn samples(ms: f64) -> f64 {
    ms * f64::from(RATE) / 1000.0
}

impl Planner<'_> {
    fn ms(&self, ms: f64) -> f64 {
        samples(ms * self.speed)
    }

    fn target(&mut self, update: ParamUpdate, transition_ms: f64) {
        self.events.push(ScheduleEvent {
            at_sample: self.at as u64,
            target: update,
            transition_samples: samples(transition_ms).max(1.0) as u32,
        });
    }

    fn phoneme(&self, name: &str) -> ParamUpdate {
        let p = self.table.get(name).copied().unwrap_or(self.table["_"]);
        ParamUpdate {
            f1: Some(p.f1),
            f2: Some(p.f2),
            f3: Some(p.f3),
            bw1: Some(p.bw1),
            bw2: Some(p.bw2),
            bw3: Some(p.bw3),
            a1: Some(p.a1),
            a2: Some(p.a2),
            a3: Some(p.a3),
            voicing: Some(p.voicing),
            aspiration: Some(0.0),
            gain: Some(self.gain),
            ..ParamUpdate::default()
        }
    }

    fn silence(&mut self, ms: f64) {
        let quiet = ParamUpdate {
            a1: Some(0.0),
            a2: Some(0.0),
            a3: Some(0.0),
            voicing: Some(0.0),
            aspiration: Some(0.0),
            ..ParamUpdate::default()
        };
        self.target(quiet, QUICK);
        self.at += self.ms(ms);
    }

    /// A vowel; one that `falls` glides down in pitch through its second
    /// half.
    fn vowel(&mut self, vowel: char, ms: f64, devoiced: bool, f0: f32, falls: bool) {
        let mut v = self.phoneme(&vowel.to_ascii_uppercase().to_string());
        v.f0 = Some(f0);
        if devoiced {
            v.voicing = Some(0.0);
            v.aspiration = Some(0.35);
        }
        self.target(v, TRANSITION);
        if falls {
            self.at += self.ms(ms / 2.0);
            let down = ParamUpdate {
                f0: Some(f0 * FALL),
                ..ParamUpdate::default()
            };
            self.target(down, ms * self.speed / 2.0);
            self.at += self.ms(ms / 2.0);
        } else {
            self.at += self.ms(ms);
        }
    }

    /// The consonant's part of a mora; gives the milliseconds it took.
    fn consonant(&mut self, name: &'static str, f0: f32) -> f64 {
        match name {
            "CH" | "JH" | "TS" => {
                // A stop released into a fricative: ち is t then し's
                // hiss, じ the same voiced, つ t then す's.
                let (stop, fricative) = match name {
                    "CH" => ("CH", "SH"),
                    "JH" => ("JH", "ZH"),
                    _ => ("T", "S"),
                };
                self.silence(AFFRICATE_CLOSURE);
                let mut burst = self.phoneme(stop);
                burst.f0 = Some(f0);
                if voiceless(Some(name)) {
                    burst.aspiration = Some(0.6);
                }
                self.target(burst, 2.0);
                self.at += self.ms(AFFRICATE_BURST);
                let mut f = self.phoneme(fricative);
                f.f0 = Some(f0);
                self.target(f, QUICK);
                self.at += self.ms(AFFRICATE_FRICATION);
                AFFRICATE_CLOSURE + AFFRICATE_BURST + AFFRICATE_FRICATION
            }
            "K" | "G" | "T" | "D" | "P" | "B" => {
                self.silence(CLOSURE);
                let mut burst = self.phoneme(name);
                burst.f0 = Some(f0);
                if voiceless(Some(name)) {
                    burst.aspiration = Some(0.6);
                }
                self.target(burst, 2.0);
                self.at += self.ms(BURST);
                CLOSURE + BURST
            }
            "S" | "SH" | "Z" | "F" | "HH" => {
                let mut f = self.phoneme(name);
                f.f0 = Some(f0);
                if name == "HH" {
                    f.aspiration = Some(0.7);
                }
                self.target(f, 15.0);
                self.at += self.ms(FRICATIVE);
                FRICATIVE
            }
            _ => {
                let mut s = self.phoneme(name);
                s.f0 = Some(f0);
                self.target(s, 15.0);
                self.at += self.ms(SONORANT);
                SONORANT
            }
        }
    }
}

/// The bank's phonemes, read once for TALK and every singing channel.
fn table() -> &'static HashMap<&'static str, PhonemeParams> {
    static TABLE: OnceLock<HashMap<&'static str, PhonemeParams>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let registry = BankRegistry::with_bundled();
        let bank = registry
            .resolve(BANK)
            .expect("klattsch-core bundles the Japanese bank");
        PHONEMES
            .iter()
            .filter_map(|&n| bank.lookup(n).map(|p| (n, p)))
            .collect()
    })
}

pub struct Talk {
    synth: FormantSynth,
    /// Samples left to play.
    left: u64,
    buffer: Vec<f32>,
}

impl Default for Talk {
    fn default() -> Self {
        Talk {
            synth: FormantSynth::new(RATE),
            left: 0,
            buffer: Vec::new(),
        }
    }
}

impl Talk {
    /// Starts saying `text`: kana, or phoneme letters after a `$`.
    pub fn say(&mut self, text: &str) {
        // Full-width letters, digits and brackets, as in ＜S128＞, read as
        // their ASCII forms.
        let text: String = text
            .chars()
            .map(|c| match c {
                '！'..='～' if !"，．！？～".contains(c) => {
                    char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)
                }
                c => c,
            })
            .collect();
        // Phoneme letters follow a `$`, which control strings may come
        // before.
        let mut tags = 0;
        while text[tags..].starts_with('<') {
            match text[tags..].find('>') {
                Some(end) => tags += end + 1,
                None => break,
            }
        }
        let letters = match text[tags..].strip_prefix('$') {
            Some(letters) => format!("{}{letters}", &text[..tags]),
            None => kana_letters(&text),
        };
        let morae = morae(&letters);
        let mut plan = Planner {
            table: table(),
            events: Vec::new(),
            at: 0.0,
            speed: 1.0,
            pitch: BASE_PITCH,
            gain: BASE_GAIN,
        };
        let spoken = morae
            .iter()
            .filter(|m| !matches!(m, Mora::Control(..) | Mora::Pause(_) | Mora::Fall))
            .count()
            .max(1);
        let mut beat = 0;
        for (i, m) in morae.iter().enumerate() {
            let fall = beat as f32 / spoken as f32;
            let f0 = plan.pitch * (PITCH_START + (PITCH_END - PITCH_START) * fall);
            match *m {
                Mora::Fall => {}
                Mora::Control(kind, n) => match kind {
                    // 0 fast to 1024 slow, 128 standard.
                    'S' => plan.speed = f64::from(n.clamp(16, 1024)) / 128.0,
                    // 0 high to 1024 low, 128 standard.
                    'P' => plan.pitch = BASE_PITCH * 128.0 / n.clamp(16, 1024) as f32,
                    'V' => plan.gain = BASE_GAIN * n.clamp(0, 128) as f32 / 128.0,
                    _ => {}
                },
                Mora::Pause(ms) => {
                    plan.silence(ms);
                }
                Mora::Hold => {
                    plan.silence(MORA);
                    beat += 1;
                }
                Mora::Nasal => {
                    let mut n = plan.phoneme("N");
                    n.f0 = Some(f0);
                    plan.target(n, TRANSITION);
                    plan.at += plan.ms(MORA * 0.85);
                    beat += 1;
                }
                Mora::Syllable {
                    consonant,
                    glide,
                    vowel,
                } => {
                    let mut taken = 0.0;
                    if let Some(c) = consonant {
                        taken += plan.consonant(c, f0);
                    }
                    if glide {
                        let mut y = plan.phoneme("Y");
                        y.f0 = Some(f0);
                        plan.target(y, QUICK);
                        plan.at += plan.ms(GLIDE);
                        taken += GLIDE;
                    }
                    // A vowel of only `u`, written with a lone consonant.
                    let silent = vowel == 'U';
                    let vowel = vowel.to_ascii_lowercase();
                    // I and u between voiceless consonants, or at the end
                    // after one, are whispered.
                    let next = morae[i + 1..]
                        .iter()
                        .find(|m| !matches!(m, Mora::Control(..) | Mora::Fall));
                    let next_voiceless = match next {
                        Some(Mora::Syllable { consonant, .. }) => voiceless(*consonant),
                        None | Some(Mora::Pause(_)) => true,
                        _ => false,
                    };
                    let devoiced = silent
                        || (matches!(vowel, 'i' | 'u') && voiceless(consonant) && next_voiceless);
                    let falls = morae.get(i + 1) == Some(&Mora::Fall);
                    plan.vowel(vowel, (MORA - taken).max(50.0), devoiced, f0, falls);
                    beat += 1;
                }
            }
        }
        plan.silence(60.0);
        let total = plan.at as u64;
        let events = plan.events;
        self.synth.reset(Params {
            gain: BASE_GAIN,
            ..Params::DEFAULT
        });
        self.synth
            .queue_schedule(Schedule::from_sample_events(RATE, events));
        self.left = total;
    }

    pub fn speaking(&self) -> bool {
        self.left > 0
    }

    pub fn stop(&mut self) {
        self.left = 0;
    }

    /// Adds the speech into the buffers.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.left == 0 {
            return;
        }
        let n = left.len().min(self.left as usize);
        self.buffer.resize(n, 0.0);
        self.synth.process(&mut self.buffer);
        for i in 0..n {
            left[i] += self.buffer[i] * 0.5;
            right[i] += self.buffer[i] * 0.5;
        }
        self.left -= n as u64;
    }
}

/// How long a sung note takes to fall silent once let go.
const SUNG_RELEASE: f64 = 30.0;

/// The pitch of MIDI key `key`, `cents` above it, in Hz.
pub(crate) fn key_hz(key: i32, cents: i32) -> f32 {
    let semitones = (key - 69) as f32 + cents as f32 / 100.0;
    440.0 * (semitones / 12.0).exp2()
}

/// One MML channel's singing voice, for `*WA,C4`: the phonemes said at
/// the note's pitch, the last vowel held until the note is let go.
pub(crate) struct Singer {
    synth: FormantSynth,
    buffer: Vec<f32>,
    /// The note's pitch, before a portamento moves it.
    hz: f32,
    /// Samples left of the release once let go.
    release: Option<u32>,
    sounding: bool,
}

impl Singer {
    pub(crate) fn new(seed: u32) -> Singer {
        Singer {
            synth: FormantSynth::with_seed(RATE, seed),
            buffer: Vec::new(),
            hz: 0.0,
            release: None,
            sounding: false,
        }
    }

    /// Starts singing `letters` (SmileBASIC's phoneme letters) at `hz`
    /// over a note of `ms`. Each mora gets an even part of the note, its
    /// consonant shortened in a short one; a lone consonant is sung with
    /// u. False, and silent, when the letters have nothing to sing.
    // UNVERIFIED: how SmileBASIC 4 shares a note between several morae,
    // and how long its consonants are.
    pub(crate) fn sing(&mut self, letters: &str, hz: f32, ms: f64) -> bool {
        let morae: Vec<Mora> = morae(letters)
            .into_iter()
            .filter(|m| matches!(m, Mora::Syllable { .. } | Mora::Nasal | Mora::Hold))
            .collect();
        if morae.is_empty() {
            return false;
        }
        let share = ms / morae.len() as f64;
        let mut plan = Planner {
            table: table(),
            events: Vec::new(),
            at: 0.0,
            speed: (share / MORA).min(1.0),
            pitch: hz,
            gain: BASE_GAIN,
        };
        for m in &morae {
            let end = plan.at + samples(share);
            match *m {
                Mora::Hold => plan.silence(share / plan.speed),
                Mora::Nasal => {
                    let n = plan.phoneme("N");
                    plan.target(n, TRANSITION);
                }
                Mora::Syllable {
                    consonant,
                    glide,
                    vowel,
                } => {
                    if let Some(c) = consonant {
                        plan.consonant(c, hz);
                    }
                    if glide {
                        let y = plan.phoneme("Y");
                        plan.target(y, QUICK);
                        plan.at += plan.ms(GLIDE);
                    }
                    let v = plan.phoneme(&vowel.to_ascii_uppercase().to_string());
                    plan.target(v, TRANSITION);
                }
                _ => {}
            }
            plan.at = plan.at.max(end);
        }
        let mut events = plan.events;
        // The pitch is the note's alone, which a portamento then moves.
        for e in &mut events {
            e.target.f0 = None;
        }
        self.synth.reset(Params {
            f0: hz,
            gain: BASE_GAIN,
            ..Params::DEFAULT
        });
        self.synth
            .queue_schedule(Schedule::from_sample_events(RATE, events));
        self.hz = hz;
        self.release = None;
        self.sounding = true;
        true
    }

    /// The note goes on at another pitch, for a slur.
    pub(crate) fn retune(&mut self, hz: f32) {
        self.hz = hz;
        self.bend(0.0);
    }

    /// Moves the note by `cents`, for a portamento.
    pub(crate) fn bend(&mut self, cents: f32) {
        let f0 = self.hz * (cents / 1200.0).exp2();
        let update = ParamUpdate {
            f0: Some(f0),
            ..ParamUpdate::default()
        };
        self.synth.set_target(update, samples(QUICK) as u32);
    }

    /// Lets the note go: it falls silent.
    pub(crate) fn release(&mut self) {
        if self.sounding && self.release.is_none() {
            let quiet = ParamUpdate {
                a1: Some(0.0),
                a2: Some(0.0),
                a3: Some(0.0),
                voicing: Some(0.0),
                aspiration: Some(0.0),
                ..ParamUpdate::default()
            };
            // Whatever of the schedule is left would sound again.
            self.synth.queue_schedule(Schedule::empty(RATE));
            let n = samples(SUNG_RELEASE) as u32;
            self.synth.set_target(quiet, n);
            self.release = Some(n);
        }
    }

    pub(crate) fn stop(&mut self) {
        self.sounding = false;
    }

    pub(crate) fn sounding(&self) -> bool {
        self.sounding
    }

    /// Adds the voice into the buffers at the gains given for each side.
    pub(crate) fn render(&mut self, left: &mut [f32], right: &mut [f32], gains: (f32, f32)) {
        if !self.sounding {
            return;
        }
        let n = left.len();
        self.buffer.resize(n, 0.0);
        self.synth.process(&mut self.buffer);
        for i in 0..n {
            left[i] += self.buffer[i] * gains.0;
            right[i] += self.buffer[i] * gains.1;
        }
        if let Some(left) = &mut self.release {
            *left = left.saturating_sub(n as u32);
            if *left == 0 {
                self.sounding = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kana_become_letters() {
        assert_eq!(kana_letters("こんにちは"), "koqnichiha");
        assert_eq!(kana_letters("キャット"), "kyaclto");
        assert_eq!(kana_letters("しゃしん、ラーメン。"), "shashiq,raameq.");
        assert_eq!(kana_letters("ファイル"), "fairu");
        // は is "wa" before 、, falling, and "ha" otherwise.
        assert_eq!(kana_letters("わたしは、ハナは。"), "watashiwa↓,hanaha.");
    }

    #[test]
    fn letters_become_morae() {
        let m = morae("kyaqcl#ts<S200>");
        assert_eq!(
            m,
            [
                Mora::Syllable {
                    consonant: Some("K"),
                    glide: true,
                    vowel: 'a'
                },
                Mora::Nasal,
                Mora::Hold,
                Mora::Pause(BREAK),
                Mora::Syllable {
                    consonant: Some("TS"),
                    glide: false,
                    vowel: 'U'
                },
                Mora::Control('S', 200),
            ]
        );
    }

    /// The help's two ways of saying the same thing take the same time.
    #[test]
    fn kana_and_letters_agree() {
        let mut a = Talk::default();
        a.say("<S128>こんにちわ。わたしは、プチコンフォーです");
        let mut b = Talk::default();
        b.say("$<S128>koqnichiwa######watashiwa##puchikoqfoodesu");
        assert_eq!(a.left, b.left);
        let mut c = Talk::default();
        c.say("＜S256＞あ");
        let mut d = Talk::default();
        d.say("<S256>$a");
        assert_eq!(c.left, d.left);
    }

    #[test]
    fn speech_makes_sound_and_ends() {
        let mut talk = Talk::default();
        talk.say("こんにちは");
        assert!(talk.speaking());
        let (mut l, mut r) = (vec![0.0; 48000], vec![0.0; 48000]);
        talk.render(&mut l, &mut r);
        assert!(l.iter().any(|s| s.abs() > 0.05));
        assert!(!talk.speaking());
    }

    /// Milliseconds of hiss from a sung syllable's first sound until its
    /// vowel: 5 ms frames whose sound is mostly high (its slope against
    /// its level) above `hiss`.
    fn frication_ms(letters: &str, hiss: f32) -> usize {
        let mut singer = Singer::new(1);
        assert!(singer.sing(letters, key_hz(60, 0), 600.0));
        let (mut l, mut r) = (vec![0.0; 9600], vec![0.0; 9600]);
        singer.render(&mut l, &mut r, (1.0, 0.0));
        let frame = (RATE / 200) as usize;
        let onset = l.iter().position(|s| s.abs() > 0.01).unwrap();
        l[onset..]
            .chunks(frame)
            .take_while(|f| {
                let level: f32 = f.iter().map(|s| s * s).sum();
                let slope: f32 = f.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
                slope > level * hiss * hiss
            })
            .count()
            * 5
    }

    /// ち, つ and じ are released into a fricative, not straight into
    /// the vowel as てぃ's ti and でぃ's di are.
    #[test]
    fn affricates_hiss_after_the_burst() {
        assert!(table().contains_key("ZH"));
        assert!(frication_ms("chi", 0.3) >= 40);
        assert!(frication_ms("ti", 0.3) <= 25);
        assert!(frication_ms("tsu", 0.3) >= 40);
        // Voicing under じ's hiss keeps its sound lower.
        assert!(frication_ms("ji", 0.1) >= 40);
        assert!(frication_ms("di", 0.1) <= 25);
    }
}

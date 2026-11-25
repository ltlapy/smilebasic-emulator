//! TALK: Japanese speech from kana or SmileBASIC's phoneme letters, on a
//! Klatt formant synthesizer (klattsch-core). Kana become phoneme letters,
//! the letters become morae, and each mora a few targets for the
//! synthesizer to glide between. MML's sung notes (`*WA,C4`) use the
//! same phonemes on each channel's own [`Singer`].
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

/// Milliseconds at the standard speed, `<S128>`: あいうえお takes 460.
const MORA: f64 = 90.0;
/// The shortest vowel after a consonant: a mora with a consonant takes
/// longer than one without (わたしは、 takes about 510 ms).
// UNVERIFIED: how SmileBASIC 4 shares a mora between its sounds.
const SHORTEST_VOWEL: f64 = 70.0;
/// The shortest vowel without a consonant.
const SHORTEST_BARE_VOWEL: f64 = 50.0;
const CLOSURE: f64 = 45.0;
const BURST: f64 = 15.0;
const FRICATIVE: f64 = 70.0;
/// ち, じ and つ: a shorter closure and burst than a stop has, then the
/// fricative they are released into.
const AFFRICATE_CLOSURE: f64 = 30.0;
const AFFRICATE_BURST: f64 = 8.0;
const AFFRICATE_FRICATION: f64 = 45.0;
const SONORANT: f64 = 45.0;
const GLIDE: f64 = 40.0;
/// 、 and 。, with the lengths from the help's example: it writes
/// "こんにちわ。わたしは、" as "koqnichiwa######watashiwa##".
const SHORT_PAUSE: f64 = 200.0;
const LONG_PAUSE: f64 = 600.0;
/// `#`: 0.1 seconds without sound.
const BREAK: f64 = 100.0;
/// The time after the last sound during which TALKCHK stays set: 20
/// vowels keep it set for 121 frames.
// UNVERIFIED: whether this is silence or the voice's own delay.
const TAIL: f64 = 205.0;
/// Glides between targets.
const TRANSITION: f64 = 25.0;
const QUICK: f64 = 8.0;
/// The glide into a stop's burst.
const BURST_TRANSITION: f64 = 2.0;
/// The glide into a fricative or a sonorant.
const CONSONANT_TRANSITION: f64 = 15.0;

/// The aspiration of a whispered vowel.
const WHISPER_ASPIRATION: f32 = 0.35;
/// The aspiration of a voiceless burst.
const BURST_ASPIRATION: f32 = 0.6;
/// The aspiration of h (`HH`).
const H_ASPIRATION: f32 = 0.7;
/// The gain of the speech on each side.
const SIDE_GAIN: f32 = 0.5;

/// The voice's pitch without a `<P…>`: one pitch, with no fall through a
/// phrase or before 、 and 。.
const BASE_PITCH: f32 = 100.0;
/// With a `<P…>`, 128 included, the voice speaks at about this divided
/// by P, in Hz (`<P64>` 314 Hz, `<P256>` 79 Hz), and its pitch moves
/// through the phrase: see [`INTONATION`].
const PITCH_TIMES_P: f32 = 20100.0;
// The old rule of `<P…>`: (128 / P) to this power times the base pitch.
// const PITCH_POWER: f32 = 0.35;
/// With a `<P…>`, the first mora is this much above the pitch, the last
/// this much below, and those between at the pitch: あいうえお at
/// `<P128>` is spoken at 180, 157, 160, 160 and 110 Hz.
// UNVERIFIED: seen on あいうえお only, at five values of P; the effect
// of longer phrases, 、 and 。 is unchecked.
const INTONATION: (f32, f32) = (1.14, 0.70);
/// The rate the voice's pitch periods are counted in: a period is a
/// whole number of these samples, so A4 sings at 16000 / 36 = 444.4 Hz.
const PERIOD_RATE: f32 = 16000.0;
/// The synthesizer's gain at `<V128>`.
const BASE_GAIN: f32 = 0.42;
/// The synthesizer's gain for a sung note.
const SINGING_GAIN: f32 = 1.88;
// The intonation this voice had before: SmileBASIC 4 speaks flat.
// /// Pitch at a phrase's start and end, as a part of the base: speech falls
// /// as it goes.
// const PITCH_START: f32 = 1.06;
// const PITCH_END: f32 = 0.88;
/// The end pitch of a falling syllable, as a fraction of its start pitch.
/// Nothing falls now: SmileBASIC 4 speaks flat.
const FALL: f32 = 0.8;

/// `hz` moved to the nearest pitch whose period is a whole number of
/// [`PERIOD_RATE`] samples, for `TALK`.
// UNVERIFIED: rounded to the nearest period, or cut down to it the same
// as for sung notes; `<P64>`'s 314 Hz matches the nearest.
pub(crate) fn whole_period(hz: f32) -> f32 {
    if hz <= 0.0 {
        return hz;
    }
    PERIOD_RATE / (PERIOD_RATE / hz).round().max(1.0)
}

/// `hz` with its period cut down to a whole number of [`PERIOD_RATE`]
/// samples, the rule for sung notes: C5 sings at 533.3 Hz, F4 at 355.6.
pub(crate) fn sung_period(hz: f32) -> f32 {
    if hz <= 0.0 {
        return hz;
    }
    PERIOD_RATE / (PERIOD_RATE / hz).floor().max(1.0)
}

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
    /// After は before 、. It made the syllable fall in pitch, which
    /// SmileBASIC 4 does not do.
    Fall,
    /// `<S…>`, `<P…>` and `<V…>`.
    Control(char, i32),
}

/// Kana and punctuation as phoneme letters.
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

/// Kana as SmileBASIC's phoneme letters: `q` for ん, `cl` for っ.
fn kana_letters(text: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    // The vowel ー repeats: the last vowel in `out`. It is kept rather
    // than searched for; a search would make long text with few vowels
    // quadratic.
    let mut last_vowel = None;
    for (i, &c) in chars.iter().enumerate() {
        // Katakana are 0x60 above hiragana.
        let c = match c {
            'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            c => c,
        };
        match c {
            // Small ゃゅょ: きゃ is kya, しゃ is sha.
            'ゃ' | 'ゅ' | 'ょ' if out.ends_with('i') => {
                out.pop();
                if !(out.ends_with("sh") || out.ends_with("ch") || out.ends_with('j')) {
                    out.push('y');
                }
                let v = small_vowel(c);
                out.push(v);
                last_vowel = Some(v);
            }
            // Small vowels: ふぁ is fa, てぃ is ti.
            'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' => {
                let v = small_vowel(c);
                if out.ends_with(['a', 'i', 'u', 'e', 'o']) {
                    out.pop();
                }
                out.push(v);
                last_vowel = Some(v);
            }
            'ー' | '～' | '〜' => {
                if let Some(v) = last_vowel {
                    out.push(v);
                }
            }
            // は before 、 is read "wa", as a particle; before 。 and other
            // punctuation, or anywhere else, it is "ha". The ↓ after it
            // once made it fall in pitch; SmileBASIC 4 keeps its pitch.
            'は' if chars.get(i + 1) == Some(&'、') => {
                out.push_str("wa↓");
                last_vowel = Some('a');
            }
            c => match TABLE.iter().find(|(k, _)| *k == c) {
                Some((_, letters)) => {
                    out.push_str(letters);
                    if let Some(v) = letters.chars().rev().find(|c| "aiueo".contains(*c)) {
                        last_vowel = Some(v);
                    }
                }
                // UNVERIFIED: anything else is skipped.
                None if "<>0123456789SPVspv#".contains(c) => out.push(c),
                None => {}
            },
        }
    }
    out
}

/// The vowel of a small kana: ゃ, ゅ, ょ or a small vowel.
fn small_vowel(c: char) -> char {
    match c {
        'ゃ' | 'ぁ' => 'a',
        'ぃ' => 'i',
        'ゅ' | 'ぅ' => 'u',
        'ぇ' => 'e',
        _ => 'o',
    }
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

/// The vowel of a consonant written alone: a silent u, in uppercase to
/// tell it from a written u (the letters are lowercased first).
const SILENT_U: char = 'U';

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
        // TALK and sung notes skip letters that are not a phoneme (`$xa`,
        // `*XA,C`): neither is an error.
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
                    vowel: SILENT_U,
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
    /// The position of the next event, in samples.
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
            v.aspiration = Some(WHISPER_ASPIRATION);
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

    /// The consonant's part of a mora; returns the milliseconds it took.
    fn consonant(&mut self, name: &'static str, f0: f32) -> f64 {
        match name {
            "CH" | "JH" | "TS" => {
                // A stop released into a fricative: ち is t then し's
                // hiss, じ the same voiced, つ t then す's hiss.
                let (stop, fricative) = match name {
                    "CH" => ("CH", "SH"),
                    "JH" => ("JH", "ZH"),
                    _ => ("T", "S"),
                };
                self.silence(AFFRICATE_CLOSURE);
                let mut burst = self.phoneme(stop);
                burst.f0 = Some(f0);
                if voiceless(Some(name)) {
                    burst.aspiration = Some(BURST_ASPIRATION);
                }
                self.target(burst, BURST_TRANSITION);
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
                    burst.aspiration = Some(BURST_ASPIRATION);
                }
                self.target(burst, BURST_TRANSITION);
                self.at += self.ms(BURST);
                CLOSURE + BURST
            }
            "S" | "SH" | "Z" | "F" | "HH" => {
                let mut f = self.phoneme(name);
                f.f0 = Some(f0);
                if name == "HH" {
                    f.aspiration = Some(H_ASPIRATION);
                }
                self.target(f, CONSONANT_TRANSITION);
                self.at += self.ms(FRICATIVE);
                FRICATIVE
            }
            _ => {
                let mut s = self.phoneme(name);
                s.f0 = Some(f0);
                self.target(s, CONSONANT_TRANSITION);
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

/// `text` with full-width letters, digits and brackets, as in ＜S128＞,
/// replaced by their ASCII forms.
fn ascii_forms(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '！'..='～' if !"，．！？～".contains(c) => {
                char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)
            }
            c => c,
        })
        .collect()
}

/// The pitch of the mora at `beat` of `spoken` sounding morae, for a
/// voice at `pitch`. With a `<P…>` (`intoned`) and more than one mora, the
/// first and last move by [`INTONATION`].
fn intoned_pitch(pitch: f32, intoned: bool, beat: usize, spoken: usize) -> f32 {
    match (intoned && spoken > 1, beat) {
        (false, _) => pitch,
        (true, 0) => whole_period(pitch * INTONATION.0),
        (true, b) if b + 1 == spoken => whole_period(pitch * INTONATION.1),
        (true, _) => pitch,
    }
}

pub struct Talk {
    synth: FormantSynth,
    /// Samples left to play.
    left: u64,
    /// Samples left while TALKCHK reads 1.
    busy: u64,
    buffer: Vec<f32>,
}

impl Default for Talk {
    fn default() -> Self {
        Talk {
            synth: FormantSynth::new(RATE),
            left: 0,
            busy: 0,
            buffer: Vec::new(),
        }
    }
}

impl Talk {
    /// Starts saying `text`: kana, or phoneme letters after a `$`.
    pub fn say(&mut self, text: &str) {
        let text = ascii_forms(text);
        // Phoneme letters follow a `$` that starts the text, as in
        // `$<S128>koqnichiwa`. A `$` after a control string marks nothing:
        // `<V16>$a` is read as kana, and says nothing.
        let letters = match text.strip_prefix('$') {
            Some(letters) => letters.to_string(),
            None => kana_letters(&text),
        };
        let morae = morae(&letters);
        let sounds = |m: &Mora| matches!(m, Mora::Syllable { .. } | Mora::Nasal);
        let spoken = morae.iter().filter(|m| sounds(m)).count();
        let intoned = morae.iter().any(|m| matches!(m, Mora::Control('P', _)));
        let mut beat = 0;
        let mut plan = Planner {
            table: table(),
            events: Vec::new(),
            at: 0.0,
            speed: 1.0,
            pitch: BASE_PITCH,
            gain: BASE_GAIN,
        };
        // The intonation this voice had before: SmileBASIC 4 speaks flat.
        // let spoken = morae
        //     .iter()
        //     .filter(|m| !matches!(m, Mora::Control(..) | Mora::Pause(_) | Mora::Fall))
        //     .count()
        //     .max(1);
        // let mut beat = 0;
        for (i, m) in morae.iter().enumerate() {
            // let fall = beat as f32 / spoken as f32;
            // let f0 = plan.pitch * (PITCH_START + (PITCH_END - PITCH_START) * fall);
            let f0 = intoned_pitch(plan.pitch, intoned, beat, spoken);
            if sounds(m) {
                beat += 1;
            }
            match *m {
                Mora::Fall => {}
                Mora::Control(kind, n) => match kind {
                    // 0 fast to 1024 slow, 128 standard.
                    'S' => plan.speed = f64::from(n.clamp(16, 1024)) / 128.0,
                    // 0 high to 1024 low, 128 standard.
                    'P' => {
                        let p = n.clamp(16, 1024) as f32;
                        // plan.pitch = whole_period(BASE_PITCH * (128.0 / p).powf(PITCH_POWER));
                        plan.pitch = whole_period(PITCH_TIMES_P / p);
                    }
                    'V' => plan.gain = BASE_GAIN * n.clamp(0, 128) as f32 / 128.0,
                    _ => {}
                },
                Mora::Pause(ms) => {
                    plan.silence(ms);
                }
                Mora::Hold => {
                    plan.silence(MORA);
                    // beat += 1;
                }
                Mora::Nasal => {
                    let mut n = plan.phoneme("N");
                    n.f0 = Some(f0);
                    plan.target(n, TRANSITION);
                    plan.at += plan.ms(MORA * 0.85);
                    // beat += 1;
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
                    let silent = vowel == SILENT_U;
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
                    // は before 、 no longer falls: SmileBASIC 4 keeps its pitch.
                    // let falls = morae.get(i + 1) == Some(&Mora::Fall);
                    let falls = false;
                    let shortest = if consonant.is_some() || glide {
                        SHORTEST_VOWEL
                    } else {
                        SHORTEST_BARE_VOWEL
                    };
                    plan.vowel(vowel, (MORA - taken).max(shortest), devoiced, f0, falls);
                    // beat += 1;
                }
            }
        }
        plan.silence(TAIL);
        let total = plan.at as u64;
        let events = plan.events;
        self.synth.reset(Params {
            gain: BASE_GAIN,
            ..Params::DEFAULT
        });
        self.synth
            .queue_schedule(Schedule::from_sample_events(RATE, events));
        // Nothing to say, or one mora, keeps TALKCHK set only until the
        // next frame, though the mora keeps sounding.
        // UNVERIFIED: whether one mora alone sounds at all.
        self.left = if spoken == 0 { 0 } else { total };
        self.busy = if spoken <= 1 { 1 } else { total };
    }

    /// TALKCHK.
    pub fn speaking(&self) -> bool {
        self.busy > 0
    }

    /// Whether there is speech to play.
    pub fn sounding(&self) -> bool {
        self.left > 0
    }

    pub fn stop(&mut self) {
        self.left = 0;
        self.busy = 0;
    }

    /// Adds the speech into the buffers.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.busy = self.busy.saturating_sub(left.len() as u64);
        if self.left == 0 {
            return;
        }
        let n = left.len().min(self.left as usize);
        self.buffer.resize(n, 0.0);
        self.synth.process(&mut self.buffer);
        for i in 0..n {
            left[i] += self.buffer[i] * SIDE_GAIN;
            right[i] += self.buffer[i] * SIDE_GAIN;
        }
        self.left -= n as u64;
    }
}

/// The time a sung note takes to fall silent after it is released.
const SUNG_RELEASE: f64 = 30.0;

/// The pitch of MIDI key `key`, `cents` above it, in Hz.
pub(crate) fn key_hz(key: i32, cents: i32) -> f32 {
    let semitones = (key - 69) as f32 + cents as f32 / 100.0;
    440.0 * (semitones / 12.0).exp2()
}

/// One MML channel's singing voice, for `*WA,C4`: the phonemes said at
/// the note's pitch, the last vowel held until the note is released.
pub(crate) struct Singer {
    synth: FormantSynth,
    buffer: Vec<f32>,
    /// The note's pitch, before a portamento moves it.
    hz: f32,
    /// Samples left of the release after the note is released.
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
    /// over a note of `ms`. Each mora gets an equal part of the note, its
    /// consonant shortened when that part is short; a lone consonant is
    /// sung with u. Returns false, and stays silent, when the letters have
    /// nothing to sing.
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
        let hz = sung_period(hz);
        let share = ms / morae.len() as f64;
        let mut plan = Planner {
            table: table(),
            events: Vec::new(),
            at: 0.0,
            speed: (share / MORA).min(1.0),
            pitch: hz,
            gain: SINGING_GAIN,
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
        // The pitch comes only from the note, and a portamento moves it.
        for e in &mut events {
            e.target.f0 = None;
        }
        self.synth.reset(Params {
            f0: hz,
            gain: SINGING_GAIN,
            ..Params::DEFAULT
        });
        self.synth
            .queue_schedule(Schedule::from_sample_events(RATE, events));
        self.hz = hz;
        self.release = None;
        self.sounding = true;
        true
    }

    /// Continues the note at another pitch, for a slur.
    pub(crate) fn retune(&mut self, hz: f32) {
        self.hz = hz;
        self.bend(0.0);
    }

    /// Moves the note by `cents`, for a portamento.
    pub(crate) fn bend(&mut self, cents: f32) {
        let f0 = sung_period(self.hz * (cents / 1200.0).exp2());
        let update = ParamUpdate {
            f0: Some(f0),
            ..ParamUpdate::default()
        };
        self.synth.set_target(update, samples(QUICK) as u32);
    }

    /// Releases the note: it falls silent.
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
            // Any part of the schedule that is left would sound again.
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

    /// The old `kana_letters`, which searched for the last vowel again for
    /// every character. `kana_letters` must return the same output.
    fn kana_letters_by_search(text: &str) -> String {
        let mut out = String::new();
        let chars: Vec<char> = text.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            let c = match c {
                'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
                c => c,
            };
            let last_vowel = out.chars().rev().find(|c| "aiueo".contains(*c));
            match c {
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
                'は' if chars.get(i + 1) == Some(&'、') => out.push_str("wa↓"),
                c => match TABLE.iter().find(|(k, _)| *k == c) {
                    Some((_, letters)) => out.push_str(letters),
                    None if "<>0123456789SPVspv#".contains(c) => out.push(c),
                    None => {}
                },
            }
        }
        out
    }

    #[test]
    fn kana_letters_match_searching_for_the_vowel() {
        // Every kana in the table, the characters that change `out` other
        // than through it, and letters that are kept or dropped.
        let mut alphabet: Vec<char> = TABLE.iter().map(|(k, _)| *k).collect();
        alphabet.extend("ゃゅょぁぃぅぇぉーー～〜はは、、ャュョァィゥェォカキシチジン".chars());
        alphabet.extend("<>0S9#aiueoxyz漢".chars());
        let mut seed = 0x2545_F491u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as usize
        };
        for _ in 0..20_000 {
            let len = next() % 24;
            let text: String = (0..len)
                .map(|_| alphabet[next() % alphabet.len()])
                .collect();
            assert_eq!(kana_letters(&text), kana_letters_by_search(&text), "{text}");
        }
    }

    /// These took minutes when every character scanned the text before it.
    #[test]
    fn long_text_takes_time_in_step_with_its_length() {
        let n = 2_000_000;
        assert_eq!(kana_letters(&"<".repeat(n)).len(), n);
        assert_eq!(kana_letters(&"ー".repeat(n)), "");
        assert_eq!(morae(&"<".repeat(n)).len(), 0);
        assert_eq!(morae(&"<S1>".repeat(n / 4)).len(), n / 4);
    }

    #[test]
    fn kana_become_letters() {
        assert_eq!(kana_letters("こんにちは"), "koqnichiha");
        assert_eq!(kana_letters("キャット"), "kyaclto");
        assert_eq!(kana_letters("しゃしん、ラーメン。"), "shashiq,raameq.");
        assert_eq!(kana_letters("ファイル"), "fairu");
        // は is "wa" before 、 and "ha" otherwise.
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
        d.say("$<S256>a");
        assert_eq!(c.left, d.left);
    }

    /// Only a `$` at the very start marks phoneme letters; after a
    /// control string it is kana text with nothing to say.
    #[test]
    fn letters_need_the_dollar_first() {
        let mut quiet = Talk::default();
        quiet.say("");
        for text in ["<V16>$a", "<V16>$aaaaaaaaaaaaaaaaaaaa"] {
            let mut t = Talk::default();
            t.say(text);
            assert_eq!(t.left, quiet.left, "{text}");
        }
        let mut a = Talk::default();
        a.say("$a");
        assert!(a.left > quiet.left);
    }

    /// Pitches have whole periods at 16 kHz: A4 is 444.4 Hz, and the
    /// standard voice 100 Hz.
    #[test]
    fn pitches_have_whole_periods() {
        assert!((whole_period(440.0) - 16000.0 / 36.0).abs() < 1e-3);
        assert_eq!(whole_period(BASE_PITCH), 100.0);
        assert_eq!(whole_period(0.0), 0.0);
        // Sung notes cut the period down.
        assert!((sung_period(523.25) - 16000.0 / 30.0).abs() < 1e-3);
        assert!((sung_period(349.23) - 16000.0 / 45.0).abs() < 1e-3);
    }

    /// Frames while TALKCHK reads 1, a frame of 800 samples at a time.
    fn busy_frames(text: &str) -> usize {
        let mut talk = Talk::default();
        talk.say(text);
        let (mut l, mut r) = (vec![0.0; 800], vec![0.0; 800]);
        let mut n = 0;
        while talk.speaking() {
            talk.render(&mut l, &mut r);
            n += 1;
        }
        n
    }

    /// Nothing to say, or one mora, keeps TALKCHK set for one frame; 20
    /// vowels for 121.
    #[test]
    fn talkchk_counts_what_is_said() {
        assert_eq!(busy_frames("<V32>$a"), 1);
        assert_eq!(busy_frames("<V32>あ"), 1);
        assert_eq!(busy_frames("$<V32>aiueoaiueoaiueoaiueo"), 121);
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
    /// the vowel the way てぃ's ti and でぃ's di are.
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

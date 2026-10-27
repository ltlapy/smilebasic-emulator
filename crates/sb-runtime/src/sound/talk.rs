//! TALK: Japanese speech from kana or SmileBASIC's phoneme letters, on a
//! Klatt formant synthesizer (klattsch-core). Kana become phoneme letters,
//! the letters become morae, and each mora a few targets for the
//! synthesizer to glide between.
//!
//! None of this is SmileBASIC 4's own voice; it only says the same words.

use std::collections::HashMap;

use klattsch_core::{
    BankRegistry, FormantSynth, ParamUpdate, Params, PhonemeParams, PhonemeTable, Schedule,
    ScheduleEvent,
};

use super::RATE;

/// The bank with Japanese vowels (Mokhtari & Tanaka 2000) over Klatt's
/// English consonants.
const BANK: &str = "ja-mokhtari-2000";
const PHONEMES: [&str; 25] = [
    "A", "I", "U", "E", "O", "K", "G", "S", "SH", "Z", "JH", "T", "CH", "D", "N", "NG", "HH", "F",
    "B", "P", "M", "Y", "DX", "W", "_",
];

/// Milliseconds at the standard speed, `<S128>`.
const MORA: f64 = 130.0;
const CLOSURE: f64 = 45.0;
const BURST: f64 = 15.0;
const FRICATIVE: f64 = 70.0;
const SONORANT: f64 = 45.0;
const GLIDE: f64 = 40.0;
const SHORT_PAUSE: f64 = 220.0;
const LONG_PAUSE: f64 = 400.0;
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
    for c in text.chars() {
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

    fn vowel(&mut self, vowel: char, ms: f64, devoiced: bool, f0: f32) {
        let mut v = self.phoneme(&vowel.to_ascii_uppercase().to_string());
        v.f0 = Some(f0);
        if devoiced {
            v.voicing = Some(0.0);
            v.aspiration = Some(0.35);
        }
        self.target(v, TRANSITION);
        self.at += self.ms(ms);
    }

    /// The consonant's part of a mora; gives the milliseconds it took.
    fn consonant(&mut self, name: &'static str, f0: f32) -> f64 {
        match name {
            "K" | "G" | "T" | "D" | "P" | "B" | "CH" | "JH" | "TS" => {
                self.silence(CLOSURE);
                let stop = if name == "TS" { "T" } else { name };
                let mut burst = self.phoneme(stop);
                burst.f0 = Some(f0);
                if voiceless(Some(name)) {
                    burst.aspiration = Some(0.6);
                }
                self.target(burst, 2.0);
                self.at += self.ms(BURST);
                let mut taken = CLOSURE + BURST;
                if name == "TS" {
                    self.target(self.phoneme("S"), QUICK);
                    self.at += self.ms(FRICATIVE / 2.0);
                    taken += FRICATIVE / 2.0;
                }
                taken
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

pub struct Talk {
    table: HashMap<&'static str, PhonemeParams>,
    synth: FormantSynth,
    /// Samples left to play.
    left: u64,
    buffer: Vec<f32>,
}

impl Default for Talk {
    fn default() -> Self {
        let registry = BankRegistry::with_bundled();
        let bank = registry
            .resolve(BANK)
            .expect("klattsch-core bundles the Japanese bank");
        let table = PHONEMES
            .iter()
            .filter_map(|&n| bank.lookup(n).map(|p| (n, p)))
            .collect();
        Talk {
            table,
            synth: FormantSynth::new(RATE),
            left: 0,
            buffer: Vec::new(),
        }
    }
}

impl Talk {
    /// Starts saying `text`: kana, or phoneme letters after a `$`.
    pub fn say(&mut self, text: &str) {
        let letters = match text.strip_prefix('$') {
            Some(letters) => letters.to_string(),
            None => kana_letters(text),
        };
        let morae = morae(&letters);
        let mut plan = Planner {
            table: &self.table,
            events: Vec::new(),
            at: 0.0,
            speed: 1.0,
            pitch: BASE_PITCH,
            gain: BASE_GAIN,
        };
        let spoken = morae
            .iter()
            .filter(|m| !matches!(m, Mora::Control(..) | Mora::Pause(_)))
            .count()
            .max(1);
        let mut beat = 0;
        for (i, m) in morae.iter().enumerate() {
            let fall = beat as f32 / spoken as f32;
            let f0 = plan.pitch * (PITCH_START + (PITCH_END - PITCH_START) * fall);
            match *m {
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
                        .find(|m| !matches!(m, Mora::Control(..)));
                    let next_voiceless = match next {
                        Some(Mora::Syllable { consonant, .. }) => voiceless(*consonant),
                        None | Some(Mora::Pause(_)) => true,
                        _ => false,
                    };
                    let devoiced = silent
                        || (matches!(vowel, 'i' | 'u') && voiceless(consonant) && next_voiceless);
                    plan.vowel(vowel, (MORA - taken).max(50.0), devoiced, f0);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kana_become_letters() {
        assert_eq!(kana_letters("こんにちは"), "koqnichiha");
        assert_eq!(kana_letters("キャット"), "kyaclto");
        assert_eq!(kana_letters("しゃしん、ラーメン。"), "shashiq,raameq.");
        assert_eq!(kana_letters("ファイル"), "fairu");
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
}

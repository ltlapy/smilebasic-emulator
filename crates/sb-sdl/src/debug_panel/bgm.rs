//! The debugger panel's BGM tab: what each BGM track that plays or is
//! paused plays, channel by channel, and the BEEP handles that sound,
//! gathered only while the tab shows. SmileBASIC 4 has no such view; this
//! is the emulator's own.

use sb_runtime::font::Font;
use sb_runtime::framebuffer::Framebuffer;
use sb_runtime::sound::inspect::{BeepInfo, BeepKind, ChannelInfo, State, TrackInfo};
use sb_runtime::sound::{Sound, TRACKS};

use crate::ui::{DISABLED, Rect, TEXT, fill};

use super::{DebugPanel, HEADING, Hit, Metrics, NUMBER, STRING, STRIPE, TAB, cut};

/// MML's channels, which preset songs' sequences share.
const CHANNELS: usize = sb_runtime::sound::mml::CHANNELS;
/// The level bar's colour, and its trough's.
const LEVEL: u32 = 0xFF40_C060;
const TROUGH: u32 = 0xFF30_3030;
/// The lines of MML a track's tip shows at most.
const MML_LINES: usize = 6;
const TIP_CHARS: usize = 60;

/// What the tab shows, from the sound as the last frame left it.
#[derive(Clone, Debug, Default)]
pub struct Bgm {
    pub tracks: Vec<TrackInfo>,
    pub beeps: Vec<BeepInfo>,
}

/// Gathers what plays.
pub fn gather(sound: &Sound) -> Bgm {
    Bgm {
        tracks: sound.inspect_tracks(),
        beeps: sound.inspect_beeps(),
    }
}

/// One line of the tab, which scrolls by lines.
#[derive(Clone, Copy, Debug)]
enum Line<'a> {
    Track(&'a TrackInfo),
    /// $0 to $7, when the song has set any.
    Vars(&'a TrackInfo),
    Channel(&'a TrackInfo, &'a ChannelInfo),
    /// The headings over BEEP's handles.
    Beeps,
    Beep(&'a BeepInfo),
}

impl Bgm {
    fn lines(&self) -> Vec<Line<'_>> {
        let mut lines = Vec::new();
        for t in &self.tracks {
            lines.push(Line::Track(t));
            if t.vars.iter().any(|&v| v != -1) {
                lines.push(Line::Vars(t));
            }
            lines.extend(t.channels.iter().map(|c| Line::Channel(t, c)));
        }
        if !self.beeps.is_empty() {
            lines.push(Line::Beeps);
            lines.extend(self.beeps.iter().map(Line::Beep));
        }
        lines
    }

    /// How many lines there are to scroll through.
    pub fn len(&self) -> usize {
        self.lines().len()
    }

    fn track(&self, n: usize) -> Option<&TrackInfo> {
        self.tracks.iter().find(|t| t.track == n)
    }

    /// The right-click menu's copies for what `hit` is on: a label, and
    /// the text, if there is any.
    pub fn copies(&self, hit: Hit) -> Vec<(&'static str, Option<String>)> {
        let lines = |l: Vec<String>| Some(l.join("\n"));
        match hit {
            Hit::BgmTrack(n) => {
                let t = self.track(n);
                vec![
                    (
                        "Copy details",
                        t.and_then(|t| lines(track_details(t, true))),
                    ),
                    (
                        "Copy MML",
                        t.and_then(|t| Some(t.source.as_deref()?.to_string())),
                    ),
                ]
            }
            Hit::BgmChannel(n, c) => {
                let t = self.track(n);
                let ch = t.and_then(|t| t.channels.iter().find(|ch| ch.channel == c));
                let details = t.zip(ch).and_then(|(t, ch)| lines(channel_details(t, ch)));
                vec![("Copy details", details)]
            }
            Hit::Beep(h) => {
                let b = self.beeps.iter().find(|b| b.handle == h);
                vec![("Copy details", b.and_then(|b| lines(beep_details(b))))]
            }
            _ => {
                let mut all = Vec::new();
                for t in &self.tracks {
                    all.extend(track_details(t, false));
                    all.push(String::new());
                }
                all.extend(self.beeps.iter().flat_map(beep_details));
                let all = (!all.is_empty()).then(|| all.join("\n").trim_end().to_string());
                vec![("Copy everything", all)]
            }
        }
    }
}

/// The level bars: how loud each track's channels sounded as last drawn,
/// and how many notes they had started, to light up on a new one.
pub struct Meters {
    levels: [[f32; CHANNELS]; TRACKS],
    notes: [[u32; CHANNELS]; TRACKS],
}

impl Default for Meters {
    fn default() -> Self {
        Meters {
            levels: [[0.0; CHANNELS]; TRACKS],
            notes: [[0; CHANNELS]; TRACKS],
        }
    }
}

impl Meters {
    /// A frame on: a new note lights its channel up to its loudness, a
    /// held one fades down to part of it, and silence to nothing. Only
    /// the drawing moves them, so the sound pays nothing.
    pub(super) fn update(&mut self, bgm: &Bgm) {
        for t in &bgm.tracks {
            let gain = t.volume as f32 / 127.0 * t.fade as f32;
            for c in t.channels.iter().filter(|c| c.channel < CHANNELS) {
                let (level, notes) = (
                    &mut self.levels[t.track][c.channel],
                    &mut self.notes[t.track][c.channel],
                );
                let loud = (c.volume * c.velocity) as f32 / (127.0 * 127.0) * gain;
                *level = if c.notes != *notes && !c.keys.is_empty() {
                    loud
                } else if !c.keys.is_empty() && t.state != State::Paused {
                    (*level * 0.92).max(loud * 0.6)
                } else {
                    *level * 0.8
                };
                *notes = c.notes;
            }
        }
    }

    fn level(&self, track: usize, channel: usize) -> f32 {
        (self.levels.get(track))
            .and_then(|l| l.get(channel))
            .copied()
            .unwrap_or(0.0)
    }
}

/// A MIDI key as a note: O4C, key 60, is C4.
pub fn note_name(key: i32) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[key.rem_euclid(12) as usize],
        key.div_euclid(12) - 1
    )
}

fn notes_text(keys: &[i32]) -> String {
    match keys.is_empty() {
        true => "-".to_string(),
        false => keys
            .iter()
            .map(|&k| note_name(k))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// General MIDI's names, which @0 to @127 follow: the manual calls them
/// a GM sound source's.
const GM: [&str; 128] = [
    "Acoustic Grand Piano",
    "Bright Acoustic Piano",
    "Electric Grand Piano",
    "Honky-tonk Piano",
    "Electric Piano 1",
    "Electric Piano 2",
    "Harpsichord",
    "Clavinet",
    "Celesta",
    "Glockenspiel",
    "Music Box",
    "Vibraphone",
    "Marimba",
    "Xylophone",
    "Tubular Bells",
    "Dulcimer",
    "Drawbar Organ",
    "Percussive Organ",
    "Rock Organ",
    "Church Organ",
    "Reed Organ",
    "Accordion",
    "Harmonica",
    "Tango Accordion",
    "Nylon Guitar",
    "Steel Guitar",
    "Jazz Guitar",
    "Clean Guitar",
    "Muted Guitar",
    "Overdriven Guitar",
    "Distortion Guitar",
    "Guitar Harmonics",
    "Acoustic Bass",
    "Finger Bass",
    "Pick Bass",
    "Fretless Bass",
    "Slap Bass 1",
    "Slap Bass 2",
    "Synth Bass 1",
    "Synth Bass 2",
    "Violin",
    "Viola",
    "Cello",
    "Contrabass",
    "Tremolo Strings",
    "Pizzicato Strings",
    "Orchestral Harp",
    "Timpani",
    "String Ensemble 1",
    "String Ensemble 2",
    "Synth Strings 1",
    "Synth Strings 2",
    "Choir Aahs",
    "Voice Oohs",
    "Synth Voice",
    "Orchestra Hit",
    "Trumpet",
    "Trombone",
    "Tuba",
    "Muted Trumpet",
    "French Horn",
    "Brass Section",
    "Synth Brass 1",
    "Synth Brass 2",
    "Soprano Sax",
    "Alto Sax",
    "Tenor Sax",
    "Baritone Sax",
    "Oboe",
    "English Horn",
    "Bassoon",
    "Clarinet",
    "Piccolo",
    "Flute",
    "Recorder",
    "Pan Flute",
    "Blown Bottle",
    "Shakuhachi",
    "Whistle",
    "Ocarina",
    "Square Lead",
    "Sawtooth Lead",
    "Calliope Lead",
    "Chiff Lead",
    "Charang Lead",
    "Voice Lead",
    "Fifths Lead",
    "Bass + Lead",
    "New Age Pad",
    "Warm Pad",
    "Polysynth Pad",
    "Choir Pad",
    "Bowed Pad",
    "Metallic Pad",
    "Halo Pad",
    "Sweep Pad",
    "Rain",
    "Soundtrack",
    "Crystal",
    "Atmosphere",
    "Brightness",
    "Goblins",
    "Echoes",
    "Sci-fi",
    "Sitar",
    "Banjo",
    "Shamisen",
    "Koto",
    "Kalimba",
    "Bagpipe",
    "Fiddle",
    "Shanai",
    "Tinkle Bell",
    "Agogo",
    "Steel Drums",
    "Woodblock",
    "Taiko Drum",
    "Melodic Tom",
    "Synth Drum",
    "Reverse Cymbal",
    "Guitar Fret Noise",
    "Breath Noise",
    "Seashore",
    "Bird Tweet",
    "Telephone Ring",
    "Helicopter",
    "Applause",
    "Gunshot",
];

/// The drum sets @128 to @148, as the manual names them.
const DRUMS: [&str; 21] = [
    "Standard1",
    "Electric1",
    "PSG",
    "Standard2",
    "Standard3",
    "Room",
    "HipHop",
    "Jungle",
    "Techno",
    "House",
    "Power",
    "Electric2",
    "BOB",
    "Dance",
    "QOQ",
    "Jazz",
    "Brush",
    "Orchestra",
    "Ethnic",
    "Asia",
    "Unn",
];

/// What @`program` is, as the manual lists them.
pub fn instrument_name(program: i32) -> String {
    match program {
        0..=127 => GM[program as usize].to_string(),
        128..=148 => format!("{} Drum Set", DRUMS[(program - 128) as usize]),
        216..=222 => format!("PSG {}", program - 216),
        223 => "Noise".to_string(),
        224..=255 => format!("WAVSET {program}"),
        256..=432 => "Sound effect".to_string(),
        _ => "(none)".to_string(),
    }
}

fn state_text(t: &TrackInfo) -> String {
    let percent = (t.fade * 100.0).round() as i32;
    match t.state {
        State::Waking => "Starting".to_string(),
        State::Playing => "Playing".to_string(),
        State::FadingIn => format!("Fading in {percent}%"),
        State::FadingOut => format!("Fading out {percent}%"),
        State::Pausing => format!("Pausing {percent}%"),
        State::Paused => "Paused".to_string(),
    }
}

fn song_text(t: &TrackInfo) -> String {
    match (t.source.is_some(), t.silent) {
        (true, _) => format!("SONG {}", t.song),
        (false, true) => format!("PRESET {} (silent)", t.song),
        (false, false) => format!("PRESET {}", t.song),
    }
}

/// Quarter notes and ticks played, as `12:30`.
fn position(t: &TrackInfo) -> String {
    let q = u64::from(t.quarter.max(1));
    format!("{}:{:02}", t.ticks / q, t.ticks % q)
}

/// A track's line: how and what it plays.
fn track_line(t: &TrackInfo) -> String {
    let mut line = format!(
        "TRACK {}  {}  {}  VOL {}  T{}  POS {}",
        t.track,
        state_text(t),
        song_text(t),
        t.volume,
        t.tempo,
        position(t)
    );
    if t.pitch != 0 {
        line.push_str(&format!("  PITCH {}", t.pitch));
    }
    line
}

fn vars_text(t: &TrackInfo) -> String {
    let vars: Vec<String> = (t.vars.iter().enumerate())
        .map(|(i, v)| format!("${i}={v}"))
        .collect();
    vars.join(" ")
}

/// Text broken into lines of [`TIP_CHARS`], at most `most` of them.
fn wrapped(text: &str, most: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
    let mut lines: Vec<String> = (chars.chunks(TIP_CHARS))
        .map(|c| c.iter().collect())
        .collect();
    if lines.len() > most {
        let more = lines.len() - (most - 1);
        lines.truncate(most - 1);
        lines.push(format!("... {more} more lines"));
    }
    lines
}

/// Everything about a track, for its tip and for copying; the copy has
/// its MML whole and its channels.
pub fn track_details(t: &TrackInfo, whole: bool) -> Vec<String> {
    let mut lines = vec![format!("TRACK {}  {}", t.track, song_text(t))];
    lines.push(format!(
        "{}  BGMVOL {}  BGMPITCH {}",
        state_text(t),
        t.volume,
        t.pitch
    ));
    lines.push(format!(
        "TEMPO {}  POS {} (tick {}, {} a quarter note)",
        t.tempo,
        position(t),
        t.ticks,
        t.quarter
    ));
    lines.push(vars_text(t));
    if !whole {
        lines.push(format!("{} channels", t.channels.len()));
    }
    if let Some(mml) = &t.source {
        lines.push("MML:".to_string());
        match whole {
            true => lines.push(mml.to_string()),
            false => lines.extend(wrapped(mml, MML_LINES)),
        }
    }
    if whole {
        for c in &t.channels {
            lines.push(channel_line(c));
        }
    }
    lines
}

fn channel_line(c: &ChannelInfo) -> String {
    let octave = c.octave.map_or(String::new(), |o| format!("  O{o}"));
    let ended = if c.ended { "  (ended)" } else { "" };
    format!(
        "CH {}  @{} {}{octave}  V{}  @V{}  P{}  NOTE {}{ended}",
        c.channel,
        c.program,
        instrument_name(c.program),
        c.volume,
        c.velocity,
        c.pan,
        notes_text(&c.keys)
    )
}

/// Everything about a channel.
pub fn channel_details(t: &TrackInfo, c: &ChannelInfo) -> Vec<String> {
    let mut lines = vec![format!("TRACK {} CHANNEL {}", t.track, c.channel)];
    lines.push(format!("@{} {}", c.program, instrument_name(c.program)));
    let octave = c.octave.map_or(String::new(), |o| format!("O{o}  "));
    lines.push(format!(
        "{octave}V{}  @V{}  P{}",
        c.volume, c.velocity, c.pan
    ));
    lines.push(format!("NOTE {}", notes_text(&c.keys)));
    lines.push(format!("{} notes started", c.notes));
    if c.ended {
        lines.push("Ended".to_string());
    }
    lines
}

fn beep_kind(b: &BeepInfo) -> String {
    match b.kind {
        BeepKind::Silent => "silent".to_string(),
        BeepKind::Effect => "effect".to_string(),
        BeepKind::Instrument(p) => format!("@{p} {}", instrument_name(p)),
        BeepKind::Wave => "WAVSET".to_string(),
    }
}

pub fn beep_details(b: &BeepInfo) -> Vec<String> {
    let stopping = if b.stopping { "  (stopping)" } else { "" };
    vec![
        format!("BEEP {}  handle {}{stopping}", b.sound, b.handle),
        beep_kind(b),
        format!("PITCH {}  VOL {}  PAN {}", b.pitch, b.volume, b.pan),
    ]
}

/// The channels' columns: heading, characters taken, and whether it can
/// be left out for want of room. The level is a bar, and the instrument's
/// name takes what is left.
const COLUMNS: [(&str, usize, bool); 9] = [
    ("CH", 3, false),
    ("@", 4, false),
    ("NOTE", 9, false),
    ("LEVEL", 7, true),
    ("V", 4, true),
    ("P", 4, true),
    ("O", 3, true),
    ("@V", 4, true),
    ("INSTRUMENT", 10, true),
];
const BEEP_COLUMNS: [(&str, usize); 6] = [
    ("H", 3),
    ("BEEP", 5),
    ("PIT", 7),
    ("VOL", 4),
    ("PAN", 4),
    ("SOUND", 0),
];

/// The columns that fit in `room` characters.
fn fitting(room: usize) -> usize {
    let mut used = 0;
    let mut n = 0;
    for (_, chars, optional) in COLUMNS {
        if optional && used + chars > room {
            break;
        }
        used += chars;
        n += 1;
    }
    n
}

fn channel_cells(c: &ChannelInfo) -> [String; 9] {
    [
        c.channel.to_string(),
        c.program.to_string(),
        notes_text(&c.keys),
        String::new(),
        c.volume.to_string(),
        c.pan.to_string(),
        c.octave.map_or("-".to_string(), |o| o.to_string()),
        c.velocity.to_string(),
        instrument_name(c.program),
    ]
}

fn beep_cells(b: &BeepInfo) -> [String; 6] {
    [
        b.handle.to_string(),
        b.sound.to_string(),
        b.pitch.to_string(),
        b.volume.to_string(),
        b.pan.to_string(),
        beep_kind(b),
    ]
}

impl DebugPanel {
    /// What plays from `(x, y)` to `(right, bottom)`: under the channels'
    /// headings, a line for each track and one for each channel its song
    /// uses, with a bar for how loud it sounds, then BEEP's handles.
    /// Channels that have ended are greyed; what does not fit is in the
    /// rows' tips.
    pub(super) fn draw_bgm(
        &mut self,
        fb: &mut Framebuffer,
        font: &Font,
        m: &Metrics,
        (x, mut y): (u32, u32),
        (right, bottom): (u32, u32),
        bgm: &Bgm,
    ) {
        let area = Rect {
            x: x - m.pad / 2,
            y,
            w: (right + m.pad / 2).saturating_sub(x),
            h: bottom.saturating_sub(y),
        };
        if bgm.tracks.is_empty() && bgm.beeps.is_empty() {
            m.draw(fb, font, x, y + m.text, "No BGM or BEEP sounds", DISABLED);
            self.hits.push((area, Hit::Bgm));
            return;
        }
        let room = (right.saturating_sub(x) / m.char) as usize;
        let shown = fitting(room);
        let mut columns = Vec::with_capacity(shown);
        let mut at = 0;
        for &(heading, chars, _) in &COLUMNS[..shown] {
            columns.push((x + at as u32 * m.char, heading, chars));
            at += chars;
        }
        // The instrument's name, last, takes the rest.
        let before: usize = COLUMNS[..shown.min(8)].iter().map(|c| c.1).sum();
        let name_chars = room.saturating_sub(before).max(1);
        if y + m.line <= bottom {
            for &(cx, heading, _) in &columns {
                m.draw(fb, font, cx, y + m.text, heading, HEADING);
            }
            y += m.line;
        }
        let lines = bgm.lines();
        for (n, line) in lines.iter().enumerate().skip(self.scroll) {
            if y + m.line > bottom {
                break;
            }
            let r = Rect {
                y,
                h: m.line,
                ..area
            };
            let ty = y + m.text;
            y += m.line;
            match *line {
                Line::Track(t) => {
                    fill(fb, r, TAB);
                    m.draw(fb, font, x, ty, &cut(&track_line(t), room), TEXT);
                    self.tips.push((r, track_details(t, false)));
                    self.hits.push((r, Hit::BgmTrack(t.track)));
                }
                Line::Vars(t) => {
                    m.draw(fb, font, x, ty, &cut(&vars_text(t), room), NUMBER);
                    self.tips.push((r, track_details(t, false)));
                    self.hits.push((r, Hit::BgmTrack(t.track)));
                }
                Line::Channel(t, c) => {
                    if n % 2 == 1 {
                        fill(fb, r, STRIPE);
                    }
                    let cells = channel_cells(c);
                    for (i, &(cx, heading, chars)) in columns.iter().enumerate() {
                        if heading == "LEVEL" {
                            let level = self.meters.level(t.track, c.channel);
                            draw_level(fb, m, (cx, ty), chars - 1, level);
                            continue;
                        }
                        let chars = if i == 8 { name_chars } else { chars - 1 };
                        let color = match (c.ended, i) {
                            (true, _) => DISABLED,
                            (false, 2) if !c.keys.is_empty() => STRING,
                            (false, 2) => DISABLED,
                            (false, 1 | 4..=7) => NUMBER,
                            _ => TEXT,
                        };
                        m.draw(fb, font, cx, ty, &cut(&cells[i], chars), color);
                    }
                    self.tips.push((r, channel_details(t, c)));
                    self.hits.push((r, Hit::BgmChannel(t.track, c.channel)));
                }
                Line::Beeps => {
                    let mut at = 0;
                    for (heading, chars) in BEEP_COLUMNS {
                        let text = cut(heading, room.saturating_sub(at));
                        m.draw(fb, font, x + at as u32 * m.char, ty, &text, HEADING);
                        at += chars;
                    }
                }
                Line::Beep(b) => {
                    if n % 2 == 1 {
                        fill(fb, r, STRIPE);
                    }
                    let mut cx = x;
                    let mut left = room;
                    for (i, (cell, (_, chars))) in
                        beep_cells(b).iter().zip(BEEP_COLUMNS).enumerate()
                    {
                        let chars = if chars == 0 { left } else { chars.min(left) };
                        let color = match (b.stopping, i) {
                            (true, _) => DISABLED,
                            (false, 1..=4) => NUMBER,
                            _ => TEXT,
                        };
                        m.draw(fb, font, cx, ty, &cut(cell, chars.saturating_sub(1)), color);
                        cx += chars as u32 * m.char;
                        left -= chars;
                    }
                    self.tips.push((r, beep_details(b)));
                    self.hits.push((r, Hit::Beep(b.handle)));
                }
            }
        }
        // After the rows, which go first.
        self.hits.push((area, Hit::Bgm));
    }
}

/// A bar `chars` characters long at `(x, y)`, lit for `level`, 0 to 1.
fn draw_level(fb: &mut Framebuffer, m: &Metrics, (x, y): (u32, u32), chars: usize, level: f32) {
    let w = chars as u32 * m.char;
    let h = m.line - 2 * m.text;
    fill(fb, Rect { x, y, w, h }, TROUGH);
    let lit = (level.clamp(0.0, 1.0) * w as f32).round() as u32;
    fill(fb, Rect { x, y, w: lit, h }, LEVEL);
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_runtime::Runtime;
    use sb_runtime::font::Font;

    fn run(rt: &mut Runtime, source: &str) {
        use sb_core::vm::{Status, Vm};
        let mut vm = Vm::new(sb_core::compile(source, &*rt).unwrap());
        while vm.run(rt, 1_000_000).unwrap() != Status::Ended {}
    }

    #[test]
    fn notes_and_instruments_by_name() {
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(73), "C#5");
        assert_eq!(note_name(0), "C-1");
        assert_eq!(notes_text(&[60, 64]), "C4 E4");
        assert_eq!(notes_text(&[]), "-");
        assert_eq!(instrument_name(0), "Acoustic Grand Piano");
        assert_eq!(instrument_name(127), "Gunshot");
        assert_eq!(instrument_name(130), "PSG Drum Set");
        assert_eq!(instrument_name(148), "Unn Drum Set");
        assert_eq!(instrument_name(223), "Noise");
        assert_eq!(instrument_name(200), "(none)");
    }

    #[test]
    fn lines_and_copies_of_what_plays() {
        let mut rt = Runtime::new(400, 240, Font::blank());
        run(
            &mut rt,
            "BGMSET 140,\"T150$1=3@16O3CDE:2V60[G]\":BGMPLAY 2,140",
        );
        for _ in 0..4 {
            rt.sound.render_frame();
        }
        run(&mut rt, "BEEP 260,0,80");
        let bgm = gather(&rt.sound);
        // The track, its variables, its two channels, BEEP's headings and
        // its handle.
        assert_eq!(bgm.len(), 6);
        assert_eq!(bgm.lines().len(), 6);
        let t = &bgm.tracks[0];
        assert!(track_line(t).starts_with("TRACK 2  Playing  SONG 140  VOL 127  T150  POS "));
        assert_eq!(
            vars_text(t),
            "$0=-1 $1=3 $2=-1 $3=-1 $4=-1 $5=-1 $6=-1 $7=-1"
        );
        let cells = channel_cells(&t.channels[0]);
        assert_eq!(
            [&cells[0], &cells[1], &cells[2], &cells[6], &cells[8]],
            ["0", "16", "C3", "3", "Drawbar Organ"]
        );
        assert_eq!(t.channels[1].volume, 60);
        let copies = bgm.copies(Hit::BgmTrack(2));
        let mml = "T150$1=3@16O3CDE:2V60[G]";
        assert_eq!(copies[1], ("Copy MML", Some(mml.to_string())));
        let details = copies[0].1.clone().unwrap();
        assert!(details.contains(mml) && details.contains("CH 2  @0 Acoustic Grand Piano"));
        let beep = bgm.copies(Hit::Beep(0))[0].1.clone().unwrap();
        assert_eq!(
            beep,
            "BEEP 260  handle 0\n@4 Electric Piano 1\nPITCH 0  VOL 80  PAN 64"
        );
        assert_eq!(bgm.copies(Hit::BgmTrack(5))[0].1, None);
        let all = bgm.copies(Hit::Bgm)[0].1.clone().unwrap();
        assert!(all.starts_with("TRACK 2") && all.ends_with("PAN 64"));
    }

    #[test]
    fn the_tab_draws_a_line_for_each_track_channel_and_beep() {
        use super::super::{PerfStats, Probe, Tab, View};
        let mut rt = Runtime::new(400, 240, Font::blank());
        run(
            &mut rt,
            "BGMPLAY 1,128:BGMSET 130,\"CDE:3G\":BGMPLAY 4,130:BEEP 300",
        );
        rt.sound.render_frame();
        let mut p = DebugPanel {
            tab: Some(Tab::Bgm),
            ..DebugPanel::default()
        };
        let view = View {
            calls: Vec::new(),
            tabs: vec![Tab::Globals(0), Tab::Perf, Tab::Sprites, Tab::Grp, Tab::Bgm],
            variables: Vec::new(),
            frame: None,
            slot: 0,
            watches: Vec::new(),
            status: String::new(),
            status_failed: false,
            keys: String::new(),
            changed: None,
            breakpoints: Vec::new(),
            perf: [PerfStats::default(); 8],
            sprites: None,
            grp: Probe::default(),
            bgm: Some(gather(&rt.sound)),
        };
        // BGMPLAY 128 had no song set, so only track 4 plays.
        assert_eq!(view.scrolled(), 5);
        let mut fb = Framebuffer::new(1280, 300);
        let f = Rect {
            x: 0,
            y: 0,
            w: 1280,
            h: 300,
        };
        p.draw(&mut fb, &Font::blank(), f, Some((400, 240)), &view, 1);
        let hits: Vec<Hit> = (p.hits.iter())
            .map(|&(_, h)| h)
            .filter(|h| {
                matches!(
                    h,
                    Hit::BgmTrack(_) | Hit::BgmChannel(..) | Hit::Beep(_) | Hit::Bgm
                )
            })
            .collect();
        assert_eq!(
            hits,
            [
                Hit::BgmTrack(4),
                Hit::BgmChannel(4, 0),
                Hit::BgmChannel(4, 3),
                Hit::Beep(0),
                Hit::Bgm
            ]
        );
        // Every line has its tip; a new stop keeps the tab.
        assert_eq!(p.tips.len(), 4);
        p.reset();
        assert_eq!(p.tab, Some(Tab::Bgm));
        // In a short panel, scrolled, the first lines go.
        let f = Rect { h: 80, ..f };
        p.draw(&mut fb, &Font::blank(), f, Some((400, 240)), &view, 1);
        p.wheel(2, view.scrolled());
        p.draw(&mut fb, &Font::blank(), f, Some((400, 240)), &view, 1);
        assert!(p.hits.iter().any(|&(_, h)| h == Hit::Beep(0)));
        assert!(!p.hits.iter().any(|&(_, h)| h == Hit::BgmTrack(4)));
    }

    #[test]
    fn a_new_note_lights_its_channel_up() {
        let mut rt = Runtime::new(400, 240, Font::blank());
        run(&mut rt, "BGMPLAY \"T60V127@V127C4R1\"");
        let mut meters = Meters::default();
        // While the sound system wakes up, nothing sounds.
        rt.sound.render_frame();
        meters.update(&gather(&rt.sound));
        assert_eq!(meters.level(0, 0), 0.0);
        for _ in 0..2 {
            rt.sound.render_frame();
        }
        meters.update(&gather(&rt.sound));
        assert_eq!(meters.level(0, 0), 1.0);
        // Held, it fades to part of it; let go, to nothing.
        for _ in 0..30 {
            meters.update(&gather(&rt.sound));
        }
        let held = meters.level(0, 0);
        assert!((0.55..0.65).contains(&held), "{held}");
        for _ in 0..70 {
            rt.sound.render_frame();
        }
        for _ in 0..30 {
            meters.update(&gather(&rt.sound));
        }
        assert!(meters.level(0, 0) < 0.01);
    }
}

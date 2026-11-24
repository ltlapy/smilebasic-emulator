//! SmileBASIC 4's preset songs and sound effects, which are sequences: up
//! to 16 tracks of notes and commands for the instruments, in Nintendo's
//! sequence code. The importer copies them out of the sound archive into
//! a file in the emulator's own format:
//!
//! ```text
//! "SBSEQ001"
//! u32 sequences, then per sequence:
//!   u8 kind (0 a song, 1 a BEEP), u16 number, u8 volume,
//!   u32 where track 0 starts, u32 code bytes, the code
//! ```
//!
//! Numbers are little-endian.

use std::sync::Arc;

use super::inspect::ChannelInfo;
use super::synth::{CHANNELS, Synth};

const MAGIC: &[u8; 8] = b"SBSEQ001";
/// Commands a track may run on one tick before it is taken to be stuck.
const COMMANDS_PER_TICK: u32 = 10_000;
/// Calls nested deeper than this end the track.
const CALL_DEPTH: usize = 8;
/// Commands run and sounding notes counted down in one block of sound (a
/// frame) before the whole sequence is taken to be stuck. Tracks can start
/// each other again forever, so that `COMMANDS_PER_TICK` never ends them,
/// and a block has as many ticks as samples at the highest tempo. Mixing
/// shares the program's thread, so this must not take long. The busiest
/// block of all 359 of SmileBASIC 4's sequences needs 195.
// Ours, for a sequence built to hang the program.
const BLOCK_WORK: u32 = 20_000;
/// The same for a whole recording, which no one is waiting to hear.
const RECORD_WORK: u32 = 1 << 28;
/// Events a recording keeps at most.
const RECORD_EVENTS: usize = 1 << 22;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sequence {
    /// The volume of the whole sound; 127 leaves it unchanged.
    pub volume: u8,
    pub start: usize,
    pub code: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Song,
    Beep,
}

/// The preset songs (BGMPLAY 0 up) and sound effects (BEEP 0 up).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sequences {
    pub songs: Vec<Option<Arc<Sequence>>>,
    pub beeps: Vec<Option<Arc<Sequence>>>,
}

impl Sequences {
    pub fn get(&self, kind: Kind, n: i32) -> Option<Arc<Sequence>> {
        let list = match kind {
            Kind::Song => &self.songs,
            Kind::Beep => &self.beeps,
        };
        list.get(usize::try_from(n).ok()?)?.clone()
    }

    pub fn insert(&mut self, kind: Kind, n: usize, seq: Sequence) {
        let list = match kind {
            Kind::Song => &mut self.songs,
            Kind::Beep => &mut self.beeps,
        };
        if list.len() <= n {
            list.resize(n + 1, None);
        }
        list[n] = Some(Arc::new(seq));
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        let all: Vec<(u8, usize, &Sequence)> = [(0u8, &self.songs), (1, &self.beeps)]
            .into_iter()
            .flat_map(|(kind, list)| {
                list.iter()
                    .enumerate()
                    .filter_map(move |(n, s)| Some((kind, n, s.as_deref()?)))
            })
            .collect();
        out.extend_from_slice(&(all.len() as u32).to_le_bytes());
        for (kind, n, s) in all {
            out.push(kind);
            out.extend_from_slice(&(n as u16).to_le_bytes());
            out.push(s.volume);
            out.extend_from_slice(&(s.start as u32).to_le_bytes());
            out.extend_from_slice(&(s.code.len() as u32).to_le_bytes());
            out.extend_from_slice(&s.code);
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Sequences> {
        if bytes.get(..MAGIC.len())? != MAGIC {
            return None;
        }
        let mut at = MAGIC.len();
        let mut take = |n: usize| {
            let b = bytes.get(at..at.checked_add(n)?)?;
            at += n;
            Some(b)
        };
        let u32_of = |b: &[u8]| u32::from_le_bytes(b.try_into().expect("four bytes")) as usize;
        let mut out = Sequences::default();
        for _ in 0..u32_of(take(4)?) {
            let head = take(12)?;
            let kind = match head[0] {
                0 => Kind::Song,
                1 => Kind::Beep,
                _ => return None,
            };
            let n = usize::from(u16::from_le_bytes([head[1], head[2]]));
            let start = u32_of(&head[4..8]);
            let code = take(u32_of(&head[8..12]))?.to_vec();
            if start >= code.len() {
                return None;
            }
            let volume = head[3];
            out.insert(
                kind,
                n,
                Sequence {
                    volume,
                    start,
                    code,
                },
            );
        }
        Some(out)
    }
}

#[derive(Clone, Debug)]
struct SeqTrack {
    pc: usize,
    wait: u32,
    calls: Vec<usize>,
    /// Notes wait for their length before the next command, unless turned
    /// off.
    note_wait: bool,
    /// Notes sounding, with the ticks they have left.
    notes: Vec<(i32, u32)>,
    /// The pitch bend, -128 to 127 across the bend range in semitones.
    bend: i32,
    bend_range: i32,
    ended: bool,
    /// The track has jumped back: it loops from here on.
    looped: bool,
}

impl SeqTrack {
    fn new(pc: usize) -> SeqTrack {
        SeqTrack {
            pc,
            wait: 0,
            calls: Vec::new(),
            note_wait: true,
            notes: Vec::new(),
            bend: 0,
            bend_range: 2,
            ended: false,
            looped: false,
        }
    }
}

/// The values a sequence last set on a channel, kept only for the
/// debugger: the synthesizer has them already.
#[derive(Clone, Copy, Debug)]
struct Settings {
    program: i32,
    volume: i32,
    expression: i32,
    pan: i32,
    velocity: i32,
    notes: u32,
}

impl Default for Settings {
    /// The MIDI defaults that a synthesizer starts with.
    fn default() -> Self {
        Settings {
            program: 0,
            volume: 100,
            expression: 127,
            pan: 64,
            velocity: 0,
            notes: 0,
        }
    }
}

/// One sequence playing on a synthesizer, track n on channel n.
#[derive(Clone, Debug)]
pub(crate) struct Player {
    seq: Arc<Sequence>,
    tracks: [Option<SeqTrack>; CHANNELS],
    settings: [Settings; CHANNELS],
    /// Quarter notes per minute.
    pub(crate) tempo: u32,
    /// Ticks in a quarter note.
    pub(crate) timebase: u32,
    /// BGMPITCH or BEEPPIT, in cents.
    pitch: i32,
    /// The part of `BLOCK_WORK` left in this block.
    work: u32,
}

impl Player {
    pub(crate) fn new(seq: Arc<Sequence>) -> Player {
        let mut tracks: [Option<SeqTrack>; CHANNELS] = Default::default();
        tracks[0] = Some(SeqTrack::new(seq.start));
        Player {
            seq,
            tracks,
            settings: Default::default(),
            tempo: 120,
            timebase: 48,
            pitch: 0,
            work: BLOCK_WORK,
        }
    }

    /// A new block of sound starts: see [`BLOCK_WORK`].
    pub(crate) fn begin_block(&mut self) {
        self.work = BLOCK_WORK;
    }

    /// Stops everything, for a sequence that has used up its work.
    fn give_up(&mut self, synth: &mut dyn Synth) {
        for (c, track) in self.tracks.iter_mut().enumerate() {
            let Some(t) = track else { continue };
            t.ended = true;
            for (key, _) in t.notes.drain(..) {
                synth.note_off(c, key);
            }
        }
    }

    /// The channels with a track, for the debugger.
    pub(crate) fn channels(&self) -> Vec<ChannelInfo> {
        (self.tracks.iter().enumerate())
            .filter_map(|(c, t)| Some((c, t.as_ref()?)))
            .map(|(c, t)| {
                let s = &self.settings[c];
                ChannelInfo {
                    channel: c,
                    program: s.program,
                    octave: None,
                    // CC7 and CC11 both scale the channel.
                    volume: s.volume * s.expression / 127,
                    velocity: s.velocity,
                    pan: s.pan,
                    keys: t.notes.iter().map(|&(key, _)| key).collect(),
                    notes: s.notes,
                    ended: t.ended,
                }
            })
            .collect()
    }

    pub(crate) fn volume(&self) -> f32 {
        f32::from(self.seq.volume) / 127.0
    }

    /// The number of samples a tick lasts at `rate` samples per second.
    pub(crate) fn tick_samples(&self, rate: u32) -> f64 {
        f64::from(rate) * 60.0 / f64::from(self.tempo.max(1) * self.timebase.max(1))
    }

    /// Whether every track has ended and every note stopped.
    pub(crate) fn ended(&self) -> bool {
        self.tracks
            .iter()
            .flatten()
            .all(|t| t.ended && t.notes.is_empty())
    }

    fn apply_bend(&self, c: usize, synth: &mut dyn Synth) {
        if let Some(t) = &self.tracks[c] {
            synth.bend(c, t.bend * t.bend_range * 100 / 128 + self.pitch);
        }
    }

    pub(crate) fn set_pitch(&mut self, cents: i32, synth: &mut dyn Synth) {
        self.pitch = cents;
        for c in 0..CHANNELS {
            self.apply_bend(c, synth);
        }
    }

    /// Plays one tick: notes and waits count down, notes whose length is
    /// up stop, and then the commands due run. A note started on a tick
    /// sounds through it.
    pub(crate) fn tick(&mut self, synth: &mut dyn Synth) {
        for (c, track) in self.tracks.iter_mut().enumerate() {
            let Some(t) = track else { continue };
            self.work = self.work.saturating_sub(t.notes.len() as u32);
            t.notes.retain_mut(|(key, left)| {
                *left = left.saturating_sub(1);
                if *left == 0 {
                    synth.note_off(c, *key);
                }
                *left > 0
            });
            t.wait = t.wait.saturating_sub(1);
        }
        for c in 0..CHANNELS {
            if self.tracks[c].is_some() {
                self.run(c, synth);
            }
        }
    }

    /// Runs track `c`'s commands until one takes time.
    fn run(&mut self, c: usize, synth: &mut dyn Synth) {
        let seq = self.seq.clone();
        let code = &seq.code;
        let mut budget = COMMANDS_PER_TICK;
        loop {
            let Some(t) = self.tracks[c].as_mut() else {
                return;
            };
            if t.ended || t.wait > 0 {
                return;
            }
            if self.work == 0 {
                self.give_up(synth);
                return;
            }
            self.work -= 1;
            budget -= 1;
            let mut r = CodeReader { code, at: t.pc };
            let Some(op) = r.u8() else {
                t.ended = true;
                return;
            };
            match self.command(c, op, &mut r, synth) {
                Some(()) if budget > 0 => {}
                // Damaged code, or a loop that never waits, ends the track.
                _ => {
                    if let Some(t) = self.tracks[c].as_mut() {
                        t.ended = true;
                    }
                    return;
                }
            }
        }
    }

    /// Runs one command, leaving the track after it; `None` ends the track.
    fn command(
        &mut self,
        c: usize,
        op: u8,
        r: &mut CodeReader,
        synth: &mut dyn Synth,
    ) -> Option<()> {
        let mut next_pc = None;
        match op {
            0x00..=0x7F => {
                let velocity = i32::from(r.u8()?);
                let len = r.varlen()?;
                let key = i32::from(op);
                synth.note_on(c, key, velocity);
                let s = &mut self.settings[c];
                s.velocity = velocity;
                s.notes = s.notes.wrapping_add(1);
                let t = self.tracks[c].as_mut()?;
                if len > 0 {
                    t.notes.push((key, len));
                }
                if t.note_wait {
                    t.wait = len;
                }
            }
            0x80 => self.tracks[c].as_mut()?.wait = r.varlen()?,
            0x81 => {
                let program = i32::try_from(r.varlen()?).ok()?;
                self.settings[c].program = program;
                synth.program(c, program);
            }
            0x88 => {
                let track = usize::from(r.u8()?);
                let at = r.u24()?;
                if track < CHANNELS && track != c {
                    self.tracks[track] = Some(SeqTrack::new(at));
                }
            }
            0x89 => {
                let to = r.u24()?;
                if to <= self.tracks[c].as_ref()?.pc {
                    self.tracks[c].as_mut()?.looped = true;
                }
                next_pc = Some(to);
            }
            0x8A => {
                let to = r.u24()?;
                let t = self.tracks[c].as_mut()?;
                if t.calls.len() >= CALL_DEPTH {
                    return None;
                }
                t.calls.push(r.at);
                next_pc = Some(to);
            }
            0xFD => next_pc = Some(self.tracks[c].as_mut()?.calls.pop()?),
            0xFF => return None,
            0xB0 => self.timebase = u32::from(r.u8()?).max(1),
            0xC0 => {
                let pan = i32::from(r.u8()?);
                self.settings[c].pan = pan;
                synth.pan(c, pan);
            }
            0xC1 => {
                let volume = i32::from(r.u8()?);
                self.settings[c].volume = volume;
                synth.volume(c, volume);
            }
            0xC4 => {
                self.tracks[c].as_mut()?.bend = i32::from(r.u8()? as i8);
                self.apply_bend(c, synth);
            }
            0xC5 => {
                self.tracks[c].as_mut()?.bend_range = i32::from(r.u8()?);
                self.apply_bend(c, synth);
            }
            0xC7 => self.tracks[c].as_mut()?.note_wait = r.u8()? != 0,
            0xCA => synth.modulation(c, i32::from(r.u8()?)),
            0xD5 => {
                let expression = i32::from(r.u8()?);
                self.settings[c].expression = expression;
                synth.expression(c, expression);
            }
            0xE1 => self.tempo = u32::from(r.u16()?).max(1),
            0xFE => {
                r.u16()?;
            }
            // UNVERIFIED: the bank select, priority and damper, which
            // SmileBASIC 4's sequences use a little, are skipped.
            0xB1..=0xBF | 0xC2 | 0xC3 | 0xC6 | 0xC8..=0xC9 | 0xCB..=0xD4 | 0xD6..=0xDF => {
                r.u8()?;
            }
            0xE0 | 0xE2..=0xE6 => {
                r.u16()?;
            }
            // Commands SmileBASIC 4's sequences never use.
            _ => return None,
        }
        let t = self.tracks[c].as_mut()?;
        t.pc = next_pc.unwrap_or(r.at);
        Some(())
    }
}

/// A change a sequence makes, as [`record`] returns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Program(i32),
    Volume(i32),
    Expression(i32),
    Pan(i32),
    Modulation(i32),
    /// In cents.
    Bend(i32),
    NoteOn(i32, i32),
    NoteOff(i32),
    /// Quarter notes per minute.
    Tempo(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timed {
    pub tick: u32,
    pub channel: usize,
    pub event: Event,
}

/// Collects what a player asks of its synthesizer.
#[derive(Default)]
struct Recorder {
    tick: u32,
    events: Vec<Timed>,
    sounding: Vec<(usize, i32)>,
}

impl Recorder {
    fn push(&mut self, channel: usize, event: Event) {
        self.events.push(Timed {
            tick: self.tick,
            channel,
            event,
        });
    }
}

impl Synth for Recorder {
    fn plays(&self, _: i32) -> bool {
        true
    }
    fn program(&mut self, c: usize, p: i32) {
        self.push(c, Event::Program(p));
    }
    fn volume(&mut self, c: usize, v: i32) {
        self.push(c, Event::Volume(v));
    }
    fn expression(&mut self, c: usize, v: i32) {
        self.push(c, Event::Expression(v));
    }
    fn pan(&mut self, c: usize, v: i32) {
        self.push(c, Event::Pan(v));
    }
    fn modulation(&mut self, c: usize, v: i32) {
        self.push(c, Event::Modulation(v));
    }
    fn bend(&mut self, c: usize, cents: i32) {
        self.push(c, Event::Bend(cents));
    }
    fn note_on(&mut self, c: usize, key: i32, velocity: i32) {
        // A note started again ends the one still sounding.
        if let Some(i) = self.sounding.iter().position(|&n| n == (c, key)) {
            self.sounding.remove(i);
            self.push(c, Event::NoteOff(key));
        }
        self.sounding.push((c, key));
        self.push(c, Event::NoteOn(key, velocity));
    }
    fn note_off(&mut self, c: usize, key: i32) {
        if let Some(i) = self.sounding.iter().position(|&n| n == (c, key)) {
            self.sounding.remove(i);
            self.push(c, Event::NoteOff(key));
        }
    }
    fn channel_off(&mut self, _: usize) {}
    fn reset(&mut self) {}
    fn render(&mut self, _: &mut [f32], _: &mut [f32]) {}
}

/// The most ticks a recording lasts.
const RECORD_TICKS: u32 = 48 * 4 * 2000;

/// Plays a sequence through without sound and returns its events, tick by
/// tick, and its ticks per quarter note. A song that loops is played until
/// every track has looped once; notes still sounding then stop.
pub fn record(seq: Arc<Sequence>) -> (Vec<Timed>, u32) {
    let mut player = Player::new(seq);
    player.work = RECORD_WORK;
    let mut rec = Recorder::default();
    let mut tempo = None;
    while rec.tick < RECORD_TICKS && rec.events.len() < RECORD_EVENTS {
        player.tick(&mut rec);
        if tempo != Some(player.tempo) {
            tempo = Some(player.tempo);
            rec.push(0, Event::Tempo(player.tempo));
        }
        let done = player.tracks.iter().flatten().all(|t| t.ended || t.looped);
        if done && rec.tick > 0 {
            break;
        }
        rec.tick += 1;
    }
    for (c, key) in std::mem::take(&mut rec.sounding) {
        rec.push(c, Event::NoteOff(key));
    }
    (rec.events, player.timebase)
}

struct CodeReader<'a> {
    code: &'a [u8],
    at: usize,
}

impl CodeReader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let b = *self.code.get(self.at)?;
        self.at += 1;
        Some(b)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes([self.u8()?, self.u8()?]))
    }

    fn u24(&mut self) -> Option<usize> {
        let [a, b, c] = [self.u8()?, self.u8()?, self.u8()?].map(usize::from);
        Some(a | b << 8 | c << 16)
    }

    /// Seven bits per byte, most significant first, while the top bit is set.
    fn varlen(&mut self) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..4 {
            let b = self.u8()?;
            v = v << 7 | u32::from(b & 0x7F);
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records what a player asks of its synthesizer.
    #[derive(Default)]
    struct Log(Vec<String>);

    impl Synth for Log {
        fn plays(&self, _: i32) -> bool {
            true
        }
        fn program(&mut self, c: usize, p: i32) {
            self.0.push(format!("{c} @{p}"));
        }
        fn volume(&mut self, c: usize, v: i32) {
            self.0.push(format!("{c} V{v}"));
        }
        fn expression(&mut self, _: usize, _: i32) {}
        fn pan(&mut self, c: usize, v: i32) {
            self.0.push(format!("{c} P{v}"));
        }
        fn modulation(&mut self, _: usize, _: i32) {}
        fn bend(&mut self, c: usize, cents: i32) {
            self.0.push(format!("{c} bend {cents}"));
        }
        fn note_on(&mut self, c: usize, key: i32, velocity: i32) {
            self.0.push(format!("{c} on {key} {velocity}"));
        }
        fn note_off(&mut self, c: usize, key: i32) {
            self.0.push(format!("{c} off {key}"));
        }
        fn channel_off(&mut self, _: usize) {}
        fn reset(&mut self) {}
        fn render(&mut self, _: &mut [f32], _: &mut [f32]) {}
    }

    fn run(code: Vec<u8>, ticks: usize) -> (Player, Vec<String>) {
        let mut p = Player::new(Arc::new(Sequence {
            volume: 127,
            start: 0,
            code,
        }));
        let mut log = Log::default();
        for t in 0..ticks {
            log.0.push(format!("tick {t}"));
            p.tick(&mut log);
        }
        (p, log.0)
    }

    #[test]
    fn notes_wait_unless_told_not_to() {
        // Track 0 opens track 1 at 9, tempo 150, waits 2 and ends. Track 1:
        // @257, a note of 2 ticks that waits, then notes that do not.
        let code = vec![
            0x88, 1, 9, 0, 0, // open
            0xE1, 150, 0,    // tempo
            0xFF, // track 0 ends
            0x81, 0x82, 0x01, // @257
            60, 100, 2, // note, waits 2
            0xC7, 0, // notes stop waiting
            64, 90, 1, 67, 90, 1, // a chord
            0x80, 1, // wait
            0xFF,
        ];
        // The debugger sees both tracks, track 0 ended, and the chord
        // sounding on channel 1.
        let channels = run(code.clone(), 3).0.channels();
        assert_eq!(channels.len(), 2);
        assert!(channels[0].ended);
        let one = &channels[1];
        assert_eq!((one.program, one.velocity, one.notes), (257, 90, 3));
        assert_eq!(one.keys, [64, 67]);
        let (p, log) = run(code, 5);
        assert_eq!(p.tempo, 150);
        assert_eq!(
            log,
            [
                "tick 0",
                "1 @257",
                "1 on 60 100",
                "tick 1",
                "tick 2",
                "1 off 60",
                "1 on 64 90",
                "1 on 67 90",
                "tick 3",
                "1 off 64",
                "1 off 67",
                "tick 4"
            ]
        );
        assert!(p.ended());
    }

    #[test]
    fn calls_jumps_and_bends() {
        // Call 10, bend, then jump back to the call forever. At 10: a
        // wait and a return.
        let code = vec![
            0x8A, 10, 0, 0, // call
            0xC4, 64, // bend half the range up
            0x89, 0, 0, 0, // jump
            0x80, 1, 0xFD,
        ];
        let (p, log) = run(code, 3);
        assert!(!p.ended());
        assert_eq!(
            log,
            ["tick 0", "tick 1", "0 bend 100", "tick 2", "0 bend 100"]
        );
    }

    #[test]
    fn a_loop_without_waiting_ends_the_track() {
        let (p, _) = run(vec![0x89, 0, 0, 0], 1);
        assert!(p.ended());
        let (p, _) = run(vec![0x8A, 0, 0, 0], 1);
        assert!(p.ended());
        let (p, _) = run(vec![0x99], 1);
        assert!(p.ended());
    }

    #[test]
    fn a_recording_stops_after_one_loop() {
        // Tempo 100, then a note and a jump back to it.
        let code = vec![0xE1, 100, 0, 60, 90, 2, 0x89, 3, 0, 0];
        let (events, timebase) = record(Arc::new(Sequence {
            volume: 127,
            start: 0,
            code,
        }));
        assert_eq!(timebase, 48);
        let at = |tick, event| Timed {
            tick,
            channel: 0,
            event,
        };
        assert_eq!(
            events,
            [
                at(0, Event::NoteOn(60, 90)),
                at(0, Event::Tempo(100)),
                at(2, Event::NoteOff(60)),
                at(2, Event::NoteOn(60, 90)),
                at(2, Event::NoteOff(60)),
            ]
        );
    }

    /// Each track sets the volume, starts the next track and loops on the
    /// volume without waiting. A track ended for that loop is started
    /// again by the track before it on the next tick, forever.
    fn tracks_restarting_each_other() -> Vec<u8> {
        let mut code = Vec::new();
        for c in 0..CHANNELS as u32 {
            let next = (c + 1) % CHANNELS as u32;
            code.extend_from_slice(&[0x88, next as u8]);
            code.extend_from_slice(&(next * 11).to_le_bytes()[..3]);
            code.extend_from_slice(&[0xC1, 100]);
            code.push(0x89);
            code.extend_from_slice(&(c * 11 + 5).to_le_bytes()[..3]);
        }
        code
    }

    /// Counts what it is asked to do.
    #[derive(Default)]
    struct Count(usize);

    impl Synth for Count {
        fn plays(&self, _: i32) -> bool {
            true
        }
        fn program(&mut self, _: usize, _: i32) {
            self.0 += 1;
        }
        fn volume(&mut self, _: usize, _: i32) {
            self.0 += 1;
        }
        fn expression(&mut self, _: usize, _: i32) {}
        fn pan(&mut self, _: usize, _: i32) {
            self.0 += 1;
        }
        fn modulation(&mut self, _: usize, _: i32) {}
        fn bend(&mut self, _: usize, _: i32) {}
        fn note_on(&mut self, _: usize, _: i32, _: i32) {
            self.0 += 1;
        }
        fn note_off(&mut self, _: usize, _: i32) {
            self.0 += 1;
        }
        fn channel_off(&mut self, _: usize) {}
        fn reset(&mut self) {}
        fn render(&mut self, _: &mut [f32], _: &mut [f32]) {}
    }

    fn player(code: Vec<u8>) -> Player {
        Player::new(Arc::new(Sequence {
            volume: 127,
            start: 0,
            code,
        }))
    }

    /// Runs as many ticks as a block has samples: the most a block can
    /// hold.
    fn stuck_within_a_block(code: Vec<u8>) -> (Player, usize) {
        let (mut p, mut synth) = (player(code), Count::default());
        p.begin_block();
        for _ in 0..super::super::FRAME_SAMPLES {
            p.tick(&mut synth);
        }
        (p, synth.0)
    }

    #[test]
    fn tracks_that_keep_restarting_each_other_are_stopped() {
        let (p, commands) = stuck_within_a_block(tracks_restarting_each_other());
        assert!(p.ended(), "the whole sequence stops");
        // Half its commands set the volume, and it stops at the budget.
        assert!(commands <= BLOCK_WORK as usize, "{commands}");
        assert!(commands > 0);
    }

    #[test]
    fn notes_piling_up_are_stopped() {
        // Notes that do not wait and last long, one per tick, on a track
        // that then loops: counting them down must not grow forever.
        let code = vec![
            0xC7, 0, // notes stop waiting
            60, 100, 0xFF, 0xFF, 0x7F, // a note of two million ticks
            0x80, 1, // wait
            0x89, 2, 0, 0, // again
        ];
        let (mut p, mut synth) = (player(code), Count::default());
        let mut blocks = 0;
        while !p.ended() {
            p.begin_block();
            for _ in 0..super::super::FRAME_SAMPLES {
                p.tick(&mut synth);
            }
            blocks += 1;
            assert!(blocks < 1000, "goes on for ever");
        }
        let notes_off = synth.0 / 2;
        assert!(notes_off > 0 && notes_off < 2 * BLOCK_WORK as usize);
    }

    #[test]
    fn work_starts_again_with_each_block() {
        // A busy tick, far below the limit, for many blocks.
        let mut code = [0xC1, 1].repeat(100);
        code.extend_from_slice(&[0x80, 1, 0x89, 0, 0, 0]);
        let (mut p, mut synth) = (player(code), Count::default());
        for _ in 0..100 {
            p.begin_block();
            for _ in 0..16 {
                p.tick(&mut synth);
            }
        }
        assert!(!p.ended());
    }

    #[test]
    fn a_recording_of_a_stuck_sequence_stops() {
        let (events, _) = record(Arc::new(Sequence {
            volume: 127,
            start: 0,
            code: tracks_restarting_each_other(),
        }));
        // Past the cap by at most a tick's worth of commands.
        let tick = CHANNELS * COMMANDS_PER_TICK as usize * 2;
        assert!(events.len() <= RECORD_EVENTS + tick, "{}", events.len());
    }

    #[test]
    fn sequences_read_back() {
        let mut s = Sequences::default();
        s.insert(
            Kind::Beep,
            3,
            Sequence {
                volume: 88,
                start: 1,
                code: vec![0xFF, 0xFF],
            },
        );
        s.insert(
            Kind::Song,
            0,
            Sequence {
                volume: 59,
                start: 0,
                code: vec![0xFF],
            },
        );
        let back = Sequences::from_bytes(&s.to_bytes()).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.get(Kind::Beep, 3).unwrap().volume, 88);
        assert!(back.get(Kind::Beep, 2).is_none() && back.get(Kind::Song, -1).is_none());
        let bytes = s.to_bytes();
        assert!(Sequences::from_bytes(&bytes[..bytes.len() - 1]).is_none());
    }
}

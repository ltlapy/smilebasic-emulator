//! The sounds that the BGM tracks and BEEP's handles play, for the
//! debugger: a snapshot taken only when asked for, so that playing costs
//! nothing more.
//! SmileBASIC 4 shows none of this; programs only have BGMCHK and BGMVAR.

use std::rc::Rc;

/// The play state of a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Started on an idle sound system, waiting for it to wake up.
    Waking,
    Playing,
    FadingIn,
    /// BGMSTOP with a time.
    FadingOut,
    /// BGMPAUSE with a time.
    Pausing,
    Paused,
}

/// One BGM track that plays or is paused.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackInfo {
    pub track: usize,
    /// BGMPLAY's song: a preset up to [`super::PRESET_SONGS`], or 128 to
    /// 255; BGMPLAY with MML plays 255.
    pub song: i32,
    /// A user song's MML as the program gave it.
    pub source: Option<Rc<str>>,
    /// A preset that plays silence, because the preset songs were not
    /// imported.
    pub silent: bool,
    pub state: State,
    /// The fade's gain, 0 to 1.
    pub fade: f64,
    /// BGMVOL, and BGMPITCH in cents.
    pub volume: i32,
    pub pitch: i32,
    /// Quarter notes per minute.
    pub tempo: u32,
    pub ticks_per_quarter: u32,
    /// Ticks played since the song started.
    pub ticks: u64,
    /// The MML variables $0 to $7, as the song has them now.
    pub vars: [i32; 8],
    /// The channels the song uses.
    pub channels: Vec<ChannelInfo>,
}

/// A channel of a track, or of a preset song's sequence.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelInfo {
    pub channel: usize,
    /// @'s instrument.
    pub program: i32,
    /// O's octave; a preset song has none.
    pub octave: Option<i32>,
    /// V, the velocity of the notes, and P, all 0 to 127.
    pub volume: i32,
    pub velocity: i32,
    pub pan: i32,
    /// The notes sounding, as MIDI keys (O4C is 60).
    pub keys: Vec<i32>,
    /// Notes started so far, wrapping: a change signals a new note.
    pub notes: u32,
    /// The channel has run out of commands.
    pub ended: bool,
}

/// The kind of sound a BEEP handle plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeepKind {
    /// A preset effect that plays nothing, because the effects were not
    /// imported.
    Silent,
    /// A preset effect.
    Effect,
    /// An instrument, @0 to @127 as BEEP 256 to 383.
    Instrument(i32),
    /// A waveform of WAVSET.
    Wave,
}

/// A BEEP handle that sounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeepInfo {
    pub handle: usize,
    /// BEEP's sound number.
    pub sound: i32,
    pub kind: BeepKind,
    /// BEEPPIT's pitch in cents, BEEPVOL and BEEPPAN.
    pub pitch: i32,
    pub volume: i32,
    pub pan: i32,
    /// BEEPSTOP with a time.
    pub stopping: bool,
}

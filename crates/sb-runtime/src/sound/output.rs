//! Between the frames' sound and the sound device: the program makes a
//! frame of samples at a time, on its own clock, and the device takes them
//! in blocks of its own size, on another. This keeps a margin of samples
//! between them, plays them a little faster or slower to hold the margin
//! (the two clocks never quite agree, as a 59.98 Hz display paces frames
//! that the device plays at 60), and when the program falls so far behind
//! that the margin runs out, fades out and back in rather than clicking.
//!
//! It is plain data, without threads: the frontend shares it between the
//! loop that pushes and the device's callback that pulls.

use std::collections::VecDeque;

use super::FRAME_SAMPLES;

/// Samples kept beyond the device's largest block: three frames, so that
/// the program can be about three frames late before it is heard.
const MARGIN: usize = 3 * FRAME_SAMPLES;
/// Samples of fade where the sound stops for want of samples, starts
/// again or skips.
const FADE: usize = 96;
/// How far from the normal speed the rate may go, as a fraction: 0.5%,
/// under a tenth of a semitone.
const MOST_SKEW: f64 = 0.005;
/// The skew for a margin off by its whole size.
const GAIN: f64 = 0.02;
/// How quickly the averaged fill level follows each block's.
const SMOOTHING: f64 = 0.05;
/// More than this beyond the target and the oldest samples are skipped,
/// as when the device stopped taking them for a while.
const MOST_OVER: usize = 6 * FRAME_SAMPLES;
/// The largest block of the device that sets the target.
const MOST_BLOCK: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Waiting for the margin to fill, playing silence.
    Filling,
    Playing,
    /// Fading out to skip to the newest samples.
    Skipping,
}

/// What happened, for measuring.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Times the samples ran out while playing.
    pub ran_out: u32,
    /// Times samples were skipped for being too far ahead.
    pub skips: u32,
    /// Silent samples played while waiting for samples.
    pub silent: u64,
}

pub struct Output {
    /// Stereo samples not yet played. The first is the one before the read
    /// position, which the interpolation also needs.
    queue: VecDeque<[f32; 2]>,
    /// The read position past `queue[1]`, 0 to 1.
    frac: f64,
    state: State,
    /// The gain of fades in and out.
    gain: f32,
    /// The last sample given, for the fade where samples run out.
    last: [f32; 2],
    /// The rest of that fade, in samples.
    tail: usize,
    /// The fill level, averaged over blocks.
    level: f64,
    block: usize,
    pub stats: Stats,
}

impl Default for Output {
    fn default() -> Self {
        Output {
            queue: VecDeque::from([[0.0; 2]]),
            frac: 0.0,
            state: State::Filling,
            gain: 0.0,
            last: [0.0; 2],
            tail: 0,
            level: 0.0,
            block: FRAME_SAMPLES,
            stats: Stats::default(),
        }
    }
}

/// Four-point Hermite interpolation between `b` and `c`.
fn hermite(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    let c1 = 0.5 * (c - a);
    let c2 = a - 2.5 * b + 2.0 * c - 0.5 * d;
    let c3 = 0.5 * (d - a) + 1.5 * (b - c);
    ((c3 * t + c2) * t + c1) * t + b
}

impl Output {
    /// Adds samples, left and right in turn.
    pub fn push(&mut self, samples: &[f32]) {
        self.queue
            .extend(samples.chunks_exact(2).map(|s| [s[0], s[1]]));
    }

    /// Samples that can be played: all but the one before the read
    /// position and the two after the last that interpolation looks at.
    fn ready(&self) -> usize {
        self.queue.len().saturating_sub(3)
    }

    /// The fill level the rate aims at, as the device's block starts.
    pub fn target(&self) -> usize {
        self.block + MARGIN + FRAME_SAMPLES / 2
    }

    /// Samples waiting, as a measure of latency.
    pub fn queued(&self) -> usize {
        self.ready()
    }

    /// Keeps only the newest `keep` samples to play.
    fn drop_oldest(&mut self, keep: usize) {
        let extra = self.ready().saturating_sub(keep);
        self.queue.drain(..extra);
    }

    /// Fills `out` with samples, left and right in turn.
    pub fn pull(&mut self, out: &mut [f32]) {
        let n = out.len() / 2;
        self.block = self.block.max(n.min(MOST_BLOCK));
        let target = self.target();
        if self.state == State::Filling && self.ready() >= target {
            // Start from the newest samples, so that waiting does not
            // add to the latency, and fade in.
            self.drop_oldest(target);
            self.frac = 0.0;
            self.state = State::Playing;
            self.level = target as f64;
        }
        if self.state != State::Filling {
            self.level += SMOOTHING * (self.ready() as f64 - self.level);
            if self.state == State::Playing && self.ready() > target + MOST_OVER {
                self.state = State::Skipping;
                self.stats.skips += 1;
            }
        }
        let error = (self.level - target as f64) / target as f64;
        let ratio = 1.0 + (error * GAIN).clamp(-MOST_SKEW, MOST_SKEW);
        let step = 1.0 / FADE as f32;
        for frame in out.chunks_exact_mut(2) {
            if self.state != State::Filling && self.queue.len() < 4 {
                // Ran out: fade from the last sample, and wait for the
                // margin to fill again.
                self.state = State::Filling;
                self.stats.ran_out += 1;
                self.tail = FADE;
            }
            if self.state == State::Filling {
                let fade = self.tail as f32 / FADE as f32;
                self.tail = self.tail.saturating_sub(1);
                frame[0] = self.last[0] * fade;
                frame[1] = self.last[1] * fade;
                self.gain = 0.0;
                self.stats.silent += 1;
                continue;
            }
            let t = self.frac as f32;
            let q = &self.queue;
            for (ch, s) in frame.iter_mut().enumerate() {
                *s = hermite(q[0][ch], q[1][ch], q[2][ch], q[3][ch], t) * self.gain;
            }
            self.last = [frame[0], frame[1]];
            if self.state == State::Skipping {
                self.gain -= step;
                if self.gain <= 0.0 {
                    self.gain = 0.0;
                    self.drop_oldest(target);
                    self.state = State::Playing;
                }
            } else {
                self.gain = (self.gain + step).min(1.0);
            }
            self.frac += ratio;
            let whole = self.frac as usize;
            self.frac -= whole as f64;
            self.queue.drain(..whole.min(self.queue.len() - 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine at 441 Hz, from sample `from`.
    fn sine(from: usize, n: usize) -> Vec<f32> {
        (from..from + n)
            .flat_map(|i| {
                let s = (i as f32 * 441.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5;
                [s, s]
            })
            .collect()
    }

    /// Runs a producer making a frame every `frame_secs` (late by `late`
    /// seconds at the frames listed) against a device pulling `block`
    /// samples at 48 kHz, for `secs`; gives the output and the stats.
    fn run(frame_secs: f64, block: usize, secs: f64, late: &[(usize, f64)]) -> (Vec<f32>, Output) {
        let mut o = Output::default();
        let (mut made, mut frames) = (0, 0);
        let mut out = Vec::new();
        let mut next_frame = 0.0;
        let mut pulled = 0usize;
        while (pulled as f64) < secs * 48_000.0 {
            let now = pulled as f64 / 48_000.0;
            // Frames due by now, as the main loop runs them, late ones
            // delaying all that follow until it catches up.
            while next_frame <= now {
                let delay = late.iter().find(|l| l.0 == frames).map_or(0.0, |l| l.1);
                if delay > 0.0 && now < next_frame + delay {
                    break;
                }
                o.push(&sine(made, FRAME_SAMPLES));
                made += FRAME_SAMPLES;
                frames += 1;
                next_frame += frame_secs;
            }
            let mut buf = vec![0.0; block * 2];
            o.pull(&mut buf);
            out.extend(buf);
            pulled += block;
        }
        (out, o)
    }

    /// The largest jump between samples next to each other.
    fn largest_step(s: &[f32]) -> f32 {
        s.chunks_exact(2)
            .zip(s.chunks_exact(2).skip(1))
            .map(|(a, b)| (a[0] - b[0]).abs())
            .fold(0.0, f32::max)
    }

    #[test]
    fn steady_frames_never_run_out() {
        let (out, o) = run(1.0 / 60.0, 1024, 30.0, &[]);
        assert_eq!(o.stats.ran_out, 0);
        assert_eq!(o.stats.skips, 0);
        // A 441 Hz sine at 0.5 changes at most 0.029 a sample.
        assert!(largest_step(&out[48_000..]) < 0.04);
    }

    #[test]
    fn slower_and_faster_frames_are_followed() {
        // A 59.98 Hz display, and a fast one, for a minute each.
        for fps in [59.98, 59.9, 60.1] {
            let (out, o) = run(1.0 / fps, 1024, 60.0, &[]);
            assert_eq!((o.stats.ran_out, o.stats.skips), (0, 0), "{fps}");
            assert!(largest_step(&out[48_000..]) < 0.04, "{fps}");
            assert!(o.queued() < o.target() + FRAME_SAMPLES, "{fps}");
        }
    }

    #[test]
    fn a_frame_late_by_two_frames_is_not_heard() {
        let late = [(200, 0.030), (400, 0.034), (900, 0.025)];
        let (out, o) = run(1.0 / 60.0, 1024, 20.0, &late);
        assert_eq!(o.stats.ran_out, 0);
        assert!(largest_step(&out[48_000..]) < 0.04);
    }

    #[test]
    fn running_out_fades_instead_of_clicking() {
        let (out, o) = run(1.0 / 60.0, 1024, 20.0, &[(300, 0.2)]);
        assert_eq!(o.stats.ran_out, 1);
        assert!(largest_step(&out[48_000..]) < 0.04);
        // The margin is back afterwards, without extra latency.
        assert!(o.queued() < o.target() + FRAME_SAMPLES);
    }

    #[test]
    fn too_much_ahead_is_skipped_smoothly() {
        let mut o = Output::default();
        let mut out = vec![0.0; 2048];
        o.push(&sine(0, 20 * FRAME_SAMPLES));
        o.pull(&mut out);
        o.push(&sine(20 * FRAME_SAMPLES, 20 * FRAME_SAMPLES));
        let mut all = Vec::new();
        for _ in 0..10 {
            o.pull(&mut out);
            all.extend_from_slice(&out);
        }
        assert_eq!(o.stats.skips, 1);
        assert!(largest_step(&all) < 0.04);
        assert!(o.queued() <= o.target());
    }

    #[test]
    fn interpolation_passes_samples_through_at_whole_positions() {
        assert_eq!(hermite(0.1, 0.2, 0.7, -0.3, 0.0), 0.2);
        assert!((hermite(0.1, 0.2, 0.7, -0.3, 1.0) - 0.7).abs() < 1e-6);
    }
}

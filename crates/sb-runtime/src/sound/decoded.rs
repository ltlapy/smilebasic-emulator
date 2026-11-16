//! The bank's DSP-ADPCM waveforms decoded once and kept, for the notes
//! that play them again.
//!
//! Decoding is a chain of predictions, each sample from the two before,
//! and took about half of the sampler's time while songs played. Songs
//! play the same few waveforms over and over, so the samples a note
//! decodes are kept, a chunk at a time as notes reach them, and the notes
//! after it read them from here. The chunks hold exactly what decoding
//! gives on a waveform's first pass. A later pass round the loop starts
//! from the loop's own history, which the bank's waveforms give as the
//! first pass's; a waveform whose history differs decodes its later passes
//! as before.
//!
//! A sound system keeps at most [`BUDGET`] bytes of them. When a chunk
//! would go beyond it, the waveform no note plays that was played longest
//! ago makes way; when every waveform kept is playing, the note decodes on
//! by itself, as notes did before.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use super::bank::{Bank, Encoding, Reader, WaveInfo};

/// Samples in a chunk: as many as a note decodes in about 20 ms.
const CHUNK: usize = 1024;
/// The most a sound system keeps decoded, in bytes: 2 Mi samples, as long
/// as about 40 seconds of a 48 kHz waveform.
pub(crate) const BUDGET: usize = 4 << 20;

/// A sound system's decoded waveforms, shared by its synthesizers.
pub(crate) type Shared = Rc<RefCell<Decoded>>;

#[derive(Default)]
pub(crate) struct Decoded {
    /// The bank the waveforms are of: another bank starts afresh.
    bank: Weak<Bank>,
    /// By the waveform's number in the bank.
    entries: Vec<Option<Rc<RefCell<Entry>>>>,
    /// The numbers of the waveforms with an entry.
    kept: Vec<usize>,
    bytes: usize,
    budget: usize,
    /// Counts the notes started, to tell which waveform played longest ago.
    clock: u64,
}

impl fmt::Debug for Decoded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Decoded")
            .field("waves", &self.kept.len())
            .field("bytes", &self.bytes)
            .finish()
    }
}

/// One waveform's samples decoded so far.
pub(super) struct Entry {
    chunks: Vec<Rc<[i16]>>,
    /// What decodes the next chunk: a reader at the end of the last one.
    decoder: Reader,
    /// Decoding gave out before the end, as for missing bytes.
    short: bool,
    /// Whether later passes round the loop give the first pass's samples.
    loops_same: Option<bool>,
    bytes: usize,
    used: u64,
}

/// What a reader keeps of the waveform it takes from here.
#[derive(Clone)]
pub(super) struct Cached {
    shared: Shared,
    entry: Rc<RefCell<Entry>>,
    /// The chunk the reader is in, and where in it the next sample is.
    pub(super) chunk: Rc<[i16]>,
    pub(super) at: usize,
}

impl fmt::Debug for Cached {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cached")
            .field("chunk", &self.chunk.len())
            .field("at", &self.at)
            .finish()
    }
}

impl Decoded {
    pub(crate) fn new() -> Shared {
        Rc::new(RefCell::new(Decoded {
            budget: BUDGET,
            ..Decoded::default()
        }))
    }

    /// The bytes kept now.
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Keeps at most `budget` bytes from now on.
    #[cfg(test)]
    pub(crate) fn set_budget(&mut self, budget: usize) {
        self.budget = budget;
    }

    /// A reader for waveform `index` of `bank` that takes what is decoded
    /// from here. A 16-bit waveform, which costs nothing to decode, gets a
    /// reader of its own.
    pub(crate) fn reader(shared: &Shared, bank: &Arc<Bank>, index: usize) -> Reader {
        let Some(wave) = bank.waves.get(index) else {
            return Reader::default();
        };
        if wave.encoding == Encoding::Pcm16 {
            return Reader::default();
        }
        let mut d = shared.borrow_mut();
        if !d.bank.upgrade().is_some_and(|b| Arc::ptr_eq(&b, bank)) {
            let budget = d.budget;
            *d = Decoded {
                bank: Arc::downgrade(bank),
                entries: vec![None; bank.waves.len()],
                budget,
                ..Decoded::default()
            };
        }
        d.clock += 1;
        let used = d.clock;
        let entry = match &d.entries[index] {
            Some(e) => e.clone(),
            None => {
                let e = Rc::new(RefCell::new(Entry {
                    chunks: Vec::new(),
                    decoder: Reader::default(),
                    short: false,
                    loops_same: None,
                    bytes: 0,
                    used,
                }));
                d.entries[index] = Some(e.clone());
                d.kept.push(index);
                e
            }
        };
        entry.borrow_mut().used = used;
        Reader::from_cache(Cached {
            shared: shared.clone(),
            entry,
            chunk: Rc::from([]),
            at: 0,
        })
    }

    /// Frees entries no reader holds, the one played longest ago first,
    /// until `bytes` more fit; false when they cannot.
    fn make_room(&mut self, bytes: usize, keep: &Rc<RefCell<Entry>>) -> bool {
        while self.bytes + bytes > self.budget {
            let oldest = (self.kept.iter().enumerate())
                .filter_map(|(k, &i)| Some((k, self.entries[i].as_ref()?)))
                .filter(|(_, e)| Rc::strong_count(e) == 1 && !Rc::ptr_eq(e, keep))
                .min_by_key(|(_, e)| e.borrow().used)
                .map(|(k, _)| k);
            let Some(k) = oldest else {
                return false;
            };
            let i = self.kept.swap_remove(k);
            if let Some(e) = self.entries[i].take() {
                self.bytes -= e.borrow().bytes;
            }
        }
        true
    }
}

impl Cached {
    /// The chunk that holds sample `pos`, decoding it if it is the next,
    /// and where in it `pos` is; `None` when it is not kept and cannot be.
    pub(super) fn chunk_at(
        &self,
        pos: usize,
        wave: &WaveInfo,
        data: &[u8],
    ) -> Option<(Rc<[i16]>, usize)> {
        let (k, at) = (pos / CHUNK, pos % CHUNK);
        let mut e = self.entry.borrow_mut();
        if let Some(c) = e.chunks.get(k) {
            return Some((c.clone(), at)).filter(|(c, _)| at < c.len());
        }
        if k != e.chunks.len() || e.short {
            return None;
        }
        let n = CHUNK.min((wave.len as usize).saturating_sub(k * CHUNK));
        if n == 0 || !self.shared.borrow_mut().make_room(2 * n, &self.entry) {
            return None;
        }
        let mut samples = Vec::with_capacity(n);
        for _ in 0..n {
            match e.decoder.next_of(wave, data) {
                Some(s) => samples.push(s),
                None => {
                    e.short = true;
                    break;
                }
            }
        }
        let c: Rc<[i16]> = samples.into();
        e.bytes += 2 * c.len();
        self.shared.borrow_mut().bytes += 2 * c.len();
        e.chunks.push(c.clone());
        Some((c, at)).filter(|(c, _)| at < c.len())
    }

    /// The two samples before `pos` on the first pass, as decoding a sample
    /// at a time has them there.
    pub(super) fn history_at(&self, pos: usize) -> Option<[i16; 2]> {
        let e = self.entry.borrow();
        let sample = |p: Option<usize>| match p {
            None => Some(0),
            Some(p) => e.chunks.get(p / CHUNK)?.get(p % CHUNK).copied(),
        };
        Some([sample(pos.checked_sub(1))?, sample(pos.checked_sub(2))?])
    }

    /// Whether a pass round the loop from `start` gives the first pass's
    /// samples: whether the loop's history is what the first pass has
    /// there.
    pub(super) fn loops_same(&self, start: usize, loop_history: [i16; 2]) -> bool {
        if let Some(same) = self.entry.borrow().loops_same {
            return same;
        }
        let same = self.history_at(start) == Some(loop_history);
        self.entry.borrow_mut().loops_same = Some(same);
        same
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xorshift(mut seed: u32) -> impl FnMut(u32) -> u32 {
        move |n| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % n
        }
    }

    /// A bank of `n` random DSP-ADPCM waveforms of up to five chunks, some
    /// looping from where the first pass's history is the loop's, some
    /// from where it is not.
    fn bank(next: &mut impl FnMut(u32) -> u32, n: usize) -> Bank {
        let mut b = Bank::default();
        for i in 0..n {
            let len = 1 + next(5 * CHUNK as u32);
            let mut coefs = [0i16; 16];
            coefs.iter_mut().for_each(|c| *c = next(4096) as i16 - 1024);
            let size = len.div_ceil(14) as usize * 8;
            let start = b.data.len();
            for k in 0..size {
                let x = next(256) as u8;
                // Frame headers: scales up to 2^11.
                b.data.push(if k % 8 == 0 {
                    (x & 0x70) | ((x & 15) % 12)
                } else {
                    x
                });
            }
            let start_at = next(len);
            let loop_start = (next(3) > 0).then_some(start_at);
            let mut wave = WaveInfo {
                right: None,
                rate: 32000,
                len,
                loop_start,
                encoding: Encoding::Adpcm {
                    coefs,
                    loop_history: [next(2000) as i16 - 1000, next(2000) as i16 - 1000],
                },
                data: start..start + size,
            };
            if let (Some(ls), true) = (loop_start, i % 3 != 0) {
                // The loop's history as the first pass has it.
                let mut r = Reader::default();
                let first: Vec<i16> = (0..ls).map(|_| r.next(&wave, &b.data).unwrap()).collect();
                let at = |k: Option<usize>| k.map_or(0, |k| first[k]);
                let ls = ls as usize;
                wave.encoding = Encoding::Adpcm {
                    coefs,
                    loop_history: [at(ls.checked_sub(1)), at(ls.checked_sub(2))],
                };
            }
            b.waves.push(wave);
        }
        b
    }

    /// Notes that read kept waveforms, starting at different times, get
    /// what decoding alone gives, whatever the budget: through chunks,
    /// round loops with and without the first pass's history, at the end,
    /// and where the budget leaves a note to decode on alone. The budget
    /// holds.
    #[test]
    fn kept_waveforms_read_as_decoding_does() {
        let mut next = xorshift(0x9E37_79B9);
        for (round, budget) in [BUDGET, 6 * CHUNK, 2 * CHUNK, 0].into_iter().enumerate() {
            let bank = Arc::new(bank(&mut next, 6));
            let shared = Decoded::new();
            shared.borrow_mut().set_budget(budget);
            // Notes, each with a reader that decodes alone, and how many
            // samples it has left to take.
            let mut notes: Vec<(usize, Reader, Reader, u32)> = Vec::new();
            for step in 0..3000 {
                if notes.len() < 8 && next(30) == 0 {
                    let w = next(bank.waves.len() as u32) as usize;
                    let kept = Decoded::reader(&shared, &bank, w);
                    notes.push((w, kept, Reader::default(), 3 * bank.waves[w].len + 20));
                }
                for (w, kept, alone, left) in &mut notes {
                    let wave = &bank.waves[*w];
                    let data = &bank.data[wave.data.clone()];
                    // Some samples through the slice, some one by one.
                    for _ in 0..next(300) {
                        let take = kept.ahead().len().min(next(4) as usize);
                        for _ in 0..take {
                            let s = kept.ahead()[0];
                            kept.skip(1);
                            assert_eq!(Some(s), alone.next_of(wave, data), "round {round}");
                        }
                        let s = kept.next_of(wave, data);
                        assert_eq!(s, alone.next_of(wave, data), "round {round} step {step}");
                        *left = left.saturating_sub(1 + take as u32);
                        if s.is_none() {
                            *left = 0;
                        }
                    }
                }
                notes.retain(|n| n.3 > 0);
                assert!(shared.borrow().bytes() <= budget, "round {round}");
            }
        }
    }

    /// Waveforms no note plays make way for new ones, the one played
    /// longest ago first; a waveform a note plays stays. Another bank
    /// starts afresh.
    #[test]
    fn the_waveform_played_longest_ago_makes_way() {
        let mut next = xorshift(7);
        let bank = Arc::new(bank(&mut next, 4));
        let shared = Decoded::new();
        let chunk = |w: usize| 2 * CHUNK.min(bank.waves[w].len as usize);
        shared
            .borrow_mut()
            .set_budget(chunk(0) + chunk(1) + chunk(2));
        let read = |w: usize| {
            let mut r = Decoded::reader(&shared, &bank, w);
            r.next(&bank.waves[w], &bank.data);
            r
        };
        let held = read(0);
        drop(read(1));
        drop(read(2));
        drop(read(1));
        assert_eq!(shared.borrow().bytes(), chunk(0) + chunk(1) + chunk(2));
        // Wave 2, played longer ago than 1, makes way for wave 3; wave 0
        // plays on.
        drop(read(3));
        let kept: Vec<bool> = (0..4)
            .map(|w| shared.borrow().entries[w].is_some())
            .collect();
        assert_eq!(kept, [true, true, false, true]);
        assert!(shared.borrow().bytes() <= chunk(0) + chunk(1) + chunk(2));
        drop(held);
        let other = Arc::new(Bank::clone(&bank));
        drop(Decoded::reader(&shared, &other, 3));
        assert_eq!(shared.borrow().bytes(), 0);
    }
}

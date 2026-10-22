use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::shared::Shared;

/// Ring buffer capacity: two seconds of stereo audio at 192 kHz.
const CAPACITY_SAMPLES: usize = 2 * 2 * 192_000;

/// The audio-thread end of the stream. Samples are stored as interleaved
/// stereo frames; channel selection happens in the worker.
pub struct Feeder {
    producer: Producer<f32>,
    shared: Arc<Shared>,
    last_call: Option<Instant>,
}

/// Creates the ring buffer connecting the audio thread to the worker.
pub fn audio_channel(shared: Arc<Shared>) -> (Feeder, Consumer<f32>) {
    let (producer, consumer) = RingBuffer::new(CAPACITY_SAMPLES);
    (
        Feeder {
            producer,
            shared,
            last_call: None,
        },
        consumer,
    )
}

impl Feeder {
    /// Hands one block of audio to the worker.
    ///
    /// Real-time safe: no allocation, no locks, never blocks. If the ring
    /// buffer is full the excess frames are dropped and counted.
    pub fn push(&mut self, left: &[f32], right: &[f32]) {
        let counters = &self.shared.input;
        let now = Instant::now();
        if let Some(prev) = self.last_call {
            counters.record_interval(now - prev);
        }
        self.last_call = Some(now);

        let frames = left.len().min(right.len());
        counters.blocks.fetch_add(1, Ordering::Relaxed);
        counters.last_block.store(frames as u32, Ordering::Relaxed);
        counters.frames.fetch_add(frames as u64, Ordering::Relaxed);

        let n = frames.min(self.producer.slots() / 2);
        if n < frames {
            counters
                .overrun_frames
                .fetch_add((frames - n) as u64, Ordering::Relaxed);
        }
        if n == 0 {
            return;
        }
        if let Ok(chunk) = self.producer.write_chunk_uninit(n * 2) {
            chunk.fill_from_iter(
                left[..n]
                    .iter()
                    .zip(&right[..n])
                    .flat_map(|(l, r)| [*l, *r]),
            );
        }
    }

    /// Forget the previous callback time, e.g. after the host stopped
    /// processing, so the gap is not counted as a callback interval.
    pub fn reset_timing(&mut self) {
        self.last_call = None;
    }
}

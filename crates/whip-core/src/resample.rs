use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};

/// Input frames per resampler call.
const CHUNK_FRAMES: usize = 256;

/// Converts interleaved audio at the host rate to 48 kHz. Passes audio through
/// unchanged when the host already runs at 48 kHz.
pub(crate) struct To48k {
    inner: Option<Inner>,
}

struct Inner {
    resampler: Async<f32>,
    channels: usize,
    pending: Vec<f32>,
    out: Vec<f32>,
}

impl To48k {
    pub fn new(input_rate: u32, channels: usize) -> Result<Self, String> {
        if input_rate == 48_000 {
            return Ok(Self { inner: None });
        }
        let params = SincInterpolationParameters::new(128, WindowFunction::Blackman2)
            .oversampling_factor(128)
            .interpolation(SincInterpolationType::Linear);
        let resampler = Async::<f32>::new_sinc(
            48_000.0 / input_rate as f64,
            1.0,
            &params,
            CHUNK_FRAMES,
            channels,
            FixedAsync::Input,
        )
        .map_err(|e| e.to_string())?;
        let out = vec![0.0; resampler.output_frames_max() * channels];
        Ok(Self {
            inner: Some(Inner {
                resampler,
                channels,
                pending: Vec::with_capacity(CHUNK_FRAMES * channels * 4),
                out,
            }),
        })
    }

    /// Resamples `input` and appends the result to `output`. Partial chunks
    /// are kept for the next call.
    pub fn process(&mut self, input: &[f32], output: &mut Vec<f32>) -> Result<(), String> {
        let Some(inner) = &mut self.inner else {
            output.extend_from_slice(input);
            return Ok(());
        };
        inner.pending.extend_from_slice(input);
        let ch = inner.channels;
        let chunk_len = CHUNK_FRAMES * ch;
        let mut offset = 0;
        while inner.pending.len() - offset >= chunk_len {
            let in_adapter =
                InterleavedSlice::new(&inner.pending[offset..offset + chunk_len], ch, CHUNK_FRAMES)
                    .map_err(|e| e.to_string())?;
            let capacity = inner.out.len() / ch;
            let mut out_adapter = InterleavedSlice::new_mut(&mut inner.out, ch, capacity)
                .map_err(|e| e.to_string())?;
            let (_, written) = inner
                .resampler
                .process_into_buffer(&in_adapter, &mut out_adapter, None)
                .map_err(|e| e.to_string())?;
            output.extend_from_slice(&inner.out[..written * ch]);
            offset += chunk_len;
        }
        inner.pending.drain(..offset);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_at_48k() {
        let mut r = To48k::new(48_000, 1).unwrap();
        let mut out = Vec::new();
        r.process(&[0.5; 100], &mut out).unwrap();
        assert_eq!(out.len(), 100);
    }

    #[test]
    fn converts_44k1_at_the_right_ratio() {
        let mut r = To48k::new(44_100, 2).unwrap();
        let mut out = Vec::new();
        // One second of stereo input in odd-sized blocks.
        let block = vec![0.25f32; 441 * 2];
        for _ in 0..100 {
            r.process(&block, &mut out).unwrap();
        }
        let frames = out.len() / 2;
        // Allow for the resampler's delay and the unprocessed remainder.
        assert!((47_500..=48_000).contains(&frames), "got {frames} frames");
    }
}

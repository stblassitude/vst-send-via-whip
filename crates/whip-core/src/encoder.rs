use opus::{Application, Bitrate, Channels, Encoder, Signal};

use crate::settings::{Content, StreamSettings};

/// Samples per channel in one 20 ms Opus frame at 48 kHz.
pub(crate) const FRAME_SAMPLES: usize = 960;

/// Cuts 48 kHz audio into 20 ms frames and encodes them with Opus.
pub(crate) struct OpusFramer {
    encoder: Encoder,
    channels: usize,
    pending: Vec<f32>,
    silence: Vec<f32>,
    out: Vec<u8>,
}

impl OpusFramer {
    pub fn new(channels: usize, settings: &StreamSettings) -> Result<Self, opus::Error> {
        let (application, signal) = match settings.content {
            Content::Voice => (Application::Voip, Signal::Voice),
            Content::Music => (Application::Audio, Signal::Music),
        };
        let opus_channels = if channels == 2 {
            Channels::Stereo
        } else {
            Channels::Mono
        };
        let mut encoder = Encoder::new(48_000, opus_channels, application)?;
        let bitrate = settings
            .bitrate
            .clamp(StreamSettings::MIN_BITRATE, StreamSettings::MAX_BITRATE);
        encoder.set_bitrate(Bitrate::Bits(bitrate as i32))?;
        encoder.set_signal(signal)?;
        encoder.set_complexity(10)?;
        encoder.set_vbr(true)?;
        encoder.set_inband_fec(true)?;
        encoder.set_packet_loss_perc(10)?;
        encoder.set_dtx(false)?;
        Ok(Self {
            encoder,
            channels,
            pending: Vec::with_capacity(FRAME_SAMPLES * channels * 4),
            silence: vec![0.0; FRAME_SAMPLES * channels],
            out: vec![0; 4000],
        })
    }

    /// Appends interleaved samples and encodes every complete frame.
    pub fn push(
        &mut self,
        samples: &[f32],
        mut emit: impl FnMut(&[u8]),
    ) -> Result<usize, opus::Error> {
        self.pending.extend_from_slice(samples);
        let frame_len = FRAME_SAMPLES * self.channels;
        let mut frames = 0;
        let mut offset = 0;
        while self.pending.len() - offset >= frame_len {
            let n = self
                .encoder
                .encode_float(&self.pending[offset..offset + frame_len], &mut self.out)?;
            emit(&self.out[..n]);
            offset += frame_len;
            frames += 1;
        }
        self.pending.drain(..offset);
        Ok(frames)
    }

    /// Encodes one frame of digital silence.
    pub fn silence(&mut self, mut emit: impl FnMut(&[u8])) -> Result<(), opus::Error> {
        let n = self.encoder.encode_float(&self.silence, &mut self.out)?;
        emit(&self.out[..n]);
        Ok(())
    }

    /// Drops a partially filled frame, e.g. when input stalls.
    pub fn clear_pending(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_emitted_per_960_samples() {
        let mut f = OpusFramer::new(1, &StreamSettings::default()).unwrap();
        let mut packets = 0;
        let n = f.push(&vec![0.1; 900], |_| packets += 1).unwrap();
        assert_eq!((n, packets), (0, 0));
        let n = f.push(&vec![0.1; 1100], |_| packets += 1).unwrap();
        assert_eq!((n, packets), (2, 2));
        assert_eq!(f.pending.len(), 2000 - 1920);
    }

    #[test]
    fn stereo_frames_consume_interleaved_pairs() {
        let mut f = OpusFramer::new(2, &StreamSettings::default()).unwrap();
        let mut sizes = Vec::new();
        f.push(&vec![0.0; 1920 * 2], |p| sizes.push(p.len()))
            .unwrap();
        assert_eq!(sizes.len(), 2);
        assert!(sizes.iter().all(|&s| s > 0));
    }
}

// SPDX-License-Identifier: Apache-2.0

//! Minimal RIFF/WAVE container.
//!
//! Piper streams raw 16-bit mono PCM; wrapping it in a WAV header lets any
//! browser play the `/voice/speak` response directly (`<audio src=blob>`), with
//! no codec dependency. Opus/Ogg streaming (bandwidth for LAN/home mode) is
//! deferred to Phase 7 (ADR-0009).

/// PCM sample width Piper emits: signed 16-bit.
const BITS_PER_SAMPLE: u16 = 16;
/// Piper voices are mono.
const CHANNELS: u16 = 1;
/// Fixed header length of a canonical PCM WAV (RIFF + fmt + data headers).
pub const HEADER_LEN: usize = 44;

/// Wraps 16-bit mono PCM in a complete little-endian RIFF/WAVE file.
///
/// `sample_rate` is the voice's native rate (e.g. 22050 Hz for Piper medium).
#[must_use]
pub fn wav_from_pcm(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
    let block_align = CHANNELS * (BITS_PER_SAMPLE / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let data_len = u32::try_from(pcm.len()).unwrap_or(u32::MAX);
    // RIFF chunk size = everything after the first 8 bytes = 36 + data.
    let riff_len = 36_u32.saturating_add(data_len);

    let mut out = Vec::with_capacity(HEADER_LEN + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_len.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16_u32.to_le_bytes()); // PCM fmt chunk length
    out.extend_from_slice(&1_u16.to_le_bytes()); // audio format = PCM
    out.extend_from_slice(&CHANNELS.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_well_formed() {
        let pcm = [1_u8, 2, 3, 4];
        let wav = wav_from_pcm(&pcm, 22050);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), HEADER_LEN + pcm.len());
        // data chunk length and RIFF length account for the PCM bytes.
        assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]), 4);
        assert_eq!(u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]]), 40);
    }

    #[test]
    fn sample_rate_and_byte_rate_are_encoded() {
        let wav = wav_from_pcm(&[], 22050);
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            22050
        );
        // byte_rate = sample_rate * channels(1) * bytes_per_sample(2).
        assert_eq!(
            u32::from_le_bytes([wav[28], wav[29], wav[30], wav[31]]),
            22050 * 2
        );
    }
}

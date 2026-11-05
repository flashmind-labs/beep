//! A clip of microphone sound as a WAV file, the one format every provider takes.

/// Bytes in a canonical WAV header.
const HEADER: usize = 44;

/// 16-bit little-endian mono PCM at `rate` Hz, wrapped as a WAV file.
pub fn wrap_pcm16(pcm: &[u8], rate: u32) -> Vec<u8> {
    let data = u32::try_from(pcm.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(HEADER + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&data.saturating_add(36).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    // PCM, one channel.
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    // Bytes per second, then bytes per frame and bits per sample.
    out.extend_from_slice(&rate.saturating_mul(2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(wav: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(wav[at..at + 4].try_into().expect("four bytes"))
    }

    fn u16_at(wav: &[u8], at: usize) -> u16 {
        u16::from_le_bytes(wav[at..at + 2].try_into().expect("two bytes"))
    }

    #[test]
    fn the_header_describes_the_sound() {
        let pcm = [1u8, 0, 2, 0, 3, 0];
        let wav = wrap_pcm16(&pcm, 16_000);
        assert_eq!(wav.len(), HEADER + pcm.len());
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32_at(&wav, 4), 36 + 6);
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u16_at(&wav, 20), 1);
        assert_eq!(u16_at(&wav, 22), 1);
        assert_eq!(u32_at(&wav, 24), 16_000);
        assert_eq!(u32_at(&wav, 28), 32_000);
        assert_eq!(u16_at(&wav, 32), 2);
        assert_eq!(u16_at(&wav, 34), 16);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32_at(&wav, 40), 6);
        assert_eq!(&wav[44..], &pcm);
    }

    #[test]
    fn no_sound_is_a_header_alone() {
        assert_eq!(wrap_pcm16(&[], 16_000).len(), HEADER);
    }
}

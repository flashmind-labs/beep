//! How long a held key's clip may be.

use std::time::Duration;

/// The rate the capture delivers: 16 kHz mono 16-bit PCM.
pub const RATE: u32 = 16_000;
/// Bytes of PCM in one second.
const BYTES_PER_SECOND: usize = RATE as usize * 2;
/// A shorter clip was a slip of the key, not speech.
pub const MIN_CLIP: Duration = Duration::from_millis(300);
/// A longer clip is cut here: the recording stops and what it has is sent.
pub const MAX_CLIP: Duration = Duration::from_secs(300);

/// What a clip's length says to do with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Length {
    TooShort,
    Fine,
}

/// The length of `pcm_bytes` of PCM.
pub fn duration(pcm_bytes: usize) -> Duration {
    let millis = u64::try_from(pcm_bytes / (BYTES_PER_SECOND / 1000)).unwrap_or(u64::MAX);
    Duration::from_millis(millis)
}

/// Whether a clip of `pcm_bytes` is worth sending.
pub fn judge(pcm_bytes: usize) -> Length {
    if duration(pcm_bytes) < MIN_CLIP {
        Length::TooShort
    } else {
        Length::Fine
    }
}

/// The most PCM a clip keeps.
pub fn max_bytes() -> usize {
    usize::try_from(MAX_CLIP.as_secs()).unwrap_or(usize::MAX) * BYTES_PER_SECOND
}

/// Whether `pcm_bytes` reached the cap, so the recording should stop.
pub fn is_full(pcm_bytes: usize) -> bool {
    pcm_bytes >= max_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slip_of_the_key_is_dropped() {
        let ms = |ms: usize| ms * BYTES_PER_SECOND / 1000;
        assert_eq!(judge(0), Length::TooShort);
        assert_eq!(judge(ms(299)), Length::TooShort);
        assert_eq!(judge(ms(300)), Length::Fine);
        assert_eq!(judge(ms(5000)), Length::Fine);
    }

    #[test]
    fn a_clip_stops_at_five_minutes() {
        assert!(!is_full(max_bytes() - 1));
        assert!(is_full(max_bytes()));
        assert_eq!(duration(max_bytes()), MAX_CLIP);
    }
}

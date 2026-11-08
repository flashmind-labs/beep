//! How loud a stretch of microphone sound is.

/// Loudness of 16-bit little-endian PCM from 0 to 1, as the Mac app measures
/// its float samples: speech peaks near 0.3 RMS, so it is scaled up to fill
/// the range. A trailing odd byte is ignored.
pub fn pcm16_level(pcm: &[u8]) -> f32 {
    let (samples, _) = pcm.as_chunks::<2>();
    let count = samples.len();
    if count == 0 {
        return 0.0;
    }
    let sum: f64 = samples
        .iter()
        .map(|&s| {
            let v = f64::from(i16::from_le_bytes(s)) / f64::from(i16::MAX);
            v * v
        })
        .sum();
    let rms = (sum / count as f64).sqrt();
    (rms * 4.0).min(1.0) as f32
}

#[cfg(test)]
#[path = "tests/level.rs"]
mod tests;

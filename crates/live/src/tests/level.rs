use super::*;

fn pcm(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

#[test]
fn silence_is_zero_and_loud_speech_tops_out() {
    assert_eq!(pcm16_level(&[]), 0.0);
    assert_eq!(pcm16_level(&[7]), 0.0);
    assert_eq!(pcm16_level(&pcm(&[0; 100])), 0.0);
    assert_eq!(pcm16_level(&pcm(&[16_384, -16_384, 16_384, -16_384])), 1.0);
    let quiet = pcm16_level(&pcm(&[1_638, -1_638]));
    assert!(quiet > 0.15 && quiet < 0.25, "{quiet}");
}

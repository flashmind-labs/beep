use super::*;

#[test]
fn silence_is_level_zero_and_loud_speech_tops_out() {
    assert_eq!(level(&[]), 0.0);
    assert_eq!(level(&[0.0; 100]), 0.0);
    assert_eq!(level(&[0.5, -0.5, 0.5, -0.5]), 1.0);
    let quiet = level(&[0.05, -0.05]);
    assert!(quiet > 0.1 && quiet < 0.3);
}

#[test]
fn a_second_at_48k_becomes_a_second_at_16k() {
    let out = resample(&vec![0.1; 48_000], 48_000, 16_000);
    assert_eq!(out.len(), 16_000);
    assert!(out.iter().all(|s| (s - 0.1).abs() < 1e-6));
}

#[test]
fn pcm_is_little_endian_and_clamped() {
    assert_eq!(pcm16(&[0.0, 1.0, -2.0]), [0, 0, 0xff, 0x7f, 0x01, 0x80]);
}

#[test]
fn the_envelope_follows_the_answer() {
    let mut samples = vec![0.0; 2400];
    samples.extend(vec![0.5; 2400]);
    let played = Played {
        envelope: envelope(&samples, 24_000.0),
    };
    assert_eq!(played.envelope.len(), 4);
    assert_eq!(played.level_at(Duration::from_millis(10)), 0.0);
    assert_eq!(played.level_at(Duration::from_millis(160)), 1.0);
    assert_eq!(played.level_at(Duration::from_secs(5)), 0.0);
}

use super::*;
use iroh::SecretKey;

fn machine(seed: u8) -> EndpointId {
    SecretKey::from_bytes(&[seed; 32]).public()
}

#[test]
fn a_machine_is_updated_one_request_at_a_time_and_once_per_release() {
    let mut asked = Asked::default();
    let (a, b) = (machine(1), machine(2));
    let v1 = Version::new(0, 9, 0);

    assert!(asked.claim(a, &v1));
    assert!(!asked.claim(a, &v1), "already in flight");
    assert!(asked.claim(b, &v1));

    // A failed request is asked again on the next change.
    asked.finish(a, &v1, false);
    assert!(asked.claim(a, &v1));

    // One the node took is not asked again for that release, but is for the next.
    asked.finish(a, &v1, true);
    assert!(!asked.claim(a, &v1));
    assert!(asked.claim(a, &Version::new(0, 9, 1)));
}

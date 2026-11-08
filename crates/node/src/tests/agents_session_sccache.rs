use super::*;

#[test]
fn cargo_wrapper_and_uncertain_configurations_are_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    assert!(!configured(&config));
    fs::write(&config, "[build]\njobs = 2\n").unwrap();
    assert!(!configured(&config));
    fs::write(&config, "[build]\nrustc-wrapper = 'other'\n").unwrap();
    assert!(configured(&config));
    fs::write(&config, "[env]\nRUSTC_WRAPPER = 'other'\n").unwrap();
    assert!(configured(&config));
    fs::write(&config, "include = 'other.toml'\n").unwrap();
    assert!(configured(&config));
    fs::write(&config, "[invalid").unwrap();
    assert!(configured(&config));
}

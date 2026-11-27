use super::*;

fn version(text: &str) -> Version {
    text.parse().expect("a valid version")
}

#[test]
fn versions_compare_as_numbers() {
    assert!(version("0.1.10") > version("0.1.9"));
    assert!(version("0.2.0") > version("0.1.14"));
    assert!(version("1.0.0") > version("0.99.99"));
    assert!(version("0.1.14") < version("0.1.15"));
    assert_eq!(version("0.1.14"), version("0.1.14"));
    assert_eq!(version("1.2"), version("1.2.0"));
    assert_eq!(version("v0.1.14\n"), version("0.1.14"));
    assert_eq!(version(" 0.1.14 ").to_string(), "0.1.14");
}

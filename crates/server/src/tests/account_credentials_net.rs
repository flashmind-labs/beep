use super::*;

#[test]
fn inside_addresses_are_not_public() {
    for ip in [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "0.0.0.0",
        "::1",
        "fd00:ec2::254",
        "fe80::1",
        "::ffff:10.0.0.1",
        "::10.0.0.1",
        "::127.0.0.1",
        "64:ff9b::a9fe:a9fe",
        "64:ff9b::192.168.0.1",
        "2002:7f00:1::1",
        "2002:a9fe:a9fe::1",
    ] {
        assert!(!is_public(ip.parse().unwrap()), "{ip}");
    }
    for ip in [
        "1.2.3.4",
        "2a00:1::1",
        "64:ff9b::102:304",
        "2002:102:304::1",
    ] {
        assert!(is_public(ip.parse().unwrap()), "{ip}");
    }
}

use super::*;

#[test]
fn only_public_addresses_pass() {
    for ip in [
        "127.0.0.1",
        "10.0.0.2",
        "172.17.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "0.0.0.0",
        "::1",
        "fe80::1",
        "fd00::1",
        "290:78f5::1",
        "2001:db8::1",
        "::ffff:127.0.0.1",
    ] {
        assert!(!is_public(ip.parse().expect("valid ip")), "{ip}");
    }
    for ip in ["93.184.215.14", "2606:4700::1111", "2a01:4f8::1"] {
        assert!(is_public(ip.parse().expect("valid ip")), "{ip}");
    }
}

#[tokio::test]
async fn refuses_private_hosts_without_connecting() {
    for url in [
        "http://127.0.0.1/a.pdf",
        "http://localhost:8080/",
        "file:///etc/passwd",
        "http://[::1]/",
    ] {
        assert!(
            fetch(&Url::parse(url).expect("valid url")).await.is_err(),
            "{url}"
        );
    }
}

#[tokio::test]
async fn ipv6_literals_are_checked_without_a_lookup() {
    let url = Url::parse("https://[2606:4700::1111]/a.pdf").expect("valid url");
    let resolved = public_addresses(&url).await.expect("a public address");
    assert_eq!(resolved.domain, None);
    assert_eq!(
        resolved.addrs,
        [SocketAddr::new(
            "2606:4700::1111".parse().expect("valid ip"),
            443
        )]
    );
    let private = Url::parse("http://[fd00::1]/").expect("valid url");
    assert!(public_addresses(&private).await.is_err());
}

#[test]
fn file_names_stay_in_the_folder() {
    let url = Url::parse("https://example.com/docs/report%20v2.pdf").expect("valid url");
    assert_eq!(file_name(None, &url), "report_20v2.pdf");
    assert_eq!(file_name(Some("../../etc/passwd"), &url), "passwd");
    assert_eq!(file_name(Some(".."), &url), "download");
    assert_eq!(file_name(Some(".hidden"), &url), "hidden");
    let bare = Url::parse("https://example.com/").expect("valid url");
    assert_eq!(file_name(None, &bare), "download");
}

use super::*;
use axum::http::HeaderValue;

fn from(ip: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        CLIENT_IP_HEADER,
        HeaderValue::from_str(ip).expect("an ASCII value"),
    );
    headers
}

#[test]
fn the_header_names_the_client_only_from_cloudflare_or_a_tunnel() {
    // 172.18.0.5 is cloudflared in a container on the compose bridge.
    let client: IpAddr = "198.51.100.1".parse().expect("an address");
    let headers = from("198.51.100.1");
    for peer in [
        "127.0.0.1",
        "::1",
        "172.18.0.5",
        "10.1.2.3",
        "192.168.1.9",
        "169.254.0.2",
        "fd12::7",
        "fe80::1",
        "104.16.1.2",
        "2606:4700::1",
        "::ffff:172.64.0.9",
    ] {
        let peer: IpAddr = peer.parse().expect("an address");
        assert_eq!(client_ip(&headers, Some(peer)), Some(client), "{peer}");
    }
    for peer in ["203.0.113.5", "104.32.0.1", "2001:db8::1"] {
        let peer: IpAddr = peer.parse().expect("an address");
        assert_eq!(client_ip(&headers, Some(peer)), Some(peer), "{peer}");
    }

    assert_eq!(client_ip(&headers, None), None);
}

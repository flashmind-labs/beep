//! Fetches files from public http(s) URLs only, so a URL cannot reach the server's network.

use anyhow::{Context, Result, bail, ensure};
use futures::StreamExt;
use reqwest::{
    Client, Response, StatusCode, Url,
    header::{CONTENT_TYPE, LOCATION},
    redirect::Policy,
};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};
use tokio::net::lookup_host;

/// Largest file the agent fetches, saves or sends.
pub const MAX_DOWNLOAD: u64 = 20 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

/// The final answer to a request after its redirects, and where it came from.
pub(crate) struct Followed {
    pub(crate) response: Response,
    pub(crate) url: Url,
}

/// Fetches `url`, and checks the answer is a file of at most `MAX_DOWNLOAD`.
pub(crate) async fn follow(url: &str) -> Result<Followed> {
    let followed = follow_public(url).await?;
    let response = &followed.response;
    if response.status() != StatusCode::OK {
        bail!("the server answered {}", response.status());
    }
    if response.content_length().is_some_and(|n| n > MAX_DOWNLOAD) {
        bail!("the file is larger than {} MiB", MAX_DOWNLOAD >> 20);
    }
    Ok(followed)
}

/// Fetches `url`, following up to `MAX_REDIRECTS` public redirects; the body is left unread.
pub(crate) async fn follow_public(url: &str) -> Result<Followed> {
    let mut url = Url::parse(url).context("not a valid URL")?;
    let mut response = None;
    for _ in 0..=MAX_REDIRECTS {
        let reply = fetch(&url).await?;
        if !reply.status().is_redirection() {
            response = Some(reply);
            break;
        }
        let location = reply
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .context("redirect without a location")?;
        url = url.join(location).context("bad redirect")?;
    }
    let response = response.context("too many redirects")?;
    Ok(Followed { response, url })
}

/// The response's `Content-Type`, when it is readable text.
pub(crate) fn content_type(response: &Response) -> Option<String> {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// A file fetched from a public URL.
pub(crate) struct Fetched {
    /// The URL's last path segment, cleaned up.
    pub(crate) name: String,
    pub(crate) content_type: Option<String>,
    pub(crate) data: Vec<u8>,
}

/// Fetches at most `MAX_DOWNLOAD` from a public URL.
pub(crate) async fn fetch_public(url: &str) -> Result<Fetched> {
    let Followed { response, url } = follow(url).await?;
    let content_type = content_type(&response);
    let mut data = Vec::new();
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.context("the download broke off")?;
        ensure!(
            (data.len() + chunk.len()) as u64 <= MAX_DOWNLOAD,
            "the file is larger than {} MiB",
            MAX_DOWNLOAD >> 20
        );
        data.extend_from_slice(&chunk);
    }
    Ok(Fetched {
        name: file_name(None, &url),
        content_type,
        data,
    })
}

/// One request to `url` without redirects, pinned to its checked public address.
async fn fetch(url: &Url) -> Result<Response> {
    let Resolved { domain, addrs } = public_addresses(url).await?;
    let mut client = Client::builder().redirect(Policy::none());
    if let Some(domain) = domain {
        client = client.resolve(domain, addrs[0]);
    }
    let client = client
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .user_agent("Beep")
        .build()?;
    Ok(client.get(url.clone()).send().await?)
}

/// A URL's domain, if any, and the addresses to connect to (never empty).
#[derive(Debug)]
struct Resolved<'a> {
    domain: Option<&'a str>,
    addrs: Vec<SocketAddr>,
}

/// Where `url` points, checked to be public addresses only.
async fn public_addresses(url: &Url) -> Result<Resolved<'_>> {
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "only http and https URLs are allowed"
    );
    ensure!(url.as_str().len() <= 2048, "the URL is too long");
    let host = url.host_str().context("the URL has no host")?;
    let port = url.port_or_known_default().context("the URL has no port")?;
    // An IPv6 address comes in brackets, which a lookup would not take.
    let literal = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    let (domain, addrs) = match literal.parse::<IpAddr>() {
        Ok(ip) => (None, vec![SocketAddr::new(ip, port)]),
        Err(_) => (
            Some(host),
            lookup_host((host, port))
                .await
                .context("the host does not resolve")?
                .collect(),
        ),
    };
    ensure!(!addrs.is_empty(), "the host does not resolve");
    ensure!(
        addrs.iter().all(|a| is_public(a.ip())),
        "the host is not on the public internet"
    );
    Ok(Resolved { domain, addrs })
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(ip),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || a == 0
        || a >= 240
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (18..20).contains(&b)))
}

/// Global unicast only (2000::/3), which keeps out mesh overlays in 200::/7.
fn is_public_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    first & 0xe000 == 0x2000 && first != 0x2001 || {
        // 2001::/16 holds both public space and documentation/Teredo ranges.
        first == 0x2001 && !matches!(ip.segments()[1], 0x0000 | 0x0db8)
    }
}

/// A plain file name from `name` or the URL's last path segment.
pub(crate) fn file_name(name: Option<&str>, url: &Url) -> String {
    let raw = name
        .filter(|n| !n.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            url.path_segments()
                .and_then(|mut s| s.next_back().map(str::to_owned))
        })
        .unwrap_or_default();
    clean_file_name(&raw)
}

/// `raw`'s last path segment with anything but letters, digits, `.`, `-` and `_` replaced.
pub(crate) fn clean_file_name(raw: &str) -> String {
    let cleaned: String = raw
        .rsplit('/')
        .next()
        .unwrap_or("")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(100)
        .collect();
    let cleaned = cleaned.trim_start_matches('.');
    if cleaned.is_empty() {
        "download".into()
    } else {
        cleaned.to_owned()
    }
}

#[cfg(test)]
mod tests {
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
}

//! Share, group and OAuth callback links.

/// The web host share and invite links point at. The Cloudflare Worker in
/// `deploy/links` serves it, and the Android manifest claims it for App Links.
pub(crate) const LINK_HOST: &str = "beep.flashmind.com";

/// The link to the shared session `token`.
pub fn share_url(token: &str) -> String {
    format!("https://{LINK_HOST}/s/{token}")
}

/// The token in a shared session's link, if `url` is one. Old `beep://share/`
/// links still read.
pub fn share_token(url: &str) -> Option<&str> {
    link_token(url, "s", "beep://share/")
}

/// Where MCP sign-ins come back. The Worker hands it to the app: Android
/// claims it as an App Link, and the page sends other apps on to
/// `beep://oauth?...`.
pub fn mcp_callback_url() -> String {
    format!("https://{LINK_HOST}/oauth/callback")
}

/// What a sign-in redirect brought back.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCallback {
    pub code: String,
    pub state: String,
    pub issuer: Option<String>,
}

/// The sign-in result in `url`, if it is `mcp_callback_url()` or
/// `beep://oauth` with a `code` and a `state`.
pub fn mcp_callback(url: &str) -> Option<McpCallback> {
    let url = url.trim();
    let query = url
        .strip_prefix(&mcp_callback_url())
        .or_else(|| url.strip_prefix("beep://oauth"))?;
    let query = query.strip_prefix('/').unwrap_or(query).strip_prefix('?')?;
    let (mut code, mut state, mut issuer) = (None, None, None);
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = percent_decode(value)?;
        match key {
            "code" => code = Some(value),
            "state" => state = Some(value),
            "iss" => issuer = Some(value),
            _ => {}
        }
    }
    Some(McpCallback {
        code: code.filter(|code| !code.is_empty())?,
        state: state.filter(|state| !state.is_empty())?,
        issuer,
    })
}

/// The token in a `beep://login?token=...` link.
pub fn login_token(url: &str) -> Option<String> {
    let query = url.trim().strip_prefix("beep://login")?;
    let query = query.strip_prefix('/').unwrap_or(query).strip_prefix('?')?;
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
        .and_then(percent_decode)
        .filter(|token| !token.is_empty())
}

/// A query value with `%XX` escapes and `+` decoded, or `None` when it is
/// not valid UTF-8 after.
fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 2;
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// The invite link with `token`.
pub fn group_url(token: &str) -> String {
    format!("https://{LINK_HOST}/g/{token}")
}

/// The token in a group invite link, if `url` is one. Old `beep://group/`
/// links still read.
pub fn group_token(url: &str) -> Option<&str> {
    link_token(url, "g", "beep://group/")
}

/// The token after `https://LINK_HOST/<path>/` or the older `legacy` prefix.
fn link_token<'a>(url: &'a str, path: &str, legacy: &str) -> Option<&'a str> {
    let url = url.trim();
    let token = url.strip_prefix(legacy).or_else(|| {
        url.strip_prefix("https://")?
            .strip_prefix(LINK_HOST)?
            .strip_prefix('/')?
            .strip_prefix(path)?
            .strip_prefix('/')
    })?;
    let token = token.strip_suffix('/').unwrap_or(token);
    (!token.is_empty() && token.bytes().all(|b| b.is_ascii_alphanumeric())).then_some(token)
}

/// The invite code in what someone typed or pasted: a code with any case,
/// spaces or dashes, or an invite link. None for other text.
pub fn group_code(input: &str) -> Option<String> {
    let code = plain_code(group_token(input).unwrap_or(input.trim()));
    let valid = (4..=64).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_alphanumeric());
    valid.then_some(code)
}

/// A typed code in capitals, without spaces or dashes.
pub(crate) fn plain_code(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// `code` as people read it: `K7QM4-X9TRP` for a ten-character code.
pub fn format_group_code(code: &str) -> String {
    match code.len() {
        10 if code.is_ascii() => format!("{}-{}", &code[..5], &code[5..]),
        _ => code.to_owned(),
    }
}

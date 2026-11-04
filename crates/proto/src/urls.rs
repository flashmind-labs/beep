//! Share, group and OAuth callback links.

/// The web host share and invite links point at. The Cloudflare Worker in
/// `deploy/links` serves it, and the Android manifest claims it for App Links.
pub const LINK_HOST: &str = "beep.flashmind.com";

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_login_link_gives_its_token() {
        assert_eq!(
            login_token("beep://login?token=abc.def").as_deref(),
            Some("abc.def")
        );
        assert_eq!(
            login_token("beep://login/?x=1&token=a%2Db.c").as_deref(),
            Some("a-b.c")
        );
        assert_eq!(login_token("beep://login"), None);
        assert_eq!(login_token("beep://login?token="), None);
        assert_eq!(login_token("beep://oauth?token=abc"), None);
    }

    #[test]
    fn share_token_reads_only_share_links() {
        assert_eq!(share_url("ab12"), "https://beep.flashmind.com/s/ab12");
        assert_eq!(
            mcp_callback(
                "https://beep.flashmind.com/oauth/callback?code=a%2Fb&state=s1&iss=https%3A%2F%2Fid.example"
            ),
            Some(McpCallback {
                code: "a/b".to_owned(),
                state: "s1".to_owned(),
                issuer: Some("https://id.example".to_owned()),
            })
        );
        assert_eq!(
            mcp_callback("beep://oauth?state=s1&code=c"),
            Some(McpCallback {
                code: "c".to_owned(),
                state: "s1".to_owned(),
                issuer: None,
            })
        );
        assert_eq!(
            mcp_callback("https://beep.flashmind.com/oauth/callback?code=c"),
            None
        );
        assert_eq!(
            mcp_callback("https://beep.flashmind.com.evil/oauth/callback?code=c&state=s"),
            None
        );
        assert_eq!(mcp_callback("beep://oauth?code=%zz&state=s"), None);
        assert_eq!(share_token(&share_url("ab12")), Some("ab12"));
        assert_eq!(
            share_token(" https://beep.flashmind.com/s/ab12/\n"),
            Some("ab12")
        );
        assert_eq!(share_token(" beep://share/ab12\n"), Some("ab12"));
        assert_eq!(share_token("beep://share/"), None);
        assert_eq!(share_token("beep://share/../x"), None);
        assert_eq!(share_token("https://beep.flashmind.com/s/"), None);
        assert_eq!(share_token("https://beep.flashmind.com.evil/s/ab12"), None);
        assert_eq!(share_token("http://beep.flashmind.com/s/ab12"), None);
        assert_eq!(share_token("beep://apps"), None);
        assert_eq!(group_token(&group_url("cd34")), Some("cd34"));
        assert_eq!(group_token("beep://group/cd34"), Some("cd34"));
        assert_eq!(group_token(&share_url("cd34")), None);
    }

    #[test]
    fn group_code_reads_typed_codes_and_old_links() {
        assert_eq!(group_code("k7qm4-x9trp").as_deref(), Some("K7QM4X9TRP"));
        assert_eq!(group_code(" K7QM4 X9TRP\n").as_deref(), Some("K7QM4X9TRP"));
        assert_eq!(
            group_code(&group_url("7a2d5f90be13")).as_deref(),
            Some("7A2D5F90BE13")
        );
        assert_eq!(
            group_code("beep://group/7a2d5f90be13").as_deref(),
            Some("7A2D5F90BE13")
        );
        assert_eq!(group_code("hey there!"), None);
        assert_eq!(group_code("ab"), None);
        assert_eq!(group_code(&share_url("cd34ef")), None);
        assert_eq!(format_group_code("K7QM4X9TRP"), "K7QM4-X9TRP");
        assert_eq!(format_group_code("7A2D5F90BE13"), "7A2D5F90BE13");
    }
}

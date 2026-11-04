//! Names of the per-owner GitHub token variables.

/// What [`github_owner_var`] names start with.
const GITHUB_OWNER_PREFIX: &str = "BEEP_GH_TOKEN_";

/// The env var for `owner`'s token when not the default account's, such as `BEEP_GH_TOKEN_ACME_LABS`.
pub fn github_owner_var(owner: &str) -> Option<String> {
    let valid = !owner.is_empty()
        && owner.len() <= 39
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    valid.then(|| {
        let name: String = owner
            .chars()
            .map(|c| match c {
                '-' => '_',
                c => c.to_ascii_uppercase(),
            })
            .collect();
        format!("{GITHUB_OWNER_PREFIX}{name}")
    })
}

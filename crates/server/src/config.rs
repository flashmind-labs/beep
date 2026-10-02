use std::{collections::HashSet, path::PathBuf};

use anyhow::{Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub secret_key: Option<PathBuf>,
    /// Explicit shared account for credential-free development. Disabled by default.
    pub testing_user: Option<String>,
    pub database: PathBuf,
    pub model: String,
    #[serde(default)]
    pub provider_url: Option<String>,
    pub provider_key: Option<String>,
    pub firecrawl_key: Option<String>,
    pub composio_key: Option<String>,
    pub fastmail_token: Option<String>,
    pub users: Vec<UserConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserConfig {
    pub id: String,
    pub token: String,
}

pub struct Auth {
    users: Vec<(String, [u8; 32])>,
    testing_user: Option<String>,
}

impl Auth {
    pub fn new(users: Vec<(String, String)>) -> Result<Self> {
        ensure!(!users.is_empty(), "at least one user is required");
        let mut ids = HashSet::new();
        let mut hashes = HashSet::new();
        let mut result = Vec::new();
        for (id, token) in users {
            ensure!(
                !id.is_empty() && ids.insert(id.clone()),
                "user IDs must be unique and nonempty"
            );
            ensure!(
                token.len() >= 32 && !token.chars().any(char::is_whitespace),
                "tokens must contain at least 32 non-whitespace characters"
            );
            let hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
            ensure!(hashes.insert(hash), "each user must have a distinct token");
            result.push((id, hash));
        }
        Ok(Self {
            users: result,
            testing_user: None,
        })
    }

    pub fn from_config(config: &Config) -> Result<Self> {
        let mut auth = Self::new(
            config
                .users
                .iter()
                .map(|user| (user.id.clone(), user.token.clone()))
                .collect(),
        )?;
        if let Some(user) = &config.testing_user {
            ensure!(
                auth.users.iter().any(|(id, _)| id == user),
                "testing_user must name a configured user"
            );
            auth.testing_user = Some(user.clone());
        }
        Ok(auth)
    }

    pub fn is_testing(&self) -> bool {
        self.testing_user.is_some()
    }

    pub fn authenticate(&self, header: Option<&str>) -> Option<String> {
        if let Some(user) = &self.testing_user {
            return Some(user.clone());
        }
        let token = header?.strip_prefix("Bearer ")?;
        let hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        self.users
            .iter()
            .find_map(|(user, expected)| bool::from(expected.ct_eq(&hash)).then(|| user.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_required_unless_testing_is_explicit() {
        let mut auth = Auth::new(vec![(
            "development".into(),
            "a-valid-test-token-at-least-32-characters".into(),
        )])
        .unwrap();
        assert!(!auth.is_testing());
        assert_eq!(auth.authenticate(None), None);
        assert_eq!(auth.authenticate(Some("Bearer wrong")), None);
        auth.testing_user = Some("development".into());
        assert!(auth.is_testing());
        assert_eq!(auth.authenticate(None).as_deref(), Some("development"));
        assert_eq!(
            auth.authenticate(Some("Bearer old-client-token"))
                .as_deref(),
            Some("development")
        );
    }
}

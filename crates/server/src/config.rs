use std::{collections::HashSet, io, path::Path, path::PathBuf};

use anyhow::{Context, Result, ensure};
use flashmind::types::{ProviderPreferences, ProviderSort, Quantization, ReasoningLevel};
use iroh::SecretKey;
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
    /// How much the assistant thinks before answering: off, low, medium or high.
    #[serde(default = "default_reasoning")]
    pub reasoning: ReasoningLevel,
    /// OpenRouter provider routing. Replaces the default entirely when set.
    #[serde(default = "default_routing")]
    pub routing: ProviderPreferences,
    pub provider_key: Option<String>,
    pub firecrawl_key: Option<String>,
    pub composio_key: Option<String>,
    /// Where Composio sends the browser after an app connects, for example a
    /// deep link back into the phone app. Composio shows its own page if unset.
    pub composio_callback_url: Option<String>,
    pub users: Vec<UserConfig>,
}

fn default_reasoning() -> ReasoningLevel {
    ReasoningLevel::Low
}

/// Fastest provider that keeps no data and serves at least fp8.
fn default_routing() -> ProviderPreferences {
    ProviderPreferences {
        data_collection: Some("deny".into()),
        quantizations: vec![
            Quantization::Fp8,
            Quantization::Fp16,
            Quantization::Bf16,
            Quantization::Fp32,
            Quantization::Other("unknown".into()),
        ],
        sort: Some(ProviderSort::Throughput),
        ..Default::default()
    }
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

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn load_secret_key(path: Option<&Path>) -> Result<SecretKey> {
    let Some(path) = path else {
        return Ok(SecretKey::generate());
    };
    match std::fs::read_to_string(path) {
        Ok(data) => Ok(data.trim().parse::<SecretKey>()?),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let key = SecretKey::generate();
            ensure_parent_dir(path)?;
            let hex: String = key.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
            std::fs::write(path, &hex)?;
            log::info!("generated new secret key at {}", path.display());
            Ok(key)
        }
        Err(err) => Err(err).context("reading secret key"),
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

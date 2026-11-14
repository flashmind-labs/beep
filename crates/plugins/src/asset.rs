use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use mime_guess::Mime;
use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

/// The largest image file a plugin may use.
pub(crate) const MAX_IMAGE_BYTES: u64 = 256 * 1024;
/// All distinct image files one plugin may use.
pub(crate) const MAX_PLUGIN_ASSET_BYTES: usize = 4 * 1024 * 1024;

const HASH_LEN: usize = 64;

#[derive(Debug, Error)]
pub(crate) enum ImageError {
    #[error("path must be relative")]
    Absolute,
    #[error("path leaves the plugin folder")]
    Outside,
    #[error("cannot read: {0}")]
    Io(#[from] std::io::Error),
    #[error("not an image file")]
    NotImage,
    #[error("{0} bytes is over the {MAX_IMAGE_BYTES} byte limit")]
    TooLarge(u64),
    #[error("the plugin's images total over {MAX_PLUGIN_ASSET_BYTES} bytes")]
    TooMuch,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid asset hash {0:?}: expected 64 lowercase hex digits")]
pub struct InvalidAssetHash(String);

/// Names an image by its content: the BLAKE3 hash as 64 lowercase hex digits.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AssetHash(String);

impl AssetHash {
    fn of(bytes: &[u8]) -> Self {
        Self(blake3::hash(bytes).to_hex().to_string())
    }
}

impl FromStr for AssetHash {
    type Err = InvalidAssetHash;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let hex = text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if text.len() == HASH_LEN && hex {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidAssetHash(text.to_owned()))
        }
    }
}

impl TryFrom<String> for AssetHash {
    type Error = InvalidAssetHash;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse()
    }
}

impl From<AssetHash> for String {
    fn from(hash: AssetHash) -> Self {
        hash.0
    }
}

impl fmt::Display for AssetHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An image file a plugin uses. Cloning shares the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    hash: AssetHash,
    mime: Mime,
    bytes: Arc<[u8]>,
}

impl Asset {
    fn new(mime: Mime, bytes: Vec<u8>) -> Self {
        Self {
            hash: AssetHash::of(&bytes),
            mime,
            bytes: bytes.into(),
        }
    }

    pub fn hash(&self) -> &AssetHash {
        &self.hash
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// What a node carries: enough for the page to ask for the bytes and type them.
    pub fn reference(&self) -> AssetRef {
        AssetRef {
            hash: self.hash.clone(),
            mime: self.mime.clone(),
        }
    }
}

/// How a published node names an image. The page fetches the bytes once per hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssetRef {
    pub hash: AssetHash,
    #[serde(serialize_with = "mime_text")]
    pub mime: Mime,
}

fn mime_text<S: Serializer>(mime: &Mime, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(mime)
}

/// Reads one plugin's image files, each path once, within the plugin's total limit.
pub(crate) struct Assets {
    root: PathBuf,
    by_path: HashMap<String, Asset>,
    distinct: HashSet<AssetHash>,
    total: usize,
}

impl Assets {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            by_path: HashMap::new(),
            distinct: HashSet::new(),
            total: 0,
        }
    }

    /// The image at `src`, a path inside the plugin folder.
    pub(crate) fn load(&mut self, src: &str) -> Result<Asset, ImageError> {
        if let Some(asset) = self.by_path.get(src) {
            return Ok(asset.clone());
        }
        if !Path::new(src).is_relative() {
            return Err(ImageError::Absolute);
        }

        let path = self.root.join(src).canonicalize()?;
        if !path.starts_with(&self.root) {
            return Err(ImageError::Outside);
        }
        let mime = mime_guess::from_path(&path)
            .first()
            .filter(|mime| mime.type_() == mime_guess::mime::IMAGE)
            .ok_or(ImageError::NotImage)?;
        let size = std::fs::metadata(&path)?.len();
        if size > MAX_IMAGE_BYTES {
            return Err(ImageError::TooLarge(size));
        }

        let asset = Asset::new(mime, std::fs::read(&path)?);
        if self.distinct.insert(asset.hash.clone()) {
            self.total = self.total.saturating_add(asset.bytes.len());
            if self.total > MAX_PLUGIN_ASSET_BYTES {
                return Err(ImageError::TooMuch);
            }
        }
        self.by_path.insert(src.to_owned(), asset.clone());

        Ok(asset)
    }
}

#[cfg(test)]
#[path = "tests/asset.rs"]
mod tests;

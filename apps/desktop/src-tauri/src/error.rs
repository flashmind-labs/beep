use beep_proto::ApiError;
use serde::{Serialize, Serializer};

/// An error a command hands the bar, which shows its text.
#[derive(Debug, thiserror::Error)]
pub enum DesktopError {
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("not linked to a phone yet")]
    Unlinked,
    #[error("this computer was removed from the account")]
    Revoked,
    #[error("{0}")]
    BadRequest(String),
    #[error("{0:#}")]
    Other(anyhow::Error),
}

impl From<anyhow::Error> for DesktopError {
    fn from(e: anyhow::Error) -> Self {
        match e.downcast::<ApiError>() {
            Ok(api) => Self::Api(api),
            Err(e) => Self::Other(e),
        }
    }
}

impl From<tauri::Error> for DesktopError {
    fn from(e: tauri::Error) -> Self {
        Self::Other(e.into())
    }
}

impl Serialize for DesktopError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T, E = DesktopError> = std::result::Result<T, E>;

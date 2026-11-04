use beep_mesh::MeshError;
use beep_projects::{imports::ImportError, issues::SourceError};
use beep_proto::ApiError;
use thiserror::Error;

/// Why a call to the mesh, a machine or a git host failed. The text is what the user is told.
#[derive(Debug, Error)]
pub enum FleetError {
    /// A machine of the mesh that is not connected.
    #[error("{0} is offline")]
    Offline(String),
    /// The mesh is not open: still opening, or it could not be.
    #[error("the mesh is not open: {0}")]
    NoMesh(String),
    #[error(transparent)]
    Mesh(#[from] MeshError),
    #[error(transparent)]
    Import(#[from] ImportError),
    /// A request that cannot be done, in words for the user.
    #[error("{0}")]
    Refused(String),
    #[error("not found")]
    NotFound,
    /// A call only a server answers, made with none.
    #[error("no server")]
    NeedsServer,
    #[error("{0:#}")]
    Other(#[from] anyhow::Error),
}

pub type Result<T, E = FleetError> = std::result::Result<T, E>;

impl FleetError {
    pub(crate) fn refused(text: impl Into<String>) -> Self {
        Self::Refused(text.into())
    }
}

impl From<ApiError> for FleetError {
    fn from(e: ApiError) -> Self {
        match e {
            ApiError::BadRequest { message } => Self::Refused(message),
            ApiError::NotFound {} => Self::NotFound,
            ApiError::Conflict { message } => Self::Refused(message),
            other => Self::Other(other.into()),
        }
    }
}

impl From<SourceError> for FleetError {
    fn from(e: SourceError) -> Self {
        match e {
            SourceError::Refused(text) => Self::Refused(text),
            SourceError::Store(e) => Self::Other(e),
        }
    }
}

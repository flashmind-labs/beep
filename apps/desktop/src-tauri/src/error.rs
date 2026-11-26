use beep_proto::ApiError;
use serde::{Serialize, Serializer};

/// An error a command hands the bar, which shows its text.
#[derive(Debug, thiserror::Error)]
pub enum DesktopError {
    #[error("{0}")]
    Api(#[from] ApiError),
    /// A machine the mesh does not hold.
    #[error("That machine is not in the mesh.")]
    NotInMesh,
    /// The mesh is not open: still opening, or it could not be.
    #[error("the mesh is not open: {0}")]
    NoMesh(String),
    /// A mesh operation failed; the text is the mesh's own.
    #[error("{0}")]
    Mesh(#[from] beep_mesh::MeshError),
    /// A machine of the mesh that is not connected.
    #[error("{0} is offline")]
    Offline(String),
    /// The link to a machine dropped in the middle of a call.
    #[error("Lost the connection to {0}.")]
    Lost(String),
    /// The link to a machine dropped after an order went: it may have taken it.
    #[error("Lost the connection to {0} while sending.")]
    Unsure(String),
    #[error("{0}")]
    BadRequest(String),
    /// A machine took longer than a request's limit; the text says which.
    #[error("{0}")]
    TimedOut(String),
    #[error("{0:#}")]
    Other(anyhow::Error),
}

impl DesktopError {
    /// Whether the machine could not be reached or the link to it dropped,
    /// as opposed to an answer it gave: the call can be made again.
    pub fn is_transport(&self) -> bool {
        match self {
            Self::Mesh(e) => e.is_transport(),
            Self::Offline(_) | Self::Lost(_) => true,
            _ => false,
        }
    }
}

impl From<anyhow::Error> for DesktopError {
    fn from(e: anyhow::Error) -> Self {
        match e.downcast::<ApiError>() {
            Ok(api) => Self::Api(api),
            Err(e) => Self::Other(e),
        }
    }
}

impl From<beep_fleet::FleetError> for DesktopError {
    fn from(e: beep_fleet::FleetError) -> Self {
        use beep_fleet::FleetError;

        match e {
            FleetError::Offline(name) => Self::Offline(name),
            FleetError::NoMesh(why) => Self::NoMesh(why),
            FleetError::Mesh(e) => Self::Mesh(e),
            FleetError::NotFound => Self::Api(ApiError::NotFound {}),
            FleetError::Conflict(message) => Self::Api(ApiError::Conflict { message }),
            FleetError::Refused(text) => Self::BadRequest(text),
            FleetError::Import(e) => Self::BadRequest(e.to_string()),
            e @ FleetError::NotImported(_) => Self::BadRequest(e.to_string()),
            FleetError::Other(e) => Self::Other(e),
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

/// A machine's reason on its own, without the "bad request:" before it.
pub(crate) fn plain(e: DesktopError) -> DesktopError {
    match e {
        DesktopError::Api(ApiError::BadRequest { message }) => DesktopError::BadRequest(message),
        other => other,
    }
}

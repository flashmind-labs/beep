//! Reading and writing files for the rest of the node: platform file calls
//! (`fsat`), small state files (`note`) and path handling (`paths`).

pub(crate) mod fsat;
pub(crate) mod git_read;
pub(crate) mod note;
pub(crate) mod paths;

//! The shared teacli check, run against a fake shell.

use crate::protocol::CredentialCheck;
use beep_projects::{
    credential_check::machines::{Reach, check_from},
    model::ForgejoUrl,
};
use std::sync::Arc;

#[cfg(test)]
#[path = "../../tests/account_credentials_machines.rs"]
mod tests;

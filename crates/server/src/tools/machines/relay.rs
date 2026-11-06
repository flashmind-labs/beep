//! Machine work for the assistant and the phones, run by one of the user's desktops.
//!
//! The server reaches no machine itself. It hands the work to a desktop of the same account
//! that has its events stream open; the desktop asks the user, runs it over the mesh and sends
//! the result back. Only `Exec` exists so far: file reads, coding agents and terminals join
//! `Work` as the relay grows. The request that carries work to the desktop is not built yet.

use iroh::EndpointId;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

/// What a desktop is asked to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Work {
    /// A shell command on `machine` (the desktop's choice when `None`), started in `cwd`.
    Exec {
        command: String,
        machine: Option<String>,
        cwd: Option<String>,
    },
}

pub(crate) const NO_DESKTOP: &str = "No desktop is online to run this.";
const NOT_BUILT: &str = "Running this through a desktop is not built yet.";

/// The desktops with an events stream open on this instance, by user.
#[derive(Clone, Default)]
pub struct Desktops(Arc<Mutex<HashMap<String, Vec<EndpointId>>>>);

impl Desktops {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Vec<EndpointId>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Notes `key`'s stream for `user` open until the guard drops.
    pub fn open(&self, user: &str, key: EndpointId) -> Open {
        self.lock().entry(user.to_owned()).or_default().push(key);
        Open {
            desktops: self.clone(),
            user: user.to_owned(),
            key,
        }
    }

    /// One desktop of `user`'s with its stream open, the latest to connect.
    pub(crate) fn online(&self, user: &str) -> Option<EndpointId> {
        self.lock().get(user)?.last().copied()
    }
}

/// A desktop's open events stream; dropping it marks the stream closed.
pub struct Open {
    desktops: Desktops,
    user: String,
    key: EndpointId,
}

impl Drop for Open {
    fn drop(&mut self) {
        let mut open = self.desktops.lock();
        if let Some(keys) = open.get_mut(&self.user) {
            if let Some(at) = keys.iter().position(|key| *key == self.key) {
                keys.remove(at);
            }
            if keys.is_empty() {
                open.remove(&self.user);
            }
        }
    }
}

/// Runs `work` for `user` through one of their online desktops; the error is for the model.
pub(crate) async fn run(desktops: &Desktops, user: &str, work: Work) -> Result<String, String> {
    let Some(desktop) = desktops.online(user) else {
        return Err(NO_DESKTOP.to_owned());
    };
    log::info!("relay: {desktop} is online, but work cannot reach it yet; work={work:?}");
    Err(NOT_BUILT.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn exec() -> Work {
        Work::Exec {
            command: "ls".into(),
            machine: None,
            cwd: None,
        }
    }

    #[tokio::test]
    async fn work_needs_a_desktop_with_its_stream_open() {
        let desktops = Desktops::default();
        assert_eq!(
            run(&desktops, "u", exec()).await,
            Err(NO_DESKTOP.to_owned())
        );
        let open = desktops.open("u", SecretKey::generate().public());
        assert_eq!(run(&desktops, "u", exec()).await, Err(NOT_BUILT.to_owned()));
        assert_eq!(
            run(&desktops, "other", exec()).await,
            Err(NO_DESKTOP.to_owned())
        );
        drop(open);
        assert_eq!(
            run(&desktops, "u", exec()).await,
            Err(NO_DESKTOP.to_owned())
        );
    }
}

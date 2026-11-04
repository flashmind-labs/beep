//! `search/1` (see `beep_proto::node::search`): where the node looks for
//! git checkouts, and the walk that finds them.

use crate::{
    AsyncMutex,
    agents::Agents,
    found,
    link::{Protocol, Session},
    state::{self, Search},
};
use anyhow::Result;
use beep_proto::node::{
    LinkRx, LinkTx,
    search::{PROTOCOL, Reply, Request},
};
use futures::future::OptionFuture;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::task::spawn_blocking;
use tokio_util::task::AbortOnDropHandle;

/// Serves `search/1` and owns the walk.
#[derive(Clone)]
pub struct Searches(Arc<Inner>);

struct Inner {
    dir: PathBuf,
    agents: Arc<Agents>,
    /// The running or last walk; held while the folder changes, so changes
    /// go one at a time.
    walk: AsyncMutex<Option<AbortOnDropHandle<()>>>,
}

impl Searches {
    /// Reads the checkouts saved in the node's folder `dir`, walking
    /// `search` in the background when none are saved.
    pub fn start(dir: PathBuf, agents: Arc<Agents>, search: &Search) -> Self {
        let walk = walk(&agents, search.root(), &dir);
        Self(Arc::new(Inner {
            dir,
            agents,
            walk: AsyncMutex::new(Some(walk)),
        }))
    }

    async fn answer(&self, request: Request) -> Reply {
        let done = match request {
            Request::Get {} => {
                let dir = self.0.dir.clone();
                blocking(move || state::saved_search(&dir)).await
            }
            Request::Set { folder } => self.set(Search::parse(&folder)).await,
            _ => return failed("Update beep-node on this machine first.".into()),
        };
        match done {
            Ok(search) => Reply::Folder {
                folder: search.to_string(),
            },
            Err(e) => failed(format!("{e:#}")),
        }
    }

    /// Saves `search`, dropping the checkouts found before, and walks it
    /// again in the background.
    async fn set(&self, search: Search) -> Result<Search> {
        let inner = &self.0;
        let mut walking = inner.walk.lock().await;
        let (dir, saving) = (inner.dir.clone(), search.clone());
        let root = blocking(move || {
            let root = saving.existing_root()?;
            state::save_search(&dir, &saving)?;
            Ok(root)
        })
        .await?;
        log::info!("looking for checkouts in {search}");
        // A walk still running saves what it found in the old folder when
        // it ends: the new one waits for it, then drops that.
        let before = walking.take();
        let (agents, dir) = (inner.agents.clone(), inner.dir.clone());
        *walking = Some(AbortOnDropHandle::new(tokio::spawn(async move {
            if let Some(Err(e)) = OptionFuture::from(before).await {
                log::debug!("the last walk ended: {e}");
            }
            let forget = dir.clone();
            match blocking(move || found::forget_checkouts(&forget)).await {
                Ok(()) => agents.warm(Some(root), dir).await,
                Err(e) => log::warn!("not looking for checkouts again: {e:#}"),
            }
        })));
        Ok(search)
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    spawn_blocking(work).await?
}

/// Has `agents` read the checkouts saved in `dir`, walking `root` when
/// none are saved.
fn walk(agents: &Arc<Agents>, root: Option<PathBuf>, dir: &Path) -> AbortOnDropHandle<()> {
    let (agents, dir) = (agents.clone(), dir.to_owned());
    AbortOnDropHandle::new(tokio::spawn(async move { agents.warm(root, dir).await }))
}

fn failed(reason: String) -> Reply {
    Reply::Failed { reason }
}

impl Protocol for Searches {
    fn name(&self) -> &str {
        PROTOCOL
    }

    fn serve(&self, mut tx: LinkTx, mut rx: LinkRx) -> Session {
        let this = self.clone();
        Box::pin(async move {
            let Some(request) = rx.recv::<Request>().await? else {
                return Ok(());
            };
            tx.send(&this.answer(request).await).await?;
            tx.finish().await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::State;
    use iroh::SecretKey;
    use std::fs;

    #[tokio::test]
    async fn a_new_search_folder_is_saved_and_a_missing_one_refused() {
        let temp = tempfile::tempdir().expect("a temp folder");
        let dir = temp.path().join("node");
        let server = SecretKey::generate().public().to_string();
        State::create(&dir, &server, "box").expect("the node's settings");
        // Saved checkouts, so starting walks nothing.
        fs::write(dir.join("checkouts.json"), "[]").expect("checkouts saved");
        let agents = Arc::new(Agents::new(Vec::new()));
        let searches = Searches::start(dir.clone(), agents, &Search::default());
        let home = Reply::Folder {
            folder: "~/".into(),
        };
        assert_eq!(searches.answer(Request::Get {}).await, home);

        let missing = temp.path().join("missing").display().to_string();
        let refused = searches.answer(Request::Set { folder: missing }).await;
        let Reply::Failed { reason } = refused else {
            panic!("a missing folder is refused: {refused:?}");
        };
        assert!(reason.contains("is not a folder"), "{reason}");

        let projects = temp.path().join("projects");
        fs::create_dir(&projects).expect("a folder");
        let folder = projects.display().to_string();
        let set = Request::Set {
            folder: folder.clone(),
        };
        let saved = Reply::Folder { folder };
        assert_eq!(searches.answer(set).await, saved);
        assert_eq!(searches.answer(Request::Get {}).await, saved);
    }
}

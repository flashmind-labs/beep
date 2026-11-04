//! `beep-node local`: the node bundled with a single-user server joins its
//! owner without a link code, through `prepare`.

use crate::{files::fsat, link::state::State};
use anyhow::{Context, Result};
use std::{fs, io, path::Path, time::Duration};
use tokio::time::sleep;

/// Where the server writes its key. Matches the server's `instance` module.
const SERVER_FILE: &str = "server.id";
/// Where the node writes its key and name, one per line.
const NODE_FILE: &str = "node.id";
/// How often to look for the server's key while it starts.
const POLL: Duration = Duration::from_secs(2);

/// Waits for the server's key in `link`, a folder both share, then makes
/// or keeps this node's key in `dir` and writes it and the node's name
/// back. The server links that key to the owner when the node connects.
pub async fn prepare(dir: &Path, link: &Path, name: &str) -> Result<State> {
    let server = wait_for_server(link).await?;
    let state = State::create(dir, &server, name)?;
    let text = format!("{}\n{}\n", state.key.public(), state.name);
    let partial = link.join("node.id.partial");
    fs::write(&partial, text).with_context(|| format!("writing {}", partial.display()))?;
    // The server runs as another user; the file only holds a public key.
    fsat::set_mode(&partial, 0o644).with_context(|| format!("opening up {}", partial.display()))?;
    fs::rename(&partial, link.join(NODE_FILE))
        .with_context(|| format!("writing {}", link.join(NODE_FILE).display()))?;
    log::info!("waiting for the server to link this node to its owner");
    Ok(state)
}

async fn wait_for_server(link: &Path) -> Result<String> {
    let path = link.join(SERVER_FILE);
    let mut said = false;
    loop {
        match fs::read_to_string(&path) {
            Ok(text) if !text.trim().is_empty() => return Ok(text.trim().to_owned()),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        }
        if !said {
            log::info!("waiting for the server's key in {}", path.display());
            said = true;
        }
        sleep(POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    #[tokio::test]
    async fn writes_its_key_for_the_server() {
        let root = std::env::temp_dir().join(format!("beep-node-local-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (dir, link) = (root.join("node"), root.join("link"));
        fs::create_dir_all(&link).unwrap();
        let server = SecretKey::generate().public();
        fs::write(link.join(SERVER_FILE), format!("{server}\n")).unwrap();
        let state = prepare(&dir, &link, "box").await.unwrap();
        assert_eq!(state.server, Some(server));
        let written = fs::read_to_string(link.join(NODE_FILE)).unwrap();
        assert_eq!(written, format!("{}\nbox\n", state.key.public()));
        // A restart keeps the key.
        let again = prepare(&dir, &link, "box").await.unwrap();
        assert_eq!(again.key.public(), state.key.public());
        fs::remove_dir_all(&root).unwrap();
    }
}

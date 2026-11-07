//! Per-project Syncthing exclusions, stored in the shared folder's ignore file.

use super::{Gui, Managed, Syncthing, TIMEOUT, get, query_value, real, timeout};
use anyhow::{Context, Result, bail};
use beep_proto::node::sync::SyncthingFiles;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub(crate) const BEGIN: &str = "// Beep project files";
pub(crate) const END: &str = "// End Beep project files";

#[derive(Serialize, Deserialize)]
struct Ignores {
    #[serde(default)]
    ignore: Vec<String>,
}

impl Syncthing {
    /// Reads or changes this project's exclusion. Syncthing's API applies
    /// the ignore file immediately; no shared folder is paused.
    pub(crate) async fn project_files(
        &self,
        folder: &Path,
        enabled: Option<bool>,
    ) -> Result<SyncthingFiles> {
        let folder = folder.to_owned();
        let folder = tokio::task::spawn_blocking(move || real(&folder)).await?;
        let listed = match self.gui().await {
            Ok(gui) => match timeout(TIMEOUT, super::fetch(&gui)).await {
                Ok(Ok(mut raw)) => {
                    for folder in &mut raw {
                        folder.paused = false;
                    }
                    let home = self.inner.home.clone();
                    Some(
                        tokio::task::spawn_blocking(move || super::resolve(raw, home.as_deref()))
                            .await?,
                    )
                }
                _ => None,
            },
            Err(_) => None,
        };
        let managed = match listed {
            Some(listed) => listed
                .iter()
                .filter(|share| share.holds(&folder.to_string_lossy()))
                .max_by_key(|share| share.path.len())
                .map(|share| Managed {
                    id: Some(share.id.clone()),
                    label: share.name().to_owned(),
                    root: share.path.clone().into(),
                }),
            None => {
                let folder = folder.clone();
                let home = self.inner.home.clone();
                tokio::task::spawn_blocking(move || super::marked(&folder, home.as_deref())).await?
            }
        };
        let Some(managed) = managed else {
            return Ok(SyncthingFiles {
                available: false,
                enabled: false,
                label: None,
            });
        };
        if let Some(enabled) = enabled {
            let id = managed
                .id
                .as_deref()
                .context("Start Syncthing before changing project file sync")?;
            let gui = self.gui().await?;
            let path = format!("/rest/db/ignores?folder={}", query_value(id));
            let raw = timeout(TIMEOUT, get(&gui, &path)).await??;
            let old: Ignores = serde_json::from_slice(&raw)?;
            let pattern = pattern(&managed.root, &folder)?;
            let updated = update(&old.ignore, &pattern, enabled);
            if updated != old.ignore {
                post(&gui, &path, &Ignores { ignore: updated }).await?;
            }
        }
        let share = managed.root.clone();
        let ignored = tokio::task::spawn_blocking(move || {
            crate::files::sync::stignore::project_ignored(&folder, &share)
        })
        .await??;
        Ok(SyncthingFiles {
            available: true,
            enabled: !ignored,
            label: Some(managed.label),
        })
    }
}

fn pattern(root: &Path, folder: &Path) -> Result<String> {
    let relative = folder
        .strip_prefix(root)?
        .to_string_lossy()
        .replace('\\', "/");
    if relative.contains(['\n', '\r', '*', '?', '[', ']', '{', '}']) {
        bail!(
            "This project's path contains characters Syncthing treats as patterns; set its ignore rule in Syncthing"
        );
    }
    Ok(if relative.is_empty() {
        "/**".into()
    } else {
        format!("/{relative}")
    })
}

fn update(lines: &[String], pattern: &str, enabled: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        if lines[at] == BEGIN
            && lines.get(at + 1).is_some_and(|line| line == pattern)
            && lines.get(at + 2).is_some_and(|line| line == END)
        {
            at += 3;
        } else {
            out.push(lines[at].clone());
            at += 1;
        }
    }
    if !enabled {
        let git_rules: Vec<_> = super::LOCAL_GIT.lines().map(str::to_owned).collect();
        let at = if out.starts_with(&git_rules) {
            git_rules.len()
        } else {
            0
        };
        out.splice(
            at..at,
            [BEGIN.to_owned(), pattern.to_owned(), END.to_owned()],
        );
    }
    out
}

async fn post(gui: &Gui, path: &str, ignores: &Ignores) -> Result<()> {
    timeout(
        TIMEOUT,
        super::request(gui, "POST", path, &serde_json::to_vec(ignores)?),
    )
    .await
    .context("Updating Syncthing's project ignore rule timed out")??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_api_hands_one_project_between_syncthing_and_beep() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };
        use tokio_util::task::AbortOnDropHandle;
        let dir = tempfile::TempDir::new().unwrap();
        let share = super::real(dir.path());
        let project = share.join("owner/widget");
        let other = share.join("owner/other");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(share.join(".stignore"), "*.tmp\n").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        std::fs::write(share.join("config.xml"), format!(r#"<configuration><gui tls="false"><address>{address}</address><apikey>test-key</apikey></gui></configuration>"#)).unwrap();
        let serving = share.clone();
        let _server = AbortOnDropHandle::new(tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut raw = Vec::new();
                let mut buf = [0; 4096];
                let split = loop {
                    let n = stream.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    raw.extend_from_slice(&buf[..n]);
                    if let Some(split) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        break split + 4;
                    }
                };
                let header = String::from_utf8(raw[..split].to_vec()).unwrap();
                let length: usize = header
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                while raw.len() < split + length {
                    let n = stream.read(&mut buf).await.unwrap();
                    assert!(n > 0);
                    raw.extend_from_slice(&buf[..n]);
                }
                let request = header.lines().next().unwrap();
                let body = if request.starts_with("GET /rest/config/folders ") {
                    serde_json::to_string(
                        &serde_json::json!([{"id":"share", "label":"Projects", "path":serving}]),
                    )
                    .unwrap()
                } else {
                    assert!(
                        request.contains(" /rest/db/ignores?folder=share "),
                        "{request}"
                    );
                    if request.starts_with("POST ") {
                        let ignores: Ignores =
                            serde_json::from_slice(&raw[split..split + length]).unwrap();
                        std::fs::write(serving.join(".stignore"), ignores.ignore.join("\n") + "\n")
                            .unwrap();
                    }
                    let ignore: Vec<_> = std::fs::read_to_string(serving.join(".stignore"))
                        .unwrap()
                        .lines()
                        .map(str::to_owned)
                        .collect();
                    serde_json::to_string(&Ignores { ignore }).unwrap()
                };
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
        }));
        let syncthing = Syncthing::at(vec![share.clone()], Some(share.clone()));
        assert!(syncthing.check(&project).await.is_some());
        assert!(
            !syncthing
                .project_files(&project, Some(false))
                .await
                .unwrap()
                .enabled
        );
        assert!(syncthing.check(&project).await.is_none());
        assert!(syncthing.check(&other).await.is_some());
        let rules = crate::files::sync::Rules::load(&project).unwrap();
        assert!(!rules.leaves_out(Path::new("src/main.rs"), false));
        assert!(rules.leaves_out(Path::new("cache.tmp"), false));
        assert!(
            syncthing
                .project_files(&project, Some(true))
                .await
                .unwrap()
                .enabled
        );
        assert!(syncthing.check(&project).await.is_some());
        assert_eq!(
            std::fs::read_to_string(share.join(".stignore")).unwrap(),
            format!("{}*.tmp\n", super::super::LOCAL_GIT)
        );
    }

    #[test]
    fn project_toggles_keep_git_protection_first_without_duplicating_it() {
        let original: Vec<String> = format!("{}!keep\n*.tmp\n", super::super::LOCAL_GIT)
            .lines()
            .map(str::to_owned)
            .collect();
        let disabled = update(&original, "/owner/widget", false);
        assert!(disabled.join("\n").starts_with(super::super::LOCAL_GIT));
        assert_eq!(update(&disabled, "/owner/widget", false), disabled);
        assert_eq!(update(&disabled, "/owner/widget", true), original);
    }

    #[test]
    fn changing_one_project_preserves_other_projects_and_user_rules() {
        let original = vec!["!keep".into(), "*.tmp".into()];
        let one = update(&original, "/owner/one", false);
        let two = update(&one, "/owner/two", false);
        assert_eq!(update(&two, "/owner/two", false), two);
        assert_eq!(
            update(&two, "/owner/one", true),
            update(&original, "/owner/two", false)
        );
        assert_eq!(update(&one, "/owner/one", true), original);
    }
}

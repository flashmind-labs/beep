//! What a desktop in a mesh tells the server, so phones and the project agent still see it.

use super::Reply;
use crate::{
    account::user::User,
    api::router::Cx,
    protocol::{ApiError, MirrorImport},
    store::{ImportRow, NewImport, NewNode},
};
use beep_projects::imports::folder::FolderPath;
use beep_projects::model::RepoName;
use iroh::EndpointId;
use std::str::FromStr;

/// The longest repository URL taken.
const MAX_URL: usize = 2048;

pub(super) async fn mirror_import(cx: Cx, request: MirrorImport) -> Reply<()> {
    record(&cx.user, &request).await
}

/// Records an import a desktop made itself: the row `ImportProject` would
/// save, with no clone and no machine contacted. The machine must be one of
/// the caller's keys or nodes. A desktop's own machine has a key and may have
/// no node, and the lists name machines from `nodes`, so it gets a row.
async fn record(user: &User, request: &MirrorImport) -> Reply<()> {
    let project = RepoName::parse(request.project.trim())
        .map_err(|e| ApiError::bad_request(e.to_string()))?
        .to_string();
    let folder = match FolderPath::parse(&request.folder) {
        Ok(Some(FolderPath::Absolute(path))) => path,
        Ok(_) => {
            return Err(ApiError::bad_request(
                "Give the folder as an absolute path.",
            ));
        }
        Err(e) => return Err(ApiError::bad_request(e.to_string())),
    };
    if request.url.len() > MAX_URL || request.url.chars().any(char::is_control) {
        return Err(ApiError::bad_request("The repository URL is not valid."));
    }
    let machine = request.machine_id.as_str();
    let nodes = user.nodes().await?;
    let node_named = nodes.iter().find(|node| node.endpoint == machine);
    if node_named.is_none() {
        let keys = user.account_keys().await?;
        let key = keys
            .iter()
            .find(|key| key.endpoint_id == machine)
            .ok_or(ApiError::NotFound {})?;
        let endpoint = EndpointId::from_str(machine).map_err(|_| ApiError::NotFound {})?;
        user.node_connected(NewNode {
            endpoint,
            name: &key.name,
            version: "",
        })
        .await?;
    }
    let before = user.imports().await?;
    let same_project = |one: &&ImportRow| one.project.eq_ignore_ascii_case(&project);
    // One project is one row per machine whatever its case.
    let stored = before
        .iter()
        .find(|one| same_project(one) && one.endpoint == machine);
    let name = before
        .iter()
        .find(same_project)
        .map_or(project.as_str(), |one| one.project.as_str());
    let url_changed = stored.is_some_and(|one| one.url != request.url);
    user.save_import(NewImport {
        project: name,
        endpoint: machine,
        folder: &folder,
        url: &request.url,
    })
    .await?;
    if url_changed {
        user.set_import_fork(name, machine, None).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        protocol::DeviceKind,
        store::{NewKey, TestDb},
    };
    use iroh::SecretKey;

    #[tokio::test]
    async fn a_new_url_clears_the_stored_fork() {
        let db = TestDb::new().await.unwrap();
        let user = User::for_test(db.store().await.unwrap(), "alice");
        let key = SecretKey::generate().public();
        user.link_key(NewKey {
            endpoint: key,
            name: "desk",
            kind: DeviceKind::Desktop {},
        })
        .await
        .unwrap();
        let machine = key.to_string();
        let request = |url: &str| MirrorImport {
            project: "org/widget".into(),
            machine_id: machine.clone(),
            folder: "/srv/widget".into(),
            url: url.into(),
        };
        record(&user, &request("https://example.com/org/widget"))
            .await
            .unwrap();
        let fork = RepoName::parse("me/widget").unwrap();
        user.set_import_fork("org/widget", &machine, Some(&fork))
            .await
            .unwrap();

        record(&user, &request("https://example.com/org/widget"))
            .await
            .unwrap();
        assert_eq!(user.imports().await.unwrap()[0].fork, Some(fork));

        record(&user, &request("https://example.org/org/widget"))
            .await
            .unwrap();
        assert_eq!(user.imports().await.unwrap()[0].fork, None);
    }
}

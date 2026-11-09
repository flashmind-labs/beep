//! What a desktop in a mesh tells the server, so phones and the project agent still see it.

use super::Reply;
use crate::{
    account::user::User,
    api::router::Cx,
    protocol::{ApiError, MirrorImport},
    store::NewNode,
};
use beep_projects::imports::folder::FolderPath;
use beep_projects::imports::records::{ImportRow, NewImport};
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
    let project = (request.project.trim())
        .parse::<RepoName>()
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
#[path = "../tests/api_mesh.rs"]
mod tests;

//! Project groups: folders the apps list projects under, kept per user.

use super::Reply;
use crate::{
    api::router::Cx,
    protocol::{
        groups::{group_name, group_paths},
        *,
    },
    store::SaveGroup,
};

pub(super) async fn list_project_groups(cx: Cx, _: ListProjectGroups) -> Reply<Vec<ProjectGroup>> {
    Ok(cx.user.project_groups().await?)
}

pub(super) async fn save_project_group(
    cx: Cx,
    SaveProjectGroup { id, name, paths }: SaveProjectGroup,
) -> Reply<ProjectGroup> {
    let paths = group_paths(&paths)?;
    let name = group_name(&name, &paths)?;
    cx.user
        .save_project_group(SaveGroup {
            id,
            name: &name,
            paths: &paths,
        })
        .await?
        .ok_or(ApiError::NotFound {})
}

pub(super) async fn delete_project_group(
    cx: Cx,
    DeleteProjectGroup { id }: DeleteProjectGroup,
) -> Reply<()> {
    if cx.user.delete_project_group(id).await? {
        Ok(())
    } else {
        Err(ApiError::NotFound {})
    }
}

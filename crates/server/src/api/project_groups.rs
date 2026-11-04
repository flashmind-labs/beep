//! Project groups: folders the apps list projects under, kept per user.

use super::Reply;
use crate::{
    api::router::Cx,
    protocol::{groups::project_group_path, *},
    store::SaveGroup,
};

/// The longest group name.
const MAX_NAME: usize = 100;
/// The longest folder.
const MAX_PATH: usize = 500;
/// The most folders one group holds.
const MAX_PATHS: usize = 16;

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

/// The folders as groups keep them, repeats left out, in the order given.
fn group_paths(given: &[String]) -> Result<Vec<String>, ApiError> {
    let mut paths: Vec<String> = Vec::new();
    for raw in given {
        if raw.len() > MAX_PATH || raw.chars().any(char::is_control) {
            return Err(ApiError::bad_request(format!(
                "A folder needs at most {MAX_PATH} characters, on one line."
            )));
        }
        let path = project_group_path(raw);
        if path.is_empty() || path == "/" {
            return Err(ApiError::bad_request(format!(
                "{} holds every project; pick a folder inside it.",
                raw.trim()
            )));
        }
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        return Err(ApiError::bad_request("A group needs a folder."));
    }
    if paths.len() > MAX_PATHS {
        return Err(ApiError::bad_request(format!(
            "A group holds at most {MAX_PATHS} folders."
        )));
    }
    Ok(paths)
}

/// `name` trimmed, or the first folder's when it is empty.
fn group_name(name: &str, paths: &[String]) -> Result<String, ApiError> {
    let name = name.trim();
    let name = if name.is_empty() {
        paths.first().map(String::as_str).unwrap_or_default()
    } else {
        name
    };
    if name.chars().count() > MAX_NAME || name.chars().any(char::is_control) {
        return Err(ApiError::bad_request(format!(
            "A group's name needs at most {MAX_NAME} characters, on one line."
        )));
    }
    Ok(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn given(list: &[&str]) -> Vec<String> {
        list.iter().map(|p| (*p).to_owned()).collect()
    }

    #[test]
    fn a_home_folder_the_root_or_nothing_is_refused() {
        for list in [
            vec![],
            vec!["~"],
            vec!["/Users/someone/Projects"],
            vec!["/"],
        ] {
            assert!(
                matches!(group_paths(&given(&list)), Err(ApiError::BadRequest { .. })),
                "{list:?}"
            );
        }
        let many: Vec<String> = (0..=MAX_PATHS).map(|i| format!("~/f{i}")).collect();
        assert!(group_paths(&many).is_err());
    }
}

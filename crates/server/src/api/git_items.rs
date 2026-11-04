//! The caller's open pull requests and issues, for Home.

use super::Reply;
use crate::{
    api::router::Cx,
    protocol::{GitItems, LabeledGitItems, ListGitItems, ListLabeledGitItems},
};

pub(super) async fn list_git_items(cx: Cx, _: ListGitItems) -> Reply<GitItems> {
    let logins = cx.user.git_logins(&cx.app.credential_key).await?;
    Ok(cx.app.git_items.list(cx.user.id(), logins).await?.found)
}

pub(super) async fn list_labeled_git_items(
    cx: Cx,
    _: ListLabeledGitItems,
) -> Reply<LabeledGitItems> {
    let logins = cx.user.git_logins(&cx.app.credential_key).await?;
    Ok(cx.app.git_items.list(cx.user.id(), logins).await?)
}

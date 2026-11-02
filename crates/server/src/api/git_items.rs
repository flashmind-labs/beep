//! The caller's open pull requests and issues, for Home.

use super::Reply;
use crate::{
    protocol::{GitItems, ListGitItems},
    router::Cx,
};

pub(super) async fn list_git_items(cx: Cx, _: ListGitItems) -> Reply<GitItems> {
    let logins = cx.user.git_logins(&cx.app.credential_key).await?;
    Ok(cx.app.git_items.list(cx.user.id(), logins).await?)
}

//! Group chats.

use super::{Reply, chat_id, check_images, found, next_run};
use crate::{
    app::{UserNotice, group_message},
    images::{self},
    protocol::*,
    router::Cx,
    store::{Join, encode_schedule},
};
use uuid::Uuid;

/// Messages a group chat loads at once.
const GROUP_MESSAGES: i64 = 200;

/// A group's name, trimmed.
fn group_name(name: &str) -> Reply<String> {
    trimmed_name(name, "group name", 60)
}

/// What a member goes by in a group, trimmed. Nobody may go by Beep.
fn group_member_name(name: &str) -> Reply<String> {
    let name = trimmed_name(name, "member name", 30)?;
    if name.eq_ignore_ascii_case("beep") {
        return Err(ApiError::bad_request("Beep is taken"));
    }
    Ok(name)
}

fn trimmed_name(name: &str, what: &str, max: usize) -> Reply<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > max {
        return Err(ApiError::bad_request(format!(
            "{what} must be 1-{max} characters"
        )));
    }
    Ok(name.to_owned())
}

pub(super) async fn create_group(
    cx: Cx,
    CreateGroup {
        id,
        name,
        member_name,
    }: CreateGroup,
) -> Reply<Group> {
    let name = group_name(&name)?;
    let member_name = group_member_name(&member_name)?;
    if !cx.user.create_group(id, &name, &member_name).await? {
        return Err(ApiError::Conflict {
            message: "group id is taken".into(),
        });
    }
    let joined = cx.user.group(id).await?.ok_or(ApiError::Internal {})?;
    Ok(cx.app.group_view(&joined, cx.user.id()))
}

pub(super) async fn list_groups(cx: Cx, _: ListGroups) -> Reply<Vec<Group>> {
    Ok(cx
        .user
        .groups()
        .await?
        .iter()
        .map(|joined| cx.app.group_view(joined, cx.user.id()))
        .collect())
}

pub(super) async fn get_group(cx: Cx, GetGroup { id }: GetGroup) -> Reply<GroupChat> {
    let id = chat_id(&id)?;
    let joined = cx.user.group(id).await?.ok_or(ApiError::NotFound {})?;
    let messages = cx.app.store.group_messages(id, GROUP_MESSAGES).await?;
    Ok(GroupChat {
        group: cx.app.group_view(&joined, cx.user.id()),
        messages: messages
            .iter()
            .map(|row| group_message(row, cx.user.id()))
            .collect(),
    })
}

pub(super) async fn send_to_group(
    cx: Cx,
    SendToGroup {
        group_id,
        request_id,
        message,
        images,
    }: SendToGroup,
) -> Reply<GroupMessage> {
    if message.trim().is_empty() || message.len() > 8192 {
        return Err(ApiError::bad_request("message must be 1-8192 bytes"));
    }
    check_images(&images)?;
    let pictures = images::pictures(&images);
    cx.app
        .send_to_group(
            &cx.user,
            chat_id(&group_id)?,
            request_id,
            message.trim(),
            &pictures,
        )
        .await?
        .ok_or(ApiError::NotFound {})
}

pub(super) async fn get_group_images(
    cx: Cx,
    GetGroupImages { group_id }: GetGroupImages,
) -> Reply<Vec<SentImages>> {
    let group_id = chat_id(&group_id)?;
    if cx.user.group(group_id).await?.is_none() {
        return Err(ApiError::NotFound {});
    }
    Ok(cx.app.store.group_pictures(group_id).await?)
}

pub(super) async fn group_invite(
    cx: Cx,
    GroupInvite { id, reset }: GroupInvite,
) -> Reply<GroupLink> {
    let token = cx
        .user
        .group_invite(chat_id(&id)?, reset)
        .await?
        .ok_or(ApiError::NotFound {})?;
    Ok(GroupLink {
        url: group_url(&token),
        token,
    })
}

pub(super) async fn peek_group(cx: Cx, PeekGroup { token }: PeekGroup) -> Reply<GroupPreview> {
    let token = group_code(&token).ok_or(ApiError::NotFound {})?;
    let joined = cx
        .app
        .store
        .group_by_invite(&token)
        .await?
        .ok_or(ApiError::NotFound {})?;
    Ok(GroupPreview {
        joined: joined.members.iter().any(|m| m.user_id == cx.user.id()),
        members: joined.members.into_iter().map(|m| m.name).collect(),
        id: joined.group.id.to_string(),
        name: joined.group.name,
    })
}

pub(super) async fn join_group(
    cx: Cx,
    JoinGroup { token, member_name }: JoinGroup,
) -> Reply<Group> {
    let member_name = group_member_name(&member_name)?;
    let token = group_code(&token).ok_or(ApiError::NotFound {})?;
    let invited = cx
        .app
        .store
        .group_by_invite(&token)
        .await?
        .ok_or(ApiError::NotFound {})?;
    // Beep tells members apart by name, so two may not share one.
    let taken = invited
        .members
        .iter()
        .any(|m| m.user_id != cx.user.id() && m.name.eq_ignore_ascii_case(&member_name));
    if taken {
        return Err(ApiError::Conflict {
            message: "someone in the group already goes by that name".into(),
        });
    }
    let id = match cx.user.join_group(&token, &member_name).await? {
        Join::Joined(id) => id,
        Join::NameTaken => {
            return Err(ApiError::Conflict {
                message: "someone in the group already goes by that name".into(),
            });
        }
        Join::UnknownInvite => return Err(ApiError::NotFound {}),
    };
    let joined = cx.user.group(id).await?.ok_or(ApiError::Internal {})?;
    for member in &joined.members {
        let _ = cx
            .app
            .notifications
            .send(UserNotice::group_changed(&member.user_id, &id.to_string()));
    }
    Ok(cx.app.group_view(&joined, cx.user.id()))
}

pub(super) async fn pin_group(cx: Cx, PinGroup { id, pinned }: PinGroup) -> Reply<()> {
    found(cx.user.pin_group(chat_id(&id)?, pinned).await?)
}

pub(super) async fn share_group_apps(
    cx: Cx,
    ShareGroupApps { id, shared }: ShareGroupApps,
) -> Reply<()> {
    found(cx.user.share_group_apps(chat_id(&id)?, shared).await?)
}

pub(super) async fn list_group_reminders(
    cx: Cx,
    ListGroupReminders { group_id }: ListGroupReminders,
) -> Reply<Vec<GroupReminder>> {
    let group_id = chat_id(&group_id)?;
    let joined = cx
        .user
        .group(group_id)
        .await?
        .ok_or(ApiError::NotFound {})?;
    let fallback = cx
        .user
        .get_timezone()
        .await
        .unwrap_or_else(|_| "UTC".into());
    let mut reminders = Vec::new();
    for job in cx.app.store.group_reminders(group_id).await? {
        if !job.enabled {
            continue;
        }
        let setter = job.metadata.get("user_id").and_then(|v| v.as_str());
        let member = joined
            .members
            .iter()
            .find(|m| Some(m.user_id.as_str()) == setter);
        // A cron expression is in the setter's time, as the runner reads it.
        let tz = match setter.and_then(|id| cx.app.user(id)) {
            Some(user) => user
                .get_timezone()
                .await
                .unwrap_or_else(|_| fallback.clone()),
            None => fallback.clone(),
        };
        let next = next_run(&job.schedule, &tz);
        reminders.push((
            next,
            GroupReminder {
                id: job.id.to_string(),
                label: (job.metadata.get("label").and_then(|v| v.as_str()))
                    .unwrap_or("Reminder")
                    .to_owned(),
                message: job.task,
                schedule: encode_schedule(&job.schedule),
                next_run_at: next.map(|t| t.to_rfc3339()),
                set_by: member.map_or_else(|| "A former member".to_owned(), |m| m.name.clone()),
            },
        ));
    }
    reminders.sort_by_key(|(next, _)| (next.is_none(), *next));
    Ok(reminders.into_iter().map(|(_, r)| r).collect())
}

pub(super) async fn delete_group_reminder(
    cx: Cx,
    DeleteGroupReminder { group_id, id }: DeleteGroupReminder,
) -> Reply<()> {
    let group_id = chat_id(&group_id)?;
    if cx.user.group(group_id).await?.is_none() {
        return Err(ApiError::NotFound {});
    }
    let id: Uuid = id.parse().map_err(|_| ApiError::NotFound {})?;
    found(cx.app.store.delete_group_reminder(group_id, id).await?)
}

pub(super) async fn answer_group_approval(
    cx: Cx,
    AnswerGroupApproval { query_id, allow }: AnswerGroupApproval,
) -> Reply<()> {
    found(
        cx.app
            .answer_group_action(&cx.user, &query_id, allow)
            .await?,
    )
}

pub(super) async fn leave_group(cx: Cx, LeaveGroup { id }: LeaveGroup) -> Reply<()> {
    let group_id = chat_id(&id)?;
    let members = match cx.user.group(group_id).await? {
        Some(joined) => joined.members,
        None => return Err(ApiError::NotFound {}),
    };
    cx.user.leave_group(group_id).await?;
    for member in &members {
        let _ = cx.app.notifications.send(UserNotice::group_changed(
            &member.user_id,
            &group_id.to_string(),
        ));
    }
    Ok(())
}

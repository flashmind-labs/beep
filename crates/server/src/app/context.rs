//! The developer message each turn starts with.

use super::{App, Message};
use crate::{
    cron,
    memories::PROFILE_HEADER,
    protocol::{AppIntegration, DeviceKind},
    skills::{self, INDEX_HEADER},
    store::Location,
    user::User,
    wake,
};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use flashmind::core::{Conversation, EntryKind};
use std::fmt::Write;

impl App {
    /// Time, location, which apps are connected,
    /// for a routine or an event how to answer it, and the skill list and the memory
    /// profile when `chat` has not had these yet, sent as a developer message. Other memories the
    /// model looks up with `memory_recall`.
    pub(super) async fn build_context(
        &self,
        user: &User,
        chat: &Conversation,
        message: &Message,
        apps: &[AppIntegration],
    ) -> Option<String> {
        let mut ctx = String::new();

        // Time with user's timezone
        if let Ok(tz_name) = user.get_timezone().await {
            let now = if let Ok(tz) = tz_name.parse::<Tz>() {
                Utc::now()
                    .with_timezone(&tz)
                    .format("%A %Y-%m-%d %H:%M %Z")
                    .to_string()
            } else {
                Utc::now().format("%A %Y-%m-%d %H:%M UTC").to_string()
            };
            let _ = writeln!(ctx, "Time: {now}");
        }

        if let Ok(Some(loc)) = user.get_location().await {
            let _ = writeln!(ctx, "{}", location_line(&loc, Utc::now()));
        }

        if let Some(platform) = message.platform {
            let _ = writeln!(ctx, "The user is writing from {}.", platform.describe());
        }
        if message.origin == (DeviceKind::Desktop {}) {
            let _ = writeln!(
                ctx,
                "The user is writing from their computer: phone actions such as alarms, \
                 maps, contacts and opening apps are not available in this turn."
            );
        }
        if message.charts {
            let _ = writeln!(
                ctx,
                "Their app draws charts under your reply: use show_stock_chart for a stock, \
                 index, currency or crypto price, and show_chart for numbers a tool or page \
                 gave you that read better as a chart."
            );
        }

        if let Some(line) = apps_line(apps) {
            let _ = writeln!(ctx, "{line}");
        }

        if let Some(label) = message.scheduled.as_deref() {
            let _ = writeln!(ctx, "{}", cron::scheduled_context(label));
        }
        if let Some(label) = message.woken.as_deref() {
            let _ = writeln!(ctx, "{}", wake::woken_context(label));
        }

        // Ends in a blank line, which is where `last_index` stops reading.
        match self.store.owned_skills(&user.skill_owner()).await {
            Ok(owned) => {
                if let Some(index) = skills::index(INDEX_HEADER, &owned)
                    && last_index(chat) != Some(index.as_str())
                {
                    let _ = writeln!(ctx, "{index}\n");
                }
            }
            Err(error) => log::warn!("unable to list skills: {error}; user={user}"),
        }

        // Last, so the profile a chat got runs to the end of its message.
        match user.memory_profile().await {
            Ok(Some(profile)) if last_profile(chat) != Some(profile.as_str()) => {
                let _ = writeln!(ctx, "{profile}");
            }
            Ok(_) => {}
            Err(error) => log::warn!("unable to build the memory profile: {error}; user={user}"),
        }

        (!ctx.is_empty()).then_some(ctx)
    }
}

/// The skill list the chat's context last carried.
fn last_index(chat: &Conversation) -> Option<&str> {
    chat.entries()
        .iter()
        .rev()
        .find_map(|entry| match &entry.kind {
            EntryKind::Developer { content, .. } => content.find(INDEX_HEADER).map(|at| {
                let index = &content[at..];
                index
                    .find("\n\n")
                    .map_or(index, |end| &index[..end])
                    .trim_end()
            }),
            _ => None,
        })
}

/// The memory profile the chat's context last carried.
fn last_profile(chat: &Conversation) -> Option<&str> {
    chat.entries()
        .iter()
        .rev()
        .find_map(|entry| match &entry.kind {
            EntryKind::Developer { content, .. } => content
                .find(PROFILE_HEADER)
                .map(|at| content[at..].trim_end()),
            _ => None,
        })
}

/// `Location: 35.6938 N, 139.7034 E (3 min ago)`.
/// The age tells the model whether "nearby" still means where the user is.
/// The phone's reverse-geocoded name is left out: it is often a nearby
/// district rather than where the user is, and the model repeats it as fact.
fn location_line(loc: &Location, now: DateTime<Utc>) -> String {
    let ns = if loc.latitude >= 0.0 { "N" } else { "S" };
    let ew = if loc.longitude >= 0.0 { "E" } else { "W" };
    let coords = format!(
        "{:.4} {ns}, {:.4} {ew}",
        loc.latitude.abs(),
        loc.longitude.abs()
    );
    let minutes = (now - loc.updated_at).num_minutes().max(0);
    let age = match minutes {
        0 => "just now".to_owned(),
        1..60 => format!("{minutes} min ago"),
        60..2880 => format!("{} h ago", minutes / 60),
        _ => format!("{} days ago", minutes / 1440),
    };
    format!("Location: {coords} ({age})")
}

/// Which apps are connected and which the user could connect, so the model
/// sends them to the Apps screen instead of guessing how accounts are linked.
fn apps_line(apps: &[AppIntegration]) -> Option<String> {
    let names = |connected: bool| {
        apps.iter()
            .filter(|app| app.configured && app.connected == connected)
            .map(|app| app.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let (connected, available) = (names(true), names(false));
    if connected.is_empty() && available.is_empty() {
        return None;
    }
    let mut line = format!(
        "Connected apps: {}.",
        if connected.is_empty() {
            "none"
        } else {
            &connected
        }
    );
    if !available.is_empty() {
        let _ = write!(
            line,
            " Not connected: {available}. The user connects these from Apps in Beep's menu."
        );
    }
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::AppKind;
    use std::time::Duration;

    fn app(name: &str, connected: bool, configured: bool) -> AppIntegration {
        AppIntegration {
            slug: name.to_lowercase(),
            name: name.into(),
            description: String::new(),
            logo_url: None,
            connected,
            configured,
            kind: AppKind::Composio {},
        }
    }

    #[test]
    fn apps_line_names_connected_and_available_apps() {
        let apps = [
            app("Fastmail", true, true),
            app("Outlook", false, true),
            app("Teams", false, true),
            app("Slack", false, false),
        ];
        assert_eq!(
            apps_line(&apps).as_deref(),
            Some(
                "Connected apps: Fastmail. Not connected: Outlook, Teams. \
                 The user connects these from Apps in Beep's menu."
            )
        );
        assert_eq!(
            apps_line(&[app("Gmail", false, true)]).as_deref(),
            Some(
                "Connected apps: none. Not connected: Gmail. \
                 The user connects these from Apps in Beep's menu."
            )
        );
        assert_eq!(apps_line(&[app("Slack", false, false)]), None);
    }

    fn fix(place: Option<&str>, updated_at: DateTime<Utc>) -> Location {
        Location {
            latitude: 35.69384,
            longitude: 139.70341,
            accuracy: 12.0,
            timezone: "Asia/Tokyo".into(),
            place: place.map(Into::into),
            updated_at,
        }
    }

    #[test]
    fn location_line_gives_only_coordinates_and_age() {
        let now = Utc::now();
        let loc = fix(
            Some("Shinjuku, Tokyo, Japan"),
            now - Duration::from_secs(180),
        );
        assert_eq!(
            location_line(&loc, now),
            "Location: 35.6938 N, 139.7034 E (3 min ago)"
        );
    }

    #[test]
    fn location_line_without_a_place_keeps_the_coordinates() {
        let now = Utc::now();
        let loc = fix(None, now - Duration::from_secs(3 * 3600));
        assert_eq!(
            location_line(&loc, now),
            "Location: 35.6938 N, 139.7034 E (3 h ago)"
        );
        assert!(location_line(&fix(None, now), now).ends_with("(just now)"));
    }

    #[test]
    fn last_profile_is_the_newest_one_the_chat_got() {
        use flashmind::core::ConversationEntry;
        let mut chat = Conversation::new();
        assert_eq!(last_profile(&chat), None);
        chat.add(ConversationEntry::system_message(format!(
            "Time: now\n{PROFILE_HEADER}\n- Is called Sam.\n"
        )));
        chat.add(ConversationEntry::user("hi"));
        chat.add(ConversationEntry::system_message("Time: later\n"));
        assert_eq!(
            last_profile(&chat),
            Some(format!("{PROFILE_HEADER}\n- Is called Sam.").as_str())
        );
    }
}

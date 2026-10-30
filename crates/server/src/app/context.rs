//! The developer message each turn starts with.

use super::{App, Message};
use crate::{
    cron, events,
    instance::Privilege,
    memories::PROFILE_HEADER,
    nodes::{self, MACHINES_HEADER},
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
    /// model looks up with `memory` recall.
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
        if message.voice.is_some() || message.spoken {
            let _ = writeln!(
                ctx,
                "The user spoke this message and it was transcribed, so a word may be misheard: \
                 read it for what they meant. Their app reads your reply aloud, so keep it short \
                 and plain, without tables or links unless they ask for them."
            );
        }
        // The background tools come with turns someone asked for, for admins.
        let delegates = self.allows(Privilege::Background, user)
            && message.cron_id.is_none()
            && message.woken.is_none()
            && !message.finished_research;
        if message.spoken {
            let _ = writeln!(ctx, "{}", live_line(delegates));
        }
        if delegates {
            let _ = writeln!(ctx, "{DELEGATION}");
        }
        if message.draws.charts {
            let _ = writeln!(
                ctx,
                "Their app draws charts under your reply: use show_stock_chart for a stock, \
                 index, currency or crypto price, and show_chart for numbers a tool or page \
                 gave you that read better as a chart."
            );
        }
        if message.draws.mac_files {
            let _ = writeln!(
                ctx,
                "You can open files on their Mac with mac_files: when they mention a file on \
                 their Desktop, in Downloads or anywhere else on the computer, find and read it \
                 rather than asking them to send it."
            );
        }
        if message.draws.cards {
            let _ = writeln!(
                ctx,
                "Their app draws recommendation cards under your reply. When you recommend \
                 places to eat, drink or visit, films, shows, books, events or anything else \
                 to pick from, search first, then call show_cards with two to five, each with \
                 its real page URL and the part for its kind. Keep the reply short: what you \
                 looked for and which one you would pick, without repeating the cards' details. \
                 Products still go in show_products. For a single place the user will go to, \
                 open_maps is still right."
            );
        }

        if let Some(line) = apps_line(apps) {
            let _ = writeln!(ctx, "{line}");
        }

        if let Some(label) = message.scheduled.as_deref() {
            let _ = writeln!(ctx, "{}", cron::scheduled_context(label));
        }
        if let Some(woken) = &message.woken {
            let _ = writeln!(ctx, "{}", wake::woken_context(woken));
        }
        // What the event triage agent kept for later comes up once, in the
        // user's next turn.
        if message.is_prompt()
            && events::enabled(self, user)
            && let Some(waiting) = events::waiting_context(user).await
        {
            let _ = writeln!(ctx, "{waiting}");
        }
        // Results the turn claimed as it was created, before the user's text.
        if let Some(results) = self.subagents.claimed_context(message.session_id) {
            let _ = writeln!(ctx, "{results}");
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

        // Ends in a blank line, which is where `last_block` stops reading.
        if self.allows(Privilege::Nodes, user) {
            match self.machines(user).await {
                Ok(Some(machines))
                    if last_block(chat, MACHINES_HEADER) != Some(machines.as_str()) =>
                {
                    let _ = writeln!(ctx, "{machines}\n");
                }
                Ok(_) => {}
                Err(error) => log::warn!("unable to list machines: {error:#}; user={user}"),
            }
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

impl App {
    async fn machines(&self, user: &User) -> anyhow::Result<Option<String>> {
        let nodes = user.nodes().await?;
        if nodes.is_empty() {
            return Ok(None);
        }
        let rows = user.memory_rows().await?;
        Ok(nodes::machines(&nodes, &self.nodes, &rows))
    }
}

/// The skill list the chat's context last carried.
fn last_index(chat: &Conversation) -> Option<&str> {
    last_block(chat, INDEX_HEADER)
}

/// The block starting at `header` that the chat's context last carried, up
/// to the blank line after it.
fn last_block<'a>(chat: &'a Conversation, header: &str) -> Option<&'a str> {
    chat.entries()
        .iter()
        .rev()
        .find_map(|entry| match &entry.kind {
            EntryKind::Developer { content, .. } => content.find(header).map(|at| {
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

/// For a message said in live mode, a spoken conversation: everything you
/// write is read aloud as it comes. `delegates` adds the line about
/// `background`, for users who have that tool.
fn live_line(delegates: bool) -> String {
    let mut line = String::from(
        "This is a live spoken conversation: every sentence you write is read aloud as it \
         streams, including text between tool calls. Before using tools, say in one short \
         sentence what you are about to do, in the user's language, such as \"Let me check \
         build-box.\"",
    );
    if delegates {
        line.push_str(
            " Keep the conversation flowing: hand anything slow to the background as below \
             and say in one sentence that it is on it.",
        );
    }
    line
}

/// How to use the background tools, for turns that have them.
const DELEGATION: &str = "Answer quick things directly. Hand slow work to the `background` \
    tool and reply right away instead of making the user wait: spawn for work of many steps \
    (several lookups, going through a mailbox, coding, research), with a brief that is \
    complete on its own since it cannot see this chat; run for one slow tool call. Use message \
    to steer an agent that is working or to ask one that reported for more. Results come back \
    to this chat: relay them briefly, leading with the result.";

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

    #[test]
    fn live_mode_says_what_it_does_and_delegates_only_with_the_tool() {
        let plain = live_line(false);
        assert!(plain.contains("read aloud"));
        assert!(!plain.contains("background"));
        assert!(live_line(true).contains("background"));
        for word in ["`background`", "spawn", "run", "message"] {
            assert!(DELEGATION.contains(word));
        }
    }
}

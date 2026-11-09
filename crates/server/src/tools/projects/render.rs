//! The text the tool answers with.

use super::{Live, MAX_PROJECTS, Place, Project, items_of, waits_on_user};
use crate::protocol::{GitItem, GitItemKind, GitReview};
use std::{
    fmt::Write,
    time::{Duration, SystemTime},
};

/// Pull requests, issues or tasks `show` lists of each kind.
pub(super) const MAX_ROWS: usize = 15;

/// Longest title shown, in characters.
pub(super) const MAX_TITLE: usize = 90;

pub(super) fn render_list(projects: &[Project], live: &Live) -> String {
    let mut out = format!(
        "{} project{}. PRs and issues count the ones that involve the user (opened, assigned \
         or asking their review).\n",
        projects.len(),
        if projects.len() == 1 { "" } else { "s" }
    );

    for note in &live.notes {
        let _ = writeln!(out, "Note: {note}");
    }

    for project in projects.iter().take(MAX_PROJECTS) {
        let _ = write!(out, "\n- {}", project.name);

        if let Some(url) = &project.url {
            let _ = write!(out, " ({url})");
        }

        out.push('\n');
        let _ = writeln!(out, "  on: {}", places_text(&project.places));
        let items = items_of(project, live);
        let prs = items
            .iter()
            .filter(|item| item.kind == (GitItemKind::PullRequest {}))
            .count();
        let waiting = items.iter().filter(|item| waits_on_user(item)).count();
        let issues = items.len() - prs;
        let _ = write!(out, "  open: {prs} PR{}", plural(prs));

        if waiting > 0 {
            let _ = write!(out, " ({waiting} waiting on the user)");
        }

        let tasks = project.tasks.len();
        let _ = writeln!(
            out,
            ", {issues} issue{}, {tasks} task{}",
            plural(issues),
            plural(tasks)
        );
    }

    if projects.len() > MAX_PROJECTS {
        let _ = writeln!(
            out,
            "\nAnd {} more projects; `show` any of them by name.",
            projects.len() - MAX_PROJECTS
        );
    }

    out.trim_end().to_owned()
}

pub(super) fn render_show(project: &Project, live: &Live, now: SystemTime) -> String {
    let mut out = project.name.to_string();

    if let Some(url) = &project.url {
        let _ = write!(out, " ({url})");
    }

    let _ = writeln!(out, "\nOn: {}", places_text(&project.places));

    for note in &live.notes {
        let _ = writeln!(out, "Note: {note}");
    }

    let items = items_of(project, live);
    let (prs, issues): (Vec<&GitItem>, Vec<&GitItem>) = items
        .into_iter()
        .partition(|item| item.kind == (GitItemKind::PullRequest {}));

    let _ = writeln!(
        out,
        "\nOpen PRs involving the user ({}):",
        count_text(prs.len())
    );

    for pr in prs.iter().take(MAX_ROWS) {
        let _ = writeln!(out, "- {}", pr_text(pr, now));
    }

    let _ = writeln!(
        out,
        "\nOpen issues involving the user ({}):",
        count_text(issues.len())
    );

    for issue in issues.iter().take(MAX_ROWS) {
        let _ = writeln!(out, "- {}", issue_text(issue, now));
    }

    let _ = writeln!(out, "\nOpen tasks ({}):", count_text(project.tasks.len()));

    for task in project.tasks.iter().take(MAX_ROWS) {
        let state: &'static str = task.state.into();
        let _ = write!(
            out,
            "- {} [{state}] {}",
            task.slug.as_ref(),
            short(&task.title)
        );
        if let Some(pr) = &task.pr_url {
            let _ = write!(out, ", PR {pr}");
        } else if let Some(issue) = &task.issue {
            let _ = write!(out, ", issue #{}", issue.number);
        }

        out.push('\n');
    }

    out.trim_end().to_owned()
}

/// `none`, a number, or `15 of 40` when the list is cut.
pub(super) fn count_text(count: usize) -> String {
    match count {
        0 => "none".to_owned(),
        n if n > MAX_ROWS => format!("{MAX_ROWS} of {n}"),
        n => n.to_string(),
    }
}

pub(super) fn places_text(places: &[Place]) -> String {
    if places.is_empty() {
        return "no machine".to_owned();
    }

    places
        .iter()
        .map(|place| format!("{} {}", place.machine, place.folder))
        .collect::<Vec<_>>()
        .join("; ")
}

pub(super) fn pr_text(pr: &GitItem, now: SystemTime) -> String {
    let mut facts = Vec::new();
    if pr.draft {
        facts.push("draft".to_owned());
    }
    if pr.authored {
        facts.push("by the user".to_owned());
    }
    if pr.review_requested {
        facts.push("asks the user's review".to_owned());
    }
    match pr.review {
        GitReview::Approved {} => facts.push("approved".to_owned()),
        GitReview::ChangesRequested {} => facts.push("changes requested".to_owned()),
        _ => {}
    }

    if pr.checks_failing {
        facts.push("checks failing".to_owned());
    }
    if pr.comments > 0 {
        facts.push(format!("{} comments", pr.comments));
    }
    facts.push(format!("updated {}", ago(pr.updated_at, now)));
    format!(
        "#{} {} ({}) {}",
        pr.number,
        short(&pr.title),
        facts.join(", "),
        pr.url
    )
}

pub(super) fn issue_text(issue: &GitItem, now: SystemTime) -> String {
    let mut facts = Vec::new();
    if issue.assigned {
        facts.push("assigned to the user".to_owned());
    }
    if issue.authored {
        facts.push("opened by the user".to_owned());
    }
    if issue.comments > 0 {
        facts.push(format!("{} comments", issue.comments));
    }
    facts.push(format!("updated {}", ago(issue.updated_at, now)));
    format!(
        "#{} {} ({}) {}",
        issue.number,
        short(&issue.title),
        facts.join(", "),
        issue.url
    )
}

pub(super) fn ago(at: SystemTime, now: SystemTime) -> String {
    let minutes = now.duration_since(at).unwrap_or(Duration::ZERO).as_secs() / 60;

    match minutes {
        0 => "just now".to_owned(),
        1..60 => format!("{minutes} min ago"),
        60..2880 => format!("{} h ago", minutes / 60),
        _ => format!("{} days ago", minutes / 1440),
    }
}

pub(super) fn short(text: &str) -> String {
    let text = text.trim();

    if text.chars().count() <= MAX_TITLE {
        return text.to_owned();
    }

    let cut: String = text.chars().take(MAX_TITLE - 3).collect();
    format!("{}...", cut.trim_end())
}

pub(super) fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

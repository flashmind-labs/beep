//! What a project worker may do without asking the user. Agents that ask
//! permission over ACP put each action to [`judge`] as a [`Question`]; the
//! verdict either answers it or hands it to the user.
//!
//! Not every agent asks. `flsh` runs its tools without a permission request,
//! so [`judge`] never sees its actions: see [`agent_asks`].

use super::model::{MachinePath, Visibility};
use crate::{leak::PublicText, protocol::node::agent::Question};

/// Whether the agent named `agent` puts its actions to the user over ACP.
/// One that does not is not held to this policy, so a worker must not rely on
/// the policy alone to keep it inside its worktree.
pub(crate) fn agent_asks(agent: &str) -> bool {
    agent != "flsh"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    Allow,
    /// Never allowed; the text tells the worker what to do instead.
    Refuse(String),
    AskUser,
}

/// What a worker may touch.
pub(crate) struct WorkerScope<'a> {
    pub(crate) worktree: &'a MachinePath,
    /// The task's own branch, `beep/<slug>`.
    pub(crate) branch: &'a str,
    pub(crate) default_branch: &'a str,
    pub(crate) visibility: Visibility,
    /// Whether the project app's level is Allow, which lets the worker push
    /// its own branch and open a pull request without asking.
    pub(crate) outside_allowed: bool,
    pub(crate) public_text: &'a PublicText,
}

const FORCE_REFUSAL: &str =
    "Force pushes are not allowed. Push without force, or ask in your last message.";
const EDIT_VERBS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];
const SHELL_SYNTAX: [&str; 8] = [";", "|", "`", "$(", ">", "<", "&", "\n"];

pub(crate) fn judge(question: &Question, scope: &WorkerScope<'_>) -> Verdict {
    let text = question.text.trim();
    let detail = question
        .detail
        .as_deref()
        .map(str::trim)
        .filter(|detail| !detail.is_empty());
    if is_cut(text) || detail.is_some_and(is_cut) {
        return Verdict::AskUser;
    }
    let mut words = text.splitn(2, char::is_whitespace);
    if words.next().is_some_and(|verb| EDIT_VERBS.contains(&verb)) {
        let rest = words.next().unwrap_or_default().trim();
        return judge_edit([rest, detail.unwrap_or_default()], scope);
    }
    let command = detail.unwrap_or_else(|| text.strip_prefix("Run ").unwrap_or(text).trim());
    judge_command(command, scope)
}

/// Whether the agent cut `text` short. Go's `./...` pattern is not a cut.
fn is_cut(text: &str) -> bool {
    text.ends_with("...") && !text.ends_with("/...")
}

/// An edit is fine inside the worktree, from the first of `candidates` that
/// is an absolute path.
fn judge_edit(candidates: [&str; 2], scope: &WorkerScope<'_>) -> Verdict {
    let path = candidates
        .into_iter()
        .find(|candidate| candidate.starts_with('/'))
        .and_then(|candidate| MachinePath::parse(candidate).ok());
    match path {
        Some(path) if scope.worktree.contains(&path) => Verdict::Allow,
        _ => Verdict::AskUser,
    }
}

fn judge_command(command: &str, scope: &WorkerScope<'_>) -> Verdict {
    if SHELL_SYNTAX.iter().any(|syntax| command.contains(syntax)) {
        return Verdict::AskUser;
    }
    let Some(tokens) = shlex::split(command) else {
        return Verdict::AskUser;
    };
    let words: Vec<&str> = tokens.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["git", "push", args @ ..] => judge_push(args, scope),
        ["gh" | "teacli", "pr", "create", ..] => judge_pr_create(command, scope),
        ["gh" | "teacli", "pr", "merge", ..] | ["gh" | "teacli", "release", ..] => Verdict::AskUser,
        ["git", "tag", ..] => Verdict::AskUser,
        ["rm", args @ ..] => judge_rm(args, scope),
        ["git", sub, args @ ..] => judge_git(sub, args),
        other if is_check_or_build(other) => Verdict::Allow,
        _ => Verdict::AskUser,
    }
}

fn judge_push(args: &[&str], scope: &WorkerScope<'_>) -> Verdict {
    let force = args.iter().any(|arg| match arg.strip_prefix("--") {
        Some(long) => long.starts_with("force"),
        None => match arg.strip_prefix('-') {
            Some(short) => short.contains('f'),
            None => arg.starts_with('+'),
        },
    });
    if force {
        return Verdict::Refuse(FORCE_REFUSAL.to_owned());
    }
    let mut positional = Vec::new();
    for arg in args {
        match *arg {
            "-u" | "--set-upstream" => {}
            flag if flag.starts_with('-') => return Verdict::AskUser,
            other => positional.push(other),
        }
    }
    let [remote, refspecs @ ..] = positional.as_slice() else {
        return Verdict::AskUser;
    };
    let target = |refspec: &str| {
        let target = refspec.rsplit(':').next().unwrap_or(refspec);
        target
            .strip_prefix("refs/heads/")
            .unwrap_or(target)
            .to_owned()
    };
    let touches_default_or_tags = refspecs.iter().any(|refspec| {
        refspec.starts_with("refs/tags/")
            || target(refspec).starts_with("refs/tags/")
            || target(refspec) == scope.default_branch
    });
    if touches_default_or_tags {
        return Verdict::AskUser;
    }
    let own = [
        scope.branch.to_owned(),
        format!("refs/heads/{}", scope.branch),
        format!("HEAD:{}", scope.branch),
        format!("HEAD:refs/heads/{}", scope.branch),
    ];
    match refspecs {
        [refspec] if *remote == "origin" && own.iter().any(|own| own == refspec) => {
            allow_if_outside_allowed(scope)
        }
        _ => Verdict::AskUser,
    }
}

fn allow_if_outside_allowed(scope: &WorkerScope<'_>) -> Verdict {
    if scope.outside_allowed {
        Verdict::Allow
    } else {
        Verdict::AskUser
    }
}

fn judge_pr_create(command: &str, scope: &WorkerScope<'_>) -> Verdict {
    if scope.visibility == Visibility::Public {
        return Verdict::AskUser;
    }
    let hits = scope.public_text.hits(command);
    if !hits.is_empty() {
        return Verdict::Refuse(format!(
            "Rewrite the pull request without: {}.",
            hits.join(", ")
        ));
    }
    allow_if_outside_allowed(scope)
}

/// `rm` of paths strictly inside the worktree. Relative paths count from the
/// worktree, which is where the worker runs.
fn judge_rm(args: &[&str], scope: &WorkerScope<'_>) -> Verdict {
    let paths: Vec<&str> = args
        .iter()
        .copied()
        .filter(|arg| !arg.starts_with('-'))
        .collect();
    let inside = |path: &str| {
        if path.contains('$') || path.starts_with('~') {
            return false;
        }
        if path.starts_with('/') {
            return MachinePath::parse(path.trim_end_matches('/'))
                .is_ok_and(|path| path != *scope.worktree && scope.worktree.contains(&path));
        }
        path.split('/').all(|part| part != "..") && !matches!(path, "." | "./")
    };
    if !paths.is_empty() && paths.iter().all(|path| inside(path)) {
        Verdict::Allow
    } else {
        Verdict::AskUser
    }
}

/// Read-only git, `add` and `commit`. Options before the subcommand, such as
/// `-c` and `-C`, are not understood and ask.
fn judge_git(sub: &str, args: &[&str]) -> Verdict {
    match sub {
        "status" | "diff" | "log" | "show" | "rev-parse" | "add" | "commit" => Verdict::Allow,
        // `git branch -D` deletes, so only the listing forms pass.
        "branch"
            if args.iter().all(|arg| {
                matches!(
                    *arg,
                    "-a" | "-r" | "-v" | "-vv" | "--list" | "--show-current"
                )
            }) =>
        {
            Verdict::Allow
        }
        _ => Verdict::AskUser,
    }
}

fn is_check_or_build(words: &[&str]) -> bool {
    matches!(
        words,
        ["cargo", "build" | "check" | "test" | "clippy" | "fmt", ..]
            | ["just", "test" | "check" | "build" | "fmt", ..]
            | ["npm" | "pnpm" | "yarn", "test", ..]
            | [
                "npm" | "pnpm" | "yarn",
                "run",
                "test" | "build" | "lint",
                ..
            ]
            | ["go", "test" | "build", ..]
            | ["pytest", ..]
            | ["make"]
            | ["make", "test"]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        worktree: MachinePath,
        public_text: PublicText,
    }

    impl Fixture {
        fn new() -> Self {
            Fixture {
                worktree: MachinePath::parse("/srv/project/.worktrees/fix-it").expect("a path"),
                public_text: PublicText::new(&["build-box".to_owned()], &["/home/sam".to_owned()]),
            }
        }

        fn scope(&self, visibility: Visibility, outside_allowed: bool) -> WorkerScope<'_> {
            WorkerScope {
                worktree: &self.worktree,
                branch: "beep/fix-it",
                default_branch: "main",
                visibility,
                outside_allowed,
                public_text: &self.public_text,
            }
        }
    }

    fn ask(text: &str, detail: Option<&str>) -> Question {
        Question {
            id: 1,
            text: text.to_owned(),
            detail: detail.map(str::to_owned),
            options: Vec::new(),
        }
    }

    fn judged(command: &str, visibility: Visibility, outside_allowed: bool) -> Verdict {
        let fixture = Fixture::new();
        judge(
            &ask("Run a command", Some(command)),
            &fixture.scope(visibility, outside_allowed),
        )
    }

    fn private(command: &str) -> Verdict {
        judged(command, Visibility::Private, false)
    }

    fn private_allowed(command: &str) -> Verdict {
        judged(command, Visibility::Private, true)
    }

    fn refused(verdict: &Verdict) -> bool {
        matches!(verdict, Verdict::Refuse(_))
    }

    #[test]
    fn edits_inside_the_worktree_pass() {
        let fixture = Fixture::new();
        let scope = fixture.scope(Visibility::Private, false);
        let edit = |text: &str| judge(&ask(text, None), &scope);
        assert_eq!(
            edit("Edit /srv/project/.worktrees/fix-it/src/lib.rs"),
            Verdict::Allow
        );
        assert_eq!(edit("Write /srv/project/src/lib.rs"), Verdict::AskUser);
        assert_eq!(
            edit("Edit /srv/project/.worktrees/fix-it/../other/x"),
            Verdict::AskUser
        );
        assert_eq!(edit("Edit src/lib.rs"), Verdict::AskUser);
        assert_eq!(
            judge(
                &ask("Write file", Some("/srv/project/.worktrees/fix-it/a.txt")),
                &scope
            ),
            Verdict::Allow
        );
    }

    #[test]
    fn checks_and_builds_pass() {
        for command in [
            "cargo test -p core",
            "cargo clippy -- -D warnings",
            "just test",
            "pnpm run lint",
            "npm test",
            "go build ./...",
            "pytest -q",
            "make",
            "make test",
            "git status",
            "git commit -m 'Fix it'",
            "git add -A",
            "git branch --show-current",
        ] {
            assert_eq!(private(command), Verdict::Allow, "{command}");
        }
        for command in [
            "make install",
            "git branch -D main",
            "cargo publish",
            "curl x",
        ] {
            assert_eq!(private(command), Verdict::AskUser, "{command}");
        }
    }

    #[test]
    fn text_is_the_command_without_detail() {
        let fixture = Fixture::new();
        let scope = fixture.scope(Visibility::Private, false);
        assert_eq!(judge(&ask("Run cargo test", None), &scope), Verdict::Allow);
    }

    #[test]
    fn shell_syntax_asks() {
        for command in [
            "cargo test; rm -rf /",
            "cargo test | tee out",
            "cargo test > out",
            "cargo test && curl x",
            "cargo test `id`",
            "cargo test $(id)",
            "cargo test &",
        ] {
            assert_eq!(private(command), Verdict::AskUser, "{command}");
        }
    }

    #[test]
    fn pushes_are_refused_or_asked() {
        for command in [
            "git push -f origin beep/fix-it",
            "git push --force-with-lease origin beep/fix-it",
            "git push origin +beep/fix-it",
            "git push -uf origin beep/fix-it",
        ] {
            assert!(refused(&private_allowed(command)), "{command}");
        }
        for command in [
            "git push origin main",
            "git push origin HEAD:main",
            "git push origin refs/heads/main",
            "git push --tags",
            "git push origin refs/tags/v1",
            "git push -u origin beep/fix-it && git push origin main",
            "git push",
            "git push origin",
            "git push origin other",
            "git push upstream beep/fix-it",
            "git push --delete origin beep/fix-it",
        ] {
            assert_eq!(private_allowed(command), Verdict::AskUser, "{command}");
        }
        assert_eq!(
            private_allowed("git push -u origin beep/fix-it"),
            Verdict::Allow
        );
        assert_eq!(
            private_allowed("git push origin HEAD:beep/fix-it"),
            Verdict::Allow
        );
        assert_eq!(private("git push -u origin beep/fix-it"), Verdict::AskUser);
    }

    #[test]
    fn public_repo_pr_always_asks() {
        let command = "gh pr create --title Fix --body Done";
        assert_eq!(judged(command, Visibility::Public, true), Verdict::AskUser);
        assert_eq!(private_allowed(command), Verdict::Allow);
        assert_eq!(private(command), Verdict::AskUser);
        assert_eq!(
            private_allowed("teacli pr create acme/widget -B main -H beep/fix-it -t T"),
            Verdict::Allow
        );
    }

    #[test]
    fn leaky_pr_is_refused() {
        let verdict = private_allowed("gh pr create --body \"tested on build-box\"");
        assert_eq!(
            verdict,
            Verdict::Refuse("Rewrite the pull request without: build-box.".to_owned())
        );
    }

    #[test]
    fn merges_and_releases_ask() {
        for command in [
            "gh pr merge 3 --squash",
            "teacli pr merge acme/widget 3",
            "gh release create v1",
            "teacli release create acme/widget",
            "git tag v1",
        ] {
            assert_eq!(private_allowed(command), Verdict::AskUser, "{command}");
        }
    }

    #[test]
    fn rm_stays_in_the_worktree() {
        assert_eq!(private("rm -rf target"), Verdict::Allow);
        assert_eq!(
            private("rm /srv/project/.worktrees/fix-it/a.txt"),
            Verdict::Allow
        );
        for command in [
            "rm -rf /srv/project",
            "rm ../a",
            "rm a /etc/passwd",
            "rm -rf /srv/project/.worktrees/fix-it",
            "rm -rf ~",
            "rm",
        ] {
            assert_eq!(private(command), Verdict::AskUser, "{command}");
        }
    }

    #[test]
    fn cut_questions_ask() {
        let fixture = Fixture::new();
        let scope = fixture.scope(Visibility::Private, true);
        assert_eq!(
            judge(
                &ask("Run cargo test", Some("cargo test -p very-long...")),
                &scope
            ),
            Verdict::AskUser
        );
        assert_eq!(
            judge(&ask("Run cargo test...", None), &scope),
            Verdict::AskUser
        );
    }

    #[test]
    fn only_flsh_skips_the_questions() {
        for agent in ["claude", "codex", "gemini", "pi"] {
            assert!(agent_asks(agent), "{agent}");
        }
        assert!(!agent_asks("flsh"));
    }
}

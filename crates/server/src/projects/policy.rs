//! What a project worker may do without asking the user: each question a
//! worker asks goes to [`judge`], whose verdict either answers it or hands
//! it to the user.
//!
//! A node starts every agent with its permission prompts off and allows a
//! permission request that comes anyway, so [`judge`] sees only questions
//! that reach the server: a choice the agent puts to the user, or a request
//! from a node older than that. A worker must not rely on this policy to
//! keep it inside its worktree, which is why every start card asks.

use super::model::{MachinePath, RepoName, Visibility};
use crate::{
    leak::PublicText,
    protocol::node::agent::{MAX_QUESTION_LINE, Question},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    Allow,
    /// Never allowed; the text tells the worker what to do instead.
    Refuse(String),
    AskUser,
}

/// What a worker may touch.
#[derive(Clone, Copy)]
pub(crate) struct WorkerScope<'a> {
    /// The project's repository: the only one a pull request may target.
    pub(crate) repo: &'a RepoName,
    pub(crate) worktree: &'a MachinePath,
    /// The task's own branch, `beep/<slug>`.
    pub(crate) branch: &'a str,
    pub(crate) default_branch: &'a str,
    pub(crate) visibility: Visibility,
    /// Project is Allow, so the worker may push its branch and open a PR unasked.
    pub(crate) outside_allowed: bool,
    pub(crate) public_text: &'a PublicText,
    /// The worker edited a build file since the last allowed build, so builds ask.
    pub(crate) build_edited: bool,
}

const FORCE_REFUSAL: &str =
    "Force pushes are not allowed. Push without force, or ask in your last message.";
const EDIT_VERBS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];
/// Shell syntax that runs, redirects or expands; braces and globs can hide paths (`rm {x,../..}`).
const SHELL_SYNTAX: [&str; 14] = [
    ";", "|", "`", "$", ">", "<", "&", "\n", "{", "}", "*", "?", "[", "]",
];

pub(crate) fn judge(question: &Question, scope: &WorkerScope<'_>) -> Verdict {
    // Measured before trimming: the node cuts the raw input.
    if is_cut(&question.text) || question.detail.as_deref().is_some_and(is_cut) {
        return Verdict::AskUser;
    }
    match action(question) {
        Action::Edit { detail, title_rest } => judge_edit(detail, title_rest, scope),
        Action::Command(command) => judge_command(command, scope),
    }
}

/// What a question asks to do.
enum Action<'a> {
    /// An edit, with the raw path in `detail` or the title after its verb.
    Edit {
        detail: Option<&'a str>,
        title_rest: &'a str,
    },
    Command(&'a str),
}

fn action(question: &Question) -> Action<'_> {
    let text = question.text.trim();
    let detail = question
        .detail
        .as_deref()
        .map(str::trim)
        .filter(|detail| !detail.is_empty());
    let mut words = text.splitn(2, char::is_whitespace);
    if words.next().is_some_and(|verb| EDIT_VERBS.contains(&verb)) {
        let title_rest = words.next().unwrap_or_default().trim();
        return Action::Edit { detail, title_rest };
    }
    Action::Command(detail.unwrap_or_else(|| text.strip_prefix("Run ").unwrap_or(text).trim()))
}

/// Whether `question` edits a file a build or test runs (`build.rs`, Makefile, git hook...).
pub(crate) fn edits_build_file(question: &Question) -> bool {
    let Action::Edit { detail, title_rest } = action(question) else {
        return false;
    };
    let path = detail.unwrap_or(title_rest);
    let parts: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    let Some(name) = parts.last() else {
        return false;
    };
    let in_hooks = parts[..parts.len() - 1].iter().any(|dir| {
        let dir = dir.to_ascii_lowercase();
        HOOK_DIRS.contains(&dir.as_str())
    });
    in_hooks || is_build_file(name)
}

/// Folders whose files git, cargo or a hook manager runs.
const HOOK_DIRS: [&str; 5] = [".git", ".githooks", ".husky", ".cargo", "git~1"];

/// Files a build or test reads as code or as the list of what to run.
const BUILD_FILES: [&str; 30] = [
    "build.rs",
    "makefile",
    "gnumakefile",
    "justfile",
    ".justfile",
    "package.json",
    ".npmrc",
    ".yarnrc",
    ".yarnrc.yml",
    "pnpm-workspace.yaml",
    "setup.py",
    "setup.cfg",
    "pyproject.toml",
    "conftest.py",
    "tox.ini",
    "noxfile.py",
    "pytest.ini",
    "cmakelists.txt",
    "cargo.toml",
    "rust-toolchain",
    "rust-toolchain.toml",
    "gradlew",
    "gradle.properties",
    "build.zig",
    "meson.build",
    "build.bazel",
    "workspace",
    "module.bazel",
    "rakefile",
    "go.work",
];
/// Endings of build files, as `rules.mk` or `app.gradle.kts`.
const BUILD_ENDINGS: [&str; 5] = [".mk", ".cmake", ".gradle", ".gradle.kts", ".bzl"];
/// Extensions of tool configs that run as code, such as `vite.config.ts`.
const CONFIG_SCRIPTS: [&str; 6] = ["js", "ts", "mjs", "cjs", "mts", "cts"];

fn is_build_file(name: &str) -> bool {
    // Bazel's BUILD; a lowercase `build` is usually a folder or a script.
    if name == "BUILD" {
        return true;
    }
    let name = name.to_ascii_lowercase();
    BUILD_FILES.contains(&name.as_str())
        || BUILD_ENDINGS.iter().any(|ending| name.ends_with(ending))
        || name
            .rsplit_once('.')
            .is_some_and(|(stem, ext)| CONFIG_SCRIPTS.contains(&ext) && stem.ends_with(".config"))
}

/// Whether `question` runs a build, test or hook-running git command.
pub(crate) fn runs_build(question: &Question) -> bool {
    let Action::Command(command) = action(question) else {
        return false;
    };
    shlex::split(command).is_some_and(|tokens| {
        let words: Vec<&str> = tokens.iter().map(String::as_str).collect();
        runs_build_words(&words)
    })
}

fn runs_build_words(words: &[&str]) -> bool {
    is_check_or_build(words) || matches!(words, ["git", "commit" | "push", ..])
}

/// Longest UTF-8 character: a cut at a char boundary can fall this many bytes short.
const UTF8_CHAR: usize = 4;

/// Whether the node may have cut `text`: near [`MAX_QUESTION_LINE`] or `...` (not Go's `/...`).
fn is_cut(text: &str) -> bool {
    text.len() > MAX_QUESTION_LINE - UTF8_CHAR || (text.ends_with("...") && !text.ends_with("/..."))
}

/// An edit is fine inside the worktree, outside any `.git`; `detail` is judged relative to the
/// worktree, else the title must name an absolute path.
fn judge_edit(detail: Option<&str>, title_rest: &str, scope: &WorkerScope<'_>) -> Verdict {
    // A drive letter, an alternate data stream or a Windows separator: the
    // path may not be where it looks.
    if detail.unwrap_or(title_rest).contains([':', '\\']) {
        return Verdict::AskUser;
    }
    let path = match detail {
        Some(detail) if detail.starts_with('/') => MachinePath::parse(detail).ok(),
        Some(detail) if path_is_inside(detail, scope) && !detail.starts_with(['{', '[', '"']) => {
            Some(scope.worktree.join(detail))
        }
        Some(_) => None,
        None if title_rest.starts_with('/') => MachinePath::parse(title_rest).ok(),
        None => None,
    };
    match path {
        Some(path) if scope.worktree.contains(&path) && !has_git_part(path.as_str()) => {
            Verdict::Allow
        }
        _ => Verdict::AskUser,
    }
}

/// Whether `path` has a `.git` part, whose files can name a config git runs.
fn has_git_part(path: &str) -> bool {
    // Case-insensitive filesystems fold case; Windows drops trailing dots and
    // spaces and also answers to the short name `git~1`.
    path.split('/').any(|part| {
        let part = part.trim_end_matches(['.', ' ']);
        part.eq_ignore_ascii_case(".git") || part.eq_ignore_ascii_case("git~1")
    })
}

fn judge_command(command: &str, scope: &WorkerScope<'_>) -> Verdict {
    let verdict = judge_plain_command(command, scope);
    let held = scope.build_edited
        && verdict == Verdict::Allow
        && shlex::split(command).is_some_and(|tokens| {
            let words: Vec<&str> = tokens.iter().map(String::as_str).collect();
            runs_build_words(&words)
        });
    if held { Verdict::AskUser } else { verdict }
}

fn judge_plain_command(command: &str, scope: &WorkerScope<'_>) -> Verdict {
    if SHELL_SYNTAX.iter().any(|syntax| command.contains(syntax)) {
        return Verdict::AskUser;
    }
    let Some(tokens) = shlex::split(command) else {
        return Verdict::AskUser;
    };
    if tokens.iter().any(|token| token.starts_with('~')) {
        return Verdict::AskUser;
    }
    let words: Vec<&str> = tokens.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["git", "push", args @ ..] => judge_push(args, scope),
        ["gh" | "teacli", "pr", "create", args @ ..] => judge_pr_create(command, args, scope),
        ["gh" | "teacli", "pr", "merge", ..] | ["gh" | "teacli", "release", ..] => Verdict::AskUser,
        ["git", "tag", ..] => Verdict::AskUser,
        ["rm", args @ ..] => judge_rm(args, scope),
        ["git", sub, args @ ..] => judge_git(sub, args, scope),
        other if is_check_or_build(other) && args_stay_inside(other, scope) => Verdict::Allow,
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

/// A PR on the project's own repository with its text inline, so it can be checked.
fn judge_pr_create(command: &str, args: &[&str], scope: &WorkerScope<'_>) -> Verdict {
    if scope.visibility == Visibility::Public || !pr_args_are_plain(args, scope) {
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

fn pr_args_are_plain(args: &[&str], scope: &WorkerScope<'_>) -> bool {
    let is_repo = |text: &str| text.eq_ignore_ascii_case(&scope.repo.to_string());
    let mut args = args.iter().copied();
    while let Some(arg) = args.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) if arg.starts_with("--") => (name, Some(value)),
            _ => (arg, None),
        };
        let mut value = || inline.or_else(|| args.next());
        let fine = match name {
            "--title" | "-t" | "--body" | "-b" | "--base" | "-B" | "--head" | "-H" | "--format" => {
                value().is_some()
            }
            "-R" | "--repo" => value().is_some_and(is_repo),
            "--draft" | "-d" => inline.is_none(),
            positional if !positional.starts_with('-') => is_repo(positional),
            _ => false,
        };
        if !fine {
            return false;
        }
    }
    true
}

/// `rm` of paths strictly inside the worktree (relative paths start there).
fn judge_rm(args: &[&str], scope: &WorkerScope<'_>) -> Verdict {
    let paths: Vec<&str> = args
        .iter()
        .copied()
        .filter(|arg| !arg.starts_with('-'))
        .collect();
    let inside = |path: &str| {
        path_is_inside(path, scope)
            && !matches!(path.trim_end_matches('/'), "." | "")
            && MachinePath::parse(path.trim_end_matches('/')).map_or(true, |p| p != *scope.worktree)
    };
    if !paths.is_empty() && paths.iter().all(|path| inside(path)) {
        Verdict::Allow
    } else {
        Verdict::AskUser
    }
}

/// Whether `path` is in the worktree (relative paths start there).
fn path_is_inside(path: &str, scope: &WorkerScope<'_>) -> bool {
    if path.contains('$') || path.starts_with('~') || has_git_part(path) {
        return false;
    }
    if path.starts_with('/') {
        return MachinePath::parse(path.trim_end_matches('/'))
            .is_ok_and(|path| scope.worktree.contains(&path));
    }
    path.split('/').all(|part| part != "..")
}

/// Whether build or check arguments name only worktree paths and no option that runs elsewhere.
fn args_stay_inside(words: &[&str], scope: &WorkerScope<'_>) -> bool {
    // Compared without dashes: Go takes `-exec` and `--exec` alike.
    const ELSEWHERE: [&str; 7] = [
        "exec",
        "toolexec",
        "basetemp",
        "manifest-path",
        "config",
        "target-dir",
        "output",
    ];
    words.iter().skip(1).all(|word| {
        let (name, value) = word.split_once('=').unwrap_or((word, ""));
        let option = name.starts_with('-').then(|| name.trim_start_matches('-'));
        !option.is_some_and(|option| ELSEWHERE.contains(&option))
            && path_is_inside(name, scope)
            && (value.is_empty() || path_is_inside(value, scope))
    })
}

/// Read-only git, `add` and `commit`; options before the subcommand (`-c`, `-C`) ask.
fn judge_git(sub: &str, args: &[&str], scope: &WorkerScope<'_>) -> Verdict {
    // Options that write a file or run a program named in the repository.
    const WRITES: [&str; 4] = ["--output", "--ext-diff", "--textconv", "--no-index"];
    let name_of = |arg: &str| {
        arg.split_once('=')
            .map_or(arg.to_owned(), |(n, _)| n.to_owned())
    };
    match sub {
        "diff" | "log" | "show"
            if args
                .iter()
                .any(|arg| WRITES.contains(&name_of(arg).as_str())) =>
        {
            Verdict::AskUser
        }
        "status" | "diff" | "log" | "show" | "rev-parse" => Verdict::Allow,
        "add" if pathspec_file_stays_inside(args, scope) => Verdict::Allow,
        "commit"
            if pathspec_file_stays_inside(args, scope) && commit_files_stay_inside(args, scope) =>
        {
            Verdict::Allow
        }
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

/// `--pathspec-from-file` (or an unambiguous prefix) must name a worktree file, since git
/// echoes lines it cannot use. `-` reads stdin.
fn pathspec_file_stays_inside(args: &[&str], scope: &WorkerScope<'_>) -> bool {
    const OPTION: &str = "pathspec-from-file";
    // Shorter prefixes are ambiguous, so git rejects them.
    const SHORTEST: usize = "pathspec-f".len();
    let mut args = args.iter().copied();
    while let Some(arg) = args.next() {
        let Some(long) = arg.strip_prefix("--") else {
            continue;
        };
        let (name, inline) = match long.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (long, None),
        };
        if name.len() < SHORTEST || !OPTION.starts_with(name) {
            continue;
        }
        let value = inline.or_else(|| args.next());
        if !value.is_some_and(|value| value == "-" || path_is_inside(value, scope)) {
            return false;
        }
    }
    true
}

/// Files `git commit` reads via `-F`, `--file` or `--template` must be in the worktree.
fn commit_files_stay_inside(args: &[&str], scope: &WorkerScope<'_>) -> bool {
    let mut args = args.iter().copied();
    while let Some(arg) = args.next() {
        let value = if let Some(long) = arg.strip_prefix("--") {
            // Git accepts any unambiguous prefix of a long option.
            let (name, inline) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (long, None),
            };
            let abbreviates = |full: &str| !name.is_empty() && full.starts_with(name);
            if !(abbreviates("file") || abbreviates("template")) {
                continue;
            }
            inline.or_else(|| args.next())
        } else if let Some(cluster) = arg.strip_prefix('-') {
            // In a cluster such as `-sF/path`, an option that takes a value
            // ends it: `-m` takes a message, `-F` and `-t` a file.
            let Some(at) = cluster.find(['m', 'C', 'c', 'F', 't']) else {
                continue;
            };
            if matches!(&cluster[at..=at], "m" | "C" | "c") {
                continue;
            }
            let rest = &cluster[at + 1..];
            if rest.is_empty() {
                args.next()
            } else {
                Some(rest)
            }
        } else {
            continue;
        };
        if !value.is_some_and(|value| value != "-" && path_is_inside(value, scope)) {
            return false;
        }
    }
    true
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
        repo: RepoName,
        worktree: MachinePath,
        public_text: PublicText,
    }

    impl Fixture {
        fn new() -> Self {
            Fixture {
                repo: RepoName::parse("acme/widget").expect("a repo"),
                worktree: MachinePath::parse("/srv/project/.worktrees/fix-it").expect("a path"),
                public_text: PublicText::new(&["build-box".to_owned()], &["/home/sam".to_owned()]),
            }
        }

        fn scope(&self, visibility: Visibility, outside_allowed: bool) -> WorkerScope<'_> {
            WorkerScope {
                repo: &self.repo,
                worktree: &self.worktree,
                branch: "beep/fix-it",
                default_branch: "main",
                visibility,
                outside_allowed,
                public_text: &self.public_text,
                build_edited: false,
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
            "git push origin beep/fix-it:refs/heads/main",
            "git push origin :main",
            "git push origin beep/fix-it main",
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
    fn a_cut_input_asks() {
        let long = format!("cargo test{}; curl x|sh", " ".repeat(295));
        assert_eq!(private(&long[..MAX_QUESTION_LINE]), Verdict::AskUser);
        let fixture = Fixture::new();
        let scope = fixture.scope(Visibility::Private, true);
        assert_eq!(
            judge(&ask(&long[..MAX_QUESTION_LINE], None), &scope),
            Verdict::AskUser
        );
    }

    #[test]
    fn expansion_cannot_leave_the_worktree() {
        for command in [
            "rm -rf /srv/project/.worktrees/fix-it/{x,../..}",
            "rm -rf {a,../..}/x",
            "rm -rf *",
            "rm -rf ~/x",
            "git add ../*",
        ] {
            assert_eq!(private(command), Verdict::AskUser, "{command}");
        }
    }

    #[test]
    fn git_cannot_write_files_elsewhere() {
        for command in [
            "git show -s --format=%B HEAD --output=/home/user/.bashrc",
            "git log --output /home/user/x",
            "git diff --ext-diff",
            "git diff --no-index /etc/passwd x",
            "git log --textconv",
            "git commit -F /home/user/msg",
            "git commit --file=../msg",
            "git commit --template /etc/x",
            "git commit -F",
        ] {
            assert_eq!(private(command), Verdict::AskUser, "{command}");
        }
        assert_eq!(private("git commit -F msg.txt"), Verdict::Allow);
        assert_eq!(
            private("git commit --file=/srv/project/.worktrees/fix-it/msg"),
            Verdict::Allow
        );
    }

    #[test]
    fn pr_text_must_be_inline_and_for_this_repo() {
        for command in [
            "gh pr create --body-file notes.md",
            "gh pr create -F notes.md",
            "gh pr create --fill",
            "gh pr create -R other/repo --title T",
            "gh pr create --repo=other/repo --title T",
            "teacli pr create other/repo -t T",
            "gh pr create --web",
            "gh pr create --title",
            "teacli pr create acme/widget --host git.example.com -t T",
        ] {
            assert_eq!(private_allowed(command), Verdict::AskUser, "{command}");
        }
        for command in [
            "gh pr create -R acme/widget --title T --body B --base main --head beep/fix-it",
            "teacli pr create acme/widget -t T -b B -B main -H beep/fix-it",
        ] {
            assert_eq!(private_allowed(command), Verdict::Allow, "{command}");
        }
    }

    #[test]
    fn builds_stay_in_the_worktree() {
        for command in [
            "go test -exec /x",
            "pytest --basetemp=/srv/project",
            "cargo build --manifest-path /other/Cargo.toml",
            "cargo test --config build.rustc=x",
            "cargo build --target-dir ../out",
            "cargo test /etc/passwd",
        ] {
            assert_eq!(private(command), Verdict::AskUser, "{command}");
        }
        assert_eq!(
            private("cargo test --manifest-path /srv/project/.worktrees/fix-it/Cargo.toml"),
            Verdict::AskUser
        );
        assert_eq!(private("cargo test -p core -- --nocapture"), Verdict::Allow);
    }

    #[test]
    fn a_multibyte_cut_still_counts_as_cut() {
        // The node cuts at a character boundary, so 297 bytes can be a cut.
        let padded = format!("cargo test{}", " ".repeat(287));
        assert_eq!(padded.len(), 297);
        assert_eq!(private(&padded), Verdict::AskUser);
        assert_eq!(private("cargo test"), Verdict::Allow);
    }

    #[test]
    fn commit_options_cannot_be_abbreviated_or_clustered() {
        for command in [
            "git commit --fil=/abs/path",
            "git commit --f=/abs/path",
            "git commit --te /abs/path",
            "git commit --t=/abs/path",
            "git commit -sF/abs/path",
            "git commit -sF /abs/path",
            "git commit -t/abs/path",
            "git commit -st /abs/path",
        ] {
            assert_eq!(private(command), Verdict::AskUser, "{command}");
        }
        assert_eq!(private("git commit -sF msg.txt"), Verdict::Allow);
        assert_eq!(private("git commit -am 'Fix the test'"), Verdict::Allow);
        assert_eq!(private("git commit -mtext"), Verdict::Allow);
    }

    #[test]
    fn go_flags_take_two_dashes() {
        for command in [
            "go test --exec curl ./...",
            "go test --exec=curl ./...",
            "go build --toolexec x",
            "cargo build --manifest-path=/other/Cargo.toml",
        ] {
            assert_eq!(private(command), Verdict::AskUser, "{command}");
        }
    }

    #[test]
    fn a_detail_is_judged_alone() {
        let fixture = Fixture::new();
        let scope = fixture.scope(Visibility::Private, false);
        let title = "Edit /srv/project/.worktrees/fix-it/a.rs";
        let edit = |detail: &str| judge(&ask(title, Some(detail)), &scope);
        assert_eq!(edit("src/a.rs"), Verdict::Allow);
        assert_eq!(edit("../a.rs"), Verdict::AskUser);
        assert_eq!(edit("src/../../a.rs"), Verdict::AskUser);
        assert_eq!(edit(r#"{"notebook_path":"/etc/x"}"#), Verdict::AskUser);
        assert_eq!(edit("/etc/x"), Verdict::AskUser);
        assert_eq!(
            edit("/srv/project/.worktrees/fix-it/n.ipynb"),
            Verdict::Allow
        );
    }

    #[test]
    fn git_internals_ask() {
        let fixture = Fixture::new();
        let scope = fixture.scope(Visibility::Private, false);
        for detail in [
            "/srv/project/.worktrees/fix-it/.git",
            "/srv/project/.worktrees/fix-it/.git/config",
            ".git",
            "sub/.git/hooks/pre-commit",
        ] {
            assert_eq!(
                judge(&ask("Write file", Some(detail)), &scope),
                Verdict::AskUser,
                "{detail}"
            );
        }
        assert_eq!(
            judge(&ask("Write file", Some(".gitignore")), &scope),
            Verdict::Allow
        );
        assert_eq!(private("rm -rf .git"), Verdict::AskUser);
    }

    #[test]
    fn pathspec_files_stay_in_the_worktree() {
        for command in [
            "git commit --pathspec-from-file=/abs/secret -m x",
            "git add --pathspec-from-file=/abs/secret",
            "git add --pathspec-from-file ../secret",
            "git add --pathspec-from-fi /abs/secret",
            "git add --pathspec-f=/abs/secret",
            "git commit --pathspec-from-file=.git/config -m x",
        ] {
            assert_eq!(private(command), Verdict::AskUser, "{command}");
        }
        for command in [
            "git add --pathspec-from-file=-",
            "git add --pathspec-from-file=list.txt",
            "git commit --pathspec-from-file list.txt -m x",
        ] {
            assert_eq!(private(command), Verdict::Allow, "{command}");
        }
    }

    #[test]
    fn git_dirs_match_like_the_filesystem_does() {
        let fixture = Fixture::new();
        let scope = fixture.scope(Visibility::Private, false);
        for detail in [
            ".GIT/config",
            "sub/.Git/hooks/x",
            ".git./config",
            ".git /config",
            ".git.. /config",
            "GIT~1/config",
            "a/git~1/config",
        ] {
            assert_eq!(
                judge(&ask("Write file", Some(detail)), &scope),
                Verdict::AskUser,
                "{detail}"
            );
        }
        for detail in [".github/ci.yml", "git/x", "my.git/x", ".gitignore"] {
            assert_eq!(
                judge(&ask("Write file", Some(detail)), &scope),
                Verdict::Allow,
                "{detail}"
            );
        }
    }

    #[test]
    fn edit_paths_with_colons_or_backslashes_ask() {
        let fixture = Fixture::new();
        let scope = fixture.scope(Visibility::Private, false);
        for detail in [
            "src\\lib.rs",
            "..\\..\\x",
            "C:/x",
            "src/a:b",
            "/srv/project/.worktrees/fix-it/a:b",
            "/srv/project/.worktrees/fix-it/src\\x",
        ] {
            assert_eq!(
                judge(&ask("Write file", Some(detail)), &scope),
                Verdict::AskUser,
                "{detail}"
            );
        }
        assert_eq!(
            judge(
                &ask("Edit /srv/project/.worktrees/fix-it/src\\x", None),
                &scope
            ),
            Verdict::AskUser
        );
    }

    #[test]
    fn build_files_are_known() {
        for path in [
            "build.rs",
            "/srv/project/.worktrees/fix-it/crates/x/build.rs",
            "Makefile",
            "GNUmakefile",
            "rules.mk",
            "justfile",
            ".justfile",
            "package.json",
            "setup.py",
            "pyproject.toml",
            "conftest.py",
            "CMakeLists.txt",
            "app/build.gradle.kts",
            "settings.gradle",
            "Cargo.toml",
            ".cargo/config.toml",
            "vite.config.ts",
            ".git/hooks/pre-commit",
            ".husky/pre-commit",
        ] {
            assert!(edits_build_file(&ask("Write file", Some(path))), "{path}");
        }
        assert!(edits_build_file(&ask(
            "Edit /srv/project/.worktrees/fix-it/Makefile",
            None
        )));
        for path in [
            "src/lib.rs",
            "README.md",
            "build/output.txt",
            "docs/Makefile.md",
        ] {
            assert!(!edits_build_file(&ask("Write file", Some(path))), "{path}");
        }
        assert!(!edits_build_file(&ask(
            "Run a command",
            Some("cat Cargo.toml")
        )));
    }

    #[test]
    fn builds_after_a_build_file_edit_ask() {
        let fixture = Fixture::new();
        let scope = WorkerScope {
            build_edited: true,
            ..fixture.scope(Visibility::Private, false)
        };
        let run = |command: &str| judge(&ask("Run a command", Some(command)), &scope);
        for command in ["cargo test", "make", "just test", "pytest -q", "npm test"] {
            assert_eq!(run(command), Verdict::AskUser, "{command}");
            assert!(
                runs_build(&ask("Run a command", Some(command))),
                "{command}"
            );
        }
        // Everything else is judged as before.
        assert_eq!(run("git status"), Verdict::Allow);
        assert!(!runs_build(&ask("Run a command", Some("git status"))));
        assert_eq!(
            judge(&ask("Write file", Some("src/lib.rs")), &scope),
            Verdict::Allow
        );
    }
}

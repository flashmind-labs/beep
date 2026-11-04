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
    memory::leak::PublicText,
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
mod tests;

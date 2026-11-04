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

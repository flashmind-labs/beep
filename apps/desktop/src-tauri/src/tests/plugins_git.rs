use super::*;

#[test]
fn owner_and_repo_expand_to_github() {
    for text in [
        "owner/repo",
        " owner/repo ",
        "owner/repo.git",
        "my-org/my.repo_2",
    ] {
        let url = RepoUrl::parse(text).unwrap();
        assert!(
            url.as_str().starts_with("https://github.com/"),
            "{text}: {url}"
        );
        assert!(url.as_str().ends_with(".git"), "{text}: {url}");
    }
    assert_eq!(
        RepoUrl::parse("owner/repo").unwrap().as_str(),
        "https://github.com/owner/repo.git"
    );
}

#[test]
fn full_https_and_ssh_addresses_are_kept() {
    for text in [
        "https://github.com/owner/repo.git",
        "https://git.example.test/team/plugins",
        "ssh://git@git.example.test/team/plugins.git",
    ] {
        assert_eq!(RepoUrl::parse(text).unwrap().as_str(), text);
    }
    assert_eq!(
        RepoUrl::parse("git@github.com:owner/repo.git")
            .unwrap()
            .as_str(),
        "ssh://git@github.com/owner/repo.git"
    );
}

#[test]
fn local_paths_other_transports_and_options_are_refused() {
    for text in [
        "",
        "   ",
        "-oProxyCommand=x",
        "--upload-pack=touch /tmp/x",
        "file:///tmp/repo",
        "file://localhost/tmp/repo",
        "/tmp/repo",
        "./repo",
        "../repo",
        "~/repo",
        "C:\\repo",
        "ext::sh -c touch% /tmp/x",
        "git://host.example.test/owner/repo",
        "http://host.example.test/owner/repo",
        "https://user:secret@host.example.test/owner/repo",
        "https://host.example.test",
        "https://host.example.test/owner/repo?x=1",
        "owner/repo extra",
        "a/b/c",
        "owner/-repo",
        "owner/../repo",
        "-x@host:owner/repo",
        "git@host:",
    ] {
        assert_eq!(
            RepoUrl::parse(text),
            Err(InvalidInput::Repository),
            "{text:?} should be refused"
        );
    }
}

#[test]
fn refs_are_plain_names() {
    for text in [
        "main",
        "v1.2.3",
        "feature/cat-2",
        "0123456789abcdef0123456789abcdef01234567",
    ] {
        assert_eq!(GitRef::parse(text).unwrap().as_str(), text);
    }
    for text in [
        "",
        "-x",
        "--upload-pack=x",
        "a..b",
        "a b",
        "/main",
        "main/",
        "main.lock",
        "a//b",
        "a:b",
        "a~1",
        "a^",
        "a?b",
        "a*b",
        "a[b",
        "a\\b",
        "a@{1}",
        ".hidden",
    ] {
        assert_eq!(
            GitRef::parse(text),
            Err(InvalidInput::Reference),
            "{text:?}"
        );
    }
    assert!(GitRef::parse(&"a".repeat(201)).is_err());
}

#[test]
fn a_folder_stays_inside_the_repository() {
    assert_eq!(
        Subfolder::parse("plugins/cat").unwrap().path(),
        Path::new("plugins/cat")
    );
    assert_eq!(
        Subfolder::parse("./plugins/cat/").unwrap().path(),
        Path::new("plugins/cat")
    );
    for text in ["", ".", "/etc", "../x", "a/../../b", "a/../b", "a\\b"] {
        assert_eq!(
            Subfolder::parse(text),
            Err(InvalidInput::Folder),
            "{text:?}"
        );
    }
}

#[test]
fn the_best_ls_remote_line_wins() {
    let tag = "1111111111111111111111111111111111111111";
    let peeled = "2222222222222222222222222222222222222222";
    let head = "3333333333333333333333333333333333333333";

    let annotated = format!("{tag}\trefs/tags/v1\n{peeled}\trefs/tags/v1^{{}}\n");
    assert_eq!(pick(&annotated).unwrap().to_string(), peeled);

    let branch = format!("{head}\trefs/heads/main\n");
    assert_eq!(pick(&branch).unwrap().to_string(), head);

    assert_eq!(pick(&format!("{head}\tHEAD\n")).unwrap().to_string(), head);
    assert!(pick("").is_none());
    assert!(pick("not a listing").is_none());
}

#[test]
fn git_complaints_become_actionable_errors() {
    for text in [
        "remote: Repository not found.\nfatal: repository 'https://github.com/o/r.git/' not found",
        "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
        "git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.",
    ] {
        assert!(matches!(failure(text), GitError::NotFound), "{text}");
    }

    let GitError::Failed(text) = failure("warning: x\nfatal: something odd\n") else {
        panic!("expected a plain failure");
    };
    assert_eq!(text, "fatal: something odd");
}

#[tokio::test]
async fn git_that_is_not_installed_is_a_typed_error() {
    let run = Git::new()
        .run_with_program(
            "beep-no-such-git",
            Path::new("."),
            LOOK_LIMIT,
            ["--version"],
        )
        .await;

    assert!(matches!(run, Err(GitError::Missing)));
}

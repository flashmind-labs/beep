use super::*;

fn keeps_only(config: &str) -> String {
    kept(config.as_bytes())
}

const GOOD: &str = "[core]\n\
    \trepositoryformatversion = 0\n\
    \tfilemode = true\n\
    \tbare = false\n\
    \tlogallrefupdates = true\n\
    [remote \"origin\"]\n\
    \turl = git@example.com:team/app.git\n\
    \tfetch = +refs/heads/*:refs/remotes/origin/*\n\
    [branch \"main\"]\n\
    \tremote = origin\n\
    \tmerge = refs/heads/main\n";

#[test]
fn the_keys_a_checkout_needs_stay() {
    let kept = keeps_only(GOOD);
    for line in [
        "repositoryformatversion = \"0\"",
        "filemode = \"true\"",
        "url = \"git@example.com:team/app.git\"",
        "fetch = \"+refs/heads/*:refs/remotes/origin/*\"",
        "merge = \"refs/heads/main\"",
    ] {
        assert!(kept.contains(line), "{line} missing from\n{kept}");
    }
    // Reading what it wrote gives the same again.
    assert_eq!(keeps_only(&kept), kept);
}

#[test]
fn keys_that_make_git_run_a_program_are_dropped() {
    let hostile = [
        "[core]\n\tfsmonitor = /tmp/payload\n",
        "[core]\n\tsshCommand = /tmp/payload\n",
        "[core]\n\thooksPath = /tmp/hooks\n",
        "[core]\n\teditor = /tmp/payload\n\tpager = /tmp/payload\n",
        "[alias]\n\tx = !/tmp/payload\n",
        "[filter \"lfs\"]\n\tclean = /tmp/payload\n\tsmudge = /tmp/payload\n",
        "[diff \"x\"]\n\ttextconv = /tmp/payload\n",
        "[credential]\n\thelper = !/tmp/payload\n",
        "[include]\n\tpath = /tmp/more\n",
        "[includeIf \"gitdir:/\"]\n\tpath = /tmp/more\n",
        "[url \"/tmp/x\"]\n\tinsteadOf = https://example.com/\n",
        "[remote \"origin\"]\n\tpushurl = ext::sh -c payload\n\tuploadpack = /tmp/payload\n",
        "[extensions]\n\tworktreeConfig = true\n\tobjectformat = sha999\n\trefstorage = /tmp/x\n",
        "[extensions]\n\tpartialClone = origin\n\tcompatobjectformat = sha1\n",
    ];
    for config in hostile {
        assert_eq!(keeps_only(config), "", "{config}");
    }
}

#[test]
fn case_variants_do_not_get_past() {
    for config in [
        "[CORE]\n\tFsMonitor = /tmp/payload\n",
        "[Core]\n\tSSHCOMMAND = /tmp/payload\n",
        "[core]\n\tHOOKSPATH = /tmp/hooks\n",
        "[ALIAS]\n\tx = !payload\n",
        "[Include]\n\tPath = /tmp/more\n",
        "[core]\nfsmonitor=/tmp/payload\n",
        "[core] fsmonitor = /tmp/payload\n",
        "[core]\n\tbare = false\n\tfsmonitor = a \\\n b\n",
    ] {
        let kept = keeps_only(config);
        assert!(
            !kept.to_lowercase().contains("fsmonitor")
                && !kept.to_lowercase().contains("sshcommand")
                && !kept.to_lowercase().contains("hookspath")
                && !kept.to_lowercase().contains("include")
                && !kept.to_lowercase().contains("alias")
                && !kept.contains("payload"),
            "{config} kept\n{kept}"
        );
    }
    // The same key in another case is the same key, so it stays.
    assert!(keeps_only("[CORE]\n\tFileMode = false\n").contains("filemode"));
}

#[test]
fn a_program_cannot_hide_in_a_kept_key() {
    for url in [
        "ext::sh -c payload",
        "EXT::sh -c payload",
        "fd::7",
        "-oProxyCommand=payload",
        "--upload-pack=payload",
        "foo::bar",
    ] {
        let config =
            format!("[remote \"origin\"]\n\turl = {url}\n[branch \"main\"]\n\tremote = {url}\n");
        assert_eq!(keeps_only(&config), "", "{url}");
    }
    for url in [
        "https://example.com/a.git",
        "git@example.com:a/b.git",
        "ssh://host/a.git",
        "/srv/git/a.git",
        "../a.git",
        "C:/work/a.git",
    ] {
        let config = format!("[remote \"origin\"]\n\turl = {url}\n");
        assert!(keeps_only(&config).contains("url ="), "{url}");
    }
    // A core value is one word, so it cannot carry a command.
    assert!(!keeps_only("[core]\n\tfilemode = a; /tmp/payload\n").contains("payload"));
    assert_eq!(keeps_only("[core]\n\tfilemode = a /tmp/payload\n"), "");
    assert_eq!(keeps_only("[core]\n\tfilemode = \"x y\"\n"), "");
}

#[test]
fn a_line_that_does_not_parse_does_not_end_the_rest() {
    let config = "[core\n\tfsmonitor = /tmp/payload\n[remote \"o\"]\n\turl = https://e/a\n\t= x\n\tbad value\n\tfetch = +a:b\n";
    let kept = keeps_only(config);
    assert!(kept.contains("url = \"https://e/a\""), "{kept}");
    assert!(kept.contains("fetch = \"+a:b\""), "{kept}");
    assert!(!kept.contains("fsmonitor"), "{kept}");
    // Bytes that are not text still give a config.
    assert_eq!(kept_bytes(b"\xff\xfe[core]\n\tfsmonitor = x\n"), "");
}

#[test]
fn the_repository_format_stays_when_it_names_a_known_one() {
    for (key, value) in [
        ("objectformat", "sha1"),
        ("objectformat", "sha256"),
        ("objectformat", "SHA256"),
        ("refstorage", "files"),
        ("refstorage", "reftable"),
    ] {
        let kept = keeps_only(&format!("[extensions]\n\t{key} = {value}\n"));
        assert!(
            kept.contains(&format!("{key} = \"{value}\"")),
            "{key} {value}: {kept}"
        );
    }
    for (key, value) in [
        ("objectformat", "sha999"),
        ("objectformat", ""),
        ("objectformat", "sha256 /tmp/x"),
        ("refstorage", "reftable2"),
        ("refstorage", "/tmp/store"),
    ] {
        let kept = keeps_only(&format!("[extensions]\n\t{key} = \"{value}\"\n"));
        assert_eq!(kept, "", "{key} {value}");
    }
}

fn git_ok(dir: &std::path::Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .args(args)
        .output()
        .expect("git runs");
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Git opens a repository whose config went through `kept`, in its own
/// object and ref format.
#[test]
fn git_opens_a_repository_with_its_config_rewritten() {
    for (flags, show, want) in [
        (
            vec!["--object-format=sha256"],
            "--show-object-format",
            "sha256",
        ),
        (
            vec!["--ref-format=reftable"],
            "--show-ref-format",
            "reftable",
        ),
    ] {
        let source = tempfile::tempdir().expect("a temp folder");
        let mut init = vec!["init", "-q"];
        init.extend(flags);
        if git_ok(source.path(), &init).is_none() {
            // This git does not know the format.
            continue;
        }
        let config = std::fs::read(source.path().join(".git/config")).expect("a config");
        let rewritten = kept(&config);
        assert!(
            rewritten.contains("repositoryformatversion = \"1\""),
            "{rewritten}"
        );
        assert!(rewritten.contains("[extensions]"), "{rewritten}");
        // A copy of the repository with only the rewritten config.
        let copy = tempfile::tempdir().expect("a temp folder");
        std::fs::create_dir(copy.path().join(".git")).expect("a Git folder");
        let mut dirs = vec![source.path().join(".git")];
        let mut at = 0;
        while at < dirs.len() {
            for entry in std::fs::read_dir(&dirs[at]).expect("a folder") {
                let path = entry.expect("an entry").path();
                let relative = path.strip_prefix(source.path()).expect("inside");
                if path.is_dir() {
                    std::fs::create_dir_all(copy.path().join(relative)).expect("made");
                    dirs.push(path);
                } else {
                    std::fs::copy(&path, copy.path().join(relative)).expect("copied");
                }
            }
            at += 1;
        }
        std::fs::write(copy.path().join(".git/config"), &rewritten).expect("written");
        assert_eq!(
            git_ok(copy.path(), &["rev-parse", show]).as_deref(),
            Some(want)
        );
        assert!(git_ok(copy.path(), &["status", "--porcelain"]).is_some());
    }
}

fn kept_bytes(raw: &[u8]) -> String {
    kept(raw)
}

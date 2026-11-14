use std::process::Command;

use beep_plugins::{Handler, Placement, Policy, PreviewKind, Topic};

use super::*;

struct Quiet;

impl Handler for Quiet {
    fn shown(&self, _placements: &[Placement]) {}

    fn wanted(&self, _topics: &[Topic]) {}
}

fn manifest(version: &str, permissions: &str) -> String {
    format!(
        r#"{{ "id": "cat-one", "name": "Cat", "version": "{version}", "description": "A cat.",
              "permissions": [{permissions}] }}"#
    )
}

/// Runs git in `dir` with a fixed identity and no user config.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.test"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit_all(repo: &Path) -> String {
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", "change"]);

    git(repo, &["rev-parse", "HEAD"])
}

/// A repository with a plugin at its root.
fn repo_with(version: &str, permissions: &str) -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(
        repo.path().join("beep-plugin.json"),
        manifest(version, permissions),
    )
    .unwrap();
    std::fs::write(repo.path().join("main.ts"), "// plugin").unwrap();
    commit_all(repo.path());

    repo
}

struct Setup {
    /// Keeps the folder until the test ends.
    _plugins: tempfile::TempDir,
    host: Host,
    installer: Installer,
}

fn setup() -> Setup {
    let plugins = tempfile::tempdir().unwrap();
    let host = Host::start(plugins.path().to_path_buf(), Quiet, Policy::Managed).unwrap();

    Setup {
        _plugins: plugins,
        host,
        installer: Installer::with(Git::allowing_files_for_tests()),
    }
}

impl Setup {
    fn dir(&self) -> &Path {
        self.host.dir()
    }

    fn request(&self, repo: &Path, reference: &str, folder: &str) -> GitRequest {
        GitRequest {
            url: RepoUrl::local_for_tests(repo),
            reference: (!reference.is_empty()).then(|| GitRef::parse(reference).unwrap()),
            subfolder: (!folder.is_empty()).then(|| Subfolder::parse(folder).unwrap()),
        }
    }

    async fn stage(&self, request: &GitRequest) -> Result<Staged, PluginsError> {
        let fetched = self.installer.fetch(self.dir(), request).await?;

        self.installer.preview(&self.host, fetched)
    }

    fn leftovers(&self) -> Vec<String> {
        std::fs::read_dir(self.dir())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with(STAGE_PREFIX))
            .collect()
    }
}

fn id() -> PluginId {
    "cat-one".parse().unwrap()
}

fn commit_of(origin: &Origin) -> String {
    match origin {
        Origin::Git(git) => git.commit.to_string(),
        other => panic!("expected a git origin, got {other:?}"),
    }
}

#[tokio::test]
async fn staging_previews_without_installing_and_confirming_installs() {
    let repo = repo_with("0.1.0", r#""ui.add""#);
    let head = git(repo.path(), &["rev-parse", "HEAD"]);
    let set = setup();

    let staged = set.stage(&set.request(repo.path(), "", "")).await.unwrap();

    let preview = &staged.preview;
    assert_eq!(preview.kind, PreviewKind::New);
    assert_eq!(preview.id, id());
    assert_eq!(preview.name, "Cat");
    assert_eq!(preview.description, "A cat.");
    assert_eq!(preview.added.len(), 1);
    assert_eq!(commit_of(&preview.source), head);
    assert!(
        !set.dir().join("cat-one").exists(),
        "nothing is installed yet"
    );
    assert!(set.host.status().is_empty(), "nothing runs yet");
    assert!(!set.dir().join(STAGE_PREFIX).exists());
    assert_eq!(set.leftovers().len(), 1);

    let installed = set.installer.confirm(&set.host, &staged.stage).unwrap();

    assert_eq!(installed, id());
    assert!(set.dir().join("cat-one/main.ts").is_file());
    assert!(
        !set.dir().join("cat-one/.git").exists(),
        "the repository's own files stay out"
    );
    assert!(set.leftovers().is_empty());
    let record = set.host.record(&id()).unwrap();
    assert_eq!(commit_of(&record.origin), head);
    assert_eq!(record.approved.len(), 1);
    assert_eq!(set.host.status().len(), 1);
}

#[tokio::test]
async fn cancelling_deletes_the_staging_folder() {
    let repo = repo_with("0.1.0", "");
    let set = setup();
    let staged = set.stage(&set.request(repo.path(), "", "")).await.unwrap();
    assert_eq!(set.leftovers().len(), 1);

    set.installer.cancel(&staged.stage);

    assert!(set.leftovers().is_empty());
    assert!(!set.dir().join("cat-one").exists());
    assert!(matches!(
        set.installer.confirm(&set.host, &staged.stage),
        Err(PluginsError::UnknownStage)
    ));
}

#[tokio::test]
async fn a_plugin_in_a_subfolder_is_found_and_the_rest_is_left_out() {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    let folder = repo.path().join("plugins/cat");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("beep-plugin.json"), manifest("0.1.0", "")).unwrap();
    std::fs::write(folder.join("main.ts"), "// plugin").unwrap();
    std::fs::write(repo.path().join("README.md"), "readme").unwrap();
    commit_all(repo.path());
    let set = setup();

    let staged = set
        .stage(&set.request(repo.path(), "", "plugins/cat"))
        .await
        .unwrap();
    set.installer.confirm(&set.host, &staged.stage).unwrap();

    assert!(set.dir().join("cat-one/main.ts").is_file());
    assert!(!set.dir().join("cat-one/README.md").exists());
    let Origin::Git(origin) = set.host.record(&id()).unwrap().origin else {
        panic!("expected a git origin");
    };
    assert_eq!(origin.subfolder.as_deref(), Some(Path::new("plugins/cat")));

    let missing = set
        .stage(&set.request(repo.path(), "", "plugins/nope"))
        .await;
    assert!(matches!(missing, Err(PluginsError::NoManifest(_))));
}

#[tokio::test]
async fn a_branch_a_tag_and_a_missing_ref() {
    let repo = repo_with("0.1.0", "");
    git(repo.path(), &["tag", "v1"]);
    git(repo.path(), &["checkout", "-q", "-b", "next"]);
    std::fs::write(repo.path().join("beep-plugin.json"), manifest("0.2.0", "")).unwrap();
    let next = commit_all(repo.path());
    git(repo.path(), &["checkout", "-q", "main"]);
    let set = setup();

    let tag = set
        .stage(&set.request(repo.path(), "v1", ""))
        .await
        .unwrap();
    assert_eq!(tag.preview.version, semver::Version::new(0, 1, 0));
    set.installer.cancel(&tag.stage);

    let branch = set
        .stage(&set.request(repo.path(), "next", ""))
        .await
        .unwrap();
    assert_eq!(branch.preview.version, semver::Version::new(0, 2, 0));
    assert_eq!(commit_of(&branch.preview.source), next);
    set.installer.cancel(&branch.stage);

    let sha = set
        .stage(&set.request(repo.path(), &next, ""))
        .await
        .unwrap();
    assert_eq!(sha.preview.version, semver::Version::new(0, 2, 0));
    set.installer.cancel(&sha.stage);

    let missing = set.stage(&set.request(repo.path(), "nope", "")).await;
    assert!(matches!(
        missing,
        Err(PluginsError::Git(GitError::RefNotFound(name))) if name == "nope"
    ));
    assert!(set.leftovers().is_empty());
}

#[tokio::test]
async fn a_repository_that_does_not_exist_is_not_found() {
    let set = setup();
    let gone = tempfile::tempdir().unwrap();

    let result = set
        .stage(&set.request(&gone.path().join("nope"), "", ""))
        .await;

    assert!(matches!(result, Err(PluginsError::Git(GitError::NotFound))));
    assert!(set.leftovers().is_empty());
}

#[tokio::test]
async fn a_repository_without_a_manifest_or_with_a_bad_one_is_refused() {
    let none = tempfile::tempdir().unwrap();
    git(none.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(none.path().join("README.md"), "x").unwrap();
    commit_all(none.path());
    let bad = repo_with("0.1.0", "");
    std::fs::write(bad.path().join("beep-plugin.json"), "{ nope").unwrap();
    commit_all(bad.path());
    let set = setup();

    let missing = set.stage(&set.request(none.path(), "", "")).await;
    let invalid = set.stage(&set.request(bad.path(), "", "")).await;

    let Err(PluginsError::NoManifest(text)) = &missing else {
        panic!("expected no manifest, got {missing:?}");
    };
    assert!(text.contains("beep-plugin.json"));
    assert!(matches!(invalid, Err(PluginsError::InvalidManifest(_))));
    assert!(set.leftovers().is_empty(), "failed stages are cleaned");
}

#[tokio::test]
async fn a_plugin_over_the_size_cap_is_refused() {
    let repo = repo_with("0.1.0", "");
    std::fs::write(
        repo.path().join("big.bin"),
        vec![0u8; usize::try_from(MAX_PLUGIN_BYTES).unwrap() + 1],
    )
    .unwrap();
    commit_all(repo.path());
    let set = setup();

    let result = set.stage(&set.request(repo.path(), "", "")).await;

    assert!(matches!(result, Err(PluginsError::TooLarge)));
    assert!(set.leftovers().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlink_out_of_the_plugin_is_refused() {
    let repo = repo_with("0.1.0", "");
    std::os::unix::fs::symlink("/etc/hosts", repo.path().join("hosts")).unwrap();
    commit_all(repo.path());
    let set = setup();

    let result = set.stage(&set.request(repo.path(), "", "")).await;

    assert!(matches!(result, Err(PluginsError::UnsafeLink(path)) if path == "hosts"));
    assert!(set.leftovers().is_empty());
}

#[tokio::test]
async fn an_id_installed_from_another_source_is_refused_and_the_stage_is_cleaned() {
    let one = repo_with("0.1.0", "");
    let two = repo_with("0.1.0", "");
    let set = setup();
    let staged = set.stage(&set.request(one.path(), "", "")).await.unwrap();
    set.installer.confirm(&set.host, &staged.stage).unwrap();

    let result = set.stage(&set.request(two.path(), "", "")).await;

    assert!(matches!(result, Err(PluginsError::AlreadyInstalled(found)) if found == id()));
    assert!(set.leftovers().is_empty());
}

#[tokio::test]
async fn an_update_shows_the_permission_difference_and_never_runs_on_its_own() {
    let repo = repo_with("0.1.0", r#""ui.add""#);
    let first = git(repo.path(), &["rev-parse", "HEAD"]);
    let set = setup();
    let request = set.request(repo.path(), "", "");
    let staged = set.stage(&request).await.unwrap();
    set.installer.confirm(&set.host, &staged.stage).unwrap();
    let Origin::Git(installed) = set.host.record(&id()).unwrap().origin else {
        panic!("expected a git origin");
    };
    assert_eq!(
        set.installer.check_update(&installed).await.unwrap(),
        UpdateCheck::Current
    );

    std::fs::write(
        repo.path().join("beep-plugin.json"),
        manifest("0.2.0", r#""sessions.read""#),
    )
    .unwrap();
    let second = commit_all(repo.path());
    let check = set.installer.check_update(&installed).await.unwrap();
    assert_eq!(
        check,
        UpdateCheck::Available {
            commit: second.parse().unwrap()
        }
    );
    assert_eq!(
        set.host.status()[0].version,
        semver::Version::new(0, 1, 0),
        "checking does not change anything"
    );

    let update = set
        .stage(&GitRequest::of(&installed).unwrap())
        .await
        .unwrap();

    assert_eq!(update.preview.kind, PreviewKind::Update);
    assert_eq!(
        update.preview.previous_version,
        Some(semver::Version::new(0, 1, 0))
    );
    assert_eq!(update.preview.added.len(), 1);
    assert_eq!(update.preview.removed.len(), 1);
    assert_eq!(commit_of(&update.preview.source), second);
    assert_eq!(
        set.host.status()[0].version,
        semver::Version::new(0, 1, 0),
        "a staged update is not installed"
    );

    set.installer.confirm(&set.host, &update.stage).unwrap();
    assert_eq!(set.host.status()[0].version, semver::Version::new(0, 2, 0));
    assert_eq!(commit_of(&set.host.record(&id()).unwrap().origin), second);
    assert_ne!(first, second);
}

#[tokio::test]
async fn a_plugin_pinned_to_a_commit_has_nothing_newer() {
    let repo = repo_with("0.1.0", "");
    let head = git(repo.path(), &["rev-parse", "HEAD"]);
    let set = setup();
    let staged = set
        .stage(&set.request(repo.path(), &head, ""))
        .await
        .unwrap();
    set.installer.confirm(&set.host, &staged.stage).unwrap();
    let Origin::Git(installed) = set.host.record(&id()).unwrap().origin else {
        panic!("expected a git origin");
    };

    let check = set.installer.check_update(&installed).await.unwrap();

    assert_eq!(check, UpdateCheck::Pinned);
}

#[test]
fn a_local_folder_is_copied_without_git_and_previews_as_local() {
    let source = tempfile::tempdir().unwrap();
    std::fs::write(
        source.path().join("beep-plugin.json"),
        manifest("0.1.0", r#""ui.add""#),
    )
    .unwrap();
    std::fs::write(source.path().join("main.ts"), "// plugin").unwrap();
    std::fs::create_dir(source.path().join(".git")).unwrap();
    std::fs::write(source.path().join(".git/config"), "x").unwrap();
    let set = setup();

    let fetched = set.installer.copy_local(set.dir(), source.path()).unwrap();
    let staged = set.installer.preview(&set.host, fetched).unwrap();

    assert_eq!(staged.preview.source, Origin::Local);
    assert_eq!(staged.preview.kind, PreviewKind::New);
    assert!(
        source.path().join("beep-plugin.json").exists(),
        "the source stays"
    );
    set.installer.confirm(&set.host, &staged.stage).unwrap();
    assert!(set.dir().join("cat-one/main.ts").is_file());
    assert!(!set.dir().join("cat-one/.git").exists());
}

#[test]
fn a_local_folder_without_a_manifest_is_refused() {
    let source = tempfile::tempdir().unwrap();
    let set = setup();

    let result = set.installer.copy_local(set.dir(), source.path());

    assert!(matches!(result, Err(PluginsError::NoManifest(_))));
    assert!(set.leftovers().is_empty());
}

#[test]
fn copying_stops_at_the_size_cap() {
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("a.bin"), vec![0u8; 600]).unwrap();
    std::fs::write(source.path().join("b.bin"), vec![0u8; 600]).unwrap();
    let target = tempfile::tempdir().unwrap();

    let result = copy_tree(source.path(), &target.path().join("out"), 1000);

    assert!(matches!(result, Err(PluginsError::TooLarge)));
}

#[cfg(unix)]
#[test]
fn copying_refuses_links_that_leave_the_folder() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), "x").unwrap();
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("main.ts"), "x").unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret"), source.path().join("link")).unwrap();
    let target = tempfile::tempdir().unwrap();

    let result = copy_tree(source.path(), &target.path().join("out"), MAX_PLUGIN_BYTES);

    assert!(matches!(result, Err(PluginsError::UnsafeLink(path)) if path == "link"));
}

#[cfg(unix)]
#[test]
fn copying_keeps_a_link_to_a_file_inside_as_a_file() {
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("main.ts"), "body").unwrap();
    std::os::unix::fs::symlink("main.ts", source.path().join("alias.ts")).unwrap();
    let target = tempfile::tempdir().unwrap();
    let out = target.path().join("out");

    copy_tree(source.path(), &out, MAX_PLUGIN_BYTES).unwrap();

    assert_eq!(
        std::fs::read_to_string(out.join("alias.ts")).unwrap(),
        "body"
    );
    assert!(!out.join("alias.ts").is_symlink());
}

#[test]
fn stale_staging_folders_are_cleaned_and_plugins_are_not() {
    let plugins = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(plugins.path().join(".stage-abc/cat-one")).unwrap();
    std::fs::create_dir_all(plugins.path().join("cat-one")).unwrap();
    std::fs::write(plugins.path().join(".plugins.json"), "{}").unwrap();

    clean_stale(plugins.path());

    assert!(!plugins.path().join(".stage-abc").exists());
    assert!(plugins.path().join("cat-one").exists());
    assert!(plugins.path().join(".plugins.json").exists());
}

#[test]
fn requests_are_checked_and_empty_text_means_none() {
    let request = GitRequest::parse("owner/repo", "  ", "").unwrap();
    assert!(request.reference.is_none() && request.subfolder.is_none());
    assert_eq!(request.url.as_str(), "https://github.com/owner/repo.git");

    for (repository, reference, folder) in [
        ("/tmp/repo", "", ""),
        ("owner/repo", "-x", ""),
        ("owner/repo", "", "../x"),
    ] {
        assert!(matches!(
            GitRequest::parse(repository, reference, folder),
            Err(PluginsError::Invalid(_))
        ));
    }
}

#[test]
fn errors_reach_the_page_as_a_kind_and_a_message() {
    let value = serde_json::to_value(PluginsError::Git(GitError::Missing)).unwrap();
    assert_eq!(value["kind"], "git");
    assert!(
        value["message"]
            .as_str()
            .unwrap()
            .contains("Git is not installed")
    );

    let value = serde_json::to_value(PluginsError::TooLarge).unwrap();
    assert_eq!(value["kind"], "too-large");

    let value = serde_json::to_value(PluginsError::NoManifest("beep-plugin.json".into())).unwrap();
    assert_eq!(value["kind"], "no-manifest");
    let value = serde_json::to_value(PluginsError::UnknownStage).unwrap();
    assert_eq!(value["kind"], "unknown-stage");
}

#[test]
fn a_stage_id_round_trips_and_rejects_other_text() {
    let stage = StageId::new();

    assert_eq!(stage.to_string().parse::<StageId>().unwrap(), stage);
    assert!(matches!(
        "../../etc".parse::<StageId>(),
        Err(PluginsError::UnknownStage)
    ));
}

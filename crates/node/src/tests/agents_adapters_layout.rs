use super::*;

#[test]
fn node_and_npm_are_where_each_os_unpacks_them() {
    let root = Path::new("/srv/node/agent-adapters/node");
    assert_eq!(
        node_program(root, "linux"),
        Path::new("/srv/node/agent-adapters/node/bin/node")
    );
    assert_eq!(
        node_program(root, "windows"),
        Path::new("/srv/node/agent-adapters/node/node.exe")
    );
    assert!(npm_script(root, "macos").ends_with("lib/node_modules/npm/bin/npm-cli.js"));
    assert!(npm_script(root, "windows").ends_with("node/node_modules/npm/bin/npm-cli.js"));
    assert_eq!(
        package_dir(Path::new("/p"), "@agentclientprotocol/claude-agent-acp"),
        Path::new("/p/node_modules/@agentclientprotocol/claude-agent-acp")
    );
    assert_eq!(
        package_dir(Path::new("/p"), "pi-acp"),
        Path::new("/p/node_modules/pi-acp")
    );
}

#[test]
fn the_script_comes_from_the_packages_bin() {
    let named = r#"{"name":"@agentclientprotocol/claude-agent-acp","bin":{"claude-agent-acp":"dist/index.js"}}"#;
    assert_eq!(
        bin_script(named, "claude-agent-acp"),
        Some(PathBuf::from("dist/index.js"))
    );
    assert_eq!(bin_script(named, "codex-acp"), None);
    let one = r#"{"name":"pi-acp","bin":"./dist/index.js"}"#;
    assert_eq!(
        bin_script(one, "pi-acp"),
        Some(PathBuf::from("./dist/index.js"))
    );
    let outside = r#"{"name":"pi-acp","bin":{"pi-acp":"../../evil.js"}}"#;
    assert_eq!(bin_script(outside, "pi-acp"), None);
    assert_eq!(bin_script(r#"{"name":"pi-acp"}"#, "pi-acp"), None);
}

#[test]
fn an_adapter_counts_only_when_node_and_its_script_are_there() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let claude = crate::known::known("claude").expect("claude");
    assert_eq!(installed(dir.path(), claude), None);
    let root = root(dir.path());
    let node = node_program(&node_root(&root), env::consts::OS);
    fs::create_dir_all(node.parent().expect("a folder")).expect("folders");
    fs::write(&node, "").expect("node");
    assert_eq!(installed(dir.path(), claude), None, "no adapter yet");
    let package = package_dir(
        &prefix(&root, "claude"),
        "@agentclientprotocol/claude-agent-acp",
    );
    fs::create_dir_all(package.join("dist")).expect("folders");
    fs::write(
        package.join("package.json"),
        r#"{"name":"@agentclientprotocol/claude-agent-acp","bin":{"claude-agent-acp":"dist/index.js"}}"#,
    )
    .expect("a manifest");
    assert_eq!(installed(dir.path(), claude), None, "no script yet");
    fs::write(package.join("dist/index.js"), "").expect("a script");
    assert_eq!(
        installed(dir.path(), claude),
        Some(Installed {
            node,
            script: package.join("dist/index.js"),
            current: false,
        }),
        "no version, so installed again"
    );
}

#[test]
fn an_adapter_older_than_its_minimum_is_not_current() {
    let dir = tempfile::tempdir().expect("a temp folder");
    let codex = crate::known::known("codex").expect("codex");
    let root = root(dir.path());
    let node = node_program(&node_root(&root), env::consts::OS);
    fs::create_dir_all(node.parent().expect("a folder")).expect("folders");
    fs::write(&node, "").expect("node");
    let package = package_dir(&prefix(&root, "codex"), "@agentclientprotocol/codex-acp");
    fs::create_dir_all(package.join("dist")).expect("folders");
    fs::write(package.join("dist/index.js"), "").expect("a script");
    let current = |version: &str| {
        let manifest = format!(
            r#"{{"name":"@agentclientprotocol/codex-acp","version":"{version}","bin":{{"codex-acp":"dist/index.js"}}}}"#
        );
        fs::write(package.join("package.json"), manifest).expect("a manifest");
        installed(dir.path(), codex).expect("installed").current
    };

    assert!(!current("2.5.0"));
    assert!(current("2.7.0"));
    assert!(current("3.0.1"));
}

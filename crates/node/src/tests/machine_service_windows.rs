use super::super::tests::launch;
use super::*;

fn windows_launch(workspace: Option<&str>) -> Launch {
    let mut launch = launch(workspace);
    launch.exe = PathBuf::from(r"\\?\C:\Users\user\AppData\Local\Programs\beep-node\beep-node.exe");
    launch.dir = PathBuf::from(r"\\?\C:\Users\user\AppData\Roaming\beep node");
    launch
}

fn xml(launch: &Launch) -> String {
    task_xml(
        launch,
        r"HOST\user",
        Path::new(r"C:\Windows\System32\conhost.exe"),
        Path::new(r"C:\Users\user\AppData\Local\beep-node\logs\beep-node.log"),
    )
}

#[test]
fn the_task_passes_the_workspace_and_escapes_it() {
    let xml = xml(&windows_launch(Some(r"D:\work & play\")));
    assert!(
        xml.contains(r#"--workspace &quot;D:\work &amp; play\\&quot; run</Arguments>"#),
        "{xml}"
    );
}

#[test]
fn arguments_quote_as_windows_reads_them() {
    assert_eq!(quote_arg(r"C:\plain\path"), r"C:\plain\path");
    assert_eq!(quote_arg(""), r#""""#);
    assert_eq!(quote_arg(r"C:\with space\"), r#""C:\with space\\""#);
    assert_eq!(quote_arg(r#"say "hi"\"#), r#""say \"hi\"\\""#);
    assert_eq!(quote_arg(r#"a\"b"#), r#""a\\\"b""#);
}

#[test]
fn verbatim_paths_lose_their_prefix() {
    assert_eq!(plain(Path::new(r"\\?\C:\x\y.exe")), r"C:\x\y.exe");
    assert_eq!(
        plain(Path::new(r"\\?\UNC\server\share\y")),
        r"\\server\share\y"
    );
    assert_eq!(plain(Path::new(r"C:\x")), r"C:\x");
}

#[test]
fn a_restart_only_follows_a_task_that_runs_this_binary() {
    let xml = from_utf16(&utf16(&xml(&windows_launch(None))));
    let exe = r"\\?\C:\Users\user\AppData\Local\Programs\beep-node\beep-node.exe";
    assert!(runs(&xml, Path::new(exe)));
    assert!(!runs(&xml, Path::new(r"C:\Users\user\beep-node.exe")));
}

#[test]
fn the_task_names_its_folder() {
    let xml = from_utf16(&utf16(&xml(&windows_launch(Some(r"D:\work & play\")))));
    assert_eq!(
        dir_in(&xml),
        Some(PathBuf::from(r"C:\Users\user\AppData\Roaming\beep node"))
    );
    let sample = "<Exec><Command>c.exe</Command><Arguments>--headless C:\\n.exe --log-file C:\\l.log \
        --with-parent --dir C:\\Users\\u\\.node run</Arguments></Exec>";
    assert_eq!(dir_in(sample), Some(PathBuf::from(r"C:\Users\u\.node")));
    assert_eq!(dir_in("<Exec/>"), None);
}

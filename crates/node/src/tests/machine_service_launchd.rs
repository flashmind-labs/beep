use super::super::tests::launch;
use super::*;

#[test]
fn a_restart_only_follows_a_plist_that_runs_this_binary() {
    let plist = plist(&launch(None), Path::new("/l.log"));
    assert!(runs(&plist, Path::new("/opt/beep node/beep-node")));
    assert!(!runs(&plist, Path::new("/opt/beep node/beep")));
    assert!(!runs(&plist, Path::new("/srv/node")));
}

#[test]
fn the_plist_passes_the_workspace_and_escapes_it() {
    let plist = plist(&launch(Some("/Users/user/R&D <new>")), Path::new("/l.log"));
    assert!(plist.contains(
        "<string>--workspace</string>\n        <string>/Users/user/R&amp;D &lt;new&gt;</string>\n        <string>run</string>"
    ), "{plist}");
}

#[test]
fn an_older_plist_is_rewritten_with_its_arguments_and_path() {
    let current = plist(
        &launch(Some("/home/user/work")),
        Path::new("/Users/user/Library/Logs/beep-node.log"),
    );
    assert_eq!(refreshed(&current), None);
    // What `service install` wrote before the node's own log filter.
    let older = current.replace(
        "    </dict>\n    <key>RunAtLoad",
        "        <key>RUST_LOG</key>\n        <string>info</string>\n    </dict>\n    <key>RunAtLoad",
    );
    assert_ne!(older, current);
    assert_eq!(refreshed(&older).as_deref(), Some(current.as_str()));
}

#[test]
fn the_plist_names_its_folder() {
    let mut spaced = launch(None);
    spaced.dir = PathBuf::from("/srv/a & b/node");
    assert_eq!(
        dir_in(&plist(&spaced, Path::new("/l.log"))),
        Some(PathBuf::from("/srv/a & b/node"))
    );
    let sample = "<array>\n        <string>/usr/local/bin/beep-node</string>\n        \
        <string>--dir</string>\n        <string>/Users/u/Library/node</string>\n        \
        <string>run</string>\n    </array>";
    assert_eq!(dir_in(sample), Some(PathBuf::from("/Users/u/Library/node")));
    assert_eq!(dir_in("<array></array>"), None);
}

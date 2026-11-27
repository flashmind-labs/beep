use super::*;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::time::Duration;

struct Fixture {
    dir: PathBuf,
    shared: PathBuf,
    #[cfg_attr(windows, allow(dead_code))]
    outside: PathBuf,
    folders: Folders,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only a leftover folder in the temp folder if this fails.
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("beep-node-files-{name}-{}", std::process::id()));
    // A leftover from an earlier run with the same process id.
    let _ = fs::remove_dir_all(&dir);
    let shared = dir.join("shared");
    let outside = dir.join("outside");
    fs::create_dir_all(shared.join("src")).expect("made");
    fs::create_dir_all(&outside).expect("made");
    fs::write(shared.join("notes.txt"), "hello\nworld\n").expect("written");
    fs::write(outside.join("secret.txt"), "nope").expect("written");
    let folders = Folders::new(std::slice::from_ref(&shared));
    let shared = paths::canonicalize(shared).expect("exists");
    let outside = paths::canonicalize(outside).expect("exists");
    Fixture {
        dir,
        shared,
        outside,
        folders,
    }
}

fn error(reply: Reply) -> String {
    match reply {
        Reply::Error { message } => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn nothing_outside_the_shared_folders_is_reachable() {
    let f = fixture("outside");
    let secret = shown(&f.outside.join("secret.txt"));
    let message = error(f.folders.answer(Request::Read {
        path: secret.clone(),
        offset: 0,
        limit: 0,
    }));
    assert!(message.contains("outside the shared folders"), "{message}");
    error(f.folders.answer(Request::Read {
        path: "../outside/secret.txt".into(),
        offset: 0,
        limit: 0,
    }));
    // A link inside that points out is refused for reading and writing.
    symlink(&secret, f.shared.join("link.txt")).expect("linked");
    error(f.folders.answer(Request::Read {
        path: "link.txt".into(),
        offset: 0,
        limit: 0,
    }));
    error(f.folders.answer(Request::Write {
        path: "link.txt".into(),
        data: b"overwritten".to_vec(),
        append: false,
    }));
    // A dangling link is not followed either.
    symlink(f.outside.join("missing.txt"), f.shared.join("dangling.txt")).expect("linked");
    error(f.folders.answer(Request::Write {
        path: "dangling.txt".into(),
        data: b"x".to_vec(),
        append: false,
    }));
    assert!(!f.outside.join("missing.txt").exists());
    symlink(&f.outside, f.shared.join("out")).expect("linked");
    error(f.folders.answer(Request::Write {
        path: "out/new.txt".into(),
        data: b"x".to_vec(),
        append: false,
    }));
    error(f.folders.answer(Request::List {
        path: "out".into(),
        limit: 0,
    }));
    assert_eq!(
        fs::read_to_string(f.outside.join("secret.txt")).expect("read"),
        "nope"
    );
    assert!(!f.outside.join("new.txt").exists());
}

#[cfg(unix)]
#[test]
fn a_pipe_is_refused_rather_than_waited_on() {
    let f = fixture("pipe");
    let pipe = f.shared.join("out.png");
    let made = std::process::Command::new("mkfifo").arg(&pipe).status();
    assert!(made.is_ok_and(|status| status.success()), "mkfifo");
    let folders = f.folders.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let read = folders.answer(Request::Read {
            path: "out.png".into(),
            offset: 0,
            limit: 0,
        });
        let written = folders.answer(Request::Write {
            path: "out.png".into(),
            data: b"x".to_vec(),
            append: false,
        });
        // The test may have given up waiting.
        let _ = tx.send([read, written]);
    });
    // Only bounds a hang: the replies come back at once.
    let replies = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("opening the pipe waited for its other end");
    for reply in replies {
        error(reply);
    }
}

#[test]
fn a_node_without_shared_folders_refuses_everything() {
    let folders = Folders::new(&[]);
    assert!(folders.is_empty());
    let message = error(folders.answer(Request::Stat {
        path: "/srv/project".into(),
    }));
    assert!(message.contains("shares no folders"), "{message}");
}

use super::*;
use crate::files::paths;
use std::net::SocketAddr;
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::task::AbortOnDropHandle;

const KEY: &str = "fake-api-key-0123456789";

fn config(address: &str) -> String {
    format!(
        r#"<configuration version="37">
    <folder id="abcd-1234" label="Notes" path="~/notes" type="sendreceive"></folder>
    <device id="AAAAAAA-BBBBBBB" name="build-box"></device>
    <gui enabled="true" tls="false" debugging="false">
        <address>{address}</address>
        <apikey>{KEY}</apikey>
        <theme>default</theme>
    </gui>
</configuration>"#
    )
}

/// A home folder with Syncthing's config in it.
struct Home {
    dir: TempDir,
}

impl Home {
    fn new() -> Self {
        Self {
            dir: TempDir::new().expect("a temp folder"),
        }
    }

    fn path(&self) -> PathBuf {
        paths::canonicalize(self.dir.path()).expect("the home exists")
    }

    fn config_dir(&self) -> PathBuf {
        self.path().join(".local/state/syncthing")
    }

    fn write_config(&self, address: &str) {
        fs::create_dir_all(self.config_dir()).unwrap();
        fs::write(self.config_dir().join(CONFIG), config(address)).unwrap();
    }

    fn mkdir(&self, path: &str) -> PathBuf {
        let dir = self.path().join(path);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn syncthing(&self) -> Syncthing {
        Syncthing::at(vec![self.config_dir()], Some(self.path()))
    }
}

/// A fake Syncthing GUI answering `/rest/config/folders` with `folders`
/// to a request that carries the key, and 403 to one that does not.
async fn fake_api(folders: serde_json::Value) -> (SocketAddr, AbortOnDropHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = folders.to_string();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut request = Vec::new();
            let mut buf = [0; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&buf[..n]),
                }
            }
            let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
            let allowed = text.starts_with("get /rest/config/folders http/1.0\r\n")
                && text.contains(&format!("x-api-key: {}", KEY.to_ascii_lowercase()));
            let reply = if allowed {
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                )
            } else {
                "HTTP/1.1 403 Forbidden\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                    .to_owned()
            };
            let _ = stream.write_all(reply.as_bytes()).await;
        }
    });
    (addr, AbortOnDropHandle::new(task))
}

/// An address nothing listens on.
async fn closed_port() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    addr.to_string()
}

#[test]
fn the_config_gives_the_gui_and_key_and_never_prints_the_key() {
    let gui = parse_gui(&config("0.0.0.0:8384")).unwrap();
    assert_eq!(gui.addr, "127.0.0.1:8384");
    assert_eq!(gui.key.expose_secret(), KEY);
    assert!(!format!("{gui:?}").contains(KEY));

    let tls = config("127.0.0.1:8384").replace(r#"tls="false""#, r#"tls="true""#);
    assert!(parse_gui(&tls).is_err());
    assert!(parse_gui("<configuration></configuration>").is_err());
    assert!(parse_gui("not xml").is_err());
}

#[test]
fn gui_addresses_are_reached_on_loopback() {
    assert_eq!(gui_addr("[::]:8384", false).unwrap(), "[::1]:8384");
    assert_eq!(gui_addr(":8384", false).unwrap(), "127.0.0.1:8384");
    assert_eq!(
        gui_addr("http://localhost:8384/", false).unwrap(),
        "localhost:8384"
    );
    assert!(gui_addr("https://127.0.0.1:8384", false).is_err());
    assert!(gui_addr("unix:///run/syncthing.sock", false).is_err());
}

#[tokio::test]
async fn the_api_says_which_folder_holds_a_project_nested_or_through_a_link() {
    let home = Home::new();
    let sync = home.mkdir("sync");
    // Only the Unix half below reaches the folder through a link.
    #[cfg_attr(not(unix), expect(unused_variables))]
    let game = home.mkdir("sync/game");
    home.mkdir("other");
    // The API names the inner folder through a link, and a project is
    // asked about through another.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&game, home.path().join("game-link")).unwrap();
        std::os::unix::fs::symlink(&sync, home.path().join("sync-link")).unwrap();
    }
    let (addr, _server) = fake_api(serde_json::json!([
        {"id": "sync-1", "label": "Sync", "path": "~/sync", "type": "sendreceive"},
        {"id": "game-2", "label": "", "path": home.path().join("game-link")},
        {"id": "gone-3", "label": "Gone", "path": "~/not-there"},
        {"id": "off-4", "label": "Off", "path": "~/other", "paused": true},
    ]))
    .await;
    home.write_config(&addr.to_string());
    let syncthing = home.syncthing();

    let notes = syncthing.check(&sync.join("notes")).await.unwrap();
    assert_eq!(notes.label, "Sync");
    assert_eq!(notes.id.as_deref(), Some("sync-1"));
    assert_eq!(notes.root, sync);
    assert_eq!(notes.reason(), "Already synced by Syncthing (Sync)");
    assert_eq!(syncthing.check(&sync).await.unwrap().label, "Sync");

    #[cfg(unix)]
    {
        let inner = syncthing.check(&game.join("src")).await.unwrap();
        assert_eq!(inner.id.as_deref(), Some("game-2"));
        assert_eq!(inner.label, "game-2");
        let linked = syncthing
            .check(&home.path().join("sync-link/notes"))
            .await
            .unwrap();
        assert_eq!(linked.id.as_deref(), Some("sync-1"));
    }

    // The API answered: a paused folder and a marker elsewhere do not count.
    fs::write(home.path().join("other/.stfolder"), "").unwrap();
    assert_eq!(syncthing.check(&home.path().join("other")).await, None);
    assert_eq!(syncthing.check(&home.path().join("syncing")).await, None);

    let listed = syncthing.folders().await;
    assert_eq!(listed.len(), 3);
    assert_eq!(listed[0].path, sync.to_string_lossy());
}

#[tokio::test]
async fn a_project_nested_under_a_paused_folder_is_beeps_to_sync() {
    let home = Home::new();
    let project = home.mkdir("share/work/project");
    let kept = home.mkdir("kept/project");
    home.mkdir("kept/inner");
    fs::write(home.path().join("share").join(MARKER), "").unwrap();
    let (addr, _server) = fake_api(serde_json::json!([
        {"id": "share-1", "label": "Share", "path": "~/share", "paused": true},
        {"id": "kept-2", "label": "Kept", "path": "~/kept"},
        {"id": "inner-3", "label": "Inner", "path": "~/kept/inner", "paused": true},
    ]))
    .await;
    home.write_config(&addr.to_string());
    let syncthing = home.syncthing();
    // The share's marker stays while it is paused, and does not count.
    assert_eq!(syncthing.check(&project).await, None);
    assert_eq!(syncthing.check(&kept).await.unwrap().label, "Kept");
    // A folder syncing around a paused one still carries what is inside.
    let inner = syncthing.check(&home.path().join("kept/inner")).await;
    assert_eq!(inner.unwrap().label, "Kept");
}

#[tokio::test]
async fn without_the_api_a_marker_up_to_home_says_it() {
    let home = Home::new();
    let sync = home.mkdir("sync");
    let game = home.mkdir("sync/game/src");
    let loose = home.mkdir("loose/project");
    home.write_config(&closed_port().await);
    let syncthing = home.syncthing();
    assert_eq!(syncthing.check(&game).await, None);
    assert!(syncthing.folders().await.is_empty());

    fs::write(sync.join(MARKER), "").unwrap();
    let found = syncthing.check(&game).await.unwrap();
    assert_eq!(found.label, "sync");
    assert_eq!(found.id, None);
    assert_eq!(found.root, sync);
    assert_eq!(syncthing.check(&loose).await, None);

    // A marker above home does not count.
    let outer = TempDir::new().unwrap();
    fs::write(outer.path().join(MARKER), "").unwrap();
    let inside = outer.path().join("home/sam/work");
    fs::create_dir_all(&inside).unwrap();
    let below = Syncthing::at(Vec::new(), Some(outer.path().join("home/sam")));
    assert_eq!(below.check(&inside).await, None);
}

#[tokio::test]
async fn a_refused_key_or_no_config_falls_back_to_markers() {
    let home = Home::new();
    let sync = home.mkdir("sync");
    fs::write(sync.join(MARKER), "").unwrap();
    // No config at all.
    assert_eq!(home.syncthing().check(&sync).await.unwrap().label, "sync");

    let (addr, _server) = fake_api(serde_json::json!([])).await;
    home.write_config(&addr.to_string());
    let wrong = fs::read_to_string(home.config_dir().join(CONFIG))
        .unwrap()
        .replace(KEY, "another-key");
    fs::write(home.config_dir().join(CONFIG), wrong).unwrap();
    assert_eq!(home.syncthing().check(&sync).await.unwrap().label, "sync");
}

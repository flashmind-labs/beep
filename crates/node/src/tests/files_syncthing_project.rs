use super::*;

#[tokio::test]
async fn the_api_hands_one_project_between_syncthing_and_beep() {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use tokio_util::task::AbortOnDropHandle;
    let dir = tempfile::TempDir::new().unwrap();
    let share = super::real(dir.path());
    let project = share.join("owner/widget");
    let other = share.join("owner/other");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(share.join(".stignore"), "*.tmp\n").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    std::fs::write(share.join("config.xml"), format!(r#"<configuration><gui tls="false"><address>{address}</address><apikey>test-key</apikey></gui></configuration>"#)).unwrap();
    let serving = share.clone();
    let _server = AbortOnDropHandle::new(tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut raw = Vec::new();
            let mut buf = [0; 4096];
            let split = loop {
                let n = stream.read(&mut buf).await.unwrap();
                assert!(n > 0);
                raw.extend_from_slice(&buf[..n]);
                if let Some(split) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    break split + 4;
                }
            };
            let header = String::from_utf8(raw[..split].to_vec()).unwrap();
            let length: usize = header
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().unwrap())
                })
                .unwrap_or(0);
            while raw.len() < split + length {
                let n = stream.read(&mut buf).await.unwrap();
                assert!(n > 0);
                raw.extend_from_slice(&buf[..n]);
            }
            let request = header.lines().next().unwrap();
            let body = if request.starts_with("GET /rest/config/folders ") {
                serde_json::to_string(
                    &serde_json::json!([{"id":"share", "label":"Projects", "path":serving}]),
                )
                .unwrap()
            } else {
                assert!(
                    request.contains(" /rest/db/ignores?folder=share "),
                    "{request}"
                );
                if request.starts_with("POST ") {
                    let ignores: Ignores =
                        serde_json::from_slice(&raw[split..split + length]).unwrap();
                    std::fs::write(serving.join(".stignore"), ignores.ignore.join("\n") + "\n")
                        .unwrap();
                }
                let ignore: Vec<_> = std::fs::read_to_string(serving.join(".stignore"))
                    .unwrap()
                    .lines()
                    .map(str::to_owned)
                    .collect();
                serde_json::to_string(&Ignores { ignore }).unwrap()
            };
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        }
    }));
    let syncthing = Syncthing::at(vec![share.clone()], Some(share.clone()));
    assert!(syncthing.check(&project).await.is_some());
    assert!(
        !syncthing
            .project_files(&project, Some(false))
            .await
            .unwrap()
            .enabled
    );
    assert!(syncthing.check(&project).await.is_none());
    assert!(syncthing.check(&other).await.is_some());
    let rules = crate::files::sync::Rules::load(&project).unwrap();
    assert!(!rules.leaves_out(Path::new("src/main.rs"), false));
    assert!(rules.leaves_out(Path::new("cache.tmp"), false));
    assert!(
        syncthing
            .project_files(&project, Some(true))
            .await
            .unwrap()
            .enabled
    );
    assert!(syncthing.check(&project).await.is_some());
    assert_eq!(
        std::fs::read_to_string(share.join(".stignore")).unwrap(),
        format!("{}*.tmp\n", super::super::LOCAL_GIT)
    );
}

#[test]
fn project_toggles_keep_git_protection_first_without_duplicating_it() {
    let original: Vec<String> = format!("{}!keep\n*.tmp\n", super::super::LOCAL_GIT)
        .lines()
        .map(str::to_owned)
        .collect();
    let disabled = update(&original, "/owner/widget", false);
    assert!(disabled.join("\n").starts_with(super::super::LOCAL_GIT));
    assert_eq!(update(&disabled, "/owner/widget", false), disabled);
    assert_eq!(update(&disabled, "/owner/widget", true), original);
}

#[test]
fn changing_one_project_preserves_other_projects_and_user_rules() {
    let original = vec!["!keep".into(), "*.tmp".into()];
    let one = update(&original, "/owner/one", false);
    let two = update(&one, "/owner/two", false);
    assert_eq!(update(&two, "/owner/two", false), two);
    assert_eq!(
        update(&two, "/owner/one", true),
        update(&original, "/owner/two", false)
    );
    assert_eq!(update(&one, "/owner/one", true), original);
}

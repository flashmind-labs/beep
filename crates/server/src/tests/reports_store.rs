use super::*;
use std::fs::{read, read_dir};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn config() -> R2Config {
    R2Config {
        account_id: "account".into(),
        bucket: "reports".into(),
        access_key_id: SecretString::from("key-id"),
        secret_access_key: SecretString::from("secret"),
    }
}

/// The error text of a put to `endpoint`, which must fail.
async fn failed_put(endpoint: &str) -> String {
    let endpoint = endpoint.parse().expect("a local URL");
    let r2 = R2::at(endpoint, &config(), Client::new()).expect("a valid R2 config");
    let error = r2
        .put("reports/mesh/abc.zip", b"zip".to_vec(), "application/zip")
        .await
        .expect_err("the put fails");
    format!("{error:#}")
}

#[tokio::test]
async fn a_failed_r2_put_does_not_log_the_signed_url() {
    // Refused: a port nothing listens on any more.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let closed = listener.local_addr().expect("an address");
    drop(listener);
    // Answered: R2 says no.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let open = listener.local_addr().expect("an address");
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("the put connects");
        let mut read = Vec::new();
        let mut chunk = [0u8; 4096];
        while !read.ends_with(b"zip") {
            let n = stream.read(&mut chunk).await.expect("the put reads");
            assert!(n > 0, "the put ended early");
            read.extend_from_slice(&chunk[..n]);
        }
        let answer = "HTTP/1.1 403 Forbidden\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
        stream
            .write_all(answer.as_bytes())
            .await
            .expect("the answer writes");
    });
    for endpoint in [format!("http://{closed}"), format!("http://{open}")] {
        let text = failed_put(&endpoint).await;
        assert!(
            text.contains("putting reports/mesh/abc.zip in R2"),
            "{text}"
        );
        assert!(!text.contains("X-Amz"), "{text}");
        assert!(!text.contains("key-id"), "{text}");
    }
}

#[test]
fn r2_uploads_go_to_the_account_endpoint_path_style_and_signed() {
    let r2 = R2::new(&config(), Client::new()).expect("a valid R2 config");
    let url = r2.upload_url("reports/mesh/abc.zip", "application/zip");
    assert_eq!(url.host_str(), Some("account.r2.cloudflarestorage.com"));
    assert_eq!(url.path(), "/reports/reports/mesh/abc.zip");
    let query: Vec<String> = url.query_pairs().map(|(k, _)| k.into_owned()).collect();
    assert!(query.iter().any(|k| k == "X-Amz-Signature"), "{url}");
    assert!(url.as_str().contains("auto%2Fs3"), "{url}");
    assert!(!url.as_str().contains("secret"), "{url}");
}

#[tokio::test]
async fn a_folder_store_writes_the_object_at_its_key() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let store = ReportStore::Folder(dir.path().to_owned());
    store
        .put("reports/mesh/abc.json", b"{}".to_vec(), "application/json")
        .await
        .expect("the folder takes it");
    let path = dir.path().join("reports/mesh/abc.json");
    assert_eq!(read(path).expect("the object"), b"{}");
    let names = read_dir(dir.path().join("reports/mesh"))
        .expect("the folder")
        .count();
    assert_eq!(names, 1, "no temp file is left behind");
}

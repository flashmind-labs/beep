//! Where reports are kept: the private R2 bucket, or a local folder.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Client;
use reqwest::header::CONTENT_TYPE;
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use secrecy::{ExposeSecret, SecretString};
use tokio::fs;
use url::Url;
use uuid::Uuid;

use crate::config::{R2Config, ReportsConfig};

/// How long a presigned upload URL stays good.
const SIGNED_FOR: Duration = Duration::from_secs(60);

pub enum ReportStore {
    R2(R2),
    /// Objects are files under this folder, at their keys.
    Folder(PathBuf),
}

/// An R2 bucket, written through presigned PUTs.
pub struct R2 {
    bucket: Bucket,
    access_key_id: SecretString,
    secret_access_key: SecretString,
    http: Client,
}

impl ReportStore {
    /// The store `[reports]` names, if any.
    pub fn from_config(config: &ReportsConfig, http: Client) -> Result<Option<Self>> {
        if let Some(r2) = &config.r2 {
            return Ok(Some(Self::R2(R2::new(r2, http)?)));
        }
        Ok(config.folder.clone().map(Self::Folder))
    }

    pub async fn put(&self, key: &str, bytes: Vec<u8>, content_type: &str) -> Result<()> {
        match self {
            Self::R2(r2) => r2.put(key, bytes, content_type).await,
            Self::Folder(folder) => put_file(folder.join(key), bytes).await,
        }
    }
}

impl R2 {
    fn new(config: &R2Config, http: Client) -> Result<Self> {
        let endpoint: Url = format!("https://{}.r2.cloudflarestorage.com", config.account_id)
            .parse()
            .context("the R2 account id does not make a URL")?;
        Self::at(endpoint, config, http)
    }

    fn at(endpoint: Url, config: &R2Config, http: Client) -> Result<Self> {
        let bucket = Bucket::new(endpoint, UrlStyle::Path, config.bucket.clone(), "auto")
            .context("the R2 bucket")?;
        Ok(Self {
            bucket,
            access_key_id: config.access_key_id.clone(),
            secret_access_key: config.secret_access_key.clone(),
            http,
        })
    }

    /// A presigned PUT of `key`; the upload must send the same content type.
    fn upload_url(&self, key: &str, content_type: &str) -> Url {
        let credentials = Credentials::new(
            self.access_key_id.expose_secret(),
            self.secret_access_key.expose_secret(),
        );
        let mut action = self.bucket.put_object(Some(&credentials), key);
        action.headers_mut().insert("content-type", content_type);
        action.sign(SIGNED_FOR)
    }

    async fn put(&self, key: &str, bytes: Vec<u8>, content_type: &str) -> Result<()> {
        let url = self.upload_url(key, content_type);
        self.http
            .put(url)
            .header(CONTENT_TYPE, content_type)
            .body(bytes)
            .send()
            .await
            .and_then(|reply| reply.error_for_status())
            // The URL carries the access key id and a live signature.
            .map_err(reqwest::Error::without_url)
            .with_context(|| format!("putting {key} in R2"))?;
        Ok(())
    }
}

/// Writes `path` whole or not at all: a temp file beside it, then a rename.
async fn put_file(path: PathBuf, bytes: Vec<u8>) -> Result<()> {
    let dir = path.parent().context("an object key names a file")?;
    fs::create_dir_all(dir)
        .await
        .with_context(|| format!("making {}", dir.display()))?;
    let temp = dir.join(format!(".{}.tmp", Uuid::new_v4()));
    fs::write(&temp, bytes)
        .await
        .with_context(|| format!("writing {}", temp.display()))?;
    if let Err(e) = fs::rename(&temp, &path).await {
        // The temp file is of no use once the rename failed.
        let _ = fs::remove_file(&temp).await;
        return Err(e).with_context(|| format!("moving a report to {}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
}

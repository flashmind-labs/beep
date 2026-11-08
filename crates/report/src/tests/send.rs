use super::*;
use iroh::{PublicKey, Signature};
use std::collections::HashMap;
use std::future::pending;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// One request as the stand-in server read it.
struct Seen {
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

/// A server on a free port that answers one request with `status` and
/// `reply`, and hands back what it read.
async fn stand_in(status: &'static str, reply: Vec<u8>) -> (Url, JoinHandle<Seen>) {
    stand_in_typed(status, "application/msgpack", reply).await
}

/// `stand_in` with the reply's content type given.
async fn stand_in_typed(
    status: &'static str,
    content_type: &'static str,
    reply: Vec<u8>,
) -> (Url, JoinHandle<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a free local port binds");
    let addr = listener
        .local_addr()
        .expect("a bound listener has an address");
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("send connects");
        let mut read = Vec::new();
        let mut chunk = [0u8; 8192];
        let head_end = loop {
            let n = stream.read(&mut chunk).await.expect("the request reads");
            assert!(n > 0, "the request ended before its headers");
            read.extend_from_slice(&chunk[..n]);
            if let Some(at) = read.windows(4).position(|w| w == b"\r\n\r\n") {
                break at + 4;
            }
        };
        let head = String::from_utf8(read[..head_end].to_vec()).expect("headers are text");
        let mut lines = head.split("\r\n");
        let path = lines
            .next()
            .and_then(|l| l.split(' ').nth(1))
            .expect("a request line")
            .to_owned();
        let headers: HashMap<String, String> = lines
            .filter_map(|l| l.split_once(": "))
            .map(|(k, v)| (k.to_ascii_lowercase(), v.to_owned()))
            .collect();
        let length: usize = headers["content-length"].parse().expect("a length");
        let mut body = read[head_end..].to_vec();
        while body.len() < length {
            let n = stream.read(&mut chunk).await.expect("the body reads");
            assert!(n > 0, "the body ended early");
            body.extend_from_slice(&chunk[..n]);
        }
        let answer = format!(
            "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            reply.len()
        );
        stream
            .write_all(answer.as_bytes())
            .await
            .expect("the reply writes");
        stream.write_all(&reply).await.expect("the reply writes");
        stream.shutdown().await.ok();
        Seen {
            path,
            headers,
            body,
        }
    });
    let url = Url::parse(&format!("http://{addr}")).expect("a local address is a URL");
    (url, task)
}

#[tokio::test]
async fn send_signs_with_the_device_key() {
    let key = SecretKey::generate();
    let receipt = Receipt {
        reference: "abcdefgh23456712".into(),
    };
    let reply = rmp_serde::to_vec_named(&receipt).expect("a receipt encodes");
    let (base, server) = stand_in("200 OK", reply).await;
    let got = send(&base, b"zip bytes".to_vec(), &key)
        .await
        .expect("the stand-in accepts it");
    assert_eq!(got, receipt);

    let seen = server.await.expect("the stand-in ran");
    assert_eq!(seen.path, PATH);
    assert_eq!(seen.body, b"zip bytes");
    assert_eq!(seen.headers["content-type"], "application/zip");
    let sender: PublicKey = seen.headers[KEY_HEADER].parse().expect("a key");
    assert_eq!(sender, key.public());
    let at_ms: u64 = seen.headers[TIME_HEADER].parse().expect("a time");
    let bytes = hex::decode(&seen.headers[SIGNATURE_HEADER]).expect("hex");
    let bytes: [u8; Signature::LENGTH] = bytes.try_into().expect("64 bytes");
    sender
        .verify(
            &signed_message(&seen.body, at_ms),
            &Signature::from_bytes(&bytes),
        )
        .expect("the signature checks out");
}

#[tokio::test]
async fn send_says_what_the_server_refused() {
    let error = ApiError::bad_request("This computer's clock is off by 12 minutes");
    let reply = rmp_serde::to_vec_named(&error).expect("an error encodes");
    let (base, _server) = stand_in("400 Bad Request", reply).await;
    let got = send(&base, b"zip".to_vec(), &SecretKey::generate()).await;
    match got {
        Err(SendError::Refused(text)) => {
            assert_eq!(text, "This computer's clock is off by 12 minutes")
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_reply_that_is_not_beeps_says_reports_are_not_available() {
    let (base, _server) =
        stand_in_typed("404 Not Found", "text/plain", b"404 Not Found".to_vec()).await;
    let got = send(&base, b"zip".to_vec(), &SecretKey::generate()).await;
    let error = got.expect_err("a plain 404 is no receipt");
    assert!(matches!(error, SendError::Unavailable), "{error:?}");
    assert_eq!(error.to_string(), "Problem reports are not available yet.");
}

#[tokio::test]
async fn a_success_that_is_not_a_receipt_says_reports_are_not_available() {
    let (base, _server) = stand_in_typed("200 OK", "text/html", b"<html></html>".to_vec()).await;
    let got = send(&base, b"zip".to_vec(), &SecretKey::generate()).await;
    let error = got.expect_err("a page is no receipt");
    assert_eq!(error.to_string(), "Problem reports are not available yet.");
}

#[tokio::test(start_paused = true)]
async fn a_server_that_never_answers_gives_up_after_the_limit() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a free local port binds");
    let addr = listener
        .local_addr()
        .expect("a bound listener has an address");
    let _server = tokio::spawn(async move {
        let (_stream, _) = listener.accept().await.expect("send connects");
        pending::<()>().await;
    });
    let base = Url::parse(&format!("http://{addr}")).expect("a local address is a URL");
    let start = Instant::now();
    let got = send(&base, b"zip".to_vec(), &SecretKey::generate()).await;
    assert!(start.elapsed() >= SEND_LIMIT, "{:?}", start.elapsed());
    match got {
        Err(SendError::Net(e)) => assert!(e.is_timeout(), "{e}"),
        other => panic!("expected a timeout, got {other:?}"),
    }
}

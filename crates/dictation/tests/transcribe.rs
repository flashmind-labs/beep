//! A clip sent through flashmind's providers to a local fake of OpenAI and Mistral.

use beep_dictation::{DictationError, Provider, RATE, transcribe, wrap_pcm16};
use secrecy::SecretString;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

/// What the fake saw: the request line, the headers and the body.
struct Seen {
    line: String,
    headers: String,
    body: Vec<u8>,
}

/// Serves one reply and returns the address and what it was asked.
async fn fake(status: &'static str, body: &'static str) -> (String, JoinHandle<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let seen = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("a request");
        let mut raw = Vec::new();
        let mut chunk = [0u8; 8192];
        let (head_end, length) = loop {
            let n = socket.read(&mut chunk).await.expect("read");
            assert!(n > 0, "the request ended early");
            raw.extend_from_slice(&chunk[..n]);
            if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&raw[..at]).to_lowercase();
                let length = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .expect("a content length");
                break (at + 4, length);
            }
        };
        while raw.len() < head_end + length {
            let n = socket.read(&mut chunk).await.expect("read");
            assert!(n > 0, "the body ended early");
            raw.extend_from_slice(&chunk[..n]);
        }
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(reply.as_bytes()).await.expect("reply");
        let head = String::from_utf8_lossy(&raw[..head_end]).into_owned();
        Seen {
            line: head.lines().next().unwrap_or_default().to_owned(),
            headers: head.to_lowercase(),
            body: raw[head_end..].to_vec(),
        }
    });
    (base, seen)
}

fn clip() -> Vec<u8> {
    wrap_pcm16(&[0u8; 3200], RATE)
}

fn has(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[tokio::test]
async fn openai_takes_the_clip_and_returns_text() {
    let (base, seen) = fake("200 OK", r#"{"text":" hello there "}"#).await;
    let key = SecretString::from("sk-test");
    let text = transcribe(
        Provider::OpenAi,
        Some(&base),
        &key,
        "gpt-4o-mini-transcribe",
        clip(),
    )
    .await
    .expect("a transcript");
    assert_eq!(text, "hello there");
    let seen = seen.await.expect("the fake ran");
    assert_eq!(seen.line, "POST /v1/audio/transcriptions HTTP/1.1");
    assert!(seen.headers.contains("authorization: bearer sk-test"));
    assert!(seen.headers.contains("multipart/form-data"));
    assert!(has(&seen.body, "gpt-4o-mini-transcribe"));
    assert!(has(&seen.body, "RIFF"));
}

#[tokio::test]
async fn mistral_takes_the_clip_and_returns_text() {
    let (base, seen) = fake("200 OK", r#"{"text":"bonjour"}"#).await;
    let key = SecretString::from("mk-test");
    let text = transcribe(
        Provider::Mistral,
        Some(&base),
        &key,
        "voxtral-mini-latest",
        clip(),
    )
    .await
    .expect("a transcript");
    assert_eq!(text, "bonjour");
    let seen = seen.await.expect("the fake ran");
    assert_eq!(seen.line, "POST /v1/audio/transcriptions HTTP/1.1");
    assert!(seen.headers.contains("authorization: bearer mk-test"));
    assert!(has(&seen.body, "voxtral-mini-latest"));
}

#[tokio::test]
async fn a_refused_key_is_a_message_without_the_providers_body() {
    let (base, _seen) = fake(
        "401 Unauthorized",
        r#"{"error":{"message":"leaky detail"}}"#,
    )
    .await;
    let key = SecretString::from("sk-bad");
    let error = transcribe(Provider::OpenAi, Some(&base), &key, "m", clip())
        .await
        .expect_err("refused");
    assert_eq!(error, DictationError::Refused("OpenAI"));
    assert!(!error.to_string().contains("leaky"));
}

#[tokio::test]
async fn an_unknown_model_and_a_server_error_read_apart() {
    let (base, _seen) = fake("404 Not Found", "{}").await;
    let key = SecretString::from("mk");
    let missing = transcribe(Provider::Mistral, Some(&base), &key, "nope", clip())
        .await
        .expect_err("unknown model");
    assert_eq!(missing, DictationError::BadModel("Mistral"));
    let (base, _seen) = fake("500 Internal Server Error", "{}").await;
    let broken = transcribe(Provider::Mistral, Some(&base), &key, "m", clip())
        .await
        .expect_err("server error");
    assert_eq!(broken, DictationError::Provider("Mistral", 500));
}

#[tokio::test]
async fn a_closed_port_is_a_network_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    drop(listener);
    let key = SecretString::from("sk");
    let error = transcribe(Provider::OpenAi, Some(&base), &key, "m", clip())
        .await
        .expect_err("nothing listens");
    assert_eq!(error, DictationError::Network("OpenAI"));
}

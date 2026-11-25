use super::*;
use crate::chat::local_dictation::{
    DownloadFailure, LocalDictation, LocalModelId, ModelPaths, VerifiedDownload, download_verified,
    installed_model_path, pcm16_to_f32, split_stream_words,
};
use futures::SinkExt as _;
use hyper::{Request, body::Incoming as IncomingBody, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::{
    convert::Infallible,
    net::Ipv4Addr,
    sync::atomic::{AtomicBool, Ordering},
};
use tempfile::tempdir;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
};
use yawc::{Frame, UpgradeFut, WebSocket};

fn urls_for(base: &Url) -> Urls {
    Urls {
        elevenlabs: base.clone(),
        mistral: base.clone(),
    }
}

#[test]
fn resolves_live_models_local_preference_and_legacy_openrouter_fallback() {
    let (local_auto, _) = resolve_model(
        None,
        |id| (id == LocalModelId::Nemotron35Asr).then(|| PathBuf::from("/tmp/nemotron.gguf")),
        |provider| (provider == ApiKey::ElevenLabs).then(|| SecretString::from("el-key")),
    )
    .expect("automatic prefers an installed local model");
    assert_eq!(
        local_auto,
        Target::Local {
            model: LocalModelId::Nemotron35Asr,
        }
    );

    let (legacy, _) = resolve_model(
        Some("openrouter:openai/whisper-large-v3-turbo"),
        |_| None,
        |provider| (provider == ApiKey::Mistral).then(|| SecretString::from("ms-key")),
    )
    .expect("legacy OpenRouter setting falls back to Automatic live model");
    assert_eq!(
        legacy,
        Target::Mistral {
            model: "voxtral-mini-transcribe-realtime-3-0".into(),
        }
    );

    let missing_local = resolve_model(Some("local:parakeet-unified"), |_| None, |_| None);
    assert!(
        missing_local
            .err()
            .is_some_and(|e| e.to_string().contains("Download Parakeet Unified")),
        "missing local model explains how to download it"
    );

    let (eleven, _) = resolve_model(
        Some("elevenlabs:scribe_v3_live"),
        |_| None,
        |provider| (provider == ApiKey::ElevenLabs).then(|| SecretString::from("el-key")),
    )
    .expect("uses explicit ElevenLabs model");
    assert_eq!(
        eleven,
        Target::ElevenLabs {
            model: "scribe_v3_live".into(),
        }
    );

    let missing_cloud = resolve_model(Some("elevenlabs:scribe_v3_live"), |_| None, |_| None);
    assert!(
        missing_cloud.is_err(),
        "refuses cloud model when its provider key is missing"
    );
}

#[test]
fn stream_word_splitting_and_pcm_conversion_preserve_boundaries() {
    assert_eq!(
        split_stream_words("Book a ", "quiet table"),
        ("Book a".into(), "quiet table".into())
    );
    assert_eq!(
        split_stream_words("Book a un", "believable table"),
        ("Book a".into(), "unbelievable table".into())
    );
    assert_eq!(
        split_stream_words("un", "believable"),
        (String::new(), "unbelievable".into())
    );
    assert_eq!(
        split_stream_words("Finished sentence.", ""),
        ("Finished sentence.".into(), String::new())
    );

    let mut samples = Vec::new();
    pcm16_to_f32(&[0, 0x40, 0, 0x80], &mut samples);
    assert_eq!(samples, vec![0.5, -1.0]);
}

#[tokio::test]
async fn downloads_verifies_sha256_rejects_bad_digest_and_cancels_cleanly() {
    let payload = Bytes::from(vec![0x5au8; 64 * 1024]);
    let expected_len = payload.len() as u64;
    let expected_sha = hex::encode(Sha256::digest(&payload));

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = payload.clone();
    let server = tokio::spawn(async move {
        for _ in 0..3 {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let data = body.clone();
            tokio::spawn(async move {
                let mut req_buf = [0u8; 1024];
                let _ = stream.read(&mut req_buf).await;
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    data.len()
                );
                let _ = stream.write_all(header.as_bytes()).await;
                let _ = stream.write_all(&data).await;
                let _ = stream.shutdown().await;
            });
        }
    });

    let dir = tempdir().unwrap();
    let url = format!("http://{addr}/model.gguf");
    let client = reqwest::Client::new();
    let cancel = AtomicBool::new(false);
    let mut seen_progress = Vec::new();

    download_verified(
        &client,
        VerifiedDownload {
            url: &url,
            expected_bytes: expected_len,
            expected_sha256: &expected_sha,
            dest_dir: dir.path(),
            filename: "good.gguf",
        },
        &cancel,
        |done, total| seen_progress.push((done, total)),
    )
    .await
    .expect("matching size and digest succeeds");

    assert!(dir.path().join("good.gguf").is_file());
    assert!(!dir.path().join("good.gguf.part").exists());
    assert_eq!(seen_progress.last(), Some(&(expected_len, expected_len)));

    let corrupt = download_verified(
        &client,
        VerifiedDownload {
            url: &url,
            expected_bytes: expected_len,
            expected_sha256: "0000000000000000000000000000000000000000000000000000000000000000",
            dest_dir: dir.path(),
            filename: "bad.gguf",
        },
        &cancel,
        |_, _| {},
    )
    .await;
    assert!(matches!(corrupt, Err(DownloadFailure::Failed(_))));
    assert!(!dir.path().join("bad.gguf").exists());
    assert!(!dir.path().join("bad.gguf.part").exists());

    let cancel_mid = AtomicBool::new(false);
    let cancelled = download_verified(
        &client,
        VerifiedDownload {
            url: &url,
            expected_bytes: expected_len,
            expected_sha256: &expected_sha,
            dest_dir: dir.path(),
            filename: "cancelled.gguf",
        },
        &cancel_mid,
        |_, _| cancel_mid.store(true, Ordering::Relaxed),
    )
    .await;
    assert_eq!(cancelled, Err(DownloadFailure::Cancelled));
    assert!(!dir.path().join("cancelled.gguf").exists());
    assert!(!dir.path().join("cancelled.gguf.part").exists());

    server.abort();
}

#[test]
fn missing_or_wrong_size_files_never_count_as_installed() {
    let own = tempdir().unwrap();
    let hex = tempdir().unwrap();
    std::fs::write(
        hex.path().join(LocalModelId::ParakeetUnified.filename()),
        b"truncated",
    )
    .unwrap();
    let paths = ModelPaths {
        own_dir: own.path().to_path_buf(),
        hex_dir: Some(hex.path().to_path_buf()),
    };
    assert_eq!(
        installed_model_path(&paths, LocalModelId::ParakeetUnified),
        None
    );
}

#[tokio::test]
async fn elevenlabs_realtime_streams_partial_and_committed_transcripts() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let service = service_fn(|mut req: Request<IncomingBody>| async move {
            assert_eq!(req.uri().path(), "/v1/speech-to-text/realtime");
            assert_eq!(
                req.headers()
                    .get("xi-api-key")
                    .and_then(|v| v.to_str().ok()),
                Some("el-secret")
            );
            let (response, upgrade): (_, UpgradeFut) = WebSocket::upgrade(&mut req).unwrap();
            tokio::spawn(async move {
                let mut ws = upgrade.await.unwrap();
                ws.send(Frame::text(
                    r#"{"message_type":"session_started","session_id":"s1","config":{}}"#,
                ))
                .await
                .unwrap();

                let first = ws.next_frame().await.unwrap();
                let body: Value = serde_json::from_str(first.as_str()).unwrap();
                assert_eq!(body["message_type"], "input_audio_chunk");
                assert_eq!(body["commit"], false);

                ws.send(Frame::text(
                    r#"{"message_type":"partial_transcript","text":"Book a quiet"}"#,
                ))
                .await
                .unwrap();
                ws.send(Frame::text(
                    r#"{"message_type":"committed_transcript","text":"Book a quiet table"}"#,
                ))
                .await
                .unwrap();

                let stop = ws.next_frame().await.unwrap();
                let stop_body: Value = serde_json::from_str(stop.as_str()).unwrap();
                assert_eq!(stop_body["commit"], true);
                ws.send(Frame::text(
                    r#"{"message_type":"insufficient_audio_activity","error":"none"}"#,
                ))
                .await
                .unwrap();
            });
            Ok::<_, Infallible>(response)
        });
        let _ = http1::Builder::new()
            .serve_connection(TokioIo::new(stream), service)
            .with_upgrades()
            .await;
    });

    let base = Url::parse(&format!("http://{addr}/")).unwrap();
    let (tx, rx) = mpsc::channel(8);
    tx.send(AudioCommand::Append(Bytes::from_static(&[1, 0, 2, 0])))
        .await
        .unwrap();
    tx.send(AudioCommand::Stop).await.unwrap();

    let mut events = Vec::new();
    run_dictation(
        Target::ElevenLabs {
            model: "scribe_v3_live".into(),
        },
        Credential::ApiKey(SecretString::from("el-secret")),
        LocalDictation::default(),
        rx,
        |event| events.push(event),
        urls_for(&base),
    )
    .await;

    server.await.unwrap();
    assert!(events.contains(&DictationEvent::Ready));
    assert!(events.contains(&DictationEvent::Transcript {
        committed: String::new(),
        partial: "Book a quiet".into(),
    }));
    assert!(events.contains(&DictationEvent::Transcript {
        committed: "Book a quiet table".into(),
        partial: String::new(),
    }));
    assert_eq!(
        events.last(),
        Some(&DictationEvent::Done {
            nothing_heard: false
        })
    );
}

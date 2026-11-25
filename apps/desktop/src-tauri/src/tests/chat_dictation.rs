use super::*;
use futures::SinkExt as _;
use hyper::{Request, body::Incoming as IncomingBody, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use std::{convert::Infallible, net::Ipv4Addr};
use tokio::net::TcpListener;
use yawc::{Frame, UpgradeFut, WebSocket};

fn urls_for(base: &Url) -> Urls {
    Urls {
        elevenlabs: base.clone(),
        mistral: base.clone(),
    }
}

#[test]
fn resolves_configured_and_default_dictation_models() {
    let (target, _) = resolve_model(None, |provider| {
        (provider == ApiKey::OpenRouter).then(|| SecretString::from("or-key"))
    })
    .expect("falls back to OpenRouter when it is the saved key");
    assert_eq!(
        target,
        Target::OpenRouter {
            model: "openai/whisper-large-v3-turbo".into(),
        }
    );

    let (eleven, _) = resolve_model(Some("elevenlabs:scribe_v3_live"), |provider| {
        (provider == ApiKey::ElevenLabs).then(|| SecretString::from("el-key"))
    })
    .expect("uses explicit ElevenLabs model");
    assert_eq!(
        eleven,
        Target::ElevenLabs {
            model: "scribe_v3_live".into(),
        }
    );

    let missing = resolve_model(Some("elevenlabs:scribe_v3_live"), |_| None);
    assert!(
        missing.is_err(),
        "refuses model when its provider key is missing"
    );
}

#[test]
fn wav_encoding_and_voice_detection_match_pcm16_mono() {
    let silence = vec![0u8; 640];
    assert!(!pcm_is_voiced(&silence));

    let loud: Vec<u8> = (0..320)
        .flat_map(|i| if i % 2 == 0 { 2400i16 } else { -2400i16 }.to_le_bytes())
        .collect();
    assert!(pcm_is_voiced(&loud));

    let wav = encode_wav_pcm16_mono(&loud, 16_000);
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[36..40], b"data");
    assert_eq!(wav.len(), 44 + loud.len());
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
        SecretString::from("el-secret"),
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

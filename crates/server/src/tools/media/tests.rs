//! The media tools against recorded answers served from a local socket.

use super::{
    call::{Call, ImageSource},
    job::{JobError, first_file},
    produce::{self, Endpoints, Job},
    provider::{Keys, MediaKind, MediaProvider},
    tool::Media,
    tools,
};
use crate::{
    protocol::ApiProvider,
    store::{ApiKey, TestDb, test_chat, test_turn},
    user::{User, UserKey},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use flashmind::types::{CompletionStream, tool::ToolContext};
use reqwest::Url;
use secrecy::SecretString;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const OPENAI_IMAGES: &str = include_str!("fixtures/openai_images.json");
const VENICE_QUEUE: &str = include_str!("fixtures/venice_video_queue.json");
const MESHY_CREATED: &str = include_str!("fixtures/meshy_created.json");
const MESHY_PREVIEW: &str = include_str!("fixtures/meshy_preview_done.json");
const MESHY_REFINED: &str = include_str!("fixtures/meshy_refine_done.json");
/// The 1x1 PNG the fixtures carry.
const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAA=";
const KEY: &str = "sk-test-not-a-real-key";

/// One recorded answer.
struct Reply {
    content_type: &'static str,
    body: Vec<u8>,
}

fn json_reply(body: &str) -> Reply {
    Reply {
        content_type: "application/json",
        body: body.as_bytes().to_vec(),
    }
}

/// One request the fake service got.
#[derive(Debug)]
struct Seen {
    /// Such as `POST /v1/images/generations`.
    line: String,
    authorization: Option<String>,
    body: Value,
}

/// Serves `replies` in order, one per connection, and gives back what was asked.
async fn serve(replies: Vec<Reply>) -> (Url, JoinHandle<Vec<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let server = tokio::spawn(async move {
        let mut seen = Vec::new();
        for reply in replies {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut data = Vec::new();
            let head_end = loop {
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).await.unwrap();
                assert!(n > 0, "the client hung up mid-request");
                data.extend_from_slice(&buf[..n]);
                if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                    break at + 4;
                }
            };
            let head = String::from_utf8_lossy(&data[..head_end]).into_owned();
            let header = |name: &str| {
                head.lines().find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case(name)
                        .then(|| value.trim().to_owned())
                })
            };
            let length: usize = header("content-length").map_or(0, |v| v.parse().unwrap());
            while data.len() < head_end + length {
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).await.unwrap();
                assert!(n > 0, "the client hung up mid-body");
                data.extend_from_slice(&buf[..n]);
            }
            let body = &data[head_end..head_end + length];
            let line = head.lines().next().unwrap_or_default();
            let line = line.rsplit_once(' ').map_or(line, |(start, _)| start);
            seen.push(Seen {
                line: line.to_owned(),
                authorization: header("authorization"),
                body: serde_json::from_slice(body).unwrap_or(Value::Null),
            });
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                reply.content_type,
                reply.body.len()
            );
            stream.write_all(head.as_bytes()).await.unwrap();
            stream.write_all(&reply.body).await.unwrap();
            stream.shutdown().await.unwrap();
        }
        seen
    });
    (root, server)
}

/// Every service at `root`, under the paths each really has.
fn endpoints(root: &Url) -> Endpoints {
    Endpoints {
        openai: root.join("v1").unwrap(),
        venice: root.join("api/v1").unwrap(),
        meshy: root.clone(),
    }
}

fn key() -> SecretString {
    SecretString::from(KEY)
}

fn keys(providers: &[ApiProvider]) -> Keys {
    Keys::new(
        providers
            .iter()
            .map(|&provider| ApiKey {
                provider,
                secret: key(),
            })
            .collect(),
    )
}

fn call(kind: MediaKind, provider: MediaProvider, args: Value) -> Call {
    Call::parse(kind, &[provider], &args).unwrap()
}

fn png() -> Vec<u8> {
    STANDARD.decode(PNG_B64).unwrap()
}

async fn make(job: Job<'_>) -> (Result<super::job::Produced, JobError>, Vec<String>) {
    let cancel = CancellationToken::new();
    let mut lines = Vec::new();
    let mut progress = |line: &str| lines.push(line.to_owned());
    let made = produce::media(job, &cancel, &mut progress).await;
    (made, lines)
}

fn bearer(seen: &Seen) -> &str {
    seen.authorization.as_deref().unwrap_or_default()
}

#[tokio::test]
async fn openai_makes_an_image_without_response_format() {
    let (root, server) = serve(vec![json_reply(OPENAI_IMAGES)]).await;
    let call = call(
        MediaKind::Image,
        MediaProvider::OpenAi,
        json!({ "prompt": "a red fox in the snow", "aspect_ratio": "16:9" }),
    );
    let (endpoints, key) = (endpoints(&root), key());
    let job = Job {
        call: &call,
        image: None,
        key: &key,
        endpoints: &endpoints,
    };
    let (made, _) = make(job).await;
    let made = made.unwrap();
    assert_eq!(made.media_type, "image/png");
    assert_eq!(made.data, png());
    let seen = server.await.unwrap();
    assert_eq!(seen[0].line, "POST /v1/images/generations");
    assert_eq!(bearer(&seen[0]), format!("Bearer {KEY}"));
    let body = &seen[0].body;
    assert_eq!(body["model"], "gpt-image-2");
    assert_eq!(body["prompt"], "a red fox in the snow");
    assert_eq!(body["size"], "1536x1024");
    assert_eq!(body["n"], 1);
    assert!(body.get("response_format").is_none(), "{body}");
}

#[tokio::test]
async fn openai_edits_an_image_it_is_given() {
    let (root, server) = serve(vec![json_reply(OPENAI_IMAGES)]).await;
    let call = call(
        MediaKind::Image,
        MediaProvider::OpenAi,
        json!({ "prompt": "make it night", "image": "photo.png", "aspect_ratio": "1:1" }),
    );
    let (endpoints, key) = (endpoints(&root), key());
    let image = format!("data:image/png;base64,{PNG_B64}");
    let job = Job {
        call: &call,
        image: Some(image.clone()),
        key: &key,
        endpoints: &endpoints,
    };
    let (made, _) = make(job).await;
    assert_eq!(made.unwrap().data, png());
    let seen = server.await.unwrap();
    assert_eq!(seen[0].line, "POST /v1/images/edits");
    let body = &seen[0].body;
    assert_eq!(body["images"][0]["image_url"], image.as_str());
    assert_eq!(body["prompt"], "make it night");
    // OpenAI's edits refuse an aspect ratio, so it is left out.
    assert!(body.get("aspect_ratio").is_none(), "{body}");
}

#[tokio::test]
async fn venice_queues_a_video_and_retrieves_it() {
    let video = b"\0\0\0\x18ftypmp42fake-video".to_vec();
    let (root, server) = serve(vec![
        json_reply(VENICE_QUEUE),
        Reply {
            content_type: "video/mp4",
            body: video.clone(),
        },
    ])
    .await;
    let call = call(
        MediaKind::Video,
        MediaProvider::Venice,
        json!({ "prompt": "waves at dusk", "aspect_ratio": "16:9" }),
    );
    let (endpoints, key) = (endpoints(&root), key());
    let job = Job {
        call: &call,
        image: None,
        key: &key,
        endpoints: &endpoints,
    };
    let (made, lines) = make(job).await;
    let made = made.unwrap();
    assert_eq!(made.media_type, "video/mp4");
    assert_eq!(made.data, video);
    assert!(
        lines.iter().any(|line| line.contains("4b1f6a2e")),
        "{lines:?}"
    );
    let seen = server.await.unwrap();
    assert_eq!(seen[0].line, "POST /api/v1/video/queue");
    assert_eq!(seen[0].body["model"], "wan-2-7-text-to-video");
    assert_eq!(seen[0].body["duration"], "5s");
    assert_eq!(seen[1].line, "POST /api/v1/video/retrieve");
    assert_eq!(
        seen[1].body["queue_id"],
        "4b1f6a2e-0c3d-4e5f-8a9b-0c1d2e3f4a5b"
    );
}

#[tokio::test]
async fn meshy_shapes_then_textures_a_model_from_a_prompt() {
    let (root, server) = serve(vec![
        json_reply(MESHY_CREATED),
        json_reply(MESHY_PREVIEW),
        json_reply(MESHY_CREATED),
        json_reply(MESHY_REFINED),
    ])
    .await;
    let call = call(
        MediaKind::Model3d,
        MediaProvider::Meshy,
        json!({ "prompt": "a low-poly wooden chair" }),
    );
    let (endpoints, key) = (endpoints(&root), key());
    let job = Job {
        call: &call,
        image: None,
        key: &key,
        endpoints: &endpoints,
    };
    let cancel = CancellationToken::new();
    let mut lines = Vec::new();
    let mut progress = |line: &str| lines.push(line.to_owned());
    let url = produce::model_3d(job, &cancel, &mut progress)
        .await
        .unwrap();
    assert_eq!(
        url.as_str(),
        "https://assets.meshy.example/018a210d/refined.glb"
    );
    assert_eq!(
        lines,
        [
            "Meshy is shaping the model",
            "Meshy is painting the textures"
        ]
    );
    let seen = server.await.unwrap();
    assert_eq!(seen[0].line, "POST /openapi/v2/text-to-3d");
    assert_eq!(bearer(&seen[0]), format!("Bearer {KEY}"));
    assert_eq!(seen[0].body["mode"], "preview");
    assert_eq!(seen[0].body["prompt"], "a low-poly wooden chair");
    assert_eq!(seen[0].body["ai_model"], "latest");
    assert_eq!(
        seen[1].line,
        "GET /openapi/v2/text-to-3d/018a210d-8ba4-705c-b111-1f1776f7f578"
    );
    assert_eq!(seen[2].body["mode"], "refine");
    assert_eq!(
        seen[2].body["preview_task_id"],
        "018a210d-8ba4-705c-b111-1f1776f7f578"
    );
}

#[tokio::test(start_paused = true)]
async fn a_job_that_never_finishes_times_out() {
    let cancel = CancellationToken::new();
    let stream: CompletionStream = Box::pin(futures::stream::pending());
    let made = first_file(stream, Duration::from_secs(60), &cancel, &mut |_: &str| {}).await;
    assert!(matches!(made, Err(JobError::TimedOut(_))), "{made:?}");
    let job = std::future::pending::<anyhow::Result<()>>();
    let done = super::job::bounded(job, Duration::from_secs(60), &cancel).await;
    assert!(matches!(done, Err(JobError::TimedOut(_))));
}

#[tokio::test]
async fn a_stopped_turn_stops_the_job() {
    let cancel = CancellationToken::new();
    cancel.cancel();
    let stream: CompletionStream = Box::pin(futures::stream::pending());
    let made = first_file(stream, Duration::from_secs(60), &cancel, &mut |_: &str| {}).await;
    assert!(matches!(made, Err(JobError::Cancelled)));
}

#[test]
fn calls_take_only_a_service_the_user_has_a_key_for() {
    let available = [MediaProvider::Venice];
    let args = json!({ "prompt": "a fox", "provider": "openai" });
    let refused = Call::parse(MediaKind::Image, &available, &args).unwrap_err();
    assert!(refused.contains("venice"), "{refused}");
    // Without one named, the first the user has.
    let picked = Call::parse(MediaKind::Image, &available, &json!({ "prompt": "a fox" })).unwrap();
    assert_eq!(picked.provider, MediaProvider::Venice);
    assert_eq!(picked.model, "qwen-image-2");
    let named = Call::parse(
        MediaKind::Image,
        &available,
        &json!({ "prompt": "a fox", "provider": "Venice", "model": "flux-2-pro" }),
    )
    .unwrap();
    assert_eq!(named.model, "flux-2-pro");
    assert!(
        Call::parse(
            MediaKind::Image,
            &available,
            &json!({ "provider": "venice" })
        )
        .is_err()
    );
    let meshy = [MediaProvider::Meshy];
    assert!(Call::parse(MediaKind::Model3d, &meshy, &json!({})).is_err());
    let image = Call::parse(MediaKind::Model3d, &meshy, &json!({ "image": "chair.png" })).unwrap();
    assert_eq!(image.image, Some(ImageSource::Path("chair.png".into())));
    let video = [MediaProvider::Venice];
    for duration in [json!(0), json!(31), json!(2.5), json!("5")] {
        let args = json!({ "prompt": "waves", "duration": duration });
        assert!(
            Call::parse(MediaKind::Video, &video, &args).is_err(),
            "{args}"
        );
    }
    let args = json!({ "prompt": "a fox", "image": "file:///etc/passwd" });
    assert!(Call::parse(MediaKind::Image, &available, &args).is_err());
}

#[tokio::test]
async fn a_made_image_is_kept_with_the_chat() {
    let db = TestDb::new().await.unwrap();
    let store = db.store().await.unwrap();
    let user = User::for_test(store.clone(), "alice");
    let chat = test_chat("media");
    test_turn(
        &store,
        &UserKey::for_test("alice"),
        chat,
        "draw a fox",
        1000,
    )
    .await
    .unwrap();
    let (root, server) = serve(vec![json_reply(OPENAI_IMAGES)]).await;
    let media = |keys| Media {
        user: user.clone(),
        chat,
        workspace: None,
        keys,
        endpoints: endpoints(&root),
    };
    assert!(tools(media(keys(&[]))).is_empty());
    let offered = tools(media(keys(&[ApiProvider::OpenAi, ApiProvider::Meshy])));
    let names: Vec<&str> = offered.iter().map(|tool| tool.name()).collect();
    assert_eq!(names, ["image_gen", "3d_gen"]);
    let cancel = CancellationToken::new();
    let args = json!({ "prompt": "a red fox", "provider": "openai" });
    let result = offered[0]
        .execute(ToolContext::new("call-1", args, None, &cancel))
        .await
        .unwrap();
    assert!(result.is_success(), "{}", result.output());
    let output: Value = serde_json::from_str(&result.output()).unwrap();
    let file = &output["images"][0]["file"];
    assert_eq!(file["mime"], "image/png");
    assert_eq!(output["provider"], "OpenAI");
    assert_eq!(output["model"], "gpt-image-2");
    // Without an environment online it is not in the home folder, and says so.
    assert!(output["note"].as_str().unwrap().contains("home folder"));
    let id = file["id"].as_str().unwrap();
    let kept = user.file_part(id, 0, 100).await.unwrap().unwrap();
    assert_eq!(kept.data, png());
    server.await.unwrap();
}

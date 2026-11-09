use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

fn args() -> Value {
    json!({
        "state": "Customer was charged twice and wants a refund.",
        "questions": {
            "needs_reply": {"type": "noul", "instructions": "Does this need a reply?"},
            "team": {
                "type": "choice",
                "instructions": "Which team should handle this?",
                "criteria": {"billing": "Charges and refunds", "technical": "Product defects"}
            }
        }
    })
}

async fn read_request(stream: &mut TcpStream) -> String {
    let mut data = Vec::new();

    loop {
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).await.unwrap();
        assert!(n > 0);
        data.extend_from_slice(&buf[..n]);

        if data.ends_with(b"}") {
            break;
        }
    }

    String::from_utf8(data).unwrap()
}

/// Answers every connection with the same response, so client retries see it too.
async fn mock(
    status: &'static str,
    body: &'static str,
) -> (DecisionTool, UnboundedReceiver<String>) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nRetry-After: 0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/api/v1/systemone", listener.local_addr().unwrap());
    let tool = DecisionTool::new("private-test-key".into())
        .unwrap()
        .with_system_one_url(url);
    let (tx, rx) = unbounded_channel();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let request = read_request(&mut stream).await;
            stream.write_all(response.as_bytes()).await.unwrap();
            let _ = tx.send(request);
        }
    });
    (tool, rx)
}

#[tokio::test]
async fn returns_answers_from_jev() {
    let body = r#"{"id":"r1","model":"jev-1.13","answers":{"needs_reply":{"p":0.93},"team":{"choice":"billing"}},"usage":{"input_tokens":12}}"#;
    let (tool, mut requests) = mock("200 OK", body).await;
    let output = tool.decide(&args()).await.unwrap();
    let request = requests.recv().await.unwrap();
    assert!(request.contains("Bearer private-test-key"));
    assert!(request.contains("\"model\":\"jev-latest\""));
    assert!(request.contains("\"type\":\"choice\""));
    assert_eq!(output["answers"]["team"]["choice"], "billing");
    assert_eq!(output["answers"]["needs_reply"]["p"], 0.93);
    assert!(output.get("usage").is_none());
}

#[tokio::test]
async fn chooses_for_each_question_in_one_request() {
    let options = [("plan", "Trips"), ("work", "Jobs")];
    let body = r#"{"model":"jev-1.13","answers":{"1":{"choice":"plan"},"2":{"choice":"travel"}}}"#;
    let (tool, mut requests) = mock("200 OK", body).await;
    let questions = [("1", "Memory 1?"), ("2", "Memory 2?"), ("3", "Memory 3?")];
    let filed = tool
        .choose_each("1. Is in Lisbon.\n2. Works at Acme.", &questions, &options)
        .await
        .unwrap();
    assert_eq!(filed, BTreeMap::from([("1".to_owned(), "plan")]));
    let request = requests.recv().await.unwrap();
    assert!(request.contains("Memory 3?"));
    assert!(requests.try_recv().is_err(), "one request for all three");
}

#[tokio::test]
async fn redacts_upstream_errors() {
    let (tool, mut requests) = mock("429 Too Many Requests", "private-test-key").await;
    let error = tool.decide(&args()).await.unwrap_err();
    requests.recv().await.unwrap();
    assert!(error.contains("rate limited"));
    assert!(!error.contains("private-test-key"));
}

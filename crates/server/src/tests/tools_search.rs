use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
fn results_are_bounded_and_unsafe_links_removed() {
    let data = json!({"success":true,"data":{"web":[
            {"url":"javascript:alert(1)","title":"bad"},
            {"url":"https://example.org/","description":"é".repeat(2000)},
            {"url":"https://example.com/"}]}});
    let output = normalize(&data, 1).unwrap();
    assert_eq!(output["results"].as_array().unwrap().len(), 1);
    assert_eq!(
        output["results"][0]["description"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        1000
    );
    assert_eq!(
        normalize(&data, 10).unwrap()["results"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let with_images = json!({"success":true,"data":{"web":[],"images":[
            {"imageUrl":"http://example.org/a.png"},
            {"imageUrl":"https://example.org/b.png","title":"B","url":"https://example.org/"}]}});
    let images = normalize(&with_images, 3).unwrap()["images"].clone();
    assert_eq!(
        images,
        json!([{"image_url":"https://example.org/b.png","title":"B","page":"https://example.org/"}])
    );
    assert!(
        !normalize(&json!({"success":false,"error":"secret"}), 3)
            .unwrap_err()
            .contains("secret")
    );
}

#[test]
fn null_and_blank_arguments_are_left_out() {
    let args = json!({"query":"hello","limit":null,"country":" ","location":""});
    let query = parse(&args).expect("valid arguments");
    assert_eq!(
        query,
        Query {
            query: "hello",
            limit: 5,
            location: None,
            country: None,
            images: false,
        }
    );
    assert!(parse(&json!({"query":"hello","country":"JPN"})).is_err());
    assert!(parse(&json!({"query":"hello","limit":0})).is_err());
}

#[tokio::test]
async fn sends_server_credentials_and_redacts_upstream_errors() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut tool = SearchTool::new("private-test-key".into()).unwrap();
    tool.endpoint = format!("http://{}/v2/search", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
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
        let request = String::from_utf8(data).unwrap();
        assert!(request.contains("Bearer private-test-key"));
        assert!(request.contains("\"query\":\"hello\""));
        assert!(request.contains("\"sources\":[\"web\"]"));
        assert!(request.contains("\"location\":\"Shinjuku,Tokyo,Japan\""));
        assert!(request.contains("\"country\":\"JP\""));
        assert!(!request.contains("scrapeOptions"));
        stream.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 16\r\nConnection: close\r\n\r\nprivate-test-key").await.unwrap();
    });
    let error = tool
        .search(&json!({"query":"hello","location":"Shinjuku, Tokyo, Japan","country":"jp"}))
        .await
        .unwrap_err();
    assert!(error.contains("rate limited"));
    assert!(!error.contains("private-test-key"));
    server.await.unwrap();
    assert!(tool.search(&json!({"query":" "})).await.is_err());
    assert!(
        tool.search(&json!({"query":"hello","limit":11}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_blank_location_searches_without_one() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut tool = SearchTool::new("key".into()).unwrap();
    tool.endpoint = format!("http://{}/v2/search", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut data = Vec::new();
        while !data.ends_with(b"}") {
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            assert!(n > 0);
            data.extend_from_slice(&buf[..n]);
        }
        assert!(!String::from_utf8(data).unwrap().contains("location"));
        stream
            .write_all(
                b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
    });
    let error = tool
        .search(&json!({"query":"hello","location":" , "}))
        .await
        .unwrap_err();
    assert!(error.contains("rate limited"));
    server.await.unwrap();
}

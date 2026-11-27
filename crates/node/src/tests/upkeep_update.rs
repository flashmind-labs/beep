use super::*;
use reqwest::StatusCode;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[tokio::test]
async fn release_channels_fetch_separate_pointers_and_never_fall_back() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        for (path, status, body) in [
            ("/minor.txt", "200 OK", "0.8.0\n"),
            ("/latest.txt", "200 OK", "0.8.3\n"),
            ("/minor.txt", "404 Not Found", ""),
        ] {
            let (mut stream, _) = listener.accept().await.expect("connection");
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(stream.read_u8().await.expect("request"));
            }
            assert!(
                String::from_utf8_lossy(&request).starts_with(&format!("GET {path} HTTP/1.1\r\n"))
            );
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("response");
        }
    });
    let http = client().expect("client");
    assert_eq!(
        newest(&http, &base, ReleaseChannel::Minor)
            .await
            .expect("minor")
            .latest
            .to_string(),
        "0.8.0"
    );
    assert_eq!(
        newest(&http, &base, ReleaseChannel::All)
            .await
            .expect("all")
            .latest
            .to_string(),
        "0.8.3"
    );
    assert!(matches!(
        newest(&http, &base, ReleaseChannel::Minor).await,
        Err(Error::Status {
            status: StatusCode::NOT_FOUND,
            ..
        })
    ));
    server.await.expect("server");
}

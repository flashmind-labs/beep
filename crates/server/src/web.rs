//! Browser transport. Both transports dispatch through the same authenticated API.
use crate::{
    app::App,
    transport::{self, Stream},
};
use anyhow::Result;
use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full, StreamBody, combinators::UnsyncBoxBody};
use hyper::{
    body::{Frame, Incoming},
    service::service_fn,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::net::TcpListener;

type Body = UnsyncBoxBody<Bytes, Infallible>;
fn full(status: StatusCode, mime: &str, content: impl Into<Bytes>) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", mime)
        .body(Full::new(content.into()).boxed_unsync())
        .unwrap()
}
fn headers(response: &mut Response<Body>) {
    let has_csp = response.headers().contains_key("content-security-policy");
    for (key, value) in [
        ("cache-control", "no-store"),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        ),
    ] {
        if key == "content-security-policy" && has_csp {
            continue;
        }
        response.headers_mut().insert(
            http::header::HeaderName::from_static(key),
            http::HeaderValue::from_static(value),
        );
    }
}
async fn request(
    app: Arc<App>,
    web_enabled: bool,
    request: Request<Incoming>,
) -> Result<Response<Body>, Infallible> {
    let mut response = if request.uri().path().starts_with("/v1/") {
        let (parts, body) = request.into_parts();
        let (send_headers, receive_headers) = tokio::sync::oneshot::channel();
        let (data, mut receive_data) = tokio::sync::mpsc::channel(8);
        tokio::spawn(async move {
            if let Err(error) = transport::handle(
                app,
                Request::from_parts(parts, ()),
                Stream::Http {
                    body,
                    headers: Some(send_headers),
                    data,
                },
            )
            .await
            {
                tracing::debug!(%error, "browser request ended");
            }
        });
        match receive_headers.await {
            Ok(response) => {
                let (parts, ()) = response.into_parts();
                let stream = async_stream::stream! {
                    while let Some(bytes) = receive_data.recv().await {
                        yield Ok::<_, Infallible>(Frame::data(bytes));
                    }
                };
                Response::from_parts(parts, StreamBody::new(stream).boxed_unsync())
            }
            Err(_) => full(
                StatusCode::INTERNAL_SERVER_ERROR,
                "application/json",
                "{\"error\":\"internal server error\"}",
            ),
        }
    } else if web_enabled && request.method() == http::Method::GET {
        // Fixed embedded allowlist: no paths into config, credentials, or the filesystem.
        match request.uri().path() {
            "/" | "/index.html" => full(
                StatusCode::OK,
                "text/html; charset=utf-8",
                include_str!("../../../web/index.html"),
            ),
            "/ui-mockups" | "/ui-mockups.html" => {
                let mut r = full(
                    StatusCode::OK,
                    "text/html; charset=utf-8",
                    include_str!("../../../web/ui-mockups.html"),
                );
                r.headers_mut().insert(
                    http::header::HeaderName::from_static("content-security-policy"),
                    http::HeaderValue::from_static("default-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src 'self' data:; object-src 'none'"),
                );
                r
            }
            "/design" | "/design/" => full(
                StatusCode::OK,
                "text/html; charset=utf-8",
                include_str!("../../../web/design.html"),
            ),
            "/design.css" => full(
                StatusCode::OK,
                "text/css; charset=utf-8",
                include_str!("../../../web/design.css"),
            ),
            "/design.js" => full(
                StatusCode::OK,
                "text/javascript; charset=utf-8",
                include_str!("../../../web/design.js"),
            ),
            "/styles.css" => full(
                StatusCode::OK,
                "text/css; charset=utf-8",
                include_str!("../../../web/styles.css"),
            ),
            "/protocol.mjs" => full(
                StatusCode::OK,
                "text/javascript; charset=utf-8",
                include_str!("../../../web/protocol.mjs"),
            ),
            "/app.js" => full(
                StatusCode::OK,
                "text/javascript; charset=utf-8",
                include_str!("../../../web/app.js"),
            ),
            _ => full(StatusCode::NOT_FOUND, "text/plain", "Not found"),
        }
    } else {
        full(
            StatusCode::NOT_FOUND,
            "application/json",
            "{\"error\":\"not found\"}",
        )
    };
    headers(&mut response);
    Ok(response)
}
pub async fn serve(listener: TcpListener, app: Arc<App>, web_enabled: bool) -> Result<()> {
    let connections = Arc::new(tokio::sync::Semaphore::new(128));
    loop {
        let (socket, _) = listener.accept().await?;
        let Ok(permit) = connections.clone().try_acquire_owned() else {
            continue;
        };
        let app = app.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let result: Result<()> = async {
                hyper::server::conn::http1::Builder::new()
                    .timer(TokioTimer::new())
                    .header_read_timeout(Duration::from_secs(10))
                    .max_buf_size(65536)
                    .serve_connection(
                        TokioIo::new(socket),
                        service_fn(move |req| request(app.clone(), web_enabled, req)),
                    )
                    .await?;
                Ok(())
            }
            .await;
            if let Err(error) = result {
                tracing::debug!(%error, "HTTP connection ended");
            }
        });
    }
}

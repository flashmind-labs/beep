use super::*;

#[tokio::test]
async fn rejects_unsafe_urls_without_fetching() {
    for url in [
        "http://example.com/a.png",
        "https://127.0.0.1/a.png",
        "https://[::1]/a.png",
        "https://localhost/a.png",
        "https://intranet/a.png",
        "https://[fd00::1]/a.png",
        "https://169.254.169.254/latest/meta-data/",
        "file:///etc/passwd",
        "not a url",
    ] {
        assert!(check(url).await.is_err(), "{url}");
    }
}

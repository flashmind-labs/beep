use super::*;

#[tokio::test]
async fn switching_channels_does_not_reuse_the_other_channels_cached_release() {
    let releases = ToolReleases::default();
    {
        let mut cache = releases.found.lock().await;
        for (channel, version) in [
            (ReleaseChannel::All, "0.8.3"),
            (ReleaseChannel::Minor, "0.8.0"),
        ] {
            cache.insert(
                Of::Node(channel),
                Found {
                    version: Some(version.to_owned()),
                    at: Instant::now(),
                },
            );
        }
    }

    assert_eq!(
        releases.latest_node(ReleaseChannel::All).await.as_deref(),
        Some("0.8.3")
    );
    assert_eq!(
        releases.latest_node(ReleaseChannel::Minor).await.as_deref(),
        Some("0.8.0")
    );
    assert_eq!(
        releases.latest_node(ReleaseChannel::All).await.as_deref(),
        Some("0.8.3")
    );
}

use super::super::{boot, shutdown};
use super::*;
use beep_proto::node::device_video::FPS;
use tokio_util::task::AbortOnDropHandle;
use tokio::time::{Instant, sleep, sleep_until};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a native device encoder; set BEEP_VIDEO_TEST"]
async fn a_real_device_streams_native_h264() -> Result<()> {
    let id = DeviceId(std::env::var("BEEP_VIDEO_TEST").context("BEEP_VIDEO_TEST")?);
    let device = find(&id).await?;
    let started = device.state == DeviceState::Off;
    let result = async {
        if started {
            boot(&id).await?;
        }
        timeout(Duration::from_secs(180), async {
            while find(&id).await?.state != DeviceState::On {
                sleep(Duration::from_millis(500)).await;
            }
            Ok::<(), anyhow::Error>(())
        })
        .await??;
        let link = beep_proto::node::local_link();
        let (mut tx, mut rx) = link.caller;
        let (node_tx, node_rx) = link.node;
        let _serving = AbortOnDropHandle::new(tokio::spawn(serve(node_tx, node_rx)));
        tx.send(&Request::Attach {
            id: id.clone(),
            max_side: 800,
        })
        .await?;
        let first = timeout(Duration::from_secs(30), rx.recv::<Reply>()).await??;
        assert!(
            matches!(first, Some(Reply::Started { fps: FPS })),
            "{first:?}"
        );
        tx.send(&Request::Input {
            input: beep_proto::node::device::Input::Button {
                button: beep_proto::node::device::Button::Home,
            },
        })
        .await?;
        let until = Instant::now() + Duration::from_secs(3);
        let mut encoded = Vec::new();
        let mut chunks = 0;
        loop {
            tokio::select! {
                _ = sleep_until(until) => break,
                reply = rx.recv::<Reply>() => match reply? {
                    Some(Reply::Data { data, .. }) => { encoded.extend(data); chunks += 1; }
                    other => bail!("video ended: {other:?}"),
                }
            }
        }
        assert!(
            encoded.len() > 1024 && chunks > 1,
            "encoder gave too little video"
        );
        if let Some(path) = std::env::var_os("BEEP_VIDEO_TEST_OUT") {
            std::fs::write(path, &encoded)?;
        }
        eprintln!(
            "captured {} encoded bytes in {chunks} chunks",
            encoded.len()
        );
        Ok(())
    }
    .await;
    if started {
        shutdown(&id).await?;
    }
    result
}

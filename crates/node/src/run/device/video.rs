//! Native H.264 capture and ordered forwarding. Encoder bytes are never dropped:
//! losing one chunk can invalidate every following delta frame.

use super::{
    Controls, DeviceId, DeviceState, MAX_SIDE, Platform, android, find,
    screen::{Turn, VideoFeed},
};
use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    device_video::{Reply, Request, Rotation},
};
use tokio::time::{Duration, timeout};

pub use beep_proto::node::device_video::PROTOCOL;

pub(crate) async fn serve(mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    let Some(Request::Attach { id, max_side }) = rx.recv::<Request>().await? else {
        bail!("a video stream must attach first");
    };
    let live = match timeout(
        Duration::from_secs(20),
        open(&id, max_side.clamp(64, MAX_SIDE)),
    )
    .await
    .context("the video capture did not start")?
    {
        Ok(live) => live,
        Err(error) => {
            return tx
                .send(&Reply::Unsupported {
                    reason: format!("{error:#}"),
                })
                .await;
        }
    };
    let VideoFeed {
        mut packets,
        mut controls,
        work: _work,
    } = live;
    // An encoder that cannot start must not leave a blank preview indefinitely.
    let first = match timeout(Duration::from_secs(15), packets.recv()).await {
        Ok(Some(packet)) => packet,
        _ => {
            return tx
                .send(&Reply::Unsupported {
                    reason: "The video encoder did not start.".to_owned(),
                })
                .await;
        }
    };
    tx.send(&first).await?;

    let inputs = async {
        while let Some(request) = rx.recv::<Request>().await? {
            if let Request::Input { input } = request
                && let Err(error) = controls.send(input).await
            {
                log::warn!("device video input failed: {error:#}");
            }
        }

        Ok(())
    };
    let frames = async {
        while let Some(packet) = packets.recv().await {
            // A timeout drops this stream, including any partial packet. Never
            // continue a partially written encoded stream after cancellation.
            timeout(Duration::from_secs(2), tx.send(&packet))
                .await
                .context("the video viewer fell behind")??;
        }

        Ok(())
    };

    tokio::select! {
        result = inputs => result,
        result = frames => result,
    }
}

async fn open(id: &DeviceId, max_side: u32) -> Result<VideoFeed<Controls>> {
    let device = find(id).await?;
    if device.state != DeviceState::On {
        bail!("the device is not running");
    }

    match device.platform {
        Platform::Android if device.phone => android::phone_video(id, max_side)
            .map(|feed| feed.map(|controls| Controls::Phone(Box::new(controls)))),
        Platform::Android => android::video(id, max_side)
            .await
            .map(|feed| feed.map(|controls| Controls::Android(Box::new(controls)))),
        #[cfg(target_os = "macos")]
        Platform::Ios => super::ios::video(id, max_side, device.phone)
            .await
            .map(|feed| {
                feed.map(|controls| match controls {
                    Some(controls) => Controls::Ios(Box::new(controls)),
                    None => Controls::Iphone,
                })
            }),
        #[cfg(not(target_os = "macos"))]
        Platform::Ios => bail!("iOS video capture requires a Mac"),
    }
}

impl From<Turn> for Rotation {
    fn from(turn: Turn) -> Self {
        match turn {
            Turn::Upright => Self::Upright,
            Turn::Left => Self::Left,
            Turn::UpsideDown => Self::UpsideDown,
            Turn::Right => Self::Right,
        }
    }
}

#[cfg(test)]
#[path = "../../tests/run_device_video.rs"]
mod tests;

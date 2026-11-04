//! Long-lived streams to the apps: notifications, updates and suggested prompts.

use super::Reply;
use super::SEND_TIMEOUT;
use crate::{
    api::router::{Cx, Sink},
    protocol::*,
    tools::stocks::{self, StockError, Symbol},
};
use tokio::{sync::broadcast::error::RecvError, time::timeout};

/// The transport reads the opening `Hello`; a later one changes nothing.
pub(super) async fn hello(_: Cx, _: Hello) -> Reply<()> {
    Ok(())
}

pub(super) async fn health(_: Cx, _: Health) -> Reply<()> {
    Ok(())
}

/// One symbol's chart over another range, for a stock chart's range buttons.
pub(super) async fn stock_chart(_: Cx, req: StockChart) -> Reply<Chart> {
    let symbol = Symbol::parse(&req.symbol).map_err(|e| ApiError::bad_request(e.to_string()))?;
    match stocks::fetch(&[symbol], req.range).await {
        Ok(histories) => Ok(stocks::chart(req.range, &histories)),
        Err(StockError::NotFound(_) | StockError::NoPrices(_)) => Err(ApiError::NotFound {}),
        Err(e) => {
            log::warn!("stock chart failed: {e}; symbol={}", req.symbol);
            Err(ApiError::Internal {})
        }
    }
}

pub(super) async fn suggest_prompts(
    cx: Cx,
    SuggestPrompts { refresh }: SuggestPrompts,
) -> Reply<PromptSuggestions> {
    Ok(cx.app.suggest_prompts(&cx.user, refresh).await)
}

/// Pushes the user's notices until the connection closes.
pub(super) async fn stream_notifications(
    cx: Cx,
    _: StreamNotifications,
    sink: Sink<Notice>,
) -> Reply<()> {
    let mut rx = cx.app.notifications.subscribe();
    // An app from before resolved questions would only refresh the chat.
    let resolves = cx.supports(Feature::QUESTIONS);
    loop {
        tokio::select! {
            _ = cx.connection.closed() => return Ok(()),
            received = rx.recv() => match received {
                Ok(event) if event.user == cx.user.id()
                    && (resolves || event.notice.resolved_question().is_none()) => {
                    let Some(notice) = event.for_device(&cx.connection.remote_id(), &cx.device) else {
                        continue;
                    };
                    match timeout(SEND_TIMEOUT, sink.send(notice)).await {
                        Ok(sent) => sent?,
                        Err(_) => return Ok(()),
                    }
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return Ok(()),
            }
        }
    }
}

/// The caller's updates after `after`, a page at a time.
pub(super) async fn get_updates(cx: Cx, GetUpdates { after }: GetUpdates) -> Reply<UpdatePage> {
    Ok(cx.user.updates_page(after).await?)
}

/// Sends the caller's newest update id now and each time it moves.
pub(super) async fn stream_updates(cx: Cx, _: StreamUpdates, sink: Sink<i64>) -> Reply<()> {
    let mut last = cx.user.watch_updates().await?;
    loop {
        let newest = *last.borrow_and_update();
        match timeout(SEND_TIMEOUT, sink.send(&newest)).await {
            Ok(sent) => sent?,
            Err(_) => return Ok(()),
        }
        tokio::select! {
            _ = cx.connection.closed() => return Ok(()),
            changed = last.changed() => if changed.is_err() {
                return Ok(());
            },
        }
    }
}

//! What the apps read besides chats: updates, notice acks, charts and suggested prompts.

use super::Reply;
use crate::{
    api::router::Cx,
    protocol::*,
    tools::stocks::{self, StockError, Symbol},
};

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

/// The caller's updates after `after`, a page at a time.
pub(super) async fn get_updates(cx: Cx, GetUpdates { after }: GetUpdates) -> Reply<UpdatePage> {
    Ok(cx.user.updates_page(after).await?)
}

/// Notes that the caller's app took a notice off its events stream.
pub(super) async fn ack_notice(cx: Cx, AckNotice { id }: AckNotice) -> Reply<()> {
    Ok(cx.app.store.ack_notice(cx.user.id(), id, &cx.key).await?)
}

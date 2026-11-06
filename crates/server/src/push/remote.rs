//! Reaches phones not on their events stream through FCM or APNs, with notices sealed to the phone's key.

use crate::{
    protocol::{Notice, seal},
    push::apns::{Apns, Environment},
    push::fcm::Firebase,
    store::{PushService, PushToken, Store},
};
use anyhow::Result;
use async_trait::async_trait;
use iroh::EndpointId;

/// Reaches a phone that did not take a notice off its events stream.
#[async_trait]
pub trait Fallback: Send + Sync + 'static {
    /// Sends `notice` to the phone. True when a push service took it.
    async fn push(&self, phone: EndpointId, notice: &Notice) -> bool;
}

/// What a push to one token came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Sent,
    /// The app was uninstalled or the token rotated: forget it.
    Gone,
    Failed,
}

/// The push services the server has keys for.
pub struct RemotePush {
    store: Store,
    firebase: Option<Firebase>,
    apns: Option<Apns>,
}

impl RemotePush {
    pub fn new(store: Store, firebase: Option<Firebase>, apns: Option<Apns>) -> Self {
        Self {
            store,
            firebase,
            apns,
        }
    }

    /// Sends `notice` to each phone registered with `phone`'s key; true when one took it.
    async fn send_all(&self, phone: EndpointId, notice: &Notice) -> bool {
        let tokens = match self.store.push_tokens(&phone).await {
            Ok(tokens) => tokens,
            Err(e) => {
                log::warn!("push: reading tokens failed; user={phone} error={e:#}");
                return false;
            }
        };
        if tokens.is_empty() {
            log::debug!("push: no tokens to push to; user={phone}");
        }
        let mut sent = false;
        // The token itself is never logged: it is all anyone needs to push
        // to that phone.
        for token in tokens {
            let service: &str = token.service.into();
            let Some(delivery) = self.send(&phone, &token, notice).await else {
                log::debug!("push: no key for {service}; user={phone}");
                continue;
            };
            match delivery {
                Ok(Delivery::Sent) => {
                    sent = true;
                    log::info!("{service}: pushed a notice; user={phone}");
                }
                Ok(Delivery::Gone) => {
                    if let Err(e) = self.store.forget_push_token(&phone, &token.token).await {
                        log::warn!("push: forgetting a token failed; user={phone} error={e:#}");
                    }
                }
                Ok(Delivery::Failed) => {}
                Err(e) => log::warn!("{service}: push failed; user={phone} error={e:#}"),
            }
        }
        sent
    }

    /// `None` when the server has no key for the token's service.
    async fn send(
        &self,
        phone: &EndpointId,
        token: &PushToken,
        notice: &Notice,
    ) -> Option<Result<Delivery>> {
        let environment = match token.service {
            PushService::Fcm => {
                let firebase = self.firebase.as_ref()?;
                return Some(firebase.push(phone, &token.token, notice).await);
            }
            PushService::ApnsSandbox => Environment::Sandbox,
            PushService::ApnsProduction => Environment::Production,
        };
        let apns = self.apns.as_ref()?;
        Some(apns.push(phone, &token.token, environment, notice).await)
    }
}

#[async_trait]
impl Fallback for RemotePush {
    async fn push(&self, phone: EndpointId, notice: &Notice) -> bool {
        self.send_all(phone, notice).await
    }
}

/// `notice` sealed in at most `max` bytes, body cut to fit; `None` when it never fits.
pub fn seal_to_fit(phone: &EndpointId, notice: &Notice, max: usize) -> Result<Option<String>> {
    let mut notice = notice.clone();
    loop {
        let sealed = seal::seal(phone, &notice)?;
        if sealed.len() <= max {
            return Ok(Some(sealed));
        }
        if notice.body.is_empty() {
            return Ok(None);
        }
        // Base64 grows the bytes by a third; cut that much and a little more.
        let over = (sealed.len() - max) * 3 / 4 + 16;
        let mut keep = notice.body.len().saturating_sub(over);
        while !notice.body.is_char_boundary(keep) {
            keep -= 1;
        }
        notice.body.truncate(keep);
        notice.body.truncate(notice.body.trim_end().len());
        if !notice.body.is_empty() {
            notice.body.push_str("...");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    const MAX: usize = 4000;

    fn notice(body: String) -> Notice {
        Notice {
            title: "Done".into(),
            body,
            session_id: "s1".into(),
            app: None,
            approval: None,
            changed: false,
            group: false,
        }
    }

    #[test]
    fn a_long_body_is_cut_to_fit_a_push() {
        let phone = SecretKey::from_bytes(&[1; 32]);
        let long = "é".repeat(5000);
        let sealed = seal_to_fit(&phone.public(), &notice(long.clone()), MAX)
            .unwrap()
            .expect("a body can always be cut");
        assert!(sealed.len() <= MAX);
        let opened = seal::open(&phone, &sealed).unwrap();
        assert!(opened.body.ends_with("..."));
        assert!(long.starts_with(opened.body.trim_end_matches("...")));
        assert!(opened.body.len() > 2000);
    }
}

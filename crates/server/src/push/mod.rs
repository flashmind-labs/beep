//! Delivers notices to every key of the account: over its connection, by dialing it, or via FCM or APNs.

mod deliver;
pub use deliver::{Directory, OwnKey, PushKey, Pusher};

pub mod apns;
pub mod fcm;
pub mod remote;

pub use beep_proto::client::CLOSE_REVOKED;

//! A user's account: the scoped `user::User`, single-user instance mode, hosted login, guest keys
//! and the credentials a user hands their machines.

pub mod credentials;
pub(crate) mod guest;
pub mod instance;
pub mod login;
pub mod user;

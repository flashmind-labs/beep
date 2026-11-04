//! The stateless layer of a project's repository watch: Actions API calls with their caches and
//! rate limits, the run shapes they read, and when a copy of a host's lists is read again.

pub mod actions;
pub mod http;
pub mod runs;
#[cfg(any(test, feature = "testing"))]
pub mod test_host;
pub mod timing;

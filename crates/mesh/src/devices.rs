//! The device list: an append-only set of signed ops, `Genesis`, `Add` and
//! `Remove`. The mesh id is the genesis desktop's `EndpointId`.
//!
//! Every op is signed over the mesh id as well as its body, so an op signed
//! for one mesh cannot be replayed into another.
//!
//! An op counts if its author was added by a counted op and no `Remove` of the
//! author was known when the op arrived. Only a `Desktop` may author any op,
//! and a device's kind is whatever the signing desktop states in its `Add`.
//! When several `Add`s name one key, a `Desktop` beats a `Node`, then the
//! earlier `(added_at_ms, author)` wins; the genesis device never changes.
//! A `Remove` holds until an `Add` of that key stamped after it: a removed
//! machine comes back with a fresh ticket and keeps its key.
//!
//! Accepted limit: an `Add` signed by a removed device, seen by a member
//! before the `Remove`, stays. Remove that device too.

mod membership;
mod signed;

pub use membership::{Devices, Merged, WriteLimit};
pub(crate) use signed::OpBody;
pub use signed::{Device, DeviceKind, DeviceOp};

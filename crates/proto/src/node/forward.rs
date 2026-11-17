//! `beep-peer/fwd/1`: a desktop reaches a TCP port on a machine's loopback
//! over its own connection. Each bi stream starts with the port as a
//! big-endian `u16`; after it the stream carries the TCP bytes both ways.

pub const ALPN: &[u8] = b"beep-peer/fwd/1";

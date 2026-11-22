//! Networking helpers.
//!
//! `http` only ever talks to kernel control endpoints on `127.0.0.1`. `latency`
//! and `remote` are the two exceptions that leave loopback: measuring a node
//! means touching the node, and fetching a subscription means talking to the
//! provider. Both are explicit, and neither is reachable from `http`.

pub mod http;
pub mod latency;
pub mod remote;

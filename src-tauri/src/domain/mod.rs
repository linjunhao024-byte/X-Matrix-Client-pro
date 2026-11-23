//! Domain model. No IO, no storage, no Tauri types — pure data.
//!
//! The whole point versus the old project: illegal states are unrepresentable.
//! A `Trojan` node cannot exist without a password; TLS cannot be attached to
//! a protocol that has no TLS layer. The compiler enforces it.

pub mod group;
pub mod node;
pub mod probe;
pub mod routing;
pub mod ruleset;
pub mod subscription;
pub mod settings;

#[allow(unused_imports)]
pub use group::{GroupKind, ProxyGroup};
#[allow(unused_imports)]
pub use node::{Node, NodeId, Protocol, Security, Transport};
#[allow(unused_imports)]
pub use probe::{NodeProbe, ProbeKind};
#[allow(unused_imports)]
pub use routing::{RoutingRule, RuleAction, RuleMatcher};
#[allow(unused_imports)]
pub use ruleset::{RuleSet, RuleSetSource, SetEntry};
#[allow(unused_imports)]
pub use settings::{ProxyMode, Settings};
#[allow(unused_imports)]
pub use subscription::Subscription;

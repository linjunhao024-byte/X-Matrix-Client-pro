use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use specta::Type;

pub type NodeId = String;

/// A single proxy node. Tagged enums carry the per-protocol requirements,
/// so a `Protocol::Trojan` can never be missing its password.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub server: String,
    pub port: u16,
    pub protocol: Protocol,
    pub transport: Transport,
    pub security: Security,
    /// Free-form grouping: subscription name, manual tag, etc.
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind")]
pub enum Protocol {
    Vless {
        uuid: String,
        #[serde(default)]
        flow: Option<String>,
    },
    Vmess {
        uuid: String,
        #[serde(default)]
        alter_id: u16,
    },
    Trojan {
        password: String,
    },
    Shadowsocks {
        method: String,
        password: String,
    },
    // QUIC-based, and sing-box only: Xray has no hysteria2 outbound. A node
    // of this kind can be stored and edited, but starting it while Xray is the
    // selected kernel fails with a message that says why — better than
    // rendering a config that silently drops the node.
    Hysteria2 {
        password: String,
        // salamander obfuscation password (`obfs-password`); empty means the
        // server does not obfuscate, which is the common case.
        #[serde(default)]
        obfs_password: Option<String>,
        // Bandwidth hints in Mbps; only used to pace congestion control when
        // the server sends no estimate of its own.
        #[serde(default)]
        up_mbps: Option<u32>,
        #[serde(default)]
        down_mbps: Option<u32>,
    },
    // TUIC v5, also sing-box only. The credential is a uuid *and* a password
    // — real providers require both, so the type demands both.
    Tuic {
        uuid: String,
        password: String,
        // `cubic` (the kernel default), `new_reno` or `bbr`.
        #[serde(default)]
        congestion_control: Option<String>,
        // `native` or `quic`.
        #[serde(default)]
        udp_relay_mode: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind")]
pub enum Transport {
    Tcp,
    Ws {
        #[serde(default)]
        path: String,
        #[serde(default)]
        host: Option<String>,
    },
    Grpc {
        service_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind")]
pub enum Security {
    None,
    Tls {
        #[serde(default)]
        sni: Option<String>,
        #[serde(default)]
        alpn: Vec<String>,
        /// Skip certificate verification. Self-signed certificates and
        /// bare-IP servers are common, and every other client ships this
        /// switch (`allowInsecure`, Clash's `skip-cert-verify`); without it
        /// those nodes simply cannot be used.
        ///
        /// sing-box honours this (`tls.insecure`). Xray removed
        /// `allowInsecure` in 26.x — it refuses to start when it sees the key —
        /// so for Xray the only honest way to accept a self-signed server is
        /// `pinned_cert_sha256`.
        #[serde(default)]
        allow_insecure: bool,
        /// SHA-256 of the server certificate (hex, colons optional). This is
        /// Xray's `pinnedPeerCertSha256`: instead of trusting any certificate
        /// it trusts exactly this one, which is stricter than skipping
        /// verification.
        #[serde(default)]
        pinned_cert_sha256: Vec<String>,
    },
    Reality {
        public_key: String,
        short_id: String,
        server_name: String,
        /// uTLS ClientHello fingerprint (`fp` in share links). Xray needs one
        /// and every client defaults to chrome, so old data can omit it.
        #[serde(default = "default_fingerprint")]
        fingerprint: String,
        /// Reality spider path (`spx`). Only Xray consumes it.
        #[serde(default = "default_spider_x")]
        spider_x: String,
    },
}

fn default_fingerprint() -> String {
    "chrome".to_string()
}

fn default_spider_x() -> String {
    "/".to_string()
}

impl Node {
    /// `server:port` — used in kernel error messages and the log view.
    /// The rules every path into the node table must satisfy: a node with no
    /// server or no name is a node the user cannot fix from the UI.
    pub fn validate(&self) -> AppResult<()> {
        if self.name.trim().is_empty() {
            return Err(AppError::InvalidInput("节点名称不能为空".into()));
        }
        if self.server.trim().is_empty() {
            return Err(AppError::InvalidInput("节点服务器不能为空".into()));
        }
        if self.port == 0 {
            return Err(AppError::InvalidInput("节点端口必须在 1–65535 之间".into()));
        }
        if let Security::Tls {
            pinned_cert_sha256, ..
        } = &self.security
        {
            // Xray refuses to start with "incorrect pinnedPeerCertSha256
            // length", which would take down the whole config; a truncated
            // fingerprint is a typo, so reject it here where we can say why.
            if let Some(bad) = pinned_cert_sha256.iter().find(|pin| {
                pin.len() != 64 || !pin.chars().all(|c| c.is_ascii_hexdigit())
            }) {
                return Err(AppError::InvalidInput(format!(
                    "证书指纹必须是 64 位十六进制：{bad}"
                )));
            }
        }
        Ok(())
    }

    pub fn endpoint(&self) -> String {
        format!("{}:{}", self.server, self.port)
    }

    /// Trim what a human typed and drop what is not a tag at all.
    ///
    /// Tags are the only free-form field on a node, so they are the only place
    /// where "  home " and "home" would otherwise become two different groups.
    /// Order is preserved and duplicates collapse: the first spelling wins.
    pub fn normalized_tags(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.tags.len());
        for raw in &self.tags {
            let tag = raw.trim();
            if tag.is_empty() {
                continue;
            }
            if !out.iter().any(|kept| kept == tag) {
                out.push(tag.to_string());
            }
        }
        out
    }

    /// Tags starting with `sub:` belong to the subscription machinery: refresh
    /// deletes nodes by tag, so a hand-made `sub:` tag would be silently eaten
    /// by the next refresh of a subscription that happens to share the id.
    pub fn check_user_tags(&self) -> AppResult<()> {
        if let Some(bad) = self
            .normalized_tags()
            .into_iter()
            .find(|tag| tag.starts_with(crate::domain::subscription::SOURCE_TAG_PREFIX))
        {
            return Err(AppError::InvalidInput(format!(
                "标签不能以 {} 开头（那是订阅自己用的）：{bad}",
                crate::domain::subscription::SOURCE_TAG_PREFIX
            )));
        }
        Ok(())
    }

    /// What makes two nodes "the same server": protocol, address, port and the
    /// credential.
    ///
    /// The name is excluded on purpose — providers rename nodes between
    /// refreshes, and the same server listed by two subscriptions has to
    /// collapse into one entry, otherwise every import doubles the list. The
    /// transport is excluded too: one endpoint reachable over both ws and tcp
    /// is a provider bug, not two services worth keeping.
    pub fn fingerprint(&self) -> String {
        let (kind, credential) = match &self.protocol {
            Protocol::Vless { uuid, .. } => ("vless", uuid.as_str()),
            Protocol::Vmess { uuid, .. } => ("vmess", uuid.as_str()),
            Protocol::Trojan { password } => ("trojan", password.as_str()),
            Protocol::Shadowsocks { password, .. } => ("shadowsocks", password.as_str()),
            Protocol::Hysteria2 { password, .. } => ("hysteria2", password.as_str()),
            // TUIC needs both halves to be the same server; the uuid is the
            // part that identifies the account.
            Protocol::Tuic { uuid, .. } => ("tuic", uuid.as_str()),
        };
        format!(
            "{kind}|{}|{}|{credential}",
            self.server.trim().to_lowercase(),
            self.port
        )
    }
}

impl Protocol {
    /// The protocols that live inside their own QUIC TLS handshake. Only
    /// sing-box can run these; Xray has no outbound for either, so its
    /// renderer has to refuse rather than pretend.
    pub fn is_quic(&self) -> bool {
        matches!(self, Protocol::Hysteria2 { .. } | Protocol::Tuic { .. })
    }

    /// How the protocol is spelled on the screen and in error messages.
    pub fn label(&self) -> &'static str {
        match self {
            Protocol::Vless { .. } => "VLESS",
            Protocol::Vmess { .. } => "VMess",
            Protocol::Trojan { .. } => "Trojan",
            Protocol::Shadowsocks { .. } => "Shadowsocks",
            Protocol::Hysteria2 { .. } => "Hysteria2",
            Protocol::Tuic { .. } => "TUIC",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node_with(security: Security) -> Node {
        Node {
            id: "n1".into(),
            name: "n1".into(),
            server: "example.com".into(),
            port: 443,
            protocol: Protocol::Vless {
                uuid: "uuid-1".into(),
                flow: None,
            },
            transport: Transport::Tcp,
            security,
            tags: vec![],
        }
    }

    #[test]
    fn a_truncated_certificate_pin_is_rejected() {
        let node = node_with(Security::Tls {
            sni: None,
            alpn: vec![],
            allow_insecure: false,
            pinned_cert_sha256: vec!["93c559a5d6605453".into()],
        });
        let error = node.validate().unwrap_err();
        assert!(format!("{error}").contains("64"));
    }

    #[test]
    fn a_full_certificate_pin_is_accepted() {
        let node = node_with(Security::Tls {
            sni: None,
            alpn: vec![],
            allow_insecure: false,
            pinned_cert_sha256: vec![
                "93c559a5d66054530f6831870c92dd7e85f2b1942507507cd12b685d838c04de".into(),
            ],
        });
        assert!(node.validate().is_ok());
    }


    #[test]
    fn trojan_requires_password() {
        // Type-level guarantee: this is the only way to build a Trojan node,
        // and it cannot compile without a password.
        let n = Node {
            id: "n1".into(),
            name: "t".into(),
            server: "example.com".into(),
            port: 443,
            protocol: Protocol::Trojan { password: "p".into() },
            transport: Transport::Tcp,
            security: Security::None,
            tags: vec![],
        };
        assert_eq!(n.endpoint(), "example.com:443");
    }

    #[test]
    fn node_roundtrips_through_json() {
        let n = Node {
            id: "n1".into(),
            name: "t".into(),
            server: "example.com".into(),
            port: 443,
            protocol: Protocol::Vless { uuid: "u".into(), flow: None },
            transport: Transport::Ws { path: "/x".into(), host: None },
            security: Security::Reality {
                public_key: "pk".into(),
                short_id: "sid".into(),
                server_name: "sni".into(),
                fingerprint: default_fingerprint(),
                spider_x: default_spider_x(),
            },
            tags: vec!["sub-a".into()],
        };
        let s = serde_json::to_string(&n).unwrap();
        let back: Node = serde_json::from_str(&s).unwrap();
        assert_eq!(back.endpoint(), n.endpoint());
    }
}

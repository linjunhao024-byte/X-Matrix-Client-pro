//! Traffic statistics, normalised across the two kernels.
//!
//! The kernels disagree about what "traffic" means:
//!
//! | kernel | endpoint | semantics |
//! |---|---|---|
//! | Xray | `GET /debug/vars` (expvar) | **cumulative** byte counters per outbound |
//! | sing-box | `GET /connections` (Clash API) | totals plus per-connection counters |
//!
//! Both are mapped to [`TrafficSample`], which is always **cumulative**. Rates
//! are a UI concern: the frontend keeps the previous sample and divides by the
//! elapsed time, so a dropped poll never produces a bogus spike.

use serde::Serialize;
use serde_json::Value;
use specta::Type;

use crate::error::{AppError, AppResult};
use crate::net::http;

/// One live connection. Only sing-box can populate this — Xray exposes no
/// connection list (see `docs/ENGINE-SPEC.md` §4).
#[derive(Debug, Clone, Default, Serialize, Type)]
pub struct ConnectionInfo {
    pub id: String,
    /// Destination host if the kernel knows it, otherwise the IP.
    pub host: String,
    pub network: String,
    pub rule: String,
    pub chains: Vec<String>,
    // Specta refuses u64 (precision loss). JS numbers are f64 and exact
    // below 2^53, which byte counters never reach.
    #[specta(type = f64)]
    pub upload: u64,
    #[specta(type = f64)]
    pub download: u64,
    /// RFC3339 start time as reported by the kernel.
    pub start: String,
}

/// Cumulative traffic counters, plus (when available) the live connection list.
#[derive(Debug, Clone, Default, Serialize, Type)]
pub struct TrafficSample {
    /// Bytes sent through the proxy outbound.
    #[specta(type = f64)]
    pub up_total: u64,
    /// Bytes received through the proxy outbound.
    #[specta(type = f64)]
    pub down_total: u64,
    /// Bytes that bypassed the proxy (Xray only; 0 on sing-box).
    #[specta(type = f64)]
    pub direct_up: u64,
    #[specta(type = f64)]
    pub direct_down: u64,
    /// Empty on Xray: that kernel has no connection-list API.
    pub connections: Vec<ConnectionInfo>,
    /// True when the kernel can report connections at all, so the UI can show
    /// "this kernel has no connection list" instead of "no connections".
    pub supports_connections: bool,
}

/// Xray: parse the expvar document at `/debug/vars`.
///
/// Handles both shapes seen in the wild — flat numeric keys
/// (`"proxy>>>traffic>>>uplink": 123`) and nested objects
/// (`"proxy": {"uplink": 123, "downlink": 456}`) — because the exact shape
/// depends on the Xray build.
pub fn read_xray(port: u16) -> AppResult<TrafficSample> {
    let vars = http::get_json(port, "/debug/vars")?;
    let outbound = vars
        .pointer("/stats/outbound")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Engine("expvar 里没有 stats.outbound".to_string()))?;

    let mut sample = TrafficSample {
        supports_connections: false,
        ..Default::default()
    };

    for (key, value) in outbound {
        let (up, down) = match value {
            Value::Object(fields) => (
                fields.get("uplink").and_then(Value::as_u64).unwrap_or(0),
                fields.get("downlink").and_then(Value::as_u64).unwrap_or(0),
            ),
            Value::Number(n) => {
                let bytes = n.as_u64().unwrap_or(0);
                if key.ends_with("uplink") {
                    (bytes, 0)
                } else if key.ends_with("downlink") {
                    (0, bytes)
                } else {
                    (0, 0)
                }
            }
            _ => (0, 0),
        };

        // Outbound stat keys are `"<tag>>>traffic>>><direction>"`; the tag is
        // the first segment.
        let tag = key.split(">>>").next().unwrap_or(key.as_str());
        match tag {
            "proxy" => {
                sample.up_total += up;
                sample.down_total += down;
            }
            "direct" => {
                sample.direct_up += up;
                sample.direct_down += down;
            }
            _ => {}
        }
    }

    Ok(sample)
}

/// sing-box: parse the Clash API `/connections` document.
pub fn read_clash(port: u16) -> AppResult<TrafficSample> {
    let doc = http::get_json(port, "/connections")?;

    let mut sample = TrafficSample {
        supports_connections: true,
        ..Default::default()
    };

    let list = doc
        .get("connections")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut summed_up = 0u64;
    let mut summed_down = 0u64;

    for item in &list {
        let upload = item.get("upload").and_then(Value::as_u64).unwrap_or(0);
        let download = item.get("download").and_then(Value::as_u64).unwrap_or(0);
        summed_up += upload;
        summed_down += download;

        let meta = item.get("metadata");
        let host = meta
            .and_then(|m| m.get("host"))
            .and_then(Value::as_str)
            .filter(|h| !h.is_empty())
            .or_else(|| meta.and_then(|m| m.get("destinationIP")).and_then(Value::as_str))
            .unwrap_or("")
            .to_string();

        sample.connections.push(ConnectionInfo {
            id: item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            host,
            network: meta
                .and_then(|m| m.get("network"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            rule: item
                .get("rule")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            chains: item
                .get("chains")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            upload,
            download,
            start: item
                .get("start")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        });
    }

    // The kernel's own totals are authoritative; per-connection sums are the
    // fallback for builds that omit them.
    sample.up_total = doc
        .get("uploadTotal")
        .and_then(Value::as_u64)
        .unwrap_or(summed_up);
    sample.down_total = doc
        .get("downloadTotal")
        .and_then(Value::as_u64)
        .unwrap_or(summed_down);

    Ok(sample)
}

/// sing-box: ask the Clash API to drop one connection.
///
/// Only sing-box can do this. Xray exposes counters, not a connection table, so
/// there is nothing there to close — the caller must say so instead of failing
/// silently.
pub fn close_connection(port: u16, id: &str) -> AppResult<()> {
    if id.trim().is_empty() {
        return Err(AppError::InvalidInput("连接 id 为空".into()));
    }
    http::delete_with_timeout(port, &format!("/connections/{id}"), http::DEFAULT_TIMEOUT)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn serve_once(body: &'static str) -> u16 {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            if let Ok((mut socket, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf);
                let _ = socket.write_all(response.as_bytes());
            }
        });
        port
    }

    #[test]
    fn xray_flat_numeric_counters_are_summed_per_tag() {
        let port = serve_once(
            r#"{"stats":{"outbound":{
                "proxy>>>traffic>>>uplink":1500,
                "proxy>>>traffic>>>downlink":2500,
                "direct>>>traffic>>>uplink":10,
                "direct>>>traffic>>>downlink":20
            }}}"#,
        );
        let s = read_xray(port).unwrap();
        assert_eq!(s.up_total, 1500);
        assert_eq!(s.down_total, 2500);
        assert_eq!(s.direct_up, 10);
        assert_eq!(s.direct_down, 20);
        // Xray cannot list connections; the flag lets the UI say so.
        assert!(!s.supports_connections);
        assert!(s.connections.is_empty());
    }

    #[test]
    fn xray_nested_counter_objects_are_also_understood() {
        let port = serve_once(
            r#"{"stats":{"outbound":{
                "proxy":{"uplink":7,"downlink":9}
            }}}"#,
        );
        let s = read_xray(port).unwrap();
        assert_eq!((s.up_total, s.down_total), (7, 9));
    }

    #[test]
    fn xray_without_stats_section_is_an_error() {
        let port = serve_once(r#"{"stats":{}}"#);
        let err = read_xray(port).unwrap_err();
        assert!(matches!(err, AppError::Engine(_)));
    }

    #[test]
    fn clash_connections_are_flattened_into_rows() {
        let port = serve_once(
            r#"{"uploadTotal":900,"downloadTotal":1800,"connections":[
                {"id":"c1","upload":100,"download":200,"start":"2026-11-21T10:00:00Z",
                 "rule":"GeoIP,CN","chains":["proxy","hop"],
                 "metadata":{"host":"example.com","network":"tcp","destinationIP":"1.2.3.4"}},
                {"id":"c2","upload":50,"download":60,"start":"2026-11-21T10:00:05Z",
                 "rule":"Match","chains":["proxy"],
                 "metadata":{"network":"udp","destinationIP":"5.6.7.8"}}
            ]}"#,
        );
        let s = read_clash(port).unwrap();
        assert!(s.supports_connections);
        assert_eq!(s.connections.len(), 2);
        assert_eq!(s.connections[0].host, "example.com");
        assert_eq!(s.connections[0].rule, "GeoIP,CN");
        assert_eq!(s.connections[0].chains, vec!["proxy", "hop"]);
        // Falls back to the destination IP when no hostname is known.
        assert_eq!(s.connections[1].host, "5.6.7.8");
        assert_eq!(s.connections[1].network, "udp");
        // Kernel totals win over the per-connection sum (150 vs 900).
        assert_eq!(s.up_total, 900);
        assert_eq!(s.down_total, 1800);
    }

    #[test]
    fn clash_without_totals_falls_back_to_the_sum() {
        let port = serve_once(
            r#"{"connections":[{"id":"c1","upload":5,"download":6,"metadata":{}}]}"#,
        );
        let s = read_clash(port).unwrap();
        assert_eq!((s.up_total, s.down_total), (5, 6));
    }

    #[test]
    fn empty_clash_document_is_not_an_error() {
        let port = serve_once(r#"{"connections":[]}"#);
        let s = read_clash(port).unwrap();
        assert_eq!(s.up_total, 0);
        assert!(s.supports_connections);
    }
}

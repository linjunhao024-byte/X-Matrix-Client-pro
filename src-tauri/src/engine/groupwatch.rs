//! The app-side executor for `fallback` and `load-balance` groups.
//!
//! Neither sing-box nor Xray has an outbound type that falls back or rotates:
//! Clash does this in its own core, and it is the reason those two group kinds
//! have no honest static rendering. The renderer therefore emits a plain
//! `selector` and this module does the part the kernel cannot:
//!
//! 1. probe every member through the Clash API (`/proxies/node:<id>/delay`);
//! 2. decide which member should carry the group
//!    ([`choose`] — pure, so the ordering rules are testable without a socket);
//! 3. read what the kernel is on (`/proxies/group:<id>`, field `now`) and only
//!    write a switch when it actually differs.
//!
//! Two deliberate limits, both visible to the user in the UI:
//!
//! * it only runs under sing-box — Xray has no selector to rewrite, so those
//!   groups stay static there;
//! * `load-balance` rotates **by time** (one member per round), not per
//!   connection. No kernel this app drives can spread individual connections
//!   across outbounds, and claiming otherwise would be a fake switch.
//!
//! The interval is a constant rather than a setting for now: one number on the
//! groups page is easier to explain than a knob nobody knows how to tune.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::stats;
use crate::domain::group::GroupKind;
use crate::net::http;

/// How often the watcher probes and re-decides.
pub const WATCH_INTERVAL: Duration = Duration::from_secs(20);

/// Per-member probe budget handed to the kernel.
///
/// The same number the groups page uses for a manual measurement, so a member
/// that looks healthy in the UI is healthy to the watcher too.
pub const PROBE_TIMEOUT_MS: u64 = 3_000;

/// One group the watcher is responsible for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupSpec {
    pub id: String,
    pub kind: GroupKind,
    /// Node ids, in the order the user arranged them.
    pub members: Vec<String>,
}

/// A switch that really happened (the kernel was told, and the write worked).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switch {
    pub group_id: String,
    /// The member tag the group was moved to, e.g. `node:n2`.
    pub to: String,
}

/// Pick the member index that should carry the group this round.
///
/// `health` is parallel to the group's members. `Select` and `UrlTest` return
/// `None`: the kernel already handles those, and a second decider would fight
/// it.
pub fn choose(kind: GroupKind, health: &[bool], tick: u64) -> Option<usize> {
    match kind {
        // First member in the user's order that answers.
        GroupKind::Fallback => health.iter().position(|healthy| *healthy),
        // Rotate through the healthy ones, one per round. Modulo rather than a
        // stored cursor so the decision stays a pure function of the tick: a
        // restart of the app resumes the same rotation instead of starting
        // over, and a group whose health set shrank degrades predictably.
        GroupKind::LoadBalance => {
            let healthy: Vec<usize> = health
                .iter()
                .enumerate()
                .filter_map(|(index, healthy)| if *healthy { Some(index) } else { None })
                .collect();
            if healthy.is_empty() {
                return None;
            }
            Some(healthy[(tick as usize) % healthy.len()])
        }
        GroupKind::Select | GroupKind::UrlTest => None,
    }
}

/// Everything one watcher thread needs. Cheap to build; owns no sockets.
pub struct Runner {
    api_port: u16,
    test_url: String,
    groups: Vec<GroupSpec>,
}

impl Runner {
    pub fn new(api_port: u16, test_url: String, groups: Vec<GroupSpec>) -> Self {
        Self {
            api_port,
            test_url,
            groups,
        }
    }

    /// One round: probe, decide, switch. Returns the switches that really
    /// happened, which is what the log line and the tests both read.
    ///
    /// A probe error is a `false` (the member is unhealthy), and a failed
    /// switch is simply not reported: a group that could not be moved must not
    /// look like it moved.
    pub fn tick_once(&self, tick: u64) -> Vec<Switch> {
        let mut switches = Vec::new();

        for group in &self.groups {
            if !matches!(group.kind, GroupKind::Fallback | GroupKind::LoadBalance) {
                continue;
            }
            if group.members.is_empty() {
                continue;
            }

            let health: Vec<bool> = group
                .members
                .iter()
                .map(|member| {
                    stats::clash_delay(
                        self.api_port,
                        &format!("node:{member}"),
                        &self.test_url,
                        PROBE_TIMEOUT_MS,
                    )
                    .is_ok()
                })
                .collect();

            let Some(index) = choose(group.kind, &health, tick) else {
                // Nobody answers: leave the kernel on whatever it was on. A
                // selector cannot be emptied, and switching to a dead member
                // would make the group strictly worse than doing nothing.
                continue;
            };
            let target = format!("node:{}", group.members[index]);
            let group_tag = format!("group:{}", group.id);

            // Only write when it differs: the Clash API is fine with redundant
            // PUTs, but a switch that did not change anything is noise in the
            // log and hides the real transitions.
            let current = stats::clash_now(self.api_port, &group_tag)
                .ok()
                .flatten();
            if current.as_deref() == Some(target.as_str()) {
                continue;
            }

            let body = format!("{{\"name\":\"{target}\"}}");
            let switched = http::put_json_timeout(
                self.api_port,
                &format!("/proxies/{}", stats::encode(&group_tag)),
                &body,
                Duration::from_secs(5),
            )
            .is_ok();
            if switched {
                switches.push(Switch {
                    group_id: group.id.clone(),
                    to: target,
                });
            }
        }

        switches
    }
}

/// Run until `stop` is set.
///
/// The sleep is sliced so a stop request is honoured within half a second
/// instead of after the full interval: stopping the kernel should not wait 20
/// seconds with a thread still probing a port that is about to close.
pub fn run(runner: Runner, stop: Arc<AtomicBool>) {
    let mut tick: u64 = 0;
    while !stop.load(Ordering::SeqCst) {
        for switch in runner.tick_once(tick) {
            eprintln!(
                "x-matrix: 策略组 {} 切到 {}，原因：健康探测",
                switch.group_id, switch.to
            );
        }
        tick = tick.wrapping_add(1);

        let deadline = Instant::now() + WATCH_INTERVAL;
        while !stop.load(Ordering::SeqCst) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            thread::sleep(left.min(Duration::from_millis(500)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn spec(kind: GroupKind, members: &[&str]) -> GroupSpec {
        GroupSpec {
            id: "g1".into(),
            kind,
            members: members.iter().map(|member| member.to_string()).collect(),
        }
    }

    #[test]
    fn fallback_takes_the_first_member_that_answers() {
        assert_eq!(choose(GroupKind::Fallback, &[false, true, true], 0), Some(1));
        assert_eq!(choose(GroupKind::Fallback, &[true, true], 7), Some(0));
    }

    #[test]
    fn fallback_with_nobody_healthy_does_nothing() {
        assert_eq!(choose(GroupKind::Fallback, &[false, false], 0), None);
        assert_eq!(choose(GroupKind::Fallback, &[], 0), None);
    }

    #[test]
    fn load_balance_rotates_through_the_healthy_members() {
        // Two healthy of three: the dead middle one is skipped entirely.
        assert_eq!(choose(GroupKind::LoadBalance, &[true, false, true], 0), Some(0));
        assert_eq!(choose(GroupKind::LoadBalance, &[true, false, true], 1), Some(2));
        assert_eq!(choose(GroupKind::LoadBalance, &[true, false, true], 2), Some(0));
        // With one healthy member the rotation is a constant.
        assert_eq!(choose(GroupKind::LoadBalance, &[false, true], 5), Some(1));
        assert_eq!(choose(GroupKind::LoadBalance, &[false, false], 0), None);
    }

    #[test]
    fn the_kernel_owned_kinds_are_never_decided_here() {
        assert_eq!(choose(GroupKind::Select, &[true], 0), None);
        assert_eq!(choose(GroupKind::UrlTest, &[true], 0), None);
    }

    /// A Clash API stub that answers the four calls one `tick_once` makes and
    /// hands the raw requests back. Not production code: `#[cfg(test)]` only.
    fn clash_stub(now: &'static str, expected: usize) -> (u16, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..expected {
                let Ok((mut socket, _)) = listener.accept() else {
                    break;
                };
                let mut data = Vec::new();
                let mut buf = [0u8; 1024];
                loop {
                    let read = socket.read(&mut buf).unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    data.extend_from_slice(&buf[..read]);
                    let text = String::from_utf8_lossy(&data);
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let length = head
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|value| value.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if body.len() >= length {
                            break;
                        }
                    }
                }
                let text = String::from_utf8_lossy(&data).into_owned();
                let request_line = text.lines().next().unwrap_or_default().to_string();
                let (status, body) = if request_line.contains("/proxies/node%3An1/delay") {
                    // A dead member: the kernel's own answer when a probe
                    // fails is a non-2xx with a message.
                    ("500 Internal Server Error", "{\"message\":\"timeout\"}")
                } else if request_line.contains("/proxies/node%3An2/delay") {
                    ("200 OK", "{\"delay\":120}")
                } else if request_line.starts_with("GET /proxies/group%3Ag1 ") {
                    ("200 OK", now)
                } else {
                    ("204 No Content", "")
                };
                requests.push(text);
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes());
            }
            requests
        });
        (port, handle)
    }

    #[test]
    fn a_fallback_group_switches_to_the_first_healthy_member() {
        let (port, handle) = clash_stub("{\"now\":\"node:n1\"}", 4);
        let runner = Runner::new(
            port,
            "http://www.gstatic.com/generate_204".into(),
            vec![spec(GroupKind::Fallback, &["n1", "n2"])],
        );

        let switches = runner.tick_once(0);
        assert_eq!(
            switches,
            vec![Switch {
                group_id: "g1".into(),
                to: "node:n2".into(),
            }]
        );

        let requests = handle.join().unwrap();
        assert!(
            requests
                .iter()
                .any(|request| request.starts_with("PUT /proxies/group%3Ag1 HTTP/1.1")),
            "no selector write in {requests:?}"
        );
        assert!(
            requests
                .iter()
                .any(|request| request.ends_with("\r\n\r\n{\"name\":\"node:n2\"}")),
            "the switch must carry the target member: {requests:?}"
        );
    }

    #[test]
    fn a_group_already_on_the_right_member_is_not_written_again() {
        // Only three calls: two probes and the read. No PUT.
        let (port, handle) = clash_stub("{\"now\":\"node:n2\"}", 3);
        let runner = Runner::new(
            port,
            "http://www.gstatic.com/generate_204".into(),
            vec![spec(GroupKind::Fallback, &["n1", "n2"])],
        );

        assert!(runner.tick_once(0).is_empty());
        let requests = handle.join().unwrap();
        assert!(
            requests
                .iter()
                .all(|request| !request.starts_with("PUT ")),
            "a no-op switch must not be sent: {requests:?}"
        );
    }

    #[test]
    fn a_group_with_nobody_healthy_keeps_what_the_kernel_has() {
        let (port, handle) = clash_stub("{\"now\":\"node:n1\"}", 2);
        let runner = Runner::new(
            port,
            "http://www.gstatic.com/generate_204".into(),
            // Both members are unknown to the stub, so both probes fail.
            vec![spec(GroupKind::LoadBalance, &["dead1", "dead2"])],
        );

        assert!(runner.tick_once(0).is_empty());
        let requests = handle.join().unwrap();
        assert_eq!(requests.len(), 2, "only the probes ran: {requests:?}");
    }

    #[test]
    fn a_stopped_watcher_does_not_probe() {
        let stop = Arc::new(AtomicBool::new(true));
        let started = Instant::now();
        run(
            Runner::new(9, "http://www.gstatic.com/generate_204".into(), vec![]),
            stop,
        );
        // Port 9 is not listening; reaching it would take the probe budget.
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_stop_request_is_honoured_between_sleep_slices() {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let setter = thread::spawn(move || {
            thread::sleep(Duration::from_millis(120));
            flag.store(true, Ordering::SeqCst);
        });

        let started = Instant::now();
        run(
            Runner::new(9, "http://www.gstatic.com/generate_204".into(), vec![]),
            stop,
        );
        setter.join().unwrap();
        // Well under the full 20 s interval: the sleep is sliced.
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
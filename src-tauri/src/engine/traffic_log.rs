//! Per-minute traffic history, kept across restarts.
//!
//! The live chart in the home page is a 60-second ring buffer: restart the app
//! and it is empty again, which makes "how much did I use today" unanswerable.
//! Clash and V2RayN both keep a longer record, so this module appends one line
//! per minute to `traffic.jsonl` next to the store.
//!
//! Why a plain text file and not the store document: the store is rewritten in
//! full on every edit, and a minute-resolution history would make that document
//! grow without bound. One append-only line per minute is the cheapest thing
//! that survives a crash, and it can be inspected with `type` when something
//! looks wrong.
//!
//! Rules that keep it honest:
//! * the numbers come from the kernel's own cumulative counters, so a minute is
//!   the difference between two samples — not a guess;
//! * a counter that goes *down* means the kernel restarted (or was switched);
//!   that sample contributes zero instead of a negative spike;
//! * the file is trimmed to the last seven days, and lines that cannot be parsed
//!   are skipped instead of refusing to load the rest.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// One minute of proxied traffic. `minute` is unix minutes (`unix_seconds / 60`),
/// so the value is timezone-free and the UI can format it however it likes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TrafficPoint {
    #[specta(type = f64)]
    pub minute: i64,
    #[specta(type = f64)]
    pub up: u64,
    #[specta(type = f64)]
    pub down: u64,
}

/// How many minutes to keep. Seven days at one line per minute is a few
/// hundred kilobytes — small enough to rewrite when trimming.
pub const KEEP_MINUTES: usize = 7 * 24 * 60;

/// One sample's worth of movement, before it is attributed to a minute.
struct Bucket {
    minute: i64,
    up: u64,
    down: u64,
}

struct Inner {
    /// Cumulative counters of the sample we last saw, so the next one can be
    /// turned into a difference.
    last_up: u64,
    last_down: u64,
    seen: bool,
    bucket: Bucket,
    points: Vec<TrafficPoint>,
}

pub struct TrafficLog {
    path: PathBuf,
    inner: Mutex<Inner>,
}

impl TrafficLog {
    /// Load the existing history (dropping unreadable lines) and trim it.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let mut points = read_points(&path);
        if points.len() > KEEP_MINUTES {
            let excess = points.len() - KEEP_MINUTES;
            points.drain(..excess);
            let _ = write_points(&path, &points);
        }
        let bucket = Bucket {
            minute: points.last().map(|point| point.minute).unwrap_or(0),
            up: 0,
            down: 0,
        };
        Self {
            path,
            inner: Mutex::new(Inner {
                last_up: 0,
                last_down: 0,
                seen: false,
                bucket,
                points,
            }),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Feed one cumulative sample. `now_seconds` is the wall clock, so the
    /// minute buckets line up with the user's expectation of "which minute".
    pub fn observe(&self, up_total: u64, down_total: u64, now_seconds: i64) {
        let minute = now_seconds.div_euclid(60);
        let mut inner = self.inner.lock();
        if !inner.seen {
            // First sample of a run establishes the baseline; the counters are
            // cumulative, so charging everything so far to this minute would
            // invent traffic that happened before the app was watching.
            inner.last_up = up_total;
            inner.last_down = down_total;
            inner.seen = true;
            inner.bucket.minute = minute;
            return;
        }

        let up_delta = up_total.saturating_sub(inner.last_up);
        let down_delta = down_total.saturating_sub(inner.last_down);
        if up_total < inner.last_up || down_total < inner.last_down {
            // The kernel restarted (or we switched kernels): counters went
            // back to zero. Charge nothing and re-baseline.
            inner.last_up = up_total;
            inner.last_down = down_total;
            return;
        }
        inner.last_up = up_total;
        inner.last_down = down_total;

        if minute != inner.bucket.minute {
            // Only a minute that actually saw traffic becomes a point: an idle
            // hour is an hour with no line, not forty empty ones.
            if inner.bucket.minute > 0 && (inner.bucket.up > 0 || inner.bucket.down > 0) {
                let point = TrafficPoint {
                    minute: inner.bucket.minute,
                    up: inner.bucket.up,
                    down: inner.bucket.down,
                };
                inner.points.push(point);
                let excess = inner.points.len().saturating_sub(KEEP_MINUTES);
                if excess > 0 {
                    inner.points.drain(..excess);
                }
                let _ = append_point(&self.path, &point);
                if excess > 0 {
                    // The in-memory list is authoritative; rewrite so the file
                    // never grows past the same bound.
                    let _ = write_points(&self.path, &inner.points);
                }
            }
            inner.bucket = Bucket {
                minute,
                up: up_delta,
                down: down_delta,
            };
        } else {
            inner.bucket.up = inner.bucket.up.saturating_add(up_delta);
            inner.bucket.down = inner.bucket.down.saturating_add(down_delta);
        }
    }

    /// The last `minutes` of history, oldest first. Zero minutes means "the
    /// whole file".
    pub fn points(&self, minutes: i64, now_seconds: i64) -> Vec<TrafficPoint> {
        let inner = self.inner.lock();
        if minutes <= 0 {
            return inner.points.clone();
        }
        let oldest = now_seconds.div_euclid(60).saturating_sub(minutes);
        inner
            .points
            .iter()
            .copied()
            .filter(|point| point.minute >= oldest)
            .collect()
    }

    /// Wipe the history. Used by the "clear" button, and nothing else.
    pub fn clear(&self) -> AppResult<()> {
        let mut inner = self.inner.lock();
        inner.points.clear();
        inner.bucket.up = 0;
        inner.bucket.down = 0;
        write_points(&self.path, &[]) .map_err(|error| {
            AppError::Engine(format!("无法清空流量历史：{error}"))
        })
    }
}

fn read_points(path: &Path) -> Vec<TrafficPoint> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let mut points = Vec::new();
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            continue;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<TrafficPoint>(trimmed) {
            Ok(point) => points.push(point),
            // A half-written line is expected after a crash: skip it instead of
            // losing the rest of the file.
            Err(_) => continue,
        }
    }
    points.sort_by_key(|point| point.minute);
    points
}

fn append_point(path: &Path, point: &TrafficPoint) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(point)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    writeln!(file, "{line}")
}

/// Rewrite the file atomically, same discipline as the store: write a
/// temporary file and rename it into place so a crash never leaves a truncated
/// history.
fn write_points(path: &Path, points: &[TrafficPoint]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut file = File::create(&tmp)?;
        for point in points {
            let line = serde_json::to_string(point)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            writeln!(file, "{line}")?;
        }
        file.flush()?;
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same convention as the other suites: scratch space under the crate's
    /// own `target/`, not the system temp directory.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("traffic-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_first_sample_only_establishes_a_baseline() {
        let log = TrafficLog::new(temp_dir("baseline").join("traffic.jsonl"));
        // 10 MB happened before we were watching: it must not be charged to now.
        log.observe(10_000_000, 20_000_000, 60 * 1000);
        assert!(log.points(0, 60 * 1000).is_empty());
        // The next sample inside the same minute is a real difference.
        log.observe(10_000_700, 20_002_000, 60 * 1000 + 1);
        log.observe(10_001_200, 20_004_000, 60 * 1000 + 2);
        assert!(log.points(0, 60 * 1000 + 2).is_empty(), "the open minute is not a point yet");
        // Crossing into the next minute closes it.
        log.observe(10_001_200, 20_004_000, 61 * 1000);
        let points = log.points(0, 61 * 1000);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].up, 1_200);
        assert_eq!(points[0].down, 4_000);
    }

    #[test]
    fn a_restarted_kernel_never_produces_a_negative_minute() {
        let log = TrafficLog::new(temp_dir("restart").join("traffic.jsonl"));
        log.observe(5_000, 9_000, 100 * 60);
        log.observe(5_100, 9_200, 100 * 60 + 30);
        // The kernel was restarted: counters are back near zero.
        log.observe(10, 20, 100 * 60 + 31);
        log.observe(500, 900, 101 * 60);
        let points = log.points(0, 101 * 60);
        assert_eq!(points.len(), 1);
        assert_eq!((points[0].up, points[0].down), (100, 200));
    }

    #[test]
    fn history_survives_a_restart_and_is_filtered_by_window() {
        let dir = temp_dir("restart-window");
        let path = dir.join("traffic.jsonl");
        {
            let log = TrafficLog::new(&path);
            log.observe(0, 0, 10 * 60);
            log.observe(100, 200, 11 * 60);
            log.observe(400, 900, 12 * 60);
            log.observe(900, 1_800, 13 * 60);
        }
        let log = TrafficLog::new(&path);
        let all = log.points(0, 13 * 60);
        assert_eq!(all.len(), 2, "two minutes carried traffic and were written");
        let recent = log.points(1, 13 * 60);
        assert_eq!(recent.len(), 1, "only the last minute survives a one-minute window");
        assert_eq!(recent[0].down, 700);
    }

    #[test]
    fn a_corrupt_line_does_not_take_down_the_history() {
        let dir = temp_dir("corrupt");
        let path = dir.join("traffic.jsonl");
        fs::write(&path, "{\"minute\":10,\"up\":1,\"down\":2}\nhello\n{\"minute\":11,\"up\":3,\"down\":4}\n").unwrap();
        let log = TrafficLog::new(&path);
        let points = log.points(0, 11 * 60);
        assert_eq!(points.len(), 2);
        assert_eq!(points[1].up, 3);
    }

    #[test]
    fn clearing_empties_both_memory_and_disk() {
        let dir = temp_dir("clear");
        let path = dir.join("traffic.jsonl");
        let log = TrafficLog::new(&path);
        log.observe(0, 0, 60 * 5);
        log.observe(10, 20, 60 * 6);
        log.observe(30, 50, 60 * 7);
        assert_eq!(log.points(0, 60 * 7).len(), 1);
        log.clear().unwrap();
        assert!(log.points(0, 60 * 6).is_empty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
    }
}

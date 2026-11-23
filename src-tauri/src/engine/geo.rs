//! Geo data the rule layer depends on: `geoip.dat` and `geosite.dat`.
//!
//! Rules written as `geoip:cn` or `geosite:geolocation-!cn` are rendered into
//! both kernels' configs, but a kernel that cannot find the data file does not
//! complain in any way a user would notice: Xray exits during config load, and
//! sing-box (whose official builds embed their own copy) simply matches
//! nothing. Either way the rule looks broken and the GUI has nothing to say.
//!
//! So the files are checked explicitly and can be replaced atomically. Nothing
//! here starts a kernel or reads a config: it is file metadata plus the list of
//! places the same bytes can be downloaded from.

use crate::error::{AppError, AppResult};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// The v2ray-rules-dat builds are the ones both kernels accept without a
/// conversion step; Xray's own release assets are the same format, but these
/// are tagged more often and are what most clients point at.
///
/// GitHub's own host is unreachable from some networks that still reach its
/// mirrors, so each asset has three interchangeable copies: jsDelivr (a CDN in
/// front of the same repo), `raw.githubusercontent.com` (a different hostname
/// with a different TLS path), and finally the canonical release asset. A
/// caller tries them in this order and keeps the first that answers.
const GEOIP_CANDIDATES: [&str; 3] = [
    "https://cdn.jsdelivr.net/gh/Loyalsoldier/v2ray-rules-dat@release/geoip.dat",
    "https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/geoip.dat",
    "https://github.com/Loyalsoldier/v2ray-rules-dat/releases/latest/download/geoip.dat",
];
const GEOSITE_CANDIDATES: [&str; 3] = [
    "https://cdn.jsdelivr.net/gh/Loyalsoldier/v2ray-rules-dat@release/geosite.dat",
    "https://raw.githubusercontent.com/Loyalsoldier/v2ray-rules-dat/release/geosite.dat",
    "https://github.com/Loyalsoldier/v2ray-rules-dat/releases/latest/download/geosite.dat",
];

/// Every place `name` can be fetched from, best first.
///
/// An unknown name has no locations at all — an empty list is the honest
/// answer, and it lets the caller skip the download loop instead of inventing
/// a URL for a file we do not know.
pub fn candidates(name: &str) -> Vec<&'static str> {
    match name {
        "geoip.dat" => GEOIP_CANDIDATES.to_vec(),
        "geosite.dat" => GEOSITE_CANDIDATES.to_vec(),
        _ => Vec::new(),
    }
}

/// Both files, in the order a config is likely to need them.
pub const ASSET_NAMES: [&str; 2] = ["geoip.dat", "geosite.dat"];

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize, specta::Type)]
pub struct GeoAssetStatus {
    pub name: String,
    pub path: String,
    pub exists: bool,
    /// Numbers the UI reads are f64 in JavaScript; specta refuses to export
    /// u64/i64 because JSON cannot carry them without losing precision.
    pub bytes: f64,
    /// Seconds since the epoch, or `None` when the file is missing.
    pub modified: Option<f64>,
}

/// What is on disk right now. A missing file is a normal answer, not an error:
/// the caller wants to tell the user "还没有 geo 数据", not fail the page.
pub fn status(dir: &Path) -> Vec<GeoAssetStatus> {
    ASSET_NAMES
        .iter()
        .map(|name| {
            let path = dir.join(name);
            let meta = std::fs::metadata(&path).ok();
            GeoAssetStatus {
                name: (*name).to_string(),
                path: path.display().to_string(),
                exists: meta.is_some(),
                bytes: meta.as_ref().map(|m| m.len() as f64).unwrap_or(0.0),
                modified: meta
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as f64),
            }
        })
        .collect()
}

/// Where the kernels should look for these files.
///
/// Xray reads `XRAY_LOCATION_ASSET` and otherwise searches its working
/// directory; passing the variable at spawn time makes the engines directory
/// authoritative instead of whatever directory the app happened to start in.
pub fn asset_dir(engines_dir: &Path) -> PathBuf {
    engines_dir.to_path_buf()
}

/// Replace one geo file without ever leaving a half-written one behind.
///
/// The download lands in `<name>.tmp` next to the target, is flushed to disk,
/// and only then renamed over `<name>`; a crash mid-download therefore leaves
/// the previous file intact. On any error the temp file is removed.
pub fn install(dir: &Path, name: &str, data: &[u8]) -> AppResult<()> {
    if !ASSET_NAMES.contains(&name) {
        return Err(AppError::InvalidInput(format!("未知的 geo 数据文件：{name}")));
    }
    std::fs::create_dir_all(dir)?;
    let target = dir.join(name);
    let temp = dir.join(format!("{name}.tmp"));

    let written = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(data)?;
        file.sync_all()
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(AppError::Io(format!(
            "写入 {} 失败：{error}",
            target.display()
        )));
    }

    if let Err(error) = std::fs::rename(&temp, &target) {
        let _ = std::fs::remove_file(&temp);
        return Err(AppError::Io(format!(
            "写入 {} 失败：{error}",
            target.display()
        )));
    }
    Ok(())
}

/// Human-readable size, because "9418240 字节" tells a user nothing.
pub fn human_bytes(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if bytes as f64 >= MB {
        format!("{:.1} MB", bytes as f64 / MB)
    } else {
        format!("{} KB", bytes / 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        // Under `target/`, not the system temp dir: the sandbox this test runs
        // in refuses writes to %TEMP%, and a test that cannot create its own
        // directory reports a permission error instead of what it asserts.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("geo-tests");
        std::fs::create_dir_all(&root).unwrap();
        let dir = root.join(format!(
            "xm-geo-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_assets_are_reported_not_treated_as_errors() {
        let dir = temp_dir("missing");
        let rows = status(&dir);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| !row.exists && row.bytes == 0.0));
        assert!(rows.iter().all(|row| row.modified.is_none()));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn an_installed_asset_reports_its_size_and_leaves_no_temp_file() {
        let dir = temp_dir("install");
        install(&dir, "geoip.dat", b"first").unwrap();
        let rows = status(&dir);
        let geoip = rows.iter().find(|row| row.name == "geoip.dat").unwrap();
        assert!(geoip.exists);
        assert_eq!(geoip.bytes, 5.0);
        assert!(geoip.modified.is_some());
        assert!(!dir.join("geoip.dat.tmp").exists());
        assert_eq!(std::fs::read(dir.join("geoip.dat")).unwrap(), b"first");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_failed_install_keeps_the_previous_file() {
        let dir = temp_dir("failed");
        install(&dir, "geosite.dat", b"good").unwrap();
        // A read-only target makes the final rename fail the way a file locked
        // by a running kernel would, and leaves the old bytes in place.
        let target = dir.join("geosite.dat");
        let mut perms = std::fs::metadata(&target).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&target, perms).unwrap();
        let error = install(&dir, "geosite.dat", b"new").unwrap_err();
        assert!(error.to_string().contains("geosite.dat"), "{error}");
        assert_eq!(std::fs::read(&target).unwrap(), b"good");
        assert!(!dir.join("geosite.dat.tmp").exists());
        let mut perms = std::fs::metadata(&target).unwrap().permissions();
        perms.set_readonly(false);
        std::fs::set_permissions(&target, perms).ok();
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn names_that_are_not_geo_assets_are_refused() {
        let dir = temp_dir("refuse");
        assert!(install(&dir, "config.json", b"{}").is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn sizes_read_the_way_a_person_would_say_them() {
        assert_eq!(human_bytes(900 * 1024), "900 KB");
        assert_eq!(human_bytes(9 * 1024 * 1024), "9.0 MB");
    }

    #[test]
    fn every_asset_has_three_locations_with_the_cdn_first_and_github_last() {
        for name in ASSET_NAMES {
            let urls = candidates(name);
            assert_eq!(urls.len(), 3, "{name} should have three sources");
            assert!(
                urls[0].contains("cdn.jsdelivr.net"),
                "{name} should try the CDN first, got {}",
                urls[0]
            );
            assert!(
                urls[2].contains("github.com/") && urls[2].contains("/releases/latest/download/"),
                "{name} should fall back to the GitHub release last, got {}",
                urls[2]
            );
            // The mirror and the release must name the same file, or the fallback
            // silently swaps in the wrong asset.
            assert!(urls.iter().all(|url| url.ends_with(name)));
        }
    }

    #[test]
    fn an_unknown_name_has_no_locations() {
        assert!(candidates("config.json").is_empty());
        assert!(candidates("").is_empty());
    }
}

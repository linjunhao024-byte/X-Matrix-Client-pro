//! Official kernel releases: which asset belongs to this machine, whether the
//! bytes match what upstream published, and how to put a new binary in place
//! without ever losing the old one.
//!
//! Everything here is pure: no network, no process, no filesystem beyond the
//! three staging helpers, which only ever touch the engines directory they are
//! handed. That is deliberate — this is the part of "update the kernel" that
//! can be wrong in ways that cost the user a working proxy, so it is the part
//! that has to be testable without any of it running.

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::engine::EngineKind;
use crate::error::{AppError, AppResult};

/// The GitHub API endpoint that names the newest release of each kernel.
///
/// `releases/latest` (not `releases`) on purpose: GitHub computes "latest" and
/// answers one object, so a draft or a pre-release never gets picked by
/// accident. Those tags ship binaries too, and "newest" must mean stable.
pub fn feed_url(kind: EngineKind) -> &'static str {
    match kind {
        EngineKind::Xray => "https://api.github.com/repos/XTLS/Xray-core/releases/latest",
        EngineKind::SingBox => "https://api.github.com/repos/SagerNet/sing-box/releases/latest",
    }
}

/// The asset this build can actually use: Windows, amd64.
///
/// Xray ships one fixed name; sing-box embeds the version in it. Neither answer
/// is guessed from a pattern match over all assets — the name is computed and
/// then looked up exactly, so a release that renamed or dropped it fails loudly
/// instead of downloading an arm64 build that would then fail the version probe.
fn asset_name(kind: EngineKind, version: &str) -> String {
    match kind {
        EngineKind::Xray => "Xray-windows-64.zip".to_string(),
        EngineKind::SingBox => format!("sing-box-{version}-windows-amd64.zip"),
    }
}

/// A release, reduced to the four things the update flow needs.
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    /// The tag exactly as upstream spells it (`v26.11.9`).
    pub tag: String,
    /// The tag without the `v`: what the version probe and the UI compare.
    pub version: String,
    pub url: String,
    /// The `.dgst` asset next to the zip, when upstream publishes one.
    pub digest_url: Option<String>,
    /// Size reported by the API; `f64` because the IPC layer cannot carry `u64`.
    pub bytes: f64,
}

#[derive(Debug, Deserialize)]
struct Release {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    #[serde(default)]
    name: String,
    #[serde(default)]
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Pick the Windows amd64 zip out of a `releases/latest` payload.
///
/// A missing asset is an error, not a fallback: an arm64 build would pass every
/// check up to the version probe (which needs to spawn it) and then fail on the
/// user's machine with an OS-level "not a valid Win32 application". Refusing up
/// front names the file it looked for, which is the only actionable thing in
/// that situation.
pub fn parse_release(kind: EngineKind, json: &str) -> AppResult<ReleaseInfo> {
    let release: Release = serde_json::from_str(json)
        .map_err(|error| AppError::Engine(format!("内核发布信息不是合法的 JSON：{error}")))?;
    let tag = release.tag_name.trim();
    if tag.is_empty() {
        return Err(AppError::Engine("内核发布信息里没有版本标签".into()));
    }
    let version = tag.trim_start_matches(['v', 'V']).to_string();
    let wanted = asset_name(kind, &version);
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name.eq_ignore_ascii_case(&wanted))
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "{} 的发布 {} 里没有 {}（找的是 Windows amd64 那一份，不回退到别的架构）",
                kind.as_str(),
                tag,
                wanted
            ))
        })?;
    if asset.browser_download_url.trim().is_empty() {
        return Err(AppError::Engine(format!(
            "{} 的发布 {} 里，{} 没有下载链接",
            kind.as_str(),
            tag,
            wanted
        )));
    }
    // The digest sits next to the zip under the same name plus `.dgst`. Looking
    // it up by name (rather than taking the first `.dgst`) keeps a release that
    // checksums several assets from handing us the wrong one.
    let digest_name = format!("{wanted}.dgst");
    let digest_url = release
        .assets
        .iter()
        .find(|asset| {
            asset.name.eq_ignore_ascii_case(&digest_name)
                && !asset.browser_download_url.trim().is_empty()
        })
        .map(|asset| asset.browser_download_url.trim().to_string());
    Ok(ReleaseInfo {
        tag: tag.to_string(),
        version,
        url: asset.browser_download_url.trim().to_string(),
        digest_url,
        bytes: asset.size as f64,
    })
}

/// The sha256 out of a `.dgst` file, or `None`.
///
/// Xray writes lines like `SHA2-256= 6f1c…` (the space after `=` is upstream's).
/// `None` means "there is no checksum here", which the caller must not turn into
/// a pass — a file that merely *looks* intact is not verified.
pub fn parse_digest(text: &str) -> Option<String> {
    for line in text.lines() {
        let Some((label, value)) = line.trim().split_once('=') else {
            continue;
        };
        if !label.trim().eq_ignore_ascii_case("SHA2-256") {
            continue;
        }
        let value = value.trim();
        if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Some(value.to_ascii_lowercase());
        }
    }
    None
}

/// Lowercase hex sha256 of the given bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        // Writing into a String cannot fail; `let _` keeps the result honest
        // without an unwrap that would need a comment.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Pull the first `major.minor.patch` out of a kernel's self-report line.
///
/// Neither kernel prints the bare version the release tag carries: Xray says
/// `Xray 26.11.9 (…)` and sing-box says `sing-box version 1.14.4`. Comparing the
/// whole line against `26.11.9` would report "update available" on every check,
/// forever. The caller falls back to the raw line when this finds nothing, so an
/// unexpected format is still shown as-is rather than swallowed.
pub fn version_from_self_report(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let first = index;
        let mut parts: Vec<&str> = Vec::with_capacity(3);
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        parts.push(&line[first..index]);
        while parts.len() < 3 {
            if index + 1 >= bytes.len() || bytes[index] != b'.' || !bytes[index + 1].is_ascii_digit() {
                break;
            }
            let part_start = index + 1;
            let mut end = part_start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            parts.push(&line[part_start..end]);
            index = end;
        }
        if parts.len() == 3 {
            return Some(parts.join("."));
        }
    }
    None
}

/// The largest executable we are willing to pull out of a zip.
///
/// Both kernels are well under 100 MB today; the ceiling is a zip-bomb guard,
/// not a size policy. A package that declares more is refused before a byte of
/// it is decompressed.
const MAX_EXE_BYTES: u64 = 150 * 1024 * 1024;

fn entry_basename(name: &str) -> Option<&str> {
    name.rsplit(['/', '\\']).find(|part| !part.is_empty())
}

/// An entry path that could escape the extraction root, if anything ever wrote
/// it to disk. Nothing here does — the bytes go to memory — but a package that
/// contains such a name is not one this app should trust for a kernel binary.
fn entry_is_safe(name: &str) -> bool {
    if name.is_empty() || name.contains('\0') {
        return false;
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return false;
    }
    // `C:\…` and `C:/…`.
    if name.len() >= 2 && name.as_bytes()[0].is_ascii_alphabetic() && name.as_bytes()[1] == b':' {
        return false;
    }
    name.split(['/', '\\']).all(|part| part != "..")
}

/// Take the kernel executable out of an official zip, by basename.
///
/// The match is case-insensitive and allowed in a subdirectory because the
/// packaging is upstream's business; the *name* is what makes the file the
/// kernel. Only one entry may match — two files claiming to be `xray.exe` is an
/// ambiguity worth refusing rather than resolving by archive order.
pub fn extract_exe(zip_bytes: &[u8], want: &str) -> AppResult<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes))
        .map_err(|error| AppError::Engine(format!("内核压缩包打不开：{error}")))?;
    // Pass one: refuse the whole package if any entry is unsafe, so a valid
    // `xray.exe` earlier in the archive cannot make a malicious entry moot.
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| AppError::Engine(format!("内核压缩包第 {index} 个条目读不出来：{error}")))?;
        let name = entry.name().to_string();
        if !entry_is_safe(&name) {
            return Err(AppError::Engine(format!(
                "内核压缩包里有一条不安全的路径：{name}"
            )));
        }
    }
    // Pass two: find it and read it.
    let mut found: Option<Vec<u8>> = None;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| AppError::Engine(format!("内核压缩包第 {index} 个条目读不出来：{error}")))?;
        let name = entry.name().to_string();
        let matches = entry_basename(&name).is_some_and(|base| base.eq_ignore_ascii_case(want));
        if !matches {
            continue;
        }
        if entry.is_dir() {
            continue;
        }
        if entry.size() > MAX_EXE_BYTES {
            return Err(AppError::Engine(format!(
                "压缩包里的 {name} 解出来有 {} MB，超过 150 MB，拒绝解压",
                entry.size() / (1024 * 1024)
            )));
        }
        let mut bytes = Vec::new();
        // Read one byte past the ceiling: the declared size can lie, and the
        // only trustworthy measure is what actually came out.
        entry
            .take(MAX_EXE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| AppError::Engine(format!("从压缩包里读 {name} 失败：{error}")))?;
        if bytes.len() as u64 > MAX_EXE_BYTES {
            return Err(AppError::Engine(format!(
                "压缩包里的 {name} 解出来超过 150 MB，拒绝解压"
            )));
        }
        if found.is_some() {
            return Err(AppError::Engine(format!(
                "内核压缩包里有多个 {want}，不知道该用哪一个"
            )));
        }
        found = Some(bytes);
    }
    found.ok_or_else(|| AppError::NotFound(format!("内核压缩包里没有 {want}")))
}

fn incoming_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!(".{name}.incoming"))
}

fn backup_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.old"))
}

/// Write the new binary next to the one it will replace.
///
/// It is a dot-file with `.incoming`: hidden from a casual look at `engines/`,
/// and never picked up by the resolver, which asks for `<name>` exactly. The
/// caller probes *this* file before it is allowed to become `<name>`.
pub fn stage(dir: &Path, name: &str, bytes: &[u8]) -> AppResult<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = incoming_path(dir, name);
    if let Err(error) = fs::write(&path, bytes) {
        // A half-written file that nobody will ever promote is worse than no
        // file: it would be the next update's surprise.
        let _ = fs::remove_file(&path);
        return Err(AppError::Io(format!(
            "写入新内核 {} 失败：{error}",
            path.display()
        )));
    }
    Ok(path)
}

/// Move the staged file into place, keeping the old one as `<name>.old`.
///
/// Returns whether something was replaced (`false` on a first install, where
/// there was no old file). The old file is moved, not deleted: if the new kernel
/// turns out not to work, the working binary is still on disk for the user to
/// put back by hand, and this function tries to restore it automatically when
/// the final rename itself fails.
pub fn promote(dir: &Path, name: &str) -> AppResult<bool> {
    let incoming = incoming_path(dir, name);
    let final_path = dir.join(name);
    let backup = backup_path(dir, name);
    if !incoming.is_file() {
        return Err(AppError::Io(format!(
            "没有待替换的内核文件：{}",
            incoming.display()
        )));
    }
    let replaced = final_path.is_file();
    if replaced {
        // One generation of backup is enough; a leftover from an older update
        // is not part of the history anyone reads.
        let _ = fs::remove_file(&backup);
        fs::rename(&final_path, &backup).map_err(|error| {
            AppError::Io(format!(
                "无法把旧内核挪到 {}：{error}",
                backup.display()
            ))
        })?;
    }
    if let Err(error) = fs::rename(&incoming, &final_path) {
        if replaced {
            let _ = fs::rename(&backup, &final_path);
        }
        return Err(AppError::Io(format!(
            "无法把新内核挪到 {}：{error}",
            final_path.display()
        )));
    }
    Ok(replaced)
}

/// Remove the staged file. Best effort: a leftover `.incoming` is not worth
/// turning a reported failure into a different failure.
pub fn discard(dir: &Path, name: &str) {
    let _ = fs::remove_file(incoming_path(dir, name));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xray_release() -> String {
        serde_json::json!({
            "tag_name": "v26.11.9",
            "assets": [
                {"name": "Xray-windows-arm64-v8a.zip", "browser_download_url": "https://example.test/arm64.zip", "size": 15000000},
                {"name": "Xray-windows-64.zip", "browser_download_url": "https://example.test/xray.zip", "size": 16700000},
                {"name": "Xray-windows-64.zip.dgst", "browser_download_url": "https://example.test/xray.zip.dgst", "size": 299},
                {"name": "Xray-linux-64.zip", "browser_download_url": "https://example.test/linux.zip", "size": 16000000}
            ]
        })
        .to_string()
    }

    fn singbox_release() -> String {
        serde_json::json!({
            "tag_name": "v1.14.4",
            "assets": [
                {"name": "sing-box-1.14.4-windows-arm64.zip", "browser_download_url": "https://example.test/arm64.zip", "size": 32000000},
                {"name": "sing-box-1.14.4-windows-amd64.zip", "browser_download_url": "https://example.test/amd64.zip", "size": 32900000}
            ]
        })
        .to_string()
    }

    #[test]
    fn parse_release_picks_the_windows_amd64_asset_and_strips_the_v() {
        let info = parse_release(EngineKind::Xray, &xray_release()).expect("xray release parses");
        assert_eq!(info.tag, "v26.11.9");
        assert_eq!(info.version, "26.11.9");
        assert_eq!(info.url, "https://example.test/xray.zip");
        assert_eq!(info.digest_url.as_deref(), Some("https://example.test/xray.zip.dgst"));
        assert_eq!(info.bytes, 16_700_000.0);
    }

    #[test]
    fn parse_release_builds_the_versioned_singbox_asset_name() {
        let info = parse_release(EngineKind::SingBox, &singbox_release()).expect("sing-box release parses");
        assert_eq!(info.version, "1.14.4");
        assert_eq!(info.url, "https://example.test/amd64.zip");
        // Upstream publishes no digest for sing-box; nothing may invent one.
        assert_eq!(info.digest_url, None);
    }

    #[test]
    fn parse_release_says_which_file_it_looked_for() {
        let json = serde_json::json!({
            "tag_name": "v26.11.9",
            "assets": [
                {"name": "Xray-windows-arm64-v8a.zip", "browser_download_url": "https://example.test/arm64.zip"}
            ]
        })
        .to_string();
        let error = parse_release(EngineKind::Xray, &json).expect_err("arm64 only must not parse");
        assert!(
            error.to_string().contains("Xray-windows-64.zip"),
            "error must name the expected asset: {error}"
        );
        assert!(!error.to_string().contains("arm64"), "must not silently fall back: {error}");
    }

    #[test]
    fn parse_digest_accepts_leading_space_uppercase_and_padding() {
        let text = "SHA2-256= 6F1C2B3A4D5E6F708192A3B4C5D6E7F8091A2B3C4D5E6F708192A3B4C5D6E7F8\n";
        assert_eq!(
            parse_digest(text).as_deref(),
            Some("6f1c2b3a4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f8")
        );
        // A different algorithm line is not the sha256.
        assert_eq!(parse_digest("SHA1= 6f1c\nMD5= abcd\n"), None);
        // A truncated value is not a checksum either.
        assert_eq!(parse_digest("SHA2-256= 6f1c2b3a\n"), None);
        assert_eq!(parse_digest(""), None);
    }

    #[test]
    fn sha256_hex_matches_the_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;

        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file(*name, options).expect("zip entry starts");
            writer.write_all(bytes).expect("zip entry writes");
        }
        writer.finish().expect("zip finishes").into_inner()
    }

    #[test]
    fn extract_exe_finds_the_binary_beside_licences_and_in_a_subdirectory() {
        let zip = zip_with(&[
            ("LICENSE", b"MIT"),
            ("README.md", b"docs"),
            ("bin/xray.exe", b"MZ fake kernel"),
        ]);
        assert_eq!(extract_exe(&zip, "xray.exe").expect("extracts"), b"MZ fake kernel");
        // Basename matching is case-insensitive: a package that changed the
        // spelling is still the kernel.
        assert_eq!(extract_exe(&zip, "XRAY.EXE").expect("extracts"), b"MZ fake kernel");
    }

    #[test]
    fn extract_exe_refuses_a_package_without_the_binary() {
        let zip = zip_with(&[("LICENSE", b"MIT"), ("sing-box.exe", b"MZ")]);
        let error = extract_exe(&zip, "xray.exe").expect_err("no xray.exe must fail");
        assert!(
            error.to_string().contains("xray.exe"),
            "error must name the missing file: {error}"
        );
    }

    #[test]
    fn extract_exe_refuses_entries_that_escape_the_root() {
        let zip = zip_with(&[("../../xray.exe", b"MZ evil")]);
        let error = extract_exe(&zip, "xray.exe").expect_err("unsafe path must fail");
        assert!(
            error.to_string().contains("不安全"),
            "error must say the path is unsafe: {error}"
        );
    }

    /// Scratch space under the crate's own `target/`, following the other
    /// suites: this sandbox refuses writes to the system temp directory, and a
    /// test that cannot create its directory reports a permission error instead
    /// of the assertion it was written for.
    fn temp_dir(tag: &str) -> PathBuf {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("release-{tag}-{}-{seq}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir is creatable");
        dir
    }

    #[test]
    fn stage_promote_keeps_the_previous_binary_as_old() {
        let dir = temp_dir("promote");
        fs::write(dir.join("xray.exe"), b"old kernel").expect("old binary");
        let incoming = stage(&dir, "xray.exe", b"new kernel").expect("staged");
        assert_eq!(incoming, dir.join(".xray.exe.incoming"));
        assert_eq!(fs::read(&incoming).expect("incoming readable"), b"new kernel");
        // Staging alone must not disturb the binary the app is running.
        assert_eq!(fs::read(dir.join("xray.exe")).expect("old readable"), b"old kernel");

        assert!(promote(&dir, "xray.exe").expect("promoted"));
        assert_eq!(fs::read(dir.join("xray.exe")).expect("new readable"), b"new kernel");
        assert_eq!(fs::read(dir.join("xray.exe.old")).expect("old readable"), b"old kernel");
        assert!(!incoming.exists(), "incoming must be consumed by promote");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn promote_reports_a_first_install_as_not_replaced() {
        let dir = temp_dir("first");
        stage(&dir, "sing-box.exe", b"first kernel").expect("staged");
        assert!(!promote(&dir, "sing-box.exe").expect("promoted"));
        assert_eq!(fs::read(dir.join("sing-box.exe")).expect("readable"), b"first kernel");
        assert!(!dir.join("sing-box.exe.old").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discard_leaves_nothing_behind() {
        let dir = temp_dir("discard");
        let incoming = stage(&dir, "xray.exe", b"half a download").expect("staged");
        discard(&dir, "xray.exe");
        assert!(!incoming.exists());
        assert_eq!(
            fs::read_dir(&dir).expect("dir readable").count(),
            0,
            "discard must not leave a trace"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn promote_without_a_staged_file_fails_loudly() {
        let dir = temp_dir("missing-incoming");
        let error = promote(&dir, "xray.exe").expect_err("nothing to promote");
        assert!(error.to_string().contains(".xray.exe.incoming"), "{error}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn version_from_self_report_extracts_three_numbers() {
        assert_eq!(
            version_from_self_report("Xray 26.11.9 (Xray, Penetrates Everything.) Custom (go1.24.1 windows/amd64)"),
            Some("26.11.9".to_string())
        );
        assert_eq!(
            version_from_self_report("sing-box version 1.14.4"),
            Some("1.14.4".to_string())
        );
        assert_eq!(version_from_self_report("no version here"), None);
        assert_eq!(version_from_self_report(""), None);
    }
}
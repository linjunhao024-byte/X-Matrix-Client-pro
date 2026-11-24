//! Running processes, merged by executable name.
//!
//! A routing rule matches a process *name* (`chrome.exe`), never a pid — the
//! same executable can hold a dozen pids at once, and twelve identical rows in
//! a picker is noise. [`group`] is the pure half that merges them; [`list`] is
//! the thin OS half.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProcessInfo {
    /// Executable name without a path, e.g. `chrome.exe`.
    pub name: String,
    /// Every pid currently running under that name, ascending.
    pub pids: Vec<u32>,
}

/// Merge `(pid, name)` samples into one row per executable.
///
/// Names merge case-insensitively (`Chrome.EXE` is the same thing as
/// `chrome.exe`) but the spelling first seen is kept for display, because that
/// is what the user recognises in Task Manager. Pids are sorted and
/// deduplicated; rows are ordered by name, with the original spelling as the
/// tie-breaker so the order is stable rather than dependent on hash order.
pub fn group(samples: Vec<(u32, String)>) -> Vec<ProcessInfo> {
    let mut merged: std::collections::HashMap<String, ProcessInfo> =
        std::collections::HashMap::new();

    for (pid, name) in samples {
        // The snapshot can report the Idle process with a blank name; an empty
        // row would be unpickable, so it never reaches the table.
        if name.is_empty() {
            continue;
        }
        let key = name.to_lowercase();
        merged
            .entry(key)
            .or_insert_with(|| ProcessInfo {
                name,
                pids: Vec::new(),
            })
            .pids
            .push(pid);
    }

    let mut rows: Vec<ProcessInfo> = merged.into_values().collect();
    for row in &mut rows {
        row.pids.sort_unstable();
        row.pids.dedup();
    }
    rows.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    rows
}

/// Every running process, one row per executable name.
pub fn list() -> AppResult<Vec<ProcessInfo>> {
    #[cfg(windows)]
    {
        Ok(group(windows_impl::samples()?))
    }
    #[cfg(not(windows))]
    {
        Err(crate::error::AppError::Internal(
            "进程列表当前只有 Windows 实现".to_string(),
        ))
    }
}

#[cfg(windows)]
mod windows_impl {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE, HANDLE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    use crate::error::{AppError, AppResult};

    /// Owns the snapshot handle. Closing it is a `Drop`, not a line every exit
    /// path has to remember: an early return still closes it.
    struct Snapshot(HANDLE);

    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    /// One `(pid, name)` per running process.
    ///
    /// A failed snapshot is an error, never an empty list: the picker would
    /// read an empty list as "this machine has no processes", which is a lie
    /// no user can act on.
    pub fn samples() -> AppResult<Vec<(u32, String)>> {
        let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if handle == INVALID_HANDLE_VALUE {
            return Err(AppError::Io(format!(
                "读取进程快照失败：{}",
                std::io::Error::last_os_error()
            )));
        }
        let snapshot = Snapshot(handle);

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut samples = Vec::new();
        let mut ok = unsafe { Process32FirstW(snapshot.0, &mut entry) };
        while ok != 0 {
            let name = utf16_name(&entry.szExeFile);
            // pid 0 is System Idle: no executable, and a rule pointing at it
            // would match nothing.
            if entry.th32ProcessID != 0 && !name.is_empty() {
                samples.push((entry.th32ProcessID, name));
            }
            ok = unsafe { Process32NextW(snapshot.0, &mut entry) };
        }
        // The walk ends with ERROR_NO_MORE_FILES; that is the normal stop, not
        // a failure, so the loop's final return value is deliberately ignored.
        Ok(samples)
    }

    /// A NUL-terminated UTF-16 name in a fixed 260-unit buffer.
    fn utf16_name(raw: &[u16; 260]) -> String {
        let end = raw.iter().position(|&unit| unit == 0).unwrap_or(raw.len());
        String::from_utf16_lossy(&raw[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(rows: &[ProcessInfo]) -> Vec<&str> {
        rows.iter().map(|row| row.name.as_str()).collect()
    }

    #[test]
    fn merges_same_name_and_collects_pids_in_order() {
        let rows = group(vec![
            (120, "chrome.exe".to_string()),
            (88, "chrome.exe".to_string()),
            (200, "discord.exe".to_string()),
            (88, "chrome.exe".to_string()),
        ]);

        assert_eq!(names(&rows), vec!["chrome.exe", "discord.exe"]);
        // Ascending and deduplicated: the same pid reported twice is still
        // one running process.
        assert_eq!(rows[0].pids, vec![88, 120]);
        assert_eq!(rows[1].pids, vec![200]);
    }

    #[test]
    fn merges_case_insensitively_keeping_the_first_spelling() {
        let rows = group(vec![
            (10, "Chrome.EXE".to_string()),
            (20, "chrome.exe".to_string()),
            (30, "Zoom.exe".to_string()),
            (40, "app.exe".to_string()),
        ]);

        // One Chrome row, named the way it first appeared.
        let chrome: Vec<&ProcessInfo> = rows
            .iter()
            .filter(|row| row.name.eq_ignore_ascii_case("chrome.exe"))
            .collect();
        assert_eq!(chrome.len(), 1);
        assert_eq!(chrome[0].name, "Chrome.EXE");
        assert_eq!(chrome[0].pids, vec![10, 20]);
        // Sorted by name without regard to case: "app.exe" < "Zoom.exe".
        assert_eq!(names(&rows), vec!["app.exe", "Chrome.EXE", "Zoom.exe"]);
    }

    #[test]
    fn empty_input_is_an_empty_table() {
        assert!(group(Vec::new()).is_empty());
        // A blank name is dropped rather than becoming an unpickable row.
        assert!(group(vec![(0, String::new())]).is_empty());
    }
}
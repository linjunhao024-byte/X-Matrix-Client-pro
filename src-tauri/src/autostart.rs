//! Launch at login.
//!
//! A proxy client that is not running when the machine boots does nothing, so
//! this is a real feature rather than a decorative switch. On Windows it is a
//! single value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//!
//! The value is a command line, so the executable path is quoted: install
//! locations routinely contain spaces, and an unquoted path silently launches
//! the wrong thing (or nothing).

use std::path::Path;

use crate::error::{AppError, AppResult};

/// The registry value name. Also the name the user sees in Task Manager's
/// startup tab, so it is the product name, not a technical id.
pub const VALUE_NAME: &str = "X-Matrix";

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Reading and writing the launch-at-login registration.
pub trait AutoStart {
    fn enabled(&self) -> AppResult<bool>;
    fn set(&self, enabled: bool) -> AppResult<()>;
}

/// The command line to register for `exe`.
pub fn command_line(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

#[cfg(windows)]
pub struct WindowsAutoStart;

#[cfg(windows)]
impl AutoStart for WindowsAutoStart {
    fn enabled(&self) -> AppResult<bool> {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(RUN_KEY)
            .map_err(|error| AppError::Io(error.to_string()))?;
        match key.get_value::<String, _>(VALUE_NAME) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(AppError::Io(error.to_string())),
        }
    }

    fn set(&self, enabled: bool) -> AppResult<()> {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
        use winreg::RegKey;

        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
            .map_err(|error| AppError::Io(error.to_string()))?;

        if enabled {
            let exe = std::env::current_exe()
                .map_err(|error| AppError::Io(format!("拿不到程序路径：{error}")))?;
            key.set_value(VALUE_NAME, &command_line(&exe))
                .map_err(|error| AppError::Io(error.to_string()))?;
        } else if key.get_value::<String, _>(VALUE_NAME).is_ok() {
            // Only delete what we wrote: a missing value is not an error.
            key.delete_value(VALUE_NAME)
                .map_err(|error| AppError::Io(error.to_string()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Fake {
        present: std::sync::Mutex<bool>,
    }

    impl AutoStart for Fake {
        fn enabled(&self) -> AppResult<bool> {
            Ok(*self.present.lock().unwrap())
        }

        fn set(&self, enabled: bool) -> AppResult<()> {
            *self.present.lock().unwrap() = enabled;
            Ok(())
        }
    }

    #[test]
    fn the_registered_command_is_quoted() {
        let line = command_line(&PathBuf::from(r"C:\Program Files\X-Matrix\x-matrix.exe"));
        assert_eq!(line, r#""C:\Program Files\X-Matrix\x-matrix.exe""#);
    }

    #[test]
    fn enabling_then_disabling_is_reversible() {
        let fake = Fake { present: std::sync::Mutex::new(false) };
        assert!(!fake.enabled().unwrap());
        fake.set(true).unwrap();
        assert!(fake.enabled().unwrap());
        fake.set(false).unwrap();
        assert!(!fake.enabled().unwrap());
    }
}

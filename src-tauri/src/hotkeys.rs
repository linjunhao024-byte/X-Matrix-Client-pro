//! Global chords: start/stop the kernel, and bring the window back, from
//! anywhere on the desktop.
//!
//! This owns the window and the message loop itself. The plugin route was
//! tried first and registered both chords successfully — `RegisterHotKey`
//! answered with 1409 (ERROR_HOTKEY_ALREADY_REGISTERED) for anyone else — yet
//! no `WM_HOTKEY` ever reached the app, because the hidden window ended up on
//! a thread that does not pump messages. Owning a thread whose only job is
//! `GetMessageW` removes that failure mode, and the whole mechanism is then
//! small enough to read in one screen.
//!
//! Registration stays best effort: another client may already own a chord, and
//! an app that refuses to start over a shortcut would be worse than one
//! without shortcuts.

use std::sync::OnceLock;

/// Which chord fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chord {
    /// Ctrl+Shift+X — start or stop the tunnel.
    Toggle,
    /// Ctrl+Shift+W — show the window, or get it out of the way.
    Window,
}

impl Chord {
    /// The id handed to `RegisterHotKey`, and the `wParam` it comes back as.
    pub fn id(self) -> i32 {
        match self {
            Chord::Toggle => 1,
            Chord::Window => 2,
        }
    }

    pub fn from_id(id: usize) -> Option<Chord> {
        match id {
            1 => Some(Chord::Toggle),
            2 => Some(Chord::Window),
            _ => None,
        }
    }

    /// How the chord is shown to the user, in one spelling everywhere.
    pub fn label(self) -> &'static str {
        match self {
            Chord::Toggle => "Ctrl+Shift+X",
            Chord::Window => "Ctrl+Shift+W",
        }
    }

    /// Modifier flags for `RegisterHotKey`: Windows spells control 0x2 and
    /// shift 0x4, which is not the order the bit names suggest.
    fn modifiers(self) -> u32 {
        const MOD_CONTROL: u32 = 0x2;
        const MOD_SHIFT: u32 = 0x4;

        MOD_CONTROL | MOD_SHIFT
    }

    /// Virtual-key code of the letter the chord uses.
    fn virtual_key(self) -> u32 {
        match self {
            Chord::Toggle => b'X' as u32,
            Chord::Window => b'W' as u32,
        }
    }
}

/// Both chords, in the order the settings page lists them.
pub const CHORDS: [Chord; 2] = [Chord::Toggle, Chord::Window];

type Callback = Box<dyn Fn(Chord) + Send + Sync>;

/// Filled once, before the message thread starts.
static CALLBACK: OnceLock<Callback> = OnceLock::new();

/// Registers both chords on a thread that does nothing but pump their
/// messages, and calls `callback` when one fires.
///
/// Returns once the attempt is over: a chord another program already owns is
/// reported on stderr and skipped, never fatal.
pub fn install(callback: impl Fn(Chord) + Send + Sync + 'static) {
    if CALLBACK.set(Box::new(callback)).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("hotkeys".to_string())
        .spawn(|| unsafe { windows_impl::run() });
    if let Err(error) = spawned {
        eprintln!("x-matrix: 全局快捷键线程启动失败：{error}");
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::{CALLBACK, CHORDS, Chord};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::RegisterHotKey;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, RegisterClassW,
        TranslateMessage, WM_HOTKEY, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
    };

    const CLASS_NAME: &str = "XMatrixHotkeys";

    /// The chord id Windows reports for a press, or `None` for a message that
    /// belongs to somebody else.
    pub(super) fn chord_for(id: usize) -> Option<Chord> {
        Chord::from_id(id)
    }

    pub(super) unsafe fn run() {
        let class: Vec<u16> = CLASS_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };

        let window_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        unsafe { RegisterClassW(&window_class) };

        // Never shown, never in the taskbar: it exists only to receive
        // WM_HOTKEY, which needs a window (or a thread queue) to arrive at.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };
        if hwnd.is_null() {
            eprintln!("x-matrix: 全局快捷键窗口创建失败，快捷键不可用");
            return;
        }

        for chord in CHORDS {
            let registered = unsafe {
                RegisterHotKey(hwnd, chord.id(), chord.modifiers(), chord.virtual_key())
            };
            if registered == 0 {
                eprintln!(
                    "x-matrix: 全局快捷键 {} 注册失败（可能已被别的程序占用）",
                    chord.label()
                );
            } else {
                eprintln!("x-matrix: 全局快捷键 {} 已就绪", chord.label());
            }
        }

        // A blocking `GetMessageW` is the point: this thread must be idle in
        // the queue, not polling, or Windows has nowhere to post the chord.
        let mut message: MSG = unsafe { std::mem::zeroed() };
        loop {
            let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
            if result <= 0 {
                break;
            }
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_HOTKEY {
            if let Some(chord) = chord_for(wparam) {
                if let Some(callback) = CALLBACK.get() {
                    callback(chord);
                }
            }
            return 0;
        }
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }
}

#[cfg(not(windows))]
pub fn install(_callback: impl Fn(Chord) + Send + Sync + 'static) {
    // The app is Windows-only; other targets keep the symbol so tests compile.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_ids_round_trip() {
        for chord in CHORDS {
            assert_eq!(Chord::from_id(chord.id() as usize), Some(chord));
        }
        assert_eq!(Chord::from_id(0), None);
        assert_eq!(Chord::from_id(99), None);
    }

    #[test]
    fn both_chords_are_ctrl_shift_and_distinct() {
        assert_eq!(CHORDS[0].modifiers(), 0x6);
        assert_eq!(CHORDS[0].modifiers(), CHORDS[1].modifiers());
        assert_ne!(CHORDS[0].virtual_key(), CHORDS[1].virtual_key());
        assert_eq!(Chord::Toggle.virtual_key(), 0x58);
        assert_eq!(Chord::Window.virtual_key(), 0x57);
    }

    #[test]
    fn labels_match_the_settings_page() {
        assert_eq!(Chord::Toggle.label(), "Ctrl+Shift+X");
        assert_eq!(Chord::Window.label(), "Ctrl+Shift+W");
    }
}

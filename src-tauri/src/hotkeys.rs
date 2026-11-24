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
//! without shortcuts. Rebinding is the same: the save always succeeds, the
//! combination is re-registered on the thread that pumps the messages, and a
//! refusal is reported on stderr rather than turned into an error dialog.

use std::sync::{Mutex, OnceLock};

/// Which action a chord is bound to. The *combination* is a setting; this is
/// the identity that survives a rebind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chord {
    /// Start or stop the tunnel.
    Toggle,
    /// Show the window, or get it out of the way.
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

    /// Position in `BINDINGS` — `Toggle` first, the order the settings page
    /// lists them.
    fn slot(self) -> usize {
        match self {
            Chord::Toggle => 0,
            Chord::Window => 1,
        }
    }
}

/// Both chords, in the order the settings page lists them.
pub const CHORDS: [Chord; 2] = [Chord::Toggle, Chord::Window];

/// What the settings page starts with, and what "恢复默认" puts back.
pub const DEFAULT_TOGGLE: &str = "Ctrl+Shift+X";
pub const DEFAULT_WINDOW: &str = "Ctrl+Shift+W";

/// Windows modifier bits, named as `RegisterHotKey` wants them — control is
/// 0x2 and shift is 0x4, which is not the order the bit names suggest.
pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;
/// Holding the chord down must not fire it fifty times a second.
pub const MOD_NOREPEAT: u32 = 0x4000;

/// One key combination: a set of modifiers plus a virtual-key code.
///
/// Parsed from the text the user types (`Ctrl+Alt+T`), stored as text in the
/// settings, and turned into `RegisterHotKey` arguments at the one place that
/// needs them. Canonical spelling comes out of [`Combo::label`], so the same
/// chord never appears two ways in the UI and the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combo {
    pub modifiers: u32,
    pub vk: u32,
}

impl Combo {
    /// Parse `Ctrl+Alt+T` and friends.
    ///
    /// Accepted modifiers are `Ctrl`/`Control`, `Alt`, `Shift` and
    /// `Win`/`Meta`/`Super`, in any order and any case; the last token is the
    /// key. Letters, digits, `F1`–`F24` and a small set of named keys are
    /// understood. Anything else is refused with the offending token named,
    /// because a silently ignored modifier is a shortcut that does something
    /// other than what the user asked for.
    pub fn parse(raw: &str) -> Result<Combo, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err("快捷键不能是空的；留空即表示关掉这一项".to_string());
        }
        let mut modifiers = 0u32;
        let mut key: Option<u32> = None;
        let tokens: Vec<&str> = raw.split('+').map(str::trim).collect();
        for (index, token) in tokens.iter().enumerate() {
            if token.is_empty() {
                return Err(format!("快捷键「{raw}」里有一个空的按键名"));
            }
            let lowered = token.to_ascii_lowercase();
            let is_last = index + 1 == tokens.len();
            let modifier = match lowered.as_str() {
                "ctrl" | "control" => Some(MOD_CONTROL),
                "alt" => Some(MOD_ALT),
                "shift" => Some(MOD_SHIFT),
                "win" | "meta" | "super" => Some(MOD_WIN),
                _ => None,
            };
            if let Some(bit) = modifier {
                if is_last {
                    return Err(format!(
                        "「{token}」是修饰键，后面还要跟一个真正的按键，例如 Ctrl+Shift+X"
                    ));
                }
                if modifiers & bit != 0 {
                    return Err(format!("「{token}」出现了两次"));
                }
                modifiers |= bit;
                continue;
            }
            if !is_last {
                return Err(format!(
                    "「{token}」不是修饰键；一个快捷键只能有一个真正的按键，而且它要放在最后"
                ));
            }
            key = Some(key_code(token)?);
        }
        let Some(vk) = key else {
            return Err(format!("快捷键「{raw}」里没有按键"));
        };
        Ok(Combo { modifiers, vk })
    }

    /// Canonical spelling: modifiers in a fixed order, the key uppercased.
    pub fn label(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.modifiers & MOD_CONTROL != 0 {
            parts.push("Ctrl");
        }
        if self.modifiers & MOD_ALT != 0 {
            parts.push("Alt");
        }
        if self.modifiers & MOD_SHIFT != 0 {
            parts.push("Shift");
        }
        if self.modifiers & MOD_WIN != 0 {
            parts.push("Win");
        }
        let key = key_name(self.vk);
        if parts.is_empty() {
            return key;
        }
        format!("{}+{}", parts.join("+"), key)
    }

    /// What actually goes to `RegisterHotKey`.
    pub fn register_flags(&self) -> u32 {
        self.modifiers | MOD_NOREPEAT
    }
}

/// Virtual-key code for one key token.
fn key_code(token: &str) -> Result<u32, String> {
    let upper = token.to_ascii_uppercase();
    if upper.len() == 1 {
        let byte = upper.as_bytes()[0];
        if byte.is_ascii_alphanumeric() {
            return Ok(byte as u32);
        }
    }
    if let Some(rest) = upper.strip_prefix('F') {
        if let Ok(number) = rest.parse::<u32>() {
            if (1..=24).contains(&number) {
                return Ok(0x70 + number - 1);
            }
        }
    }
    let code = match upper.as_str() {
        "SPACE" => 0x20,
        "ENTER" | "RETURN" => 0x0D,
        "TAB" => 0x09,
        "ESC" | "ESCAPE" => 0x1B,
        "BACKSPACE" => 0x08,
        "INSERT" => 0x2D,
        "DELETE" | "DEL" => 0x2E,
        "HOME" => 0x24,
        "END" => 0x23,
        "PAGEUP" => 0x21,
        "PAGEDOWN" => 0x22,
        "UP" => 0x26,
        "DOWN" => 0x28,
        "LEFT" => 0x25,
        "RIGHT" => 0x27,
        _ => {
            return Err(format!(
                "看不懂「{token}」这个按键；可以是 A–Z、0–9、F1–F24，或 Space/Enter/Tab/Esc/方向键"
            ))
        }
    };
    Ok(code)
}

/// Inverse of [`key_code`] for the keys we accept, used to spell the label.
fn key_name(vk: u32) -> String {
    if (0x41..=0x5A).contains(&vk) || (0x30..=0x39).contains(&vk) {
        if let Some(character) = char::from_u32(vk) {
            return character.to_string();
        }
    }
    if (0x70..=0x87).contains(&vk) {
        return format!("F{}", vk - 0x70 + 1);
    }
    let name = match vk {
        0x20 => "Space",
        0x0D => "Enter",
        0x09 => "Tab",
        0x1B => "Esc",
        0x08 => "Backspace",
        0x2D => "Insert",
        0x2E => "Delete",
        0x24 => "Home",
        0x23 => "End",
        0x21 => "PageUp",
        0x22 => "PageDown",
        0x26 => "Up",
        0x28 => "Down",
        0x25 => "Left",
        0x27 => "Right",
        other => return format!("VK{other:02X}"),
    };
    name.to_string()
}

/// Parse a setting the way the app uses it: empty means "switched off".
pub fn parse_optional(raw: &str) -> Result<Option<Combo>, String> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    Combo::parse(raw).map(Some)
}

type Callback = Box<dyn Fn(Chord) + Send + Sync>;

/// Filled once, before the message thread starts.
static CALLBACK: OnceLock<Callback> = OnceLock::new();

/// Current combinations, `None` for a switched-off action.
static BINDINGS: Mutex<[Option<Combo>; 2]> = Mutex::new([None, None]);

/// The hidden window, once it exists — needed to re-register from another thread.
static TARGET: OnceLock<isize> = OnceLock::new();

/// Message that asks the hotkey thread to unregister everything and register
/// whatever `BINDINGS` holds now. Delivered on the window's own thread, so the
/// registration always happens where `GetMessageW` runs.
const WM_APP_REREGISTER: u32 = 0x8000 + 1;

/// Registers the given chords on a thread that does nothing but pump their
/// messages, and calls `callback` when one fires.
///
/// Returns once the thread is up: a chord another program already owns is
/// reported on stderr and skipped, never fatal.
pub fn install(
    callback: impl Fn(Chord) + Send + Sync + 'static,
    toggle: Option<Combo>,
    window: Option<Combo>,
) {
    if CALLBACK.set(Box::new(callback)).is_err() {
        return;
    }
    store(toggle, window);
    let spawned = std::thread::Builder::new()
        .name("hotkeys".to_string())
        .spawn(|| unsafe { windows_impl::run() });
    if let Err(error) = spawned {
        eprintln!("x-matrix: 全局快捷键线程启动失败：{error}");
    }
}

/// Point the chords at new combinations, or switch one off with `None`.
///
/// Called after every successful save. Registration happens on the hotkey
/// thread; this function only stores the wish and wakes that thread.
pub fn apply(toggle: Option<Combo>, window: Option<Combo>) {
    store(toggle, window);
    #[cfg(windows)]
    {
        if let Some(hwnd) = TARGET.get() {
            unsafe {
                windows_impl::request_reregister(*hwnd);
            }
        }
    }
}

fn store(toggle: Option<Combo>, window: Option<Combo>) {
    if let Ok(mut bindings) = BINDINGS.lock() {
        bindings[Chord::Toggle.slot()] = toggle;
        bindings[Chord::Window.slot()] = window;
    }
}

/// Map the `wParam` of a `WM_HOTKEY` message to the chord it belongs to.
fn chord_for_message(id: usize) -> Option<Chord> {
    Chord::from_id(id)
}

#[cfg(windows)]
mod windows_impl {
    use super::{
        BINDINGS, CALLBACK, CHORDS, Chord, TARGET, WM_APP_REREGISTER, chord_for_message,
    };
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PostMessageW,
        RegisterClassW, TranslateMessage, WM_HOTKEY, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
    };

    const CLASS_NAME: &str = "XMatrixHotkeys";

    /// Ask the hotkey thread to re-read `BINDINGS` and register again.
    pub(super) unsafe fn request_reregister(hwnd: isize) {
        unsafe { PostMessageW(hwnd as HWND, WM_APP_REREGISTER, 0, 0) };
    }

    /// Unregister both ids, then register whatever is bound now.
    unsafe fn register_all(hwnd: HWND) {
        for chord in CHORDS {
            unsafe {
                UnregisterHotKey(hwnd, chord.id());
            }
        }
        // One snapshot taken under the lock, then registration happens without
        // it: `RegisterHotKey` is a system call that can be slow, and a save
        // waiting behind it would only add latency for no gain — the last
        // snapshot to arrive wins, which is the behaviour wanted anyway.
        let bindings = BINDINGS
            .lock()
            .map(|guard| *guard)
            .unwrap_or([None, None]);
        for chord in CHORDS {
            let Some(combo) = bindings[chord.slot()] else {
                eprintln!("x-matrix: 全局快捷键 {} 已关闭", chord_name(chord));
                continue;
            };
            let registered =
                unsafe { RegisterHotKey(hwnd, chord.id(), combo.register_flags(), combo.vk) };
            if registered == 0 {
                eprintln!(
                    "x-matrix: 全局快捷键 {} 注册失败（可能已被别的程序占用）",
                    combo.label()
                );
            } else {
                eprintln!("x-matrix: 全局快捷键 {} 已就绪", combo.label());
            }
        }
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
        let _ = TARGET.set(hwnd as isize);

        unsafe { register_all(hwnd) };

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
            if let Some(chord) = chord_for_message(wparam) {
                if let Some(callback) = CALLBACK.get() {
                    callback(chord);
                }
            }
            return 0;
        }
        if message == WM_APP_REREGISTER {
            // No lock is held here on purpose. `register_all` reads the pair
            // through `BINDINGS`, and locking the same mutex twice would
            // deadlock the only thread that can ever register a chord again —
            // which is exactly what happened once: after the first save the
            // thread hung and every later chord was dead until a restart.
            unsafe { register_all(hwnd) };
            return 0;
        }
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    fn chord_name(chord: Chord) -> &'static str {
        match chord {
            Chord::Toggle => "启动/停止",
            Chord::Window => "显示/隐藏窗口",
        }
    }
}

#[cfg(not(windows))]
pub fn install(
    _callback: impl Fn(Chord) + Send + Sync + 'static,
    _toggle: Option<Combo>,
    _window: Option<Combo>,
) {
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
    fn the_defaults_parse_and_keep_their_spelling() {
        assert_eq!(Combo::parse(DEFAULT_TOGGLE).unwrap().label(), "Ctrl+Shift+X");
        assert_eq!(Combo::parse(DEFAULT_WINDOW).unwrap().label(), "Ctrl+Shift+W");
    }

    #[test]
    fn spelling_is_forgiving_and_canonical_comes_out() {
        let cases = [
            ("ctrl+shift+x", "Ctrl+Shift+X"),
            ("SHIFT+CTRL+T", "Ctrl+Shift+T"),
            ("Control + Alt + F5", "Ctrl+Alt+F5"),
            ("meta+space", "Win+Space"),
            ("alt+1", "Alt+1"),
            ("ctrl+shift+pageup", "Ctrl+Shift+PageUp"),
        ];
        for (raw, expected) in cases {
            assert_eq!(Combo::parse(raw).unwrap().label(), expected, "raw: {raw}");
        }
    }

    #[test]
    fn modifiers_come_out_in_one_order() {
        let a = Combo::parse("Shift+Alt+Ctrl+W").unwrap();
        let b = Combo::parse("Ctrl+Alt+Shift+W").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.label(), b.label());
    }

    #[test]
    fn auto_repeat_is_always_suppressed() {
        let combo = Combo::parse(DEFAULT_TOGGLE).unwrap();
        assert_eq!(combo.register_flags(), MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT);
    }

    #[test]
    fn nonsense_is_refused_with_the_token_named() {
        let cases = [
            ("", "留空"),
            ("Ctrl+", "空的按键名"),
            ("Ctrl+Shift", "修饰键"),
            ("Ctrl+Shift+Hyper+X", "不是修饰键"),
            ("Ctrl+Shift+X+Y", "只能有一个真正的按键"),
            ("Ctrl+Ctrl+X", "出现了两次"),
            ("Ctrl+F25", "F1–F24"),
            ("Ctrl+Ü", "看不懂「Ü」"),
        ];
        for (raw, needle) in cases {
            let error = Combo::parse(raw).unwrap_err();
            assert!(
                error.contains(needle),
                "raw {raw:?} gave {error:?}, expected it to mention {needle:?}"
            );
        }
    }

    #[test]
    fn a_key_without_modifiers_is_allowed() {
        assert_eq!(Combo::parse("F9").unwrap().label(), "F9");
        assert_eq!(Combo::parse("F9").unwrap().modifiers, 0);
    }

    #[test]
    fn empty_text_means_switched_off() {
        assert_eq!(parse_optional("   "), Ok(None));
        assert_eq!(parse_optional(""), Ok(None));
        assert!(parse_optional(DEFAULT_TOGGLE).unwrap().is_some());
    }

    #[test]
    fn key_names_round_trip() {
        for raw in [
            "Ctrl+X",
            "Alt+0",
            "Shift+F12",
            "Win+Space",
            "Ctrl+Enter",
            "Ctrl+Tab",
            "Ctrl+Esc",
            "Ctrl+Delete",
            "Ctrl+Home",
            "Ctrl+End",
            "Alt+PageDown",
            "Ctrl+Left",
        ] {
            let combo = Combo::parse(raw).unwrap();
            let again = Combo::parse(&combo.label()).unwrap();
            assert_eq!(combo, again, "raw: {raw}");
        }
    }
}

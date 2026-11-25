//! 轻量模式：窗口收进托盘时，让界面那半边真的停下来。
//!
//! 这不是「让内核少用内存」——内核是独立进程，界面收起来与否和它无关。这里
//! 处理的是本进程自己的工作集：WebView 和后端在窗口不可见时仍然占着上一次
//! 刷新留下的内存，而用户此时的期待就是「它安静一点」。
//!
//! 关于内存目标级别（`ProcessMemoryUsageTargetLevel`）：
//!
//! 任务原本要求先调
//! `SetProcessInformation(GetCurrentProcess(), ProcessMemoryUsageTargetLevel, ...)`
//! 把目标级别压到 Low，再回收工作集。核实结果：`windows-sys` 0.60.2 导出了
//! `SetProcessInformation` 函数本身，但**没有**导出
//! `ProcessMemoryUsageTargetLevel` 这个 `PROCESS_INFORMATION_CLASS` 成员；
//! 本机 Windows SDK 10.0.26100 的 `processthreadsapi.h` 里同样没有它（该枚举
//! 只有 `ProcessReservedValue1`，注释是「原用于 ActivityThrottlePolicy」）。
//! 按任务书的约定，这里不手写 FFI 声明、也不去猜那个未公开的枚举数字——
//! 猜错时 API 会返回 FALSE，功能看似存在实则从未生效，那比明说没做更糟。
//!
//! 所以本模块做两件能核实的事，缺一不可：
//!
//! 1. `EmptyWorkingSet` 把**本进程**的工作集还给系统（约 30 MB 那部分）；
//! 2. 通过 WebView2 的 `ICoreWebView2_13::SetMemoryUsageTargetLevel(Low)` 让
//!    **界面子进程**（`msedgewebview2.exe`）也把内存目标降下来——界面真正的
//!    大头在那里，只做第 1 步摘掉的是零头。
//!
//! 两半都能恢复：`set_background(false)` 不回收（无事可做）而 `set_webview_memory
//! (…, false)` 把目标级别设回 Normal，所以「关」不是"假装恢复了"。

use crate::error::AppResult;

/// 把本进程的内存目标降到「低」，并立刻把工作集还一点给系统。
/// `enabled = false` 把目标恢复成「正常」。
///
/// 在本版本的绑定里只有工作集回收这一半（见模块文档）：`false` 不调
/// `EmptyWorkingSet`，也没有目标级别可恢复。
pub fn set_background(enabled: bool) -> AppResult<()> {
    #[cfg(windows)]
    {
        windows_impl(enabled)
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Err(crate::error::AppError::Internal(
            "只有 Windows 实现了轻量模式".to_string(),
        ))
    }
}

#[cfg(windows)]
fn windows_impl(enabled: bool) -> AppResult<()> {
    use windows_sys::Win32::System::ProcessStatus::EmptyWorkingSet;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    if !enabled {
        // 目标级别那条 API 在本版本绑定里没暴露，所以没有留下任何需要恢复的
        // 状态。这里返回 Ok 是如实回答「不用再回收」，不是吞掉一个失败。
        return Ok(());
    }
    // SAFETY: `GetCurrentProcess` 返回本进程的伪句柄（-1），不需要也不应当
    // 释放；`EmptyWorkingSet` 只读取这个句柄，不接管它的所有权。两个调用都
    // 在本进程上下文中执行，不涉及其他进程的内存。
    let ok = unsafe { EmptyWorkingSet(GetCurrentProcess()) };
    if ok == 0 {
        // 失败不是致命错误：调用方只打日志，窗口该隐藏照隐藏。把
        // `GetLastError` 的原文带出来，才是"为什么没生效"的唯一线索。
        return Err(crate::error::AppError::Io(format!(
            "回收工作集失败：{}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

/// 把界面那半边（WebView2）的内存目标压到「低」/恢复「正常」。
///
/// 这是本项目里真正能让界面少占内存的那一半：渲染与 JS 都跑在
/// `msedgewebview2.exe` 子进程里，`EmptyWorkingSet` 只回收本进程自己那 30 MB，
/// 而 WebView2 在窗口不可见时接受 `SetMemoryUsageTargetLevel(Low)` 后会自己
/// 把渲染进程的内存目标压下去。接口在 `ICoreWebView2_19` 上（`MemoryUsageTargetLevel` 这对方法是在
/// WebView2 1.0.2592 之后才挪到这个接口上的，旧版本会 `cast` 失败）。
///
/// `window.with_webview` 会把闭包丢到 UI 线程执行（本函数在任意线程都能调），
/// 闭包里拿不到返回值，所以失败只能打日志——调用方本来也只需要日志。
pub fn set_webview_memory(window: &tauri::WebviewWindow, low: bool) -> AppResult<()> {
    #[cfg(windows)]
    {
        webview_impl(window, low)
    }
    #[cfg(not(windows))]
    {
        let _ = (window, low);
        Err(crate::error::AppError::Internal(
            "只有 Windows 实现了轻量模式".to_string(),
        ))
    }
}

#[cfg(windows)]
fn webview_impl(window: &tauri::WebviewWindow, low: bool) -> AppResult<()> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    use windows_core::Interface;

    let level = if low {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
    } else {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
    };
    window
        .with_webview(move |webview| {
            let outcome = (|| -> windows_core::Result<()> {
                // SAFETY: 控制器由 Tauri 在 UI 线程上给出，接口本身是活的；`CoreWebView2`
                // 只取一个子接口，不接管所有权。
                let core = unsafe { webview.controller().CoreWebView2() }?;
                // 老 WebView2 运行时没有 1.0.1901 之后的接口，`cast` 会失败：
                // 那时候只能靠工作集那一半，日志里说明白。
                let modern: ICoreWebView2_19 = core.cast()?;
                // SAFETY: `modern` 是活着的 COM 接口指针，调用在取得它的同
                // 一个 UI 线程里进行；`SetMemoryUsageTargetLevel` 只写接口自身
                // 的状态，不接管任何指针所有权。
                unsafe { modern.SetMemoryUsageTargetLevel(level) }
            })();
            if let Err(error) = outcome {
                eprintln!("x-matrix: 设置界面内存目标失败：{error}");
            }
        })
        .map_err(|error| {
            crate::error::AppError::Internal(format!("设置界面内存目标失败：{error}"))
        })
}

#[cfg(test)]
mod tests {
    use super::set_background;

    /// 本机就是 Windows，所以这两条能跑出真实系统调用，而不是空转。
    /// 不断言内存数字：工作集大小由系统决定，断言它只是在测测试环境。
    #[test]
    fn turning_it_off_is_a_no_op_that_succeeds() {
        assert!(set_background(false).is_ok());
    }

    #[test]
    fn turning_it_on_then_off_succeeds() {
        assert!(set_background(true).is_ok());
        assert!(set_background(false).is_ok());
    }
}
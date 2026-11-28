# 实测问题记录 / Issues found in real use

> 用途：用户真人实测中发现的问题，按发现顺序登记。每条写清「现象 / 复现 / 根因 / 影响 / 修法 / 状态」。
> 状态只有三种：`未修`、`已修（待真机复核）`、`已修并复核`。
> 验收结论仍写在 `docs/ACCEPTANCE.md`；本文件只做问题台账，不替代验收章节。

---

## I-01 以管理员身份启动时，屏幕上闪 1–3 个黑窗口（cmd / PowerShell 样式）

- **报告**：2026-11-28，用户实测（第一条问题）。
- **现象**：双击（或以管理员身份运行）`X-Matrix.exe` 后，桌面/任务栏一闪而过 1～3 个黑色控制台窗口，随后主窗口正常出现。分辨不出里面是什么，但明显不是应用自己的界面。
- **复现**：启动应用即出现；数量随「这一次有哪些地方要读内核版本」变化，所以是 1～3 个而不是固定数。
- **根因**：应用是 GUI 子系统程序（没有自己的控制台）。而 Xray / sing-box 都是**控制台子系统**可执行文件；用 `std::process::Command` 启动它们时如果没有 `CREATE_NO_WINDOW`，Windows 会为子进程**新分配一个控制台**，于是一个黑窗口一闪。代码里两条路径漏了这个标志：
  - `src-tauri/src/engine/runtime.rs:1012` `Runtime::version()` —— 首页与设置页「内核版本」都靠它，两个内核各一次；
  - `src-tauri/src/commands/kernel.rs:134` `probe_version()` —— 内核安装时对暂存文件跑 `version`，再闪一次。
  - 对照：真正起内核的 `src-tauri/src/engine/runtime.rs:111`（`start()`）、`src-tauri/src/engine/temp.rs:135`（临时试跑）、`src-tauri/src/service.rs:727`（服务里起内核）、`src-tauri/src/service.rs:624`（`icacls`）**都带了** `CREATE_NO_WINDOW`，所以只有「读版本」这一路会闪。
- **影响**：不影响功能，但第一印象很差，像在偷偷起后台命令；而且以管理员身份运行时更容易被注意到（同一登录会话里窗口直接绘制在桌面上）。
- **修法**：两处 `Command::new(...)` 加 `creation_flags(CREATE_NO_WINDOW)`（做法与 `start()` 完全一致）；再抽一个 `crate::sysproc::hidden(cmd)` 之类的助手，避免以后新增调用点又漏。改完在云电脑上启动应用，用「黑窗口计数」复核：应当 0 个。
- **状态**：`未修`

---

## I-02 「从剪贴板导入」时左上角弹出浏览器样式的「允许 / 拒绝」通知

- **报告**：2026-11-28，用户实测（第二条问题）。
- **现象**：在导入对话框点「从剪贴板」，窗口左上角出现一个 Chromium 风格的权限气泡（允许 / 拒绝），像网页在申请读剪贴板；用户因此问「我们不是桌面程序吗，怎么是浏览器 HTML」。
- **复现**：`src/components/ImportDialog.vue:62` 的 `navigator.clipboard.readText()`，每次调用都会触发 WebView2 的剪贴板读权限询问（用户选「允许」后仍可能只对本次生效）。
- **根因**：Tauri 用系统自带的 **WebView2（Edge Chromium）** 渲染界面，前端就是网页；凡走浏览器 Web API 的权限（剪贴板读、通知、地理位置等），Chromium 会按网页那套流程弹权限气泡，这是 WebView2 的默认行为，不是应用自己画的界面。
  应用其实已经装了原生通道：`tauri-plugin-clipboard-manager`（`src-tauri/Cargo.toml:23`、`src-tauri/src/lib.rs:79` 注册、`package.json:23` 有 JS 包），但 UI 里读剪贴板还在用浏览器 API。
- **影响**：每次都多一步点击，且「允许/拒绝」会让人怀疑这是个网页壳；拒绝后导入直接失败（虽然代码有回退提示，要手动粘贴）。
- **修法**：`ImportDialog.vue` 改用 `@tauri-apps/plugin-clipboard-manager` 的 `readText()`；同一原因，`QrDialog.vue:59`、`AboutView.vue:128`、`LogsView.vue:121` 的 `navigator.clipboard.writeText()` 也一并换成插件的 `writeText()`（写通常不弹，但换掉可以统一行为）。若仍希望压掉所有 WebView2 权限气泡，可在窗口构建时加 `with_permission_handler` 显式拒绝（本应用没有需要浏览器权限的功能）。
- **状态**：`未修`

---

## 待登记

（用户后续实测发现的问题按 I-03、I-04 … 继续往下写。）
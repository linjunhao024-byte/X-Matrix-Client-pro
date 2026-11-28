# 重写计划：Avalonia UI + Rust 内核 sidecar

> 决策日期：2026-11-28。用户原话：「按照顶部候选表剩下的执行」之后，真人实测中提出
> 「不要 Tauri 形式，要 Clash 的 UI 设计质感」，并在给出两条路线后选择
> **「换原生框架重写（Avalonia / Qt）」**，进一步确认为 **Avalonia + Rust sidecar**。
>
> 本文件是这次转向的路线图与判据。它的地位低于 `docs/ACCEPTANCE.md`（行为真相源），
> 高于普通笔记：每次阶段推进都要在下面「进度」表里留一行。

---

## 0. 为什么会有这次转向（先把话说明白）

- 现状：Tauri 2（Rust 后端）+ Vue 3（前端）。**Clash Verge Rev 也是同一套**（Tauri + 前端框架），
  所以「Clash 的形式」并不等于「原生控件」；v2rayN 才是 .NET 原生控件那一支。
- 但用户的直观感受没错：现在的应用带着**浏览器的味道**——权限气泡、文本可拖选、右键出网页菜单、
  Ctrl+滚轮缩放、启动闪黑窗、内存偏高。这些是**我们没做的产品化**（`tauri.conf.json` 连
  `visible:false` 都没设、CSS 里没有一条 `user-select:none`），不是框架做不到。
- 结论：即便这些都能在 Tauri 里修掉，用户要的是**原生控件的质感**（菜单、滚动、焦点、托盘、
  字体渲染都由系统画）。这个诉求只有换 UI 层才能满足，所以按用户选择重写。

## 1. 架构决定

```
┌────────────────────────────┐        ┌──────────────────────────────┐
│ XMatrix.exe（Avalonia, C#）│  HTTP  │ x-matrix-core.exe（Rust）    │
│ 窗口/托盘/主题/i18n/视图    │ ─────► │ 内核进程、配置生成、系统代理、│
│ 只负责画界面与发命令        │ JSON   │ TUN、服务模式、WebDAV、多配置 │
└────────────────────────────┘        └──────────────────────────────┘
```

- **保留**：`src-tauri` 里除了 Tauri 接线以外的全部 Rust 逻辑（`engine/`、`sysproxy/`、
  `net/`、`service.rs`、`commands/` 里的业务体、539 个单测）。它们是这个项目最贵的部分。
- **重写**：所有用户可见层——11 个视图 + 14 个组件 + 主题 token + i18n 装配 + 托盘 + 单实例 +
  提权握手。设计语汇（颜色、圆角、间距、信息层级）从 `src/styles/tokens.css` 与现有页面
  逐条搬到 Avalonia 样式里，像素级对齐，不重新发明。
- **丢掉**：`tauri` / `tauri-plugin-*` 依赖、`src/` 整个 Vue 前端、`src/services/bindings.ts`。
- **传输**：回环 HTTP/1.1 + JSON（沿用 `src-tauri/src/net/http.rs` 的极简风格，服务端同样手写，
  不引入 axum/hyper）。端口由 core 启动时选一个空闲端口，写入
  `%APPDATA%\com.xmatrix.client\core.endpoint`（`{"port":…,"token":…,"pid":…}`）；
  UI 读这个文件连上去，请求带 `X-Core-Token`。文件权限仅当前用户可读。
- **谁拥有内核**：core。UI 退出不该杀内核（托盘常驻语义不变），core 常驻；UI 与 core 的
  生命周期解耦，关窗口 = 隐藏，退出 = 给 core 发 `shutdown`。
- **提权**：仍由 UI 层发起（`ShellExecuteEx runas` 拉起自己 + `--relaunch-wait=<pid>` 握手），
  但 core 若以非管理员运行，提权后需要重启 core（沿用现有 `elevate.rs` 的判据）。
- **服务模式**：`XMatrixKernel` 服务与作业目录协议原样保留，core 是唯一客户端。

## 2. 阶段与判据

| 阶段 | 目标 | 判据（可核对） | 预计 |
|---|---|---|---|
| P0 骨架 | core 暴露 5 条命令（状态、节点列表、启停内核、设置读写、版本） | Avalonia 壳能连上 core，节点页读出真数据并启停内核一次；`ProxyEnable` 变化正确 | 3–5 天 |
| P1 通道 | 98 条 IPC 全部映射成 HTTP 路由 + token 校验 + 错误码 | 一条脚本把 98 条逐个打通（含错误分支），Rust 侧 539 测试仍全绿 | 1 周 |
| P2 视图 | 11 个视图按依赖序迁移（节点→订阅→路由→策略组→连接/日志→配置→规则集→设置→关于→首页） | 每页与 Tauri 版截图并排比对：布局、字号、颜色、空态、错误态一致 | 3–4 周 |
| P3 系统面 | 托盘、单实例、热键、提权、服务模式、轻量模式、打包（单安装包带 core） | §56/§57/§60 的既有判据在 Avalonia 版重跑通过 | 1 周 |
| P4 验收 | 按 `docs/ACCEPTANCE.md` §49–§65 逐节在新 UI 重跑 | 每节新增「Avalonia 版」小节；旧 Tauri 版结论保留不删 | 1 周 |

**并行共存**：P0–P2 期间 Tauri 版必须保持可构建、可安装（用户还在用它实测）。
只有 P4 全部通过后才删 Vue 前端。

## 3. 边界与禁区

- 不在本机起内核、不写本机系统代理（沿用全局规则）；真机一律 cloudpc-test。
- 重写期间**不再往 Vue 里加新功能**；新需求一律进 Avalonia 版。
- Rust 侧的对外行为（配置生成、错误文案、命令语义）**不改**；只加一层 HTTP 映射。
- i18n：873 键 × 3 语言从 `scripts/i18n-pairs.json` 直接生成 C# 资源，不手抄。
- 设计：颜色/圆角/间距一律取 `src/styles/tokens.css` 的现值；`C:\Users\83611\DESIGN-SKILL.md`
  的门槛（不允许写死颜色、圆角 ≤ 20 等）在 Avalonia 样式里同样适用。

## 4. 风险

1. **观感回退风险**：原生控件默认样式平庸，若照搬 token 不做细节（阴影、层级、间距节奏），
   会比现在的 Vue 版更丑。对策：P2 每页必须并排比对截图，不达标不算完成。
2. **迁移期双份维护**：修 bug 要判断改哪一边。对策：bug 先修 Rust/核心层，UI 层两版各跟一次；
   Vue 版只做「不修就不能用」的修复。
3. **Rust 依赖 Tauri 的运行期**：`commands/` 里用了 `tauri::State`/`AppHandle`（事件、托盘、
   窗口）。对策：P0 就把业务体与 Tauri 接线分开（`commands` 只留薄壳，业务进 `core/`），
   这也是 P1 能一条条映射的前提。
4. **时间**：按上表 6–7 周；若中途用户改主意，P0/P1 的产出（core 通道）对 Tauri 版也仍有价值。

## 5. 进度

| 日期 | 阶段 | 做了什么 | 判据结果 |
|---|---|---|---|
| 2026-11-28 | 决策 | 用户选定 Avalonia + Rust sidecar；本文件建立 | — |
| 2026-11-28 | 记账 | 实测问题 I-01（闪黑窗）、I-02（剪贴板权限气泡）、I-03（回首页再闪两次）登记在 `docs/ISSUES.md` | — |
| 2026-11-28 | **P0 完成** | `x-matrix-core.exe --serve`：端点文件（tmp+rename）+ token 校验 + `/health` `/status` `/nodes` `/engine/start` `/engine/stop` `/shutdown`；Avalonia 壳（`desktop/XMatrix.Desktop`）四项导航 + 节点页读真数据 + 窗内启停内核；顺手挖出四个真缺陷（core 继承 `set_nonblocking`、`PostAsJsonAsync` 的 chunked 正文、`HttpClient` 连接复用、错误行被刷新抹掉） | §66 八条判据全过：`xray#2424` + `ProxyEnable=1 ProxyServer=127.0.0.1:7890` + `/status running=True`；停止后全复原。本机 `cargo test --lib` 564 / `npm run check` 136 || 2026-11-29 | **P1 批次 A 完成**（engine 22 + sysproxy 2） | 业务体搬进 `src-tauri/src/services/`，`commands/{engine,sysproxy}.rs` 只留薄壳；`core/endpoints.rs` 改表驱动 `ROUTES`（P0 六条 + 新 22 条）+ `NOT_ROUTED`；`every_ipc_command_has_a_route()` 对 `["engine","sysproxy"]` 严格断言 24 条 | 云电脑逐条打：`200=17 non-200=6`（6 条全是设计内错误分支），`engine_apply`/`engine_start` 起 `xray#3048`/`xray#6172` 且 `ProxyEnable=1 ProxyServer=127.0.0.1:7890`，`engine_stop` 复原；`cargo test --lib` **590** / vitest 136 / `bindings.ts` 未变 |

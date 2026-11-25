# 候选表：还剩什么（2026-11-25 核查）

> 用户 2026-11-25 指令：「按照顶部候选表剩下的执行」。
> 本文件是执行前的核查记录：先把「候选表」里哪些已经落地、哪些真的还没做，
> 用代码位置证明清楚，再按旧流程逐项执行。
> 口径：进度只认这份文件与 `docs/ACCEPTANCE.md`；不凭记忆、不凭旧审计的结论。

## 1. 两张「候选表」在哪

| 表 | 文件 | 现状 |
|---|---|---|
| 下一步候选（按性价比 / 风险排序） | `docs/HANDOVER.md` §6（第 373 行起） | 除「设计审计残余」一行外全部 `~~划掉~~` 并标「已完成」；那一行只是指向 `D:\tmp\design-audit-2.md` 的历史说明，**不是功能项** |
| 只读审计的差距清单 | `D:\tmp\parity-gaps.md`（2026-11-23，HEAD `535124b` 时代） | §1 已覆盖 31 条；§2.1 High value H1–H7；§2.2 Nice to have N1–N13；§2.3 Skip（S1–S11）已被用户解禁并全部做完 |

`docs/PARITY-PLAN.md` 里没有叫「候选」的表；它的 S1–S11 进度表已全为「已完成」。

## 2. 逐条核查（拿代码位置说话）

### 2.1 High value

| # | 项 | 状态 | 证据 |
|---|---|---|---|
| H1 | 换节点/换模式不断线 | 已做 | 前端 `setMode()` → `settings.save` + `engine.apply`（`src/views/HomeView.vue:292-300`），注释写明「restart on the same ports, proxy takeover stays up」 |
| H2 | 托盘当控制面（节点 / 模式 / 引擎子菜单） | 已做 | `src-tauri/src/tray.rs:177-207` 建 `nodes` / `modes` 子菜单（`CheckMenuItem` + `Submenu`），`lib.rs:491` 注释「Everything else is a node or a proxy mode picked from a submenu」，回调 `tray::on_row` |
| H3 | 真日志页（跟随 / 级别 / 清空 / 复制） | 已做 | `src/views/LogsView.vue`（`timer = setInterval(poll, POLL_MS)`，级别过滤与清空在页内） |
| H4 | Hysteria2 / TUIC 节点 | 已做 | `src-tauri/src/domain/node.rs` 的 `Protocol::Hysteria2 { password, obfs_password, up_mbps, down_mbps }`、`Protocol::Tuic { uuid, password, congestion_control, udp_relay_mode }`，注释写明 sing-box only、选 Xray 时报错而非静默丢节点 |
| H5 | 真延迟 / 组延迟 | 已做 | `src/views/NodesView.vue:624-660` 的 `fastestNode()`/`pickFastest()` + 组内成员延迟；H5(b) 无内核真实延迟见 §43 |
| H6 | 订阅走代理拉取 | 已做 | `settings.subscription_via_proxy` + `commands/subscription.rs` 的 `fetch_route()` |
| H7 | 导入 Clash 保留组与规则 | 已做 | `src-tauri/src/documents.rs:357-363`（组类型映射）与规则映射 |

### 2.2 Nice to have

| # | 项 | 状态 | 证据 |
|---|---|---|---|
| N1 | 关闭全部连接 / 进程列 | 已做 | `commands/engine.rs:640` `close_connections(runtime, ids: Vec<String>)`（空 = 全部），`engine/stats.rs:303` |
| N2 | 持久化流量历史 | 已做 | `commands/engine.rs:501` `traffic_history(log, minutes)`，已注册 `src-tauri/src/ipc.rs:64` |
| N3 | 节点二维码 | 已做 | §30 |
| N4 | 可重绑热键 + **更多动作** | 已做（§60）：四个动作各自可绑，id 固定 1–4 | `src-tauri/src/hotkeys.rs` 的 `enum Chord { Toggle, Window }`、`CHORDS: [Chord; 2]`；`domain/settings.rs:177-181` 只有 `hotkey_toggle` / `hotkey_window` |
| N5 | geo 数据现状 / 刷新 | 已做 | §26 |
| N6 | 内核意外退出通知 | 已做 | `src-tauri/src/lib.rs:404` `notify_engine_exit()`，三语标题，成功/失败都写 stderr |
| N7 | 出口 IP | 已做 | §26 |
| N8 | 测速可调（超时 / 并发 / **启动时测速** / 选最快） | 已做（§60）：`test_on_start` 补上，启动即测 | `domain/settings.rs:75/81` 的 `test_timeout_ms` / `test_concurrency`（含校验）、`NodesView.vue:643` `pickFastest()`；`grep -rn test_on_start src` 无命中 |
| N9 | 更多匹配器 | 已做 | `domain/routing.rs:22` `RuleMatcher` 含 `DomainRegex` / `GeoSite` / `Port` / `ProcessName` 等 9 种 |
| N10 | 订阅重命名 / 过滤 | 已做 | §28 |
| N11 | 内核二进制选择 | 已做 | §29 |
| N12 | 应用更新检查 | 已做 | `src/views/AboutView.vue` 的「检查更新」 |
| N13 | 更多界面语言 | 已做 | §42（zh / en / zh-TW） |

## 3. 结论：真的还剩两项（已在 §60 做完）

1. **N4 的后半截**：热键再加两个动作 —— 切代理模式（`mode`）、测全部节点（`test_all`）。
   审计原意是 `Settings::hotkeys { toggle, window, mode, test_all }`；现有形状是扁平两字段，补两个同形字段即可，不重排已有字段。
2. **N8 的后半截**：「启动时测速」开关（`test_on_start`）。后端已有 `test_nodes_latency(state, ids)`（空 = 全部），只需在启动后按开关跑一次。

其余条目都在代码里有落点，不再重做。

## 4. 这一轮的执行口径（S12）

一次任务书、一个子代理、按旧流程走完：

| 步 | 内容 |
|---|---|
| 任务书 | `D:\tmp\handover\tasks\S12-hotkeys-and-test-on-start.md` |
| 代码 | `hotkeys.rs` 加两个 `Chord`；`settings.rs` 加 `hotkey_mode` / `hotkey_test_all` / `test_on_start`；`lib.rs` 两个动作的真实处理；`SettingsView.vue` 两行热键 + 一个开关；i18n |
| 门槛 | `npm run check`、`cargo test --lib`、`check-design.py`，IPC 仍 **97**（不加命令） |
| 真机 | 在 cloudpc-test 上真发组合键（`keybd_event`），看模式真的从「规则」跳到「全局」且配置真的重渲染；看「测全部」后节点页出现延迟；`test_on_start=true` 时启动后 `probes` 里自然有数 |
| 文档 | `docs/ACCEPTANCE.md` §60 + README/ARCHITECTURE/HANDOVER/PARITY-PLAN/本文件的状态列 |
| 提交 | 单独一次提交并 push，`cleanup.sh` 收尾 |

判据（写进 §60 的那张表）：

1. 绑上 `Ctrl+Shift+M` 后，在窗口前台按键，首页的模式胶囊从「规则」变「全局」，`store.json` 的 `mode` 跟着变；内核在跑时配置被重渲染（`kernels` 的 PID 变化），系统代理仍指向同一端口。
2. 绑上 `Ctrl+Shift+T` 后按键，10 秒内节点页每个节点出现延迟数字（或「超时」），`store.json` 的 `probes` 非空。
3. `test_on_start` 打开后，起应用不点任何按钮，`probes` 里也会出现一轮结果；关掉后重启不再自动测。
4. 两个新热键留空时不注册、也不报错；与已有组合冲突时只在 stderr 记一行（沿用现有「失败不拦启动」的语义）。
# 交接清单（X-Matrix Client Pro）

> 写于 2026-11-23 11:20（Asia/Shanghai），交接时的真实状态。工作区：`D:\GitHub-Warehouse\X-Matrix-Client-Pro`
> 目标（用户原话，长期有效）：**功能与前端设计追平 V2RayN / Clash**，做得比它们更干净。

---

## 0. 三句话现状

1. 主体功能已追平 V2RayN / Clash 的日常面：双内核（Xray / sing-box）渲染与启停、系统代理接管与还原、订阅/整包导入、Clash 配置连同 **策略组与规则** 一起导入、运行中热切换节点与代理模式、托盘子菜单、日志页、hysteria2 / tuic、真实延迟（分组与节点）、流量历史落盘、一键断开全部 + 进程归属、测速预算与「选最快」、域名正则 / 域名分类匹配器、诊断包导出、全局快捷键。
2. 验收方式是**真机**：每一条都跑到云电脑 `100.106.121.0`（Windows Server，Console 会话）上点出来、读日志/读配置文件核对，结论写进 `docs/ACCEPTANCE.md`（现有 §0–§20，共 20 节）。
3. 当前有**一处未提交的工作**（N12 应用内更新检查，见 §5），代码已写完、测试全绿、还没构建进 exe，也没在真机上验过。

### 0.1 进度快照（交接这一刻的硬数字）

| 项 | 值 |
|---|---|
| 代码 HEAD | `5a44af3`「feat(routing): domain regex and geosite lists…」（= 最后一个**已验收**的功能） |
| 未提交 | N12：9 个已改文件 + `src-tauri/src/update.rs`（新文件） |
| 本文档 | 随 `docs: a handover map of what is done, what is half done, and what is next` 提交入库（HEAD 上这一条） |
| IPC 命令 | **73**（`README.md:81`/`ARCHITECTURE.md:277` 已同步） |
| Rust 单测 | **272 passed**（`ARCHITECTURE.md:278` 已同步） |
| 前端检查 | `npm run check` 全绿：vue-tsc / architecture / contract / vitest **57**（8 文件） |
| i18n | **666** 键（`scripts/i18n-pairs.json` → `python scripts/gen_i18n.py`） |
| 桌面产物 | `X-Matrix.exe` **9,823,232 B @10:32**、`X-Matrix_0.1.0_x64-setup.exe` **3,181,050 B @10:32** —— 都是 **N9 版**，不含 N12 |
| 验收文档 | `docs/ACCEPTANCE.md` §0–§20（§7 历史空缺，编号不连续但内容完整） |
| 云电脑 | 干净：`ProxyEnable=0x0`、无 X-Matrix/sing-box/xray 进程、无 `xm*` 计划任务 |

**下一步的确定顺序**（做完一条打一个勾，别跳）：

- [x] 1. `npm run tauri build`（2026-11-23 23:27 完成，`x-matrix.exe` 9,854,976 B）
- [ ] 2. `cp` 刷桌面两件 + `cmp` 校验
- [x] 3. 云电脑真机点「关于 → 检查更新」：未发版时 404 文案正确，发布 `v0.1.0` 后显示「已是最新版本（0.1.0）」
- [x] 4. 写 `docs/ACCEPTANCE.md` §21（正常退出）与 §22（检查更新）
- [ ] 5. 提交 N12（提交信息建议 `feat(update): ask the release feed, never touch the binary`）
- [x] 6. `src-tauri/src/update.rs:23` 的 `FEED_URL` 已指向真实仓库 `linjunhao024-byte/X-Matrix-Client-pro`
- [x] 7. 已同步 `README.md:81` 的 71→73、`ARCHITECTURE.md:277` 的 260→270
- [ ] 8. `bash /d/tmp/shots/cleanup.sh` 收尾云电脑

---

## 1. 项目坐标与命令

| 项 | 值 |
|---|---|
| 仓库 | `D:\GitHub-Warehouse\X-Matrix-Client-Pro` |
| 技术栈 | Tauri 2（Rust 后端）+ Vue 3 + TypeScript（前端，Vite 构建） |
| 内核 | Xray-core、sing-box（用户手动放进 `engines/` 目录，界面只读版本号） |
| 构建 | `export PATH="$HOME/.cargo/bin:$PATH"` 然后 `npm run tauri build`（cargo 命令要在 `src-tauri/` 下跑） |
| 前端检查 | `npm run check`（= `typecheck` + `check:arch` + `check:contract` + `test`） |
| 后端测试 | `cd src-tauri && cargo test --lib` |
| 契约重生成 | `npm run gen:contract`（改了命令签名就必须跑，`check:contract` 会拦） |
| 桌面产物 | `/c/Users/83611/Desktop/X-Matrix.exe`、`/c/Users/83611/Desktop/X-Matrix_0.1.0_x64-setup.exe` |
| 当前桌面产物版本 | exe **9,823,232 B @10:32**、NSIS **3,181,050 B @10:32**（= N9 那一次构建，**不含 N12**） |

**计数基线（改动后记得同步文档）**：命令 **73**、`cargo test --lib` **272**、vitest **57**（8 个文件）、i18n **666** 键。
> `README.md:81` 与 `ARCHITECTURE.md:277` 已同步为 73 条命令 / 272 项测试。

命令数核算法：

```bash
awk '/collect_commands!/,/^];/' src-tauri/src/ipc.rs | grep -o "commands::[a-z_]*::[a-z_]*," | wc -l
```

---

## 2. 架构地图（改代码前先看这张表）

| 文件 / 目录 | 职责 | 改动注意 |
|---|---|---|
| `src-tauri/src/ipc.rs` | **所有 IPC 命令的唯一出口**（`collect_commands![…]`） | 新命令必须在这里注册，否则前端看不到；注册后跑 `npm run gen:contract` |
| `src/services/bindings.ts` | `tauri-specta` 生成，**不要手改** | 由 `gen:contract` 覆盖 |
| `src/services/contract.ts` | 唯一允许直接碰 bindings 的模块；把 `Result` 解包成异常 | 新类型既要进顶部 `import type { … } from './bindings'`，也要进文件末尾 `export type { … }`；**只加后者会报 `TS2304`** |
| `src-tauri/src/store.rs` | 单文件 JSON 持久化（临时文件 + 原子替换，坏文件移 `.corrupt`） | 存储路径 `%APPDATA%\com.xmatrix.client\store.json` |
| `src-tauri/src/state.rs` | `AppState`：`nodes/settings/rules/subscriptions/groups/rule_sets` 各一把 `RwLock` + `persist()` | 任何写操作后要 `persist()`；改了节点/设置要 `refresh_tray(&app)` |
| `src-tauri/src/engine/render.rs` | **纯函数**：把状态渲染成 Xray / sing-box 配置 | 全部渲染测试都在这里，不需要任何内核二进制 |
| `src-tauri/src/engine/runtime.rs` | 内核进程、端口预检、就绪探测、日志（`log_delta`）、`KillOnCloseJob`（JobObject 防孤儿） | |
| `src-tauri/src/engine/stats.rs` | 统计与连接（Xray `expvar` / sing-box Clash API）、`clash_delay`、`close_connections` | |
| `src-tauri/src/engine/traffic_log.rs` | 流量历史 `traffic.jsonl`，一分钟一行，7 天保留 | |
| `src-tauri/src/sysproxy/` | 系统代理接管：先快照，`Drop` 兜底还原 | |
| `src-tauri/src/domain/` | `node.rs` / `settings.rs` / `routing.rs` / `group.rs` / `subscription.rs` / `rule_set.rs` | `Protocol`、`RuleMatcher`、`RuleAction` 的穷尽 `match` 分散在 `engine/`、`documents.rs`、`export.rs`，加变体要全找一遍 |
| `src-tauri/src/import.rs` / `documents.rs` / `export.rs` | 分享链接解析 / Clash+sing-box 文档解析 / 导出分享链接 | 三者对同一组协议要保持一致 |
| `src-tauri/src/tray.rs` | 托盘子菜单（节点 ≤20 条、代理模式），整菜单重建 | 菜单没法用测试断言，靠 `eprintln!` 日志证明「那一刻托盘以为自己在显示什么」 |
| `src-tauri/src/hotkeys.rs` | **自研**全局快捷键（隐藏窗 + `GetMessageW` 线程），`Ctrl+Shift+X` 启停、`Ctrl+Shift+W` 显隐 | 不要换回 `tauri-plugin-global-shortcut`：它在本机注册成功但 `WM_HOTKEY` 永远到不了处理函数 |
| `src-tauri/src/diagnostics.rs` | 诊断报告（打码后可直接分享） | 打码必须保留 `"key": "***"` 的引号与冒号，否则报告不是合法 JSON |
| `src-tauri/src/update.rs` | N12：问 GitHub releases 有没有新版（**未提交**） | 见 §5 |
| `scripts/i18n-pairs.json` | **i18n 唯一真相源**（`[key, zh, en]` 三元组数组） | 改完跑 `python scripts/gen_i18n.py` 生成 `src/i18n/{zh,en}.ts`；中英必须同时给 |

---

## 3. 开发纪律（违反会直接坏掉用户体验或验收）

1. **契约出口单一**：新命令只加在 `ipc.rs` 的 `collect_commands![…]`，前端只经 `contract.ts` 调用（`npm run check:arch` 会抓违规 import）。
2. **i18n 成对**：`scripts/i18n-pairs.json` 里加一条就要给中英两份；`npm run check` 里有条测试强制两边键集相同。
3. **渲染是纯函数**：任何「写进内核配置」的逻辑都必须能在没有内核的机器上被测到。
4. **失败要留证据**：读不到的统计、探测不到的延迟，返回 `None`/`skipped` 并附人类可读原因，不要 `Err` 掉整个页面。
5. **绝不静默丢弃**：导入时认不出的行进 `skipped` 并说明原因（这条是 N9 的直接动因）。
6. **改 UI 前先看 token**：颜色一律用 `src/styles/tokens.css` 的变量；没有合适的就新增变量，不要在组件里写死 `rgba()`。

**硬约束（用户明确要求，永久有效）**：

- **绝不在这台本机上启动内核**，**绝不写本机系统代理注册表**。一切真机验证都在云电脑上做。
- 不要杀 `PID 5684 TACloud\watchDog\watchdog-teleagent.ps1`（云电脑保活代理，杀了就再也连不上）。
- 子代理在本环境**极不可靠**（多次静默死亡、零产出）：并行上限用户已放宽到 3，但关键路径建议自己做。

---

## 4. 已完成的功能与验收索引

`docs/ACCEPTANCE.md` 是**唯一可信的进度表**（20 节，每节都有「验法 + 判据 + 设计理由 + 坑」）：

| 节 | 内容 |
|---|---|
| §0 | 准备（云电脑、内核二进制、WebView2） |
| §1 | 启动与外观 |
| §2 | 数据（不需要内核）：节点编辑、订阅、路由、策略组、规则集 |
| §3 | 真启动（需要内核 + 网络） |
| §4 | 系统代理（真启动才有意义） |
| §5 | 明确不支持（不是 bug，别当问题报） |
| §6 | 已实测汇总（2026-11-22，Windows Server 2022 + WebView2 157 + 真内核） |
| （无 §7） | 编号从 §6 直接跳到 §8，历史上删过一节，不代表缺内容 |
| §8 | 首页代理模式（规则/全局/直连）与启动参数 |
| §9 | 窗口几何记忆、设置页 sticky 头两版迭代、快捷键面板 |
| §10 | 全局快捷键（自研）与诊断报告 12,575 字节实样 |
| §11 | 运行中热切换（节点 / 内核 / 模式），PID 与端口证据 |
| §12 | hysteria2 / tuic：导入 → 起得来 → Xray 明确拒绝 |
| §13 | 托盘子菜单（含「托盘图标在溢出区点不到」的诚实说明） |
| §14 | Clash 配置导入：节点 + 策略组 + 规则一起进来 |
| §15 | 订阅走代理拉取 |
| §16 | 分组 / 节点真实延迟（sing-box Clash API），失败如实「测不到」 |
| §17 | 连接页一键断开全部 + 进程归属 |
| §18 | 测速预算：并发 1 → 6793 ms vs 并发 6 → 1639 ms，「选最快」 |
| §19 | 流量历史：每分钟一条，重启还在，1h/24h 对照 |
| §20 | 域名正则 / 域名分类匹配器，两个内核的渲染证据 |

已提交的提交序列（新→旧，14 条主体）：
`5a44af3` 域名正则+geosite · `569a7ce` 流量历史 · `44b857b` 测速预算+选最快 · `42f85bd` 一键断开+进程名 · `f817726` 真实延迟 · `f3fc750` 订阅走代理 · `d892b9e` Clash 导入带组和规则 · `6851775` 托盘子菜单 · `f9a0406` H4 验收文档 · `14ddb20` hysteria2/tuic · `fdd0b8d` 运行中热切换 · `eb3f453` 日志页 · `79ccadd` 设计修复（粘右操作列、错误用红、焦点陷阱等）· `535124b` 快捷键与诊断包验收。

`D:\tmp\parity-gaps.md` 是最初的对标审计（31 行已覆盖、7 高价值、13 锦上添花、11 明确不做），H1–H7 与 N1、N2、N8、N9、N12 已吃掉其中大部分。

---

## 5. 未提交的工作：N12「应用内更新检查」（做到一半）

**状态：已完成并真机验收（2026-11-24，见 `docs/ACCEPTANCE.md` §22）：仓库发布 `v0.1.0` 后「检查更新」显示「已是最新版本（0.1.0）」，未发版时显示「发布页还没有发布记录（HTTP 404）」。`cargo test --lib` 272 通过。**

改动的文件（`git status` 当前输出）：

```
 M scripts/i18n-pairs.json      +8 键（about.update.*）→ 666
 M src-tauri/src/commands/system.rs   新增 check_update / app_open_url
 M src-tauri/src/ipc.rs         注册两条命令（→73）
 M src-tauri/src/lib.rs         mod update;
 M src/i18n/en.ts, zh.ts        重新生成
 M src/services/bindings.ts     重新生成
 M src/services/contract.ts     checkUpdate / appOpenUrl 包装 + 类型导出
 M src/views/AboutView.vue      「检查更新」卡片
?? src-tauri/src/update.rs      新文件（约 200 行）
```

设计要点（别退回去）：

- `update.rs`：`FEED_URL` + `FEED_TIMEOUT=10s`；`UpdateStatus{current,latest,has_update,url,notes}`；`parse_version` 去 `v` 前缀、缺位补 0、`1.3.0-beta.1` 记 1.3.0；**任一侧版本号解析失败一律「不算有新版本」**（说不准不能变成催促）；非 JSON / 空 tag 都给人话错误。
- `check_update` 命令走 `spawn_blocking`，**不在 IPC 线程上开 socket**（10 秒超时时窗口必须还能动）。
- `app_open_url` **只放行 `http(s)`**：URL 来自网络响应，直接丢给 shell 打开是这类应用最容易被利用的口子。
- 关于页：`updateFailed` 与页面级 `error` **分开**——「问不到发布页」不是应用坏了，用 muted 小字，不刷红。

**必须做的两件事**：

1. **`FEED_URL` 还是占位地址** `https://api.github.com/repos/x-matrix-client/x-matrix/releases/latest`，仓库当前**没有 git remote**。发布前必须换成真实仓库。
2. 收尾流程：`npm run tauri build` → `cp` 刷桌面两件并 `cmp` → 云电脑部署验证（见 §6）→ 写 `docs/ACCEPTANCE.md` **§21** → 提交 → `cleanup.sh`。
   验证时注意：云电脑上 GitHub 可能不可达，**如实记录失败文案**（应该是「没能问到发布页：…」），不要伪造成「已是最新」。

---

## 6. 下一步候选（按「性价比 / 风险」排序）

| 候选 | 落点 | 代价 | 备注 |
|---|---|---|---|
| **N12 收尾**（见 §5） | `update.rs`、`AboutView.vue` | 小 | 只差构建 + 真机一格 |
| **N3 节点二维码** | 新命令 `node_share_qr(node_id) -> String`（SVG），复用导出链接逻辑与现有节点对话框 | 中 | 需要 `qrcode` crate；**本机 cargo 缓存里没有**，离线会失败——先试 `cargo add qrcode --offline` 探一下 |
| **N6 内核意外退出通知** | 挂在 `src-tauri/src/engine/runtime.rs` 的退出监视上；Windows toast | 中 | 需要 `tauri-plugin-notification`（未装）；注意 fail-open，别因为通知失败影响启停 |
| **N10 订阅重命名 / 过滤** | `domain/subscription.rs` 加 `rename_prefix` + `include`/`exclude` 正则，刷新时应用；`ImportPreview` 式预览计数 | 中 | 纯解析，可本机测 |
| **N11 内核二进制选择** | 设置页指定 Xray / sing-box 的路径，存 `Settings`，用 `version` 命令校验 | 中 | 与 `engine/runtime.rs` 的 `binaries()` 有耦合，要先读 |
| **N13 更多语言** | `scripts/i18n-pairs.json` 加 zh-TW / ja；`Language` 枚举 + 托盘 `Labels` + `describeError` 的文案都要跟上 | 大 | parity 测试会强制键集完整，666 键 ×2 语言 |
| **H5(b) 无内核时的逐节点真实延迟** | 临时起一个最小内核做探测 | 大 | 风险高（要多一条内核生命周期），当初明确「先不做」 |
| 设计审计残余 | `D:\tmp\design-audit-2.md` | — | 二审 15 条已全部处理完；这份文件可以当历史读 |

明确**不做**的（`D:\tmp\parity-gaps.md` §2.3，S1–S11）：TUN 模式、PAC 脚本、内核自动更新与下载、开机直接接管系统代理等。

---

## 7. 验收环境手册（云电脑）

**接入**：

```bash
SSH="C:/Windows/System32/OpenSSH/ssh.exe"
KEY="-i C:/Users/83611/.ssh/id_ed25519_cpc -o StrictHostKeyChecking=no -o BatchMode=yes"
"$SSH" $KEY Administrator@100.106.121.0 "cmd 命令"
scp -i /c/Users/83611/.ssh/id_ed25519_cpc 本地文件 Administrator@100.106.121.0:C:/Users/Administrator/
```

- 本地路径给 `D:/tmp/...`（**不要写 `/d/tmp/...`**）。
- cmd 里用 `&` 串命令；**cmd 嵌套 `for` 在 ssh 下会挂住**，清理用平铺 `&` 或 `cleanup.sh`。
- 远端应用数据在 `C:\Users\Administrator\AppData\Roaming\com.xmatrix.client\`（`store.json`、`configs/`、`logs/`、`traffic.jsonl`）。

**现成脚本（都在 `D:\tmp\shots\`，直接 `bash` 调）**：

| 脚本 | 用途 |
|---|---|
| `deploy-tray.sh <exe>` | scp + 替换 exe + 建计划任务 `xmrun` 启动 + 打印 stderr（`run-xm.bat` 把 stderr 落到 `xm-err.txt`） |
| `run-uia.sh <cmd-file>` | 推 `uia2.ps1` + 指令文件 → 计划任务 `xmui` → 打印 `uia.txt`；指令：`list` / `dump` / `invoke\|名字` / `invoke_at\|名字\|序号（0 基）` / `invoke_last\|名字` / `setvalue\|名字\|文件` / `value\|名字` |
| `run-act.sh <act-file> [shot]` | 推 `x-act.ps1` 的 act 脚本 → 计划任务 `xmact` → 打印 `act.log`；act 语法：`click X Y` / `rclick` / `wheel X Y N` / `key K` / `paste NAME`（读 `paste-NAME.txt`）/ `focus` / `wait MS` / `shot NAME` / `shotfull NAME` / `log TEXT` |
| `run-ps1.sh <local.ps1> <remote-name> [out]` | 跑任意 PS 脚本（脚本必须自己 `Out-File`） |
| `cleanup.sh` | 先用 `X-Matrix.exe --quit` 让程序自己退出（走 `quit_cleanly`，不留死图标），两次不走再兜底强杀；删全部 `xm*` 计划任务与临时文件；`ProxyEnable=0` 并回读确认 |

**踩过的坑（几乎每条都花过半小时）**：

1. **应用重启后必须先 act `focus`**（内部 `ShowWindow(hwnd, 3)` = SW_MAXIMIZE）让 WebView2 建立元素树，否则 UIA `invoke` 全是 `0 (of 0 matches)`。
2. **能 `invoke|名字` 就不要点坐标**：UIA 矩形是逻辑坐标，`click` 是物理像素，缩放会话里对不上。
3. 同名控件用 `invoke_last|名字`（例：工具栏「导入」vs 对话框「导入」）；`invoke_at` 的序号是 **0 基**。
4. 侧栏导航是 **Hyperlink** 不是 Button；内核切换（Xray/sing-box）是 **TogglePattern**，按 Button 筛会 0 匹配。
5. **远端 OCR / 输出必须 `scp` 回来读**：ssh `type` 会把 UTF-8 中文打散成 `�`，用中文 grep 会假阴性。**能用 ASCII 关键字就用 ASCII**。
6. 远端 `.ps1` 必须 **UTF-8 with BOM**（PS 5.1 按 ANSI 解析含中文的脚本会报「字符串缺少终止符」）。
7. 计划任务要跑 GUI 进程，action 里**不能带 `>` 重定向**（cmd 会算作 action 的一部分）；脚本自己写文件。
8. `Get-Process | MainWindowHandle` 对计划任务起的进程恒为 0 → 用 `FindWindow([NullString]::Value, 'X-Matrix')`。
9. 停程序一律用 `X-Matrix.exe --quit`（会话 1 里的计划任务，脚本 `D:\tmp\shots\quit-run.ps1`）。`taskkill /F` 不会走 `quit_cleanly()`，**`ProxyEnable` 会留在 1**，还会留下死托盘图标：收尾必须手动 `reg add "HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings" /v ProxyEnable /t REG_DWORD /d 0 /f`（是 **HKCU** 不是 HKLM）。
10. 托盘图标常落进通知区溢出区，**UIA 找不到、右键点不到**：涉及托盘的验收只能靠 `eprintln!` 日志（`托盘菜单 N 个节点 · 模式 X · 内核运行中`）。
11. `Local-WS` 节点依赖本地测试服务器（`xmsrv` 计划任务跑 `srv-lite2.json`，两入站 uuid 必须与节点一致）；被 `cleanup.sh` 删过之后要重建，否则「隧道在跑但全是 502」。
12. 追加带反斜杠/中文的文档段落：先用 `write` 落一个临时 `.md`，再用 python 按字节拼接（heredoc 会把 `\t`/`\n` 吃成 TAB 与断行）。

---

## 8. 已知技术债与需要记住的真相

1. **sing-box 1.14 忽略 Clash API 的 `url` 形参**：`/proxies/{tag}/delay?url=…` 给一个必死地址仍返回 `{"delay":109}`。所以「用户的测试地址会被采用」这个功能假设**不成立**；已在 `engine/stats.rs::clash_delay` 注释里写明。
2. **`metadata.processPath` 默认是空的**，必须开 `route.find_process = true` 才拿得到进程名（`engine/singbox.rs` 里已开）。
3. **别用 grep 判断代码有没有编进二进制**：Rust 会把短字符串拆成 `movabs` 立即数（`H\xb9find_pro` + `\xc7@\x08cess`），文件里根本不存在连续字节。要用行为判断。
4. **Xray 不支持 hysteria2 / tuic**：`engine/xray.rs::validate_node` 直接报「Xray 内核不支持 Hysteria2 协议，请在首页把内核切换为 sing-box 后再启动」，而不是渲染出一份丢节点的配置。
5. **导出型绑定是「宽」的**：specta 把所有 `f64` 与 `usize` 都渲染成 `number | null`，TS 侧要 `?? 0`。
6. **规则表非空时，导入整批跳过规则**（设计：不覆盖用户已排好的顺序，因为规则自顶向下、追加到兜底之后等于没导入）。验证导入前要先清 `rules` 并重启。
7. **store 顶层键**：`version / nodes / settings / rules / subscriptions / groups / rule_sets`（路由规则在 `rules`，不是 `routing.rules`）。
8. `ops_grep` 对 `.vue` 会返回 `skippedFiles=52`（等于没扫）→ 用 `bash grep -n`。

---

## 9. 开工检查表（新窗口照着做）

1. `cd /d/GitHub-Warehouse/X-Matrix-Client-Pro && git status` —— 确认只有 N12 那批改动，没有别人在写。
2. 读本文件的 §5 与 `docs/ACCEPTANCE.md` 的最后两节，对齐上下文。
3. `export PATH="$HOME/.cargo/bin:$PATH"`；`cd src-tauri && cargo test --lib`（应为 272 passed）。
4. `cd .. && npm run check`（应为全绿）。
5. 决定这一步要做哪一项（建议先把 N12 收尾：构建 + 真机 + 文档 + 提交），**一项一提交**，提交信息用英文 `feat(...)/fix(...)/docs: ...`。
6. 真机验证前先跑 `bash /d/tmp/shots/deploy-tray.sh <exe>`；远端被 `cleanup.sh` 清过的话，先 `scp x-act.ps1 uia2.ps1 ocr.ps1` 上去。
7. 验完写 `docs/ACCEPTANCE.md` 新的一节（**验法 / 判据 / 设计理由 / 坑** 四件套），并同步 `README.md:81`、`ARCHITECTURE.md:277` 的命令数与测试数。
8. 收尾 `bash /d/tmp/shots/cleanup.sh`，再手查 `ProxyEnable` 与 `schtasks /query | findstr xm` 确实是空的。

---

## 10. 一句话交接

**能用的东西已经都在了，剩下的都是边角。**接着做的人只需要：先把 §5 的 N12 收尾（含把 `FEED_URL` 换成真实仓库），再按 §6 表挑一项，每项都按「实现 → 单测 → `npm run check` → 构建 → 云电脑真机 → 写 ACCEPTANCE → 提交 → 清理」这套流程走完。

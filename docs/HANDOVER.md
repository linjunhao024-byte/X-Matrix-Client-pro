# 交接清单（X-Matrix Client Pro）

> 写于 2026-11-23 11:20（Asia/Shanghai），交接时的真实状态。工作区：`D:\GitHub-Warehouse\X-Matrix-Client-Pro`
> 目标（用户原话，长期有效）：**功能与前端设计追平 V2RayN / Clash**，做得比它们更干净。

> **刚接手这个项目？先读 `docs/HANDOFF-BRIEF.md`（一页纸：现状、四条命令、下一个任务、坑）。** 本文是全书。

---

## 0. 三句话现状

1. 主体功能已追平 V2RayN / Clash 的日常面：双内核（Xray / sing-box）渲染与启停、系统代理接管与还原、订阅/整包导入、Clash 配置连同 **策略组与规则** 一起导入、运行中热切换节点与代理模式、托盘子菜单、日志页、hysteria2 / tuic、真实延迟（分组与节点）、流量历史落盘、一键断开全部 + 进程归属、测速预算与「选最快」、域名正则 / 域名分类匹配器、诊断包导出、全局快捷键。
2. 验收方式是**真机**：每一条都跑到云电脑 `100.106.121.0`（Windows Server，Console 会话）上点出来、读日志/读配置文件核对，结论写进 `docs/ACCEPTANCE.md`（现有 §0–§48）。
3. 交接时那处未提交的工作（N12 应用内更新检查）已经构建、真机验收并提交；此后又合入四件事：`--quit` 正常退出并删除托盘图标（§21）、GEO 数据文件卡片与镜像回退下载（§23）、测速记录落盘（§24）。工作区现在是干净的。

### 0.1 进度快照（交接这一刻的硬数字）

| 项 | 值 |
|---|---|
| 代码 HEAD | 「feat(sysproxy): a PAC script as the second way to take the system proxy over」，含 §48（S2 PAC）——就是 `git log --oneline -- src-tauri/src/sysproxy/pac.rs` 的第一条；再往前：`6fd790f`（§47）、`ca53381`（§46） |
| 未提交 | 无。§48（S2 PAC）的代码、构建、真机验收、`docs/ACCEPTANCE.md` §48 与 `docs/PARITY-PLAN.md` 都在同一次提交里（见 §0.4） |
| 上一版未提交清单（留档） | `src-tauri/src/domain/settings.rs`、`src-tauri/src/engine/{mod,render,runtime,xray}.rs`、`src/services/bindings.ts`、`src/views/SettingsView.vue`、`src/i18n/{zh,en}.ts`、`scripts/i18n-pairs.json`（§39 分片）；再上一版：`src-tauri/src/engine/{mod,runtime}.rs`、`src/services/bindings.ts`、`src/views/SettingsView.vue`、`src/i18n/{zh,en}.ts`、`scripts/i18n-pairs.json`（§37 统一提示）；再上一版：`src-tauri/src/commands/settings.rs`、`src-tauri/src/commands/sysproxy.rs`、`src/stores/sysproxy.ts`、`src/views/SettingsView.vue`、`src/i18n/{zh,en}.ts`、`scripts/i18n-pairs.json`、`docs/ACCEPTANCE.md`（§36 系统代理即时生效） |
| 本文档 | 随 `docs: a handover map of what is done, what is half done, and what is next` 提交入库；之后每次收尾都跟着更新 |
| IPC 命令 | **81**（`README.md:81`/`ARCHITECTURE.md:277` 已同步） |
| Rust 单测 | **394 passed**（`ARCHITECTURE.md:278` 已同步） |
| 前端检查 | `npm run check` 全绿：vue-tsc / architecture / contract / vitest **98**（10 文件） |
| i18n | **764** 键 × 3 份字典（简/繁/英，`scripts/i18n-pairs.json` → `python scripts/gen_i18n.py`） |
| 桌面产物 | 桌面上的旧副本**已不存在**；唯一产物是 `src-tauri/target/release/x-matrix.exe`（10,466,816 B @2026-11-25 04:05，md5 `2f66e09124a2d5c937e2677c9d7e03ed`，含 §21–§48 全部改动） |
| 验收文档 | `docs/ACCEPTANCE.md` §0–§48（§7 历史空缺，编号不连续但内容完整） |
| 云电脑 | 干净：`ProxyEnable=0x0`、无 X-Matrix/sing-box/xray 进程、无 `xm*` 计划任务 |

**下一步的确定顺序**（做完一条打一个勾，别跳）：

- [x] 1. `npm run tauri build`：`--quit`、N12、GEO、测速落盘、退出通知各一轮
- [ ] 2. `cp` 刷桌面两件 + `cmp` 校验（**唯一常被忘的一步**）
- [x] 3. 云电脑真机验收：§21 退出、§22 检查更新、§23 geo、§24 测速、§25 退出通知、§26 出口 IP
- [x] 4. `docs/ACCEPTANCE.md` 写到 §26
- [x] 5. 提交（`fix(tray)` / `feat(update)` / `feat(geo,nodes)` / `feat(engine)` 各一条）
- [x] 6. `FEED_URL` 已指向真实仓库 `linjunhao024-byte/X-Matrix-Client-pro`，`v0.1.0` 已发布并带 `X-Matrix.exe` 资产
- [x] 7. 计数已同步：81 命令 / 394 单测 / 98 vitest / 764 i18n 键 × 3
- [x] 8. `bash /d/tmp/shots/cleanup.sh` 收尾云电脑（2026-11-24 23:55 再跑一次：`ProxyEnable 0x0`、无 `xm*` 任务）
- [x] 9. §47 订阅刷新后前端握着死 ID：补丁 → 构建 → 云电脑两轮对比验收 → 写 §47 → 提交（2026-11-25 02:36）
- [x] 10. §48（S2）PAC 自动配置：生成脚本 + 注册表互斥 + 回环/私网兜底 → 构建 → 云电脑验收 → 写 §48 → 提交（2026-11-25 04:20）；后续 S8/S10/S11/S9/S3/S6/S5/S4/S1/S7 见 `docs/PARITY-PLAN.md`

### 0.2 这个窗口（2026-11-24）新完成的六件事

按验收节顺序，细节都在 `docs/ACCEPTANCE.md`：

1. **§41 可录制的全局快捷键**：两个组合键存成 `settings.hotkey_toggle` / `hotkey_window` 的纯文本，录制按钮写值，保存即重注册（`hotkeys::apply` 往消息线程投 `WM_APP_REREGISTER`）。顺带修掉一个真死锁：`WM_APP_REREGISTER` 分支持着 `BINDINGS` 锁又去 `register_all` 里再锁同一把 `std::sync::Mutex`，结果新键永不生效、旧键也被注销。
2. **§42 第三种界面语言（繁體中文）**：`scripts/i18n-pairs.json` 变成四格 `[key, zh, en, tw]`，繁体外加台湾用词表由 `zhconv` 机械转换；`Language::ZhTw` 用 `#[serde(rename = "zh-tw")]`（`rename_all = "lowercase"` 会拼成 `zhtw`）。同时补了一条真正的协调缺口：语言选择器以前只写 `localStorage`，**托盘菜单和退出通知根本不跟着换**；现在走窄命令 `set_language`（写设置 + `refresh_tray`）。
3. **§43 无内核时的真实延迟**：`engine/temp.rs` 起一个一次性内核（两个空闲端口、单节点 `global` 配置、`Drop` 保证杀掉删净），测完即走；「拒绝」与「测不出」分开记，失败不覆盖上一次的好数字。
4. **§44 换代理模式要说一声**：`EngineStatus.mode` 由内核自报，设置页把它并进统一提示；顺手修了「重启按钮点了没反应」——`engine.reapply()` 以前从设置镜像读 `last_node_id`，那个 id 可能已被删除/替换，现在优先用内核自报的 `status().node_id`。
5. **§45 测试地址提示 + sing-box 启动失败的真凶**：发现我们一直给 sing-box 写 `"geosite"`/`"geoip"`，而 sing-box ≥1.12 已删除这两个字段 —— **只要规则表里有 geo 规则（默认就有），切到 sing-box 必然启动失败**，界面只报「8 秒内没打开控制端口 9090」。最终方案：应用启动前自己下 `.srs`（`geo::rule_set_candidates` + `runtime::prepare_geo_rule_sets`，落到 `rule-sets/geo-srs/`），渲染只指本地文件；`.srs` 必须二进制写（`std::fs::write` + rename），用 `write_atomic` 走 `String::from_utf8_lossy` 会被内核判 `flate: corrupt input before offset 10`。
6. **§46 geo 数据换了、跑着的内核还在用旧的**：`update_geo_assets` 改成返回 `GeoUpdate { lines, updated, restart_needed }`，`updated` 在自己真正装了文件的地方计数（不解析文案），前端 `RuleSetsView.vue` 用 `StaleConfig` 说「geo 数据已更新，但内核还在用旧的一份」并一键重载重启。

### 0.3 §47 订阅刷新后，前端握着一个已经不存在的节点 ID（已完成，2026-11-25）

- **现象**：内核在跑时刷新订阅 → 后端按指纹「采纳」了同一个服务器的新 ID（内核不断，§35），但前端 `src/views/SubscriptionsView.vue` 的 `kernelStopped()` 在「内核还活着」时提前 `return false`，**没有重读设置**。于是侧栏/首页的设置镜像里还是旧 ID：首页节点下拉框选不中，「启动」时会报「节点 … 已经不在了」，而那个节点明明就在屏幕上。
- **修复（已写好，未提交）**：`kernelStopped()` 先 `await engine.refreshStatus()`，再**无条件** `await settings.load(true)`，最后才判断内核是否真的停了；函数上的 docblock 也要跟着改成「后端要么采纳新 ID（内核继续跑），要么清空并停内核」。
- **补丁**：`D:\tmp\handover\47-subscription-refresh-id.patch`（`git apply` 即可，35 行，只改这一个文件）。
- **验收方法（脚本已有九成）**：复用 `D:\tmp\shots\adopt-accept.sh`（假订阅源 `sub-server-b.ps1` 顺序反转、store 里 `subscription_via_proxy=false`、把 `last_node_id` 钉到订阅节点），在现有正文「`active id after`」之后追加：停止 → 首页「启动」→ 断言起了新内核（`kernels` 非空）且页面没有「已经不在了」字样。改前应能复现失败，改后应通过。
- **完成标准**：`bash` 跑通 → 写 `docs/ACCEPTANCE.md` §47（判据表 + 3 条设计理由 + 坑）→ 计数同步（命令仍 81，Rust 仍 377，vitest 98，i18n 758）→ 提交并 push（`https_proxy=http://127.0.0.1:10808 git push origin main`）→ `bash /d/tmp/shots/cleanup.sh`。

- **结果（2026-11-25 02:36，细节见 `docs/ACCEPTANCE.md` §47）**：验收脚本换成 `D:\tmp\shots\47-accept.sh`，探针从「首页模式按钮」改成「设置页保存」（前者存完立刻 `applyLive()`，会把要观察的状态抹掉）。改前二进制（`D:\tmp\handover\x-matrix-46.exe`，md5 `ccdc8803…`）在设置页保存后 store 的 `last_node_id` 退回死 id（`store id reverted: True`，且该 id 已不在 8 个节点里）；改后（md5 `bc5f0adc…`）保持采纳后的新 id（`reverted: False`）。两轮都确认刷新不打断内核（PID 不变）、没有「内核已停止」提示、停止后首页「启动」可用。
- **顺带纠正简报的一处描述**：`src/views/HomeView.vue:284-286` 的下拉默认取 `nodes.items[0].id`，从没读过 `last_node_id`，所以「首页下拉选不中 / 启动报已经不在了」不会那样出现；真正的伤害是**任何整份设置回写都会把死 id 写回后端**，要到下次带 `auto_connect` 启动才在外面显形。

### 0.4 §48（S2）PAC 自动配置（已完成，2026-11-25）

- **为什么做**：用户 2026-11-25 解禁 S1–S11（`docs/PARITY-PLAN.md`），S2 是第一批第一项；此前 `AboutView` 把 PAC 标成 `supported: false`。
- **做了什么**：新增 `src-tauri/src/sysproxy/pac.rs` 生成 `.pac`（`FindProxyForURL`：绕过列表 + 规则里「直连」的域名匹配器 → `DIRECT`，其余 → `PROXY 127.0.0.1:<mixed_port>; DIRECT`，非 ASCII 一律转义成 `\uXXXX` 形式）；`ProxyGuard` 增加 `enable_pac(port, bypass, script)`，先 `tmp+rename` 落盘再写注册表，接管时 `ProxyEnable=0` + 清 `ProxyServer` + 写 `AutoConfigURL=file:///…`，与固定代理互斥；`Settings.pac_enabled`；设置页开关「用 PAC 按域名分流」；`AboutView` 改 `supported: true`。
- **真机跑出来一个真缺陷**：第一版把「直不直连」完全交给 store 的 `proxy_bypass`，而测试机那份值是被人手改坏的（`和http：//cp.cloudfllare.com/`），结果 PAC 把 `localhost`/`127.0.0.1`/`10.1.2.3` 全送回 `127.0.0.1:7890`——代理自己就在那儿，等于回环。已加 `isAlwaysDirect(host)`（`localhost`、无点裸名、`127.`/`10.`/`192.168.`/`169.254.`/`172.16–31`、`::1`/`fe80`/`fc`/`fd`/`.local`）并在 `isBypassed` 之前无条件调用。
- **验收**：`bash /d/tmp/shots/48-pac-accept.sh`（配套 `mk_48_pac.py` 生成 PS 驱动、`pac-check.js` 是 cscript/JScript 5.8 harness）在云电脑上跑通：`AutoConfigURL` 出现/消失/还原、14 个 host 的判定（含 `172.32.0.1 → PROXY` 反例）、关掉 PAC 后 `ProxyEnable=1 ProxyServer=127.0.0.1:7890`、内核没被打断。细节与判据表见 `docs/ACCEPTANCE.md` §48。
- **计数**：命令 81（不变）、Rust **394**、vitest 98、i18n **764**（+6）。

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
| 当前构建产物 | `src-tauri/target/release/x-matrix.exe` **10,466,816 B @2026-11-25 04:05，md5 `2f66e09124a2d5c937e2677c9d7e03ed`**（含 §21–§48）；桌面副本已不在，需要时自己 `cp` 过去 |

**计数基线（改动后记得同步文档）**：命令 **81**、`cargo test --lib` **394**、vitest **98**（10 个文件）、i18n **764** 键。
> `README.md:81` 与 `ARCHITECTURE.md:277-278` 已同步为 81 条命令 / 394 项测试。

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

`docs/ACCEPTANCE.md` 是**唯一可信的进度表**（28 节，每节都有「验法 + 判据 + 设计理由 + 坑」）：

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
| §21 | 正常退出：`--quit` 与托盘图标真的被删除（30 次启停不漏图标） |
| §22 | 应用内更新检查：未发版 404 与 `v0.1.0` 已是最新两种状态 |
| §23 | geo 数据文件：三地址回退下载、卡片显示大小与时间 |
| §24 | 测速记录：落盘、重启还在、区分 tcp/real、拒绝不覆盖 |
| §25 | 内核意外退出：外部杀掉 → 通知 + 首页退出码；主动停止 → 安静 |
| §26 | 出口 IP：直连给地址、经代理真走内核、失败不留旧答案 |
| §27 | 订阅「下次刷新」倒计时：周期、上次更新、剩余时间同屏 |
| §28 | 订阅改名与过滤：刷新时先过滤再改名，替换上一次那一批 |
| §29 | 内核二进制：换一个文件就真的跑那个文件，geo 数据回落到 engines |
| §30 | 节点二维码：前端出码、与「复制链接」同源、失败不留白 |
| §31 | 内核崩溃：通知之前先交还系统代理，机器不会留在断网状态 |
| §32 | 规则/分组/规则集保存后提示「内核还在用旧的」，一键重启生效 |
| §33 | 端口改保存后提示「内核还在旧端口上跑」，一键用新端口重启并重设系统代理 |
| §34 | 日志级别改保存后提示「内核还在用旧级别」，一键用新级别重启，渲染配置随之改变 |
| §35 | 节点没了就停内核：删除即停、刷新换 ID 则跟随、代理同时交还 |
| §36 | 保存系统代理开关/例外清单时，正在跑的内核立刻接管或交还 |
| §37 | 一条提示说全「内核还在用旧设置」：端口、日志级别、局域网连接、DNS |
| §38 | 多路复用：一条全局开关同时写进 Xray 的 `mux` 与 sing-box 的 `multiplex` |
| §39 | 分片：`tlsSettings.fragment`（`packets`/`length`/`interval`），只写进 TLS 分支 |
| §40 | 改正在跑的节点：说出「内核还在用旧的一份」，一键按新配置重启 |
| §41 | 全局快捷键可改可关：录制、保存即生效、两键不许相同 |
| §42 | 第三种语言：机械转繁体 + 台湾用词表，选择器落库，托盘与通知跟着换 |
| §43 | 不启动内核也能测真实延迟：临时内核打一枪就撤，拒绝与失败分开记 |
| §44 | 换代理模式也要说一声：内核自报模式，一键按新表重启 |
| §45 | 测试地址只对 sing-box 有意义的提示；顺带修好带 geo 规则时 sing-box 起不来 |
| §46 | geo 数据换了、跑着的内核还在用旧的：说出来并一键重载重启 |

已提交的提交序列（新→旧，14 条主体）：
`5a44af3` 域名正则+geosite · `569a7ce` 流量历史 · `44b857b` 测速预算+选最快 · `42f85bd` 一键断开+进程名 · `f817726` 真实延迟 · `f3fc750` 订阅走代理 · `d892b9e` Clash 导入带组和规则 · `6851775` 托盘子菜单 · `f9a0406` H4 验收文档 · `14ddb20` hysteria2/tuic · `fdd0b8d` 运行中热切换 · `eb3f453` 日志页 · `79ccadd` 设计修复（粘右操作列、错误用红、焦点陷阱等）· `535124b` 快捷键与诊断包验收。

`D:\tmp\parity-gaps.md` 是最初的对标审计（31 行已覆盖、7 高价值、13 锦上添花、11 明确不做），H1–H7 与 N1、N2、N8、N9、N12 已吃掉其中大部分。

---

## 5. 最近完成的事（已提交、已真机验收）

**测试在空载与满载下不是一回事（2026-11-24）**：`cargo test --lib` 在开发机空载时连续 7 次全绿（372 passed，默认 16 线程与 4 线程都一样），但在两个进程同时抢盘（一边跑 `npm run check`、一边连跑三轮 cargo）的那一次里出现 13 条假失败，全部集中在 `commands::node`（`["a","b","a"]`、`["b","b"]`）、`commands::ruleset`（`loaded_sets` 多出一条）、`commands::subscription`（换个新 id 的采纳断言、`nothing was installed`）。同一批用例单独跑 `cargo test --lib commands::node` 连跑 6 次全过，所以不是断言写错，而是并发写盘时 `target/test-tmp` 下各用例自己的目录仍被拖慢到一个边界上。判据：先空载复跑；仍红才算回归，红了先单独跑那个模块。

**不启动内核的真实延迟（2026-11-24，§43）**：此前「真实延迟」只有一条路——经正在跑的内核打一次探测（`node_delay`），没启动内核时只能提示「先启动内核」。H5(b) 就是补这一半：`src-tauri/src/engine/temp.rs` 起一个一次性内核（自己挑两个空闲端口、渲染只含该节点、`mode=Global` 的配置、`XRAY_LOCATION_ASSET` 与主内核同源、`CREATE_NO_WINDOW`），轮询到混合端口监听后经 `http_latency_via_proxy_with_timeout` 打一枪，`Cleanup` 的 `Drop` 负责 kill + 删 `probe-*.json/.err`。命令 `node_delay_offline`（前端 `api.nodeDelayOffline`）在内核运行中直接拒绝（「直接用「真实延迟」经它测」），按 `settings.last_engine` 选内核并先 `validate_node()`，所以 hysteria2/tuic 配 Xray 会得到和首页一样的人话。三种结局分开记：`Measured` 写数字、`Failed` 写 `delay_ms: null`、`NotStarted`（拒绝）一个字节都不写。节点页在没有内核时每行多一个「真实延迟」文字按钮，成功后用页面原有的提示条说「真实延迟 539 ms」。真机证据（`offline-accept.txt`）：本机 sing-box 假服务端 `127.0.0.1:8388`，`SS-Local` → 界面 `539 ms · 刚刚`、store `{"delay_ms":539,"kind":"real"}`；`SS-Dead` → 界面 `经代理请求返回 HTTP 503`、store `{"delay_ms":null,"kind":"real"}`；内核在跑时 `real delay buttons while running: 0`；事后 `dir probe-*.json` 为空、无残留进程、`ProxyEnable=0`。`cargo test --lib` 372（新增 4 条），vitest 96，i18n 754×3。剩下的大候选表里只剩设计审计残余与明确不做的 S1–S11。

**测试地址也要说一声，而且 sing-box 曾经根本起不来（2026-11-24，§45）**：`Settings.test_url` 只被 sing-box 的 `urltest` 分组读走，Xray 渲染器完全不看它，所以 `EngineStatus.test_url` + 设置页仅在 `engine.kind === 'sing-box'` 时比较，才是诚实的。查这条时撞上一个更大的 bug：`render.rs` 给 sing-box 也写了 `"geosite"`/`"geoip"`，而 sing-box 1.8 起弃用、1.12 起删除该字段——**默认规则表里就有一条 geosite，所以任何用户切到 sing-box 都必然启动失败**，界面只报「8 秒内没打开控制端口 9090」。修法分三层：改成规则集引用；`download_detour: direct`（不然下载走占位节点，回 404）；最终不让内核上网，改由应用启动前把 `.srs` 下到 `rule-sets/geo-srs/`（`geo::rule_set_candidates` + `runtime::prepare_geo_rule_sets`），内核只读本地文件（IPv6 AAAA 先解析、云电脑无 IPv6 路由时内核自己下会吊死）。顺带修了 `ensure_free`：端口处于 `TIME_WAIT` 时不再说「另一个代理客户端还在跑」。真机证据（`testurl-accept.txt`）：`config remote rule sets: 0/1`、`stale: 内核还在用旧设置：测试地址`、`config probe url while stale: gstatic`、`sing-box pids before=6620 after=4760`、`config probe url after restart: http://cp.cloudflare.com/`、`VERDICT listed/restarted/applied/restored: True/True/True/True`；箱上 `rule-sets\geo-srs\geosite-category-ads-all.srs` = 8,175 字节。`cargo test --lib` 376，vitest 98，i18n 758×3。

**geo 数据也要说一声（2026-11-24，§46）**：`geoip.dat`/`geosite.dat` 只在启动时读一次，所以在「规则集」页点「更新 geo 数据」时若内核在跑，它手里还是旧文件。`update_geo_assets` 现在返回 `GeoUpdate { lines, updated, restart_needed }`（`updated` 只在真的装上时自增，`stale_data_needs_restart(updated, running)` 有单测），规则集页据此显示「geo 数据已更新，但内核还在用旧的一份」+ 按钮「重载 geo 并重启内核」，复用 `StaleConfig`。真机证据（`geostale-accept.txt`）：`kernels: xray#9120` → `geo 数据已更新 · geoip.dat 已更新（15.7 MB） · geosite.dat 已更新（11.0 MB）` → `notice present: True` → `kernels after restart: xray#7376 (was xray#9120)` → `notice after restart: False` → `VERDICT listed=True restarted=True cleared=True`。计数：377 Rust，758 i18n×3。

**代理模式也会落后（2026-11-24，§44）**：`Settings.mode` 只在启动时被渲染成路由表，`EngineStatus` 原先不带它，所以内核在跑时把「规则」改成「全局」，界面一条提示都没有、旧表继续跑。现在 `Running.mode`/`EngineStatus.mode` 和端口、日志级别、DNS 等一样由内核自报，设置页的 `staleItems` 末尾加上代理模式（i18n 755×3）。顺带修了 `reapply()` 的一个真缺陷：它从设置镜像读 `last_node_id`，而镜像可能指向刷新后已换 id 或已删除的节点，于是提示里的按钮点了毫无反应——现在先问内核（`status.node_id`），镜像只当兜底。真机证据（`mode-accept.txt`）：`routing rules before: 3` → 保存全局 → `stale: 内核还在用旧设置：代理模式`、`routing rules while stale: 3` → 点按钮 `pids before=7164 after=2552`、`routing rules after restart: 1`、`stale after restart: 0` → 改回规则并重启 `routing rules restored: 3`；全程 `ProxyEnable=1 ProxyServer=127.0.0.1:7890`。`cargo test --lib` 373，vitest 98。

**第三种语言（2026-11-24，§42）**：此前只有简体与英文两份字典，`Language` 也只有 `Zh`/`En`。现在多了繁体（台湾用词）：`scripts/i18n-pairs.json` 每行四格 `[key, zh, en, tw]`，`python scripts/gen_i18n.py` 一次写 `src/i18n/{zh,en,tw}.ts`，繁体列由 `zhconv` 机械转换 + 用词表（`默认→預設`、`设置→設定`、`端口→埠`、`保存→儲存`、`配置→設定` …），不是逐键手翻，所以三份字典键集由 `i18n.test.ts` 的 `keyDiff` 卡死。`src/i18n/index.ts` 的 `Locale` 加 `'zh-tw'`，`applyDocumentLang` 写 `zh-TW`。后端 `Language` 加 `ZhTw` 并单独 `#[serde(rename = "zh-tw")]`（`rename_all = "lowercase"` 会拼成 `zhtw`，与前端字典键不一致）；`tray.rs` 与退出通知各有繁体分支。顺手补了一个协调性缺口：以前选择器只写 `localStorage`，`settings.language` 从不落库，于是托盘菜单和内核退出通知在英文界面下仍说中文——新增窄命令 `set_language`（写字段 + `persist()` + `refresh_tray`），前端 `chooseLocale()` 同时调它。真机证据（`lang-accept.txt`）：`picked zh-tw: True` → `store language after zh-tw: 'zh-tw'` → `save button is 儲存: True`、`texts zh-tw: … 設定 … 埠、模式與系統集成 …`、`home nav in zh-tw: True` → `language after quit: 'zh-tw'` → 重启后 `after restart texts: … 首頁 …`、`after restart home in zh-tw: True` → 换回 `store language after zh: 'zh'`、`home nav back in zh: True`。`cargo test --lib` 368，vitest 96，i18n 750×3。

**快捷键（2026-11-24，§41）**：`Ctrl+Shift+X` / `Ctrl+Shift+W` 此前是 `src-tauri/src/hotkeys.rs` 里的常量，设置页只能读。现在两个键都是设置项（`Settings.hotkey_toggle` / `hotkey_window`，`#[serde(default)]` 指回同一批常量，老 store 照读），设置页用「点一下，再按下新的组合键」录制（`src/services/hotkey.ts` 的 `comboFromKeyPress()`），后端 `Combo::parse()` 宽松接受大小写与顺序、落库统一写法，`validate()` 拒绝空键、重复修饰键、多个真键，以及两个键相同（`两个快捷键不能相同（都是 {label}）`）。`save_settings` 末尾调 `hotkeys::apply()`，用 `PostMessageW(WM_APP_REREGISTER)` 把重注册交给热键消息线程，所以保存即生效、不用重启应用；注册失败只写日志不拦保存。真机证据：录 `Ctrl+Alt+F9` 保存后立刻 `kernels after new chord: xray#3652`，旧键 `kernels after old chord: xray#3652 (was xray#3652)` 无反应，重启后 `kernels after restart chord: xray#5940`，`关掉这一项` 后旧窗口键不再藏窗口，两个「恢复默认」把 chip 还原成 `Ctrl+Shift+X` / `Ctrl+Shift+W`。`cargo test --lib` 366（新增 9 条），vitest 95（新增 `hotkey.test.ts` 9 条）。验收过程中修掉一个真死锁：`window_proc` 持着 `BINDINGS` 锁再调 `register_all()`，而后者又去锁同一个 `std::sync::Mutex`。

**改节点（2026-11-24，§40）**：编辑正在跑的那个节点，`upsert_node` 只落库不重启内核，所以以前保存完界面说「已保存」、内核却还在用旧参数。做法是纯前端的：`src/services/node-stale.ts` 的 `rendersDifferently(before, after)` 只比 `server`/`port`/`protocol`/`transport`/`security`（改名、改标签不算），`NodesView.vue` 在 `engine.running && engine.status?.node_id === saved.id` 时置 `staleNode`，复用 `StaleConfig` 说一句 `节点改好了，内核还在用旧的一份` 并给一个 `用新配置重启内核`（调 `engine.reapply()`），`started_at_ms` 一变就自己消失。真机证据：端口 443→8443 保存后 `notice present: True`，点按钮后 `kernels after restart: xray#1008 before=xray#1992`、`rendered server port: 8443`、`notice gone: True`、系统代理一直 `127.0.0.1:7890`。vitest 86（新增 `node-stale.test.ts` 8 条）。

**分片（2026-11-24，§39）**：V2rayN 的「分片」在本项目里此前连字段都没有。做成全局设置（`Settings.fragment_enabled` / `fragment_length` / `fragment_interval`，`#[serde(default)]` 老 store 照读），由 `RenderInput::fragment() -> Option<(String, String)>` 这个唯一判断点决定要不要写，`src-tauri/src/engine/render.rs` 只在 `Security::Tls` 分支插入 `"fragment": {"packets": "tlshello", "length": ..., "interval": ...}`；Reality 与 None 分支各有测试断言这个键不在。范围用 `parse_positive_range` 在 `validate()` 里校验（`a-b`、`a<=b`、不为 0），保存即拒绝而不是渲染时悄悄兜底；`EngineStatus` 也带上这三个值，接进 §37 的提示（`settings.stale.itemFragment`）。真机证据：`store after save: fragment=true length=120-220 interval=20-40` → `stale: 内核还在用旧设置：分片` → 重启 `xray#4672→#1464` → `render fragment key: 1`、`render packets tlshello: 1`、`kernel alive after fragment restart: 1`（配置被内核接受）→ `stale after restart: 0` → 关掉保存 `stale back: 1` → 重启 `render fragment keys restored: 0`。`cargo test --lib` 357（新增 9 条）。sing-box 侧未做：它的等价物不是猜出来的，猜错会让内核拒绝整份配置。

**多路复用（2026-11-24，§38）**：V2rayN/Clash 都有 mux，本项目此前连字段都没有。做成全局设置（`Settings.mux_enabled` / `mux_concurrency`，`#[serde(default)]` 老 store 照读）而不是逐节点参数，因为它和端口、日志级别一样属于「内核怎么说话」这一层，顺带接进了 §37 的那一条提示（`settings.stale.itemMux`）。两个渲染器都从 `RenderInput::mux_streams() -> Option<u16>` 取唯一答案：`None` 就整块不写，`Some(n)` 时 Xray 写 `"mux": {"enabled": true, "concurrency": n}`、sing-box 写 `"multiplex": {"enabled": true, "protocol": "smux", "max_streams": n}`；`EngineStatus` 也带上这两个值，所以设置页能说出「内核还在用旧设置：多路复用」。真机证据：`store after save: mux=true concurrency=12` → `stale: 内核还在用旧设置：多路复用` → 重启 `xray#8600→#1676` → `render mux enabled: 4`、`render mux concurrency 12: 4`、`stale after restart: 0` → 关掉再保存 `stale back: 1` → 重启 `render mux keys restored: 0` → 停止后 `ProxyEnable=0`、内核 0。`cargo test --lib` 348（新增 4 条）。本节只证明「开关 → 渲染配置 → 重启生效」，不证明 mux 真的更快。

**一条提示说全四件事（2026-11-24，§37）**：设置页原本有两条几乎一样的提示（端口、日志级别），而同样写进渲染配置的「允许局域网连接」和「DNS」一个字都不说。现在 `EngineStatus` 多带 `allow_lan` 与 `dns_servers`（`start()` 在 `with_options` 之前抄进 `Running`，DNS 保留设置原文 `RunOptions::dns_servers_raw`，因为解析后的列表拿不回空格/尾逗号），`SettingsView.vue` 的 `staleItems` 按面板顺序列出四项、`staleMessage` 用「、」拼成一句，页面只剩一个 `<StaleConfig>` + 一个「用新设置重启内核」。真机证据：`stale: 内核还在用旧设置：端口、日志级别、局域网连接、DNS`（`stale lines: 1`）→ 重启 `xray#9072→#8968` → `proxy now: ProxyServer=127.0.0.1:7891`、`render loglevel: warning`、`render dns has 9.9.9.9: True`、`render lan listen 0.0.0.0: True` → `stale after restart: 0` → 停止后 `ProxyEnable=0`、内核 0。`cargo test --lib` 344（新增 2 条）。

**系统代理开关即刻生效（2026-11-24，§36）**：`save_settings` 多了 `runtime`/`proxy` 两个参数，写盘之后按 `takeover_change(before, after)`（纯函数，6 条单测）决定 `enable_with_bypass` 还是 `disable`；接管用的是**正在跑**的 `status.mixed_port`（为 0 才退回设置里的值），因此「先改端口再改开关」不会指向空端口。前端 `SettingsView.vue` 的 `save()` 在 `refreshAutostart()` 之后补了 `proxy.refresh()`，页面下方的「系统代理当前」角标跟着变；`settings.proxyIntent` 文案改成「内核在跑时立刻生效」。真机证据：`TAKEN ProxyEnable=1 ProxyServer=127.0.0.1:7890` → 关掉保存 `RELEASED ProxyEnable=0`（内核仍在）→ 再打开保存 `RETAKE ProxyEnable=1` → 改例外 `override now: …;*.example.com` → 还原 → 停止后 `ProxyEnable=0`、内核 0。`cargo test --lib` 344。

**改了日志级别就说出来（2026-11-24，§34）**：`EngineStatus.log_level` 由 `Running.log_level` 给出（`start()` 时塞进 `RenderInput` 的那个值，不是回头读设置），`SettingsView.vue` 的 `staleLog` 比较「已存设置」与它，仍用 `StaleConfig`（覆盖文案两处）。真机证据：`store=debug / render=debug` → 点「警告」保存 → `notice present=True` → 重启 `xray#1224→#2868`、`configs/xray.json` 的 `"loglevel":"warning"`、提示自清；再改回 `debug` 同样成立；停止后无内核。单元测试 2 条，`cargo test --lib` 344。

**节点没了，内核就得停（2026-11-24，§35）**：`EngineStatus.node_id` 让前端知道内核在跑哪个节点；`commands::engine::stop_if_running_node` 先 `proxy.disable()` 再 `runtime.stop()`；`delete_node(s)` / `apply_delete_subscription` 清掉 `settings.last_node_id` 并调用它。订阅刷新时 ID 必变，所以 `apply_document` 换表**前**从旧行取下 `Node::fingerprint()`，换表后找到同指纹的替身就 `adopt_node_id`（内核不动），找不到才算真的消失。真机证据：删掉正在跑的节点→内核空、`ProxyEnable=0`、`last_node_id` 无、页面提示；刷新同一订阅（源内容倒序）→ `xray#4784` 不变、`last_node_id` 从 `vless-18eac9ae1ba07634-0` 变成 `vless-18ead5d985cb4444-3`、无误报提示、代理仍在。`cargo test --lib` 344。

**改了端口就说出来（2026-11-24，§33）**：设置页保存端口后，内核仍绑旧端口，`SettingsView.vue` 用 `stalePorts` 比较「已存设置」与 `engine.status`（非草稿），复用 `StaleConfig`（新增 `message`/`applyLabel`/`busyLabel` 三个可覆盖 prop），按钮走 `engine.reapply()`。重启由 `start_engine` 自己完成：启动成功后 `proxy.enable_with_bypass(..., endpoints.mixed_port, …)` 会在已接管时重写注册表，所以新端口自动生效。真机证据：`xray#5072→#1636`、`ProxyServer=127.0.0.1:7891`、改回 7890 后 `:7890`、停止后 `ProxyEnable=0`。

**崩溃时交还系统代理（2026-11-24，§31）**：`ProxyGuard::release_if_active()`（`src-tauri/src/sysproxy/mod.rs`）只在「快照证明是我们接管的」时还原并删快照；`watch_engine_exits` 在 `app.emit("engine://exited")` **之前**调用它，通知文案只在真交还成功时追加「系统代理已交还，网络已恢复直连」，前端 `src/stores/engine.ts` 的 `watchExits` 顺带刷新系统代理角标。真机证据：`ProxyEnable 1→0`、`app.err.txt` 的 `内核退出后已交还系统代理`、首页回到 `直连（未经代理）`。单元测试 2 条（活动/非活动两条分支），`cargo test --lib` 320。

**N12 应用内更新检查**（`docs/ACCEPTANCE.md` §22）：仓库发布 `v0.1.0` 后「关于 → 检查更新」显示「已是最新版本（0.1.0）」，未发版时显示「发布页还没有发布记录（HTTP 404）」。

- `src-tauri/src/update.rs`：`FEED_URL = https://api.github.com/repos/linjunhao024-byte/X-Matrix-Client-Pro/releases/latest`、`FEED_TIMEOUT=10s`；`UpdateStatus{current,latest,has_update,url,notes}`；`parse_version` 去 `v` 前缀、缺位补 0、`1.3.0-beta.1` 记 1.3.0；**任一侧解析失败一律「不算有新版本」**；非 JSON / 空 tag 给人话错误；`explain_status` 把 404 翻成「发布页还没有发布记录」。
- `check_update` 走 `spawn_blocking`（不在 IPC 线程开 socket）；`app_open_url` **只放行 `http(s)`**。
- 关于页 `updateFailed` 与页面级 `error` 分开：「问不到发布页」是 muted 小字，不刷红。
- 文案归属：`fetch_answer(..., noun)` 让同一套请求逻辑能说「订阅」或「发布页」，别让它替别人道歉。

**出口 IP**（`docs/ACCEPTANCE.md` §26）：
- 命令 `exit_ip`（`src-tauri/src/commands/system.rs`）先按 `runtime.status().running` 决定路线（`via=proxy` 走 `127.0.0.1:mixed_port`，否则 `direct`），再在 `spawn_blocking` 里依次问 `ipv4.icanhazip.com` → `ifconfig.me/ip` → `ipinfo.io/ip`，首个 2xx 且能解析成 IP 的回答即返回。
- 失败行固定形如 `出口 IP 都问不到：{host} {reason}；…`；全部失败才报错，单个主机失败不影响下一次尝试。
- 前端 `src/views/HomeView.vue` 卡片：地址 + `经代理`/`直连（未经代理）` 徽章 + 更新时间；**失败即清空 `exit`/`exitAt`**，只留错误行——旧地址配新错误会冒充一条没验证过的路线。

**内核意外退出通知**（`docs/ACCEPTANCE.md` §25）：

- `EngineRuntime` 增 `unexpected_exit: Mutex<Option<i32>>` + `take_unexpected_exit()`（取走即清）；`Running.expected` 在 `stop()` 里、`kill()` **之前**置位，所以主动停止与 `--quit` 永不通知。
- `lib.rs` 的 `watch_engine_exits` 每秒调一次 `status()`，拿到非预期退出就 `app.emit("engine://exited", code)` + `notify_engine_exit`（`tauri-plugin-notification`，中英双语，失败只打一行 stderr）；`--quit` 路径根本走不到这个线程。
- 前端 `src/services/events.ts`（`listen` 只允许出现在 `src/services/`，架构检查会拦）+ `src/stores/engine.ts` 的 `watchExits()` 单飞刷新。
- 成功与失败都打日志：「已发送」那一行是测试机通知被系统关掉时唯一可验证的证据。

---

## 6. 下一步候选（按「性价比 / 风险」排序）

| 候选 | 落点 | 代价 | 备注 |
|---|---|---|---|
| ~~N3 节点二维码~~ | 已完成：前端 `qrcode` npm 包出 SVG，节点行「二维码」按钮 + 对话框（§30） | — | 原计划要走 Rust 与 `qrcode` crate，但本机 cargo 缓存里没有；npm 可用，于是改为纯前端，零新增 IPC |
| ~~N10 订阅重命名 / 过滤~~ | 已完成：`rename_prefix` + 保留/排除正则，刷新时先过滤再改名（§28） | — | 见「已完成」说明 |
| ~~N11 内核二进制选择~~ | 已完成：`settings.xray_binary` / `singbox_binary` + 原生对话框 + 版本校验 + geo 数据回落 engines（§29） | — | 换内核时暴露了 `XRAY_LOCATION_ASSET` 的定位错误，已一并修 |
| ~~N13 更多语言~~ | 已完成：`scripts/i18n-pairs.json` 四格 `[key, zh, en, tw]`，繁体由 `zhconv` + 台湾用词表机械转换，`Language::ZhTw` + 托盘/通知分支 + `set_language` 落库（§42） | — | 剩 ja 等语言可按同一流程加；键集由 `i18n.test.ts` 卡住 |
| ~~H5(b) 无内核时的逐节点真实延迟~~ | 已完成：`engine/temp.rs` 起一次性内核 + `node_delay_offline`，拒绝与失败分开记（§43） | — | 当初列为「风险高，先不做」，实际只需一条受控的临时生命周期 |
| ~~N5 出口 IP~~ | 已完成：首页卡片 + 三主机退让 + 失败清空旧值（§26） | — | 见下方「已完成」说明 |
| ~~N7 订阅下次刷新倒计时~~ | 已完成：每行「刚刚 / 还有 N 小时」，30 秒共用计时器（§27） | — | 见「已完成」说明 |
| ~~§47 订阅刷新后前端握着死 ID~~ | 已完成：`src/views/SubscriptionsView.vue` 的 `kernelStopped()` 无条件重读设置，验收见 §47 | 小（1 文件 + 2 轮真机对比） | 2026-11-25 关闭 |
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
12. **测试机默认关掉通知**：`ToastEnabled=0` 时插件仍返回 `Ok` 但屏幕上没有横幅，肉眼验收会误判。要么先 `reg add HKCU\Software\Microsoft\Windows\CurrentVersion\PushNotifications /v ToastEnabled /t REG_DWORD /d 1 /f`，要么以 stderr 那行「内核退出通知已发送」为判据。Windows Server 2022 即使开了也不弹横幅（只进通知中心），**枚举窗口找不到 toast 是正常的**。
13. 追加带反斜杠/中文的文档段落：先用 `write` 落一个临时 `.md`，再用 python 按字节拼接（heredoc 会把 `\t`/`\n` 吃成 TAB 与断行）。

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

1. `cd /d/GitHub-Warehouse/X-Matrix-Client-Pro && git status` —— 工作区应当干净；有改动先弄清是谁的。
2. 读本文件的 §5 与 `docs/ACCEPTANCE.md` 的最后两节，对齐上下文。
3. `export PATH="$HOME/.cargo/bin:$PATH"`；`cd src-tauri && cargo test --lib`（应为 394 passed）。
4. `cd .. && npm run check`（应为全绿）。
5. 从 §6 表里挑一项（按性价比排序），**一项一提交**，提交信息用英文 `feat(...)/fix(...)/docs: ...`。
6. 真机验证前先跑 `bash /d/tmp/shots/deploy-tray.sh <exe>`；远端被 `cleanup.sh` 清过的话，先 `scp x-act.ps1 uia2.ps1 ocr.ps1` 上去。
7. 验完写 `docs/ACCEPTANCE.md` 新的一节（**验法 / 判据 / 设计理由 / 坑** 四件套），并同步 `README.md:81`、`ARCHITECTURE.md:277` 的命令数与测试数。
8. 收尾 `bash /d/tmp/shots/cleanup.sh`，再手查 `ProxyEnable` 与 `schtasks /query | findstr xm` 确实是空的。

---

## 10. 一句话交接

**能用的东西已经都在了，剩下的都是边角。**接着做的人只需要：从 §6 表挑一项，按「实现 → 单测 → `npm run check` → 构建 → 云电脑真机 → 写 ACCEPTANCE → 提交 → 清理」这套流程走完，并且始终记得 §7 里那些验收环境的坑。

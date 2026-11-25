# 交接清单（X-Matrix Client Pro）

> 写于 2026-11-23 11:20（Asia/Shanghai），交接时的真实状态。工作区：`D:\GitHub-Warehouse\X-Matrix-Client-Pro`
> 目标（用户原话，长期有效）：**功能与前端设计追平 V2RayN / Clash**，做得比它们更干净。

> **刚接手这个项目？先读 `docs/HANDOFF-BRIEF.md`（一页纸：现状、四条命令、下一个任务、坑）。** 本文是全书。

---

## 0. 三句话现状

1. 主体功能已追平 V2RayN / Clash 的日常面：双内核（Xray / sing-box）渲染与启停、系统代理接管与还原、订阅/整包导入、Clash 配置连同 **策略组与规则** 一起导入、运行中热切换节点与代理模式、托盘子菜单、日志页、hysteria2 / tuic、真实延迟（分组与节点）、流量历史落盘、一键断开全部 + 进程归属、测速预算与「选最快」、域名正则 / 域名分类匹配器、诊断包导出、全局快捷键。
2. 验收方式是**真机**：每一条都跑到云电脑 `100.106.121.0`（Windows Server，Console 会话）上点出来、读日志/读配置文件核对，结论写进 `docs/ACCEPTANCE.md`（现有 §0–§59）。
3. 交接时那处未提交的工作（N12 应用内更新检查）已经构建、真机验收并提交；此后又合入四件事：`--quit` 正常退出并删除托盘图标（§21）、GEO 数据文件卡片与镜像回退下载（§23）、测速记录落盘（§24）。工作区现在是干净的。

### 0.1 进度快照（交接这一刻的硬数字）

| 项 | 值 |
|---|---|
| 代码 HEAD | 「feat(profiles): several configurations, one kernel at a time」，含 §59（S7 多配置）；前一条是「feat(tun): take over the machine at the adapter, and only when sing-box and admin can do it」（§58）；前一条是「feat(service): let a Windows service own the kernel, so it can run as SYSTEM」（§57）；再往前：§56（S4a 提权）、§54（S6，查 `src-tauri/src/engine/release.rs`/`net/webdav.rs`）、§53（S3，查 `src-tauri/src/engine/release.rs`）、§52（S9）、§51（S11）、§50（S10）、§49（S8）、`a794438`（§48）、`6fd790f`（§47） |
| 未提交 | 无。§59（S7 多配置）的代码、构建、真机验收、`docs/ACCEPTANCE.md` §59 与 `docs/PARITY-PLAN.md` 都在同一次提交里（见 §0.15） |
| 上一版未提交清单（留档） | `src-tauri/src/domain/settings.rs`、`src-tauri/src/engine/{mod,render,runtime,xray}.rs`、`src/services/bindings.ts`、`src/views/SettingsView.vue`、`src/i18n/{zh,en}.ts`、`scripts/i18n-pairs.json`（§39 分片）；再上一版：`src-tauri/src/engine/{mod,runtime}.rs`、`src/services/bindings.ts`、`src/views/SettingsView.vue`、`src/i18n/{zh,en}.ts`、`scripts/i18n-pairs.json`（§37 统一提示）；再上一版：`src-tauri/src/commands/settings.rs`、`src-tauri/src/commands/sysproxy.rs`、`src/stores/sysproxy.ts`、`src/views/SettingsView.vue`、`src/i18n/{zh,en}.ts`、`scripts/i18n-pairs.json`、`docs/ACCEPTANCE.md`（§36 系统代理即时生效） |
| 本文档 | 随 `docs: a handover map of what is done, what is half done, and what is next` 提交入库；之后每次收尾都跟着更新 |
| IPC 命令 | **97**（`README.md:81`/`ARCHITECTURE.md:277` 已同步） |
| Rust 单测 | **514 passed**（`ARCHITECTURE.md:278` 已同步） |
| 前端检查 | `npm run check` 全绿：vue-tsc / architecture（68 文件）/ contract / vitest **116**（12 文件） |
| i18n | **856** 键 × 3 份字典（简/繁/英，`scripts/i18n-pairs.json` → `python scripts/gen_i18n.py`） |
| 桌面产物 | 桌面上的旧副本**已不存在**；唯一产物是 `src-tauri/target/release/x-matrix.exe`（11,056,640 B @2026-11-25 18:26，md5 `907a38674359352f22cddcb9c61c627f`，含 §21–§59 全部改动） |
| 验收文档 | `docs/ACCEPTANCE.md` §0–§59（§7 历史空缺，编号不连续但内容完整） |
| 云电脑 | 干净：`ProxyEnable=0x0`、无 X-Matrix/sing-box/xray 进程、无 `xm*` 计划任务 |

**下一步的确定顺序**（做完一条打一个勾，别跳）：

- [x] 1. `npm run tauri build`：`--quit`、N12、GEO、测速落盘、退出通知各一轮
- [ ] 2. `cp` 刷桌面两件 + `cmp` 校验（**唯一常被忘的一步**）
- [x] 3. 云电脑真机验收：§21 退出、§22 检查更新、§23 geo、§24 测速、§25 退出通知、§26 出口 IP
- [x] 4. `docs/ACCEPTANCE.md` 写到 §26
- [x] 5. 提交（`fix(tray)` / `feat(update)` / `feat(geo,nodes)` / `feat(engine)` 各一条）
- [x] 6. `FEED_URL` 已指向真实仓库 `linjunhao024-byte/X-Matrix-Client-pro`，`v0.1.0` 已发布并带 `X-Matrix.exe` 资产
- [x] 7. 计数已同步：92 命令 / 489 单测 / 116 vitest / 829 i18n 键 × 3
- [x] 8. `bash /d/tmp/shots/cleanup.sh` 收尾云电脑（2026-11-24 23:55 再跑一次：`ProxyEnable 0x0`、无 `xm*` 任务）
- [x] 9. §47 订阅刷新后前端握着死 ID：补丁 → 构建 → 云电脑两轮对比验收 → 写 §47 → 提交（2026-11-25 02:36）
- [x] 10. §48（S2）PAC 自动配置：生成脚本 + 注册表互斥 + 回环/私网兜底 → 构建 → 云电脑验收 → 写 §48 → 提交（2026-11-25 04:20）；后续 S10/S11/S9/S3/S6/S5/S4/S1/S7 见 `docs/PARITY-PLAN.md`
- [x] 11. §49（S8）HTTP 与 SOCKS 分离端口：`Endpoints` 一处决议 + 两个渲染器 + 界面生效值 → 构建 → 云电脑三阶段验收 → 写 §49 → 提交（2026-11-25 06:5x）
- [x] 12. §50（S10）六个强调色 + 自定义 hex：`tokens.css` 每预设两块 + `accent.ts` 现算对比度 → 构建 → 云电脑像素计数验收 → 写 §50 → 提交（2026-11-25 07:0x）
- [x] 13. §51（S11）进程选择器：`sysproc.rs` 归并 + 一条 `list_processes` + 规则编辑器第二个弹窗 → 构建 → 云电脑 57 行真实进程/收窄/回读/空态验收 → 写 §51 → 提交（2026-11-25 07:3x）
- [x] 14. §52（S9）回退组与负载均衡组：`groupwatch.rs` 每 20 秒探健康、只改 selector → 构建 → 云电脑两个会话（sing-box 三轮、xray 一轮）验收 → 写 §52 → 提交（2026-11-25 08:3x）
- [x] 16. §54（S6）WebDAV 云备份：`net/webdav.rs` + `syscreds.rs` + `save_settings` 携密码 → 构建 → 云电脑五段验收（上传 4889 B、恢复把删掉的节点带回来、内核在跑时拒绝且不联网、密码错落成 401 一句话、走代理但内核没跑先拒）→ 写 §54 → 提交（2026-11-25 10:5x）
- [x] 17. §55（S5）轻量模式：`power.rs` 两半记账 + `set_quiet()` 接三藏一显 + 前端 `quiet` 停表 → 构建 → 云电脑两阶段（对照/实验）验收 → 写 §55 → 提交（2026-11-25 12:4x）
- [x] 18. §56（S4a）提权：`elevate.rs` 令牌判断 + `ShellExecuteEx` runas + `--relaunch-wait` 握手 + 设置页「权限」节；判据 7、8（非管理员文案、真提权）在这台机器上造不出条件，已如实标未验
- [x] 20. §59（S7）多配置：`store.json` v2（激活内联 + 非激活每份一个文件）+ 五条 profile 命令 + `ProfilesView` + 备份带全部配置 → 构建 → 云电脑 v1 升级/两向切换/云备份三段验收 → 写 §59 → 提交（2026-11-25 18:4x）
- [x] 19. §58（S1）TUN：`Settings.tun_enabled/tun_stack` + sing-box tun 入站与 `sniff` + `start_engine` 三条渲染前拒绝 + 界面开关 → 构建 → 云电脑四阶段验收（两条拒绝、网卡与路由接管、隧道 204、停止复原）→ 写 §58 → 提交（2026-11-25 17:4x）
- [x] 15. §53（S3）内核下载/更新：`release.rs` 解析 + sha256 核对 + `stage/promote` → 构建 → 云电脑三阶段验收（xray 真换文件并用同节点重启、sing-box 无校验值照装但明说未核、自定义路径拒绝且不下载）→ 写 §53 → 提交（2026-11-25 10:10）

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

### 0.5 §49（S8）HTTP 与 SOCKS 分离端口（已完成，2026-11-25）

- **为什么做**：`docs/PARITY-PLAN.md` 的第 2 项。以前只有一个 `mixed` 入站，HTTP 与 SOCKS 挤在 7890；Clash 与 v2rayN 都允许分端口，而「给别人用的」永远是 HTTP 那个。
- **做了什么**：`Settings.http_port` / `socks_port`（`#[serde(default)]`，0 = 跟 `mixed_port` 一样）；生效端口只在 `Endpoints::new(mixed_port, http_port, socks_port, api_port)` 决议一次，对外给 `http_port` / `socks_port` / `proxy_port()`；`EngineStatus` 加两个生效值。渲染：相等时仍一个 `mixed`（tag `socks`，形状与旧版一字不差），不等时 `socks` + `http` 两个入站（Xray http 带 `sniffing.destOverride ["http","tls"]`、`allowTransparent:false`；sing-box SOCKS 带 `udp:true`）。接管系统代理、PAC、订阅抓取、真实延迟、临时内核、诊断全改用 `proxy_port()`。界面：设置页两个数字框 + 提示 + 生效值行 `HTTP 7891 · SOCKS 7892`；首页在分离时把「入站端口」换成「入站端口（HTTP / SOCKS）」。
- **真机跑出来一个设计错位**：分离后没有进程再 bind `mixed_port`，但校验还拿「`mixed_port` 低于 1024 / 等于控制端口」去否决启动——等于因为一个没被使用的数字拒绝合法启动。已给这两条检查加 `!split` 前置，并加单测 `a_leftover_mixed_port_cannot_block_a_split_start` 守住两边。
- **验收**：`bash /d/tmp/shots/49-split-accept.sh`（`mk_49_split.py` 生成三阶段 PS 驱动）＋ `49-ui-accept.sh`（`mk_49_ui.py`，只读界面）在云电脑上跑通：默认单 `mixed` 7890、分离 `socks`7892/`http`7891 且 `ProxyServer=127.0.0.1:7891`、非法端口在渲染前被拦且不起内核不接管、停止后按快照还原。细节见 `docs/ACCEPTANCE.md` §49。
- **计数**：命令 81（不变）、Rust **402**（+8：子代理 5 + 我 3）、vitest 98、i18n **768**（+4）。

### 0.6 §50（S10）强调色与自定义主题（已完成，2026-11-25）

- **为什么做**：`docs/PARITY-PLAN.md` 的第 3 项。主题早就有 light/dark/system，但强调色写死在 tokens 里；Clash Verge 允许换色，v2rayN 允许换主题。
- **做了什么**：新增 `src/services/accent.ts`（六个预设 `ACCENTS` + `DEFAULT_ACCENT` + `resolveAccent` / `deriveCustom` / `relativeLuminance` / `contrastRatio`）。`src/styles/tokens.css` 每个预设两块（`:root[data-accent="…"]` 与 `:root[data-theme="dark"][data-accent="…"]`），**只覆盖 `--accent` / `--accent-hover` / `--accent-soft` / `--accent-contrast` 四个变量**，其余颜色由它们派生。自定义 `#rrggbb` 由 JS 写内联变量，切回预设时 `removeProperty`；对比度按 WCAG 2.x 现算，<4.5 拒绝并把提示改成告警色。`src/stores/ui.ts` 持久化到 `localStorage['xm.ui.accent']`（预设 id 或 `custom:#rrggbb`），**不进 `store.json`**（那是会随备份走的配置）。设置页在「语言」之前多一个「外观」段：主题三档 + 六个色板 + 自定义输入 + 应用/恢复默认。indigo 的四个值与改动前逐字节一致，`src/services/accent.test.ts` 直接读 `src/styles/tokens.css` 做双源比对（15 个用例）。
- **验收**：`bash /d/tmp/shots/50-accent-accept.sh`（`mk_50_accent.py` 生成两阶段 PS 驱动）在云电脑上跑通，证据是**像素数**：全新配置首启 4886 px `#5b5bd6`（默认观感不变）、点 violet 后浅色 5181 px `#7c3aed` / 深色 4893 px `#a78bfa`、自定义 `#0ea5e9` 3644 px 且六个预设全 `Off`、恢复默认回到 4896 px `#8f8ff8`、重启后仍是 violet。细节见 `docs/ACCEPTANCE.md` §50。
- **两个假失败要先知道**：① `恢复默认` 在 UIA 树里有 5 个同名 Button（高级项里还有两个重置），第一个在屏幕外，按名字取第 0 个会点到别处 —— 要取与唯一 `应用` 按钮同一行右边那个；② `Scroll-To` 返回的是 `{CX,CY}` 而不是 AutomationElement，对它取 `.Current.BoundingRectangle` 得到 `0,0,0x0`，于是点了标题栏。
- **计数**：命令 81（不变）、Rust **402**（不变）、vitest **113**（+15，11 文件）、i18n **778**（+10）。

### 0.7 §51（S11）按进程分流的进程选择器（已完成，2026-11-25）

- **为什么做**：`docs/PARITY-PLAN.md` 的第 4 项。`RuleMatcher::ProcessName` 早就在（Xray 渲染 `process: [...]`、sing-box 渲染 `process_name: [...]`），但用户要在规则里**手打** `chrome.exe`——Clash Verge 与 v2rayN 都能从进程表里挑。
- **做了什么**：新增 `src-tauri/src/sysproc.rs`（`ProcessInfo { name, pids }` + 纯函数 `group()` + `list()`）、`src-tauri/src/commands/process.rs`（**唯一一条新命令** `list_processes`，IPC 从此 81 → 82）、`src/services/processlist.ts`（`filterProcesses` 纯函数）。`group()` 大小写不敏感归并、保留首次拼写、pid 升序去重、按小写名排序原名破平；`list()` 走 Windows Toolhelp32（`CreateToolhelp32Snapshot` + `Process32FirstW/NextW`，句柄用 `Drop` 关，`INVALID_HANDLE_VALUE` → `AppError::Io("读取进程快照失败：…")`，跳过 pid 0 与空名，非 Windows → `AppError::Internal`）。`src/components/RuleEditor.vue` 在 `process_name` 分支里多一个 `选择进程` 按钮 → 第二个 `ModalDialog`：搜索框、`名称 (pid数)` 单行列表、loading / 后端原文错误 / 空态三态、刷新按钮、点行只写 `draft.value` 不提交；`onEditorClose()` 让 Escape 先关选择器、不连坐半填的规则。界面上如实写明：sing-box 走 `process_name` 字段，**Xray 的 `process` 匹配取决于内核版本与平台、本项目没在真机验过**。
- **验收**：`bash /d/tmp/shots/51-proc-accept.sh`（`mk_51_proc.py` 生成驱动；store 钉成只有一条 `process_name=chrome.exe` 规则，路由表只有一行，`编辑` 不歧义）在云电脑上跑通：**57 行真实进程**（`AggregatorHost.exe (1)`、`ais.exe (1)`、`ChsIME.exe (1)` …）、搜 `x-matrix` 收窄到 1 行 `X-Matrix.exe (1)`、点行后编辑器回读 `匹配内容=X-Matrix.exe`、填 `zzzznosuchprocess` 时 0 行且界面出现 `没有匹配的进程`、关选择器后编辑器仍在。细节见 `docs/ACCEPTANCE.md` §51。
- **坑（真机脚本）**：① 输入框的 UIA `Name` 就是 placeholder，所以驱动靠 `按名字过滤，例如 chrome.exe` 这段文案定位——**改这条文案会让驱动失效**，改的时候顺手改 `mk_51_proc.py`；② 列表行名带 ` (n)` 后缀，数行用 `-match '.+ \(\d+\)'`，点行要把完整名字（含 ` (1)`）传给 `NameProperty`；③ `Buttons-Named` 只有一个匹配时会解包成单个元素，`.Count` 打出来是空串（不是「按钮不存在」）；④ `reg query "…\Internet Settings"` 经 ssh 进 cmd 会被吃掉引号，末态一律由 `cleanup.sh` 证明。
- **计数**：命令 **82**（+1）、Rust **405**（+3：`group()` 三条）、vitest **116**（+3，12 文件）、i18n **784**（+6）。

---

### 0.8 §52（S9）回退组与负载均衡组（已完成，2026-11-25）

- **为什么做**：`docs/PARITY-PLAN.md` 的第 5 项。两个内核都没有 `fallback`/`load-balance` 这种出口，但 Clash Verge 用户天天用它们；不做就只能回退到手动切组。
- **做了什么**：`GroupKind` 加 `Fallback`/`LoadBalance`（Kebab-case serde，Clash yaml 的 `fallback`/`load-balance` 不再降级成 `url-test`）；新 `src-tauri/src/engine/groupwatch.rs`（`choose()` 纯函数 + `Runner::tick_once` + 500ms 分片睡眠的 `run()`）：每 20 秒对每个启用的这两类组探一次健康（走 Clash API 的 `/proxies/node:<member>/delay`，3 秒预算、同一个 `test_url` 与手动测速一把尺），Fallback 取第一个健康成员、LoadBalance 取健康集合里 `tick % len` 那个，只有和内核当前 `now` 不同才 `PUT /proxies/group%3A<id>`（body `{"name":"node:<member>"}`）改 selector；无人健康就原地不动。`net/http.rs` 加 `put_json_timeout`（带 JSON body），`stats.rs` 加 `clash_now`。sing-box 两类都渲染成原生 `selector`（`default` 是第一个成员），Xray 保持静态副本。界面：组编辑器多两个类型、组页对 Xray 明说「不回退也不轮换」、成员延迟列表覆盖这三类。
- **验收**：`bash /d/tmp/shots/52-group-run.sh`（`mk_52_b.py` 生成两个驱动；本地两个 shadowsocks 服务 + 一个死端口成员）在云电脑跑通：health 探测 `ss-dead-1` 503、`ss-local-1` delay=99、`ss-local-2` delay=111；fallback 从 `node:ss-dead-1` 一次落到 `node:ss-local-1` 后 72 秒不抖；load-balance 每 20 秒真切换（local-1 → local-2 → local-1 → local-2）；应用日志 5 条「原因：健康探测」；Xray 会话两句说明都在。细节见 `docs/ACCEPTANCE.md` §52。
- **坑（真机脚本）**：① store 里 `Protocol`/`Transport`/`Security` 的 serde 名是 **PascalCase**（`Shadowsocks`/`Tcp`/`None`），写小写会让整份文档被判 corrupt（远端落成 `store.corrupt`、应用以默认值启动）——第一轮就白跑在这上面；而 `last_engine`/`mode` 是两个小写词，两种风格并存；② `ssh` 里 `start /b` 起的本地服务会随控制台死，必须 `schtasks /sc once /st 23:59 /it` 在会话 1 里起；③ 驱动日志别用 `sed -n '1,45p'` 截断，watcher 的切换证据常在文件末尾。
- **计数**：命令 **82**（+0）、Rust **421**（+16）、vitest **116**（+0）、i18n **789**（+5）。

### 0.9 §53（S3）内核下载与更新（已完成，2026-11-25）

- **为什么做**：`docs/PARITY-PLAN.md` 的第 6 项。以前内核要用户自己放进 `engines/`、自己找下载链接、自己解压；v2rayN 与 Clash Verge 都能在界面里更新内核。这是「追平日常面」里最常被点的一次。
- **做了什么**：新增 `src-tauri/src/engine/release.rs`（`feed_url`/`asset_name`/`parse_release`/`parse_digest`/`sha256_hex`/`version_from_self_report`/`extract_exe`/`stage`/`promote`/`discard`，13 项单测）、`src-tauri/src/commands/kernel.rs`（**恰好两条**新命令 `kernel_check` / `kernel_install`，IPC 82 → 84）、`net/remote.rs` 的 `fetch_bytes_through(url, timeout, proxy, noun)`；`Cargo.toml` 只加 `zip`（deflate）与 `sha2`。`kernel_install` 的次序是钉死的：自定义路径先拒绝（**不联网**）→ 解析上游 → 下载（120 秒）→ 核 sha256（不符不落盘、报错带上游值与实得值；取不到 digest 则装、`verified:false`、note 明说）→ 解 exe → `.{name}.incoming` 落盘 → 跑 `<incoming> version` 自报版本 → 内核在跑就先 `proxy.disable()` 再 `runtime.stop()` → `promote`（旧文件 → `.old`，rename 失败回滚）→ 用同一节点重启（失败只记 note、不回滚）→ `refresh_tray`。`extract_exe` 两遍扫描：先对整包做路径安全检查（`..`/绝对路径/盘符/空字节即整包拒绝），再按 basename 唯一匹配、`MAX_EXE_BYTES = 150 MB` 且用 `take(MAX+1)` 实测。界面在设置页每个内核行加「检查更新」「更新到最新」，说明行给出「最新版本 X · 本机 Y」/「已是最新」/「最新版本 X · 文件不存在」，装完的说明带 `sha256 前 12 位 · 已核对|上游未提供校验值，未核对`。
- **验收**：`bash /d/tmp/shots/53-kernel-accept.sh`（`mk_53_kernel.py` 生成 PS 驱动，三段 `xray`/`singbox`/`custom`）在云电脑上跑通：xray 从 26.10.31 换成 26.11.9（`已核对`、旧文件进 `.old`、内核 PID 变了、`ProxyEnable=1 ProxyServer=127.0.0.1:7890` 恢复、再查 `已是最新`）；sing-box 从 1.14.3 换成 1.14.4 且回执明写「上游未提供校验值，未核对」（它确实不发 digest 资产）；custom 阶段在 `xray_binary` 指到 `not-a-kernel.exe` 时给出「这个按钮只更新内核目录里的官方文件。恢复默认后再更新」，文件一个字节没动。
- **坑（真机脚本）**：① 云电脑到 GitHub 只有 **5.5 KB/s**，16.7 MB 要 ~50 分钟，120 秒预算里必然超时（报 `内核压缩包内容读取失败：error decoding response body`）——所以验收把**测试二进制**里两条 feed URL 等长原地改指本机 `http://127.0.0.1:8080/`，由 `mirror.ps1` 回真上游 JSON 形状与真官方压缩包；② `cleanup.sh` 会删掉 home 目录下的 `*.ps1`/`*.bat`/`*.txt`/`*.png`，每次验收都要重推驱动；③ cmd 不认 `C:\Users\...` 双反斜杠，`del` 会静默失败——残留的 `stop` 文件让镜像进程一启动就退出，白跑两轮；④ 镜像改成收到 `/quit` 才退，不再依赖跨会话的状态文件；⑤ 设置页比窗口长，内核行按钮坐标会是屏幕外值，必须先 `Scroll-Row`；「检查更新」两行各一个，要用行标签定位。
- **计数**：命令 **84**（+2）、Rust **434**（+13）、vitest **116**（+0）、i18n **798**（+9）。

### 0.10 §54（S6）WebDAV 云备份（已完成，2026-11-25）

- **为什么做**：`docs/PARITY-PLAN.md` 的第 7 项。以前「导出/导入」只认本地文件；v2rayN 与 Clash Verge 都能把配置丢到云端（WebDAV/Gist），换机时不用 U 盘。
- **做了什么**：新增 `src-tauri/src/net/webdav.rs`（`resolve_target`/`basic`/`put_bytes`/`get_bytes`，15 项单测）、`src-tauri/src/syscreds.rs`（Windows 凭据管理器：`CredWriteW`/`CredReadW`/`CredDeleteW`，`ERROR_NOT_FOUND` 当正常值，句柄 `Drop` 走 `CredFree`）、`src-tauri/src/commands/webdav.rs` 的**三条**命令（`webdav_state` / `webdav_upload` / `webdav_restore`，IPC 84 → 87）；`commands/backup.rs` 提出 `collect()`/`apply()`，让「导出文件」与「上传云端」共用一份代码；`Settings` 只加 `webdav_url`/`webdav_username`/`webdav_via_proxy` 三个字段，**密码不进 store**（并在 `save_settings` 上加第 6 个参数 `webdav_password: Option<String>`：`None` 不动、`""` 删除、其他覆盖）；设置页「备份与恢复」后面新增「WebDAV 云备份」一节（地址/用户名/密码 + 「走代理上传」开关 + 上传/恢复两个按钮 + 一行状态说明）。
- **验收**：`bash /d/tmp/shots/54-webdav-accept.sh "" dav1`（五段：push/restore/refuse/auth/proxy，服务器是 `dav.ps1` 手写的 175 行假 WebDAV）在云电脑跑通：上传 4889 字节且服务器侧 `PUT -> 201 bytes=4889 auth=ok`；restore 段把前一步删掉的节点带回来（托盘菜单 4 → 5 个节点）、`GET -> 200 bytes=4889`；refuse 段内核在跑（`xray#5692`）给出「先停止内核再恢复配置：正在跑的内核还是按旧配置渲染的」且该时段服务器零请求；auth 段密码错时 `PUT -> 401 auth=[Basic d2VidGVzdDpub3Bl]`，界面落成「服务器拒绝认证（HTTP 401）：检查用户名和密码」；proxy 段 `webdav_via_proxy=true` 且内核没跑，直接拒绝、没有联网。细节见 `docs/ACCEPTANCE.md` §54。
- **坑（真机脚本）**：① 计划里的第四条命令 `webdav_save` 被合并进 `save_settings`（设置页只有一个「保存」按钮），命令数 84 → 87 而非 88；② WebView2 把 `<label>` 文本挂成输入框自己的可访问名，按「标签找输入框」一律 `label missing`，要直接按 `Name` 匹配 `Edit`；③ `reqwest`/`hyper` 把 header 名写成小写，单测断言原始请求文本要先 `to_lowercase()`；④ specta 只给 `WebdavState`/`WebdavPush` 生成类型，没有 `_Serialize` 后缀；⑤ `cleanup.sh` 会删 home 下 `*.ps1`/`*.bat`/`*.txt`/`*.png`，驱动每轮重推。
- **计数**：命令 **87**（+3）、Rust **455**（+21）、vitest **116**（+0）、i18n **813**（+15）。

### 0.11 §55（S5）轻量模式（已完成，2026-11-25）

- **为什么做**：`docs/PARITY-PLAN.md` 的第 8 项。窗口收进托盘后界面还在每 2 秒轮询内核、图表每 60 秒取样，用户看不到却一直在烧内存与请求。
- **做了什么**：`Settings.lite_mode`；新 `src-tauri/src/power.rs`（宿主 `EmptyWorkingSet` + 界面 `ICoreWebView2_19::SetMemoryUsageTargetLevel`，两半各自回报）；`lib.rs` 的 `set_quiet()` 接到三条隐藏路径与唯一显示入口 `reveal()`，失败原因分条记日志；前端 `src/stores/lite.ts`（一个 `quiet` 值 + 只接一次事件），`engine.ts`/`HomeView.vue`/`LogsView.vue` 隐藏时停表、恢复时先拉一次再开表；设置页「轻量模式」开关 + 「清理界面缓存」按钮（`clearAllBrowsingData()`，提示会连主题/强调色一起回默认）；IPC 仍 **87**（不新增命令）。
- **验收**：`bash /d/tmp/shots/55-lite-accept.sh "" lite5`（off 对照 + on 两阶段，同一 exe）。宿主工作集 34.4 → 2.6（隐藏瞬间）→ 3.8 MB（40 秒）；总占用 364.9 → 339.5 MB（−25.4，−7.0%），对照组同时长 364.7 → 363.9 MB（−0.8）；日志 `轻量模式开/关` 各一行；隐藏期间杀掉内核，回窗首页立刻显示「未运行」「还没有启动内核」。细节见 `docs/ACCEPTANCE.md` §55。
- **坑**：① `windows-sys 0.60.2` 没有 `ProcessMemoryUsageTargetLevel`（宿主侧只能 `EmptyWorkingSet`）；② `SetMemoryUsageTargetLevel` 在 `ICoreWebView2_19` 上，`webview2-com 0.39` + `windows-core 0.62` 必须与 `Cargo.lock` 对齐；③ 界面 330 MB 全在 6 个 `msedgewebview2.exe` 里，低内存档对这套界面几乎没效果——**不许把工作集回收说成「省 25 MB 界面内存」**；④ 停表在 CPU 上看不出来，证据要用「回窗后立刻是新状态」；⑤ PowerShell 里 `"$tag: …"` 会被当驱动器变量、解析期整份脚本死，`ParseFile` 不抛异常也要数 errors；⑥ 首页「启动」与节点页行内按钮同名，改让 `auto_connect` 起内核最稳。
- **计数**：命令 **87**（+0）、Rust **457**（+2）、vitest **116**（+0）、i18n **818**（+5）。

### 0.12 §56（S4a）提权：先看清自己是谁，再决定要不要请人点「是」（已完成，2026-11-25）

- **为什么做**：`docs/PARITY-PLAN.md` 第 9 项的前半。TUN 与服务都要管理员令牌，不先解决这个，用户每次点 TUN 都得手抄一遍 UAC。
- **做了什么**：新 `src-tauri/src/elevate.rs`（`is_elevated()` 走 `OpenProcessToken` + `TokenElevation`，任何一步失败答 false；`restart_elevated()` 用 `ShellExecuteExW` 的 `runas` **先起新进程**、成功之后才 `quit_cleanly`；1223 报「提权被取消」、2/3 报「找不到要启动的程序」）；`--relaunch-wait=<父 pid>` 是任务书没写、但不写就「点提权 → 应用消失」的必需件（新实例在装配 Builder 前等旧实例退出，否则被单实例插件转交 argv 后自杀）；设置页「权限」节（状态行 + 只在 `elevated === false` 时可点的按钮 + 「需要提权时提示我」开关，默认开）；命令 87 → **89**（`app_elevated`、`restart_elevated`）。
- **验收**：`bash /d/tmp/shots/56-elev-accept.sh "" elev3`。已验：已提权时状态行写「当前以管理员身份运行：内核可以用 TUN」、按钮 `enabled=False`、开关 On→Off 后 `store.json` 里 `prompt_elevation=False` 且重启后仍是 Off。**未验**：非管理员下的文案与可点、点一下真的换成管理员令牌、提权后单实例/auto_connect/托盘恢复——这台机器造不出「非管理员但可提权」的进程（详见 §56 坑 1、判据 7/8）。
- **坑**：① 云电脑原始 `EnableLUA=0`（UAC 全关，一切进程都是 High）；打开后 `runas /trustlevel:0x20000` 起不来（code 1）、`schtasks` 起的仍是 High、ssh 在会话 0 够不到会话 1，最后把三处策略恢复原值并重启；② 「点禁用按钮」`Click` 也返回 True，但进程 PID 不变——**别把点击成功当行为证据**，要读 PID/实例数；③ `Buttons-Named` 返回 `PSCustomObject`，`enabled` 要用 `$b.El.Current.IsEnabled`；④ 每阶段生成 wrapper 时 `\r\n` 被写成字面文本导致整份 ps1 解析失败，改成单一 `56-run.ps1` + `-Phase` 参数；⑤ `python` 在 Windows 下 `open()` 默认 GBK，读 `scripts/i18n-pairs.json` 必须 `encoding='utf-8'`。
- **计数**：命令 **87 → 89**（+2）、Rust **457 → 464**（+7）、vitest **116**（+0）、i18n **818 → 824**（+6）。


---

### 0.13 §57（S4b）内核服务：装上以后内核是 SYSTEM（已完成，2026-11-25）

- **为什么做**：`docs/PARITY-PLAN.md` 第 9 项的后半，也是 S1（TUN）的前置——TUN 要管理员权限，每次点都弹 UAC 不是能用的产品。
- **做了什么**：新 `src-tauri/src/service.rs`（1274 行：`--service` 入口走真的 SCM dispatcher `StartServiceCtrlDispatcherW` → `RegisterServiceCtrlHandlerW` → `SetServiceStatus`，不再是裸循环；作业协议 `<nonce>.job.json` → `rename` 认领 → `<nonce>.result.json`；`EnginesDir` 每作业重读、`engine_binary()` 只认两个内核名；就绪 100ms 探测 ≤8s；`running.json` 平铺、启动最前只回收 exe 名属于两个内核的孤儿；install/uninstall 走 `CreateServiceW`/`ChangeServiceConfigW` + `icacls *S-1-5-32-545:(OI)(CI)M`）、`service_client.rs`（386 行：作业往返 10s 超时、`probe()` 只在「已安装且运行中」返回、`ServiceProcess` 停内核走作业、存活判断用 Toolhelp 快照避免读不到 SYSTEM 进程而误判）、`commands/service.rs`（111 行：三条命令，未提权报书里原文）。`engine/runtime.rs` 里 `service: RwLock<Option<...>>` + `refresh_service()`，有服务就交作业、不再本地 spawn，`Running.via_service` 决定停止路径。命令 89 → **92**。
- **验收**：`bash /d/tmp/shots/57-service-accept.sh "" svc6`（一轮里先 install 再 uninstall）。已验：`sc query` 由 1060 → `STATE : 4 RUNNING`；`EnginesDir REG_SZ C:\Users\Administrator\AppData\Roaming\com.xmatrix.client\engines`；`C:\ProgramData\X-Matrix\jobs` 上 `BUILTIN\Users:(OI)(CI)(M)`；界面状态行「服务在运行：内核由 SYSTEM 启动」；从界面启动的内核是 `"xray.exe","1332","Services","0",…,"NT AUTHORITY\SYSTEM"`；卸载后 `sc 1060`、`reg` 空、内核回到 `"xray.exe","3988","Console","1",…,"Administrator"`；按钮 enabled 跟着状态翻。**未验**：服务启动慢到 SCM 判 1053 的时序、`icacls` 失败分支、服务内核日志在 `%ProgramData%\X-Matrix\service\<kind>.log` 而界面日志面板仍读 app-data 的 `logs\<kind>.log`（服务分支没回填 `log_path`，是已知缺口）。
- **坑**：① UIA 给的按钮坐标可能在窗口外（`rect=444,1152`，点 1174 等于点在边框上，按钮 enabled 还是 True、日志还说 clicked），必须按 `Get-Win` 的 `BoundingRectangle` 算可点区间；② disabled 按钮点下去也返回 True（§56 同一个坑）；③ `Buttons-Named` 返回 `PSCustomObject`，要取 `$b.Left/$b.Top/$b.W/$b.H` 与 `$b.El.Current.IsEnabled`；④ python 打印日志在 GBK 控制台崩（PUA 字符），`show_log.py` 里 `sys.stdout.reconfigure(encoding='utf-8', errors='replace')`；⑤ `sc query` 中文经 ssh 管道乱码，判定按 `STATE :`/`1060`/`RUNNING` 关键字；⑥ 每次会话开头先 `sc stop` + `sc delete`，只跑 uninstall 单阶段时服务本来就 absent——验卸载必须同一轮先 install。
- **计数**：命令 **89 → 92**（+3）、Rust **464 → 489**（+25）、vitest **116**（+0）、i18n **824 → 829**（+5）。


### 0.14 S1 · TUN（2026-11-25）

**为什么做**：这是「和 v2rayN / Clash 对齐」里唯一还缺的接管方式——系统代理只覆盖 WinINet，TUN 才管得住不认代理的程序。它排最后，是因为要先有 S4 的提权探测与服务模式：「内核以 SYSTEM 跑时不需要额外提权」这件事得先成立。

**做了什么**：
- `Settings.tun_enabled` / `tun_stack`（只认 `system`/`gvisor`/`mixed`，默认 `mixed`），`RenderInput` 与 `RunOptions` 各带一份。
- `engine/singbox.rs` 的 `singbox_tun_inbound()`：`{type:tun,tag:tun,address:["172.19.0.1/30"],mtu:9000,auto_route:true,strict_route:false,stack:…,route_exclude_address:["100.64.0.0/10"]}`，追加在 mixed/socks/http 之后；`route.rules` 最前面插 `{"action":"sniff"}`。关掉时两样都不出现（单测逐字节对拍）。
- `commands/engine.rs` 的 `start_engine` 渲染前三条拒绝：TUN+系统代理、TUN+Xray、TUN+界面自启内核+非管理员；服务模式在跑时不拦（`EngineRuntime::has_service()`）。
- 界面：TUN 开关 + stack 三档选择 + 说明；打开 TUN 时系统代理开关禁用并自动关掉。
- 不下载 wintun.dll：真机证明 1.14.4 已把它编进二进制。

**验收（§58）**：tun0 出现、默认路由 `192.168.0.1@10`、`curl cp.cloudflare.com` = 204、出口 IP「经代理」、DNS 可用；停止后网卡与路由复原、直连 204；两条拒绝真的拦住且无内核。未验：非管理员拒绝（本机 UAC 关闭，造不出非管理员令牌）、gstatic 这类目标经环回测试节点超时（测试环境限制，未作判据）。

**坑**：计划任务 action 不能直接给 `.ps1`（要 `powershell -File`）；切 prelude 别按 `$w = Get-Win` 这种出现 12 次的短串切，要用行首正则并数 ParseFile 的 errors；每阶段日志必须分文件，否则 `findstr DONE` 会命中上一阶段；`cleanup.sh` 删 home 下 `*.bat/*.ps1`，环回测试服务器要么每轮重推、要么由驱动自己 `Start-Process`。

**计数**：IPC **92**、Rust **496**、vitest **116**、i18n **835**。


---

### 0.15 S7 · 多配置（2026-11-25）

- **为什么做**：一个客户端常常要在「机场 A」和「机场 B」之间来回换，过去的做法是导出再导入，或者手工改节点；v2rayN / Clash Verge 都是「配置」这一层。S7 把这一层补上：一套节点、订阅、规则与策略组叫一个配置，切换后内核按新配置重建。
- **做了什么**：`store.json` 升到 v2（`active` + `profiles` 元数据，激活那份仍内联，非激活的每份一个 `profiles/<id>.json`）；`Store::normalize()` 负责 v1 → v2 的原地升级且只写一次盘；`AppState` 多两把锁（配置元数据 / 激活 id）与 `active_parts`/`profile_parts`/`save_profile_parts`/`drop_profile_file`；`commands/profile.rs` 五条命令（list/create/rename/delete/switch），切换走「先落盘离开那份 → 读目标 → 停内核 → 换内存 → persist → 按需重启 → 刷托盘」；备份 `Backup.profiles` 带全部非激活配置；界面新增 `ProfilesView.vue`（`/profiles`，侧栏图标 `copy`）与 `src/stores/profiles.ts`。
- **验收**：`bash /d/tmp/shots/59-profiles-accept.sh p1`（switch / back / backup）与 `PHASES="migrate" ./59-profiles-accept.sh p4`。已验：v1（5 节点）打开一次即 `version=2 active=default profiles=1 nodes=5`、`store.json` 3773 B / mtime 跨一次重启不变（只写一次）；「默认」→「工作」`nodes 2→3`、`rules 2→0`，反向切回 `nodes 3→2`、`rules 0→2`，节点页与路由页逐项对得上；两次切换 `xray.exe` PID 都变（#896→#7392、#748→#6572）且提示「内核已按 {name} 重启」；云备份 5072 字节里 `document.profiles` 两条、非激活那份带 3 个节点，`profiles/` 目录里只剩 `p-work.json`。**未验**：新配置没有节点时的停机分支、托盘菜单刷新、以及计划里提到的 merge / script 覆盖脚本（本轮没做）。
- **坑**：① v1 升级条件要写成「`profiles` 空且没有 `active`」，只看 `version < 2` 会把用户「删到只剩一个配置」的正常状态又迁一遍；② `ProfileParts` 不能 derive `PartialEq`（`Subscription` 没实现，且本轮不许改 `domain/**`），测试只能逐字段断言；③ 侧栏图标 `layers` 已被策略组占用，配置页用 `copy`；④ 切换时旧内核已停，端口必然空闲，所以「新配置端口冲突」这条分支在真机走不到；⑤ 备份里非激活配置的字段是扁平的（`StoredProfile` flatten 了 `ProfileParts`），验收脚本按 `parts.nodes` 读会拿到 0；⑥ 设置页比窗口长（WebDAV 那块 `top=7093`），点前必须 `Scroll-Row`。
- **计数**：Rust 496 → **514**、IPC 92 → **97**、vitest 116（未变）、i18n 835 → **856**。

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
| 当前构建产物 | `src-tauri/target/release/x-matrix.exe` **11,056,640 B @2026-11-25 18:26，md5 `907a38674359352f22cddcb9c61c627f`**（含 §21–§59）；桌面副本已不在，需要时自己 `cp` 过去 |

**计数基线（改动后记得同步文档）**：命令 **92**、`cargo test --lib` **489**、vitest **116**（12 个文件）、i18n **829** 键。
> `README.md:81` 与 `ARCHITECTURE.md:277-278` 已同步为 97 条命令 / 514 项测试。

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

`docs/ACCEPTANCE.md` 是**唯一可信的进度表**（§0–§59 共 59 节，每节都有「验法 + 判据 + 设计理由 + 坑」）：

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

`D:\tmp\parity-gaps.md` 是最初的对标审计（31 行已覆盖、7 高价值、13 锦上添花、11 条当时判定不做（2026-11-25 已全部解禁），H1–H7 与 N1、N2、N8、N9、N12 已吃掉其中大部分。

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
| ~~S2 PAC 脚本~~ | 已完成：`src-tauri/src/sysproxy/pac.rs` 生成 + `AutoConfigURL` 接管/还原/互斥，界面开关，验收见 §48 | 中 | 2026-11-25 关闭 |
| ~~S8 HTTP 与 SOCKS 分端口~~ | 已完成：`Endpoints` 一处决议 + 两个渲染器各开两个入站 + 界面显示生效值，验收见 §49 | 中 | 2026-11-25 关闭 |
| ~~S10 强调色 / 自定义主题~~ | 已完成：六个预设两块 CSS + 自定义 hex 现算对比度 + `localStorage` 持久化，验收见 §50 | 小 | 2026-11-25 关闭 |
| ~~S11 进程选择器~~ | 已完成：`sysproc::list()` + 一条 `list_processes` + 规则编辑器第二个弹窗，验收见 §51 | 中 | 2026-11-25 关闭 |
| 设计审计残余 | `D:\tmp\design-audit-2.md` | — | 二审 15 条已全部处理完；这份文件可以当历史读 |

**S1–S11 已全部解禁**（用户 2026-11-25 指示：功能要做到 V2RayN 与 Clash 二者结合），旧口径「明确不做」作废。进度与顺序只认 `docs/PARITY-PLAN.md`：S2（§48）、S8（§49）、S10（§50）、S11（§51）已完成；剩 S9 回退/负载组、S3 内核下载更新、S6 WebDAV 备份、S5 轻量模式、S4 提权/服务、S1 TUN、S7 多配置。

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
3. `export PATH="$HOME/.cargo/bin:$PATH"`；`cd src-tauri && cargo test --lib`（应为 514 passed）。
4. `cd .. && npm run check`（应为全绿）。
5. 从 §6 表里挑一项（按性价比排序），**一项一提交**，提交信息用英文 `feat(...)/fix(...)/docs: ...`。
6. 真机验证前先跑 `bash /d/tmp/shots/deploy-tray.sh <exe>`；远端被 `cleanup.sh` 清过的话，先 `scp x-act.ps1 uia2.ps1 ocr.ps1` 上去。
7. 验完写 `docs/ACCEPTANCE.md` 新的一节（**验法 / 判据 / 设计理由 / 坑** 四件套），并同步 `README.md:81`、`ARCHITECTURE.md:277` 的命令数与测试数。
8. 收尾 `bash /d/tmp/shots/cleanup.sh`，再手查 `ProxyEnable` 与 `schtasks /query | findstr xm` 确实是空的。

---

## 10. 一句话交接

**能用的东西已经都在了，剩下的都是边角。**接着做的人只需要：从 §6 表挑一项，按「实现 → 单测 → `npm run check` → 构建 → 云电脑真机 → 写 ACCEPTANCE → 提交 → 清理」这套流程走完，并且始终记得 §7 里那些验收环境的坑。

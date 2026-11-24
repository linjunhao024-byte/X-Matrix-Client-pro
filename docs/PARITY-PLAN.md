# S1–S11 解禁计划（与 v2rayN / Clash Verge 对齐的最后一批）

> 2026-11-25 用户解禁。此前的 `D:\tmp\parity-gaps.md` §2.3 把这 11 项列为「明确不做」，理由是上一任窗口对风险的保守估计，不是用户的目标。
> 用户原话（2026-11-25）：「我最开始的目标就是把 X-Matrix-Client-pro 的功能做到 V2rayN 和 Clash 二者相结合……你全部解禁，一个一个写明确的任务书 & 提示词派子代理去做」。
>
> **口径**：每一项都要真的能用、能验收，不许留假开关。做不到的那一半要写清楚（例如某个特性只有 sing-box 支持），写进 `docs/ACCEPTANCE.md`，并把 `AboutView` 的能力表改成实话。
> **验收**：真机只在 `cloudpc-test`（`100.106.121.0`）上做；本机绝不起内核、绝不写系统代理注册表。

## 顺序与理由

| 顺序 | 项 | 内容 | 依赖 | 为什么排在这里 |
|---|---|---|---|---|
| 1 | S2 | PAC 脚本自动配置 | — | 纯生成 + 一处注册表读写，能把「任务书 → 子代理 → 构建 → 真机 → 文档」这条流水线先跑顺 |
| 2 | S8 | HTTP 与 SOCKS 分离端口 | — | 改 `Settings` + 两个渲染器，边界清楚 |
| 3 | S10 | 强调色 / 自定义主题 | — | 只动设计层，先把设计约束（tokens）摸清 |
| 4 | S11 | 按进程分流的进程选择器 | — | 匹配器与平台枚举都在，缺的只是枚举命令 + 选择器 UI |
| 5 | S9 | 回退组 / 负载均衡组 | 需要运行中内核验收 | 策略组模型要扩，sing-box 侧才诚实 |
| 6 | S3 | 内核下载 / 更新 | 需要网络 + 代理 | 与 S1 无耦合，但要处理「换文件时内核在跑」 |
| 7 | S6 | WebDAV 云备份 | — | 复用现有备份包，加一层传输 |
| 8 | S5 | 轻量模式 | — | WebView2 内存与轮询降载，独立 |
| 9 | S4 | 提权 / 服务模式 | — | TUN 的前置：不解决权限，TUN 每次都要 UAC |
| 10 | S1 | TUN 虚拟网卡 | **S4** | 需要管理员权限与 sing-box `tun` 入站；只对 sing-box 诚实支持 |
| 11 | S7 | 多配置（profile 层） | 前面各项都会改 `render.rs` | 架构级改动，放最后，避免前面的成果反复重写 |

## 每一项的判据（写完打勾）

### S2 · PAC
生成 `.pac`（`FindProxyForURL`）落到应用数据目录，开启时写 `AutoConfigURL`，关闭时按快照还原（原本没有该值就删掉）。
PAC 只表达**能表达的**：绕过列表 + 规则里「直连」的域名类匹配器 → `DIRECT`，其余 `PROXY 127.0.0.1:<mixed_port>; DIRECT`。协议/IP/端口类规则无法表达，必须在界面注明。
判据：注册表 `AutoConfigURL` 出现/消失/还原；pac 文件内容随绕过列表与直连规则变化；退出应用后不留残余。

**结果（2026-11-25）**：已实现并真机验收，判据全过（见 `docs/ACCEPTANCE.md` §48）。实施中补了两条原计划没写的硬约束：
- PAC 接管时 `ProxyEnable=0` 且清空 `ProxyServer`，与固定代理**互斥**；脚本先 `tmp+rename` 落盘再写注册表，写前校验脚本确实带了 `PROXY 127.0.0.1:<当前端口>; DIRECT`。
- 回环 / 私网 / 链路本地（`localhost`、无点裸名、`127.`、`10.`、`192.168.`、`169.254.`、`172.16–31`、`::1`、`fe80`/`fc`/`fd`、`.local`）在脚本里**无条件直连**，不依赖用户可编辑的绕过列表——真机上那份列表被改坏时，PAC 会把 `127.0.0.1` 送回代理自己形成回环。
- 老实说明：窗口退出后只还原注册表、不删 `.pac` 文件；IP 段/端口/geosite/进程/规则集类直连规则进不了脚本，界面已注明。

### S8 · 分离端口
`Settings` 增加 `http_port` / `socks_port`（缺省 0 = 继续用 mixed 单端口）。Xray 渲染拆 `http` + `socks` 两个 inbound，sing-box 同理；端口预检、`ProxyServer` 仍用 HTTP 端口；订阅「走代理」用 HTTP 端口。
判据：两条端口都起得来、系统代理指向 HTTP 端口、SOCKS 端口能单独握手。

**结果（2026-11-25）**：已实现并真机验收（见 `docs/ACCEPTANCE.md` §49）。实际做出来的比原计划多两条：
- 生效端口收进 `Endpoints` 一处决议（`0 = 跟 mixed 一样`），对外只给 `http_port` / `socks_port` / `proxy_port()`；渲染、接管、订阅抓取、真实延迟、临时内核、诊断全从它取，`mixed_port` 降级为回退基值。
- 分离后 SOCKS 仍占 `socks` tag，HTTP 新开 `http` tag（引用旧 tag 的用户规则不受影响）；`mixed_port` 在分离态不再被 bind，因此「低于 1024 / 等于控制端口」两条检查加了 `!split` 前置——否则一个没被使用的数字会否决完全合法的启动。
- 老实说明：真机只验了 Xray 的两种形态（单 `mixed` 入站 / `socks`+`http` 两入站），sing-box 的分离渲染只有单测；测试机不通外网，「两个端口各自握手成功」这件事没有机器证据；半分离（只填一个）只有单测。

### S10 · 强调色 / 主题
主题仍是 light/dark/system，另加强调色（若干预设 + 自定义 hex），落在 `src/styles/tokens.css` 的语义变量上，由 `data-accent` 切换；对比度必须可读。
判据：切换后主按钮/选中态跟随变化、深色下可读、刷新后保持、`python /c/Users/83611/check-design.py` 无 error。

结果（2026-11-25）：六个预设（indigo / violet / ocean / teal / amber / rose）各一浅一深两块 CSS，只覆盖 `--accent` / `--accent-hover` / `--accent-soft` / `--accent-contrast` 四个变量；自定义 hex 由 JS 写内联变量、切回预设时 `removeProperty`；对比度用 WCAG 2.x 现算，<4.5 拒绝。indigo 的四个值与改动前逐字节一致，`src/services/accent.test.ts` 直接解析 `src/styles/tokens.css` 做双源比对。强度：默认观感不变、六预设两档、自定义、恢复默认、活过重启——五条都在真机上按像素数验过。

### S11 · 进程选择器
`list_processes()`（Windows Toolhelp32，去重、带图标可省）供规则编辑器选择 `ProcessName`；说明只有 sing-box 的 `process_name` 匹配在运行期生效。
判据：选择器能列出真实进程；选中后规则落库；sing-box 运行中该进程流量按规则走（真机）。

结果（2026-11-25）：`src-tauri/src/sysproc.rs` 里 `ProcessInfo { name, pids }` + 纯函数 `group()`（大小写不敏感归并、保留首次拼写、pid 升序去重、按小写名排序）+ `list()`（Windows Toolhelp32，RAII 关句柄，快照失败报 `AppError::Io`，非 Windows 报 `AppError::Internal`，跳过 pid 0 与空名）；命令只有一条 `list_processes`（81 → 82）。界面在规则编辑器 `process_name` 分支里多一个「选择进程」按钮 → 第二个 `ModalDialog`（搜索、`名称 (pid数)` 单行、loading/错误/空三态、刷新、点行只写值不提交），过滤是 `src/services/processlist.ts` 的纯函数（三条 vitest）。真机（`label=s11b`）：57 个真实进程、搜 `x-matrix` 收窄到 `X-Matrix.exe (1)`、点行后编辑器值域回读为 `X-Matrix.exe`、无匹配时显示「没有匹配的进程」、关选择器不关规则；见 `docs/ACCEPTANCE.md` §51。老实说明：只验了「枚举 → 收窄 → 写进值域 → 空态」，没有跑 sing-box 运行期按进程分流（测试机无外网出口，也怕在云电脑上起内核留下状态）；Xray 的 `process` 匹配是否生效未证，界面已如实写明。

### S9 · 回退组 / 负载均衡组
`GroupKind` 增加 `Fallback`（按成员顺序，健康检查失败就顺延）与 `LoadBalance`（轮询）。sing-box 用 `selector` + 应用侧健康检查切换；Xray 下如实标注为「静态副本，不具备回退语义」。
判据：sing-box 下杀掉首选成员的内核出口能自动落到下一个；轮询组连续请求分散到不同成员；Xray 下界面明确说明差异。

### S3 · 内核下载 / 更新
从 GitHub releases（可走应用内代理）下载官方内核压缩包，校验 sha256，解压到 engines 目录；运行中先停内核再替换，替换失败保留旧文件。
判据：能下载并替换；`engine_version` 读到新版本；失败路径（断网、校验不符）有明确提示且旧文件仍在。

### S6 · WebDAV 备份
在现有备份包之上加 WebDAV 上传/下载（URL + 用户名 + 密码，密码进 Credential Manager 或本地加密，不明文进 store）。提供「立即上传」「从云端恢复」。
判据：上传后云端文件可被另一台/清空后恢复；凭据不以明文出现在 `store.json`。

### S5 · 轻量模式
隐藏到托盘后：暂停图表与轮询定时器、`SetMemoryUsageTargetLevel(Low)`、按需清理 WebView2 缓存；恢复窗口时还原。
判据：内存占用可见下降；恢复窗口后数据立刻刷新、无空白。

### S4 · 提权 / 服务模式
先做提权：`is_elevated()` + 以管理员重启（`ShellExecuteEx` runas），设置里可选「需要时提示」。
再做服务：`x-matrix.exe --service` 注册为 Windows 服务（SYSTEM），命名管道 IPC 转发内核启停，非管理员也能带 TUN 跑。
判据：未提权时 TUN 开关给出可执行的提示；服务已装时内核以 SYSTEM 身份运行且 UI 可启停。

### S1 · TUN
sing-box `tun` 入站（`auto_route`、`strict_route`、`stack: system`），需要管理员权限与 Wintun；Xray 无 TUN，界面直接说明「TUN 需要 sing-box」。
判据：真机开启 TUN 后，未设系统代理的流量也走代理（用出口 IP 页对比）；关闭后路由表还原；与系统代理同时开启不冲突。

### S7 · 多配置
引入 profile 层：每个 profile 保存节点来源（订阅/手工）、规则、策略组与覆盖脚本（merge / script），切换时重新生成配置。`render.rs` 从「一份全局配置」改为「按 profile 生成」。
判据：两个 profile 各自的节点与规则互不污染；切换 profile 后运行中内核按新配置重启；导出备份含全部 profile。

## 进度

| 项 | 状态 | 提交 | 验收 |
|---|---|---|---|
| S2 PAC | **已完成**（2026-11-25） | 与 `src-tauri/src/sysproxy/pac.rs` 同一次提交（`git log --oneline -- src-tauri/src/sysproxy/pac.rs` 最新一条） | `docs/ACCEPTANCE.md` §48；真机脚本 `D:\tmp\shots\48-pac-accept.sh` |
| S8 分离端口 | **已完成**（2026-11-25） | 与 `src-tauri/src/engine/mod.rs` 同一次提交（`git log --oneline -- src-tauri/src/engine/mod.rs` 最新一条） | `docs/ACCEPTANCE.md` §49；真机脚本 `D:\tmp\shots\49-split-accept.sh`、`49-ui-accept.sh` |
| S10 主题 | **已完成**（2026-11-25） | 与 `src/services/accent.ts` 同一次提交（`git log --oneline -- src/services/accent.ts` 最新一条） | `docs/ACCEPTANCE.md` §50；真机脚本 `D:\tmp\shots\50-accent-accept.sh` |
| S11 进程选择器 | **已完成**（2026-11-25） | 与 `src-tauri/src/sysproc.rs` 同一次提交（`git log --oneline -- src-tauri/src/sysproc.rs` 最新一条） | `docs/ACCEPTANCE.md` §51；真机脚本 `D:	mp\shots)-proc-accept.sh` |
| S9 回退/负载组 | 未开始 | — | — |
| S3 内核更新 | 未开始 | — | — |
| S6 WebDAV | 未开始 | — | — |
| S5 轻量模式 | 未开始 | — | — |
| S4 提权/服务 | 未开始 | — | — |
| S1 TUN | 未开始 | — | — |
| S7 多配置 | 未开始 | — | — |

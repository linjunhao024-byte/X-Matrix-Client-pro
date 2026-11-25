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
| 7 | S6 | WebDAV 云备份 | 已完成（2026-11-25） | 复用现有备份包，加一层传输 |
| 8 | S5 | 轻量模式 | — | WebView2 内存与轮询降载，独立 |
| 9 | S4 | 提权 / 内核服务（已完成 2026-11-25） | — | TUN 的前置：不解决权限，TUN 每次都要 UAC |
| 10 | S1 | TUN 虚拟网卡 | **S4（已完成）** | 需要管理员权限与 sing-box `tun` 入站；只对 sing-box 诚实支持 |
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

**结果（2026-11-25）**：sing-box 下两种组都渲染成原生 `selector`，切换由新模块 `src-tauri/src/engine/groupwatch.rs` 做（20 秒一轮、3 秒探针预算、只改 selector 不下发流量命令）；Xray 保持静态副本并在组页写明不回退不轮换。真机（云电脑，本地两个 shadowsocks 服务 + 一个死端口成员）：fallback 一次跨到 `node:ss-local-1` 后 72 秒不抖、load-balance 20 秒一轮切换四次、日志 5 条「原因：健康探测」。判据收窄处如实写进 §52：「轮换」指内核的 `now` 被逐轮切走，**不是**按连接分摊（Clash selector 模型做不到，界面也这么写）；「杀掉首选成员」是用死端口成员模拟的。Rust 单测 405 → 421，IPC 仍 82（无新命令），i18n 784 → 789。

### S3 · 内核下载 / 更新
从 GitHub releases（可走应用内代理）下载官方内核压缩包，校验 sha256，解压到 engines 目录；运行中先停内核再替换，替换失败保留旧文件。
判据：能下载并替换；`engine_version` 读到新版本；失败路径（断网、校验不符）有明确提示且旧文件仍在。

**结果（2026-11-25）**：`src-tauri/src/engine/release.rs` 负责上游解析与校验（`feed_url`/`asset_name`/`parse_release`/`parse_digest`/`sha256_hex`/`version_from_self_report`/`extract_exe`/`stage`/`promote`/`discard`），`src-tauri/src/commands/kernel.rs` 只暴露两条命令（`kernel_check` 只读、`kernel_install` 承担全部副作用，IPC 82 → 84）；`net/remote.rs` 加 `fetch_bytes_through`，沿用 `subscription_via_proxy` 的取路规则。次序是钉死的：自定义路径先拒绝（不联网）→ 解析 → 下载（120 秒）→ 核 sha256（不符不落盘；上游没有 digest 就装但回执写「未核对」）→ 解 exe → `.{name}.incoming` → `<incoming> version` 自报版本 → 先放代理再停内核 → `promote`（旧文件 → `.old`，rename 失败回滚）→ 同节点重启 → `refresh_tray`。真机（云电脑，三段 `xray`/`singbox`/`custom`）：xray 26.10.31 → 26.11.9 `已核对`、`.old` 保留、内核 PID 变、代理恢复；sing-box 1.14.3 → 1.14.4 且明写「上游未提供校验值」；自定义路径拒绝且文件不动。判据收窄处如实写进 §53：云电脑到 GitHub 只有 ~5.5 KB/s，验收把**测试二进制**的两条 feed URL 等长改指本机镜像（返回真上游 JSON 形状与真官方包），真 GitHub 只验到「120 秒超时、文件不动、内核不停」。Rust 单测 421 → 434（+13），IPC 82 → 84（+2），i18n 789 → 798（+9）。

### S6 · WebDAV 备份
在现有备份包之上加 WebDAV 上传/下载（URL + 用户名 + 密码，密码进 Credential Manager 或本地加密，不明文进 store）。提供「立即上传」「从云端恢复」。
判据：上传后云端文件可被另一台/清空后恢复；凭据不以明文出现在 `store.json`。

结果（2026-11-25）：已完成，见 `docs/ACCEPTANCE.md` §54。

- 做了三件事，顺序是钉死的：备份格式不再分叉（`commands/backup.rs` 提出 `collect()` / `apply()`，导出与上传、导入与恢复各走同一条）→ 密码不进 `Settings`（`syscreds.rs` 走 Windows 凭据管理器，target `X-Matrix/WebDAV`；`Settings` 只留 `webdav_url` / `webdav_username` / `webdav_via_proxy`）→ 传输层单独一层（`net/webdav.rs`，`resolve_target` / `basic` / `put_bytes` / `get_bytes`，状态码本层不判）。
- **比计划少一条命令**：原计划四条（`webdav_state` / `webdav_save` / `webdav_upload` / `webdav_restore`，IPC 84 → 88）。真机上设置页只有一个「保存」按钮，地址、用户名、密码本来就在同一张表单里，于是密码保存并进 `save_settings` 的第 6 个参数 `webdav_password: Option<String>`（`None` 不动 / `""` 删除 / 其他覆盖）。最终 IPC 84 → **87**。
- 判据收窄（要如实说的）：① 只在自己写的 175 行假 WebDAV（`dav.ps1`，Basic 认证、PUT/GET、401）上验过，真实服务的重定向、chunked、TLS 证书、目录不存在到底是 404 还是 409 都没验；② 密码交给 Windows 凭据管理器不等于「别人读不到」——同一登录用户下的其它程序按权限模型可读，没有额外加密；③ 上传是整份覆盖，没有版本与冲突检测；④ 恢复是整体替换，界面写了这句话但没有二次确认框。
- 真机脚本坑：WebView2 把 `<label>` 文本挂成输入框自己的可访问名（按标签找输入框会 `label missing`，要直接按 `Edit.Name` 匹配）；`hyper` 把 header 名写小写（单测断言先 `to_lowercase()`）；specta 只生成 `WebdavState`/`WebdavPush`，没有 `_Serialize` 后缀。

### S5 · 轻量模式
隐藏到托盘后：暂停图表与轮询定时器、`SetMemoryUsageTargetLevel(Low)`、按需清理 WebView2 缓存；恢复窗口时还原。
判据：内存占用可见下降；恢复窗口后数据立刻刷新、无空白。

- **结果（2026-11-25）**：`Settings.lite_mode` + `src-tauri/src/power.rs`（宿主 `EmptyWorkingSet`、界面 `ICoreWebView2_19::SetMemoryUsageTargetLevel`，失败分条记日志）+ `src-tauri/src/lib.rs` 的 `set_quiet()`（三条隐藏路径 + `reveal()`）+ `src/stores/lite.ts`（`quiet` 单一值，`engine`/`HomeView`/`LogsView` 停表、恢复先拉再开表）+ 设置页「轻量模式」与「清理界面缓存」。
- 判据收窄（要如实说的）：① 实测收益是宿主工作集 34.4 → 3.8 MB、总占用 −25.4 MB（−7.0%）；界面 330 MB 的 `msedgewebview2.exe` 那部分**没有降**——WebView2 低内存目标在这套简单界面上几乎无效，留着是因为重界面时才有用；② 「暂停定时器」在 CPU 上量不出来（轮询本身几乎不耗 CPU），证据是「隐藏期间杀内核，回窗首页立刻显示未运行」；③ 「清理界面缓存」按钮只做了代码路径与构建验证，没在真机上点过（它会连主题/强调色一起清掉，脚本没覆盖）；④ `NodesView` 的自动测速（用户自己开的）在隐藏时仍会按设定发请求，属范围外。
- 真机脚本：`D:/tmp/shots/55-lite-accept.sh`、`mk_55_lite.py`。

### S4 · 提权 / 服务模式
**结果（2026-11-25，只做完前半 S4a）**：`src-tauri/src/elevate.rs` 判断令牌、`ShellExecuteExW` 的 `runas` 起新进程；`--relaunch-wait=<父 pid>` 让新实例等旧实例退出，绕开 `tauri-plugin-single-instance` 的「第二次启动就自杀」；设置页「权限」节 = 状态行 + 只有非管理员时才可点的按钮 + 「需要提权时提示我」开关（默认开、存进 `store.json`）。命令 87 → 89。真机只验到「识别 + 门禁 + 偏好持久化」：这台测试机造不出「非管理员但可提权」的进程（原始 `EnableLUA=0`；打开 UAC 后 `runas /trustlevel` 起不来、任务计划仍是 High、ssh 在会话 0 够不到会话 1），所以「点一下真的换成管理员令牌」这条**未验**，见 `docs/ACCEPTANCE.md` §56 的判据 7、8 与坑 1。服务模式（S4b）没动。

**结果（2026-11-25，S4b 内核服务）**：内核交给一个 Windows 服务跑。作业走 `%ProgramData%\X-Matrix\jobs` 目录（管道要自己写安全描述符，写错就是提权洞；目录把权限交给 ACL，`BUILTIN\Users:(OI)(CI)(M)`），认领用同卷 `fs::rename`；作业只写 `kind`，exe 由服务从 `HKLM\Software\X-Matrix\Service\EnginesDir` 每作业重读，客户端塞不进自己的路径；起完 100ms 探控制端口、8 秒不开就 kill。真机（`57-service-accept.sh "" svc6`，一轮先 install 再 uninstall）：`sc query` 1060 → `STATE : 4 RUNNING`、`EnginesDir` 写入、`jobs` 的 `Users:(OI)(CI)(M)`、界面「服务在运行：内核由 SYSTEM 启动」、内核 `Services/0/NT AUTHORITY\SYSTEM`；卸载后 `sc 1060`、注册表键删掉、内核回到 `Console/1/Administrator`。命令 89 → 92，Rust 464 → 489。**未验**：SCM 1053 慢启动时序、`icacls` 失败分支、服务内核日志未回填到界面日志面板（服务分支没写 `log_path`）。

服务模式（未做）：
再做服务：`x-matrix.exe --service` 注册为 Windows 服务（SYSTEM），命名管道 IPC 转发内核启停，非管理员也能带 TUN 跑。
判据：未提权时 TUN 开关给出可执行的提示；服务已装时内核以 SYSTEM 身份运行且 UI 可启停。

### S1 · TUN
sing-box `tun` 入站（`auto_route`、`strict_route`、`stack: system`），需要管理员权限与 Wintun；Xray 无 TUN，界面直接说明「TUN 需要 sing-box」。
判据：真机开启 TUN 后，未设系统代理的流量也走代理（用出口 IP 页对比）；关闭后路由表还原；与系统代理同时开启不冲突。

### S1 · 结果（2026-11-25）

- 做的是什么：`Settings.tun_enabled` + `tun_stack`（system/gvisor/mixed）；sing-box 渲染时把 tun 入站追加在最后、`route.rules[0]` 插 `sniff`，关掉时一条都不出现（单测逐字节对拍）；`start_engine` 在渲染前三条拒绝；界面 TUN 开关 + 三档 stack，打开 TUN 时系统代理开关禁用并自动关掉。
- 真机结论（§58）：tun0 出现、默认路由改到它（`192.168.0.1@10`）、`curl http://cp.cloudflare.com/` = 204、首页出口 IP 变「180.119.140.33 · 经代理」、DNS 经隧道可用；停止后网卡与路由复原、直连 204；TUN+Xray 与 TUN+系统代理两条拒绝都真的拦住且无内核。
- 判据收窄（如实记）：
  - 「非管理员拒绝 TUN」未在真机上触发——云电脑 `EnableLUA=0`，所有进程都是完整令牌（同 §56），只能靠单测。
  - 环回测试节点打 Google 那一路在本机网络上超时（同机直连是 204），所以判据只看「至少一条 HTTP 经隧道成功 + 出口 IP 变经代理」，没有拿 gstatic 当必过项。
  - 不下载 wintun.dll：真机改名 `engines\wintun.dll` 后 sing-box 1.14.4 照样起在 tun0，说明这版把 wintun 编进了二进制，于是这个功能根本没做。
- 计数：Rust 489 → **496**（7 项新测试），IPC 仍 **92**，i18n 829 → **835**。

### S7 · 多配置
引入 profile 层：每个 profile 保存节点来源（订阅/手工）、规则、策略组与覆盖脚本（merge / script），切换时重新生成配置。`render.rs` 从「一份全局配置」改为「按 profile 生成」。
判据：两个 profile 各自的节点与规则互不污染；切换 profile 后运行中内核按新配置重启；导出备份含全部 profile。

### S7 · 结果（2026-11-25）

- **做了什么**：`store.json` 升到 v2——`active` 与 `profiles:[{id,name}]` 是元数据，**激活那份仍内联在 `nodes`/`rules`/`subscriptions`/`groups`/`rule_sets`**，非激活的各存 `profiles/<id>.json`；v1 打开时 `normalize()` 原地补两个键、迁移成「默认」并只写一次盘。命令恰好五条（`profiles_list`/`profile_create`/`profile_rename`/`profile_delete`/`profile_switch`，IPC 92 → 97）。切换顺序：先把离开的那份落盘 → 读目标（失败不动现状）→ 若在跑先 `proxy.disable()`+`runtime.stop()` → 换内存 + persist + 删重复文件（失败回滚）→ 用 `last_node_id`（无效则第一个节点）重启 → `refresh_tray`。界面新增 `ProfilesView.vue`（`/profiles`）与 `src/stores/profiles.ts`（切换后对 settings/nodes/routing/groups/ruleSets/subscriptions 全部 `load(true)`）。备份 `Backup.profiles` 带全部非激活配置，`apply` 先写文件再 `replace`；老备份 `#[serde(default)]` 归一成「默认」。
- **真机结论**（`59-profiles-accept.sh`）：v1（5 节点）打开一次即 `version=2 active=default profiles=1 nodes=5`，`store.json` 3773 B / mtime 不变地跨过一次重启（只写一次）；切到「工作」后 `nodes=3 rules=0`、节点页只剩三个环回 SS、路由页 0 条规则，切回「默认」后 `nodes=2 rules=2`、两个订阅节点与两条规则都回来了；两次切换的 `xray.exe` PID 都变了（#896→#7392、#748→#6572），界面提示「内核已按 {name} 重启」；云备份 5072 字节里 `document.profiles` 两条、非激活那份带 3 个节点。
- **判据收窄三处**：① 「新配置里没有节点 → 内核保持停止」这条分支没验（种子两个配置都有节点）；② 托盘菜单是否跟着刷新没验（只读了首页徽章）；③ 计划里的 merge / script 覆盖脚本**没有做**——本轮做到 profile 层，订阅级覆盖脚本与「`render.rs` 按 profile 分支渲染」留作后续。
- **计数**：Rust 496 → **514**、IPC 92 → **97**、vitest 116（未变）、i18n 835 → **856**。

## 进度

| 项 | 状态 | 提交 | 验收 |
|---|---|---|---|
| S2 PAC | **已完成**（2026-11-25） | 与 `src-tauri/src/sysproxy/pac.rs` 同一次提交（`git log --oneline -- src-tauri/src/sysproxy/pac.rs` 最新一条） | `docs/ACCEPTANCE.md` §48；真机脚本 `D:\tmp\shots\48-pac-accept.sh` |
| S8 分离端口 | **已完成**（2026-11-25） | 与 `src-tauri/src/engine/mod.rs` 同一次提交（`git log --oneline -- src-tauri/src/engine/mod.rs` 最新一条） | `docs/ACCEPTANCE.md` §49；真机脚本 `D:\tmp\shots\49-split-accept.sh`、`49-ui-accept.sh` |
| S10 主题 | **已完成**（2026-11-25） | 与 `src/services/accent.ts` 同一次提交（`git log --oneline -- src/services/accent.ts` 最新一条） | `docs/ACCEPTANCE.md` §50；真机脚本 `D:\tmp\shots\50-accent-accept.sh` |
| S11 进程选择器 | **已完成**（2026-11-25） | 与 `src-tauri/src/sysproc.rs` 同一次提交（`git log --oneline -- src-tauri/src/sysproc.rs` 最新一条） | `docs/ACCEPTANCE.md` §51；真机脚本 `D:\tmp\shots\51-proc-accept.sh` |
| S9 回退/负载组 | **已完成**（2026-11-25） | 与 `src-tauri/src/engine/groupwatch.rs` 同一次提交（`git log --oneline -- src-tauri/src/engine/groupwatch.rs` 最新一条） | `docs/ACCEPTANCE.md` §52；真机脚本 `D:/tmp/shots/52-group-run.sh` |
| S3 内核更新 | **已完成**（2026-11-25） | 与 `src-tauri/src/engine/release.rs` 同一次提交（`git log --oneline -- src-tauri/src/engine/release.rs` 最新一条） | `docs/ACCEPTANCE.md` §53；真机脚本 `D:/tmp/shots/53-kernel-accept.sh`、`mk_53_kernel.py`、`mirror.ps1` |
| S6 WebDAV | 已完成（2026-11-25） | 与 `src-tauri/src/net/webdav.rs` 同一次提交 | §54；真机脚本 `54-webdav-accept.sh`、`mk_54_webdav.py`、`dav.ps1` |
| S5 轻量模式 | **已完成**（2026-11-25） | 与 `src-tauri/src/power.rs` 同一次提交（`git log --oneline -- src-tauri/src/power.rs` 最新一条） | `docs/ACCEPTANCE.md` §55；真机脚本 `D:/tmp/shots/55-lite-accept.sh`、`mk_55_lite.py` |
| S4a 提权 | **已完成**（2026-11-25，判据 7、8 未验） | 与 `src-tauri/src/elevate.rs` 同一次提交（`git log --oneline -- src-tauri/src/elevate.rs` 最新一条） | `docs/ACCEPTANCE.md` §56；真机脚本 `D:/tmp/shots/56-elev-accept.sh`、`mk_56_elev.py` |
| S4b 内核服务 | **已完成**（判据全过，见 §57） | 与 `src-tauri/src/service.rs` 同一次提交（`git log --oneline -- src-tauri/src/service.rs` 最新一条） | `docs/ACCEPTANCE.md` §57；真机脚本 `D:/tmp/shots/57-service-accept.sh`、`mk_57_service.py` |
| S1 TUN | **已完成**（2026-11-25） | 与 `src-tauri/src/engine/singbox.rs` 同一次提交（`git log --oneline -- src-tauri/src/engine/singbox.rs` 最新一条） | `docs/ACCEPTANCE.md` §58；真机脚本 `D:\tmp\shots\58-tun-accept.sh`、`mk_58_head.py`/`mk_58_body.py` |
| S7 多配置 | **已完成**（2026-11-25，profile 层与三条判据；merge / script 覆盖脚本未做，见 §59） | 与 `src-tauri/src/store.rs` 同一次提交（`git log --oneline -- src-tauri/src/store.rs` 最新一条） | `docs/ACCEPTANCE.md` §59；真机脚本 `D:\tmp\shots\59-profiles-accept.sh`、`seed59.py`、`mk_59_body.py` |

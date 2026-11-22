# X-Matrix

给代理客户端做一张好看的皮 —— 内核不是我们写的，界面是。

Tauri 2 + Vue 3 + TypeScript。Rust 侧管配置、进程、系统代理与持久化，Vue 侧只管画。

---

## 现在能做什么

| 能力 | 说明 |
|---|---|
| 节点管理 | 手填或粘贴分享链接（`vless` / `vmess` / `trojan` / `ss` / `hysteria2`(`hy2`) / `tuic`，支持整包 base64），重复节点自动跳过；hysteria2 与 tuic 走 sing-box 内核，选 Xray 启动时会明确告诉你换内核 |
| 启动内核 | Xray 或 sing-box 二选一，启动前生成配置、等就绪、失败自动清理 |
| 实时统计 | 上下行累计值与速率、最近一分钟曲线、本次会话的用量（可清零）；sing-box 额外提供连接列表，可搜索 / 排序 / 逐条「断开」 |
| 路由规则（含「试一试」：输入域名看命中哪条规则） | 7 种匹配器（域名后缀/关键字/完整、IP 段、GeoIP、端口、进程名）× 4 种动作（代理/直连/拦截/策略组），双内核各自渲染 |
| 三种模式 | 规则 / 全局 / 直连，首页控制卡直接切换（运行中禁用，下次启动生效），切换不丢规则表 |
| 系统代理 | 启动时接管、停止时还原，原值存快照，异常退出后下次启动自动还原；bypass 列表可编辑；启动前先检查端口是否被别人占着（通常是另一个代理客户端），而不是让内核死在自己的日志里 |
| 节点测速 | 到节点的 TCP 握手耗时，不启动内核也能测，可选每 5 / 15 / 60 分钟自动重测；启动后另有「真实延迟」——经隧道发一个请求量到的往返，含内核与节点 DNS——以及「测速」：经隧道读 5 秒响应体算下载吞吐（上限 8 MiB，只数正文） |
| 内核日志 | 窗口内直接看内核最后 200 行输出 |
| 开机自启 | 写当前用户的启动项（`HKCU\...\CurrentVersion\Run`）；另有「启动时自动连接」把上次的节点拉起来 |
| 托盘常驻 | 关窗只是隐藏；托盘菜单「显示窗口 / 启动·停止内核 / 退出」，图标提示当前节点与端口；退出前先还原系统代理再停内核 |
| 策略组 | Clash 式 `select` / `url-test`：把几个节点打包成一个出口，规则可以指向组；sing-box 下是原生 selector，Xray 下是选中成员的静态副本 |
| 节点页（支持从剪贴板 / 文件导入，把选中链接保存为文件） | Clash 式卡片，可按手动顺序 / 名称 / 延迟 / 协议排序（手动顺序用卡片上的上下箭头调整，会保存），一键全部测速；搜索、按标签筛选（订阅来源的标签显示成订阅名）、多选、批量测速 / 批量打标签 / 批量删除、复制分享链接 |
| 界面语言 | 中文 / English 全量切换，记住选择 |
| 订阅 | 添加机场订阅链接，按设定间隔（或仅手动）后台拉取；解析 Clash YAML / sing-box / Xray 整包与分享链接；刷新只替换该订阅自己的节点；读 `Subscription-Userinfo` 显示已用流量与到期日 |
| 套餐提醒 | 订阅给得出已用 / 总量 / 到期时，首页会在启动控制卡上提示「快用完」「已用尽」「快到期」「已过期」，并给一个去订阅页的入口；停用的订阅不提醒 |
| 规则集 | Clash 式 rule-provider：远程拉取或本地粘贴域名 / IP 列表，路由规则的匹配方式可以整份引用；Xray 内联进配置，sing-box 生成原生 `rule_set` 文件 |
| 内核版本 | 首页显示已装内核自报的版本号（`xray version` / `sing-box version`）；关于页有与 V2RayN / Clash Verge 的能力对照，明确不支持的项逐条给原因 |
| 配置备份 | 导出整个配置（节点、订阅、规则、策略组、规则集条目）到一个 JSON 文件；恢复是整体替换，要求先停内核 |

**尚未实现**：TUN 虚拟网卡、PAC 自动配置脚本、在线更新内核（这三项没做，界面上也不会出现假开关，关于页的「能力对照」逐条写明了原因）；人工验收（界面已可用，但还没有人在真窗口里逐页看过）。

**DNS**：设置页可以填自定义解析器，逗号分隔，三种写法：`1.1.1.1`（UDP）、`tls://1.1.1.1`（DoT）、`https://1.1.1.1/dns-query`（DoH）。会写进内核配置的 `dns` 段——Xray 用 `dns.servers` 字符串数组（保留 `tls://`/`https://` 前缀），sing-box 1.14 用带 `type` 的新格式（`udp`/`tls`/`https`）并以 `dns-0` 作 `final`。留空则完全不动 DNS，交给系统。主机部分只接受 IP 字面量：`dns.google` 这种域名得先被解析，而那就是解析器自己的活。

## 跑起来

```bash
npm install
npm run tauri dev
```

打包：`npm run tauri build`。

### 内核二进制要自己放

内核 exe 不进仓库（体积大、许可不同）。把 `xray.exe` 或 `sing-box.exe` 放进应用数据目录的 `engines/` 下：

```
%APPDATA%\com.xmatrix.client\engines\xray.exe
%APPDATA%\com.xmatrix.client\engines\sing-box.exe
```

首页会在缺文件时直接告诉你期望路径，并给一个「打开目录」按钮。

## 校验

```bash
npm run check          # typecheck + 架构 lint + 契约过期检查 + 前端单测
cd src-tauri && cargo test --lib
```

- `npm run check:arch`：禁止 `src/services/` 之外出现 `invoke(` / `listen(` / `@tauri-apps/api` —— 组件不许直接碰 IPC。
- `npm run check:contract`：`bindings.ts` 必须与 Rust 命令签名一致，过期就报错。
- 前端单测只测纯函数（格式化）；业务逻辑的测试都在 Rust 侧（87 项），因为渲染配置是纯函数，不需要任何内核。

## 结构

```
src/                     前端（Vue 3 + TS）
  services/contract.ts   唯一允许碰 bindings 的模块
  services/bindings.ts   tauri-specta 生成，勿手改
  stores/                Pinia，一个领域一个 store
  views/                 5 个路由页面
  components/            图标、弹窗、开关、空状态、编辑器、图表、日志
src-tauri/src/
  domain/                领域模型（带标签枚举，非法状态不可表示）
  engine/                引擎抽象 + 双内核渲染 + 进程运行时 + 统计
  net/                   最小 HTTP 客户端 + TCP 测速（唯一离开 loopback 的地方）
  sysproxy/              系统代理接管与还原（快照、Drop 兜底）
  commands/              命令层（IPC 边界，65 个命令）
  store.rs               单文件 JSON 原子替换持久化
  autostart.rs           开机自启
docs/ENGINE-SPEC.md      两个内核的行为规格（每条结论都带上游源码行号）
ARCHITECTURE.md          约束 → 机制、状态所有权、里程碑
VISION.md                为什么重做、四条硬约束
```

## 验收

逐页该看到什么、哪些是"明确不支持"，写在 `docs/ACCEPTANCE.md`。

## 已知问题

- **窗口在本机非提权启动会失败**（WebView2 报 `0x8000FFFF` / `0x800700AA`）。已定位为这台机器的进程完整性级别问题：同一 exe 提权启动正常，原生 Win32 最小程序同样复现，与本项目代码无关。开发时用管理员终端跑 `npm run tauri dev`。
- 系统代理与其它客户端（V2RayN 等）同时接管会互相覆盖。启动前先关掉对方。
- 内核二进制缺失时启动按钮会禁用，不会给你一个"点了才知道"的错误。
- **目标机要有 WebView2 Runtime**。Windows 11 自带；Windows Server / LTSC 需要装一次（`MicrosoftEdgeWebView2Setup.exe /silent /install`）。没有它程序起不来，且报错是底层的 `WebView2 error`。
- **Xray 没有连接列表接口**（只有 expvar 的累计流量）。连接页在 sing-box 下能看到逐条连接，在 Xray 下会明确写"该内核不提供连接列表"，不是没接上。
- **Xray 下策略组是"选中成员的静态副本"**。Xray 没有 selector outbound，切成员等于改配置重启；sing-box 下才是内核原生 selector / urltest。
- **REALITY 节点在 sing-box 下可能连不上**：实测 Xray 26.11.9 能连通的 REALITY + `xtls-rprx-vision` 节点，sing-box 1.14.4 的握手被服务端直接重置（`wsarecv: 远程主机强迫关闭了一个现有的连接`）。同一节点、同一份字段（`public_key` / `short_id` / `server_name` / `utls.fingerprint=chrome`）在 Xray 下正常，说明是 sing-box 对最新 REALITY 服务端的兼容问题，不是配置生成错了。遇到这种情况把引擎切到 Xray。
- **自签证书的节点，两个内核的开关不一样**：sing-box 用「允许不安全证书」（`tls.insecure`）；Xray 26 已经**移除**了 `allowInsecure`（写进去会拒绝启动整份配置），必须改用「证书指纹」（`pinnedPeerCertSha256`，64 位十六进制 SHA-256）。节点编辑器里两个都能填，指纹优先级更高也更安全。
- **节点测速是 TCP 握手，不是隧道测试**：延迟低只代表能连到端口。真要确认能不能上网，看首页的速率曲线和内核日志。

## 启动行为

设置页有两个开关决定程序怎么出现和怎么消失：

- **启动时隐藏窗口** —— 配合「开机自启」用，只在托盘里出现；
- **关闭窗口时最小化到托盘** —— 关掉后点 × 就是真的退出（会先还原系统代理、再杀掉内核）。

托盘创建失败时（极少数环境），× 一律退出：留一个看不见又关不掉的窗口比没有托盘更糟。

## 打包

```bash
npm run tauri build
```

产物（`src-tauri/target/release/bundle/`）：

- `msi/X-Matrix_0.1.0_x64_en-US.msi`（3.59 MiB）
- `nsis/X-Matrix_0.1.0_x64-setup.exe`（2.54 MiB）

安装包不含内核：装完把 `xray.exe` / `sing-box.exe` 放进内核目录（关于页有「打开内核目录」按钮）。

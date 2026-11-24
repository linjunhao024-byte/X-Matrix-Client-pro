# X-Matrix 架构设计

> 上游目标见 `VISION.md`。本文只回答一个问题：**怎么保证不再粘连、不再烂**。
> 状态：M0–M3 已落地（引擎层、路由规则、策略组、规则集、系统代理、导入、订阅、持久化、测速/曲线/日志/自启/托盘、中英双语、关于页与首启动引导、全局错误面），并已打出 MSI/NSIS 安装包；剩下人工验收。

---

## 一、四条硬约束 → 落到机制

上一版的四个死因，每条都必须有一个**机制**兜住，光靠自觉没用。

| 约束 | 机制（可执行、可检查） |
|---|---|
| **单一真相源** | 见 §三「状态所有权表」。任何数据只有一个写入方，前端持有的是只读副本。 |
| **一层契约** | `tauri-specta` 从 Rust 命令签名生成 `src/services/bindings.ts`。前端禁止手写 `invoke('...')`，由 `scripts/check-architecture.mjs` 拦截。 |
| **组件化 + 可构建** | Vue SFC + Vite 构建产物；没有运行时模板拼接，没有 3800 行单文件。 |
| **可测** | Rust 侧单测覆盖配置生成/解析/状态机；前端 Vitest 覆盖 store 与纯逻辑。 |

外加一条：**内核能力不自研**。流量统计、连接列表、规则匹配都走内核自己的 API。

---

## 二、目录结构

```
X-Matrix-Client-pro/
├── VISION.md                    # 立项愿景（唯一原点）
├── ARCHITECTURE.md              # 本文
├── docs/
│   └── ENGINE-SPEC.md           # Xray / sing-box 配置与 API 规格（研究产出）
│
├── src/                         # 前端（Vue 3 + Vite + TS）
│   ├── main.ts
│   ├── App.vue
│   ├── router/
│   ├── styles/
│   │   ├── tokens.css           # 设计令牌（浅/深两套，唯一色值来源）
│   │   └── global.css
│   ├── services/
│   │   ├── bindings.ts          # ⚙ 自动生成，禁止手改
│   │   ├── contract.ts          # 唯一 IPC 出口，手写薄封装
│   │   └── events.ts            # 后端事件订阅
│   ├── stores/                  # Pinia：只放「后端状态的副本 + UI 偏好」
│   ├── components/
│   │   ├── ui/                  # 无业务的基础组件
│   │   └── <业务组件>
│   ├── views/                   # 路由页面
│   └── composables/
│
└── src-tauri/                   # 后端（Rust）
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── capabilities/
    └── src/
        ├── main.rs
        ├── lib.rs               # 组装：state / plugins / ipc builder
        ├── ipc.rs               # ⚙ 契约唯一出口（specta builder）
        ├── commands/            # 命令层：只做参数校验 + 调 service
        │   ├── mod.rs
        │   ├── node.rs
        │   ├── engine.rs
        │   ├── runtime.rs
        │   ├── settings.rs
        │   └── system.rs
        ├── domain/              # 领域模型（强类型，无 IO）
        │   ├── node.rs          # Node / Protocol / Transport / Security
        │   ├── profile.rs
        │   └── settings.rs
        ├── engine/              # 内核抽象 + 适配器
        │   ├── mod.rs           # trait Engine
        │   ├── xray.rs
        │   └── singbox.rs
        ├── runtime/             # 进程生命周期 / 状态机 / 事件推送
        ├── api/                 # 内核 API 客户端（stats / connections）
        │   ├── xray_grpc.rs
        │   └── clash_api.rs
        ├── storage/             # SQLite 单一持久化
        └── sys/                 # 系统代理 / 自启 / 路径
```

**分层铁律**：`commands` 不含业务逻辑，`domain` 不含 IO，`engine` 不认识 SQLite。
跨层调用只允许单向：`commands → runtime/engine/storage → domain`。

---

## 三、状态所有权表（解决「三处真相源」）

| 数据 | 唯一所有者 | 存在哪 | 前端能做什么 |
|---|---|---|---|
| 节点列表 | Rust | SQLite | 只读副本；改动走命令 |
| 订阅 | Rust | SQLite | 同上 |
| 设置（端口/DNS/TUN/路由规则） | Rust | SQLite | 同上 |
| 运行态（内核是否运行、当前节点、模式） | Rust | 内存 | 只读副本 |
| 流量/连接快照 | Rust | 内存 | 只读副本（事件推送） |
| 主题 / 侧栏折叠 / 表格密度 / 语言 | 前端 | localStorage | **唯一所有者** |

判定标准一句话：**「重启应用后是否必须还在」→ 是则归 Rust，否则归前端。**
前端 store 里的业务数据一律带 `sync` 标记，只能由 `refresh*()` 从后端拉取覆盖，组件不得直接改写。

---

## 四、契约层

### 4.1 生成方式

```bash
npm run gen:contract      # cargo run --example gen-contract → src/services/bindings.ts
```

Rust 侧：

```rust
// ipc.rs —— 所有命令在这里登记，是唯一出口
pub fn builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::new()
        .commands(tauri_specta::collect_commands![
            commands::node::list_nodes,
            commands::engine::start_engine,
            // ...
        ])
        .events(tauri_specta::collect_events![runtime::events::TrafficTick])
}
```

前端侧：

```ts
// contract.ts —— 组件只准 import 这里
import { commands } from './bindings'
export const api = {
  listNodes: () => unwrap(commands.listNodes()),
  // ...
}
```

### 4.2 规则

1. `bindings.ts` 是生成物，**禁止手改**，禁止提交未重新生成的版本。
2. 组件/视图里**禁止出现** `invoke(`、`listen(` 字符串 —— CI 脚本拦截。
3. 命令返回值统一 `Result<T, AppError>`，`AppError` 是结构化枚举（不是裸字符串）。
4. 命令参数用结构体，不用一堆位置参数（旧版 151 个位置参数方法的下场）。

---

## 五、领域模型（告别裸 dict）

旧版所有数据都是 `dict`，字段靠记。新版的节点是**带标签的枚举**，非法状态无法表示：

```rust
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub server: String,
    pub port: u16,
    pub protocol: Protocol,   // enum：不同协议携带各自的必填字段
    pub transport: Transport,
    pub security: Security,
    pub tags: Vec<String>,
}

pub enum Protocol {
    Vless { uuid: String, flow: Option<String> },
    Vmess { uuid: String, alter_id: u16 },
    Trojan { password: String },
    Shadowsocks { method: String, password: String },
}

pub enum Transport {
    Tcp,
    Ws { path: String, host: Option<String> },
    Grpc { service_name: String },
}

pub enum Security {
    None,
    Tls { sni: Option<String>, alpn: Vec<String> },
    Reality { public_key: String, short_id: String, server_name: String },
}
```

好处：`Trojan` 节点不可能没有 password，`Tls` 不可能出现在 Shadowsocks 上——**编译期就把脏数据挡掉**。

---

## 六、引擎抽象

首发 Xray + sing-box，其余留口子。

```rust
pub trait Engine: Send + Sync {
    fn kind(&self) -> EngineKind;
    /// 内核可执行文件名（运行时按引擎目录解析；找不到 → 明确错误，不静默降级）
    fn binary_name(&self) -> &'static str;
    /// 前台运行指定配置的参数
    fn args(&self, config_path: &str) -> Vec<String>;
    /// 由「节点 + 端口规划」生成完整配置文档（纯函数：不落盘、不 spawn）
    fn render_config(&self, input: &RenderInput<'_>) -> AppResult<serde_json::Value>;
    /// 该内核的统计通道
    fn stats_channel(&self, endpoints: Endpoints) -> StatsChannel;
    /// 节点自检：内核不支持的能力在写盘/启动前就拒绝
    fn validate_node(&self, node: &Node) -> AppResult<()>;
}

pub enum StatsChannel {
    /// Xray：`metrics.listen` → `GET http://127.0.0.1:{port}/debug/vars`（expvar，累计字节）
    XrayExpVar { port: u16 },
    /// sing-box：`experimental.clash_api.external_controller`（`/traffic` 是 WebSocket，给瞬时速率）
    ClashApi { port: u16 },
}
```

加一个内核 = 新增一个 `impl Engine` + 一个适配器测试，**不改架构**。

> 事实修正（2026-11-21，依据 `docs/ENGINE-SPEC.md` §3）：Xray 侧**不是** gRPC StatsService。
> 上游 v2rayN 全仓没有 `StatsService`，也不生成 `api` 段，统计走的是 `metrics` + expvar HTTP
> `/debug/vars`（1s 轮询，累计值→差分）。两家的语义差异（累计 vs 瞬时速率）由统计读取层
> 归一化，枚举里只保留"传输方式"，不把语义塞进类型。

---

## 七、数据流

```
        用户点击
           │
           ▼
   ┌───────────────┐   意图（命令）
   │  Vue 组件      │ ───────────────► ┌──────────────┐
   └───────────────┘                  │ Rust commands│
           ▲                          └──────┬───────┘
           │ 只读副本                        │
           │                          ┌──────▼───────┐
   ┌───────┴───────┐   事件推送        │  domain/     │
   │ Pinia store   │ ◄─────────────── │  engine/     │
   └───────────────┘   (traffic/log)   │  storage/    │
                                       └──────────────┘
```

**单向**：组件 → 命令 → 后端状态 → 事件/查询 → store → 组件。
组件永远不「改」状态，只「请求」后端改，然后等事件确认。

---

## 八、里程碑

| 里程碑 | 交付 | 验收 |
|---|---|---|
| **M0 骨架** | Tauri 2 跑起来、契约生成链路通、一条端到端命令 | `npm run tauri:dev` 出窗口；`gen:contract` 产出 bindings；一个命令能被前端调用并渲染 |
| **M1 可用** | 节点 CRUD + 订阅导入 + Xray 启停 + 系统代理 | 导入真实节点 → 激活 → 浏览器能出网 |
| **M2 好看** | 设计系统 + 5 个视图（首页/节点/连接/路由/设置） | 浅深双主题、无写死色值、首屏 1s 出内容 |
| **M3 完整** | 流量图表、连接雷达、日志、sing-box 支持、TUN | 两个内核都能跑，统计数字对得上 |

---

## 九、与旧版的对照（防止回潮）

| 旧版做法 | 新版禁止 | 检查手段 |
|---|---|---|
| 151 个裸 dict 方法 | 强类型命令 + 结构体参数 | 类型检查 |
| 前端 localStorage 存业务配置 | 业务数据只存后端 | code review + 所有权表 |
| 单文件 3882 行 HTML | 路由 + 组件 | 单文件行数上限 400 |
| 全局对象 `Object.assign` 合并 | Pinia 分模块 store | 目录结构 |
| 手写 `invoke('xxx')` | 只用 `contract.ts` | `check-architecture.mjs` |
| 前端 0 测试 | Vitest + Rust 单测 | `npm test` / `cargo test` |

---

### 单实例与外观一致性（验收期补的两条）

- **单实例**：`tauri-plugin-single-instance` 注册为**第一个**插件。第二次启动会把参数交给正在运行的实例、把它从托盘唤到前台，然后自己退出——否则会出现两个托盘图标、两份状态文件互相覆盖（验收时真的踩到了）。
- **标题栏跟随主题**：标题栏是操作系统画的，CSS 管不到。前端每次解析出主题就调 `ui_set_window_theme(dark)`，让系统把框也刷成深色；否则深色界面配白色窗框像 bug。

## 十、当前进度（按代码说话）

| 层 | 状态 | 证据 |
|---|---|---|
| 契约层 | 完成 | `tauri-specta` 生成 `src/services/bindings.ts`，77 个命令；`npm run check:contract` 校验不过期 |
| 引擎层 | Xray + sing-box 配置渲染、启停、就绪探测、统计读取、日志尾部、启动前端口预检 | `cargo test --lib`（全库 357 项）；渲染是纯函数，测试不需要任何内核二进制 |
| 数据层 | 单文件 JSON 原子替换 + 坏文件留证 | `store.rs`，重启后节点/设置/规则都在 |
| 导入 | vless / vmess / trojan / ss + 整包 base64 | `import.rs`，不支持的行进 `skipped` 而不是报错 |
| 路由 | 7 种匹配器 × 4 种动作（代理/直连/拦截/策略组），双内核各自渲染 | `domain/routing.rs` + `render.rs`（唯一的内核差异点） |
| DNS | 设置页填自定义解析器（逗号分隔），三种写法：`1.1.1.1` / `tls://1.1.1.1`（DoT）/ `https://1.1.1.1/dns-query`（DoH）。`DnsEntry::parse` 解析成带标签的枚举，两个渲染器各说各的方言：Xray 拼回 `tls://`/`https://` 字符串，sing-box 用 `{type:"udp"|"tls"|"https", tag:"dns-N"}` + `final`。留空完全不生成 `dns` 键。主机部分只接受 IP 字面量（域名要先被解析，那正是解析器的活） | `domain/settings.rs` 的 `DnsEntry`、`engine/xray.rs`、`engine/singbox.rs`，含 4 项解析校验 + 3 项渲染测试；`xray -test` 与 `sing-box check` 真二进制验过 |
| 系统代理 | 快照 → 接管 → 还原，异常退出可恢复 | `sysproxy/mod.rs`，7 项测试跑在假后端上 |
| 启动行为 | 「启动时隐藏窗口」「关闭窗口时最小化到托盘」两个设置：前者在 setup 末尾 `window.hide()`（先创建再隐藏，避免闪一下）；后者决定 × 是 hide 还是 `quit_cleanly`（还原系统代理 + 杀内核 + 退出）。没有托盘时 × 一律退出——否则窗口既看不见又关不掉 | `src-tauri/src/lib.rs`、`domain/settings.rs`（2 项测试） |
| 窗口几何 | `tauri-plugin-window-state` 记住大小、位置、最大化状态；flags 只要这三样——`visible` 归「启动时隐藏」管，把磁盘上的可见性也还原回来会跟设置打架。还原前过一次 `ensure_window_reachable`：拔掉扩展屏后旧坐标可能落在不存在的显示器上，落在任何显示器外就 `window.center()`，宁可不记得也不能还原出一个看不见的窗口 | `src-tauri/src/lib.rs` 的 `ensure_window_reachable`、`Cargo.toml` |
| 系统集成 | 开机自启（写 HKCU Run）、启动时自动连接（`settings.auto_connect` + 记住上次的节点与内核）、托盘三动词（显示窗口 / 启停内核 / 退出，标签与 tooltip 跟随状态）、节点 TCP 测速、流量曲线、内核日志（级别可选 warn/info/debug）、配置预览 | `autostart.rs` / `lib.rs` 的 `auto_connect`、`toggle_kernel`、`refresh_tray` / `net/latency.rs` / `runtime.log_tail` |
| 速度测试 | 完成 | `proxy_speed` 经隧道读 5 秒 / 上限 8 MiB 的响应体算吞吐（`net/latency.rs::download_speed_via_proxy`，只数正文不数响应头），首页「测速」按钮显示 `下载速度 xx MB/s` |
| 规则试跑 | 完成 | `test_domain` 按内核的顺序从上到下匹配，只报第一条命中；geoip / 端口 / 进程三种规则无法只凭域名判断，如实计入 `skipped` 而不是猜 |
| 文件导入导出 | 完成 | 导入会先按分享链接解析，一条都不认得时再按整份文档（Clash YAML / sing-box JSON）解析——粘贴一份配置不该静默得到零个节点； 节点链接可「保存为文件」（`save_text_file`），导入对话框可「从文件读取」（`open_text_file`，4 MB 上限）——订阅、Clash 配置、链接列表都只是文本，所以导入只有一条代码路径 |
| 内核版本 | 完成 | 首页显示已装内核自报的版本（`engine_version` 跑 `version`，不读配置、不开端口）；关于页列出与 V2RayN / Clash Verge 的能力对照，TUN / PAC / 在线更新内核明确写「不支持」并给原因 |
| 托盘 | 关窗隐藏、左键唤回、菜单退出（退出前先还原代理再停内核） | `lib.rs` 的 `build_tray` / `quit_cleanly` |
| 前端 | 9 个视图 + 组件库 + 单一轮询器 | `vue-tsc` / `vite build` / `check:arch`（52 文件）/ `vitest`（57 项，含 store → IPC 接线测试）全绿 |
| 套餐提醒 | 后端只如实存「已用 / 总量 / 到期」，判断留给前端一处：`quotaReport` 按「已过期 > 用尽 > 用掉九成 > 一周内到期」出结论，`quotaAlerts` 过滤掉停用的源并按严重度排序；首页在启动控制卡里以 `alert-warn` 提示「快用完 / 已用尽 / 快到期 / 已过期」并给出订阅页入口。停用源不提醒——用户已经说了不用它，再唠叨就是噪音 | `src/services/quota.ts`（8 项测试）、`HomeView` 的 `quotaNotices` |
| 订阅 | 拉取（HTTP，UA 可配置）、整包解析（Clash YAML / sing-box / Xray / 分享链接）、只替换自己那批节点、**跨订阅按 `kind|server|port|credential` 去重**（手工节点不参与比较）、后台每 60 秒扫一次到期项、`subscription-userinfo` 头 → 已用/总量/到期 | `commands/subscription.rs`、`documents.rs`、`net/remote.rs`（`Fetched` 带响应头），含配额解析 3 项测试、去重 2 项测试 |
| 策略组 | `select` / `url-test`；sing-box 渲染成原生 selector/urltest，Xray 渲染成选中成员的静态副本（Xray 没有 selector，UI 会说出来）；删组时指向它的规则自动回落到「代理」，删节点时从各组里摘掉 | `domain/group.rs`、`commands/group.rs`、`engine/{xray,singbox}.rs` |
| 界面多语言 | 中文 / English 全量；`scripts/i18n-pairs.json` 是唯一真相源，`scripts/gen_i18n.py` 生成两份字典，测试断言键集一致（半翻译会 fail 而不是上线）；语言存 `localStorage['xm.ui.locale']` | `src/i18n/`、`src/i18n/i18n.test.ts` |
| 节点页 | Clash 式卡片 + 延迟排序（未测速的排在最后，避免列表跳动）+ 一键全部测速（串行，避免测到队列而不是节点）+ 搜索 / 多选 / 批量测速 / 批量删除 / 复制分享链接 | `views/NodesView.vue` |
| 节点标签 | 自由标签（导入/订阅之外的归类手段）：编辑器里逗号分隔输入、卡片显示 chip、按标签筛选、选中后批量打/移除；`sub:` 前缀被保留给订阅（刷新按 tag 删节点，手写同名标签会被悄悄吃掉），所以后端拒绝用户写它，编辑器把它显示成只读 | `domain/node.rs` 的 `normalized_tags` / `check_user_tags`、`commands/node.rs` 的 `apply_tag`（6 项测试） |
| 分享链接导出 | 导入的逆运算：`export.rs` 把节点重新拼成 `vless://` / `vmess://` / `trojan://` / `ss://`；往返测试（导出 → 解析 → 六项字段相等）钉住正确性；表达不了的组合直接报错而不是拼一条假链接 | `export.rs`（9 项测试）、`commands/node.rs` 的 `delete_nodes` / `export_node_links` |
| 规则集 | Clash 式 rule-provider：远程拉取或本地粘贴，解析 Clash `payload:` / 纯域名 / CIDR / v2ray 前缀；路由规则的匹配方式可以整份引用。Xray 没有规则集概念，**必须内联**，且因为 Xray 对同一条 rule 的字段是 AND，混合集合会被拆成「域名一条 + IP 一条」；sing-box 有原生 `rule_set`，渲染成 `<config>/rule-sets/<id>.json`（**source 格式是 JSON**：`{"version":1,"rules":[…]}`，按条目类型分组成多条 rule，因为一条 rule 内的字段是 AND）并在 `route.rule_set` 里声明；空集合两边一起跳过，启动时顺手清掉目录里已删集合留下的文件。删集合时引用它的规则被**停用**而不是删掉 | `domain/ruleset.rs`、`rulesets.rs`、`commands/ruleset.rs`、`render.rs` |
| 发布打磨 | 完成 | `AboutView`（版本 / 四个目录 / 许可 / 内核放哪里，路径可复制）、首启动三步引导（`localStorage['xm.ui.onboarding-dismissed']`）、`app.config.errorHandler` + `unhandledrejection` 汇到一条可关闭的错误条；`src-tauri/icons/` 全部 PNG 由 `icon.ico` 重生成 |
| 打包 | 完成 | `npm run tauri build` → `msi/X-Matrix_0.1.0_x64_en-US.msi`（3.59 MiB）、`nsis/X-Matrix_0.1.0_x64-setup.exe`（2.54 MiB） |
| 云电脑实测 | 干净的 Windows Server 2022 + WebView2 157：9 个页面、弹窗、深浅两套主题逐页截图核对；数据目录正常创建；**真内核**（Xray 26.11.9 / sing-box 1.14.4）启动、配置生成、统计、连接列表、停止与系统代理往返全部跑通；订阅与规则集的远程拉取用本机 http provider 真拉过；强杀后内核被作业对象带走、代理下次启动自动还原；自建 vless+ws / vless+REALITY / vless+TLS 三种服务端，把两内核的隧道逐一验过（结论：ws 与 TLS 两边都通，REALITY 只有 Xray 通）。结论表见 `docs/ACCEPTANCE.md` §6 | `docs/ACCEPTANCE.md` |
| 会话流量 | 内核报的是「累计」（Xray）或「滑动窗口」（sing-box），都回答不了「这次连了多少」。首页按两次采样的差值累加出本次会话的上下行，可清零；清零只清计数器不动基线，否则下一次采样会把内核总量当成一次尖峰报出来 | `src/stores/engine.ts` 的 `session` / `resetSession`（3 项测试） |
| 连接筛选 | 连接页可按目标/规则/链路搜索，并按最新、下载、上传、目标排序。排序在客户端做：列表本来就小，且每轮采样重排会让正在读的行跳走 | `src/views/ConnectionsView.vue` |
| 连接汇总 | 同一张表可切成「按目标」/「按出口」两种汇总（连接数 + 上传/下载合计，最忙的排最前）。Clash 的「按规则」「按进程」两种视图我们做不了：sing-box 的 Clash API 不给进程字段，且它的 `route.rules[].tag` 是未知字段（`sing-box check` 实测 `json: unknown field "tag"`），所有连接都被标成同一个 `final` | `src/services/connections.ts`（4 项测试） |
| 真实延迟 | 经**运行中的隧道**发一个 HTTP 请求量到的往返（V2RayN 叫 real delay）：包含内核、隧道和节点自己的 DNS，只有跑着才有数。节点列表里那个数是到节点的 TCP 握手，启动前就能测，两者各有用处 | `net/latency.rs` 的 `http_latency_via_proxy`（3 项测试，含「502 不算延迟」）、`commands/engine.rs` 的 `proxy_latency` |
| 连接管理 | 连接页每行可「断开」（sing-box 的 Clash API `DELETE /connections/{id}`）；Xray 没有连接表，按钮直接不出现而不是点了才报错 | `engine/stats.rs` 的 `close_connection`、`views/ConnectionsView.vue` |
| 配置备份 | 导出把整个 JSON 文档 + 每个规则集的条目文本写进一个文件；恢复是**整体替换**而不是合并（合并会造出重复 id 和指向不存在策略组的规则），并且要求先停内核；解析有 4 项测试（往返、外来 JSON 被拒、坏节点被拒、乱码被拒） | `commands/backup.rs`、`state.rs` 的 `document` / `replace` |
| 系统代理例外 | 接管系统代理时写进注册表的 bypass 列表可编辑（默认本机回环 + 内网段）；留空回落默认值——写空的 `ProxyOverride` 会把本机回环也推进隧道，本机服务全断 | `sysproxy/mod.rs` 的 `enable_with_bypass`（2 项测试）、设置页「代理例外」 |
| 自动测速 | 设置里可选 5 / 15 / 60 分钟重新测一遍全部节点，默认关（按流量计费的人会注意到后台在打节点）；手动测速进行中时自动的那次跳过，避免两次扫描互相排队 | `views/NodesView.vue` 的 `armAutoTest` |
| 端口预检 | 启动前先 `bind` 一次混合端口与控制端口：内核绑不上只会死在自己的日志里，而桌面上最常见的原因是另一个代理客户端还占着 7890。只在**空闲时**检查——内核跑着的时候端口本来就是它自己占的，那时检查会拒绝每一次切节点。绑定即检测，不先问 OS 再绑（中间会有窗口期） | `net/ports.rs`（2 项测试）、`commands/engine.rs` 的 `start_engine` |
| 从剪贴板导入 | 机场链接常常是浏览器里复制的一大串 base64，让人手选两百个字符就是给错字机会。剪贴板 API 在非安全上下文不可用，拒绝时明确提示「手动粘贴」，不静默失败 | `components/ImportDialog.vue` 的 `loadFromClipboard` |
| 首页代理模式 | 规则 / 全局 / 直连放在首页控制卡，不必进设置页。模式在渲染时烤进配置，所以运行中控件是禁用的（点了不生效比点了没反应更糟）；空闲时切换对下一次启动生效 | `views/HomeView.vue` 的 `setMode` |
| 未做 | 人工验收的写类操作（导入、启动内核、接管系统代理） | 需要真实内核与网络，按用户要求不在本机跑 |

### 真机才暴露的九个 bug（2026-11-22 修）

单元测试全绿也发现不了，因为它们照着**我以为的**格式写的；只有真内核会拒绝。

6. **sing-box 1.14 一旦出现 `dns` 段，就必须给 `route.default_domain_resolver`**：只写 `dns.servers` 会让内核 FATAL（`missing route.default_domain_resolver ... deprecated in 1.12.0 and will be removed in 1.14.0`）。修法是生成 `dns` 时同时写 `route.default_domain_resolver = "dns-0"`，并用 `sing-box check -c` 在真二进制上验过（exit=0）。

1. **sing-box 规则集文件格式**：原来写成 `domain_suffix:example.org` 行列表，而 `format: "source"` 要的是
   JSON。sing-box 启动时 `FATAL parse rule-set[0]: invalid character 'd'` 直接退出——**一个 provider 文件
   能让整个内核起不来**。现在写 `{"version":1,"rules":[{"domain_suffix":[…]},{"ip_cidr":[…]}]}`，并且
   「集合为空」时声明与引用一起跳过（引用未声明的 rule-set 同样是配置错误）。
2. **Xray 域名后缀前缀**：原来用 `suffix:example.com`，Xray 的后缀匹配其实是 `domain:`，未知前缀会被
   当成普通关键字 → **规则永远不命中**，流量静默走默认出站。改完在真机上验证：集合内域名日志是
   `[socks -> direct]`，集合外是 `[socks >> proxy]`。

3. **带 BOM 的配置被当成损坏**（不在渲染层，在存储层）：PowerShell 5.1 的 `Set-Content -Encoding UTF8`
   会写 BOM，`serde_json` 读不了，于是整份 `store.json` 被挪成 `store.corrupt`、程序从默认值启动——
   用户的节点、规则、设置看起来全没了。现在读盘前剥掉 BOM（记事本改配置是日常操作，不能当损坏处理）。
4. **Xray 26 删了 `allowInsecure`**：写进 `tlsSettings` 时 Xray 不是忽略而是**拒绝启动**
   （`The feature "allowInsecure" has been removed and migrated to "pinnedPeerCertSha256"(pcs) and
   "verifyPeerCertByName"(vcn)`）——一个节点的字段能废掉整份配置。现在 Xray 侧只渲染
   `pinnedPeerCertSha256`（证书指纹固定，比跳过校验更严格），`allowInsecure` 只对 sing-box 生效
   （`tls.insecure`）。
5. **`pinnedPeerCertSha256` 是字符串**：写成数组报 `cannot unmarshal array into … of type string`，
   截断的指纹报 `incorrect pinnedPeerCertSha256 length`（同样让整份配置起不来）。现在渲染成逗号拼接的
   字符串，`Node::validate` 与节点编辑器都强制 64 位十六进制。

教训写在这里：渲染层每一条「前缀/字段名」都必须对着真内核验一次，或者至少对着官方文档抄，别凭印象；
存储层则要假设「文件会被别的编辑器改过」。

### 真实节点实测（2026-11-22）

拿到一个真实节点（VLESS + REALITY + `xtls-rprx-vision` + tcp）后跑通的部分：

- Xray 下 `curl -x 127.0.0.1:7890 https://ipinfo.io/json` → 出口 `192.3.222.51`（Los Angeles），
  内核日志 `[socks >> proxy]`，`/debug/vars` 有真实字节；**系统代理接管后不指定代理的
  `Invoke-WebRequest` 也返回同一个出口 IP**，说明 Windows 层面的流量确实进了隧道。
- 为此给 `Security::Reality` 补了 `fingerprint`（`fp`）与 `spider_x`（`spx`）两个字段：
  之前渲染层把 fingerprint 写死 `chrome`、spiderX 直接丢掉，节点换指纹就得连不上。
- sing-box 1.14.4 对同一节点握手被服务端重置（详见 `docs/ACCEPTANCE.md` §6「互操作限制」），
  所以节点页加了一行实话：**测速只量到节点的 TCP 握手，不代表隧道一定能通**。

### 自签证书与证书指纹（2026-11-22 加）

TLS 是最常见的节点形态，而自签证书/裸 IP 在默认配置下必然校验失败，所以两条内核各按自己的能力实现：

| 能力 | Xray 26 | sing-box 1.14 |
|---|---|---|
| 跳过证书校验 | **已移除**（写了就拒绝启动） | `tls.insecure: true` |
| 证书指纹固定 | `tlsSettings.pinnedPeerCertSha256`（**字符串**，64 位十六进制） | 未渲染（sing-box 的 `certificate_public_key_sha256` 钉的是 SPKI，语义不同） |

节点模型里两个字段并存：`allow_insecure`（链接里的 `allowInsecure=1` / Clash 的 `skip-cert-verify`）与
`pinned_cert_sha256`（链接里的 `pcs` / Clash 的 `fingerprint`）。渲染时按内核各取所需，UI 把差别写进说明。
实测：自建 vless+tls 服务端（自签 CN=local.test），Xray 用指纹固定、sing-box 用跳过校验，两边都是 200。

### 验收期修的排版问题（宽窗口/窄窗口都要能看）

在云电脑上按真实窗口逐页看过之后修的，记在这里免得以后又改回去：

- **页头换行而不是压缩**：`.page-header` 加 `flex-wrap: wrap`，标题块 `flex: 1 1 320px`，动作块 `flex: 0 0 auto`。原来窗口窄一点时副标题会在词中间断开、引擎切换器把「sing-box」压成两行。
- **分段控件不收缩、不折行**：`.segmented` 加 `flex: 0 0 auto`，按钮加 `white-space: nowrap`。
- **弹窗字段有节奏**：`.modal-body` 改成 `display: flex; flex-direction: column; gap: 14px`。原来插槽内容是裸堆叠，下一个字段的标签紧贴上一个输入框，看着像同一组。
- **信息不是错误**：新增 `.alert-info`（accent 底）。「Xray 下策略组是静态副本」这类说明原来借用 `.alert`（红底），把提示写成了报警。
- **工具栏次要按钮统一有边框**：刷新类按钮原来用 `btn-ghost`（无边框），和旁边的「新建」并排像纯文字。表格行内的操作仍保留 ghost（那是链接式操作）。
- **路径不拆词**：关于页的目录路径 `overflow-wrap: anywhere` 会把 `com.xmatrix.client` 劈成两行，改成 `white-space: nowrap; overflow-x: auto`。
- **空图表不占满**：`TrafficChart` 在没数据时（少于两个采样点）把容器收到 76px，只留一行提示。原来固定 132px 里飘着一句话，看着像图加载失败。
- **运行中换内核要说明原因**：内核跑着的时候引擎切换器和节点下拉都是禁用的，禁用却没有解释等于坏掉——现在切换器下面直接写「内核运行中不能切换，先停止再换」。
- **宽表格不许把操作列裁掉**：三个表格页的 `.table-wrap` 从 `overflow: hidden` 改成 `overflow-x: auto`（八列在 1050px 窗口里放不下，`hidden` 会让「状态 / 操作」直接消失且无法滚动到），同时把 `.data-table td/th.actions` 粘在右侧（`position: sticky; right: 0`，配不透明底色，否则滚动的单元格会透出来），窄窗口下按钮始终在手边。
- **配额不做成独立一列**：`12.5 GB / 100.0 GB · 到期 2026-03-01` 作为一列有 230px 宽，在 1290px 窗口里正好是被挤出视野的那一列——而这页就是为看它才打开的。改成名称下面的两行（`12.5 GB / 100 GB` + `到期 2026-03-01`），并把地址列收进名称的 tooltip（编辑框里仍有），六列才排得下。
- **点遮罩不能吃掉填了一半的表单**：`ModalDialog` 加 `closeOnOverlay`（默认 `true`）。表单类对话框（节点 / 规则 / 策略组 / 导入 / 规则集 / 订阅编辑器）传 `false`，误点面板旁边不再丢掉已输入内容；确认框与只读对话框保持可点遮罩关闭。Esc 与「关闭」按钮仍然有效——那是明确动作。

### 系统代理的所有权规则（M2 定稿）

**只有「启动内核」能接管系统代理，只有「停止内核」和「还原按钮」能交还。**

- 接管顺序：内核就绪 → 写注册表；交还顺序：还原注册表 → 杀内核。反过来会在切换的瞬间留下一个指向死端口的代理；
- 接管前先把原来的 `ProxyEnable`/`ProxyServer`/`ProxyOverride` 写进 `proxy-snapshot.json`，还原时逐字写回；
- 没有快照时**什么都不做**——我们没拿过的东西，不许碰；
- 启动时若发现残留快照，说明上次是异常退出，先还原再继续。

这一条是旧版没做到、Clash Verge 也没做（它只在退出时 `clear()` 全关）的地方，写在代码注释里防止以后被"简化"。

### 订阅的所有权规则（M2 定稿）

一个订阅**只**拥有它带进来的节点，判据是节点上的 `sub:<id>` 标签：

- 刷新 = 删掉旧标签的那批 + 写入新的那批，手加的节点一个都不碰；
- 删除订阅 = 连它那批节点一起删（UI 明确说明这一点，不做静默清理）；
- 自动更新只由 `enabled` + `update_interval_minutes` 决定（`0` 表示"只在我点刷新时更新"），后台线程每 60 秒看一次谁到期；
- 失败写回 `last_error` 并刷新 `updated_at_ms`：一条悄悄死掉的订阅，比一条报错的订阅更糟。

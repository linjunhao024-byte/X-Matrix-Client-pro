# P1 批次 A 报告：engine + sysproxy 迁入 services 并开 HTTP 路由

日期：2026-11-29
范围：`commands/engine.rs`（22 条）与 `commands/sysproxy.rs`（2 条）；只做这一批，批次 B 未动。

## 1. 交付物

| 文件 | 变化 |
| --- | --- |
| `src-tauri/src/services/mod.rs` | 新增：`Ctx`（可选 state / proxy + runtime + `Notifier`）、`EmptyArgs`、`NoopNotifier`/`NOOP_NOTIFIER` |
| `src-tauri/src/services/engine.rs` | 新增：22 条命令的业务体 + 参数结构 + `StartRequest` / `prepare_start` / `start_engine` / `stop_if_running_node` / `render_for_preview` / 延迟探测等助手 |
| `src-tauri/src/services/sysproxy.rs` | 新增：2 条系统代理命令的业务体 |
| `src-tauri/src/commands/engine.rs` | 变薄壳：签名与 `///` 文档一字未改，函数体 = 构造 `Ctx` → 调 `services::engine`；`pick_engine_binary`、`engine_open_dir` 保留原生实现并注明不进路由；原有测试模块原样保留 |
| `src-tauri/src/commands/sysproxy.rs` | 变薄壳，签名与文档未改 |
| `src-tauri/src/core/endpoints.rs` | `dispatch` 改为查 `ROUTES` 表；新增 22 条 `POST /<module>/<command_name>`；`NOT_ROUTED` 白名单；`handle()` 统一反序列化/包装错误；新增 24 条路由单测 + 覆盖测试 |
| `src-tauri/src/core/mod.rs` | `CoreContext` 增加 `traffic: Arc<TrafficLog>`（traffic 路由要读与窗口同一个文件），`serve()` 用 `base/traffic.jsonl` 构造 |
| `src-tauri/src/core/server.rs` | 测试里的 `CoreContext::new` 跟上四参数 |
| `src-tauri/src/lib.rs` | 注册 `pub mod services;` |
| `src-tauri/src/ipc.rs` | **未改**（注册表是覆盖测试的输入） |
| `src/services/bindings.ts` | **未改**；重新生成后与提交版逐字节相同 |

## 2. 本批 24 条命令 → 路由 → service → 单测

`POST` 省略；单测均在 `core::endpoints::tests`。

| 路由 | service 函数 | 单测 |
| --- | --- | --- |
| `/engine/engine_status` | `services::engine::engine_status` | `engine_status_route_answers_the_idle_status` |
| `/engine/engine_kinds` | `services::engine::engine_kinds` | `engine_kinds_route_lists_both_kernels` |
| `/engine/engine_binaries` | `services::engine::engine_binaries` | `engine_binaries_route_reports_paths_and_absence` |
| `/engine/engine_version` | `services::engine::engine_version` | `engine_version_for_a_missing_binary_is_404` |
| `/engine/engine_log_tail` | `services::engine::engine_log_tail` | `engine_log_tail_route_reads_an_empty_log`（另有 `a_missing_kind_on_engine_log_tail_is_400`） |
| `/engine/engine_log_delta` | `services::engine::engine_log_delta` | `engine_log_delta_route_returns_the_zero_delta` |
| `/engine/engine_log_clear` | `services::engine::engine_log_clear` | `engine_log_clear_route_is_ok_without_a_log_file` |
| `/engine/proxy_latency` | `services::engine::proxy_latency` | `proxy_latency_route_without_a_kernel_is_a_500_engine_error` |
| `/engine/proxy_speed` | `services::engine::proxy_speed` | `proxy_speed_route_without_a_kernel_is_a_500_engine_error` |
| `/engine/group_delay` | `services::engine::group_delay` | `group_delay_for_a_missing_group_is_404` |
| `/engine/node_delay` | `services::engine::node_delay` | `node_delay_for_a_missing_node_is_404` |
| `/engine/close_connection` | `services::engine::close_connection` | `close_connection_route_without_a_kernel_is_a_500_engine_error` |
| `/engine/close_connections` | `services::engine::close_connections` | `close_connections_route_without_a_kernel_is_a_500_engine_error`（另有 `ids_must_be_a_list_on_close_connections`） |
| `/engine/engine_start` | `services::engine::engine_start` | `engine_start_and_stop_routes_drive_the_fake_runtime` |
| `/engine/engine_apply` | `services::engine::engine_apply` | `engine_apply_route_starts_the_node` |
| `/engine/engine_stop` | `services::engine::engine_stop` | `engine_start_and_stop_routes_drive_the_fake_runtime` |
| `/engine/traffic_sample` | `services::engine::traffic_sample_now` | `traffic_sample_route_is_null_without_a_stats_channel` |
| `/engine/traffic_history` | `services::engine::traffic_history` | `traffic_history_route_starts_empty` |
| `/engine/traffic_clear_history` | `services::engine::traffic_clear_history` | `traffic_clear_history_route_answers_null` |
| `/engine/engine_config_preview` | `services::engine::engine_config_preview` | `engine_config_preview_route_renders_for_a_known_node` |
| `/sysproxy/system_proxy_status` | `services::sysproxy::system_proxy_status` | `system_proxy_status_route_reports_inactive` |
| `/sysproxy/system_proxy_disable` | `services::sysproxy::system_proxy_disable` | `system_proxy_disable_route_is_idempotent` |
| 无路由（`NOT_ROUTED`） | `pick_engine_binary` | 理由：Tauri 原生文件选择对话框，headless core 没有窗口可弹 |
| 无路由（`NOT_ROUTED`） | `engine_open_dir` | 理由：在所属机器上打开 Explorer，属于窗口进程的壳动作 |

已有 6 条 P0 路由（`GET /health`、`GET /status`、`GET /nodes`、`POST /engine/start`、`POST /engine/stop`、`POST /shutdown`）原样保留，handler 只是改成函数指针后转调原函数。

覆盖测试：`every_ipc_command_has_a_route()` 用 `include_str!("../ipc.rs")` 扫注册表，只对 `COVERED_MODULES = ["engine", "sysproxy"]` 严格；本批校验数固定为 24，后续批次把模块名加进数组即可自动收紧。另有 `no_two_routes_share_a_method_and_path()` 防重复。

## 3. 验证结果

- `cargo test --lib`：`test result: ok. 590 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`（全部走 FakeSpawner/FakeProbe，未起任何真实内核；系统代理用 `OffProxy`，写入即报错）
- `npm run check`：通过 —— `typecheck` ok、`architecture ok (70 files checked)`、`check:contract` 通过（重新生成后与 `src/services/bindings.ts` 字节相同）、`vitest 136 passed`
- `git status --porcelain`：`M src-tauri/src/{commands/engine.rs, commands/sysproxy.rs, core/endpoints.rs, core/mod.rs, core/server.rs, lib.rs}`、`?? src-tauri/src/services/`（另有任务自带的未跟踪 `docs/REWRITE-P1-TASK.md`，未改动）

## 4. 设计决定

1. **表驱动路由**：`ROUTES: &[Route]` 是唯一事实源，`dispatch` 只做 token 校验 + 查表；覆盖测试直接读同一张表，不会出现“测试绿但真路由没接”。
2. **薄壳不碰签名**：22+2 条命令的名字、参数名、返回类型、`///` 文档全部保留；`bindings.ts` 重新生成后逐字节一致，`check:contract` 通过。
3. **异步命令保持 `run_blocking`**：把业务拆成「取目标（借 state/runtime，同步）+ 阻塞测量（`'static` 闭包）」两段，`engine_start`/`engine_stop` 同理，避免把 `State` 借入 `'static` 闭包。
4. **`Ctx` 的 state/proxy 是可选的**：不是每条命令都需要窗口状态或代理守卫；缺失时返回 `AppError::Internal` 而不是 panic。
5. **`CoreContext` 增加 `traffic`**：traffic 三条路由要读与窗口同一个 `traffic.jsonl`；core 进程自己持有一份，避免每次请求新建实例读到空历史。
6. **`engine_kinds` 无 runtime**：命令签名本来就没有，路由直接忽略 `ctx`。
7. **不做的事件**：核对 `git show HEAD:...commands/engine.rs` 与 `sysproxy.rs`，这批命令原本没有任何 `app.emit`，所以批次 A 只接了 `NoopNotifier`；`AppHandle` 版 `Notifier` 等到第一个真正要发事件的批次再加，现在加就是死代码。

## 5. 遗留问题 / 拿不准的点

1. **`AppError` 没有 `Conflict`/`Unsupported`**：`bindings.ts` 导出了 `AppError` 联合类型，加变体会让 `check:contract` 漂移。因此「内核没在运行」目前是 HTTP 500 + `code:"engine"`，语义上更像 409。要改就得同时改契约与前端（P2），本批没动。
2. **services 反向依赖 commands**：`services::engine::prepare_start` / `render_preview` 调 `crate::commands::ruleset::loaded_sets`（规则集缓存还在 commands 里）。等 ruleset 批次迁完，应改为 services 内部读取，消除这处分层倒挂。
3. **traffic 的所有者**：单进程模式下窗口自己 `manage` 一个 `TrafficLog`，core 模式又建一个；两者写同一个路径但目前不会同时存在。P2 把前端改成轮询 core 后，需要明确“只有 core 写”。
4. **HTTP 侧的延迟命令会占住连接线程**：`proxy_latency`/`proxy_speed`/`node_delay`/`group_delay` 同步执行，最长约 `DELAY_BUDGET_MS = 5s`。core 是每连接一线程，能接受，但客户端要设超时。
5. **`engine_open_dir` 的归类**：它确实有业务含义（打开日志/配置目录），但动作是“在用户桌面上弹 Explorer”，headless core 不该做，因此放进 `NOT_ROUTED`；复核若认为应改成“返回路径”由壳去打开，可以下批调整（那会改签名，需走契约）。
6. **复杂参数**：本批参数最多是 `EngineKind`（枚举，serde kebab-case）、`Vec<String>`（`close_connections.ids`）、`f64`（`since`/`minutes`）。没有嵌套对象，HTTP JSON 体与 Tauri 的 camelCase 参数一一对应；`EmptyArgs` 忽略未知字段，无参命令传 `{}` 或空体都行。
7. **`traffic_sample` 在没有 stats channel 时返回 200 + `null`**（与原命令语义一致：`Option<TrafficSample>`），不是错误。
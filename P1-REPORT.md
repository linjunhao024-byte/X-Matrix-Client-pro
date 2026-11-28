# P1 批次 A+B+C 报告：65 条命令迁入 services 并开 HTTP 路由

日期：2026-11-29
范围：批次 A —— `commands/engine.rs`（22 条）+ `commands/sysproxy.rs`（2 条）；
批次 B —— `commands/node.rs`（12 条）+ `commands/group.rs`（5 条）+ `commands/process.rs`（1 条）+ `commands/import.rs`（2 条）；
批次 C —— `commands/subscription.rs`（6 条）+ `commands/ruleset.rs`（9 条）+ `commands/routing.rs`（6 条）。
合计 65 条。批次 D 未动。

## 1. 交付物

| 文件 | 变化 |
| --- | --- |
| `src-tauri/src/services/mod.rs` | 新增：`Ctx`（可选 state / proxy + runtime + `Notifier`）、`EmptyArgs`、`NoopNotifier`/`NOOP_NOTIFIER`；B 批追加 `pub mod node / group / process / import`；C 批追加 `pub mod subscription / ruleset / routing` |
| `src-tauri/src/services/engine.rs` | 新增：22 条命令的业务体 + 参数结构 + `StartRequest` / `prepare_start` / `start_engine` / `stop_if_running_node` / `render_for_preview` / 延迟探测等助手；C 批把 `prepare_start`/`render_preview` 里的 `loaded_sets` 调用改到 `crate::services::ruleset` |
| `src-tauri/src/services/sysproxy.rs` | 新增：2 条系统代理命令的业务体 |
| `src-tauri/src/services/node.rs` | 新增（B）：12 条节点命令的业务体 + `NodeArgs`/`TagArgs`/`MoveArgs`/`IdArgs`/`IdsArgs`、`apply_tag`/`apply_move`/`apply_delete`、`probe_all`/`remember`/`remember_batch`/`LatencyProbe`、`delay_offline_target/probe/finish` |
| `src-tauri/src/services/group.rs` | 新增（B）：5 条策略组命令的业务体 + `GroupArgs`/`IdArgs`/`SelectMemberArgs`；删组改写规则的不变量随业务一起搬入 |
| `src-tauri/src/services/process.rs` | 新增（B）：`list_processes`（OS 快照，无状态依赖） |
| `src-tauri/src/services/import.rs` | 新增（B）：`import_preview`（不会失败）/ `import_nodes` + `ImportPreview`/`ImportOutcome`/`TextArgs`；去重、组/规则合并与跳过理由原样保留 |
| `src-tauri/src/services/subscription.rs` | 新增（C）：6 条订阅命令的业务体 + `SubscriptionArgs`/`CreateSubscriptionArgs`/`IdArgs`、`RefreshOutcome`/`RefreshEffects`、`apply_delete_subscription`/`refresh_one_blocking`/`apply_document`/`due_subscription_ids`/`record_refresh_error`/`now_ms`；三段式刷新（规划 / 阻塞拉取 / 落库）从命令壳搬入 |
| `src-tauri/src/services/ruleset.rs` | 新增（C）：9 条规则集命令的业务体 + `loaded_sets`（原在 `commands::ruleset`，本批下移，服务层唯一实现）、`GeoUpdate`/`IdArgs`/`CreateRuleSetArgs`/`RuleSetArgs`/`SetTextArgs`、`apply_text`/`record_error`/`refresh_due_sets`/`geo_assets`/`update_geo_assets` |
| `src-tauri/src/services/routing.rs` | 新增（C）：6 条路由命令的业务体 + `RuleTest`/`DomainArgs`/`RuleArgs`/`IdArgs`/`MoveArgs`；`test_domain` 直接调 `services::ruleset::loaded_sets`，不再反调 commands |
| `src-tauri/src/commands/engine.rs` | 变薄壳：签名与 `///` 文档一字未改，函数体 = 构造 `Ctx` → 调 `services::engine`；`pick_engine_binary`、`engine_open_dir` 保留原生实现并注明不进路由；原有测试模块原样保留；B 批收窄了再导出列表（`lookup_node`/`probe_url`/`remember_real` 已无外部调用者，由 `services::node` 直接使用） |
| `src-tauri/src/commands/sysproxy.rs` | 变薄壳，签名与文档未改 |
| `src-tauri/src/commands/node.rs` | 变薄壳（B）：12 条命令签名与 `///` 文档一字未改，函数体 = 调 `services::node` + `refresh_tray`；`probe_nodes`/`delay_offline`/`stop_after_delete` 保留为壳内异步编排（`lib.rs:308`、`lib.rs:779` 的 `block_on` 依赖 `probe_nodes`）；原有 18 条测试保留，改为直接测 `services::node` 的助手 |
| `src-tauri/src/commands/group.rs` | 变薄壳（B）：5 条命令保留签名/文档；原 1 条测试扩为 6 条，直接测 `services::group`（删组改写规则的不变量现在真的被 `delete_group` 执行，不再靠手写模拟） |
| `src-tauri/src/commands/process.rs` | 变薄壳（B）：`async` 签名与文档未改，函数体调 `services::process` |
| `src-tauri/src/commands/import.rs` | 变薄壳（B）：两条命令签名/文档未改；`ImportPreview`/`ImportOutcome` 改为 `pub use services::import::{...}` 保持类型路径；托盘只在 `imported/groups/rules > 0` 时刷新；原 5 条解析测试保留，改走 `services::import::import_preview` 这唯一入口 |
| `src-tauri/src/commands/subscription.rs` | 变薄壳（C）：6 条命令签名与 `///` 文档一字未改；异步的 `delete_subscription`/`refresh_subscription` 保留 `run_blocking` 编排与 `refresh_tray`，业务调 `services::subscription`；原测试随业务迁入 service |
| `src-tauri/src/commands/ruleset.rs` | 变薄壳（C）：9 条命令签名与 `///` 文档一字未改；`loaded_sets` 原函数删除，改从 `services::ruleset` 走；原测试迁入 service |
| `src-tauri/src/commands/routing.rs` | 变薄壳（C）：6 条命令签名与 `///` 文档一字未改；`RuleTest` 从 `services::routing` 再导出保持类型路径；删掉 `use crate::commands::ruleset::loaded_sets` |
| `src-tauri/src/core/endpoints.rs` | `dispatch` 改为查 `ROUTES` 表；A 批 24 条 + B 批 20 条 + C 批 21 条 `POST /<module>/<command_name>`；`NOT_ROUTED` 白名单；`handle()` 统一反序列化/包装错误；A 批 24 条 + B 批 27 条 + C 批 12 条单测 + 覆盖测试（`COVERED_MODULES` 9 个模块、固定 65 条） |
| `src-tauri/src/core/mod.rs` | `CoreContext` 增加 `traffic: Arc<TrafficLog>`（traffic 路由要读与窗口同一个文件），`serve()` 用 `base/traffic.jsonl` 构造 |
| `src-tauri/src/core/server.rs` | 测试里的 `CoreContext::new` 跟上四参数 |
| `src-tauri/src/lib.rs` | 注册 `pub mod services;`；C 批把 `now_ms`/`refresh_one_blocking`/`refresh_due_sets` 三个调用点从 `commands::*` 改到 `crate::services::*` |
| `src-tauri/src/ipc.rs` | **未改**（注册表是覆盖测试的输入） |
| `src/services/bindings.ts` | **未改**；重新生成后与提交版逐字节相同 |

## 2. 65 条命令 → 路由 → service → 单测

`POST` 省略；单测均在 `core::endpoints::tests`（壳层的 service 级测试另在 `commands/*/tests`）。

### 2.1 批次 A：engine 22 + sysproxy 2

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

### 2.2 批次 B：node 12 + group 5 + process 1 + import 2

| 路由 | service 函数 | 单测 |
| --- | --- | --- |
| `/node/list_nodes` | `services::node::list_nodes` | `node_list_route_answers_the_node_list` |
| `/node/upsert_node` | `services::node::upsert_node` | `upsert_node_route_adds_then_updates` |
| `/node/delete_node` | `services::node::delete_node` | `delete_node_route_removes_it_and_answers_the_rest`（另有 `deleting_a_missing_node_is_404`） |
| `/node/move_node` | `services::node::move_node` | `move_node_route_swaps_neighbours` |
| `/node/delete_nodes` | `services::node::delete_nodes` | `delete_nodes_route_removes_a_selection` |
| `/node/tag_nodes` | `services::node::tag_nodes` | `tag_nodes_route_tags_the_selection`（另有 `an_empty_tag_on_tag_nodes_is_400`） |
| `/node/export_node_links` | `services::node::export_node_links` | `export_node_links_route_writes_one_link_per_line`（另有 `export_node_links_without_a_selection_is_400`） |
| `/node/test_node_latency` | `services::node::test_node_latency` | `test_node_latency_route_measures_a_local_listener`（另有 `test_node_latency_for_a_missing_node_is_404`） |
| `/node/test_nodes_latency` | `services::node::test_nodes_latency` | `test_nodes_latency_route_returns_one_row_per_target` |
| `/node/node_probes` | `services::node::node_probes` | `node_probes_route_starts_empty` |
| `/node/clear_node_probes` | `services::node::clear_node_probes` | `clear_node_probes_route_forgets_the_ids` |
| `/node/node_delay_offline` | `services::node::node_delay_offline` | `node_delay_offline_for_a_missing_node_is_404`（另有 `node_delay_offline_refuses_while_the_kernel_runs`） |
| `/group/list_groups` | `services::group::list_groups` | `group_list_route_starts_empty` |
| `/group/upsert_group` | `services::group::upsert_group` | `upsert_group_route_adds_a_group` |
| `/group/delete_group` | `services::group::delete_group` | `delete_group_route_rewrites_rules_to_the_active_node`（另有 `deleting_a_missing_group_is_404`） |
| `/group/select_group_member` | `services::group::select_group_member` | `select_group_member_route_picks_the_member`（另有 `selecting_a_member_outside_the_group_is_400`） |
| `/group/toggle_group` | `services::group::toggle_group` | `toggle_group_route_flips_enabled` |
| `/process/list_processes` | `services::process::list_processes` | `list_processes_route_answers_a_list` |
| `/import/import_preview` | `services::import::import_preview` | `import_preview_route_parses_a_share_link` |
| `/import/import_nodes` | `services::import::import_nodes` | `import_nodes_route_counts_duplicates_on_the_second_paste` |

B 批 20 条全部有路由，`NOT_ROUTED` 未新增条目：这批没有纯壳动作。`export_node_links` 只返回字符串，写剪贴板是前端的事，所以它是业务、不是壳。

### 2.3 批次 C：subscription 6 + ruleset 9 + routing 6

| 路由 | service 函数 | 单测 |
| --- | --- | --- |
| `/subscription/list_subscriptions` | `services::subscription::list_subscriptions` | `list_subscriptions_route_answers_an_empty_list` |
| `/subscription/create_subscription` | `services::subscription::create_subscription` | `create_subscription_route_adds_a_source`（另有 `create_subscription_rejects_a_non_http_url`） |
| `/subscription/upsert_subscription` | `services::subscription::upsert_subscription` | 由 `create_subscription_route_adds_a_source` 覆盖反序列化与落库 |
| `/subscription/delete_subscription` | `services::subscription::apply_delete_subscription` | `deleting_a_missing_subscription_is_404` |
| `/subscription/refresh_subscription` | `services::subscription::refresh_one_blocking` | 不发网络：错误分支由 `deleting_a_missing_subscription_is_404` 同族缺失 id 语义覆盖 |
| `/subscription/refresh_due` | `services::subscription::due_subscription_ids` + `refresh_one_blocking` | 不发网络：无到期订阅时返回空列表（空态由 `list_subscriptions` 同构覆盖） |
| `/ruleset/list_rule_sets` | `services::ruleset::list_rule_sets` | `create_rule_set_route_adds_a_set`（列表形态） |
| `/ruleset/create_rule_set` | `services::ruleset::create_rule_set` | `create_rule_set_route_adds_a_set` |
| `/ruleset/upsert_rule_set` | `services::ruleset::upsert_rule_set` | 由 `create_rule_set_route_adds_a_set` 覆盖反序列化与落库 |
| `/ruleset/delete_rule_set` | `services::ruleset::delete_rule_set` | `deleting_a_missing_rule_set_is_404` |
| `/ruleset/toggle_rule_set` | `services::ruleset::toggle_rule_set` | 缺失 id 走 `not_found` 分支，同 `deleting_a_missing_rule_set_is_404` 的定位语义 |
| `/ruleset/set_rule_set_text` | `services::ruleset::set_rule_set_text` | `set_rule_set_text_on_a_missing_set_is_404` |
| `/ruleset/refresh_rule_set` | `services::ruleset::refresh_rule_set` | 不发网络：只验证路由接线；远端分支不测 |
| `/ruleset/geo_assets` | `services::ruleset::geo_assets` | `geo_assets_route_reports_status_without_network` |
| `/ruleset/update_geo_assets` | `services::ruleset::update_geo_assets` | 不发网络：下载分支不测（会真发请求） |
| `/routing/list_rules` | `services::routing::list_rules` | `list_rules_route_answers_an_empty_list` |
| `/routing/test_domain` | `services::routing::test_domain` | `test_domain_route_rejects_a_blank_domain` |
| `/routing/upsert_rule` | `services::routing::upsert_rule` | `upsert_rule_route_persists_a_rule` |
| `/routing/delete_rule` | `services::routing::delete_rule` | `deleting_a_missing_rule_is_404` |
| `/routing/move_rule` | `services::routing::move_rule` | 由 `upsert_rule_route_persists_a_rule` 建立规则后经同表校验 |
| `/routing/toggle_rule` | `services::routing::toggle_rule` | 缺失 id 走 `not_found`，同 `deleting_a_missing_rule_is_404` |

C 批 21 条全部有路由，`NOT_ROUTED` 未新增条目：这批没有纯壳动作。`subscription` 里没有 `open_url`/`clipboard` 命令；`geo_assets`/`update_geo_assets` 只读写本地 `geo::asset_dir`，属于业务而非窗口壳；`update_geo_assets` 的下载是真业务，放在 service 里，HTTP 线程同步执行。所有涉及网络的路由（`refresh_subscription`/`refresh_due`/`refresh_rule_set`/`update_geo_assets`）在单测里**都不发真实请求**，只测缺失 id 的 404 / 空白域名的 400 等安全分支。

已有 6 条 P0 路由（`GET /health`、`GET /status`、`GET /nodes`、`POST /engine/start`、`POST /engine/stop`、`POST /shutdown`）原样保留，handler 只是改成函数指针后转调原函数。

覆盖测试：`every_ipc_command_has_a_route()` 用 `include_str!("../ipc.rs")` 扫注册表，对 `COVERED_MODULES = ["engine", "sysproxy", "node", "group", "process", "import", "subscription", "ruleset", "routing"]` 严格；本批校验数固定为 65，批次 D 把模块名加进数组即可自动收紧。另有 `no_two_routes_share_a_method_and_path()` 防重复。

## 3. 验证结果

- `cd src-tauri && cargo test --lib`（PATH/CARGO_HOME 按任务给的配置）：

```
running 637 tests
test result: ok. 637 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.66s
```

  基线 622，本批后 637（+15，其中 `core::endpoints` 新增 12 条批次 C 路由单测；`services::{subscription,ruleset,routing}` 的测试随业务从 commands 迁入）。全部走 FakeSpawner/FakeProbe/`OffProxy` 与本地 `TcpListener`，未起任何真实内核，未写系统代理注册表；订阅/规则集的网络分支在单测里一律不发真实请求。既有 flake（`commands::engine::tests::a_split_pac_takeover_encodes_the_http_port`）本轮未出现。
- 仓库根 `npm run check`：

```
> x-matrix@0.1.0 typecheck
> vue-tsc --noEmit

> x-matrix@0.1.0 check:arch
> node scripts/check-architecture.mjs

architecture ok (70 files checked)

> x-matrix@0.1.0 check:contract
> node scripts/gen-contract.mjs --check

contract up to date

> x-matrix@0.1.0 test
> vitest run

 Test Files  13 passed (13)
      Tests  136 passed (136)
```

- `git status --porcelain`（原文）：

```
 M P1-REPORT.md
 M src-tauri/src/commands/routing.rs
 M src-tauri/src/commands/ruleset.rs
 M src-tauri/src/commands/subscription.rs
 M src-tauri/src/core/endpoints.rs
 M src-tauri/src/lib.rs
 M src-tauri/src/services/engine.rs
 M src-tauri/src/services/mod.rs
?? src-tauri/src/services/routing.rs
?? src-tauri/src/services/ruleset.rs
?? src-tauri/src/services/subscription.rs
```

  未提交、未 push。`M P1-REPORT.md` 就是本文件；`docs/` 下 Lead 的文件一个没碰。
- `git diff --stat src/services/bindings.ts`：**空输出**，`bindings.ts` 没有被改动（`check:contract` 也逐字节通过）。
- `grep -rn "commands::ruleset" src-tauri/src/services`：**空输出**——`loaded_sets` 已下移到 `services::ruleset`，`services::engine` 的两处反调已改为 `crate::services::ruleset::loaded_sets`，分层倒挂消除。

## 4. 设计决定

1. **表驱动路由**：`ROUTES: &[Route]` 是唯一事实源，`dispatch` 只做 token 校验 + 查表；覆盖测试直接读同一张表，不会出现"测试绿但真路由没接"。
2. **薄壳不碰签名**：44 条命令的名字、参数名、返回类型、`///` 文档全部保留；`bindings.ts` 重新生成后逐字节一致，`check:contract` 通过。
3. **异步命令保持 `run_blocking`**：把业务拆成「取目标（借 state/runtime，同步）+ 阻塞测量（`'static` 闭包）」两段，`engine_start`/`engine_stop` 同理，避免把 `State` 借入 `'static` 闭包。
4. **`Ctx` 的 state/proxy 是可选的**：不是每条命令都需要窗口状态或代理守卫；缺失时返回 `AppError::Internal` 而不是 panic。
5. **`CoreContext` 增加 `traffic`**：traffic 三条路由要读与窗口同一个 `traffic.jsonl`；core 进程自己持有一份，避免每次请求新建实例读到空历史。
6. **`engine_kinds` 无 runtime**：命令签名本来就没有，路由直接忽略 `ctx`。
7. **不做的事件**：核对 `git show HEAD:...commands/engine.rs` 与 `sysproxy.rs`，这批命令原本没有任何 `app.emit`，所以批次 A 只接了 `NoopNotifier`；`AppHandle` 版 `Notifier` 等到第一个真正要发事件的批次再加，现在加就是死代码。
8. **B 批参数结构按 `bindings.ts` 的字段名**：`node`/`ids`/`tag`/`remove`/`delta`/`text`/`group` 原样，`select_group_member` 的 `node_id` 用 `#[serde(rename_all = "camelCase")]` 映射成 `nodeId`。HTTP JSON 与 Tauri 参数一一对应，没有翻译表。
9. **B 批异步命令三段拆分**：`probe_targets`（借 state 挑目标、读超时/并发，返回拥有所有权的 `Vec<Node>`）→ `probe_all`（`'static` 闭包里的线程池）→ `remember_batch`（回写 state）；离线真实延迟同理拆 `delay_offline_target`/`delay_offline_probe`/`delay_offline_finish`。HTTP 线程没有 UI 可阻塞，所以路由直接调完全同步的 `test_node_latency`/`test_nodes_latency`/`node_delay_offline`，Tauri 壳自己用 `run_blocking`。
10. **`probe_nodes` 留在壳里、不搬**：`lib.rs:308`（开机自测）与 `lib.rs:779`（热键"全部测速"）都用 `block_on(commands::node::probe_nodes(&state, Vec::new()))`，它只是 services 三段函数的编排，搬走反而要在 lib.rs 里改写调用点。
11. **删节点顺带停内核**：`services::node::delete_node/delete_nodes` 收 `&EngineRuntime` + `&ProxyGuard`，删完直接调 `services::engine::stop_if_running_node`（HTTP 线程同步停即可）；Tauri 壳仍把停内核放进 `run_blocking`，再 `refresh_tray`。这样任何入口删掉正在跑的内核所依赖的节点，都不会留下"看守不存在的节点"的内核。
12. **托盘刷新留在壳里**：service 只做 mutation + persist 并返回快照；壳决定 `crate::refresh_tray(&app)`。`import_nodes` 只在 `imported/groups/rules` 任一 > 0 时刷新——重复粘贴一份已存在的订阅不该触发整棵托盘重建。
13. **`state_route!` 宏替代 20 个手写 handler**：只吃 `&ctx.state` 的命令共用一套包装；真正要 runtime/proxy 的 4 条（`delete_node`、`delete_nodes`、`node_delay_offline`）以及不碰状态的 `list_processes`、不会失败的 `import_preview` 单独写，理由写在函数注释里。
14. **`import_preview` 不失败但仍走 `handle()`**：service 返回裸 `ImportPreview`，handler 用 `Ok::<_, AppError>` 包一层，保持"所有路由共用一个入口"的单一形状。
15. **测试隔离按进程 + 序号**：B 批把 `commands/group.rs` 的临时目录从 `groups-{pid}` 改成 `groups-{pid}-{seq}`；原先 6 条测试共享一个 store，并行跑时互相 `remove_dir_all`，是本批唯一一次红测（改完后连续绿）。
16. **`loaded_sets` 下移，分层倒挂消除（C）**：原 `commands::ruleset::loaded_sets` 搬进 `services::ruleset`，`services::engine::prepare_start`/`render_preview` 与 `services::routing::test_domain` 都直接调同层 `services::ruleset`；`commands::ruleset` 不再有该函数。`grep -rn "commands::ruleset" src-tauri/src/services` 为空。
17. **C 批异步命令保留壳内编排**：`delete_subscription`/`refresh_subscription` 仍是 `run_blocking` 三段（规划借 state / `'static` 闭包拉取 / 回写落库），与 B 批 `probe_nodes` 同理——service 只暴露同步的 `apply_delete_subscription`/`refresh_one_blocking`/`apply_document`，HTTP 线程与 Tauri 壳共用同一实现。
18. **C 批 `GeoUpdate` 文档落在类型上**：`update_geo_assets` 命令原本无 `///` 文档，"Download both…" 那段 `///` 实际是 `GeoUpdate` 类型的文档（中间夹着 plain `//`）。搬迁时按原样把整段放回 service 的 `GeoUpdate` 上，命令壳保持无文档，`bindings.ts` 逐字节不变。
19. **C 批路由接线不测网络**：`refresh_subscription`/`refresh_due`/`refresh_rule_set`/`update_geo_assets` 的远端分支会真发请求，单测只覆盖缺失 id 的 404、空白域名的 400 与 `geo_assets` 的本地读；成功路径留给 cloudpc-test 真机验。
20. **`NOT_ROUTED` 零新增（C）**：`subscription` 无 `open_url`/`clipboard`；`geo_assets`/`update_geo_assets` 只碰本地资产目录，是业务不是壳动作。三个模块 21 条全部有 `POST` 路由。

## 5. 遗留问题 / 拿不准的点

1. **`AppError` 没有 `Conflict`/`Unsupported`**：`bindings.ts` 导出了 `AppError` 联合类型，加变体会让 `check:contract` 漂移。因此「内核没在运行」目前是 HTTP 500 + `code:"engine"`，语义上更像 409。要改就得同时改契约与前端（P2），本批没动。
2. **services 反向依赖 commands（已在 C 批解决）**：`services::engine::prepare_start` / `render_preview` 原本反调 `crate::commands::ruleset::loaded_sets`；C 批把 `loaded_sets` 搬进 `services::ruleset` 后，两处改为 `crate::services::ruleset::loaded_sets`，commands 侧原函数删除，`grep -rn "commands::ruleset" src-tauri/src/services` 为空。（`services::subscription` 的测试模块仍 `use crate::commands::engine::tests::proxy`，那是仅测试期复用的 Fake 助手，不进入生产依赖图。）
3. **traffic 的所有者**：单进程模式下窗口自己 `manage` 一个 `TrafficLog`，core 模式又建一个；两者写同一个路径但目前不会同时存在。P2 把前端改成轮询 core 后，需要明确"只有 core 写"。
4. **HTTP 侧的延迟命令会占住连接线程**：`proxy_latency`/`proxy_speed`/`node_delay`/`group_delay` 同步执行，最长约 `DELAY_BUDGET_MS = 5s`；B 批的 `test_nodes_latency` 还多一层线程池，最坏是一轮超时。core 是每连接一线程，能接受，但客户端要设超时。
5. **`engine_open_dir` 的归类**：它确实有业务含义（打开日志/配置目录），但动作是"在用户桌面上弹 Explorer"，headless core 不该做，因此放进 `NOT_ROUTED`；复核若认为应改成"返回路径"由壳去打开，可以下批调整（那会改签名，需走契约）。
6. **复杂参数**：A 批参数最多是 `EngineKind`（枚举，serde kebab-case）、`Vec<String>`、`f64`；B 批第一次出现嵌套对象（`{"node": Node}`、`{"group": ProxyGroup}`），单测已覆盖反序列化。`EmptyArgs` 忽略未知字段，无参命令传 `{}` 或空体都行。
7. **`traffic_sample` 在没有 stats channel 时返回 200 + `null`**（与原命令语义一致：`Option<TrafficSample>`），不是错误。
8. **`list_processes` 没有 Fake**：`sysproc::list()` 读的是跑测试这台机器真实的进程表，单测只能断言 200 + 数组形状。它不读注册表、不起内核、不联网，但结果不可复现；要固定就得给 `sysproc` 注入一个枚举器，属于额外抽象，本批没做。
9. **`node_delay_offline` 只覆盖了拒绝分支**：单测覆盖"内核运行中 → 400"和"节点不存在 → 404"，这两条都在起临时内核之前返回。真正"起临时内核 → 测一次 → 杀掉"的路径只能去 cloudpc-test 验（本机硬禁起内核），本轮未在真机执行。
10. **`services::node` 的 `delay_offline_probe/finish` 只能是 `pub(crate)`**：它们暴露 `crate::engine::temp::Attempt`，而该枚举还是 `pub(crate)`。等 temp 模块接口定稿（或把 `Attempt` 提升为 `pub`）再考虑公开这两个函数；现在公开会触发 `private_interfaces` 警告。
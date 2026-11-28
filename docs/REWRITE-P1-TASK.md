# P1 任务书：把 98 条 IPC 全部搬到 core 的 HTTP 通道

> 上级文档：`docs/REWRITE-PLAN.md`（为什么换 Avalonia）、`docs/REWRITE-P0-TASK.md`（P0 已交付）。
> 这一份只管 P1：**core 长出全部路由，Tauri 版继续能用，两边共用同一份业务体。**

## 0. 一句话目标

`x-matrix-core.exe --serve` 暴露的 HTTP 路由要覆盖 `src-tauri/src/ipc.rs` 里注册的**全部 98 条命令**，
且每条路由的结果与 Tauri 命令**逐字段相同**；Tauri 命令本身改成薄壳，业务体只剩一份。

## 1. 硬约束

1. **本机绝不起内核、绝不写系统代理注册表**。开发机只跑 `cargo test` / `npm run check`；凡是要真起内核、
   真接管代理的验收，由 Lead 在 `cloudpc-test` 上做。
2. **不许改 `src/services/bindings.ts` 的命令名与参数名**。前端（Tauri 版）还在用，改了就断。
   `npm run check` 里的 `check:contract` 必须一直绿。
3. **不许给前端加新依赖**、不许引入 axum/hyper/tokio；HTTP 服务仍用手写的 `core/http.rs`。
4. **不碰** `docs/ACCEPTANCE.md`、`docs/HANDOVER.md`、`docs/HANDOFF-BRIEF.md`、`docs/PARITY-PLAN.md`
   （Lead 负责）；可以写 `docs/REWRITE-P1-TASK.md` 之外的 `P1-REPORT.md` 汇报。
5. 一次只做一个批次（见 §4），做完让 Lead 复核再进下一批。

## 2. 路由约定（机械可查）

- **路径** = `/<module>/<command_name>`，其中 `<module>` 就是 `commands/<module>.rs` 的文件名，
  `<command_name>` 就是 `ipc.rs` 里那个函数名，**原名照抄，不做复数化、不转连字符**。
  例：`commands::node::upsert_node` → `POST /node/upsert_node`。
- **方法**一律 `POST`（含读操作），请求体是命令参数的 JSON 对象；没有参数就发 `{}`。
  已存在的 6 条 `GET /health`、`GET /status`、`GET /nodes`、`POST /engine/start`、`POST /engine/stop`、
  `POST /shutdown` **保持原样**，不要动（桌面壳已经在用）。
- **鉴权**：所有路由都要 `X-Core-Token`（`dispatch()` 现在就在最前面做了，保持不变）。
- **成功**：`200` + 命令返回值的 JSON（`null` 也要回 `200` + `null`，不要回 204）。
- **失败**：`{"error":{"code":"…","message":"…"}}`，状态码映射沿用 `core/endpoints.rs::from_error`：
  `InvalidInput`→400 `invalid_input`、`NotFound`→404 `not_found`、`Io`/`Engine`/`Internal`→500，
  新增 `Conflict`→409 `conflict`、`Unsupported`→501 `unsupported`（`AppError` 里没有就加）。
- **参数反序列化失败**：400 `invalid_input`，message 里必须带命令名，便于定位。

## 3. 代码形状

新增 `src-tauri/src/services/`（**业务体唯一的家**）：

```rust
pub struct Ctx<'a> {
    pub state: &'a AppState,
    pub runtime: &'a EngineRuntime,
    pub proxy: &'a ProxyGuard,
    pub notify: &'a dyn Notifier,
}

pub trait Notifier: Send + Sync {
    fn emit(&self, event: &str, payload: serde_json::Value);
}
```

- 每个模块一个文件（`services/engine.rs`、`services/node.rs` …），函数名与命令同名，
  签名 `pub fn xxx(ctx: &Ctx, args: XxxArgs) -> AppResult<T>`；`XxxArgs` 是 `Deserialize` 的结构体，
  字段名与前端发的 JSON 一致（也就是命令参数名）。
- `commands/<module>.rs` 的命令体改成三行：构造 `Ctx`（Tauri 侧的 `Notifier` 实现用 `AppHandle::emit`）
  → 调 `services::<module>::xxx` → 返回。
- **事件不进 P1**：桌面上凡是原来靠事件推送的（进度、日志、状态），P1 一律改成「UI 自己轮询」，
  `Notifier` 在 core 侧用 no-op 实现。事件通道是 P2 的事。
- `core/endpoints.rs` 只做「解析 → 调 service → 序列化」，不放业务判断。

## 4. 批次与验收

| 批次 | 模块（命令数） | 小计 |
|---|---|---|
| **A（本批）** | `engine`(22) + `sysproxy`(2) | 24 |
| B | `node`(12) + `group`(5) + `process`(1) + `import`(2) | 20 |
| C | `subscription`(6) + `ruleset`(9) + `routing`(6) | 21 |
| D | `settings`(4) + `profile`(6) + `backup`(2) + `webdav`(3) + `kernel`(2) + `service`(3) + `elevate`(2) + `system`(11) | 33 |

每批必须交：

1. 该批全部路由 + 每条至少一个单测（成功路径；入参非法路径各一条更好）；
2. **路由覆盖测试**：`core/endpoints.rs` 里有一个 `#[test] fn every_ipc_command_has_a_route()`，
   用 `include_str!("../ipc.rs")` 抓 `commands::` 出现的函数名，与路由表比对，缺一条就失败；
   确实不该暴露的写进 `NOT_ROUTED: &[(&str, &str)]`（命令名, 理由），保持为空或极小；
3. `cargo test --lib` 全绿（P0 结束时是 564，只许涨不许跌）；
4. `npm run check` 全绿（`check:contract` 不许改）；
5. `P1-REPORT.md`：本批路由清单、每条一行「哪个 service 函数 / 单测名」、遗留问题。

## 5. 不在 P1 范围

- 事件推送、日志流、托盘、单实例、提权、安装包（P2/P3）。
- 桌面壳的 11 个视图（P2 一个视图一个视图迁）。
- 删除 Tauri 版（P1 期间两条路并存，Tauri 版必须仍可构建、可安装）。

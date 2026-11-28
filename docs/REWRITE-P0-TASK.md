# P0 任务书：Rust core 的 HTTP 通道 + Avalonia 骨架

> 依据：`docs/REWRITE-PLAN.md`（决策与阶段）。P0 只证明「原生 UI 能通过本地通道驱动 Rust 内核」，
> 不做完整体验，不迁移任何视图的细节。
> 派发对象：一个子代理，一次做完；主窗口只验收。

## 1. 目标（三个，缺一不算完成）

1. **Rust 侧新增 headless 服务模式**：`x-matrix.exe --serve [--port <n>]` 启动后不创建任何 Tauri
   窗口，直接在回环地址上提供 HTTP/JSON 路由；把 `{port, token, pid}` 写进
   `%APPDATA%\com.xmatrix.client\core.endpoint`（JSON，覆盖写，权限仅当前用户）。
2. **五条路由**（足够画出节点页并启停内核）：
   - `GET  /health` → `{"ok":true,"version":"0.1.0"}`
   - `GET  /status` → 现有 `EngineStatus` 的形状（`running/kind/pid/...`）+ 系统代理是否被接管
   - `GET  /nodes`  → 节点数组（`id/name/protocol/host/port`，不含订阅正文）
   - `POST /engine/start` `{"kind":"xray"|"sing-box","nodeId":"..."}` → `{"ok":true}` 或错误
   - `POST /engine/stop` → `{"ok":true}`
   所有请求必须带 `X-Core-Token`，不匹配返回 401；错误统一 `{"error":{"code":"...","message":"中文"}}`。
3. **Avalonia 骨架**（`desktop/`）：窗口左侧导航（首页/节点/订阅/设置 四项即可），节点页从
   `GET /nodes` 读真数据渲染列表，选中一行后可以「启动 / 停止」，顶部显示 `/status` 的运行状态。
   颜色、圆角、间距的取值必须来自 `src/styles/tokens.css` 的现有变量（不允许自创色板）。

## 2. 可改 / 不可改

- **可改**：新增 `src-tauri/src/core/`（`mod.rs`、`server.rs`、`http.rs`、`endpoints.rs`）；
  `src-tauri/src/lib.rs` 只加 `pub mod core;`；`src-tauri/src/main.rs` 加 `--serve` 分支；
  新增整个 `desktop/` 目录；`docs/REWRITE-PLAN.md` 的进度表**不要动**（主窗口更新）。
- **不可改**：`engine/`、`sysproxy/`、`net/`、`service.rs` 的任何行为与签名（只能调用）；
  `src/` 下所有 Vue/TS；`docs/ACCEPTANCE.md`；`docs/ISSUES.md`；i18n 三份文件。
- **不许**在本机启动内核、不许写本机系统代理注册表（这是硬规则）；启动/停止只在云电脑上验。

## 3. 验收命令与判据

| # | 命令 | 期望 |
|---|---|---|
| 1 | `npm run check` | 全绿（vitest 136） |
| 2 | `cd src-tauri && cargo test --lib` | 539 passed，且 `core` 新增单测至少覆盖「token 401」「未知路由 404」「错误体形状」 |
| 3 | `cargo build --release` | 成功；`target/release/x-matrix.exe --serve --port 47100` 起来不弹任何窗口（桌面上不该出现新窗口） |
| 4 | `curl -s -H "X-Core-Token: <token>" http://127.0.0.1:47100/health` | `{"ok":true,...}`；不带 token → 401 |
| 5 | `curl -s .../nodes` | 与 `store.json` 里节点数一致（用种子 store 验：2 条） |
| 6 | `dotnet build desktop/XMatrix.Desktop` | 0 error |
| 7 | 运行 Avalonia 骨架（本机，只读页面） | 截图里节点页出现那 2 条节点的名字；不点「启动」（本机禁止起内核） |
| 8 | 云电脑：`+` 同上第 4–5 条 + 点「启动」 | 内核 PID 出现、`/status` 变 running、`ProxyEnable=1`；点「停止」后还原为 0 |

第 8 条如果云电脑 .NET 构建不便，允许用 self-contained 发布（`dotnet publish -r win-x64 --self-contained`）
单文件拷过去跑。

## 4. 交付物

- 代码：上述新增/改动文件。
- `desktop/README.md`：怎么 build、怎么跑、怎么和 core 一起起。
- 一份 `P0-REPORT.md`（放仓库根或 `desktop/`）：第 3 节 8 条判据的实测输出（命令 + 关键行 + 截图路径）。
- 不要提交 git（主窗口统一提交）。

## 5. 约束与坑（前人踩过，照做）

- 本机 `dotnet` 只有运行时没有 SDK；SDK 由主窗口装在 `D:\CODE.config\dotnet`（用户态），
  子代理直接用 `DOTNET_ROOT=D:\CODE.config\dotnet` 与全路径 `dotnet.exe`。
- HTTP 服务端不许引入 axum/hyper；照 `src-tauri/src/net/http.rs` 的风格手写（`TcpListener` +
  单行请求行 + `Content-Length` 读完 + `Connection: close`）。
- 端口占用：`--port 0` 时由系统分配，读回实际端口再写 endpoint 文件。
- 中文错误文案沿用 `AppError` 的现有措辞，不要另起一套。
- Windows 下写文件用「tmp + rename」，避免半截 JSON 被 UI 读到。
- 关闭 core：`POST /shutdown` 也要有（只允许 token 匹配），用于验收脚本收尾。
# 新窗口开工简报（X-Matrix Client Pro）

> 写于 2026-11-25 08:35（§52 收口这一版）。给**下一个窗口**的你自己看：先读这一页，够用就别翻长文档；需要细节时按文末索引跳。
> 长文档：`docs/HANDOVER.md`（地图、候选表、云电脑手册）、`docs/ACCEPTANCE.md`（每条功能的判据与踩坑）、`docs/PARITY-PLAN.md`（S1–S11 的解禁计划与进度表——**下一个任务从这里取**）。

## 1. 三十秒现状

| 项 | 值 |
|---|---|
| 仓库 / 工作区 | `D:\GitHub-Warehouse\X-Matrix-Client-Pro`（Tauri 2 + Vue 3 + TS） |
| HEAD | 本简报这条 docs 提交；功能提交查 `git log --oneline -- src-tauri/src/engine/groupwatch.rs` 的第一条（含 §52），再往前 §51（`src-tauri/src/sysproc.rs`）、§50（`src/services/accent.ts`）、§49（S8）、`a794438`（§48）、`6fd790f`（§47） |
| 工作区 | **干净**，没有未提交改动 |
| 规模 | 82 个 IPC 命令 / Rust 单测 **421** / vitest **116**（12 文件）/ i18n **789 键 × 3 语言** |
| 验收 | `docs/ACCEPTANCE.md` 写到 **§52**，全部是云电脑真机点出来的 |
| 云电脑 | 干净：`ProxyEnable=0x0`、无 `xm*` 任务、无内核进程 |
| 构建产物 | `src-tauri/target/release/x-matrix.exe`（10,522,624 B，md5 `b2397c8b92234a54aa180485ec321bc2`，含 §21–§52） |
| 目标 | 功能追平 V2RayN / Clash、二者互补，并把各功能之间**说得清、配合得上**（用户原话，长期有效） |

## 2. 常用的四条命令

```bash
cd /d/GitHub-Warehouse/X-Matrix-Client-Pro

# 门槛（必须全绿再谈提交）
npm run check                                        # typecheck + 架构 + 契约 + vitest
cd src-tauri && cargo test --lib && cd ..            # Rust 单测

# 构建（4–8 分钟；PATH/CARGO_HOME 必须这样写）
export PATH=/d/CODE.config/Cargo.config/bin:$PATH
export CARGO_HOME='D:\CODE.config\Cargo.config'
npx tauri build --no-bundle                          # 产物 src-tauri/target/release/x-matrix.exe

# 推送（本机解析不了 github.com，必须带代理）
https_proxy=http://127.0.0.1:10808 git push origin main
```

改 Rust 的 `#[tauri::command]` 签名后必须 `npm run gen:contract`；新增 i18n 键必须走 `scripts/i18n-pairs.json` + `python scripts/gen_i18n.py`（三份字典会一起生成）。

## 3. 下一个任务：`docs/PARITY-PLAN.md` 进度表的下一行（现在是 **S3 内核下载 / 更新**）

用户的指示（2026-11-25）是「S1–S11 全部解禁，一个一个写明确的任务书派子代理去做」，所以**别再自作主张跳过**。每项固定流程：

1. 写任务书到 `D:\tmp\handover\tasks\S<n>-*.md`（判据、钉死的设计、可改文件范围、验证命令、禁区）；
2. 派**一个**子代理（同时最多 1 个，用完即弃；它常静默死亡，所以必须自己读 diff 复验）；
3. 自己跑 `npm run check` + `cd src-tauri && cargo test --lib`，必要时补测试/补实现；
4. `npx tauri build --no-bundle`，记下 `stat -c%s` 与 `md5sum`；
5. 云电脑真机验收（见 §4），判据以**日志里的数字与回读值为准**，不信「看起来像」；
6. 写 `docs/ACCEPTANCE.md` 新章（要证的事 / 验法 / 判据表 / 设计理由 / 坑 / 一句实话），同步 `docs/PARITY-PLAN.md`（结果段 + 进度表行）、`docs/HANDOVER.md`（§0.1 硬数字 + 一条 §0.x）、`ARCHITECTURE.md:277-278,292`、`README.md:65,81`；
7. `git add -A && git commit`，push，最后 `bash /d/tmp/shots/cleanup.sh`。

已完成的：S2 PAC（§48）、S8 分离端口（§49）、S10 强调色（§50）、S11 进程选择器（§51）、S9 回退/负载组（§52）。剩：**S3 → S6 → S5 → S4 → S1 → S7**。

## 4. 云电脑（真机验收的唯一场所）

```bash
SSH="C:/Windows/System32/OpenSSH/ssh.exe"
"$SSH" -i C:/Users/83611/.ssh/id_ed25519_cpc -o StrictHostKeyChecking=no Administrator@100.106.121.0 "命令"
```

- 现成脚本都在 `D:\tmp\shots\`：每个功能一对 `mk_xxx.py`（生成 ps1 驱动）+ `xxx-accept.sh`（部署、跑、回拉日志、收尾）。**照抄最近的 `52-group-run.sh`（要起本地测试服务、读 Clash API）或 `51-proc-accept.sh`（只点界面）最快**。
- 单实例应用必须用 `schtasks /sc once /st 23:59 /it` 在会话 1 启动，直接 ssh 起会死（`WebView2 … 0x800700AA`）。
- 收尾一律 `bash /d/tmp/shots/cleanup.sh`（会 `--quit`、删 `xm*` 任务、验 `ProxyEnable=0`）。**别在开发机上起内核、别动系统代理注册表。**

## 5. 已经踩过的坑（照抄别重踩）

1. `.ps1` 必须 UTF-8 **带 BOM** + CRLF，否则 PowerShell 5.1 按 ANSI 读，中文全变乱码；`param(...)` 必须写在 head 之前。
2. 先 `[Ui]::SetProcessDPIAware()`，否则 150% 缩放下坐标全飘。
3. WebView2 行内按钮：用 `ControlType.Button` + `InvokePattern`，别用坐标，也别被同名 `DataItem` 骗；行内按钮常在行名下方约 160px（取 `Top >= 行顶` 的最小者）。
4. `Scroll-To` 返回的是 `{CX,CY}` 而不是 AutomationElement；对它取 `.Current.BoundingRectangle` 得到 `0,0,0x0`，点了标题栏还不知道为什么。
5. 同名控件可能有好几个（§50 的「恢复默认」有 5 个，第一个在屏幕外）；`Buttons-Named` 只有一个匹配时会解包成单元素，`.Count` 打出来是空串。
6. 输入框的 UIA `Name` 常常就是 placeholder，驱动靠它定位——**改文案会让驱动失效**。
7. 中文输入法会吃掉注入按键，输入文本用 `Set-Clipboard` + Ctrl+A/Ctrl+V。
8. below-the-fold 控件先 `ScrollItemPattern.ScrollIntoView()` 再点。
9. 窗口隐藏后从 UIA 树上消失；第一次点击可能只是激活窗口。
10. `store.json` 有 BOM，写回要保留；`Store-Get` 取不存在的键返回空串（不是失败）。
11. Rust 测试在满载时可能红，空载复跑再判断。
12. 走 ssh 进 cmd 时引号容易被吃掉（`reg query "…\Internet Settings"` 会报无效语法），末态交给 `cleanup.sh` 证明。
13. ssh 里 `start /b` 起的本地服务会随控制台退出而死；要活到应用启动之后，用 `schtasks /sc once /st 23:59 /it` 在会话 1 里起，收尾 `schtasks /delete`。
14. `store.json` 里节点的 `Protocol`/`Transport`/`Security` serde 名是 **PascalCase**（`Shadowsocks`/`Tcp`/`None`），写小写会让整份文档被判 corrupt（远端落成 `store.corrupt`、应用以默认值启动）；而 `last_engine`/`mode` 是小写。

## 6. S1–S11 全部解禁（2026-11-25 用户指示）

旧口径「TUN/PAC/内核更新/服务模式/轻量模式/WebDAV/多配置/分离端口/回退组/主题/按应用分流 明确不做」**已作废**。现在按 `docs/PARITY-PLAN.md` 的顺序做，进度表里有每项状态；已完成的五项见 §3 末。**别再拿旧口径当理由跳过。**

## 7. 索引

- 交接地图与候选表：`docs/HANDOVER.md` §0（现状，含 §0.4–§0.8 五次收口的过程）/ §1（命令）/ §2（架构）/ §7（云电脑手册）
- 每条功能的判据与踩坑：`docs/ACCEPTANCE.md`（§48 PAC、§49 分离端口、§50 强调色、§51 进程选择器、§52 回退/负载组是最新五章）
- 解禁计划与进度：`docs/PARITY-PLAN.md`

# 新窗口开工简报（X-Matrix Client Pro）

> 写于 2026-11-25 00:05。给**下一个窗口**的你自己看：先读这一页，够用就别翻长文档；需要细节时按文末索引跳。
> 长文档：`docs/HANDOVER.md`（地图、候选表、云电脑手册）、`docs/ACCEPTANCE.md`（§21–§46 每条功能的判据与踩坑）。

## 1. 三十秒现状

| 项 | 值 |
|---|---|
| 仓库 / 工作区 | `D:\GitHub-Warehouse\X-Matrix-Client-Pro`（Tauri 2 + Vue 3 + TS） |
| HEAD | 本简报这条 docs 提交（`git log -1` 可查）；最后一个功能提交是 `ca53381`（含 §46） |
| 工作区 | **干净**，没有未提交改动 |
| 规模 | 81 个 IPC 命令 / Rust 单测 **377** / vitest **98**（10 文件）/ i18n **758 键 × 3 语言** |
| 验收 | `docs/ACCEPTANCE.md` 写到 **§46**，全部是云电脑真机点出来的 |
| 云电脑 | 干净：`ProxyEnable=0x0`、无 `xm*` 任务、无内核进程 |
| 构建产物 | `src-tauri/target/release/x-matrix.exe`（10,447,872 B，含 §21–§46） |
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

## 3. 下一个任务只有一件：§47（有现成补丁和验收法子）

**问题**：内核在跑时刷新订阅，后端会把运行中节点「采纳」成新 ID（内核不断），但前端 `src/views/SubscriptionsView.vue` 的 `kernelStopped()` 在内核还活着时提前 return、**不重读设置**，于是界面握着已经不存在的旧 ID —— 首页节点下拉框选不中，「启动」会报「节点 … 已经不在了」。

**做法**（补丁已写好，35 行，只改一个文件）：

```bash
git apply /d/tmp/handover/47-subscription-refresh-id.patch
```

补丁把 `kernelStopped()` 改成：先 `await engine.refreshStatus()`，再**无条件** `await settings.load(true)`，最后才 `return !engine.running`（并把 docblock 改成「后端要么采纳新 ID，要么清空并停内核」）。

**验收**（脚本已有九成，复制 `D:\tmp\shots\adopt-accept.sh` 改成 `mk_*` 新脚本即可）：假订阅源用 `sub-server-b.ps1`（四个节点顺序反转），store 里 `subscription_via_proxy=false`、`last_node_id` 钉到订阅节点；正文在 `active id after` 之后追加 **停止 → 首页「启动」 → 断言内核起来了、页面没有「已经不在了」**。改前应能复现失败（这一步很重要，别只测改后）。

**收尾顺序**：`npm run check` → 构建 → 云电脑验收 → `docs/ACCEPTANCE.md` §47 → 计数同步（命令 81 / Rust 377 / vitest 98 / i18n 758 不变）→ 提交 + push → `bash /d/tmp/shots/cleanup.sh`。

## 4. 云电脑（真机验收的唯一场所）

```bash
SSH="C:/Windows/System32/OpenSSH/ssh.exe"
"$SSH" -i C:/Users/83611/.ssh/id_ed25519_cpc -o StrictHostKeyChecking=no Administrator@100.106.121.0 "命令"
```

- 现成脚本都在 `D:\tmp\shots\`：每个功能一对 `mk_xxx_accept.py`（生成 ps1）+ `xxx-accept.sh`（部署、跑、回拉日志、收尾）。**照抄最近的 `geostale-accept.sh` / `mode-accept.sh` 最快**。
- 单实例应用必须用 `schtasks /sc once /st 23:59 /it` 在会话 1 启动，直接 ssh 起会死（`WebView2 … 0x800700AA`）。
- 收尾一律 `bash /d/tmp/shots/cleanup.sh`（会 `--quit`、删 `xm*` 任务、验 `ProxyEnable=0`）。**别在开发机上起内核、别动系统代理注册表。**

## 5. 已经踩过的坑（照抄别重踩）

1. `.ps1` 必须 UTF-8 **带 BOM** + CRLF，否则 PowerShell 5.1 按 ANSI 读，中文全变乱码。
2. 先 `[Ui]::SetProcessDPIAware()`，否则 150% 缩放下坐标全飘。
3. WebView2 行内按钮：用 `ControlType.Button` + `InvokePattern`，别用坐标，也别被同名 `DataItem` 骗；行内按钮常在行名下方约 160px（取 `Top >= 行顶` 的最小者）。
4. 中文输入法会吃掉注入按键，输入文本用 `Set-Clipboard` + Ctrl+A/Ctrl+V。
5. below-the-fold 控件先 `ScrollItemPattern.ScrollIntoView()` 再点。
6. 窗口隐藏后从 UIA 树上消失；第一次点击可能只是激活窗口。
7. `store.json` 有 BOM，写回要保留；`Store-Get` 取不存在的键返回空串（不是失败）。
8. Rust 测试在满载时可能红，空载复跑再判断。

## 6. 哪些是明确不做的

TUN、PAC、内核自动更新、服务模式、轻量模式、WebDAV、多配置、分端口、回退组、主题、按应用分流（S1–S11，见 `docs/HANDOVER.md` 候选表）。别自作主张加。

## 7. 索引

- 交接地图与候选表：`docs/HANDOVER.md` §0（现状）/ §1（命令）/ §2（架构）/ §6（候选）/ §7（云电脑手册）
- 每条功能的判据与踩坑：`docs/ACCEPTANCE.md` §21–§46（§41 快捷键、§42 语言、§43 离线延迟、§44 模式、§45 测试地址 + sing-box geo、§46 geo 数据陈旧）
- §47 补丁：`D:\tmp\handover\47-subscription-refresh-id.patch`

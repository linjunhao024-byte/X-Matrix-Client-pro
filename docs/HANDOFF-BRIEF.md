# 新窗口开工简报（X-Matrix Client Pro）

> 写于 2026-11-26 02:5x（§61 这一版：S1–S11 与两张候选表全部完成；用户点名的「代码缺口 1」已写完，**真机验收没跑成**——测试机整机掉线）。给**下一个窗口**的你自己看：先读这一页，够用就别翻长文档；需要细节时按文末索引跳。
> 长文档：`docs/HANDOVER.md`（地图、候选表、云电脑手册）、`docs/ACCEPTANCE.md`（每条功能的判据与踩坑）、`docs/PARITY-PLAN.md`（S1–S13 计划与进度表）、`docs/CANDIDATES-PLAN.md`（两张候选表逐条核查的结论）。

## 1. 三十秒现状

| 项 | 值 |
|---|---|
| 仓库 / 工作区 | `D:\GitHub-Warehouse\X-Matrix-Client-Pro`（Tauri 2 + Vue 3 + TS） |
| HEAD | 本简报这条 docs 提交；功能提交查 `git log --oneline -- <anchor>` 的第一条：§61 `src-tauri/src/engine/merge.rs`、§60 `src-tauri/src/hotkeys.rs`、§59 `src-tauri/src/store.rs`、§58 `src-tauri/src/engine/singbox.rs`、§57 `src-tauri/src/service.rs`、§56 `src-tauri/src/elevate.rs`、§55 `src-tauri/src/power.rs`、§54 `src-tauri/src/net/webdav.rs`、§53 `src-tauri/src/engine/release.rs`、§52 `src-tauri/src/engine/groupwatch.rs`、§51 `src-tauri/src/sysproc.rs`、§50 `src/services/accent.ts`、§49（S8）、`a794438`（§48）、`6fd790f`（§47） |
| 规模 | **98** 个 IPC 命令 / Rust 单测 **539** / vitest **116**（12 文件）/ i18n **865 键 × 3 语言** |
| 验收 | `docs/ACCEPTANCE.md` 写到 **§61**：§0–§60 全是云电脑真机点出来的（各节末尾的「一句实话」列着未验项）；**§61 例外——代码与门槛已过、真机没跑**，判据表里需要真机的行都标 ⏸，原因写在节首与「一句实话」里 |
| 云电脑 | **当前不可达**（2026-11-26 02:25 起：Tailscale 显示 `cloud-pc-test active`，但 `ping` / `ssh` 100% 丢包、`tailscale ping` 超时 `rx 0`）。上次干净收尾时状态是：`ProxyEnable=0x0`、无 `xm*`/`tunkill`/`xmlisten` 任务、无内核进程、无 `XMatrixKernel` 服务 |
| 构建产物 | `src-tauri/target/release/x-matrix.exe`（11,115,008 B @2026-11-26 02:14，md5 `e5ec8f604cd4e63da510f05897010693`，含 §21–§61）；**安装包已能离线打**：`bundle/msi/X-Matrix_0.1.0_x64_en-US.msi`（5,156,864 B）、`bundle/nsis/X-Matrix_0.1.0_x64-setup.exe`（3,587,111 B） |
| 目标 | 功能追平 V2RayN / Clash、二者互补，并把各功能之间**说得清、配合得上**（用户原话，长期有效） |

## 2. 常用的四条命令

```bash
cd /d/GitHub-Warehouse/X-Matrix-Client-Pro

# 门槛（必须全绿再谈提交）
npm run check                                        # typecheck + 架构 + 契约 + vitest
cd src-tauri && cargo test --lib && cd ..

# 构建（4–8 分钟；PATH/CARGO_HOME 必须这样写）
export PATH=/d/CODE.config/Cargo.config/bin:$PATH
export CARGO_HOME='D:\CODE.config\Cargo.config'
npx tauri build --no-bundle                          # 只出 exe
npx tauri build                                      # 连安装包一起（WixTools314/NSIS 已缓存在 %LOCALAPPDATA%\tauri，离线可打）

# 推送（本机解析不了 github.com，必须带代理）
https_proxy=http://127.0.0.1:10808 git push origin main
```

改 Rust 的 `#[tauri::command]` 签名后必须 `npm run gen:contract`；新增 i18n 键必须走 `scripts/i18n-pairs.json` + `python scripts/gen_i18n.py`（三份字典会一起生成）。

## 3. 下一个任务：把 §61 的真机验收补上，然后做安装包 E2E（§62）

用户 2026-11-26 的指令是「先做 1 和 2，3 我之后自己测试，4 你也可以在做完 1 和 2 之后做」：

- **1（两个代码缺口）**：服务日志路径 + 配置覆盖层 merge —— 代码、门槛、构建都完成了，**真机验收待补**（§61）。
- **2（安装包 + 全新机器 E2E）**：MSI/NSIS 已经打出来了（见 §1），E2E 脚本还没写；**等机器回来**才能跑。
- **3（真键盘 / 真 UAC / 真 WebDAV）**：用户自己测，不要抢。
- **4（长跑 / 压力）**：用户说做完 1 和 2 之后「也可以」做。环回节点就够压（`127.0.0.1:8388/8389` + 死端口 9999）；不需要用户提供真节点。

机器回来后的顺序：

1. `bash /d/tmp/shots/61-merge-accept.sh m1` —— 两个阶段（`merge` / `svclog`）。要拿到：落盘 `configs/xray.json` 的 `dns.servers` 恰好是覆盖里的两条（`1.1.1.1` / `9.9.9.9`，不是设置里的 `8.8.8.8` 加覆盖的三条）、界面里坏 YAML 被拒并显示解析器原话、装服务后 `%ProgramData%\X-Matrix\service\xray.log` 非空且日志页能看到行。
2. 把 `docs/ACCEPTANCE.md` §61 判据表里 ⏸ 的行换成实测数字，改 `docs/HANDOVER.md` §0.17 的「真机结论」段，把这一页的「验收」行也改掉。
3. 写 `D:\tmp\shots\62-msi-e2e.sh`（`e2e-sub.yaml` / `subserv.ps1` / `ss-e2e.json` 已在 `D:\tmp\shots\e2e\`）：卸旧 → 清 `%APPDATA%\com.xmatrix.client` → `msiexec /i X-Matrix_0.1.0_x64_en-US.msi /qn` → 首启 → 加假订阅（`http://127.0.0.1:8090/sub`）→ 起内核 → `curl -x http://127.0.0.1:7890 http://cp.cloudflare.com/` 期望 204 → 开系统代理查注册表 → 停 → 卸载 → 验 `%ProgramData%`/AppData/服务/注册表都清干净。**§62 里要如实写两件事**：内核二进制是预置的（云电脑到 GitHub 只有 5.5 KB/s），以及 `msiexec` 需要管理员（这台机器是 Administrator）。
4. 若用户还要第 4 项：长跑（30 分钟）+ 20 次配置切换 + 200 次启停，记 `WorkingSet64` / 句柄数 / 日志大小 / 端口绑定时延的曲线，别只写「稳定」。

若用户给的是**新功能**，沿用这套固定流程（十三项都是这么走完的）：

1. 写任务书到 `D:\tmp\handover\tasks\S<n>-*.md`（判据、钉死的设计、可改文件范围、验证命令、禁区）；
2. 派**一个**子代理（同时最多 1 个，用完即弃；它常静默死亡，所以必须自己读 diff 复验，必要时自己写完）；
3. 自己跑 `npm run check` + `cd src-tauri && cargo test --lib`，必要时补测试/补实现；
4. `npx tauri build --no-bundle`，记下 `stat -c%s` 与 `md5sum`；
5. 云电脑真机验收（见 §4），判据以**日志里的数字与回读值为准**，不信「看起来像」；
6. 写 `docs/ACCEPTANCE.md` 新章（要证的事 / 验法 / 判据表 / 设计理由 / 坑 / 一句实话），同步 `docs/PARITY-PLAN.md`（结果段 + 进度表行）、`docs/HANDOVER.md`（§0.1 硬数字 + 一条 §0.x + checklist）、`ARCHITECTURE.md:277-278`、`README.md:65,81`；
7. `git add -A && git commit`，push，最后 `bash /d/tmp/shots/cleanup.sh`。

已完成：S2 PAC（§48）、S8 分离端口（§49）、S10 强调色（§50）、S11 进程选择器（§51）、S9 回退/负载组（§52）、S3 内核更新（§53）、S6 WebDAV 备份（§54）、S5 轻量模式（§55）、S4a 提权（§56，判据 7、8 未验）、S4b 内核服务（§57，三项未验）、S1 TUN（§58，非管理员拒绝未验）、S7 多配置（§59，merge/script 未做→merge 已在 §61 补上）、候选表两项（§60，真实按键未验）、代码缺口两项（§61，真机待补）。

## 4. 云电脑（真机验收的唯一场所）

```bash
SSH="C:/Windows/System32/OpenSSH/ssh.exe"
"$SSH" -i C:/Users/83611/.ssh/id_ed25519_cpc -o StrictHostKeyChecking=no Administrator@100.106.121.0 "hostname"
```

- 主机 `100.106.121.0`（仅 tailnet）、用户 `Administrator`、系统 Windows Server 2022、默认 shell **cmd.exe**（多命令用 `&`，禁 `&&`/单引号/heredoc）。
- 单实例应用（Tauri + single-instance 插件）**不能用 ssh 直接起**，必须 `schtasks /create ... /sc once /st 23:59 /it` 在会话 1 里起；task 的 action **不能直接指 `.ps1`**，要 `powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File C:/.../run-app.ps1`。
- 驱动脚本一律「本地 `mk_xxx.py` 生成 → `scp` 推上去 → `powershell -File` 触发」，日志按阶段分文件（`xxx-<phase>.txt`），`findstr /C:DONE` 轮询。
- 收尾必须 `bash /d/tmp/shots/cleanup.sh`，并确认 `ProxyEnable REG_DWORD 0x0` 与 `tasks-above-should-be-empty`。
- `cleanup.sh` **会删 home 下的 `*.ps1`/`*.bat`/`*.txt`/`*.png`**，所以每一轮都要重新推 `run-app.ps1` 与驱动。
- 服务相关验收一条会话里先 `install` 再 `uninstall`，否则一开头 `sc stop/delete` 会把要验的东西提前清掉。
- 机器掉线时（`ping` 通但 `ssh` 超时 / 反过来）：先 `tailscale status | grep cloud-pc-test`，再重连一次；整机 `ping` 100% 丢包 + `tailscale ping` 超时 = VM 关机了，我这边没有开机权限，**别把验收先写成通过**，标 ⏸ 并告诉用户。

## 5. 已经踩过的坑（照抄别重踩）

1. PowerShell 脚本必须 UTF-8 **BOM + CRLF**；`param(...)` 要写在文件第一行。
2. 高 DPI 下 UIA 坐标是物理像素、窗口 rect 是逻辑像素，混用会点偏。
3. 按钮用 `InvokePattern` 点，别用 `Click()`（会话被遮挡时后者无效）。
4. `Scroll-To` 返回的是 `[pscustomobject]{CX,CY}`，不是 AutomationElement；`$e.Current.BoundingRectangle` 对它取不到值。
5. 同名控件会有一堆（「恢复默认」有 5 个），按 `Top` 排序取第 0 个常常在屏幕外；用「同一行 + 锚点右侧」挑。
6. `Buttons-Named` 返回 PSCustomObject（`Top/Left/W/H/El`），`$b.Current.IsEnabled` 永远是空串。
7. WebView2 把 `<label>` 文本挂成 **Edit 自己的 `Name`**，按 label 找输入框必失败；用 `Edit-Rows` 按 Name 匹配 + `ValuePattern.SetValue`。
8. 中文要用 `SendKeys` 或剪贴板，`Type()` 打不出来。
9. 设置页比窗口长：点任何按钮前先确认它的坐标在窗口 rect 内，越界先 `Scroll-Row`（按钮自身 `ScrollIntoView` 也行）。
10. 窗口隐藏后整棵 UIA 树消失，驱动别在隐藏期间读控件；隐藏期间的证据只能用进程/文件/注册表。
11. store.json 是 UTF-8 **BOM**；读之前 `TrimStart([char]0xFEFF)`。
12. 满载红（traffic chart）只在有真实流量时出现，别拿它当「内核在跑」的判据。
13. ssh 里 cmd 会吃掉引号；带引号的 `powershell -Command "..."` 别用，上传 `.ps1` 用 `-File`。
14. `start /b` 起的进程活不过 ssh 会话；要在会话 1 起就用 `schtasks /it`。
15. store 里 `Protocol`/`Transport`/`Security` 是 **PascalCase**（`{"kind":"Shadowsocks"}`）；写小写会让整份文档被判 corrupt。
16. cmd 不认双反斜杠路径（`C:\\Users\\...`）里的 `del` 会静默失败；bash 里拼 Windows 路径别用 `\\`。
17. `cleanup.sh` 会删 home 下 `*.ps1/*.bat/*.txt/*.png`，每轮重新推。
18. 打补丁脚本一律用 `write` 工具写成 `.py` 再 `python 文件`；`ops_bash` 的 heredoc 里 `\\` 会折叠成 `\`（`D:\\tmp` → `\t`），python 在 Windows 下也不认 `/d/...` 路径（用 `D:/...`）。
19. 明文密码不许进 `store.json`；WebDAV 密码走 Windows 凭据管理器（`X-Matrix/WebDAV`）。
20. `hyper` 把 header 名写成小写（断言用 `to_lowercase()`）；specta 只给与非 `_Serialize` 后缀的类型（`WebdavState` 没有 `_Serialize`）。
21. `"$tag: ..."` 在 PowerShell 里是**驱动器限定变量**（`TAG` 盘），解析期就死；用 `$($tag)`。
22. WebView2 渲染在 `msedgewebview2.exe` 子进程，回收本进程工作集不等于省界面内存；`SetMemoryUsageTargetLevel` 在 `ICoreWebView2_19` 上。
23. 首页「启动」按钮与节点行的「启动」同名；用 `auto_connect=true` 让内核自己起，或 `Row-Top-Like` + `Invoke-Below`。
24. CPU 时间不能当「定时器停了」的证据（轮询本身几乎不耗 CPU）。
25. **点禁用按钮也返回 True**：证据要读 PID / 实例数 / 状态值，不能只看「点了」。
26. 云电脑 `EnableLUA=0`，造不出「非管理员但可提权」的进程；真 UAC 只能在用户自己的机器上验。
27. 逐阶段 wrapper 的 `\r\n` 会被写成字面文本 → 用**一个** `run.ps1` 带 `-Phase`。
28. python 往 GBK 控制台打印中文/私有区字符会 `UnicodeEncodeError`；用 `sys.stdout.reconfigure(encoding='utf-8', errors='replace')`。
29. 卸载服务时保留 `jobs` 目录与 ACL 是设计，不是漏删。
30. `schtasks /tr` 不能直接指 `.ps1`（不会执行）；必须 `powershell -File`。
31. `push <local> <remote-name>` 是两参数签名，写错会把 bat 的内容写进 json。
32. TUN 的 `auto_route` 会带走 ssh：跑之前先挂 kill 看门狗；每阶段 `taskkill /im sing-box.exe` 会连胜信号一起杀。
33. 环回节点必须 `bind_interface` 指真实网卡，否则节点自己的出网又进隧道形成环路；gstatic 经环回超时不能当隧道判据。
34. v1 → v2 的 store 升级条件要「`profiles` 空且没有 `active`」，否则每次启动都写盘。
35. 备份里 `StoredProfile` 是扁平结构（顶层 `nodes`），按 `parts.nodes` 读会得 0。
36. 设置页比窗口长，`WebDAV 云备份` 在 `top=7093`，点前必须 `Scroll-Row`。
37. 模拟按键到不了别的进程的 `RegisterHotKey`（三种注入都试过）；用 `EnumWindows` 找类名 `XMatrixHotkeys` 再 `PostMessage(0x0312, id)`。`FindWindow(null,'X-Matrix')` 会返回 0。
38. 热键触发要先打一行日志（`快捷键 X 触发`），否则「触发了但没做事」和「没收到」分不开。
39. 驱动里打印 stderr 的文件路径要自己定义（`$errFile = Join-Path $env:USERPROFILE 'app.err.txt'`）；`58-head.ps1` 那套 helper 里没有这个变量。
40. `!!binary` 不是「YAML 标签」：`serde_yaml 0.9` 会自己解析成字符串，测「拒绝自定义 tag」要用 `!custom hi`。
41. 「逐字节相等」不能用 `serde_json::Value` 比（键序差异会被吞），要 `to_string` 后再比。
42. `RenderInput.merge_yaml` 是 `String` 不是 `&'a str`：`with_options` 按值消费 `RunOptions`，借不出来。
43. 测试机整机掉线时（`ping` 100% 丢包 + `tailscale ping` 超时 `rx 0`）= VM 关机，不是网络抖动；验收标 ⏸，别写成通过。

## 6. 这一段为什么「解禁」了

用户 2026-11-25 原话：「为什么S1–S11？我最开始的目标就是把X-Matrix-Client-pro的功能做到V2rayN和Clash二者相结合……你全部解禁，一个一个写明确的任务书&提示词派子代理去做」。旧的「明确不做」口径（`D:/tmp/parity-gaps.md` §2.3）**已作废**；进度只认 `docs/PARITY-PLAN.md` 与 `docs/ACCEPTANCE.md`。

## 7. 索引

- 交接地图与候选表：`docs/HANDOVER.md`（§0.x 是每次收尾的追加小节，§6 是候选表；本版对应 §0.17）
- 验收判据与踩坑：`docs/ACCEPTANCE.md`（§0–§61；§7 历史空缺）
- 计划与进度：`docs/PARITY-PLAN.md`（S1–S13）
- 候选表核查：`docs/CANDIDATES-PLAN.md`
- 设计规则：`C:\Users\83611\DESIGN-SKILL.md`，检查器 `python /c/Users/83611/check-design.py`
- 真机脚本目录：`D:\tmp\shots\`（每个 § 一对 `mk_xx.py` + `xx-accept.sh`；`cleanup.sh` 是收尾）
- 任务书目录：`D:\tmp\handover\tasks\`
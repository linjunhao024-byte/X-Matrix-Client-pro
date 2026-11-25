# 新窗口开工简报（X-Matrix Client Pro）

> 写于 2026-11-25 20:5x（§60 / 候选表收口这一版：S1–S11 与两张候选表上的剩余项**全部做完**）。给**下一个窗口**的你自己看：先读这一页，够用就别翻长文档；需要细节时按文末索引跳。
> 长文档：`docs/HANDOVER.md`（地图、候选表、云电脑手册）、`docs/ACCEPTANCE.md`（每条功能的判据与踩坑）、`docs/PARITY-PLAN.md`（S1–S11 计划与进度表）、`docs/CANDIDATES-PLAN.md`（两张候选表逐条核查的结论）。

## 1. 三十秒现状

| 项 | 值 |
|---|---|
| 仓库 / 工作区 | `D:\\GitHub-Warehouse\\X-Matrix-Client-Pro`（Tauri 2 + Vue 3 + TS） |
| HEAD | 本简报这条 docs 提交；功能提交查 `git log --oneline -- <anchor>` 的第一条：§60 `src-tauri/src/hotkeys.rs`、§59 `src-tauri/src/store.rs`、§58 `src-tauri/src/engine/singbox.rs`、§57 `src-tauri/src/service.rs`、§56 `src-tauri/src/elevate.rs`、§55 `src-tauri/src/power.rs`、§54 `src-tauri/src/net/webdav.rs`、§53 `src-tauri/src/engine/release.rs`、§52 `src-tauri/src/engine/groupwatch.rs`、§51 `src-tauri/src/sysproc.rs`、§50 `src/services/accent.ts`、§49（S8）、`a794438`（§48）、`6fd790f`（§47） |
| 工作区 | **干净**，没有未提交改动 |
| 规模 | 97 个 IPC 命令 / Rust 单测 **519** / vitest **116**（12 文件）/ i18n **860 键 × 3 语言** |
| 验收 | `docs/ACCEPTANCE.md` 写到 **§60**，全部是云电脑真机点出来的（§56 判据 7、8、§57 的三项、§58 的非管理员拒绝、§59 的「新配置没节点」「托盘刷新」「merge/script」、§60 的「真实按键 → 热键注册」都明标了「未验」与原因） |
| 云电脑 | 干净：`ProxyEnable=0x0`、无 `xm*`/`tunkill`/`xmlisten` 任务、无内核进程、无 `XMatrixKernel` 服务、`HKLM\\Software\\X-Matrix\\Service` 已删、`X-Matrix/WebDAV` 凭据已删、`engines\\wintun.dll.bak` 与 `ss-server.json` 已删；UAC 三项策略已恢复原值（`EnableLUA=0`） |
| 构建产物 | `src-tauri/target/release/x-matrix.exe`（11,084,288 B，md5 `8ebbcba7fbd37af649bff3af4d2d56d2`，含 §21–§60） |
| 目标 | 功能追平 V2RayN / Clash、二者互补，并把各功能之间**说得清、配合得上**（用户原话，长期有效） |

## 2. 常用的四条命令

```bash
cd /d/GitHub-Warehouse/X-Matrix-Client-Pro

# 门槛（必须全绿再谈提交）
npm run check                                        # typecheck + 架构 + 契约 + vitest
cd src-tauri && cargo test --lib && cd ..            # Rust 单测

# 构建（4–8 分钟；PATH/CARGO_HOME 必须这样写）
export PATH=/d/CODE.config/Cargo.config/bin:$PATH
export CARGO_HOME='D:\\CODE.config\\Cargo.config'
npx tauri build --no-bundle                          # 产物 src-tauri/target/release/x-matrix.exe

# 推送（本机解析不了 github.com，必须带代理）
https_proxy=http://127.0.0.1:10808 git push origin main
```

改 Rust 的 `#[tauri::command]` 签名后必须 `npm run gen:contract`；新增 i18n 键必须走 `scripts/i18n-pairs.json` + `python scripts/gen_i18n.py`（三份字典会一起生成）。

## 3. 下一个任务：队列是空的（先问用户要什么，不要自己加戏）

- S1–S11 十一项：2026-11-25 全部完成、逐项真机验收、逐项提交（§48–§59，`docs/PARITY-PLAN.md` 进度表全是「已完成」）。
- 候选表：按用户 2026-11-25「按照顶部候选表剩下的执行」核查了 `docs/HANDOVER.md` §6 与 `D:\\tmp\\parity-gaps.md` §2.1/§2.2，**真正没做过的只有两项**——热键的 `mode` / `test_all` 两个动作、测速的 `test_on_start`；两项都在 §60 做完并验收。其余 20 余条在代码里都有落点（逐条证据在 `docs/CANDIDATES-PLAN.md`）。
- 所以现在**没有排队任务**。动手前先问用户；不要自己把「订阅级 merge / script 覆盖脚本」「按连接分摊的真负载均衡」这类**当时没被解禁**的东西当授权（它们在 `docs/PARITY-PLAN.md` 的判据段里写着，属未做边界，不是待办）。

若用户给的是新功能，沿用下面这套固定流程（十二项都是这么走完的）：

1. 写任务书到 `D:\\tmp\\handover\\tasks\\S<n>-*.md`（判据、钉死的设计、可改文件范围、验证命令、禁区）；
2. 派**一个**子代理（同时最多 1 个，用完即弃；它常静默死亡，所以必须自己读 diff 复验，必要时自己写完）；
3. 自己跑 `npm run check` + `cd src-tauri && cargo test --lib`，必要时补测试/补实现；
4. `npx tauri build --no-bundle`，记下 `stat -c%s` 与 `md5sum`；
5. 云电脑真机验收（见 §4），判据以**日志里的数字与回读值为准**，不信「看起来像」；
6. 写 `docs/ACCEPTANCE.md` 新章（要证的事 / 验法 / 判据表 / 设计理由 / 坑 / 一句实话），同步 `docs/PARITY-PLAN.md`（结果段 + 进度表行）、`docs/HANDOVER.md`（§0.1 硬数字 + 一条 §0.x + checklist）、`ARCHITECTURE.md:277-278`、`README.md:65,81`；
7. `git add -A && git commit`，push，最后 `bash /d/tmp/shots/cleanup.sh`。

已完成：S2 PAC（§48）、S8 分离端口（§49）、S10 强调色（§50）、S11 进程选择器（§51）、S9 回退/负载组（§52）、S3 内核更新（§53）、S6 WebDAV 备份（§54）、S5 轻量模式（§55）、S4a 提权（§56，判据 7、8 未验）、S4b 内核服务（§57，三项未验）、S1 TUN（§58，非管理员拒绝未验）、S7 多配置（§59，merge/script 未做）、候选表两项（§60，真实按键未验）。

## 4. 云电脑（真机验收的唯一场所）

```bash
SSH="C:/Windows/System32/OpenSSH/ssh.exe"
"$SSH" -i C:/Users/83611/.ssh/id_ed25519_cpc -o StrictHostKeyChecking=no Administrator@100.106.121.0 "命令"
```

- 现成脚本都在 `D:\\tmp\\shots\\`：每个功能一对 `mk_xxx.py`（生成 ps1 驱动）+ `xxx-accept.sh`（部署、跑、回拉日志、收尾）。**照抄最近的 `60-hotkeys-accept.sh`（多阶段 + 每阶段自己的日志 + 宿主侧独立复核）或 `57-service-accept.sh`（统一 `run.ps1` + `-Phase`）最快。**
- 单实例应用必须用 `schtasks /sc once /st 23:59 /it` 在会话 1 启动，直接 ssh 起会死（`WebView2 … 0x800700AA`）；任务的 action **必须是 `powershell -File xxx.ps1`**，直接指向 `.ps1` 不会执行。
- 服务相关验收要一条会话里先 install 再 uninstall（每次会话开头会 `sc stop` + `sc delete`）。
- TUN 阶段必须自己起环回测试节点（`ss-server.json`，`bind_interface` 写网卡名，且该节点**在**同一台机器上），并挂一个 `tunkill.bat` 看门狗任务（`ping -n 181` 后 `taskkill` 内核与 app）——`auto_route` 会接管默认路由，把 ssh 一起带走。
- 热键类的验收：注册链看 `app.err.txt` 的「全局快捷键 … 已就绪/已关闭/注册失败」，动作链用 `PostMessage(WM_HOTKEY=0x0312, id)` 打到类名 `XMatrixHotkeys` 的窗口（见 §5 第 37 条）。
- 收尾一律 `bash /d/tmp/shots/cleanup.sh`（会 `--quit`、删 `xm*` 任务、验 `ProxyEnable=0`）。**别在开发机上起内核、别动系统代理注册表。**

## 5. 已经踩过的坑（照抄别重踩）

1. `.ps1` 必须 UTF-8 **带 BOM** + CRLF，否则 PowerShell 5.1 按 ANSI 读，中文全变乱码；`param(...)` 必须写在 head 之前。
2. 先 `[Ui]::SetProcessDPIAware()`，否则 150% 缩放下坐标全飘。
3. WebView2 行内按钮：用 `ControlType.Button` + `InvokePattern`，别用坐标，也别被同名 `DataItem` 骗；行内按钮常在行名下方约 160px（取 `Top >= 行顶` 的最小者）。
4. `Scroll-To` 返回的是 `{CX,CY}` 而不是 AutomationElement；对它取 `.Current.BoundingRectangle` 得到 `0,0,0x0`，点了标题栏还不知道为什么。
5. 同名控件可能有好几个（§50 的「恢复默认」有 5 个，第一个在屏幕外）；`Buttons-Named` 只有一个匹配时会解包成单元素，`.Count` 打出来是空串，且它返回的是 `PSCustomObject`——读 `enabled` 要用 `$b.El.Current.IsEnabled`，点它要用 `$b.CX/$b.CY`。
6. 输入框的 UIA `Name` 常常就是 placeholder 或它外面 `<label>` 的文字（§54：`<label>用户名</label>` 变成 `Edit.Name = 用户名`，label span 根本不是独立元素）。所以「按标签找输入框」会失败，直接按 `Edit.Name` 匹配；**改文案会让驱动失效**。
7. 中文输入法会吃掉注入按键，输入文本用 `Set-Clipboard` + Ctrl+A/Ctrl+V，或 `ValuePattern.SetValue`。
8. below-the-fold 控件先 `ScrollItemPattern.ScrollIntoView()` 再点；设置页比窗口长，内核行按钮坐标会是屏幕外值（3706 这种），必须先 `Scroll-Row`。**点之前还要判断坐标落在窗口 rect 内**：§57 里 `rect=444,1152`（窗口只有 800 高）点下去按钮纹丝不动、日志照样说 `clicked: True`——取 `Get-Win` 的 `BoundingRectangle` 算可点区间。
9. 窗口隐藏后从 UIA 树上消失；第一次点击可能只是激活窗口。
10. `store.json` 有 BOM，写回要保留；`Store-Get` 取不存在的键返回空串（不是失败）。
11. Rust 测试在满载时可能红，空载复跑再判断。
12. 走 ssh 进 cmd 时引号容易被吃掉（`reg query "…\\Internet Settings"` 会报无效语法），末态交给 `cleanup.sh` 证明。
13. ssh 里 `start /b` 起的本地服务会随控制台退出而死；要活到应用启动之后，用 `schtasks /sc once /st 23:59 /it` 在会话 1 里起，收尾 `schtasks /delete`。
14. `store.json` 里节点的 `Protocol`/`Transport`/`Security` serde 名是 **PascalCase**（`Shadowsocks`/`Tcp`/`None`），写小写会让整份文档被判 corrupt（远端落成 `store.corrupt`、应用以默认值启动）；而 `last_engine`/`mode` 是小写。
15. **cmd 不认双反斜杠路径**（`del C:\\\\Users\\\\...\\\\stop` 会静默失败，`dir` 还看得见文件）。写脚本时 `del` 用单反斜杠。
16. `cleanup.sh` 会删掉云电脑 home 目录下的 `*.ps1`/`*.bat`/`*.txt`/`*.png`，所以每次验收都要重推驱动脚本与启动器；子目录（如 `mirror\\`、`dav-store\\`）里的文件不受影响。
17. 云电脑到 GitHub 只有 ~5.5 KB/s（16.7 MB 要 ~50 分钟）。要验下载链路就给**测试二进制**把 feed URL 等长改指本机镜像（`53-kernel-accept.sh` 的 `MIRROR=1`），并在验收文档里如实写明哪一段被替代。
18. 用 python 改文档/脚本时，普通字符串里的 `\\t`/`\\s`/`\\U` 会被当转义（会把 `D:\\tmp` 变成制表符、或直接 SyntaxError）。**打补丁脚本用 `write` 工具写成 .py 再执行**，别用 `ops_bash` 的 heredoc 内联（含中文 + 反斜杠 + 三引号必被截断）；写完 `grep -c $'\\t'` 验一遍。另外 python 在 Windows 下不认 `/d/...` 这种 MSYS 路径，脚本里的文件路径写成 `D:/...`。
19. **明文密码不许进 `store.json`。** 那份文件会整份写盘、还会随导出/备份离开本机；密码走 Windows 凭据管理器（`syscreds.rs`，target `X-Matrix/WebDAV`），界面只知道「有没有存过」。
20. `reqwest`/`hyper` 发出的 header 名是小写，断言原始 HTTP 文本前先 `to_lowercase()`；specta 给新结构体只生成原名（`WebdavState`），别照 `Settings_Serialize` 的规律加后缀。
21. PowerShell 会把 `"$tag: …"` 里的 `$tag:` 当**驱动器限定变量**（`TAG` 盘），整份脚本解析期就死；`[Parser]::ParseFile` 不抛异常**不代表没有解析错误**，必须把 `$errors.Count` 印出来。
22. WebView2 的内存大头在 6 个 `msedgewebview2.exe` 子进程（约 330 MB），宿主 `X-Matrix.exe` 只有 34 MB；`SetMemoryUsageTargetLevel(Low)` 在这套简单界面上几乎不产生效果——**别把宿主工作集回收当成「界面省了 25 MB」**。
23. 首页「启动」按钮与节点页行内按钮同名，按名字点会歧义；让 `auto_connect`（启动 1.2 秒后自动连 `last_node_id`）起内核最稳。
24. 隐藏窗口这类「看不见的状态」别用 CPU 增量当证据（本地轮询几乎不耗 CPU）；要证「回到窗口拿到的是新状态」，就在隐藏期间把内核杀掉、回窗看徽章是否立刻变「未运行」。
25. **点了按钮返回 True 不等于产品做了事。** §56 里点一个 `enabled=False` 的按钮，`Click` 照样返回 True；证据要读 PID / 进程实例数 / 回读值，别信点击结果。
26. 云电脑原本 `EnableLUA=0`（UAC 全关，所有进程都是完整令牌），所以**造不出「非管理员但可提权」的进程**：打开 UAC 后 `runas /trustlevel:0x20000` 起不来、`schtasks` 起的仍是 High、ssh 在会话 0 够不到会话 1。要验真提权，得换一台能用普通用户登录且 UAC 管理审批打开的机器；改注册表前先记下三个值，验完恢复并重启。
27. 每阶段生成 wrapper ps1 时，把 `\\r\\n` 拼进单引号字符串会变成**字面文本**（`$env:XM_PHASE = 'ui'\\r\\n& ...`），整份脚本解析失败且日志根本不出现。改成**一个** `run.ps1` 带 `-Phase` 参数，比每阶段生成一份稳。
28. python 打印驱动日志会在 GBK 控制台崩（日志里有 PUA 图标字符，`UnicodeEncodeError: 'gbk' codec can't encode character '\\ue047'`）；回拉日志的打印器要 `sys.stdout.reconfigure(encoding='utf-8', errors='replace')`（见 `D:\\tmp\\shots\\show_log.py`）。`sc query` 的中文经 ssh 管道也是乱码，判定按 `STATE :`/`1060`/`RUNNING` 这些 ASCII 关键字。
29. 内核服务的作业目录 `C:\\ProgramData\\X-Matrix\\jobs` 故意保留给 `BUILTIN\\Users:(OI)(CI)(M)`（界面要往里丢作业）；卸载服务时只删服务与注册表键、不删目录与 ACL——这是设计，不是漏删。
30. `schtasks /tr` 的 action **不能直接写 `.ps1`**（给了也不执行，日志里表现成「app 根本没起」）；必须 `powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File C:/Users/.../xxx.ps1`。
31. 验收脚本里的 `push <local> <remote-name>` 是**两参数**签名；写成 `push a.bat b.json` 会把 bat 的内容写进 json（§58 里这条让 `sing-box` 报 `decode config …: invalid character '@' looking for beginning of value`，看起来像配置写错，其实是推错文件）。
32. TUN 会接管默认路由：`auto_route` 起来后 ssh 也走隧道，所以每个 TUN 阶段都要先挂 `tunkill.bat` 看门狗；另外**每阶段 `taskkill /f /im sing-box.exe` 会连环回测试节点一起杀掉**，之后的「隧道不通」其实是没有节点——最后让驱动自己起节点并打 `node server listening: N`。
33. 环回测试节点必须 `bind_interface` 指向真实网卡（中文网卡名经 shell 会变 GBK，要用 UTF-8 生成配置），否则节点自己的出网又进隧道，日志是 `dial tcp 127.0.0.1:8388 … i/o timeout`。gstatic 这类目标在测试环境里超时**不能**当隧道故障判据（同机直连 `direct_gstatic=204`，经环回节点却 `x/x/x`）。
34. `store.json` 的 v1 升级条件要写成「`profiles` 空且没有 `active`」，只看 `version < 2` 会把「用户删到只剩一个配置」的正常状态又迁一遍。
35. 备份里非激活配置的字段是**扁平**的（`StoredProfile` flatten 了 `ProfileParts`，没有 `parts` 这一层）；验收脚本按 `parts.nodes` 读会拿到 0，读顶层才对。
36. 设置页比窗口长（WebDAV 那块 `top=7093`），点前必须 `Scroll-Row`，否则 `clicked: True` 而什么都没发生。
37. **模拟按键到不了别的进程的 `RegisterHotKey`。** `keybd_event`（同会话、先 `SetForegroundWindow`、`/rl HIGHEST` 任务）三种都试过，应用一个都没收到；同一段代码能让注入进程**自己**注册的热键命中。要验动作链就用 `EnumWindows` 找类名 `XMatrixHotkeys` 的窗口，再 `PostMessage(0x0312, id)`；`FindWindow(null,'X-Matrix')` 会返回 0（标题里有不可见字符），`GetForegroundWindow` 也可能返回 0。
38. 看「热键响没响」要在 `window_proc` 里、调动作之前先打一行 `x-matrix: 快捷键 … 触发`；否则「按键到了但动作没跑」与「按键根本没到」无法区分。
39. 驱动里打印 stderr 的文件路径要自己定义（`$errFile = Join-Path $env:USERPROFILE 'app.err.txt'`）；`58-head.ps1` 那套 helper 里没有这个变量，直接用会 `Test-Path $null` 静默失败，日志里连 `<no file>` 都不出现。

## 6. S1–S11 已全部解禁并全部完成

旧口径「TUN/PAC/内核更新/服务模式/轻量模式/WebDAV/多配置/分离端口/回退组/主题/按应用分流 明确不做」**已作废**。进度看 `docs/PARITY-PLAN.md`（十二行全「已完成」）；候选表看 `docs/HANDOVER.md` §6（已全部划掉）与 `docs/CANDIDATES-PLAN.md`（逐条核查）。**别再拿旧口径当理由跳过，也别把没解禁的条目当待办。**

## 7. 索引

- 进度唯一真相源：`docs/ACCEPTANCE.md`（§0–§60，§7 历史空缺）
- 全文地图与云电脑手册：`docs/HANDOVER.md`
- 计划与逐项判据：`docs/PARITY-PLAN.md`
- 候选表逐条核查（含「已做」的证据位置）：`docs/CANDIDATES-PLAN.md`
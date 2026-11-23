# AGENTS.md — X-Matrix Client Pro 项目规则

> 适用范围：本目录 `D:\GitHub-Warehouse\X-Matrix-Client-Pro` 及其所有子目录。
> 本文件只写**本项目特有**的内容；本机全局规则一律继承，不在这里重复正文。
> 优先级：`C:\Users\83611\AGENTS.md`（本机全局）> 本文件 > 项目文档。冲突时以全局为准。

---

## 0. 开工前必读 / Read first

1. **先读本机全局规则** `C:\Users\83611\AGENTS.md`（最高优先级）：不许弹黑窗、产物用绝对路径、不许乱杀进程、新建文件英文命名、前端设计路由、SSH、mdp、看门狗、浏览器自动化。
2. dsh 应用级 `C:\Users\83611\.dsh\AGENTS.md` 同样生效（它只做指路，正文以上一条为准）。
3. 再按任务读对应细则，不许凭记忆猜工具用法：
   - 界面 / 视觉 / 任何 UI 产物 → `C:\Users\83611\DESIGN-SKILL.md`（颜色、圆角、动效一律取 `src/styles/tokens.css`；交付前 `python check-design.py <page>` 必须 0 error）
   - SSH / 云电脑 → `C:\Users\83611\SSH-SKILL.md`，权威清单是 `C:\Users\83611\SSH.md`
   - 浏览器自动化 → `C:\Users\83611\EGO-LITE-SKILL.md`
   - 派 mdp / codearts → `C:\Users\83611\MDP-SKILL.md`
   - 计划任务 / 保活 → `C:\Users\83611\WATCHDOG-SKILL.md`
4. 项目进度以 `docs/ACCEPTANCE.md` 为唯一可信表，不要重复造清单。

## 1. 目标设定 / Objective

> 用户原话（长期有效）：
> **“功能与前端设计追平 V2RayN / Clash，做得比它们更干净。”**
>
> *Long-term goal: match V2RayN / Clash feature for feature and in front-end design, and be cleaner than both.*

拆开执行：

- **这是 UI 项目，不是内核项目。** Xray / sing-box 的协议栈与配置格式是上游的活；本项目只负责配置生成、进程生命周期、系统代理与界面。内核已有的能力不重复自研。
- **“追平”指日常面。** 节点、订阅、路由规则、策略组、规则集、连接、测速、托盘、运行中热切换等，用户能点到的都要有，且两个内核都要说得清差异。
- **“更干净”指信息层级、文案与失败提示。** 读不到的统计、探测不到的延迟，如实返回「测不到」并给原因，不许假装成功；不许留假开关。
- **验收标准是真机点出来。** 代码写完、单测全绿不等于做完；每条功能都要在云电脑上点出来、读配置文件或日志核对，结论写进 `docs/ACCEPTANCE.md`（验法 / 判据 / 设计理由 / 坑）。

## 2. 测试设备：cloudpc-test / Test device

**一切真机行为——启动内核、接管系统代理、点界面、验托盘——只在这台机器上做。**
*All real-machine testing happens on `cloudpc-test` only.*

| 项 | 值 |
|---|---|
| Tailscale 地址 | `100.106.121.0:22`（**仅 tailnet**，无公网入口） |
| 用户 | `Administrator` |
| 私钥 | 路径 `C:\Users\83611\.ssh\id_ed25519_cpc`（只记路径，永不写入内容） |
| 系统 | Windows Server 2022（10.0.20348.5761） |
| 主机名 | `0cadhjae-vm2vX2` |
| 默认 shell | **`cmd.exe`**，不是 bash |
| 开放端口 | 仅 TCP 22（tag `tag:cloudpc-test`，ACL 只放行这一个端口） |
| 性质 | 纯 Windows 测试机，无生产服务；关机后离线属正常 |

连接方式（本机必须用全路径 ssh）：

```bash
SSH="C:/Windows/System32/OpenSSH/ssh.exe"
KEY="-i C:/Users/83611/.ssh/id_ed25519_cpc -o StrictHostKeyChecking=no -o BatchMode=yes"
"$SSH" $KEY Administrator@100.106.121.0 "hostname & ver"
```

铁律 / Hard rules：

1. **本机绝不起内核、绝不写本机系统代理注册表。** *Never start a kernel or touch the system proxy registry on this dev machine.* 所有真机行为只在 cloudpc-test 上。
2. **远端是 cmd 语义。** 多命令用 `&` 串，禁用 `&&`、单引号、heredoc；不要传带引号的 `powershell -Command "..."`（cmd 会吃掉引号），要跑 PS 就上传 `.ps1` 并用 `-File`。
3. **不要混机。** `cloudpc-test`（`100.106.121.0`）≠ `cloudpc`（`100.94.243.28`，跑 MC 服务器，不要动）≠ `cloudpc-mc`（`100.79.50.56`，默认 PowerShell）。
4. **现成脚本在 `D:\tmp\shots\`**：`deploy-tray.sh` / `run-uia.sh` / `run-act.sh` / `run-ps1.sh` / `cleanup.sh`。验完必跑 `cleanup.sh`，并手动确认 `ProxyEnable=0`、`schtasks /query | findstr xm` 为空。
5. **不要问用户要地址、端口、密钥。** 有变化以 `C:\Users\83611\SSH.md` 为准；发现矛盾先停下问，不要自行猜测或“修正”。

## 3. 验收记录 / Acceptance record

真机结论写进 `docs/ACCEPTANCE.md`，新开一节按四件套写：**验法 / 判据 / 设计理由 / 坑**。它是进度的唯一真相源，不另建清单。

## 4. 分工与并发 / Roles and concurrency

主窗口（Lead）的上下文是稀缺资源，**只做四件事**：
*The main window only designs, decides, dispatches and accepts; the context is precious.*

1. **设计** —— 定方案、拆步骤、写清边界与验收标准。
2. **决策** —— 在多个做法之间拍板，并记录理由。
3. **派发** —— 把开发任务包成自包含的指令交给子代理或团队成员。
4. **验收** —— 读最终 diff、跑测试、在 cloudpc-test 上核对行为、写 `docs/ACCEPTANCE.md`。

**开发实现一律委派**，主窗口不长时间连续写代码。派发时至少给全：目标、验收标准、可改的文件范围、验证命令、禁区（不许碰的文件/配置）。
*Delegate all implementation to subagents or team members; the Lead never sits in a long coding loop.*

**并发上限：同时最多 1 个子代理运行（主窗口 + 子代理 = 2 个并发窗口）。** 其余任务排队，等这一个结束再派。
*At most 1 subagent at a time — the Lead plus one worker is the whole budget; queue the rest.*

2026-11-24 实测：两个子代理同时跑会连续触发平台 429（TPM/RPM 限流），整轮对话被打断；改为 1 个之后恢复。所以这条上限按「1 个」执行，别再加。

**子代理用一次就丢掉，不复用。** 每个子代理只承载一个任务：拿到任务包、做完、交回结果，然后结束；不要给它追加第二个任务，也不要「接着问它一句」。同一件事需要继续做，就开一个新的子代理并把当前状态重新讲清楚。
*One subagent = one task. Never hand a second task to the same subagent; start a fresh one and restate the state.*

理由：子代理的上下文和主窗口不共享，复用它意味着它带着上一次任务的假设继续做事；一旦它静默死亡，上一次和这一次的产出会一起丢，主窗口无从判断哪一步做了、哪一步没做。用一次一换，每次的输入和产出都是可核对的。

**派发前先减上下文。** 派发前如果主窗口已经很长，先压缩或分段，别把一整个长会话当成任务包塞进去；任务包只写做这件事真正需要的东西（目标、边界、验证命令、已知坑）。
*Compress or split the Lead context before dispatching; the task packet carries only what the job needs.*

已知本环境的子代理曾出现静默死亡、零产出，因此：
- 每个任务都要有**可核对的落点**（改哪个文件、跑哪条命令、期望输出什么）；
- 汇报只作参考，验收以工作区里的实际 diff 与测试结果为准；
- 子代理失败就重新派发或换成员，不要把"它说做完了"当成完成。

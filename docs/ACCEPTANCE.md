# 验收清单（真机）

按顺序做一遍，每一步都写了「应该看到什么」。做不到的那一条才是问题。

> 约束：本清单里的「真启动」部分会让内核真的联网。别在一台正跑着 V2RayN / Clash Verge 的机器上做，
> 两个客户端会抢系统代理。

## 0. 准备

- **WebView2 Runtime**：Windows 11 自带；Windows Server / LTSC 要装一次
  （`MicrosoftEdgeWebView2Setup.exe /silent /install`）。没有它程序起不来。
- **内核二进制**：把 `xray.exe` 或 `sing-box.exe` 放进 `%APPDATA%\com.xmatrix.client\engines\`。
  程序不内置内核，缺了启动按钮会禁用并在首页写明期望路径。
- 关掉其它代理客户端。

## 1. 启动与外观

| 做 | 应该看到 |
|---|---|
| 双击 `X-Matrix.exe` | 窗口出现，不白屏；节点为空时首页有三步引导卡 |
| 侧栏逐项点过去（首页/节点/路由/策略组/规则集/订阅/连接/设置/关于） | 每页标题一行、副标题不被挤断；工具栏按钮都有边框 |
| 切「深色」 | 界面变深，**标题栏也变深**；重启程序仍是深色 |
| 点窗口 × | 窗口隐藏、程序还在（托盘有图标） |
| 点托盘图标 | 窗口回来 |
| 再双击一次 exe | 不会出现第二个窗口 / 第二个托盘图标，而是把已有窗口唤到前面 |

## 2. 数据（不需要内核）

| 做 | 应该看到 |
|---|---|
| 节点 → 导入 → 粘贴一条 `vless://` 或 `ss://` | 节点出现在卡片列表，显示协议/地址 |
| 节点 → 测速 | 几秒后出现 `123 ms` 或「超时」；超时不算错误、不进错误条 |
| 节点 → 排序改为「延迟」 | 未测速的排在最后（不是排最前） |
| 规则集 → 新建（本地）→ 粘贴 `payload:` 列表 | 条目数正确，能编辑、能启停 |
| 路由 → 新增规则，匹配方式选「规则集」 | 下拉里只有启用的规则集；保存后表格显示集合名字 |
| 策略组 → 新建 `select` 组，勾两个成员 | 成员以 chip 显示，可点选当前出口 |
| 订阅 → 新增，填一个真实机场链接 → 刷新 | 节点数变成机场的节点数；再刷新一次不会翻倍（只替换自己那批） |
| 订阅 → 刷新一个带配额的机场 | 名称下面显示 `已用 / 总量` 与 `到期 日期`；provider 没给头则显示 `—` |
| 全部做完后**重启程序** | 节点 / 规则 / 策略组 / 规则集 / 订阅 / 设置都还在 |

## 3. 真启动（需要内核 + 网络）

| 做 | 应该看到 |
|---|---|
| 选一个节点 → 启动 | 首页状态变「运行中」，出现最近一分钟速率曲线 |
| 点「配置」 | 弹出渲染后的 JSON：Xray 有 `metrics` 段没有 `api` 段；sing-box 有 `experimental.clash_api` |
| 点「日志」 | 能看到内核自己的输出（不是空白） |
| 连接页 | sing-box：逐条连接（目标 / 网络 / 上传 / 下载 / 链路）；Xray：写明「该内核不提供连接列表」 |
| 停止 | 状态回「未运行」，曲线清空 |

## 4. 系统代理（真启动才有意义）

| 做 | 应该看到 |
|---|---|
| 设置里打开「接管系统代理」→ 启动内核 | Windows「设置 → 网络和 Internet → 代理」里手动代理指向 `127.0.0.1:<混合端口>` |
| 停止内核 | 代理设置逐字恢复成启动前的样子（原来没开就是没开） |
| 启动后**强杀进程**（任务管理器结束任务）→ 重新打开程序 | 启动时自动还原成原设置，不会把你留在「指向死端口」的状态 |

## 5. 明确不支持（不是 bug，别当问题报）

- **Xray 下策略组是静态副本**：Xray 没有 selector outbound，切成员等于改配置重启；sing-box 下才是内核原生 selector / urltest。
- **只有 `select` 和 `url-test`**，没有 `fallback`：两个内核都没有原生 fallback，做了就是按了没反应的开关。
- **Xray 没有连接列表接口**：它只暴露 expvar 的累计流量。
- **内核二进制不内置**，需要自己放。
- **界面语言只有中文 / English**。
- **Xray 的规则集必须内联**（没有 rule-provider 概念），所以集合很大时配置会变大；sing-box 才落成独立文件
  （`configs\rule-sets\<id>.json`，sing-box 的 source 格式）。

## 6. 已经实测过的（2026-11-22，Windows Server 2022 + WebView2 157 + 真内核）

这一节记的是「真跑过、有落盘或截图证据」的结论，用来区分「没验」和「验过没问题」。

| 项 | 结论 |
|---|---|
| 两个内核真启动 | Xray 26.11.9 / sing-box 1.14.4 都能被拉起、能生成配置、能被停止 |
| 系统代理往返 | 启动 → `ProxyEnable=1` + `ProxyServer=127.0.0.1:7890`；停止 → `ProxyEnable=0` 且 `ProxyServer` 值**被删回不存在**（与基线逐字一致） |
| 强杀 | 应用被 `taskkill /f` 后内核立即消失（作业对象 `KILL_ON_JOB_CLOSE`），代理留到下次启动自动还原 |
| 流量统计 | Xray `/debug/vars` 累计字节可读；sing-box Clash API 速率与连接明细可读；曲线能出真实尖峰 |
| 连接列表 | sing-box 逐条可见（目标/网络/规则/链路/上传/下载/开始时间）；Xray 页面如实写「该内核不提供连接列表」 |
| 订阅远程拉取 | 本地 http provider → 2 个节点入表，协议/传输/安全字段解析正确；再刷新只替换自己那批 |
| 规则集远程拉取 | `payload:` 列表 → 5 条入表（`IP-CIDR,...,no-resolve` 不会被当成 pattern） |
| 规则集命中（sing-box） | 集合内域名走 `direct`（200）、集合外走 `proxy`（假节点 502） |
| 规则集命中（Xray） | 同上：日志里 `[socks -> direct]` vs `[socks >> proxy]` |
| **真实节点隧道（Xray）** | 真机真节点（VLESS + REALITY + `xtls-rprx-vision` + tcp）：`curl -x 127.0.0.1:7890 https://ipinfo.io/json` 返回出口 `192.3.222.51`（Los Angeles / HostPapa），内核日志 `accepted //ipinfo.io:443 [socks >> proxy]`，`/debug/vars` 出真实字节（proxy 下行 14355 / 上行 10436） |
| **系统代理真上网** | 接管后不指定代理的 `Invoke-WebRequest https://api.ipify.org` 返回同一个出口 IP（192.3.222.51）→ Windows 层面的流量确实走了隧道；停止后注册表还原 |
| 真实节点隧道（sing-box） | **同一节点 sing-box 1.14.4 连不上**：TCP 通、握手被服务端重置。同一份字段在 Xray 下正常 → 见下方「互操作限制」 |

| **TLS 隧道（Xray，自签证书 + 证书指纹固定）** | 自建 vless+tls 服务端（自签 CN=local.test）：填 `pinnedPeerCertSha256` 后 `curl -x 127.0.0.1:7890 http://example.org/` = **200**，日志 `accepted … [socks >> proxy]` |
| **TLS 隧道（sing-box，自签证书 + 跳过校验）** | 同一服务端：打开「允许不安全证书」后 `http://example.org/` = **200** |
| 明文 ws 隧道（两内核） | 自建 vless+ws（无 TLS）服务端：Xray 200 / sing-box 200 |
| **订阅流量配额** | provider 回 `Subscription-Userinfo: upload=…; download=…; total=…; expire=…` → 订阅行的名称下面两行：`12.5 GB / 100 GB` 与 `到期 2026-03-01`，`store.json` 里 `traffic_used/traffic_total/expires_at_ms` 落盘；`total=0` 只显示已用、`expire=0` 不显示到期 |
| **节点搜索** | 输入 `reality` → 只剩 `Local-REALITY`；输入 `zzz` → 「没有匹配的节点」+「清空搜索」按钮可用 |
| **多选与批量条** | 「选择」进入选择模式 → 卡片出现勾选框、批量条显示 `已选 N`，「全选」一次选中当前筛选出的全部节点 |
| **批量测速** | 「测速选中」串行跑完两个节点 → 两张卡片都出现红色 `超时`（服务端已停，符合预期） |
| **复制分享链接** | 单条与多条都验过：点「复制链接」→ 绿色「已复制 N 条链接」，从**系统剪贴板**读回的内容是完整链接（`security=tls&sni=local.test&allowInsecure=1&pcs=<64 位十六进制>#Local-TLS`、`type=ws&path=%2Fws&security=none#Local-WS`）；4 秒后提示自动消失 |
| **批量删除** | 选中 1 个 → 确认框「将删除 1 个节点，引用它们的策略组会一并清理。」→ 确认后 `store.json` 里该节点消失、其余保留、界面退出选择模式 |
| **启动时自动连接** | `store.json` 里 `auto_connect=true` + `last_node_id` + `last_engine` → 重启程序后约 1.2 秒内核自己起来，首页显示「运行中 / 引擎 Xray / 节点 Local-WS」，`configs/xray.json` 里 `loglevel` 跟随设置（设成「调试」后是 `debug`） |
| **隧道连通（本地自建服务端）** | 本地起 vless+ws 服务端 → `curl -I -x http://127.0.0.1:7890 http://example.org/` = `HTTP/1.1 200 OK` |
| 设置页三个新控件 | 「启动时自动连接」为开、「内核日志级别」选中「调试」、「界面语言」中文选中；系统代理与开机自启显示真实状态（不是假开关） |
| **端口预检** | 先让 7890 被别的监听占住 → 点「启动」：首页立刻报「混合端口 7890 已被占用…先关掉它，或在设置里换一个端口」，引擎仍是「未运行」，**没有拉起 `xray.exe`**；释放端口后再点启动 → 「运行中」，控制区显示「入站端口 7890 / 控制端口 9990」 |
| **从剪贴板导入** | 剪贴板为空 → 对话框里直接提示「剪贴板是空的」；剪贴板是一行 vless 链接 → 输入框被填入并「识别 1 个节点」（`Clip-Test` / `127.0.0.1:18081` / vless）；剪贴板是一段无关长文（15 行）→ 「识别 0 个节点 · 15 行无法识别」并逐行说明「不支持的类型：…」，不会静默吃进去 |
| 托盘菜单 | **无法在计划任务会话里用脚本点开**（任务栏不接收该会话的鼠标事件，点「开始」也不弹菜单）→ 只做了代码审查，托盘三项与 tooltip 留给本机人工验收 |
| **首页四张统计卡** | 1292×1136 窗口（150% 缩放）下排成 2×2，第四张不再单独一行（`auto-fit` 留下的孤儿行已改成固定列数 + 断点） |
| **真实延迟** | 隧道起来后点「真实延迟」→ `真实延迟 83 ms`（经 Xray + vless+TLS 自签节点打 `generate_204`，含内核与节点 DNS） |
| **断开单连接** | sing-box 下挂一条长连接（`CONNECT example.org:443`）→ 连接页出现该行 + 「断开」按钮 → 点击后行消失，`curl http://127.0.0.1:9090/connections` 同时变回 `{"connections":[]}` |
| **配置导出** | 点「导出配置」→ 系统保存对话框（默认名 `x-matrix-<时间戳>`，类型「X-Matrix 配置」）→ 保存出 2,213 字节文件，首行 `"kind": "x-matrix-backup"`，`document.nodes` 是完整节点表 |
| **恢复的前置条件** | 内核运行中点「从备份恢复」→ 面板里直接回「先停止内核再恢复配置：正在跑的内核还是按旧配置渲染的」，不会弹对话框也不会写盘 |
| **会话流量清零** | 首页图表下方「本次会话」显示 `↓ 5.7 KB · ↑ 450 B`，点「清零」后变 `↓ 0 B · ↑ 0 B`，随后的采样继续从零累加（基线没被清掉，不会把内核总量当尖峰） |
| **连接页搜索** | 挂一条 `CONNECT example.org:443` 长连接 + 一条 `edge.microsoft.com` → 搜索框输入 `example` 只剩 `example.org` 一行；改成 `zzz` → 「没有匹配的连接」空状态。**注意：用中文输入法（IME）打字时 `v-model` 收不到组合中的字符，必须用剪贴板粘贴，否则会误判成「过滤不生效」** |
| **连接页排序** | 「排序」下拉四档（最新在前 / 下载最多 / 上传最多 / 目标名称），切换后行序随之改变，字节数缺失的行按 0 参与比较 |
| **自动测速间隔** | 设置页选「每 5 分钟」→ `store.json` 的 `auto_test_minutes=5`；手动测速进行中时自动那一轮会跳过（不叠加） |
| **代理例外（bypass）** | 输入框末尾追加 `;*.corp.test` → 保存后 `store.json` 的 `proxy_bypass` 含该值；重启（自动连接起内核）后注册表 `ProxyOverride` 逐字包含 `*.corp.test`，同时 `ProxyEnable=0x1`、`ProxyServer=127.0.0.1:7890` |
| **订阅节点去重** | 两个订阅指向同一份内容 → 第二个订阅刷新后 `imported=0`、提示里带「，跳过 2 个（重复或无法识别）」、节点列表没有翻倍；手工复制的同服务器节点**不会**被去重删掉（只有订阅自己带来的才比较） |
| **订阅 User-Agent** | 设置页填 `clash-verge/v1.7.0` → 保存后 `store.json` 落盘；本地服务端日志记到 `UA=clash-verge/v1.7.0`（默认是 `X-Matrix/版本号`） |
| **测速地址可配置** | 设置页把测速地址改成 `http://127.0.0.1:8898/sub-a.txt` → 点「真实延迟」后内核访问日志里的 URL 就是这个地址（不是 gstatic）；填 `https://…` 会被 `Settings::validate` 拒绝（探测走本机代理，不带 TLS） |
| **自定义 DNS（真机）** | 填 `1.1.1.1, 8.8.8.8` 后重启：Xray 的 `configs/xray.json` → `"dns":{"servers":["1.1.1.1","8.8.8.8"]}`，内核起得来；切 sing-box 后 `configs/sing-box.json` → `dns.servers[{type:"udp",tag:"dns-0",server:"1.1.1.1"}]` + `dns.final:"dns-0"` + `route.default_domain_resolver:"dns-0"`，日志 `sing-box started (0.07s)`、`clash-api listening at 127.0.0.1:9090`。**少了 `default_domain_resolver` 内核会 FATAL**（见「六个 bug」第 6 条），用 `sing-box check -c` 复验 exit=0 |
| **自定义 DNS** | 设置页填 `1.1.1.1, 8.8.8.8` → 保存 → 重启内核 → `configs/xray.json` 的 `dns.servers` 是这两条；切 sing-box 后 `configs/sing-box.json` 的 `dns.servers[0]` 是 `{"type":"udp","tag":"dns-0","server":"1.1.1.1"}`、`dns.final` 是 `dns-0`，内核能正常起来（格式错了内核会拒绝整份配置）。填域名会被拒绝 |
| **首页主操作在顶部** | 首页第一屏就是「启动控制」卡（节点 / 内核 / 启动·停止 / 配置 / 日志 / 真实延迟），统计卡与曲线在其下；1292×1136 窗口下四张统计卡排成 2×2 |
| 首启动引导 | 节点为空时首页出现三步引导卡（导入节点 / 打开内核目录 / 启动）；点「不再显示」后写 `localStorage['xm.ui.onboarding-dismissed']`，重启不再出现 |


### 互操作限制：REALITY 节点在 sing-box 下可能连不上（2026-11-22 实测）

同一个真实节点（VLESS + REALITY + `xtls-rprx-vision`）：

| 客户端 | 结果 |
|---|---|
| Xray 26.11.9 | 通，出口 `192.3.222.51` |
| sing-box 1.14.4 | `open connection to api.ipify.org:443 using outbound/vless[proxy]: read tcp …: wsarecv: An existing connection was forcibly closed by the remote host.` |

排查过的变体（都失败，说明不是某个字段写错）：去掉 `flow`、去掉 `utls`（sing-box 直接
`FATAL uTLS is required by reality client`，证明 utls 必须留）、`alpn: []`、`fingerprint: firefox`、
`fingerprint: random`。配置本身能被 sing-box 接受并启动监听，只是握手被服务端重置。

结论：这是 sing-box 对最新 Xray REALITY 服务端的兼容问题（该节点链接里带 `pqv=`，属于 Xray 26.x
的握手扩展）。产品侧的处理是**如实显示**：内核日志里就是这条错误，节点测速（TCP 握手）仍然会显示
低延迟——所以节点页现在明确写着「测速只量到节点的 TCP 握手，不代表隧道一定能通」。

### 真机上抓到的九个 bug（已修，都只有真机/真内核能暴露）

1. **sing-box 规则集文件格式错了**：写成了 `domain_suffix:example.org` 这样的行列表，
   而 `format: "source"` 要的是 JSON（`{"version":1,"rules":[...]}`）。sing-box 直接
   `FATAL parse rule-set[0]: invalid character 'd'` **拒绝启动**——一个 provider 文件能把整个内核搞死。
   现在按条目类型分组成 JSON；空集合两边一起跳过（声明和引用都不会出现）。
2. **带 UTF-8 BOM 的配置文件被当成损坏**：验收时用 PowerShell 的
   `Set-Content -Encoding UTF8`（PS 5.1 默认写 BOM）改了 `store.json`，程序读不动就把整份配置
   挪到 `store.corrupt` 并从默认值启动——节点、规则、设置全"消失"（数据没丢，但用户会以为丢了）。
   现在读盘先剥 BOM，并加了 `a_utf8_bom_does_not_count_as_corruption` 测试。记事本改配置是常见操作，
   这条必须容错。
3. **Xray 的域名后缀前缀写错了**：用了 `suffix:example.com`，而 Xray 的后缀匹配是 `domain:`；
   未知前缀会被当成普通关键字，于是**这条规则永远不命中**，流量静默走默认出站。
   单元测试当时也是照着错的前缀写的，所以 136 个测试全绿也没发现。

4. **Xray 26 移除了 `allowInsecure`，写了它整个配置都起不来**：为了支持自签证书，我照旧把
   `tlsSettings.allowInsecure: true` 写进去，Xray 26.11.9 直接
   `Failed to build TLS config: The feature "allowInsecure" has been removed and migrated to
   "pinnedPeerCertSha256"(pcs) and "verifyPeerCertByName"(vcn)` 并**拒绝启动**——不是这一个节点不通，
   是整份配置废掉。现在 Xray 侧一律不写这个键，改渲染 `pinnedPeerCertSha256`；`allowInsecure`
   只在 sing-box 侧生效（`tls.insecure`）。节点编辑器把这件事写在开关说明里。
5. **`pinnedPeerCertSha256` 是字符串不是数组**：写成 JSON 数组时 Xray 报
   `json: cannot unmarshal array into Go struct field … of type string`；改成字符串后又报
   `incorrect pinnedPeerCertSha256 length`（我第一次只填了前 16 位）。现在渲染时用逗号拼成字符串，
   并且 `Node::validate` 与节点编辑器都要求 64 位十六进制——**内核会因为一个截断的指纹拒绝启动整份配置**，
   这种错误必须挡在 UI 里。
| 连接页按目标汇总 | 真机 | 4 条活连接切成「按目标」：四行各 1 条连接，目标/连接数/上传/下载四列正确 |
| 连接页按出口汇总 | 真机 | 切「按出口」后 4 条收敛成 1 行 `proxy`（连接数 2，另 2 条已被断开），证明按链路第一跳聚合 |
| 启动时隐藏窗口 | 真机 | `start_minimized=true` 重启后桌面截图无窗口、进程存活（PID 8556） |
| 关闭窗口最小化到托盘 | 真机 | `close_to_tray=true` 点 × 后进程仍在（PID 3088），窗口消失 |
| 关闭窗口即退出 | 真机 | `close_to_tray=false` 点 × 后 `tasklist` 报「没有运行的任务匹配指定标准」 |
| DoH / DoT 解析器 | 真二进制校验 | `dns.servers = [1.1.1.1, tls://9.9.9.9, https://8.8.8.8/dns-query]` → `xray run -test` 输出 `Configuration OK.`；sing-box 侧 `type:tls`/`type:https` + `route.default_domain_resolver` → `sing-box check` 退出码 0 |

7. **分段控件没有选中态**：`class="active"` 与 CSS 的 `[aria-pressed="true"]` 不匹配，除设置页外所有分段控件都显示成"未选"。修法：CSS 两种都认 + 5 个视图补 `aria-pressed`。
8. **日志对话框读错内核的日志**：前端 `kind` 写死 `'xray'`，自动连接却按后端 `last_engine`（sing-box）启动 → 点「日志」看到的是 `xray.log`。修法：`refreshStatus()` 用后端 `status.kind` 校正前端选择。

9. **双内核只显示一个版本号**：`{ ...versions.value, [kind]: await … }` 的展开早于 await，两个并发探测互相覆盖（实测 xray 1072 ms / sing-box 1068 ms，后者总赢）→ 装了双内核时 Xray 的版本行消失。修法：先 await 再赋值 + 两条回归测试 + 切换内核时补探测。

| **速度测试（真机）** | 起内核后点「测速」→ 首页显示「下载速度 x MB/s」（5 秒采样、上限 8 MiB、只数正文）；隧道不通时给「测速地址没有返回任何内容」，不显示 0 |

| **从文件导入（真机）** | 通过。导入对话框「从文件读取」→ 选 `imp.txt`（一条 vless）→ 文本框回填并自动预览「识别 1 个节点」→ 导入后卡片出现 `from-file`（127.0.0.1:18082）。超过 4 MB 的文件被拒（单测覆盖，未在真机上造 4 MB 文件） |
| **把选中链接保存为文件（真机）** | 通过。默认名 `x-matrix-nodes-2026-11-22`（TXT 类型），保存后绿条「已保存到 C:\Users\Administrator\nodes.txt」，文件 118 字节、以 `vless://` 开头 |
| **路由规则试跑（真机）** | 通过。建 `test-suffix`（域名后缀 example.com→代理）与 `test-block`（域名后缀 blocked.test→拦截）后：`www.example.com` → 命中 test-suffix / 去向代理；`blocked.test` → 命中 test-block / 去向拦截；`nothing.org` → 「没有规则命中，走默认出口（代理）」。另加一条 IP 段 + 一条端口规则后，无命中时底部出现「有 1 条规则需要 IP 库、端口或进程名才能判断，已跳过」——IP 段没被计入是对的：域名输入下它确定不匹配，不算「判断不了」 |

| **节点标签显示订阅名（真机）** | 通过。订阅 `TestSub` 刷新后节点卡片标签显示「订阅：TestSub」；`store.json` 里 tags 仍是 `sub:sub-18ea…` —— 映射只发生在展示层，存储不变（改名不会打断引用） |
| **内核分段控件不横向位移（真机）** | 通过。像素测量：未运行 / 运行中两种状态下 sing-box 胶囊都是 x 1305–1413（中心 1359），Δx = 0 px |
| **速度测试（真机）** | 通过。本地 4 MB 资源 + vless+ws 隧道，点「测速」显示「下载速度 126.0 MB/s」，同时 `curl -x 127.0.0.1:7890` 拿到 4,194,304 字节（0.034s） |

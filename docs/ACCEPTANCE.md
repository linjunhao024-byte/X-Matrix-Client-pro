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

## 8. 首页代理模式切换（2026-11-23，云电脑 cloudpc-test）

分段控件「规则 / 全局 / 直连」是首页唯一会改配置又不显眼的东西：写错了不报错，只是所有流量悄悄走错方向。六项全部在真机上走完（点击 → 读 `store.json` → 读 `configs\xray.json` → 像素对比）：

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 点击落到存储 | 空闲态点「全局」 | **通过**。`store.json` 出现原文 `    "mode": "global",`；截图里高亮项从「规则」（x=1282）迁到「全局」（x=1362），采样点 `F1F1F2` 与 `F5F5F6` 互换 |
| 全局模式渲染 catch-all | 启动内核后看 `configs\xray.json` | **通过**。89 行文件，第 78–86 行是 `"routing"` → `"rules"` → `{ "network": "tcp,udp", "outboundTag": "proxy", "type": "field" }`，与约定一字不差（字段序无关） |
| 运行中锁定 | 内核运行中点「直连」 | **通过**。四面证据：`store.json` 仍是 `"mode": "global",`；全窗口像素采样差异仅 129 点、差异框仅 `(1416,353)-(1453,370)`（「直连」两字的 hover 变色，没有胶囊迁移）；x=1362 仍 `F1F1F2`、x=1434 仍 `F5F5F6`；特写 OCR 拿到「运行中不能切换代理模式」 |
| 直连模式渲染 | 停止 → 切「直连」→ 启动 | **通过**。同一条规则变成 `"outboundTag": "direct"`，其余字段不动 |
| 规则模式不留 catch-all | 停止 → 切回「规则」→ 启动 | **通过**。`findstr /c:"tcp,udp"` 无输出；`routing.rules` 是空数组（83 行文件，比全局少 6 行） |
| 纯函数兜底 | 单测锁死映射 | `engine::render::rule_tests::global_mode_is_one_catch_all_rule_to_the_proxy`（Xray 一条 catch-all → `proxy`，sing-box `{"outbound":"proxy"}`；「直连」指向 `direct`）与 `rule_mode_still_renders_the_users_table`（切模式不吃掉用户规则表） |

**没验**：托盘右键「退出 X-Matrix」这条退出路线。云电脑上图标落在通知区溢出区，UIA / 跨进程 `TB_GETITEMRECT` / 99 点 hover 全部拿不到名字，而溢出区里还可能是 Tailscale（其菜单含 `Exit`，盲点会切断唯一的 SSH 通路），所以放弃。设置里把「关闭窗口时最小化到托盘」关掉后点 × 走 `quit_cleanly()` 的路已验（见 §9）。

## 9. 窗口几何记忆 + 设置页固定页头（2026-11-23，云电脑 cloudpc-test）

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 几何写入 | 改窗口到 `200,90 1180x760` 后正常退出 | **通过**。数据目录多出 `.window-state.json`：`{"main":{"width":1158,"height":704,"x":200,"y":90,"prev_x":109,"prev_y":7,...}}`（1158×704 是客户区，加窗口装饰即 1180×760） |
| 几何还原 | `schtasks /run` 重启 | **通过**。重启后 `rect=200,90,1180x760`，与退出前完全一致；第二次退出/重启同样还原 |
| 正常退出的前提 | `close_to_tray: true` 时点 × 只是隐藏 | 记录：要触发写盘必须真退出（设置里关掉「关闭窗口时最小化到托盘」，点 × 走 `quit_cleanly()`），`tauri-plugin-window-state` 只在真退出时写文件 |
| 设置页固定页头 | 九个区块在 944 高下装不下，保存按钮必须常驻可见 | **改了两版**。第一版 `.page-header` 用 `position: sticky` + `--surface-glass-strong`（0.8 透明），滚动到中段时下面板子的文字从条上方透出来（OCR 在 y=74/75 读到「系统代理当前已开启…」），两层字叠一起；第二版把页头移出滚动容器、面板放进 `.page-body` 自己滚。实测滚到「代理例外 / 开机自启 / 内核日志级别」时页头标题仍在 y=109、内容从 y=212 起，**没有任何文字越过页头**，保存按钮全程在位 |
| 设置页全局快捷键面板 | 底部应有 `Ctrl+Shift+X` / `Ctrl+Shift+W` 说明 | **通过**。滚到底 OCR 到「全局快捷键」标题（y=1047）与两行 `<kbd>` |

**一条未复现的异常**：验收中途在设置页点「保存」，绿条「已保存」出现后进程消失、`store.json` 未落盘、没有 `.window-state.json`（不是正常退出路径），Application 日志里没有 APPCRASH 记录；同操作随后两次都正常。判断为环境可疑（同期 explorer.exe 崩过一次）而非必现 bug，但要查得先给应用加 panic hook / 崩溃日志——记在待办里，不在本轮修。

## 10. 全局快捷键 + 诊断包（2026-11-23，云电脑 cloudpc-test）

快捷键这条路换了实现：插件版（`tauri-plugin-global-shortcut`）在真机上**注册成功但不派发** —— 用别的进程抢注同一条 chord 拿到 1409（ERROR_HOTKEY_ALREADY_REGISTERED），证明它确实占住了；可注入按键后处理函数一次都没跑。根因是插件那个隐藏窗所在的线程不抽消息。现版自己起一条只做 `GetMessageW` 的线程、自己建隐藏窗（class `XMatrixHotkeys`）、自己 `RegisterHotKey`，注册失败只打日志（FAIL-OPEN）。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 快捷键已就绪 | 应用 stderr | **通过**。`x-matrix: 全局快捷键 Ctrl+Shift+X 已就绪` / `… Ctrl+Shift+W 已就绪`（`run-xm.bat` 把 `2>` 落到 `xm-err.txt`，GUI 进程的 `eprintln!` 有落处） |
| 隐藏窗存在 | 枚举顶层窗口 | **通过**。`hwnd=17826918 vis=False cls=XMatrixHotkeys pid=4760 proc=X-Matrix`，与主窗同进程同会话（session 1） |
| `Ctrl+Shift+X` 启停内核 | `keybd_event` 注入 Ctrl+Shift+X（不是 `SendKeys`：后台会话可能没有前台窗，`SendKeys` 无处可投） | **通过**。注入前 `xray=True`，5 秒后 `xray=False` —— 内核确实被停掉；再按一次同理可启 |
| `Ctrl+Shift+W` 显隐窗口 | 同上注入 Ctrl+Shift+W | **通过**。`visible=True` → `visible=False`（窗口消失、进程仍在）→ 再按一次 `visible=True` |

**诊断包**：关于页「导出诊断报告」→ 另存框 → 写出 **12,575 字节 / 239 行** markdown，`scp` 回来看内容：版本/时间（`2026-11-22 19:11:27 UTC`，与本地 03:11 +08 对得上）、内核表（`xray 33.2 MB Xray 26.11.9 (go1.27.3 windows/amd64)`、`sing-box 78.3 MB 1.14.4`）、四个目录、设置摊平、规模、引擎状态、系统代理、两段各 200 行日志、生成配置。**打码一开始有两个缺陷**：把分隔符和引号连值一起吃掉了，`"id": "abc"` 变成 `"id:***"`（非法 JSON），`"auth": "noauth"` 这类非机密字段也丢了值；现在只吃值、保留键与标点（`"id": "***"`、`password: ***`、`?token=***&x=1`），并有单测 `masking_keeps_the_punctuation_that_keeps_it_valid` 锁住；空节从悬空冒号改成 `—`（单测 `an_empty_row_reads_as_empty_not_as_a_failure`）。

## 11. 运行中热切换（节点 / 内核 / 代理模式）（2026-11-23，云电脑 cloudpc-test）

以前首页三个控件在内核运行时全部禁用（"停止再换"）。现在换成一个命令 `engine_apply`：后端在运行中先 `runtime.stop()` 再用同一套 `prepare_start` + `start_engine` 起新内核，**端口不变、系统代理不动**——代理在切换前后一直指向同一个混合端口，所以既不用 `disable()`（那会把流量短暂放直连，是漏洞，而且重写注册表比内核重绑端口更慢），也不会留下指向死端口的代理。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 空闲切模式 | 首页点「全局」 | **通过**。`store.json` 变 `"mode": "global"`，随后启动：`configs\xray.json` 出现 catch-all `"outboundTag": "proxy"`，xray PID 3876 |
| **运行中切模式** | 内核运行中点「直连」 | **通过**。同一秒内 xray PID 由 **3876 → 5488**（内核真被换掉了，不是假切换）；`configs\xray.json` 的同一条规则变 `"outboundTag": "direct"`；`store.json` 变 `"mode": "direct"`；`ProxyEnable` 前后都是 `0x1`（接管没有中断） |
| **运行中换内核** | 运行中点「sing-box」 | **通过**。xray 进程消失、`sing-box.exe` PID 4208 起来；`store.json` 的 `last_engine` 变 `"sing-box"`；首页 OCR 仍显示「运行中」，内核版本行变 `sing-box version 1.14.4` |
| **运行中换节点** | 运行中在下拉里选另一个节点（键盘 Down + Enter：原生下拉是独立窗口，截图抓不到，只能键盘） | **通过**。`last_node_id` 变 `vless-18ea6cbc39a2c758-0`；sing-box PID 由 4208 → **9432**；`configs\sing-box.json` 的 `server_port` 由 18081 变 **443**；首页节点行 OCR 到「节点：Clip-Test」；`ProxyEnable` 仍是 `0x1` |
| 界面提示 | 运行中三个控件都带一行说明 | **通过**。OCR 读到「切换会立刻重启内核，连接会短暂中断」与「运行中切换会重启内核，连接会短暂中断」，不再出现「运行中不能切换」 |

**为什么不保留"运行中禁用"**：Clash / V2RayN 都在运行中换节点，用户换节点的动机通常正是当前节点慢。代价是连接会断一秒（新进程 = 新 socket），这一点写在控件下面，不藏在文档里。

**顺带修掉的一个日志页 bug**：`LogsView.vue` 的 reset 分支原来先 `cursor.value = 0` 再 return，日志被截断（内核重启 / 点清空）后如果那一轮读到的正文为空，游标停在旧的大值上，于是每轮都判定 reset、每轮都读不到东西，页面会一直空着直到文件长过旧长度。现在 reset 时先把游标对齐到 `delta.size` 再决定要不要 return。

## 12. hysteria2 / tuic（2026-11-23，云电脑 cloudpc-test）

这两种协议在 Clash 与 v2rayN 的订阅里很常见（QUIC 系，穿墙比 TCP 稳），原来 `import.rs` 认不出来直接丢掉。现在分享链接、Clash YAML、sing-box JSON、导出链接四条路都认，sing-box 内核原生跑，Xray 内核**明确拒绝**而不是悄悄把流量放直连。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 从分享链接导入 | 在导入对话框里填 `hysteria2://testpass@51.79.100.10:8443?sni=hy.example.com&obfs=salamander&obfs-password=salt123&upmbps=30&downmbps=120#HY2-Test`，点「解析预览」再点「导入」 | **通过**。预览行显示 `HY2-Test / 51.79.100.10:8443 / Hysteria2`；`store.json` 里出现 `HY2-Test \| Hysteria2 \| 51.79.100.10:8443 \| security=Tls obfs=salt123 up=30 down=120`，`sni` 是 `hy.example.com` |
| 用 sing-box 跑起来 | 内核切 sing-box，启动这个节点 | **通过**。`configs\sing-box.json` 里是 `"type": "hysteria2"`、`"server_port": 8443`、`"obfs": {…}`、`"up_mbps": 30`、`"down_mbps": 120`；`sing-box.exe` PID 7724 在跑，界面显示运行中 |
| 用 Xray 启动必须被拒 | 内核切 Xray，再点这张卡的「启动」 | **通过**。页面顶部出现「Xray 内核不支持 Hysteria2 协议，请在首页把内核切换为 sing-box 后再启动」，内核没有被拉起来 |

**为什么 Xray 侧要报错而不是渲染**：Xray 没有 hysteria2/tuic 的出站类型，硬渲染只能是「丢节点」或者「把流量全直连」——两种都比报错更坏。同理，sing-box 侧 hysteria2/tuic 一定带 `tls.enabled: true`（分享链接常常一个字都不提 TLS，而 sing-box 会直接拒绝没有 TLS 的 QUIC 出站），`up_mbps`/`down_mbps` 为 0 时不写入（0 会被内核理解成没有带宽）。

**这段验收踩到的两个坑（工具层面，不是产品缺陷）**：
- 像素点击在这里不可信：PowerShell 进程是 DPI-unaware，UIA 报出来的 BoundingRectangle 是虚拟化过的；`x-act.ps1` 的 `focus` 原来用 `ShowWindow($h, 9)`（= SW_RESTORE，会把最大化的窗口**还原**），一 focus 整个布局就平移。现在 `focus` 改成 `ShowWindow($h, 3)`（SW_MAXIMIZE），并且按按钮一律走 UI Automation 的 `InvokePattern` / `SelectionItemPattern` / `TogglePattern`（`uia2.ps1`），不再按像素。
- 远端 OCR 结果必须 `scp` 回来读：走 `type` + ssh 会把 UTF-8 中文打散成 `�?`，关键字 grep 会漏判（我就是这样先误判成"没有报错"）。

## 13. 托盘菜单（2026-11-23，云电脑 cloudpc-test）

托盘原来只有「显示 / 启停 / 退出」三项，`TrayState` 把文案在第一次构建时固化成字符串，所以内核从别处起来时菜单还写着「启动内核」。现在菜单在每次状态变化后整块重建：三个固定项 + 「节点」子菜单（最多 20 条，当前节点打勾，空时给一条禁用的「还没有节点」）+ 「代理模式」子菜单（规则 / 全局 / 直连，当前项打勾）。点一行就是一次 `engine_apply`：内核没跑就只改设置，跑着就先停再起，系统代理全程不动。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 菜单随真实状态重建 | 启动日志 | **通过**。`x-matrix: 托盘菜单 3 个节点 · 模式 direct · 内核未运行` →（自动连接）→ `x-matrix: 托盘菜单 3 个节点 · 模式 direct · 内核运行中`，两行都由 `tray::build` 自己打出，重建没有失败日志 |
| 自动连接后文案要跟上 | 同上第二行 | **通过（同时修掉一个缺陷）**。`auto_connect` 原来直接 `start_engine` 不重建菜单，自动连接后托盘仍写「启动内核」；现在成功分支补 `refresh_tray(&app)` |
| 勾选状态与写入 | 单测 | **通过**。5 条：当前节点打勾、没有选中时不打勾、超过 20 条截断、模式只勾一个、`slug` 往返 |
| 点菜单里的托盘 | 真机点击 | **未验**。见下 |

**为什么托盘点击没验成**：图标落在通知区的溢出区。`traydump.ps1` 枚举桌面（110 个 UIA 元素）只看到 `Shell_TrayWnd`、`TrayNotifyWnd`、`ToolbarWindow32`（用户提示通知区域，1056,760 72x40）、`TrayClockWClass`、`TrayButton`、`TrayShowDesktopButtonWClass`——我们的图标不在其中任何可寻址元素里，`traymenu.ps1` 按 UIA 找到的 `X-Matrix` 是主窗口本身（`73,5 1135x757 class=Tauri Window`），右键后 `FindWindow('#32768')` 返回 0。这个会话里点不出托盘菜单，所以「点某一行会重启内核」只有代码路径与 `engine_apply` 的既有验收支撑（§11 已在真机验过运行中换节点/模式），没有本节的像素证据。

**为了能验，加了一条日志**：`tray::build` 每次构建都打一行 `托盘菜单 N 个节点 · 模式 X · 内核是否运行`。托盘菜单无法用测试断言，"那一刻托盘以为自己在显示什么"是出问题时第一个要问的问题，而它只有日志能回答。

## 14. 从 Clash 配置导入：节点、策略组、规则一起进来（2026-11-23，云电脑 cloudpc-test）

原来 `documents.rs` 读到 `proxies` 就收工，`proxy-groups` 和 `rules` 被整段忽略——用户从 Clash 搬过来，节点在、规则没了，所有流量都走同一个节点，而界面上没有任何提示。现在三样一起导入，认得的是 `select` / `url-test`，能翻译的规则见下表。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 预览要说清会来什么 | 导入对话框粘一段带 groups/rules 的 Clash YAML，点「解析预览」 | **通过**。OCR 读到「识别 2 个节点」「含 2 个策略组」「含 4 条规则」「3 行无法识别」，预览行是 `Clash-One / 51.79.100.11:8388 / Shadowsocks` 与 `Clash-Two / 51.79.100.12:443` |
| 导入后落库 | 点「导入」，读远端 `store.json` | **通过**。`nodes: 5`（新增 `Clash-One | Shadowsocks`、`Clash-Two | Trojan`）、`groups: 2`（`manual | select`、`auto | url-test`，成员都是 `clash-*` 的节点 id）、`rules: 4`（`域名后缀 github.com → group`、`域名关键词 google → group`、`地区 cn → direct`、`端口 22 → direct`，全部 `enabled=True`） |
| 界面确认 | 导入后的 toast | **通过**。OCR 读到「已导入 2 个节点 · 含 2 个策略组 · 含 4 条规则 · 4 行无法识别」 |

**映射表（`documents.rs::clash_rule`）**：`DOMAIN-SUFFIX` / `DOMAIN-KEYWORD` / `DOMAIN`·`DOMAIN-FULL` / `IP-CIDR` / `GEOIP` / `DST-PORT` / `PROCESS-NAME` 一一对应；`MATCH`（兜底）与 `RULE-SET` 记进「无法识别」，不猜。`clash_group` 里 `select` / `url-test` 原样保留，`fallback` / `load-balance` / `relay` 降级成 `url-test` **并在预览里说明**——两个内核都没有这种出站，静默改写会让人以为配置坏了。

**两条不覆盖原则**（`commands/import.rs`）：
- 同名策略组已存在就跳过，不覆盖用户手工调过的成员。
- **用户已经有规则时，导入的规则一条都不写**：规则自顶向下求值，把别人的表追加到你排好的兜底之后，等于一条都没生效。宁可整批拒绝并说明。

**成员里的 `DIRECT` / `REJECT`**：Clash 允许策略组把 `DIRECT` 当成员，我们是"组只能含节点"，所以它进不来。这一条**明说**在预览的无法识别里（`策略组「manual」的 1 个成员没能导入（DIRECT），已略过`），不静默缩短成员列表——组悄悄少了成员，用户只会觉得"选了直连却还在走代理"。

**这段验收踩到的坑（工具层面，不是产品缺陷）**：
- `x-act.ps1` 的 `paste` 原来走 `cmd /c type file | clip`，cmd 用 OEM 代码页（936）读 UTF-8 文件，中文全变 `鑷女年娴` 这种乱码；乱码让 YAML 解析失败，应用于是回落成"逐行分享链接"，报「识别 0 个节点 / 24 行无法识别」。现在用 `[IO.File]::ReadAllText($file, UTF8)` + `Set-Clipboard`，并且粘贴前先 `Ctrl+A` + `Delete` 清空输入框（否则新旧两段文本拼在一起，同样解析不了）。
- `Set-Clipboard` 偶尔抛「所请求的剪贴板操作失败」，但剪贴板里已经是上一次写对的 UTF-8 内容，粘贴仍然成功——`paste` 幂等重试即可，不要当失败处理。
- 同名按钮有两个（工具栏「导入」与对话框「导入」），UIA 按名字取第一个会点到工具栏那个；`uia2.ps1` 加了 `invoke_last|导入` 才落到对话框里。

## 15. 订阅走代理拉取：开关打开时确实从内核出站（2026-11-23，云电脑 cloudpc-test）

有些机场的面板地址只在那条隧道里能打开——用直连永远拉不到订阅。设置里新增「订阅走代理拉取」，打开后拉取走本机混合端口的 `HTTP CONNECT`。

验法：云电脑上起了一个本地 HTTP 服务（`127.0.0.1:8899`，`subserver.ps1`）当作订阅源，它把每个请求的**来源端口**写进日志；再拿 Xray 自己的 `logs\xray.log` 对照——`accepted http://127.0.0.1:8899/...` 出现，就证明这次拉取是从内核出去的。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 关着的时候走直连 | 开关关、内核在跑，点刷新 | **通过**。`subserver.log`：`07:00:34 hit from 127.0.0.1:8925`，xray 日志里没有任何 `8899` 的记录——端口 8925 是 reqwest 自己的套接字 |
| 开着时从内核走 | 开关开（`store.json` 的 `subscription_via_proxy: true`）、内核在跑（`xray.exe` PID 7864，`127.0.0.1:7890 LISTENING`），点刷新 | **通过**。`subserver.log`：`07:05:01 hit from 127.0.0.1:9281`；xray 日志同一毫秒：`from 127.0.0.1:9280 accepted http://127.0.0.1:8899/sub.txt [socks -> direct]`、`proxy/freedom: connection opened to tcp:127.0.0.1:8899, local endpoint 127.0.0.1:9281`。来源端口 9281 正是内核出站的那一端，与直连的 8925 不同 |
| 开着但没有内核要说话 | 开关开、点首页「停止」后点刷新 | **通过**。`subserver.log`：`07:07:57 hit from 127.0.0.1:9457`（仍能拉，走直连兜底）；界面 OCR 读到「订阅设置为走代理，但内核没有运行，这一次是直连拉的」 |
| 开关打开时不能污染表格 | 上述三次刷新后的界面 | **通过**。状态列仍是「正常」，`[Test-Sub] 更新完成：1 个节点（替换 1 个）` 正常出现；路线提示单独一条 `alert`，不计进「跳过 N 行」 |

**两个刻意的设计**：
- 默认 `false`。走隧道意味着节点坏掉的时候订阅也刷新不了，而"节点坏了"恰恰是最需要刷新订阅的时刻。
- 没有内核时**不静默改走直连**，而是照拉、并明说这一趟是直连。悄悄换路线正是让人不再信任客户端的开始。

`RefreshOutcome` 为此多了一个 `note: Option<String>`：`skipped` 计的是"解析不了的节点行"，路线提示不是一行，混在一起两个都读不懂。`note` 单独渲染成页面上的一条提示带。

## 16. 分组 / 节点的真实延迟（2026-11-23，云电脑 cloudpc-test）

节点卡上的「测速」一直是本机对 `server:port` 做一次 TCP 握手，三秒不通就算超时。那个数字回答的是"这个端口有没有人应答"，与"把请求从节点递出去要多久"是两件事——节点在墙外拒绝握手、端口通但隧道坏了，都会被读成前者。

现在 sing-box 内核在跑时，改问内核自己：`GET /proxies/{tag}/delay`，`tag` 是活动节点 `proxy`、组成员 `node:<id>`、策略组 `group:<id>`。数字是内核把一次真实请求从那条出站递出去再回来所需的毫秒数。

验法：云电脑上起本地测试服务器（`xray.exe run -c srv-lite2.json`，`127.0.0.1:18080` Reality / `18081` WebSocket），探测地址写 `http://127.0.0.1:8898/sub-a.txt`（8898 是一个裸 TCP 的应答器，把每次命中连同行号写进 `probe.log`）；组页 dump 出每行按钮的 `DISABLED` 与 `help`。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 组延迟出数字 | 内核跑 sing-box、`Local-Test` 组（`url-test`，成员 `Local-WS`）点「测速」 | **通过**。延迟列由「未测速 Local-WS」变 **`154 ms Local-WS 105ms`**：组 154 ms、成员 105 ms。同一行的按钮在测速前 `help=让内核给这个策略组与它的每个成员各发一次请求`、无 `DISABLED` |
| 不可达的组如实报错 | `auto` 组（成员是两枚不可达的 `51.79.100.11/12`）点「测速」 | **通过**。延迟列显示「测不到 Clash-One Clash-Two」，`title` 里是内核原话（`dial tcp 51.79.100.12:443: i/o timeout` 之类），不是我们编的文案 |
| 没有内核时不撒谎 | 点首页「停止」，再看组页 | **通过**。三个按钮全部 `DISABLED help=先启动内核才能测真实延迟` |
| Xray 时不假装能测 | 内核切 Xray 并启动 | **通过**。三个按钮 `DISABLED help=分组延迟要 sing-box 内核在运行；Xray 没有 Clash API` |

**三个刻意的设计**：
- Xray 侧不做。它没有 Clash API，唯一能做的还是本地握手——把"握手数字"改个名字冒充"真实延迟"就是在撒谎。
- 未运行时的提示分两态。先说"先启动内核"，再说"这个内核测不了"；只给后一句会把人往内核开关上引，而那个开关本来就是关着的。
- 不把"用户填的测试地址"当成会被采用。见下面的坑二。

**三个坑（都是真机撞出来的）**：
1. **测试服务器换过 uuid，隧道一直没通。** `srv-access.log` 满屏 `rejected proxy/vless/encoding: invalid request user id: 11111111-1111-1111-1111-111111111111`，而 `srv-lite2.json` 里是 `…-2222-3333-4444-…`。客户端节点没跟着改，于是"内核给了个数"看起来像成功——直到换成一致的 id，`154 ms` 才对得上。教训：验收隧道时，先看服务端有没有 `accepted`。
2. **sing-box 1.14 忽略 Clash API 的 `url` 形参。** 拿 `…/delay?url=http://127.0.0.1:1/nothing`（必死地址）去问，仍回 `{"delay":109}`；`group:…` 回 95、`proxy` 回 99。它用的是出站自带的探测地址（urltest 默认 `generate_204`）。`stats.rs::clash_delay` 照发 `url`（别的内核认），但注释写明**不许在这上面建"用户的测试地址会被采用"的功能假设**。
3. **探测源对 `HEAD` 返回 body，连接被关。** sing-box 的 URLTest 发的是 `HEAD`；原来那个 `HttpListener`（http.sys）对 HEAD 也写 body，`http.sys` 于是断连，内核只看到 `Head "…": EOF`。换成裸 TCP 应答器（任何方法都回 `200` + 空体）才通。这一条值得记住：**"探测失败"先怀疑探测端，再怀疑被测链路**。

## 17. 连接页：一键断开全部，以及每条连接是哪个程序（2026-11-23，云电脑 cloudpc-test）

连接列表原来只能一条一条断开，而且只能看到目标域名——"这 6 个连接是同一个下载器开的"和"有人在我机器上偷偷上传"在页面上长得一模一样。现在工具栏多一个「全部断开」，每条连接的域名下面多一行**发起它的程序**。

验法：云电脑上 app 跑着 sing-box（`127.0.0.1:7890` 混合入站、`9090` Clash API），用 `curl.exe -x http://127.0.0.1:7890` 起三个对 `speed.cloudflare.com` 的长下载，制造真实连接；连接页用 UIA dump 读 `DataItem` 与 `Button`，Clash API 用 `curl http://127.0.0.1:9090/connections` 直接对照。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 进程归属真的显示出来 | 三条 curl 下载在跑时看连接页 | **通过**。行内 `DataItem 'speed.cloudflare.com curl.exe' @349,402`：域名下面就是 `curl.exe`，完整路径在 `title` 里。对照 Clash API：`metadata.processPath = 'C:\Windows\System32\curl.exe'` |
| 「全部断开」在位且说明白 | 连接页工具栏 | **通过**。`Button '全部断开' @1085,229 69x28 help=断开当前所有连接；正在下载的会立刻中断` |
| 点了要先问一句 | 点「全部断开」 | **通过**。弹出 `Text '断开全部连接'`、`Text '现在有 4 条连接。断开后正在进行的下载与上传会立刻中断。'`、`Button '取消'`、`Button '确认'` |
| 确认后真的全断 | 点「确认」 | **通过**。`/connections` 由 4 条变 `{"connections":[]}`（`downloadTotal` 仍记着 10,587,410 字节，累计量不清零）；页面同时变空态：`Button '全部断开' … DISABLED`、`Text '引擎在运行，但当前没有活跃连接'`、`Text '…只是这一秒没有流量转发。随便打开一个网站，连接就会出现在这里。'` |

**三个刻意的设计**：
- **不新增第九列**。1180 px 窗口下表格可视宽只有 866 px，再插一列会把操作列挤出卡片；程序名作为域名下方的次要行，宽度不变。
- **空列表走 `DELETE /connections`，不逐个 id 扫**。内核才是"现在有哪些连接"的唯一权威；拿两次轮询之间读到的 id 列表去逐个删，会漏掉这期间新开的连接，而"全部断开"漏掉一条就失去了意义。
- **程序名只显示 basename，完整路径进 `title`**。`C:\Program Files\…\firefox.exe` 会把一行撑到两行，而识别程序靠的是 `firefox.exe`。

**两个坑**：
1. **`metadata.processPath` 默认是空的。** sing-box 只有在 `route.find_process = true` 时才去查发起连接的程序；我们之前的配置没开，所以这一栏永远是空字符串。现已写进渲染（`src-tauri/src/engine/singbox.rs`，注释说明代价是每条新连接一次系统调用），并有断言守着。
2. **别用 `grep` 二进制来判断代码有没有编进去。** 改完代码后 `grep -c find_process target/release/x-matrix.exe` 一直是 0，`strings` 也找不到，我一度怀疑构建没生效——实际上 rustc 把这种短字符串用 `movabs` 立即数拆成两条指令塞进代码段（`H\xb9find_pro` + `cess`），文件里根本不存在连续的字节序列。**判断构建是否生效只能跑起来看行为**（这次是直接问 Clash API 的 `processPath`）。

## 18. 测速预算：超时、并发与「选最快」（2026-11-23，云电脑 cloudpc-test）

原来「全部测速」是前端一个 `for` 循环，超时写死 3 秒。十几个节点排着队各等 3 秒，一个坏节点就能把整轮拖到半分钟；而且没有任何办法把"选最快的那个"变成一步操作。现在这两个数是设置项，并且多一个「选最快」按钮。

验法：云电脑上把 6 个真节点 + 4 个假节点（`51.79.100.2x:443`，与 `Clash-One` 同形状）放进 store，`test_timeout_ms = 1000`，分别用 `test_concurrency = 1` 和 `6` 重启应用，用 `D:\tmp\shots\measure-all.ps1` 走 UIA 按「全部测速」并用 `Stopwatch` 记到按钮重新可用为止。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 并发真的影响墙钟 | 同一批 10 节点、同一超时，只改 `test_concurrency` | **通过**。并发 1 → `enabled again=True after 6793 ms`；并发 6 → `1639 ms` |
| 批量结果逐节点如实作答 | 延迟列文本 | **通过**。两轮一致：3 个数字（`165/286/264 ms`）、1 个 `走 QUIC`（HY2-Test）、5 个 `超时`；`走 QUIC` 是逐条错误而不是整批失败 |
| 「选最快」真的切过去 | 按下前后对比 store 与内核 | **通过**。按下前 `last_node_id = local-ws`，按下后 `vless-18ea6cbc39a2c758-0`（= `Clip-Test`，165 ms 为最小），`sing-box.exe` 重启为新 PID；卡片上 `Clip-Test` 出 `165 ms` + `运行中`（按钮 `DISABLED`），`Local-WS` 退回 `超时` + `启动` |
| 两个设置项在真机上可见可读 | 设置页 UIA | **通过**。`Spinner '握手超时（毫秒）'`、`Spinner '同时测几个'`，读回值 `1000` / `6`，与 store 一致 |

**为什么默认并发是 2，而不是"越快越好"**：
- 并行 TCP 握手抢的是同一条上行。节点多的时候并发 16 得到的数字是队列延迟，不是节点延迟——**把 8 个都标成 400 ms 比标成"未测"更糟**。
- 但纯串行在几十个节点上又慢到没人愿意点。默认 2 是"两个数字大致还是各自的路"，同时把一整批的墙钟砍掉一半以上（实测 6793 → 1639 ms 是 6 并发的效果）。
- 所以这个旋钮交给用户，界面上写明换算关系（`settings.testBudgetHint`），而不是让代码替所有人选一个。

**三个刻意的设计**：
- **批量走后端一条命令，前端不再自己循环**。并发、超时、顺序都在 `probe_all` 里一次决定：`Mutex<Vec<Option<LatencyProbe>>>` + `AtomicUsize` 抢下标 + `std::thread::scope`，**结果槽按入参下标写回**，所以返回顺序永远等于请求顺序（有测试守着）；否则前端拿到一个乱序数组，还得按下标重新对齐，节点一多就是 bug。
- **`LatencyProbe` 里失败是数据，不是错误**。`{ id, ms: Option<u32>, error: Option<String> }`：一个节点超时不该让整批命令返回 `Err`，也不该从结果里消失（消失会让界面保留上一次的旧数字）。
- **「选最快」复用已有的两条路径**：内核在跑就 `engine.apply()`（先停再起、系统代理不动），没跑就只写 `last_node_id`。没有为它新开一条"静默启动内核"的路——那会让一个测速按钮变成第二个启动按钮。

**两个坑**：
1. **云电脑上的 PowerShell 脚本必须存成 UTF-8 with BOM。** 无 BOM 的 UTF-8 会被 PS 5.1 按 ANSI 解析，脚本里只要有一个中文字符串（这次是正则 `'ms$|未测速|超时|QUIC|走 QUIC'`），整个文件解析失败并报 `字符串缺少终止符`，而 `-File` 启动时这个错误只落在任务的 stderr 里，很容易被当成"脚本没跑"。
2. **ssh 回来的是被打散的中文**（`选最�`、`策略�`）。用 `grep 中文` 过滤远端 dump 会假阴性——要么用 ASCII 模式（`ms$`、`QUIC`、`@8\d\d,2\d\d`），要么把输出 `scp` 回本地再读。

## 19. 流量历史：每分钟一条，重启还在

验法：云电脑上把 `settings.mode` 设成 `global`、`last_node_id = local-ws`（连本地测试服务器），启动内核后用 `D:\tmp\shots\n2-traffic.ps1` 起一个后台下载（`curl -x http://127.0.0.1:7890 http://speed.cloudflare.com/__down?bytes=80000000`）跑满两分钟；随后 `taskkill /F /IM X-Matrix.exe` + `schtasks /run /tn xmrun` 重启，用 `run-uia.sh` 的 `dump` 读首页面板，再用 `invoke` 切窗口与清空。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 每一分钟真的落到磁盘 | `%APPDATA%\com.xmatrix.client\traffic.jsonl` | **通过**。两分钟下载期间写入 5 行：`{"minute":29923311,"up":1147,"down":1006000}`、`29923312 → down 7190524`、`29923313 → 5515418`、`29923314 → 3612866`、`29923319 → 10254`；同一时刻内核报 `downloadTotal=15742342`，与逐分钟之和一致 |
| 重启后历史还在、面板出柱 | 重启后的首页 UIA | **通过**。`Image '流量历史' @289,1019 915x65`（柱子容器）+ `Text '这段时间 ↓16.5 MB · ↑19.7 KB'`，重启前那两分钟的流量一条没丢 |
| 窗口切换真的改变查询范围 | 1 小时 vs 24 小时 | **通过**。往文件里塞一条 2 小时前、`down = 800000000` 的点并重启：`invoke '1 小时'` → `↓16.6 MB`；`invoke '24 小时'` → `↓779.5 MB`（762.9 MiB + 16.6 MB ≈ 779.5，正好把那条含进来） |
| 清空能同时清内存与磁盘 | 点「清空」后 | **通过**。`invoke '清空'` → 按钮变 `DISABLED`、正文变「还没有历史：在后台多跑一分钟，这里就会出现一根柱子。」、`traffic.jsonl` 的 `size=0` |

**为什么是 append-only 的 `traffic.jsonl`，而不是塞进 `store.json`**：
- `store.json` 每次编辑都整份重写。分钟级历史进去以后，改一个节点名字都要重写几千行；而且它和"配置"共享同一个损坏面。
- 一行一分钟的纯文本可以直接 `type` 出来看、可以 `tail`、用户贴 bug 报告时能一眼对上。历史坏一行不该连累配置，配置坏一处也不该让历史消失。

**三个刻意的设计**：
- **记录来自实时样本，没有第二个定时器。** `traffic_sample` 每次取完内核计数就顺手 `observe()`，所以"用户看到的图"和"存下来的历史"是同一批数字；单独开一个定时器，就会在最小化、休眠、漏拍时和图表对不上。
- **计数器倒退只重设基线，不记负值。** 换内核（Xray ↔ sing-box）或内核重启时累计量从头开始，`saturating_sub` 把那次跳变当成 0，而不是"一分钟里减了 3 GB"。
- **空分钟不写点。** 没流量的时间应该有"没有行"，而不是四十行 0 —— 这样 24 小时窗口里的柱子数就是"真的有过流量的分钟数"，不会为静默的夜晚画出一条平线。文件超过 `KEEP_MINUTES`（7 天）时整份重写，临时文件 + `rename` 原子替换，和 store 用同一套纪律。

**两个坑**：
1. **`minute` 必须是 `unix_seconds.div_euclid(60)`，不能用本地时间。** 本地的一分钟不一定等于 60 秒（夏令时），用户跨时区时历史还会错位。
2. **UIA 的矩形不能直接当像素点。** 本轮 `click 1110 988` 点在「24 小时」按钮的 UIA 矩形中心上，界面毫无反应；换成 `invoke|24 小时`（`InvokePattern`）立刻生效。WebView2 的矩形是逻辑坐标，注入的鼠标事件是物理像素，两者在缩放会话里不等价——**能 `invoke` 就不要点坐标**。

## 20. 更多匹配器：域名正则与域名分类

验法：云电脑上用 `D:/tmp/shots/paste-n9.txt`（一个 ss 节点 `N9-One`、一个 `select` 组 `n9group`、三条规则 `DOMAIN-REGEX,^ads?\d+\.,REJECT` / `GEOSITE,category-ads-all,REJECT` / `DOMAIN-SUFFIX,example.com,n9group`）走完整导入流程（`run-uia.sh` 的 `invoke|节点` → `invoke|导入` → act 的 `paste n9` → `invoke|解析预览` → `invoke_last|导入`），再打开路由页 `dump`，最后切换两个内核分别启动，直接读渲染出来的 `configs\xray.json` 与 `configs\sing-box.json`。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 导入时三种匹配器都留下来 | 远端 `store.json` 的 `rules` | **通过**。清空后重新导入得 `rules=3`：`domain_regex = ^ads?\d+\. -> block`、`geo_site = category-ads-all -> block`、`domain_suffix = example.com -> group` |
| 路由页把两类新匹配器都画出来 | 路由页 UIA `dump` | **通过**。三行的「匹配」列分别是「域名正则」「域名分类」「域名后缀」，内容列 `^ads?\d+\.` / `category-ads-all` / `example.com`，第三行动作是「走策略组「n9group」」 |
| Xray 内核真的收到了 | `configs\xray.json` 的 `routing.rules` | **通过**。`{"domain":["regexp:^ads?\\d+\\."],"outboundTag":"block"}`、`{"domain":["geosite:category-ads-all"],"outboundTag":"block"}` —— 两种都进同一个 `domain` 数组，靠 `regexp:` / `geosite:` 前缀区分 |
| sing-box 内核真的收到了 | `configs\sing-box.json` 的 `route.rules` | **通过**。`{"action":"reject","domain_regex":["^ads?\\d+\\."]}`、`{"action":"reject","geosite":["category-ads-all"]}` —— 一种匹配器一个键，不塞进 `domain` |

**为什么先做这两条**：`DOMAIN-REGEX` 与 `GEOSITE` 是真实 Clash 订阅里最常见、也是我们此前唯一会整条丢弃的两类。丢掉它们不会报错，只会让「广告拦截」这类规则静默失效——用户以为导进来了，其实一条都没生效。

**三个刻意的设计**：
- **坏正则在保存时就报错，不等内核。** `RuleMatcher::validate` 里直接 `regex::Regex::new`，失败返回「域名正则不是合法的正则：…」。内核拿到坏正则会拒绝**整份配置**，报的还是 JSON 里第几行——那时用户已经在用另一台机器排查了。
- **`matches_domain` 对坏正则答「说不准」，不答「不匹配」。** 返回 `None` 而不是 `Some(false)`：路由页的即时判定只是提示，把"配置有错"伪装成"这条规则不适用"会把用户指向错误的行。
- **`GEOSITE` 的值落库时折成小写。** Clash 用户习惯随手写 `Category-Ads-All`，而两个内核的 geosite 名单都区分大小写；导入时 `to_ascii_lowercase()` 一次，比让内核在启动时悄悄跳过这条规则好。

**两个坑**：
1. **UIA 里的分段控件不是按钮。** 内核切换 `Xray` / `sing-box` 走的是 `TogglePattern`（`uia2.ps1` 输出里写作 `via toggle`），按 `Button` 找会得到 `0 (of 0 matches)`。`invoke` 那段代码本来就按 pattern 逐个试，所以脚本里统一用 `invoke|名字` 就够，不要按控件类型筛。
2. **读远端配置截片段要夹长度。** `$t.Substring($i, 900)` 在文件短于 900 字节时会抛 `ArgumentOutOfRangeException: length`；写 `[Math]::Min(760, $t.Length - $s)`。另外 `IndexOf('routing')` 只对 Xray 有效，sing-box 那边键叫 `route`。

**旁证：规则表非空时整批跳过。** 第一次导入时远端 `rules` 已有 4 条旧规则，结果 `nodes` 与 `groups` 都进了，`rules` 一条没动——这是 H7 定下的"不覆盖用户已排好的顺序"，本轮验收时为了看到新规则，先把 `store.json` 的顶层 `rules` 清成 `[]` 再重启导入。

## 21. 正常退出：`--quit` 与托盘图标真的被删除（2026-11-23，云电脑 cloudpc-test）

起因是开发机的任务栏里积了 30 个 X-Matrix 图标。查下来单例锁没坏（`tauri-plugin-single-instance` 用命名互斥体 + `WM_COPYDATA` 转发，一次干净竞速 6 个进程只活下来 1 个），积的是**被 `taskkill /F` 强杀的进程留下的死图标**：进程被强杀时来不及调 `Shell_NotifyIcon(NIM_DELETE)`，Windows 就把按钮留在通知区，直到鼠标扫过才消失。所以真正要补的是**一个能正常退出的入口**，再让开发脚本不再强杀。

改法（`src-tauri/src/lib.rs`）：单实例回调先看参数，带 `--quit` 就调 `quit_cleanly` 而不是唤醒窗口；`setup` 里也处理「本来就没实例、启动参数就是 `--quit`」——这种启动不能留下窗口或图标，隐藏窗口后走同一条退出路径；`quit_cleanly` 用 `AtomicBool` 保证只做一次，先还原系统代理、停内核，再**删除**托盘图标，停 150 ms 让 shell 处理 `NIM_DELETE`，最后 `app.exit(0)`；`RunEvent::ExitRequested|Exit` 再兜一次删除（注销、关窗这类不经过 `quit_cleanly` 的路径）。`build_tray` 每次构建打一行 `创建托盘图标 pid=… 第 N 次`，一个进程生命周期里出现两行就说明有东西建了第二个图标。

验法：`bash /d/tmp/shots/run-quit-accept.sh`（scp 新 exe + `quit-accept.ps1`，用 `schtasks /it` 在**会话 1** 里跑，先 `--quit` 停旧实例、重启 explorer 清掉历史死图标，再跑 `C:\Users\Administrator\quit-accept.ps1`）。脚本跨进程读通知区工具条按钮（`TB_BUTTONCOUNT`/`TB_GETBUTTON`/`TB_GETBUTTEXTW`），统计文本含 `X-Matrix` 的按钮数。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 连续启动 30 次 | 进程数始终 1、X-Matrix 图标数始终 1、只有一个 `Tauri Window` | **通过**。`P0 process=0 icons=0 windows=0` → `A1 process=1 icons=1 windows=1` → `A5/A10/A20/A30` 全是 `1/1/1`，`A-OK 1 process 1 icon` |
| 30 次「启动 → `--quit`」 | 每轮结束进程 0，且图标数不比该轮开始时多 | **通过**。`B-END process=0 icons=0 maxIcons=0 leaks=0 quitFail=0`、`B-OK 30 clean cycles, no icon left`（第一轮 `base=1`，退出后归 0） |
| 没有实例时 `--quit` | 不留进程、不留图标、不留窗口 | **通过**。`C process=0 icons=0 windows=0 before=0`、`C-OK nothing left behind` |
| 参数只认整词 | `--quitter`、路径里含 `--quit` 都不触发退出 | **通过**。3 条单测（`wants_quit` 整词匹配），`cargo test --lib` 共 **270 passed** |
| 单例语义没被改坏 | 第二个实例退出，第一个实例仍在 | **通过**。A 阶段 30 次启动后仍只有 1 个进程，且窗口被唤醒（`windows=1`） |

**三个刻意的设计**：
- **退出走 `--quit`，不走 `taskkill`。** Windows 上没有「礼貌地关掉窗口」这条路：点 X 只是隐藏到托盘，`taskkill` 不带 `/F` 也走不到 `quit_cleanly`。`--quit` 复用已经在跑的实例做退出，是唯一能保证「还原代理 → 停内核 → 删图标 → 退出」顺序的入口，开发脚本从此用它当常规停止手段。
- **删除图标靠 `Drop`，不靠 `set_visible(false)`。** `tray-icon 0.25.1` 的 `set_visible(false)` 只是 `Shell_NotifyIcon(NIM_MODIFY)` + `NIS_HIDDEN`——图标在 shell 里仍然存在，只是不画；真正 `NIM_DELETE` 只在最后一个 `TrayIcon` 句柄被 drop 时发生。而 `TrayIcon` 是引用计数的，所以托盘的托管状态必须放成 `Mutex<Option<TrayIcon>>`，退出时 `take()` 出来丢掉，再 `remove_tray_by_id` 丢掉资源表那一份，两个句柄都放掉才轮得到删除。
- **`--quit` 的整词匹配。** 用 `== "--quit"` 而不是 `contains`：节点名、路径、`--quit-later` 都不该让一个正常启动的进程退出。

**四个坑**：
1. **从 SSH（会话 0）直接跑 `X-Matrix.exe --quit` 会变成一个新的主实例。** 命名互斥体是按会话隔离的，会话 0 看不到会话 1 的互斥体，于是它自己建窗口、抢 WebView2 用户目录，报 `WebView2 error: WindowsError(0x800700AA)`「请求的资源在用中」然后 panic。单例相关的任何验证都必须在会话 1 里做（`schtasks /create … /it`），否则结论是假的。
2. **「图标数没变」不等于「图标删掉了」。** 被测脚本最初把 `NIS_HIDDEN` 的按钮也算进图标数，第一次跑出来 `A1 icons=2`、`B20 icons=21`，看着像泄漏 30 个；实际上是隐藏态按钮还在工具条里。判据要写成「删除后计数归零」而不是「计数不增长」，否则修好之后仍然测不出来。
3. **`COPYDATASTRUCT` 在 x64 上是 24 字节**（`IntPtr dwData; int cbData; IntPtr lpData`）。手搓消息验证转发时把 `dwData` 写成 `int`，结构体布局错位，处理函数当成未知数据直接丢弃、返回 1，看上去像「转发坏了」，其实消息格式就是错的。
4. **验收脚本自己的 P/Invoke 也会错。** `OpenProcess`/`CloseHandle` 在 `kernel32`，写成 `user32` 时脚本一进 `Count-XmIcon` 就抛「无法在 DLL user32.dll 中找到入口点」，而 `trap` 之前没写，日志只有两行就断在半路——现在脚本把所有异常写进 `FATAL`/`STACK` 行并补一个 `DONE`，跑挂了也看得见。

**为什么先修这条**：它挡着后面所有真机验收。每轮开发验收都要重启程序几十次，强杀一次多一个死图标，用户看到的就是「你们的程序开了好多份」，而实际上单例锁一直是好的——这种「看起来像崩溃」的假象比真崩溃更费时间。

## 22. 关于页的「检查更新」：只问发布页，不碰自己的二进制（2026-11-24，云电脑 cloudpc-test）

这一节原本要验的是「点一下能不能拿到版本号」，真机第一点就点出一个文案错误：界面写的是**「没能问到发布页：订阅返回 HTTP 404」**——更新检查借用了订阅的抓取函数，而那个函数的失败文案是写死的「订阅」。用户按「关于」是在问这个程序自己有没有新版，报错却说订阅出了问题，等于把排查方向指错。

改法：抓取层（`src-tauri/src/net/remote.rs`）把「这次抓的是什么」变成参数——新增 `fetch_answer(url, timeout, user_agent, proxy, noun)` 返回 `Answer { status, body, subscription_userinfo }`，所有会在界面上出现的句子都用 `{noun}` 拼；订阅那条路继续传「订阅」，所以订阅的文案一个字没变。`update.rs` 传「发布页」，并且把非 2xx 状态解释成人话：404 → 「发布页还没有发布记录（HTTP 404）」，其它 → 「发布页返回 HTTP {code}」。

验法：`bash /d/tmp/shots/verify-update.sh`（scp 新 exe → 会话 1 里用 `--quit` 停旧实例 → 覆盖 `C:\Users\Administrator\X-Matrix.exe` → 启动 → 点侧栏「关于」→ UIA `invoke|检查更新` → 截图 → OCR）。核对窗口时**先 `dir` 看大小/md5**，覆盖失败时脚本仍会打印 deployed。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 仓库还没发版时（v0.1.0 之前） | 文案指向「发布页」且说清是「还没有发布记录」，不出现「订阅」两个字 | **通过**。真机显示「没能问到发布页：发布页还没有发布记录（HTTP 404）」 |
| 仓库发了 v0.1.0 之后 | 能读到 tag，显示「已是最新版本（0.1.0）」 | **通过**。真机显示「更新 — 已是最新版本（0.1.0）」（`v0.1.0` release id 455684881） |
| 两个状态都只读不写 | 检查更新不改任何本地文件、不下载、不替换程序 | **通过**。对比前后 `store.json` 与 exe 的 md5 都没变；代码里 `check()` 只发一次 GET 并解析 JSON |
| 订阅的报错文案没被带歪 | 订阅抓取失败仍然是「订阅返回 HTTP …」 | **通过**。`fetch_through` 仍传「订阅」，原有单测全绿；`cargo test --lib` **272 passed** |
| 状态码不是 404 时 | 文案带「发布页」和真实状态码 | **通过**。单测 `other_statuses_still_name_the_feed_and_the_code` |

**三个设计理由**：
- **「抓的是谁」必须由调用方说。** 抓取函数的错误文案会被原样显示给用户，任何写死的主语迟早会在别处说错话；把它变成参数，是让文案跟着调用点走，而不是让调用点去替换字符串。
- **404 不是错误，是正常状态。** 一个还没发过版的仓库，`releases/latest` 就是 404；把它当异常会让用户以为网络或程序坏了。所以单独给它一句人话。
- **只问不拿。** 程序自己替换自己的二进制需要签名校验与回滚策略，这一版没有，所以宁可不做——界面上也这么写，不留一个「下载更新」的假开关。

**四个坑**：
1. **UIA 树比窗口晚就绪。** 启动后约 10 秒内 `invoke|检查更新` 返回 `0 (of 0 matches)`，看着像按钮不存在；页面画完再跑就返回 `1 (of 2 matches)`。凡是靠 UIA 点 WebView2 里的元素，第一次拿到 0 匹配要重试一次再下结论。
2. **按钮可能在折叠线以下。** 「检查更新」在 1280×800 的窗口里要滚动才可见，像素点击点不到；`InvokePattern` 会自动把它滚进视野，所以能用 UIA 就别用坐标。
3. **OCR 会打散数字。** `0.1.0` 被读成 `0 1 0`，`已是最新版本` 被读成 `已 是 最 新 版 本`。判据要按「关键词 + 结构」认，别拿整句做字符串比较。
4. **开发机直连 GitHub API 会撞限流。** 未带令牌的请求从本机发出去是 `API rate limit exceeded`（共享出口 IP），而云电脑能正常访问 `api.github.com`；涉及发布页的检查在真机上验，别在本机用 curl 下结论。

**为什么这节要同时记 404 和成功两条路**：只验一条等于没验。404 那条暴露了文案 bug，成功那条才证明「读到 tag 后能正确比对版本」。发布 `v0.1.0` 本身也是这条验收的一部分——没有发布记录，成功路径根本走不到。

## 23. GEO 数据文件：看得见「在不在」，也能把文件真的下下来（2026-11-24，云电脑 cloudpc-test）

规则里的 `geoip:cn`、`geosite:geolocation-!cn` 靠内核目录下的 `geoip.dat` / `geosite.dat` 才能匹配。文件缺失时内核照常启动、规则不报错，只是永远匹配不到——所以「规则写对了却不生效」最常见的根因，就是这两个文件根本不在。这一节补两件事：规则集页能把两个文件的存在/大小/修改时间说清楚，并且能一键把文件下下来。

验法：`bash /d/tmp/shots/geo-accept.sh`（部署 exe → 会话 1 启动 → `xmgeo` 任务跑 `geo-accept.ps1` → 轮询，截图 OCR）。导航不按像素点，改为在 UIA 里按名字找侧栏项、点它的中心、再用该页自己的工具栏按钮确认页面已切换。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 缺文件时说清楚 | 规则集页出现「GEO 数据文件」卡片，两个文件都标「还没有」，不报错 | **通过**。OCR 读到 `GEO 数据文件` 与 `geoip.dat 还没有 geosite.dat 还没有` |
| 点一下能真的下下来 | 卡片上显示真实大小，磁盘上出现两个文件 | **通过**。`geoip.dat` **16,503,149 B**、`geosite.dat` **11,482,331 B**，结果行 `geo 数据已更新 · geoip.dat 已更新（15.7 MB） · geosite.dat 已更新（11.0 MB）` |
| 下载失败要能说明原因 | 每个资产把每次尝试的原因按地址顺序写进一行 | **通过**。cloudpc-test 上 `github.com` 直连不通时，旧实现只显示 `error sending request for url (…)`；新实现按 jsDelivr → raw.githubusercontent → GitHub releases 依次尝试 |
| 中断不毁旧数据 | 写入走 `.tmp` + 改名，失败时旧文件仍在 | **通过**。单测 `a_failed_install_keeps_the_previous_file`（用只读目标注入失败） |
| 内核读的就是卡片说的那份 | 启动内核时把 `XRAY_LOCATION_ASSET` 指向 `engines/` | **通过**。`SystemSpawner::spawn` 里设置；卡片读的是同一个目录 |

**三个设计理由**：
- **三个地址是有顺序的退路，不是噱头。** 真机上 `https://github.com/…/releases/latest/download/geoip.dat` 直接 `无法连接到远程服务器`，而 jsDelivr 与 `raw.githubusercontent` 同一秒就能取到同样 16,503,149 字节。只用官方地址，等于把「某些网络下更新 geo 一定失败」写进代码。
- **缺失是常态，不是错误。** `status()` 对不存在的文件返回 `exists: false, bytes: 0` 而不是 `Err`，因为「没有文件」正是这条信息要传达的内容；只有真的读不了目录才报错。
- **失败文案要能拿来排查。** 一行里按候选顺序排每次尝试（`HTTP 403` / `error sending request…：tcp connect error：由于目标计算机积极拒绝…`），用户照着第一个地址就能判断是自己的网络还是镜像挂了。

**四个坑**：
1. **`github.com` 在测试网络上不通，镜像通。** 同一台机器 `Invoke-WebRequest` 打 `cdn.jsdelivr.net`、`raw.githubusercontent.com` 都是 200 且长度一致，打 `github.com` 是「无法连接到远程服务器」。看到 `error sending request` 先分别测每个候选地址，别急着怀疑 TLS 或代码。
2. **PowerShell 在 150% 缩放下坐标会被虚拟化。** 未声明 DPI 感知时，`SetCursorPos` 的坐标按 1.42 倍映射，点侧栏「规则集」会落到「日志」或「设置」——现象很像「页面自己跳了」。脚本开头必须 `SetProcessDPIAware()`，之后 UIA 的矩形与点击坐标才在同一坐标系。
3. **首点只激活窗口。** 窗口未聚焦时第一次点击只把窗口提到前台，页面不会切。导航要「点 → 用目标页自己的按钮名验证 → 没到就再点」，最多 5 次，别假设一次就成。
4. **`schtasks /tr` 有 261 字符上限。** 把四个截图路径内联进动作字符串会超限，脚本静默不跑；要跑多用例就上传一个 `.ps1` 包装，动作里只写 `-File` 那一条。

**为什么单测要覆盖候选顺序**：镜像顺序是行为的一部分——排在前面的地址决定了国内网络的下载体验。把它写进断言（长度 3、首条 jsDelivr、末条 GitHub releases、未知名字返回空），以后有人「顺手重排」时测试会立刻说话。

## 24. 测速记录：点出来的数字要留在下次启动之后（2026-11-24，云电脑 cloudpc-test）

以前延迟只活在前端内存里（`src/views/NodesView.vue` 的一个 `ref`）：切页、重启程序就归零，用户每次打开都要重新测一遍才敢选节点。这一节把测速结果落到 `store.json`，并且说清楚这个数字是「什么时候测的、用哪种方法测的」。

验法：`bash /d/tmp/shots/geo-accept.sh` 的后半段（同一个脚本，规则集验完切到节点页 → 「全部测速」→ 读 `store.json` 的 `probes` → `--quit` → 重启 → 再读一次）。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 测速结果落盘 | `store.json` 的 `probes` 里出现本次测速的节点，带延迟与时间戳 | **通过**。6 条记录，样例 `{"delay_ms":175,"at":1795457911,"kind":"tcp"}` |
| 重启后还在 | `--quit` 干净退出再启动，记录条数不减 | **通过**。重启前后都是 **6** 条 |
| 区分测法 | 记录里带 `kind`（`tcp` / `real`），界面提示当前数字是哪一种 | **通过**。`ProbeKind::{Tcp,Real}`；节点页延迟气泡的 tooltip 说明来源 |
| 测不到不覆盖 | 探测被拒绝（`Refused`）时不把已有数字改成失败值 | **通过**。`DelayAttempt::{Measured,Failed,Refused}` 三分支，只有 `Measured` 才写记录 |
| 删订阅要带走记录 | 删除订阅时，它带进来的节点的测速记录一并清掉 | **通过**。`commands/subscription.rs` 删除路径调用 `drop_probes` |
| 能手动清空 | 节点页有「清除测速记录」 | **通过**。命令 `clear_node_probes`，按钮在节点页工具栏 |

**三个设计理由**：
- **存的是「最近一次可信的观测」，不是「最后一条结果」。** 失败与拒绝都不写盘，否则一次网络抖动就会把用户刚测出来的 175 ms 抹成「超时」。宁可显示旧数字加时间，也不显示一条无意义的失败。
- **时间戳进界面。** 数字会过时，所以气泡里跟着「3 分钟前」这类相对时间；没有时间的延迟等于假数据。
- **`kind` 必须存。** TCP 握手延迟和真实请求延迟不是一回事，混在一起会让人误判节点质量；存下来，界面才有资格说「这个数字是怎么来的」。

**两个坑**：
1. **`probes` 要能缺省。** 老 `store.json` 没有这个键，字段必须 `#[serde(default)]`，否则升级即崩。
2. **specta 导出会拒绝 `i64`/`u64`。** 时间戳用 `i64` 会触发 `Attempted to export … BigInt-style types`，所以对外的 `at` 是 `f64`（JSON 本来就是数字），内部仍按整秒存。

**为什么验收要连带重启**：落盘的唯一意义就是「下次还在」。只读一次 `store.json` 证明不了这一点——写进去可能是内存快照，只有杀进程再读才算数。

## 25. 内核自己死了要出声：外部杀掉内核 → 通知 + 首页说明；自己点「停止」→ 安静（2026-11-24，云电脑 cloudpc-test）

以前内核死了只有「下次打开首页瞄一眼」才会发现：`EngineRuntime::status()` 在没人调的时候什么都不做，于是程序可以安静地挂着，用户以为在跑代理，其实早断了。这一节让内核的非预期退出被**主动发现**：1 秒一次的后台检查 + 系统通知 + 首页一行「上一次内核退出码 N」。

验法：`bash /d/tmp/shots/exit-accept.sh`（部署 → 隐藏窗口启动并重定向 stderr → 首页点「启动」→ 从外部 `taskkill /F /PID` 杀掉 `xray.exe` → 读 stderr、枚举新增窗口、读首页文字 → 再启动一次并点「停止」，确认这一次什么都不发）。

| 项 | 判据 | 结果 |
| --- | --- | --- |
| 非预期退出被主动发现 | 外部杀掉内核后 1 秒内，stderr 出现 `内核退出通知已发送（退出码 1）` | **通过**。`x-matrix: 内核退出通知已发送（退出码 1）` |
| 系统收下了通知 | 插件 `show()` 返回 `Ok`（不是「尝试了但失败」） | **通过**。同一条 stderr 只会在 `Ok(())` 分支打印 |
| 首页说清楚发生过什么 | 首页文本框出现 `上一次内核退出码 1，点「日志」看内核最后说了什么。` | **通过**。UIA 读到该句 |
| 主动「停止」不吵人 | 点「停止」后，stderr 没有「已发送」，首页也没有退出码提示 | **通过**。两次检查都为空 |
| 只报一次 | 同一次退出不会每秒重复通知 | **通过**。`take_unexpected_exit()` 取走即清空；新增单测 `a_kernel_that_dies_on_its_own_is_announced_exactly_once` |
| 单元测试 | `cargo test --lib` 全绿 | **通过**。293 passed（新增 2 条：主动停止不算意外、意外只报一次） |

**三个设计理由**：
- **判定靠「这次退出是不是我要的」，不靠猜退出码。** `Running.expected: bool` 在 `stop()` 里、`kill()` **之前**置位，所以主动停止与 `--quit` 走的是同一条「预期路径」；只有没置位的退出才记账。用退出码判断会把「内核自己以 0 退出」也漏掉。
- **通知是礼貌，不是生命周期的一部分。** 通知失败只打一行 stderr，绝不影响启停；没有通知服务的机器照样跑代理，首页那行字仍然在。
- **后台线程只做一件事。** `watch_engine_exits` 每秒调一次 `status()`（顺带回收僵尸进程），拿到非预期退出就发事件 + 通知；它不碰 UI、不写 store，`--quit` 路径根本走不到它。

**四个坑**：
1. **测试机默认关掉了通知。** 云电脑 `HKCU\Software\Microsoft\Windows\CurrentVersion\PushNotifications\ToastEnabled` 是 `0`，此时插件仍返回 `Ok`（系统收下了），但屏幕上不会出现横幅，肉眼验收会误判成「没发」。要么先 `reg add … /v ToastEnabled /t REG_DWORD /d 1 /f`，要么就以 stderr 那行作为判据——这也是为什么代码要同时打印成功与失败两种结果。
2. **Windows Server 2022 不弹横幅。** 即便 `ToastEnabled=1`，枚举窗口也看不到 toast 窗口（通知只进通知中心）。所以「有没有横幅」不能当判据；`show()` 的返回值 + 首页提示才是可验证的事实。
3. **PowerShell 5.1 用 ANSI 读无 BOM 的脚本。** 脚本里的 `启动` 会变成 `鍚姩`，UIA 按名字找按钮永远 0 个候选，日志看起来像「按钮不存在」。含中文的 `.ps1` 必须存成 **UTF-8 with BOM**。
4. **`ControlType` 对象不能用 `-eq` 比较。** `$e.Current.ControlType -eq $ct::Button` 在 PowerShell 里恒为假；要用 `AndCondition(Name, ControlType)` 一次查到位。另外 WebView2 的按钮有时 `InvokePattern` 不响应，按元素矩形中心做 DPI 感知点击更接近用户操作。

**为什么验收要分「杀」和「停」两遍**：只验证「杀了会通知」会让「停止也通知」这种反向 bug 溜过去，而用户每次正常断开都弹一条通知，比不通知更烦。两遍都过，才算这条功能成立。

## 26. 出口 IP：一个数字、一条路线，失败时不留下旧答案（2026-11-24，云电脑 cloudpc-test）

**验法**：`bash /d/tmp/shots/ip-accept.sh`（构建 → `--quit` 停掉旧实例 → 部署 `X-Matrix.exe` → 起应用 → `xmip` 探测任务读三态）：

1. 内核未启动时，首页读一次出口 IP；
2. 点「启动」，等内核进程出现后，把首页所有「刷新」按钮都点一遍；
3. 点「停止」，再点一次「刷新」。

判据读的是首页的文本元素（UIA，不靠截图）：地址、路线徽章、「还没查过」占位、错误行。

| 判据 | 验法 | 结果 |
|---|---|---|
| 直连时给出地址与路线 | 未启内核时读到的文本同时含点分 IP 与 `直连（未经代理）` | **通过**。`出口 IP / 180.119.140.33 / 直连（未经代理）` |
| 请求真的穿过内核 | 起内核后再查：同样的三个主机，直连时都成功，经代理时全部失败——说明请求确实走了 mixed inbound，而不是仍从本机直出 | **通过**。`ipv4.icanhazip.com 拉取失败：error sending request for url (…)；ifconfig.me/ip 拉取失败：…；ipinfo.io/ip 拉取失败：…` |
| 失败时不留旧答案 | 失败后首页不出现任何地址、不出现 `直连`/`经代理` 徽章，只剩错误行与「还没查过」 | **通过**。`PROXY verdict: badge_present=False error_present=True`；文本为 `出口 IP / 还没查过 / 查不到出口 IP：出口 IP 都问不到：…` |
| 停掉内核后回到直连 | 点「停止」，再刷新，重新给出地址且徽章是 `直连` | **通过**。`STOP verdict: direct_ip=True direct_badge=True` |
| 三个候选按顺序退让 | 全部失败时错误行按顺序各列一次主机名，用 `；` 分隔 | **通过**。同一条错误行里 icanhazip → ifconfig.me → ipinfo.io 依次出现 |
| 单元测试 | `cargo test --lib` 全绿 | **通过**。299 passed（含候选顺序、失败行一/三主机） |

**三个设计理由**：
- **先决定路线，再发请求。** `via` 在 `spawn_blocking` 之前就按 `runtime.status().running` 定下：内核在跑就走 `127.0.0.1:mixed_port`，否则直出。反过来「先发再猜」会把「请求成功但其实没走代理」也说成经代理，用户就失去了判断泄漏的能力。
- **旧答案比错误更危险。** 一次失败的查询不证明任何路线，若把上次的地址和 `直连` 徽章留在旁边配一句新错误，界面上就出现了一组自相矛盾的断言。所以失败即清空 `exit`/`exitAt`，只留错误行。
- **三个回声服务、只问一次。** 首个被接受（2xx + 能被解析成 IP）的回答立即返回，后面的主机不再请求；失败才依次退让。这样正常情况只有一次往返，网络挑剔时仍有三条路。

**四个坑**：
1. **回声服务在这张网里不齐。** 云电脑上 `https://api.ipify.org`（`基础连接已经关闭`）、`https://api.myip.com`（超时）、`https://ip.3322.net`（TLS 不受信）都不可用；可用的是 `ipv4.icanhazip.com`、`ifconfig.me/ip`、`ipinfo.io/ip`、`myip.ipip.net`。候选表就是按这张实测清单排的。
2. **测试机的节点没有上游。** 当前 `last_node_id=local-ws`，是 `127.0.0.1:18081`（`netstat` 显示该端口无人监听），所以「经代理」这条路线注定失败。这正好成了路线证据：直连三个主机都成功、经代理三个全失败，说明请求确实进了 mixed inbound。要验收「经代理也能拿到 IP」，得先有一个能出网的节点。
3. **首页有多个「刷新」。** 只看名字会点到订阅或另一次刷新；验收脚本把所有名为「刷新」的按钮挨个点一遍再读文本，比赌一个坐标稳。
4. **按钮列表要等页面画完再读。** 启动后 3 秒读按钮会得到空列表（`buttons+r: ` 后面什么都没有），8–10 秒后才有 15 个按钮；过早读取会被误判成「按钮不存在」。

## 27. 订阅的「下次刷新」：写清楚下一次什么时候来（2026-11-24，云电脑 cloudpc-test）

**验法**：`bash /d/tmp/shots/sub-accept.sh`（部署 → 起应用 → 用一个本机假机场建一条订阅 → 刷新 → 读订阅表里那一行）：

| 判据 | 验法 | 结果 |
|---|---|---|
| 自动更新显示周期 | 订阅行「自动更新」列读出「每 6 小时」（默认 360 分钟） | **通过** |
| 上次更新与倒计时同时出现 | 同一行「上次更新」列是「刚刚 还有 5 小时」——时间和剩余时间各自成句，不互相吃掉 | **通过** |
| 刷新后倒计时重置 | 刚刷新完读数是「刚刚 / 还有 5 小时」，不是空的也不是「已到期」 | **通过** |
| 状态行 | 同一行「状态」列读出「正常」，行内没有多余的红字 | **通过** |
| 单元测试 | `npm run check` 里 `format.test.ts` 的 11 个新用例 | **通过**。76 tests passed（到期/边界/手动 0/停用/从未更新/分钟小时天/en 单复数） |

**三个设计理由**：
- **一条订阅一个定时器。** 页面只在 `onMounted` 起一个 30 秒的 `setInterval`，所有行共用同一个 `now`；每行各自起定时器会在长列表里滚出几十个计时器，卸载时也很难收干净。
- **「还有多久」是算出来的，不是存下来的。** store 里只有 `last_state.updated_at` 与 `update_interval_minutes`，剩余时间每次渲染用 `updatedAt + interval - now` 现算；跨过零点、改时间、夏令时都不会让存值过时。
- **到点就说「到期」，不说负数。** 过期时显示 `该刷新了`（`sub.next.due`），不显示「-3 分钟」；停用或周期为 0 时显示「手动刷新」，不显示「还有 0 分钟」。

**两个坑**：
1. **倒计时在刷新前是空的。** 从未拉取过的订阅没有 `updated_at`，此时那一格留白而不是「还有 0 分钟」——空着才诚实，写 0 会让人以为「马上就要刷新」，其实一次都没拉过。
2. **「刚刚」与「还有 5 小时」是两个元素。** UIA 里它们合并在同一个 `DataItem` 的名字里（`刚刚 还有 5 小时`），按「还有」这个子串去找会漏；验收要读整格文本。

## 28. 订阅改名与过滤：刷新时改，不是建节点时改（2026-11-24，云电脑 cloudpc-test）

**验法**：同一条脚本的后续阶段（`sub-accept.ps1` 第二段）：先不填三条规则刷新一次，再填「机场A · 」/`香港`/`旧` 刷新一次。

| 判据 | 验法 | 结果 |
|---|---|---|
| 不带规则时原样导入 | 首次刷新后提示 `「机场A」更新完成：4 个节点（替换 0 个）`，节点名保持机场原文 | **通过** |
| 改名生效且只加一次前缀 | 填前缀后再刷新，节点页读出 `机场A · 香港 01`、`机场A · 香港 02` | **通过** |
| 只保留规则生效 | 保留 `香港` 后只剩两条；页面提示 `按过滤规则丢掉 2 个` | **通过** |
| 排除规则生效 | 排除 `旧` 后 `旧鸟 04` 不再出现（节点页名字里没有「旧鸟」） | **通过**（`VERDICT prefix=True excluded_present=False`） |
| 替换语义 | 第二次刷新的提示是 `替换 4 个`，说明它换掉的是上一次带进来的那一批，不是新增一批 | **通过** |
| 规则错误要说人话 | 单元测试：非法正则保存时返回 `只保留的名字正则写错了：…` / `排除的名字正则写错了：…` | **通过**。`cargo test --lib` 312 passed（含 13 条订阅测试） |

**三个设计理由**：
- **规则在刷新时应用，不在创建节点时应用。** 机场换名、规则改词之后，用户只要点一次刷新就能得到新名字；若在节点创建时固化，改规则得删了重建订阅。
- **先过滤再改名。** 保留/排除读的是机场原始名字（`香港 01`），改名只影响最后写进 store 的名字。反过来的话，第二次刷新时名字已带前缀，`香港` 规则就再也匹配不上，节点会在下一次刷新时整批消失。
- **前缀只加一次。** `prefixed_name` 判断名字是否已经以该前缀开头；否则每刷新一次就多一层「机场A · 机场A · 香港 01」。

**两个坑**：
1. **排除优先于保留。** 同时命中时节点被丢掉；这条在界面上没有单独提示，只体现在「丢掉 N 个」里，验收要看的名字而不是计数。
2. **过滤发生在去重之前。** 指纹去重会按名字+参数算，先过滤能把「同一机场改过名的重复节点」在进库之前处理掉；顺序颠倒会让被排除的节点先占用一次替换名额。

## 29. 内核二进制：换一个文件，就真的跑那个文件（2026-11-24，云电脑 cloudpc-test）

**验法**：`bash /d/tmp/shots/n11-accept.sh`（设置页 → 内核二进制 → 选择… → 原生文件对话框 → 版本 → 恢复默认），以及 `bash /d/tmp/shots/n11-spawn.sh`（把覆盖路径写进 store → 启动 → 用 `Get-Process` 读实际进程路径）。

| 判据 | 验法 | 结果 |
|---|---|---|
| 选中的文件记进 store | 选完 `store.json` 的 `settings.xray_binary` 等于所选路径 | **通过** |
| 卡片说清当前用的是哪个 | 卡片显示「自定义路径」+ 完整路径 + 「文件已就位」徽标 | **通过** |
| 版本来自那个文件 | 点「版本」读出 `Xray 26.11.9 (Xray, Penetrates Everything.) 676eaf6 (go1.27.3 windows/amd64)` | **通过**。这个副本在 `C:\Users\Administrator\kernels\xray-copy.exe`，输出与默认内核一致但取自覆盖路径 |
| 重启后仍然记住 | `--quit` 再启动，卡片仍显示该路径（`override_path_visible=True`） | **通过** |
| 恢复默认清空覆盖 | 点「恢复默认」后 `xray_binary=''`，卡片回到「默认路径」 | **通过** |
| 启动的确实是覆盖文件 | 设覆盖后启动，`Get-Process` 读到 `xray-copy -> C:\Users\Administrator\kernels\xray-copy.exe`，日志里 `Xray 26.11.9 started`、7890/9090 都在监听 | **通过**（先失败后修，见理由三） |
| 停止后进程消失 | 点「停止」后 `xray*` 进程数为 0 | **通过** |

**三个设计理由**：
- **路径只认文件，不认目录。** 用户给的是一个具体文件；`binary_path` 里覆盖优先、否则回落到 engines 目录，缺文件时报 `找不到 xray 内核文件：{path}`，而不是悄悄用回默认内核。
- **改过的路径从下次启动生效。** 正在跑的内核不会被中途换掉——换内核意味着断线，应该由用户的「停止 → 启动」决定时机。界面上直接写了这句话。
- **geo 数据跟着 engines 目录，而不是跟着内核文件。** 这里踩过一次：`XRAY_LOCATION_ASSET` 原来设成 `exe.parent()`，换到别处的内核就找不到 `geosite.dat`，整套 `geosite:` 规则加载失败（`illegal domain rule: geosite:category-ads-all … failed to open geosite.dat`）。现在内核自己目录里有数据文件就用它，否则回落到 engines 目录——`asset_dir_for` 是纯函数，三个分支都有测试。

**四个坑**：
1. **原生对话框不能被 `SetValue` 直接写。** 资源管理器风格的 `#32770` 文件名框走 ValuePattern 会超时（`操作超时。 (HRESULT:0x80131505)`）；要先激活对话框，再用 `SendKeys.SendWait($path)` + `{ENTER}`。
2. **设置页的按钮在折叠线以下。** 选择… 在 y≈2295、窗口只有约 1136 高，直接按坐标点会点空；必须先 `ScrollItemPattern.ScrollIntoView()` 再把矩形读一遍。
3. **同名任务重入会把两次运行写进同一个日志。** `schtasks /run` 不会等你上一次跑完；每次跑之前 `/end` + `/delete`，否则日志里的时间戳会跳来跳去。
4. **换内核会暴露协调性问题，而不是内核问题。** 覆盖路径能跑起来之后才发现 geo 数据定位错——这类「A 功能改了 B 功能的前提」的问题只有真机换一次内核才会露出来，所以验收必须真的启动、真的读进程路径，不能只看卡片。

## 30. 节点二维码：手机扫一眼就能导入，链接和「复制链接」是同一个（2026-11-24，云电脑 cloudpc-test）

**验法**：`bash /d/tmp/shots/qr-shot.sh`（部署 → 启动 → 节点页点「二维码」→ 读对话框文字 → `PrintWindow` 抓窗口存证 → `--quit`）。前端纯功能，用 `qrcode` npm 包在渲染层出 SVG，没有新增 IPC。

| 判据 | 验法 | 结果 |
|---|---|---|
| 按钮在节点行里 | 节点页按钮序列含 `二维码`（在「复制链接」与「删除」之间） | **通过** |
| 点了就开，不用等刷新 | 点「二维码」后对话框立即出现，标题是节点名 `Local-WS` | **通过** |
| 说清用途 | 对话框文字含 `用手机或另一台设备扫这个码导入节点` | **通过** |
| 二维码和链接同源 | 对话框里同时给出明文链接 `vless://1111…@127.0.0.1:18081?encryption=none&type=ws&path=%2Fws&security=none#Local-WS`，与 `exportNodeLinks` 的返回逐字一致 | **通过**（`link.txt` 与对话框文字比对） |
| 码真的画出来了 | 窗口截图里二维码区是一块 240×240 的黑白矩阵（约 45% 深色像素），不是空白方块；渲染失败时组件会显示 `生成二维码失败：{错误}` 文案而不是留白 | **通过**（截图存证 `qr.png`） |
| 出图失败不会留空 | `QRCode.toString` 抛错时 `failure` 有值，模板走 `nodes.qr.failed` 分支（161 行组件，:92） | **通过**（代码路径 + `npm run check`） |
| 关闭要顺手 | 点「关闭」对话框消失，不改变节点本身 | **通过** |

**三个设计理由**：
- **前端出码，不碰内核也不碰 IPC。** 二维码只是把已经能导出的链接换个画法；放前端意味着零新增命令、零 Rust 依赖（本机 cargo 缓存里没有 `qrcode` crate，离线加不上）。
- **码和明文链接一起给。** 扫码失败的场合（旧手机、屏幕反光、截图发给别人）仍能把链接复制走；两个出口的内容来自同一个 `exportNodeLinks` 返回值，不会出现「码里是 A、文字是 B」。
- **换链接就重画，失败就说出来。** `watch([open, link], { immediate: true })` 加 generation 计数，避免上一条节点的码在慢渲染后覆盖新的；`toString` 失败写入 `qrError`，界面上看到的是「没能生成」，不是一块白。

**三个坑**：
1. **`CopyFromScreen` 在这台机器上抓到的是黑屏加一块噪声。** 桌面会话里 WebView2 的硬件表面不会被 GDI 拷走；必须用 `PrintWindow(hwnd, hdc, 2)` 抓窗口，而且抓到的是半分辨率（每个像素横竖各重复两次），直接按截图坐标量模块会全军覆没。
2. **没有扫码设备就没有「扫出来是什么」的硬证据。** 这台测试机没有摄像头、也没有离线解码器（npm registry 此刻解析不了 `jsqr`），所以本节记录的是「码被画出来 + 它和明文链接同源」，不是「扫出来逐字一致」。要补这一条，得用真手机扫一次并记录解码结果——留作后续。
3. **`navigator.clipboard` 在非安全上下文里会拒绝。** 复制失败时对话框显示 `复制失败，剪贴板不可用`，不会假装成功。

## 31. 内核崩了，机器也不能留在断网状态：崩的一瞬间把系统代理还回去（2026-11-24，云电脑 cloudpc-test）

**验法**：`bash /d/tmp/shots/proxy-accept.sh`（`--quit` 旧实例 → 部署 → 启动 → 点「启动」→ 读注册表 `ProxyEnable/ProxyServer` → 用 `taskkill /F /PID` 从外部杀掉内核 → 再读注册表 → 读首页文字与 `app.err.txt`）。此前（§25）只证明了「内核死了会被通知」，没证明「通知的同时机器还能上网」——这一节补的就是那段空隙。

| 判据 | 验法 | 结果 |
|---|---|---|
| 启动内核就接管系统代理 | 点「启动」后 `HKCU\...\Internet Settings`：`ProxyEnable=1`、`ProxyServer=127.0.0.1:7890` | **通过** |
| 内核被外部杀掉后代理真的还回去 | 杀掉后 3.5 秒再读：`ProxyEnable=0`、`ProxyServer` 变回未设置 | **通过** |
| 还的时候留了话 | `app.err.txt` 里 `x-matrix: 内核退出后已交还系统代理`，紧跟 `x-matrix: 内核退出通知已发送（退出码 1）` | **通过** |
| 界面不再假装还接管着 | 首页文字回到 `直连（未经代理）`，同时显示 `上一次内核退出码 1…` | **通过** |
| 没有快照就不许乱动 | 单元测试 `releasing_an_inactive_guard_never_writes`：`writes().len()` 前后一致 | **通过**（`cargo test --lib` 320 passed） |
| 还完不留垃圾 | 单元测试 `releasing_an_active_guard_restores_the_previous_state`：快照文件被删除、`is_active()` 变回 false | **通过** |

**三个设计理由**：
- **交还在推送之前。** 前端收到 `engine://exited` 时会重新读一次状态（`src/stores/engine.ts` 的 `watchExits` 顺带 `useSystemProxyStore().refresh()`）；如果先 emit 再交还，界面就会先读到一个「已接管」的旧世界，随后再被下一次轮询纠正。先交还，事件描述的就是已经修好的机器。
- **通知里那句话是可选的。** `notify_engine_exit(app, code, released_proxy)` 只在真的交还成功时追加「系统代理已交还，网络已恢复直连」；交还失败时宁可只说「内核退出」，也不让用户以为自己已经能上网——失败本身会以 `内核退出后交还系统代理失败…` 打在 stderr 上。
- **交还逻辑放在 `ProxyGuard` 里，而不是散在监听线程里。** `release_if_active()` 只有 6 行，但它决定「动别人的设置」还是「什么都不做」；放进能注入 `FakeProxy` 的类型里，两条分支都能被单元测试钉死，而不是靠真机注册表碰运气。

**三个坑**：
1. **`findstr /C:"代理"` 在 cmd 里什么都找不到。** 远程 shell 是 `cmd.exe`，中文参数到 `findstr` 时编码已经变了，一条都不匹配；结论要从 `app.err.txt` 拉回本机再 `grep`。这也顺带解释了为什么第一次跑完「app stderr (proxy lines)」是空的，但日志里其实有两行。
2. **只有 `system_proxy=true` 且用了「启动」才走这条路。** 崩溃时若我们本来没接管（用户自己关的、或从没启动过），`release_if_active()` 必须什么都不写——真机上验证这一点要靠单元测试，因为云电脑的 store 一直是 `system_proxy=true`。
3. **代理是不是「我们的」靠快照判断。** 快照文件 `proxy-snapshot.json` 只在接管前写、交还后删；如果别的软件（V2RayN、手动设置）把代理开着，我们崩了也不该动它——这正是 `is_active()` 而不是「读注册表看开没开」的原因。

## 32. 保存了规则、内核却还在跑旧的：说出来，并给一个按钮（2026-11-24，云电脑 cloudpc-test）

**验法**：`bash /d/tmp/shots/stale-accept.sh`（`--quit` 旧实例 → 部署 → 启动 → 点「启动」起内核 → 进路由页把一条规则停用 → 读页面文字与是否存在「重启内核以生效」→ 点它 → 比较两次内核 PID → 再点「停止」收尾）。节点/模式的切换本来就走 `apply()` 热切，这一节管的是**规则、策略组、规则集**这三类只有内核启动时才读的东西。

| 判据 | 验法 | 结果 |
|---|---|---|
| 内核在跑时改规则，页面立刻说出来 | 停用第一条规则后页面文字出现 `规则已保存，但内核还在用旧的一份` | **通过** |
| 话旁边就有一个动作 | 同一屏出现按钮 `重启内核以生效` | **通过** |
| 按钮真的重启内核 | 点击前 `xray#7952`，点击后 `xray#6260`（PID 变了） | **通过** |
| 重启后那句话自己消失 | 重启完成后再读文字：`notice after restart=False` | **通过** |
| 内核没在跑时不打扰 | `markStale()` 只在 `engine.running` 为真时置位；停内核后重进页面无提示 | **通过** |
| 重启失败要说出来，不许假装成功 | 单元测试 `src/stores/engine.test.ts:120-143`（`reapply` 在未运行/无节点时返回 false），对话框内 `describeError` 显示错误 | **通过**（`npm run check`：78 tests） |

**三个设计理由**：
- **提示只在「内核正在跑」时出现。** 内核没运行时，保存的规则本来就会在下次启动时被读到，此时弹「重启内核」是让用户做无用功。判断放在视图里（`engine.running`），而不是让后端去猜——后端不知道用户此刻看着哪一页。
- **能不能自动重启？不能。** 重启会让所有连接瞬断，也会让正在下载的东西断掉；这是一个只有用户能决定代价的动作，所以给一个按钮而不是替用户按。
- **提示跟着「内核启动时间」复位，而不是跟着「我点过按钮」。** `StaleConfig` 监听 `engine.status?.started_at_ms`；只要内核重启（无论是这个按钮、托盘菜单还是快捷键触发的），旧提示就自动消失。这样不会出现「我已经重启过了，它还在催我」的假警告。

**三个坑**：
1. **UIA 里表格行有两层同名控件。** 「停用」既是 `DataItem`（整格，`942,405 105x119`）也是 `Button`（真按钮，`960,443 69x43`），前者排在前面；按名字取第一个就会点到格子中心，恰好落在旁边的「编辑」上，于是打开编辑对话框、看起来像「点了没反应」。必须 `AndCondition(Name, ControlType.Button)`。
2. **WebView2 的行内按钮点坐标不可靠。** 同一次运行里 x=930 命中「编辑」、x=950~1030 全部落空；换成 `InvokePattern.Invoke()` 后一次就中。行内控件一律走 Invoke，不要走鼠标。
3. **UIA 读回来的中文会被截断成半个字。** 日志里 `规则已保存，但内核还在用旧的一份` 会变成 `规则已�存��但内核�在用旧的一�`；判据只能按**子串**匹配（例如 `内核还在用旧的一份` 的前若干字），不能整串比对。


## 33. 改了端口、内核还在旧端口上：先说出来，再用新端口重启（2026-11-24，云电脑 cloudpc-test）

**验法**：`bash /d/tmp/shots/port-accept.sh`（部署 → 首页点「启动」→ 进设置页把「本地混合端口」改成 7891 → 保存 → 读提示与按钮 → 点「用新端口重启内核」→ 读注册表与内核 PID → 改回 7890 再走一遍 → 停止内核收尾）。

端口不像规则：规则改了要等下次启动才生效，端口改了**连重启都要按新值重新占用**，而且系统代理是内核启动时按当时的端口设置的——所以这一节同时验证「提示」和「重启后代理真的指向新端口」。

| 判据 | 验法 | 结果 |
|---|---|---|
| 内核在跑、改了端口，页面立刻说出来 | 保存后页面文字出现 `端口已改，内核还在旧端口上跑` | **通过** |
| 话旁边就有一个动作 | 同一屏出现按钮 `用新端口重启内核` | **通过** |
| 按钮真的把内核搬到新端口 | 点击前 `xray#5072`，点击后 `xray#1636` | **通过** |
| 系统代理跟着搬家 | 重启后注册表 `ProxyServer=127.0.0.1:7891`（改前是 `:7890`） | **通过** |
| 提示自己消失 | 重启完成后再读文字：`notice cleared=True` | **通过** |
| 改回旧端口同样成立 | 再存 7890 → 提示重现 → 重启 → `ProxyServer=127.0.0.1:7890`，`xray#7712` | **通过** |
| 停止内核后不留在断网态 | 点「停止」后 `ProxyEnable=0 ProxyServer=`（空） | **通过** |
| 内核没在跑时不打扰 | `stalePorts` 以 `engine.running` 为前提；未启动时改端口保存不出现提示 | **通过**（`npm run check`：78 tests） |

**三个设计理由**：
- **比的是「存下来的设置」和「内核实际在用的端口」，不是输入框里的草稿。** 输入框边打边比会在用户还在改第二位数时就开始喊；`store.settings` 只有保存成功才会变，`engine.status` 是内核自己的回声，两者不等才是真的不一致。
- **重启按钮不必额外做「重新设置代理」。** `start_engine` 在启动成功后调 `proxy.enable_with_bypass("127.0.0.1", endpoints.mixed_port, …)`，而 `ProxyGuard` 在已接管时会重新写入而不是二次快照——所以新端口一启动，注册表自动跟着新值走；这一点由真机注册表读数证明，而不是靠代码推理。
- **复用同一个提示组件，只换文案。** 规则页说「内核还在用旧的一份」，设置页说「内核还在跑旧端口」；组件只多三个可覆盖的文案 prop，复位逻辑（监听 `started_at_ms`）一个字都没改，于是「任何方式重启都清提示」这条性质对两处同时成立。

**四个坑**：
1. **端口输入框是 `ControlType.Spinner`，不是 `Edit`。** 按 `Edit` 找会一个都找不到；而且它的 `Name` 是「本地混合端口 给 HTTP + SOCKS 共用的入口端口」这种「标题 + 说明」拼接，不能用等值匹配，要按顺序/前缀取第一个。
2. **`ValuePattern.SetValue` 之后要等一拍再保存。** 立即点「保存」偶尔会存到旧值（前端 `v-model` 还没回写）；脚本里固定 `Start-Sleep -Milliseconds 500`。
3. **提示文案必须用子串判断。** UIA 读回的中文会被截断（`端口已改` 完整，但长句会掉字），整串比对会假失败。
4. **改端口这条路径只有在 `system_proxy=true` 时才动注册表。** 如果 store 里关掉了系统代理，重启内核不会写 `ProxyServer`；判据要先读 store，别把「没接管」当成「没生效」。

## 34. 改了日志级别、内核还在用旧级别：同一句话说在另一个位置（2026-11-24，云电脑 cloudpc-test）

**验法**：`bash /d/tmp/shots/log-accept.sh`（部署 → 首页点「启动」→ 进设置页点与当前值不同的级别（store 是 `debug` 就点「警告」）→ 保存 → 读提示与按钮 → 点「用新级别重启内核」→ 读内核渲染出来的 `configs/xray.json` 的 `loglevel` 与 PID → 再点回原级别走一遍 → 停止内核收尾）。

日志级别和端口是同一类谎话：设置页显示「已保存」，可内核进程里还是启动时那一份。区别是端口能被注册表看见，日志级别只能从**内核真正读到的配置**里看见，所以判据直接读 `%APPDATA%\com.xmatrix.client\configs\xray.json`。

| 判据 | 验法 | 结果 |
|---|---|---|
| 启动时记下的级别就是渲染出来的级别 | `store level: debug`，启动后 `render at start: debug` | **通过** |
| 改成另一个级别后页面说出来 | 点「警告」保存 → `saved level: warn`、`notice present: True` | **通过** |
| 话旁边就有动作 | 同屏出现按钮 `用新级别重启内核` | **通过** |
| 按钮真的换了内核的级别 | 重启后 `render after restart: warning`（`xray.json` 的 `loglevel`） | **通过** |
| 换的是新进程 | `pids before=xray#1224 after=xray#2868` | **通过** |
| 提示自己消失 | 重启后 `notice cleared: True` | **通过** |
| 改回去同样成立 | 点「调试」保存 → `notice back: True` → 重启 → `render restored: debug`、`store restored: debug` | **通过** |
| 停止内核后收尾干净 | 点「停止」→ `kernels after stop:`（空） | **通过** |
| 内核没在跑时不打扰 | `staleLog` 以 `engine.running` 为前提；未启动时改级别保存不出现提示 | **通过**（`npm run check`：78 tests） |

**三个设计理由**：
- **内核自己说出它用的级别，而不是回头再读一遍设置。** `EngineStatus.log_level` 来自 `Running.log_level`——那是 `start()` 当时塞进 `RenderInput` 的那个值；如果改成「重新读 settings」，设置一存两边就永远相等，这个提示会在最需要它的时候消失。
- **不比「输入框草稿」，只比「存下来的设置」。** 用户在设置页来回点级别只是在挑，只有点「保存」才算数；比草稿会让提示在挑的过程中反复闪。
- **同一个组件、同一套复位逻辑，第三处复用。** 端口、日志级别、规则页用的都是 `StaleConfig`；文案靠 prop 覆盖，`started_at_ms` 监听不变，于是「任何方式重启都清提示」这条性质自动对三处都成立。

**四个坑**：
1. **级别按钮不支持 `InvokePattern`。** `GetCurrentPattern(InvokePattern)` 直接抛「不支持的模式」，鼠标点也没用——必须先 `ScrollItemPattern.ScrollIntoView()`（这一节在页面很下面，`y≈4856`），再按最新 rect 的**中心**点；直接点旧坐标会落在别处。
2. **点一个和当前值相同的按钮不算「改了」。** 第一次跑验收时 store 已经是 `warn`，脚本又点了「警告」——值没变，提示当然不出现；这不是 bug，是判据写错了。脚本要先读 store，再点**另一个**级别。
3. **渲染出来的拼写和我们存的不一样。** 我们存 `warn`，xray 写的是 `"loglevel": "warning"`；比对要做映射（`warn → warning`），否则「重启成功」会被判成失败。
4. **`store.json` 带 BOM，`ConvertFrom-Json` 会直接报错。** `Get-Content -Raw | ConvertFrom-Json` 抛「传入的对象无效，应为“:”或“}”」；用 `[IO.File]::ReadAllText` + 正则抠 `"log_level"` 才稳。

## 35. 节点没了，内核就不能还占着它：删除即停，换 ID 则跟随（2026-11-24，云电脑 cloudpc-test）

**验法**：两段。删节点用 `bash /d/tmp/shots/delnode-accept.sh`（启动内核 → 节点页删掉正在跑的那个节点 → 确认 → 读内核、注册表、store、提示）；订阅刷新用 `bash /d/tmp/shots/adopt2-accept.sh`（把订阅里的节点设成当前节点并启动 → 用内容相同但顺序不同的假订阅源刷新 → 读内核 PID、`last_node_id`、提示）。

这一节管的是「应用说自己在跑，其实那个节点已经不存在了」。以前删节点只是把行从表里拿掉：`settings.last_node_id` 还指着它，内核里也还跑着它——界面上点什么都对不上，用户只能自己点「停止」。订阅刷新更隐蔽：整批节点换了新 ID，正在跑的那个也在其中，旧代码要么把内核杀掉（哪怕服务器一模一样），要么留着内核配一个已经消失的 ID。

| 判据 | 验法 | 结果 |
|---|---|---|
| 删掉正在跑的节点，内核跟着停 | 删除后 `kernels after delete:`（空） | **通过** |
| 系统代理同时交还 | `proxy after delete: ProxyEnable=0 ProxyServer=` | **通过** |
| 设置里不再指着已删的节点 | `active after: none` | **通过** |
| 页面把原因说清楚 | 出现 `删掉的节点正在被内核使用…内核已停止并交还系统代理` | **通过** |
| 刷新同一个订阅，内核不被打断 | 刷新前后同一个 PID：`xray#4784 → xray#4784` | **通过** |
| 新 ID 被认出来并接手 | `active id before: vless-18eac9ae1ba07634-0` → `after: vless-18ead5d985cb4444-3`，`id changed: True` | **通过** |
| 不误报「已停止」 | 同一轮 `stopped notice present: False` | **通过** |
| 系统代理没被碰 | `proxy after refresh: ProxyEnable=1 ProxyServer=127.0.0.1:7890` | **通过** |
| 刷新结果本身正常 | `「机场A」更新完成：2 个节点（替换 2 个），按过滤规则丢掉 2 个` | **通过** |

**三个设计理由**：
- **「同一个服务器」由指纹判断，不由 ID 判断。** 每次导入都会铸新 ID（`documents.rs` 里的 `node()` 用时间戳），所以刷新后 ID 必然变；能证明「还是那台机器」的只有 `Node::fingerprint()`。它在换表**之前**从旧行上取下来，换完表再比对——顺序反了就永远拿不到旧指纹。
- **先改指针，再决定要不要杀。** 找到替身就 `settings.last_node_id = Some(new)` 并让运行中的内核跟着改 ID（`EngineRuntime::adopt_node_id`）；找不到替身才清空并停内核。于是「服务器还在」和「服务器没了」走两条完全不同的路，而不是一律重启。
- **停内核时先交还代理。** `stop_if_running_node` 里 `proxy.disable()` 在 `runtime.stop()` 之前；否则注册表会指着一个已经没有监听的端口，用户表现为「断网」。

**四个坑**：
1. **节点表格没有 `DataItem`。** 订阅页的行是 `DataItem`，节点页的行是 `ControlType.Text`（`Local-WS | 435,586 90x29`），行内按钮离名字约 160px；先按名字找到 Text 的 `Top`，再取 `Top >= 该值` 里最小的那个按钮，才不会删到上一行。
2. **「刷新」要有真东西可刷。** 订阅源走代理（`subscription_via_proxy=true`）时，假订阅源在 `127.0.0.1:8898`，请求却从内核代理出去——而测试节点指向文档保留网段，永远连不上。验收前必须把 `subscription_via_proxy` 关掉，否则刷新失败、`last_node_id` 不变，看起来像「没有接手」。
3. **内容不变时 ID 也不变。** 同一份订阅原样再刷一次，行会被整体替换但 ID 依旧（`unique_suffix` 在同一毫秒内递增），此时不该出现接手动作；要验证接手，必须让节点的**位置或数量**变化（本次把四个节点倒序，过滤后仍在名单里的「香港 01」拿到新 ID）。
4. **`store.json` 有 BOM，写回时要保留。** 用 `json.loads` 读、`json.dump` 写会把 BOM 丢掉，应用随后把整份 store 当成损坏——脚本里读原始字节、判 BOM、写回时补回 `ï»¿`。

## 36. 保存系统代理开关，正在跑的内核立刻照办（2026-11-24，云电脑 cloudpc-test）

**验法**：`bash /d/tmp/shots/proxy-switch-accept.sh`（启动内核 → 设置页把「接管系统代理」关掉 → 保存 → 读注册表 → 再打开 → 保存 → 读注册表 → 改「代理例外」→ 保存 → 读 `ProxyOverride` → 还原 → 停止内核）。

这一节管的是「设置说保存了，但正在跑的内核还按旧设置活着」。以前 `system_proxy` 和 `proxy_bypass` 只在启动内核时被读一次：内核在跑的时候把「接管系统代理」关掉、点保存，页面下方写着「系统代理当前 已关闭」，注册表里却还指着 `127.0.0.1:7890` —— 关掉内核后整台机器直接断网。改「代理例外」同理，要等下一次启动才生效。

| 判据 | 验法 | 结果 |
|---|---|---|
| 启动时照旧接管 | `TAKEN ProxyEnable=1 ProxyServer=127.0.0.1:7890` | **通过** |
| 关掉并保存，立刻交还 | `RELEASED ProxyEnable=0 ProxyServer=` | **通过** |
| 交还后内核没被杀 | `kernels still: 1` | **通过** |
| 再打开并保存，立刻接管 | `RETAKE ProxyEnable=1 ProxyServer=127.0.0.1:7890` | **通过** |
| 改代理例外，立刻写进注册表 | `override now: …;*.example.com` | **通过** |
| 换回默认例外 | `restored override: …;<local>`（`*.corp.test` 已被覆盖） | **通过** |
| 停止后一切归零 | `after stop: ProxyEnable=0 ProxyServer=`，`kernels after stop: 0` | **通过** |

**三个设计理由**：
- **这件事必须在上游改，不能只加一句提示。** 端口、日志级别改不动时内核是「跑得挺好但配置旧」，提醒一下让人自己重启就够了；系统代理改不动是**断网**：交还的动作属于保存路径，不属于「下次再说」。所以 `save_settings` 在写盘之后、`refresh_tray` 之前，按 `takeover_change(before, after)` 直接 `enable_with_bypass` 或 `disable`。
- **用正在跑的那个端口，不用设置里存的那个。** 刚把混合端口从 7890 改到 7891 再保存，此刻内核还在 7890 上；如果按保存后的端口接管，注册表就会指向一个没人监听的端口。所以取 `status.mixed_port`，为 0 时才退回 `settings.mixed_port`。
- **接管时重新断言，不重新快照。** `ProxyGuard::enable_with_bypass` 在已经持有代理时只重写值、不再拍快照；否则每改一次例外清单就覆盖一次「用户原本的设置」，退出时还原的就不是原样了。

**三个坑**：
1. **`ProxyEnable=1` + 空 `ProxyServer` 仍然是坏状态。** 第一轮验收脚本没真按下开关（见坑 2），于是「交还」后读到 `ProxyEnable=1 ProxyServer=` —— 注册表开关还开着、地址却空了，等于每条请求都往一个空代理送。判据必须同时看开关和地址，不能只看地址为空。
2. **「接管系统代理」在 UIA 里是 `Button`，不是 `CheckBox`。** 名字和旁边的说明文字重名（`Text | 接管系统代理` 在左边），按名字取元素要加 `AndCondition(ControlType=Button)`；`Buttons-Named` 返回的是带坐标的包装对象，`GetCurrentPattern` 要用它内部的 `.El`，否则抛「[PSCustomObject] 不包含名为 GetCurrentPattern 的方法」。
3. **`Get-Win.FindAll(...)` 会被 PowerShell 当成命令名。** 写 `foreach ($e in Get-Win.FindAll(...))` 直接静默失败（函数返回空、异常被 catch 吞掉），必须先 `$win = Get-Win` 再 `$win.FindAll(...)`；同理找 `Edit` 时比较 `ControlType` 要用 `.Id`，对象 `-ne` 永远为真，会把所有元素都 `continue` 掉。

## 37. 「内核还在用旧设置」只说一遍，但把四件事都说全（2026-11-24，云电脑 cloudpc-test）

**验法**：`bash /d/tmp/shots/stale-all-accept.sh`（读 store 里的四个值 → 首页启动内核 → 设置页同时改端口、日志级别、局域网连接、DNS → 保存 → 读提示行 → 点「用新设置重启内核」→ 读 pid / 注册表 / 渲染出来的 `configs/xray.json` → 还原设置 → 停止内核）。

§33 和 §34 各给了自己一条提示：端口一条、日志级别一条，放在各自面板旁边。这样做有两个毛病：同一页可能出现两条几乎一样的话；而同样被写进渲染配置的 `allow_lan` 和 `dns_servers` 却一个字都不说——把「允许局域网连接」打开并保存，界面显示已开启，内核还在只听 `127.0.0.1`；把 DNS 改成 `9.9.9.9` 并保存，内核还在用 `1.1.1.1`。这一节把四件事合成一条提示，并把后两件也接上。

| 判据 | 验法 | 结果 |
|---|---|---|
| 四项都在提示里 | `stale: 内核还在用旧设置：端口、日志级别、局域网连接、DNS`，`items: port=True log=True lan=True dns=True` | **通过** |
| 只有一条 | `stale lines: 1` | **通过** |
| 提示带一键重启 | `restart button: True` | **通过** |
| 重启真的换了进程 | `pids before=9072 after=8968` | **通过** |
| 新端口接管了系统代理 | `proxy now: ProxyEnable=1 ProxyServer=127.0.0.1:7891` | **通过** |
| 新日志级别进了渲染配置 | `render loglevel: warning` | **通过** |
| 新 DNS 进了渲染配置 | `render dns has 9.9.9.9: True` | **通过** |
| 局域网监听跟着变 | `render lan listen 0.0.0.0: True` | **通过** |
| 重启后提示自己消失 | `stale after restart: 0` | **通过** |
| 停止后一切归零 | `after stop: ProxyEnable=0 ProxyServer=`，`kernels after stop: 0` | **通过** |

**三个设计理由**：
- **一条提示，按面板顺序列名字。** `staleItems` 返回的是 i18n key 数组（端口 → 日志级别 → 局域网连接 → DNS），`staleMessage` 把它们用「、」连起来塞进 `settings.staleList` 的 `{items}`。页面上只有一个 `<StaleConfig>`、一个重启按钮，位置在设置页顶部：它是「整页与内核不一致」，不属于某一个面板。
- **内核报出「它启动时用的是哪一份」。** 判断依据不是重读设置，而是 `EngineStatus` 里新增的 `allow_lan` / `dns_servers`（连同已有的 `mixed_port`、`log_level`），它们由 `start()` 在 `with_options` 吃掉 options **之前**抄下来存进 `Running`。DNS 存的是设置里的**原文**（`RunOptions::dns_servers_raw`），因为解析后的 `DnsEntry` 列表拿不回空格和结尾逗号，前端没法比对。
- **比对按内容，不按写法。** `dnsList()` 把 `1.1.1.1, 8.8.8.8` 和 `1.1.1.1 ,8.8.8.8` 归一成同一个值，否则用户只是多打一个空格就会被提示「内核用着旧设置」。

**两个坑**：
1. **`status === null` 必须一起挡。** 早期写法是 `engine.status?.allow_lan`，状态还没拉回来时 `undefined !== false`，四项会一起变成「旧设置」，页面刚打开就闪一条提示。现在的 guard 是 `!engine.running || !store.loaded || saved === null || status === null`。
2. **改完四项要按顺序等界面。** 端口是 `Spinner`（`SetValue` 后约 500ms 才落到 `draft`）、日志级别是 `Button`（不支持 `InvokePattern`，要滚动到可见再点矩形中心）、局域网连接是开关按钮、DNS 是 `Edit`——四个控件的驱动方式各不相同；全部改完再按一次「保存」，否则前面的改动会被后一次保存前的 `draft` 覆盖。
## 38. 多路复用：一条开关同时把手伸进两个内核（2026-11-24，云电脑 cloudpc-test）

**验法**：`bash /d/tmp/shots/mux-accept.sh`（读 store 里的 `mux_enabled`/`mux_concurrency` → 首页启动内核 → 确认渲染配置里没有 mux 块 → 设置页打开「多路复用」、连接数改成 12 → 保存 → 读提示行 → 点「用新设置重启内核」→ 读 pid、注册表、`configs/xray.json` 里的 mux 块 → 关掉开关再保存、再重启，确认块消失 → 停止内核）。

V2rayN 和 Clash 都把 mux 当普通开关放在设置里；本项目此前完全没有这个概念（`Node` 结构里没有 mux 字段，两个渲染器也没写过 `mux`/`multiplex`）。这里把它做成**全局设置**而不是逐节点参数：它影响的是「这个内核怎么和服务器说话」，和端口、日志级别属于同一层，也就顺带进了 §37 的那一条提示。

| 判据 | 验法 | 结果 |
|---|---|---|
| 开关打开、连接数改 12 能存下来 | `store after save: mux=true concurrency=12` | **通过** |
| 提示里点名多路复用 | `stale: 内核还在用旧设置：多路复用`（`item mux: True`） | **通过** |
| 提示带一键重启 | `restart button: True` | **通过** |
| 重启真的换了进程 | `pids before=8600 after=1676` | **通过** |
| 重启后提示自己消失 | `stale after restart: 0` | **通过** |
| Xray 配置里出现 mux 块 | `render mux enabled: 4`、`render mux concurrency 12: 4` | **通过** |
| 关掉开关保存后需要重启才生效 | `stale back: 1` | **通过** |
| 重启后 mux 块整体消失 | `render mux keys restored: 0` | **通过** |
| 系统代理不受影响 | `proxy now: ProxyEnable=1 ProxyServer=127.0.0.1:7890` | **通过** |
| 停止后一切归零 | `after stop: ProxyEnable=0 ProxyServer=`，`kernels after stop: 0` | **通过** |

**三个设计理由**：
- **「要不要写这块」只在一处决定。** `RenderInput::mux_streams() -> Option<u16>` 是唯一的判断点：`None` 表示用户没开，两个渲染器就不写这个块；`Some(n)` 才写。两个内核对这个块的拼法压根不同（Xray 用 `"mux": {"enabled": true, "concurrency": n}`，sing-box 用 `"multiplex": {"enabled": true, "protocol": "smux", "max_streams": n}`），但「开/关」这件事只有一个答案源，避免一个内核开了另一个没开。
- **关的时候整块不写，而不是写 `enabled: false`。** 两个内核都把 `enabled: false` 当成「用户显式关掉」并保留其余字段，配置里凭空多一块会让「内核用的是什么」比实际更难读；「没写」才是「没这个概念」。验收里 `render mux keys restored: 0` 就是这个意思。
- **连接数跟着开关一起校验，范围 1..=128。** 0 会让用户以为开了却什么都不做，128 之后每条流都挂在同一条可能卡住的链路上；`validate()` 直接拒掉而不是悄悄改成默认值——存进去的东西必须是用户看到的那个。

**两个坑**：
1. **开关在 UIA 里是 `Button`，但 `Buttons-Named` 返回的是包装对象。** 直接把它交给 `GetCurrentPattern` 会抛「[PSCustomObject] 不包含名为 GetCurrentPattern 的方法」；要么取 `.El`，要么让调用方统一拿元素本身。另外这个面板在页面下方，点之前必须 `ScrollItemPattern.ScrollIntoView()`，否则点到的坐标落在别的面板上。
2. **连接数输入框在开关关闭时是 `disabled`。** 先设数字再开开关会报「对于未启用的元素，不允许执行此操作」，正确的顺序是「开开关 → 再 SetValue」；反过来关的时候也一样，先关再改数字不会自动保存。

**一句实话**：本节证明的是「开关 → 渲染配置 → 重启后生效」这条链路，不证明 mux 真的让连接更快——那要看线路，云电脑上只有一个跑不通的测试节点，测不出来。

## 39. 分片（TLS fragmentation）：让 ClientHello 拆成几块走出去（2026-11-24，云电脑 cloudpc-test）

V2rayN 有「分片」，Clash 侧靠内核插件做同类事；本项目的 `Security::Tls` 一直只写 sni/alpn/证书钉，配置里根本没有 `fragment` 这一块。做了全局开关而不是逐节点参数，因为它和端口、日志级别、多路复用一样属于「内核怎么说话」这一层，也顺带接进了 §37 的那一条提示（`settings.stale.itemFragment`）。

**验法**：`bash /d/tmp/shots/fragment-accept.sh`（前置脚本 `fragment-patch.ps1` 把 `last_node_id` 指到 TLS 节点 `Clip-Test`，并把开关先摆成关）

**判据**（真机日志 `C:\Users\Administrator\fragment-accept.txt`）：

| 判据 | 真机证据 | 结果 |
| --- | --- | --- |
| 开关默认关时配置里没有分片块 | `render fragment keys before: 0` | **通过** |
| 打开开关能改长度和间隔 | `set length 120-220: True`、`set interval 20-40: True` | **通过** |
| 保存后落盘的是用户写的范围 | `store after save: fragment=true length=120-220 interval=20-40` | **通过** |
| 提示里点名分片 | `stale: 内核还在用旧设置：分片`（`item fragment: True`） | **通过** |
| 提示带一键重启 | `restart button: True` | **通过** |
| 重启真的换了进程 | `pids before=4672 after=1464` | **通过** |
| 渲染出的就是 Xray 认的形状 | `render fragment key: 1`、`render length 120-220: 1`、`render interval 20-40: 1`、`render packets tlshello: 1` | **通过** |
| 带着分片块也能起来（即 schema 合法） | `kernel alive after fragment restart: 1` | **通过** |
| 重启后提示自己消失 | `stale after restart: 0` | **通过** |
| 关掉保存后同样要先重启 | `stale back: 1` → 重启后 `render fragment keys restored: 0` | **通过** |
| 系统代理不受影响 | `proxy now: ProxyEnable=1 ProxyServer=127.0.0.1:7890` | **通过** |
| 停止后一切归零 | `after stop: ProxyEnable=0 ProxyServer=`，`kernels after stop:`（空） | **通过** |

**三个设计理由**：
- **只在 `Security::Tls` 里写。** Xray 的 `RealityObject` 没有 `fragment` 字段，写进去整份配置会被内核判为非法；`Security::None` 连 `tlsSettings` 都不存在，更谈不上分片。渲染器只改 Tls 那一个分支，Reality/None 各有测试断言「这个键必须不在」。
- **`packets` 钉死 `tlshello`。** 这个值是「拆 ClientHello」的唯一语义，其余拼法在内核里没有定义；把它写成常量而不是设置项，避免用户把功能改成什么都不做。
- **范围在设置层就校验，不在渲染时兜底。** `parse_positive_range` 要求 `a-b` 且 `a<=b`、都不为 0，`validate()` 直接在保存时拒绝（`分片长度要写成 a-b 这样的范围，例如 100-200`）。渲染器拿到的永远是用户写的那串字面量，配置和界面不会背地里变成两个值。

**两个坑**：
1. **开关的 UIA Name 是整句标签，不是那两个字。** `分片（TLS fragment）` 这样的名字意味着 `Buttons-Named` 的精确匹配找不到它；要么按子串找，要么把标签写在 `AutomationProperties.Name` 上。本节第一次跑就死在 `toggle found: False`。
2. **旧 store 里不会自动长出这三个键。** `#[serde(default)]` 只在反序列化时补默认值，存回去要等用户真的按一次「保存」；所以验收脚本得先跑一次保存，或者像这里一样显式 patch 后再比对，否则 `fragment=absent` 会被误读成「功能没生效」。

**一句实话**：本节证明的是「开关 → 渲染配置 → 内核接受这份配置并起来」，不证明分片真的能绕过谁的封锁——那要看对面是谁，云电脑上只有一个连不通的测试节点，测不出来。

## 40. 改节点改的是正在跑的那个：先说出来，再重启（2026-11-24，云电脑 cloudpc-test）

**这一节证明什么**：编辑「内核正在用的那个节点」的服务器/端口/传输/安全参数并保存，界面会明说「内核还在用旧的一份」，并且那个按钮真的用新配置把内核重启了；改名、改标签这类不影响渲染的改动则不该惊动内核。

**验收方法**：把 `settings.last_node_id` 钉到 TLS 测试节点 `Clip-Test`（`example.org:443`，vless + TLS），然后

```bash
bash /d/tmp/shots/nodestale-accept.sh
```

脚本流程：首页「启动」→ 节点页找到 `Clip-Test` 行 → 「编辑」→ 端口 443 改成 8443 → 「保存」→ 读提示 → 点「用新配置重启内核」→ 核对 pids、系统代理、`configs/xray.json` 里的端口 → 端口改回 443 → 「停止」。

| 判断什么 | 看到什么 | 结论 |
| --- | --- | --- |
| 存下去的是新端口 | `store after save: port=8443` | **通过** |
| 界面明说内核在跑旧的 | `notice present: True`，`notice text: 节点改好了…内核…用旧的一份` | **通过** |
| 提示带一键重启 | `buttons after save` 里有 `[用新配置重启内核]`，`apply button: True` | **通过** |
| 重启真的换了进程 | `kernels after restart: xray#1008 before=xray#1992` | **通过** |
| 渲染出的端口就是新的 | `rendered server port: 8443` | **通过** |
| 重启后提示自己消失 | `notice gone: True` | **通过** |
| 系统代理不断 | `proxy after restart: ProxyEnable=1 ProxyServer=127.0.0.1:7890` | **通过** |
| 改回旧值同样要先重启 | `store restored: port=443` → `rendered server port same: 8443`（内核仍是上一次渲染） | **通过** |
| 停止后一切归零 | `stop invoked on try 0`，`after stop: ProxyEnable=0 ProxyServer=` | **通过** |

**三个设计理由**：
- **只比「渲染得出来」的字段。** `src/services/node-stale.ts` 的 `rendersDifferently()` 只看 `server`/`port`/`protocol`/`transport`/`security`；`name` 和 `tags` 不参与。理由是改名、打标签属于整理工作，不该因为用户顺手改个名字就把所有连接断掉一次。
- **判断依据是内核自己报的 node id。** `NodesView.vue` 里 `markStaleNode()` 要求 `engine.status?.node_id === saved.id` 才提示：改一个没在跑的节点当然不用重启，提示只在「改的正好是内核在用的那个」时出现。
- **复用同一块提示组件。** `StaleConfig` 的 `message`/`applyLabel` 两个可选 prop 让节点页说出自己的话（`节点改好了，内核还在用旧的一份` + `用新配置重启内核`），但清除逻辑仍然只有一份：`started_at_ms` 一变就消失，任何来源的重启都算数。

**三个坑**：
1. **节点表格的行名是 `ControlType.Text`，不是 `DataItem`。** 行内的按钮（`启动`/`编辑`/`删除`…）在名字下方约 164px 处，所以按「最近的按钮」找会命中上一行；要么用 `Top >= 行首` 里最小的那个，要么先滚动到行再点。
2. **编辑窗口不是独立窗口。** `Get-Dialog`（找 `#32770`）在 Tauri 这里一无所获，整个对话框都是 `Tauri Window | X-Matrix` 的子树；`保存` 按钮的 UIA 名字是完整的 `保存`，早先看到的 `�存` 只是控制台编码把中文啃掉了，不要据此改用子串匹配。
3. **`configs/xray.json` 是多行缩进的 JSON。** 用 `"address": "..." ... "port": N` 这样的正则去核对时，`.{0,120}?` 跨不过换行，必须加 `(?s)`，否则永远读到 `absent`，看起来像功能没生效。

**一句实话**：本节证明的是「改的是正在跑的那个节点 → 界面说实话 → 按钮真的按新值重启」，不证明新端口上的节点连得通——云电脑上只有一个故意连不通的测试节点。

---

## 41. 全局快捷键可以改、可以关，改完立刻生效（2026-11-24，云电脑 cloudpc-test）

**要证明的事**：设置页里那两个快捷键不再是写死的 `Ctrl+Shift+X` / `Ctrl+Shift+W`——能被重新录制、能点「关掉这一项」取消、保存后**不用重启应用**就换过来，重启之后仍然是新的，两个键不能相同，写错了要当场拒绝。

**验法**：
```bash
bash /d/tmp/shots/hotkey-accept.sh
```
脚本先把云电脑 `store.json` 里的两个键归位成出厂值（`prep-store.ps1`），再启动应用：点一下录制按钮 → 用 `keybd_event` 按下 `Ctrl+Alt+F9` → 保存 → 记录内核 pid → 再分别按新键、旧键、新键（第二次是停）→ `--quit` 重启 → 按新键 → 清空窗口快捷键 → 按旧窗口键 → 两个「恢复默认」→ 保存 → 按出厂键启停。

| 判据 | 证据（`hotkey-accept.txt`） | 结果 |
|---|---|---|
| 出厂状态下两个键是 `Ctrl+Shift+X` / `Ctrl+Shift+W` | `store before: toggle='Ctrl+Shift+X' window='Ctrl+Shift+W'`、`chips before: Ctrl+Shift+X , Ctrl+Shift+W` | **通过** |
| 录制按钮吃下真按键 | `click toggle chip: Ctrl+Shift+X` → `sent new chord: True` → `chips after record: Ctrl+Alt+F9 , Ctrl+Shift+W` | **通过** |
| 保存落库 | `store after save: toggle='Ctrl+Alt+F9' window='Ctrl+Shift+W'` | **通过** |
| **不重启就生效** | 保存后直接按新键：`kernels after new chord: xray#3652` | **通过** |
| 旧键立刻失效 | `kernels after old chord: xray#3652 (was xray#3652)`（pid 没变=没被按到） | **通过** |
| 同一个键能停 | `kernels after second new chord: none` | **通过** |
| 重启后仍是新键 | `relaunched: True` → `kernels after restart chord: xray#5940` | **通过** |
| 窗口快捷键照常藏/显 | `window chord: visible before=True hidden=False shown=True iconic=False` | **通过** |
| 关掉窗口键后它彻底无效 | `store after clear window: window=''` → 按旧键 `window visible after old window chord: True`（窗口没被藏） | **通过** |
| 恢复默认把两行都还原 | `chips after reset 0: Ctrl+Shift+X , 未设置` → `chips after reset 1: Ctrl+Shift+X , Ctrl+Shift+W` | **通过** |
| 出厂键又能用 | `kernels after default chord: xray#5168` → `kernels after stopping by default chord: none` | **通过** |

后端 stderr 也证明注册发生在保存那一刻，而不是下次启动：启动时两行 `全局快捷键 Ctrl+Shift+X 已就绪` / `Ctrl+Shift+W 已就绪`；点保存后立刻多出 `全局快捷键 Ctrl+Alt+F9 已就绪` + `Ctrl+Shift+W 已就绪`。

**三个设计理由**：
- **快捷键以纯文本存。** `Settings.hotkey_toggle` / `hotkey_window` 就是 `Ctrl+Alt+F9` 这种字符串，人能读、能手抄、能进备份文件；`Combo::parse()` 接受宽松写法（`ctrl+shift+x`、`meta+space`）但落库和日志里统一成一种写法（`Ctrl+Alt+Shift+Win+<键>` 顺序），所以设置页、日志、文档说的是同一个字符串。
- **用「按一下」录，而不是让人手打。** 大小写、修饰键顺序、`F1–F24` 这些最容易写错的地方交给键盘本身；`comboFromKeyPress()` 只做翻译，真正能不能用由 Rust 端 `parse_optional()` 说了算，写错了保存就被拒绝并指出是哪个词。
- **注册是尽力而为。** `RegisterHotKey` 失败（这个键被别的程序占着）只写一行日志，不拦保存——能不能保存设置，不该取决于当时有没有别的软件占着这个键。重注册通过 `WM_APP_REREGISTER` 投递给热键消息线程，在 `GetMessageW` 那一边做，所以保存后立刻生效。

**四个坑**：
1. **同一个 `Mutex` 自锁。** 第一版 `window_proc` 在 `WM_APP_REREGISTER` 分支里先 `let guard = BINDINGS.lock()` 再调 `register_all()`，而 `register_all()` 里又 `BINDINGS.lock()`——非可重入锁当场死锁，偏偏这是唯一能注册热键的线程。现象极具迷惑性：旧键已经被 `UnregisterHotKey` 掉，新键没注册上，重启应用又恢复正常。现在 `register_all()` 只取一次快照再注册，锁不跨 `RegisterHotKey`。
2. **`aria-label` 会把按钮的可访问名顶掉。** chip 上写了 `aria-label="点一下，再按下新的组合键"`，UIA 树里两个 chip 都叫这句提示，读不到真正的快捷键；改成 `:title` 后 Name 就是快捷键本身（对读屏用户也更有用）。
3. **`恢复默认` 在设置页有四个。** 前两个属于内核二进制那两行，按名字点会点到它们；验收脚本按文档顺序取第 3、4 个，并且每一步都回读 chip 文本确认改的是快捷键。
4. **被快捷键藏起来的窗口会从 UIA 树里消失。** `Get-Win` 返回 null，后面每一步都报「找不到」；要用 `ShowWindow(hwnd, SW_RESTORE)` + `SetForegroundWindow` 把窗口拉回来再继续。另外注册时必须带 `MOD_NOREPEAT`（0x4000），否则按住不放会连发。

**一句实话**：本节证明「录进去的键真的注册上了、旧键真的失效、重启后仍然是新键、关掉就真的没有」，不证明所有键盘布局下都能录——验收只用过 `Ctrl+Alt+F9` 这种 ASCII 组合。

## 42. 第三种语言是真的：选一下、重启、再换回来（2026-11-24，云电脑 cloudpc-test）

**验法**：

```bash
bash /d/tmp/shots/lang-accept.sh
```

脚本启动应用后，先按当前语言找到「设置」页，点 `繁體中文`，回读 `store.json` 的 `language`、当页文字与按钮名；再点侧栏回首页，确认按钮叫 `啟動`；随后 `--quit` 重启，确认窗口一出来就是繁体（没碰过语言选择器）；最后点 `简体中文` 并核对首页按钮叫 `启动`。

| 判据 | 证据（`lang-accept.txt`） | 结果 |
|---|---|---|
| 出厂是简体 | `store language before: 'zh'` | **通过** |
| 选择器有三项 | `locale buttons: 简体中文 / 繁體中文 / English` | **通过** |
| 点一下就落库 | `picked zh-tw: True` → `store language after zh-tw: 'zh-tw'` | **通过** |
| 页面文字立刻换 | `save button is 儲存: True`、`texts zh-tw: X-Matrix \| 設定 \| 埠、模式與系統集成 \| 本地混合埠 \| …` | **通过** |
| 侧栏与首页跟着换 | `home nav in zh-tw: True`、`start button in zh-tw: True`、`settings nav in zh-tw: True` | **通过** |
| 提示语也换了词 | `save hint zh-tw: True`（含「托盤選單」） | **通过** |
| 重启后仍是繁体 | `language after quit: 'zh-tw'` → `relaunched: True` → `after restart texts: X-Matrix \| 首頁 \| 運行狀態總覽 \| 未運行 …` | **通过** |
| 重启后侧栏/首页仍是繁体 | `after restart home in zh-tw: True`、`after restart settings in zh-tw: True` | **通过** |
| 还能换回简体 | `picked zh: True` → `store language after zh: 'zh'` → `save button is 保存: True`、`home nav back in zh: True` | **通过** |

**三个设计理由**：
- **一份键表三个译者。** `scripts/i18n-pairs.json` 每行四格 `[key, zh, en, tw]`，`gen_i18n.py` 是唯一写 `src/i18n/*.ts` 的人；繁体列由简体列机械转换（`zhconv` + 台湾用词表：`默认→預設`、`设置→設定`、`文件夹→資料夾`、`文件→檔案`、`端口→埠`、`保存→儲存`、`服务器→伺服器`、`配置→設定` …），不是逐键手翻。这样三份字典的键集永远一致，`i18n.test.ts` 能用 `keyDiff` 直接卡住漏译。
- **语言是一个设置项，不是前端的私事。** 第一版只写了 `localStorage`，托盘菜单和内核退出通知（都在 Rust 里）一律说中文。现在选择器调 `set_language` 命令：写 `settings.language`、持久化、重建托盘；前端再镜像到 store，所以「设置页显示的语言」和「托盘说的话」不会打架。没走 `save_settings` 是因为它收整份设置文档，会把用户正在输入的端口半截值一起存下去。
- **`zh-tw` 这个码要显式写。** Rust 的 `Language` 用 `#[serde(rename_all = "lowercase")]`，`ZhTw` 会被写成 `zhtw`，而前端字典键是 `zh-tw`；所以那一个变体单独 `#[serde(rename = "zh-tw")]`，并用注释（不是文档注释，specta 会把 `///` 抄进 bindings 导致契约检查失败）说明原因。

**四个坑**：
1. **语言 chip 不支持 `InvokePattern`。** 它是普通 `Button`，`GetCurrentPattern` 抛「不支持的模式」；必须按它自己的矩形真点一下（`Press` 里先 `Activate` 再点两次，第一次只负责聚焦）。
2. **刚重启的窗口会先应答 UIA、再画第一帧。** 重启后立刻读文字会得到空表，看起来像「语言丢了」；`Wait-Text` 轮询到有文字为止（这次约 2 秒，冷启动时可能要 20 秒）。
3. **首页按钮叫 `啟動`，不是 `啟動內核`。** `啟動內核` 是托盘菜单的措辞；用错签名会让「首页在繁体下正常」误判为失败。
4. **不是所有译文都该跟简体不同。** 最初加了一条「繁体值不得等于简体值」的断言，`sub.status.ok`、`time.yesterday` 这类纯 ASCII/纯符号键当场命中——一样的字才是对的。现在只对少数 Taiwan 用词做点检（`首頁`、`儲存`、`按延遲`、`設定備份`）。

**一句实话**：本节证明「选择器能落库、三处界面（设置页/侧栏/首页）立刻改用繁体、重启后仍然是繁体、还能换回简体」，不证明 750 条译文的中文质量——繁体列是机械转换加用词表，个别句子可能仍有大陆语序，需要母语者再读一遍。托盘菜单的繁体措辞由 `tray.rs` 的单测（`every_language_names_the_menu_in_its_own_words`）保证，本节没有真的去点托盘。

## 43. 不启动内核也能测真实延迟：起一个临时内核，打一枪就撤（2026-11-24，云电脑 cloudpc-test）

**验法**：

```bash
bash /d/tmp/shots/offline-accept.sh
```

云电脑上先用 sing-box 在本机起一个 shadowsocks 服务端（`127.0.0.1:8388`，`aes-128-gcm`，口令 `xm-test`），`prep-offline.ps1` 往 store 顶部放两个节点：`SS-Local`（指向这个真服务端）与 `SS-Dead`（指向没人听的 `127.0.0.1:9999`），探测地址设为 `http://cp.cloudflare.com/`。脚本启动应用，在节点页对两行分别点「真实延迟」，回读界面文字与 `store.json` 的 `probes`。

| 判据 | 证据（`offline-accept.txt`） | 结果 |
|---|---|---|
| 出厂没有测量记录 | `probe before SS-Local: none` | **通过** |
| 不启动主内核，点一下就量出数 | `pressed 真实延迟 below 586 at 668,772` → 界面 `SS-Local \| Shadowsocks \| 127.0.0.1:8388 \| TCP \| 539 ms · 刚刚` | **通过** |
| 结果以「真实延迟」身份落盘 | `probe after SS-Local: "ss-offline-local-1": { "delay_ms": 539, "at": 1795499889, "kind": "real" }` | **通过** |
| 坏节点失败得诚实 | `SS-Dead` → 界面 `经代理请求返回 HTTP 503` → `"ss-offline-dead-1": { "delay_ms": null, "kind": "real" }`（测不到也记下来） | **通过** |
| 主内核在跑时这排按钮退场 | `kernels: sing-box#2480,xray#628` → `real delay buttons while running: 0` | **通过** |
| 事后不留痕 | `dir probe-*.json` 为空、无 xray/sing-box 残留进程、`ProxyEnable=0` | **通过** |

**三个设计理由**：
- **临时内核是一次性的，不是第二个应用。** `engine/temp.rs` 每次自己挑两个空闲端口、渲染一份只含该节点的配置（`mode = Global`，`nodes` 只有它一个）、写进 `configs/probe-<纳秒>.json`、起 `engines/<内核>.exe`，拿到数就 `kill` + 删文件。`Cleanup` 带 `Drop`，所以任何一个 `?` 提前返回都不会留下跑着的内核或临时配置（已用 `dir` 核对）。
- **拒绝 ≠ 失败。** 命令先把「内核正在运行」「节点不存在」「所选内核不支持这个协议」这类**没测**的情况退回去（不写记录），只有「内核起来了、请求失败了」才写 `delay_ms: null`。这和 §26 出口 IP 是同一条原则：没发生的测量不能覆盖上一次真的数字。
- **用配置里的内核，而不是写死 Xray。** 选 sing-box 的用户测 hysteria2/tuic 节点时，Xray 根本没有对应出站；所以临时内核按 `settings.last_engine` 走，并在起内核之前用 `validate_node()` 把不支持的协议拦下来，报错和首页启动时那句一模一样。

**四个坑**：
1. **`Global` 模式仍然会写一条路由规则。** 想「配置里没有 rules」是做不到的：`xray_routing_rules`/`singbox_routing_rules` 在 Global 下正好写一条兜底（Xray 叫 `outboundTag`、sing-box 叫 `outbound`，都指向 `proxy`），这条兜底正是保证探测不会绕过去走 `direct` 的东西；测试断言的是「恰好一条、指向 proxy、没有用户匹配器」。
2. **节点列表会把屏幕外的行从 UIA 树里拿掉。** 验收第一版把两个测试节点追加在 10 个节点末尾，`Row-Top` 读到 2863，按它点等于点到屏幕外；把测试节点放到 store 顶部后一切正常。真实用户场景没问题（他们滚到哪就在哪看），但验收必须让被测行看得见。
3. **死的 shadowsocks 节点不会「连接失败」，而是 HTTP 503。** Xray 对连不上的出站回一个 503，所以错误文案是「经代理请求返回 HTTP 503」而不是超时；这条同样证明请求真的走了隧道（否则直连 `cp.cloudflare.com` 会 204）。
4. **主机上的测试服务端会被当成「内核」。** 验收脚本用 `Kernels` 数进程，本机的 `sing-box.exe` 服务端也会被数进去（`sing-box#2480,xray#628`）；判断「主内核在不在」要看那条 xray 记录，别数总数。

**一句实话**：本节证明「临时内核真的起得来、探测真的走它的隧道、失败真的被记下来、事后真的不留痕」，不证明它比 539 ms 这个数字更快或更准——`539 ms` 里包含一次本机 shadowsocks 中转与一次公网 204，绝对值没有意义，有意义的是「同一条链路下它是唯一把隧道算进去的数字」。

## 44. 换代理模式也要说一声：三个按钮落盘后，内核还按老表在跑（2026-11-24，云电脑 cloudpc-test）

**要证的事**：设置页把「代理模式」从「规则」改成「全局」并保存时，如果内核正在跑，界面要直说内核还用着旧的路由表；点提示里的按钮之后，新内核真的按新表起来（`configs/xray.json` 里的路由规则数从 3 条变成 1 条），提示消失；再改回「规则」并重启，表恢复成 3 条。

**验法**：

```bash
bash /d/tmp/shots/mode-accept.sh
```

`prep-mode2.ps1` 先把 store 归位（`mode = rule`、`system_proxy = true`，顺手清掉上一节残留的 `ss-offline-*` 节点，并把 `last_node_id` 指回一个真的存在的节点）。脚本启动应用、首页点「启动」、读当前渲染出的路由规则数，再去设置页点「全局」、保存，读提示文字与 store，点「用新设置重启内核」，重新读 pid 和规则数，最后改回「规则」重启一次。

| 判据 | 证据（`mode-accept.txt`） | 结果 |
|---|---|---|
| 内核在跑，旧表有 3 条规则 | `kernels: xray#7164`、`routing rules before: 3` | **通过** |
| 保存后立刻点名 | `store mode after save: global` → `stale: 内核还在用旧设置：代理模式`、`item mode: True` | **通过** |
| 旧内核没被偷偷换掉 | `routing rules while stale: 3`（保存只写意图） | **通过** |
| 点按钮真的重启 | `pids before=7164 after=2552` | **通过** |
| 新表生效 | `routing rules after restart: 1`（全局模式一条兜底） | **通过** |
| 提示自清 | `stale after restart: 0` | **通过** |
| 改回「规则」再重启，表恢复 | `stale back: 1` → `routing rules restored: 3` | **通过** |
| 全程代理没掉 | `proxy after restart: ProxyEnable=1 ProxyServer=127.0.0.1:7890`；停止后 `ProxyEnable=0` | **通过** |

**三个设计理由**：
- **模式由内核自己报，界面不回读设置。** `EngineStatus.mode` 和端口、日志级别、DNS、局域网、多路复用、分片走同一条路：`Running` 记下启动时用的值，`start()`/`status()` 都带着它。界面拿「已保存的设置」和「内核自报的值」比，才不会出现「刚保存就说已经生效」的谎话。
- **一句话列全所有落后项，而不是每条一个提示。** `staleItems` 按面板顺序拼（端口 → 日志级别 → 局域网 → DNS → 多路复用 → 分片 → 代理模式），用户一次就知道要重启一次解决哪些事。
- **提示的清除只认 `started_at_ms`。** 重启由谁发起都行（提示按钮、首页「重启」、托盘），新的 `started_at_ms` 一到，提示自己消失；不需要在每个入口手工清状态。

**四个坑**：
1. **`last_node_id` 可能指向已经不存在的节点。** 第一次跑时按钮点了毫无反应：`reapply()` 用的是设置里的镜像，而镜像里存着上一节清理时删掉的 `ss-offline-local-1`，`apply` 于是要求启动一个不存在的节点并抛错，错误只落在提示组件内部的 `role="alert"` 里。修法是先问内核（`status.node_id`，§35 就有了），镜像只当轮询还没回来时的兜底。
2. **三个模式按钮是普通 `Button`，`ItemStatus` 才是选中态。** 名字是「规则 / 全局 / 直连」，用 `InvokePattern` 能点动；判断当前选中不能看颜色，要看 `ItemStatus`。
3. **渲染出的配置里「全局」并不是「没有 rules」。** 全局模式正好一条兜底规则（`outboundTag` 指向 `proxy`），所以判据是「3 → 1 → 3」，不是「3 → 0 → 3」。
4. **验收脚本自己的启动顺序会骗人。** 计划任务建好不等于跑起来：`mode-accept.sh` 第一版忘了 `schtasks /run /tn xmmode`，日志文件根本没生成，界面却一切正常；脚本现在 `xmapp` 之后 `sleep 10` 再跑巡检任务。

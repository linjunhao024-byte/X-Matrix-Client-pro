# X-Matrix 引擎规格：代理内核管理（ENGINE-SPEC）

> 用途：为 X-Matrix（Tauri 2 + Rust 后端 + Vue 3 前端）重写「代理内核管理」层提供工程事实依据。
> 首发内核：**Xray**、**sing-box**。
>
> **来源约定**
> - `v2rayN: <相对路径>:<行号>` —— 指 `D:\GitHub-Warehouse\_refs\v2rayN\` 下的文件。
> - `clash-verge: <相对路径>:<行号>` —— 指 `D:\GitHub-Warehouse\_refs\clash-verge-rev\` 下的文件。
> - 行号以本次调研时的克隆版本为准。
>
> **许可提醒**：两个上游均为 GPL-3.0，本项目为 MIT。本文只记录**行为事实与数据结构**，不含可编译的源码搬用。文中出现的 JSON/C# 片段均为结构示意，重写时须按事实独立实现。
>
> **重要前提**：`clash-verge-rev` 只支持 mihomo（Clash Meta）内核，**不包含 Xray / sing-box 的配置生成能力**（`clash-verge: src-tauri/src/config/verge.rs:99,246-275`，内核名固定为 `verge-mihomo`；全仓 `grep v2ray|vless|sing-box|xray` 仅命中测试用样例 `clash-verge: src-tauri/src/enhance/seq.rs:161,260`）。因此第 1、2 节的事实**全部来自 v2rayN**，clash-verge 只贡献架构、IPC、进程生命周期与系统代理部分。

---

## 0. 术语与通用约定

| 概念 | 说明 | 来源 |
|---|---|---|
| 本地 mixed 入站 | 同时接受 SOCKS5 与 HTTP 的入站；两家内核都用 `mixed` 类型 | `v2rayN: v2rayN/ServiceLib/Services/CoreConfig/V2ray/V2rayInboundService.cs:165`、`v2rayN: v2rayN/ServiceLib/Services/CoreConfig/Singbox/SingboxInboundService.cs:18,98` |
| 代理出站 tag | 固定字符串 `"proxy"` | `v2rayN: v2rayN/ServiceLib/Global.cs:57` |
| 直连 / 阻断 tag | `"direct"` / `"block"` | `v2rayN: v2rayN/ServiceLib/Global.cs:58-59` |
| 本地回环 | `"127.0.0.1"` | `v2rayN: v2rayN/ServiceLib/Global.cs:66` |
| 负载均衡器 tag 后缀 | `"-balancer"` | `v2rayN: v2rayN/ServiceLib/Global.cs:63` |
| DNS 出站 tag / DNS 模块 tag | `"dns"` / `"dns-module"` | `v2rayN: v2rayN/ServiceLib/Global.cs:60-61` |
| 默认出站 email | `"t@t.tt"` | `v2rayN: v2rayN/ServiceLib/Global.cs:79` |
| 默认 VMess security | `"auto"` | `v2rayN: v2rayN/ServiceLib/Global.cs:50` |

### 0.1 端口规划（v2rayN 的偏移式分配，值得直接沿用）

基准端口 = 配置里 `socks` 入站的 `LocalPort`（默认 10808）；其余端口 = 基准 + 枚举值：

```csharp
enum EInboundProtocol { socks = 0, socks2 = 1, socks3 = 2, pac = 3, api = 4, api2 = 5, mixed = 6, speedtest = 21 }
```
来源：`v2rayN: v2rayN/ServiceLib/Enums/EInboundProtocol.cs:3-13`；计算方式 `v2rayN: v2rayN/ServiceLib/Manager/AppManager.cs:159-163`。

| 用途 | 公式 | 默认值 | 来源 |
|---|---|---|---|
| 主 mixed 入站 | `base + 0` | 10808 | `v2rayN: .../AppManager.cs:159-163` |
| 第二本地端口 | `base + 1` | 10809 | 同上 |
| LAN 独立端口 | `base + 2` | 10810 | 同上 |
| PAC 服务端口 | `base + 3` | 10811 | `v2rayN: .../Handler/SysProxy/SysProxyHandler.cs:113` |
| **Xray 指标/统计端口**（`StatePort`） | `GetFreePort(base + 4)` | 10812 起找空闲 | `v2rayN: .../AppManager.cs:14-21` |
| **sing-box Clash API 端口**（`StatePort2`） | `GetFreePort(base + 5) + (TUN ? 1 : 0)` | 10813/10814 | `v2rayN: .../AppManager.cs:23-30` |

注意 `StatePort` / `StatePort2` 是**惰性缓存 + 空闲端口探测**：只解析一次，进程内复用。重写时同样应把「配置声明的端口」与「实际可用端口」分开，避免 TUN 与 sidecar 抢同一端口。

---

## 1. Xray 配置

### 1.1 上游的基线模板（事实）

v2rayN 的 Xray 客户端配置**不是从零手写**，而是先反序列化一份内嵌样板，再逐段改写：
`v2rayN: v2rayN/ServiceLib/Services/CoreConfig/V2ray/CoreConfigV2rayService.cs:33-45`（`EmbedUtils.GetEmbedText(Global.V2raySampleClient)`）。

样板原文（`v2rayN: v2rayN/ServiceLib/Sample/SampleClientConfig`）：

```json
{
  "log": { "access": "Vaccess.log", "error": "Verror.log", "loglevel": "warning" },
  "inbounds": [],
  "outbounds": [
    { "protocol": "freedom", "tag": "direct" },
    { "protocol": "blackhole", "tag": "block" }
  ],
  "routing": { "domainStrategy": "IPIfNonMatch", "rules": [] }
}
```

入站样板（`v2rayN: v2rayN/ServiceLib/Sample/SampleInbound`）：

```json
{
  "tag": "tag1", "port": 10808, "protocol": "socks", "listen": "127.0.0.1",
  "settings": { "auth": "noauth", "udp": true, "allowTransparent": false },
  "sniffing": { "enabled": true, "destOverride": ["http", "tls"] }
}
```

> **事实**：基线模板里**没有** `dns` / `stats` / `api` / `policy` / `metrics` / `observatory` 节点，这些都是生成阶段按开关注入的。

### 1.2 最小可用 config.json（由各生成器产物拼接）

下面的合成结果对应「一个节点 + 一个本地 mixed 入站 + 统计开启」这一最简场景。它是把 §1.3 各生成器的实际输出逐段拼起来得到的**结构示意**，不是上游某个现成文件；每一段都有出处，拼装关系见标注。

```json
{
  "log": { "access": null, "error": null, "loglevel": "warning" },
  "inbounds": [
    {
      "tag": "socks",
      "port": 10808,
      "protocol": "mixed",
      "listen": "127.0.0.1",
      "settings": { "auth": "noauth", "udp": true, "allowTransparent": false },
      "sniffing": { "enabled": true, "destOverride": ["http", "tls"], "routeOnly": false }
    }
  ],
  "outbounds": [
    { "tag": "proxy", "protocol": "<节点协议>", "settings": { }, "streamSettings": { } },
    { "protocol": "freedom", "tag": "direct" },
    { "protocol": "blackhole", "tag": "block" }
  ],
  "routing": { "domainStrategy": "IPIfNonMatch", "rules": [] },
  "stats": { },
  "metrics": { "listen": "127.0.0.1:10812" },
  "policy": { "system": { "statsOutboundDownlink": true, "statsOutboundUplink": true } }
}
```

逐段出处：

| 段 | 事实 | 来源 |
|---|---|---|
| `log` | 按开关写 `loglevel`；关日志时 `access`/`error` 置 `null` | `v2rayN: v2rayN/ServiceLib/Services/CoreConfig/V2ray/V2rayLogService.cs:9-21` |
| `inbounds[0]` | `tag="socks"`、`port=base+0`、`protocol="mixed"`、`listen="127.0.0.1"`、`settings.udp` 跟随配置、sniffing 三项跟随配置 | `v2rayN: .../V2rayInboundService.cs:150-182` |
| `outbounds[0]` | 代理出站**插在数组最前面**（`InsertRange(0, …)`），tag 固定 `"proxy"` | `v2rayN: .../V2rayOutboundService.cs:5-8,62` |
| `outbounds[1..2]` | `freedom/direct`、`blackhole/block` 来自基线模板，保留在尾部 | `v2rayN: .../Sample/SampleClientConfig` |
| `routing` | `domainStrategy` 默认取 `IPIfNonMatch`（基线值），可被路由配置覆盖 | `v2rayN: .../V2rayRoutingService.cs:40,45-48`；`Global.cs:74` |
| `stats` | 空对象 `{}` | `v2rayN: .../V2rayStatisticService.cs:13` |
| `metrics` | `{"listen": "127.0.0.1:<StatePort>"}` | `v2rayN: .../V2rayStatisticService.cs:15-16` |
| `policy` | `{"system": {"statsOutboundDownlink": true, "statsOutboundUplink": true}}` | `v2rayN: .../V2rayStatisticService.cs:18-21` |

### 1.3 字段清单（Xray 顶层，含 v2rayN 是否实际使用）

模型定义：`v2rayN: v2rayN/ServiceLib/Models/CoreConfigs/V2rayConfig.cs:3-17`。

| 顶层字段 | 类型 | v2rayN 是否生成 | 说明 / 来源 |
|---|---|---|---|
| `log` | object | ✅ 总是 | `{access, error, loglevel}`，`v2rayN: .../V2rayConfig.cs:38-45`、`V2rayLogService.cs:9-21` |
| `dns` | object（弱类型 `JsonObject`） | ✅ 条件（DNS 开启） | 见 §1.3.4 |
| `fakedns` | 任意 | ⚠️ 模型保留 | `v2rayN: .../V2rayConfig.cs:7`；未见生成器写入 |
| `inbounds` | array | ✅ 总是 | §1.3.1 |
| `outbounds` | array | ✅ 总是 | §1.3.2 |
| `routing` | object | ✅ 总是 | §1.3.3 |
| `metrics` | `{listen}` | ✅ 条件（统计/实时速度开启） | `v2rayN: .../V2rayStatisticService.cs:7-16` |
| `policy` | `{system}` | ✅ 条件（同上） | `v2rayN: .../V2rayStatisticService.cs:10-21` |
| `stats` | `{}` | ✅ 条件（同上） | `v2rayN: .../V2rayStatisticService.cs:13` |
| `observatory` | object | ✅ 条件（多节点负载均衡） | `v2rayN: .../V2rayConfig.cs:273-279`；由 `GenObservatory()` 生成 |
| `burstObservatory` | object | ✅ 条件 | `v2rayN: .../V2rayConfig.cs:281-294` |
| **`api`** | — | ❌ **不生成** | 模型里**没有** `api` 字段；全仓 `grep '"api"'` 无命中。v2rayN 走 `metrics`（expvar）而不是 gRPC 控制接口 |
| `remarks` | string | ⚠️ 模型字段 | `v2rayN: .../V2rayConfig.cs:16` |

#### 1.3.1 inbounds 字段

`Inbounds4Ray`：`tag` / `port` / `listen` / `protocol` / `sniffing` / `settings`（`v2rayN: .../V2rayConfig.cs:53-66`）。

- `settings`（`Inboundsettings4Ray`）：`auth`、`udp`、`ip`、`address`、`decryption`、`allowTransparent`、`accounts[{user,pass}]`、`name`、`MTU`、`gateway[]`、`autoSystemRoutingTable[]`、`autoOutboundsInterface`、`dns[]`、`autoSystemDnsToGateway`、`autoSystemWfpBlockLeak[]`（`v2rayN: .../V2rayConfig.cs:68-99`）。
- `sniffing`：`enabled` / `destOverride[]` / `routeOnly`（`v2rayN: .../V2rayConfig.cs:101-106`）。

**生成规则（事实）** `v2rayN: .../V2rayInboundService.cs:5-54`：

1. 默认 `listen = "0.0.0.0"`，实际下发给首个入站的是 `127.0.0.1`（样板值），只有 `AllowLANConn` 且不启用独立 LAN 端口时才改成 `0.0.0.0`。
2. 主入站永远存在，除非「TUN 开启 **且** 当前节点就是本机 mixed 端口」（避免自环）。
3. `SecondLocalPortEnabled` → 追加 `socks2` 入站（`port = base+1`，tag=`socks2`）。
4. `AllowLANConn` + `NewPort4LAN` → 追加 `socks3` 入站（`listen=0.0.0.0`，`port=base+2`），若配置了账号密码则 `auth="password"` 且填 `accounts`。
5. `FakeIP` 开启时，往 `sniffing.destOverride` 补 `"fakedns"`（`v2rayN: .../V2rayInboundService.cs:171-179`）。
6. TUN 入站单独注入（`v2rayN: .../V2rayInboundService.cs:56-142`），关键字段：`settings.name="xray_tun"`（macOS 用 `utun{随机}`）、`settings.MTU`、`settings.gateway=[IPv4(,IPv6)]`、`settings.autoSystemRoutingTable`、`sniffing.routeOnly=true`；Windows 严格路由时 `settings.autoSystemWfpBlockLeak=["dns","misconfigtun"]`。

#### 1.3.2 outbounds 字段

`Outbounds4Ray`：`tag` / `protocol` / `sendThrough` / `targetStrategy` / `settings` / `streamSettings` / `mux`（`v2rayN: .../V2rayConfig.cs:108-123`）。

⚠️ **重要结构事实**：v2rayN 的 `Outboundsettings4Ray` 是**扁平模型**（`address` / `port` / `id` / `alterId` / `security` / `encryption` / `flow` / `method` / `password` / `user` / `pass` / `level` / `email` / `headers` / `peers[]` / `mtu` / `reserved[]` / `workers` / `uot` / `ota` / `remoteDNS` / `secretKey` / `response` …，见 `v2rayN: .../V2rayConfig.cs:125-178`），**不是** Xray 原生的 `settings.vnext[]` / `settings.servers[]` 形态。这个扁平模型到 `vnext`/`servers` 的最终序列化转换实现**未查证**（全仓 `vnext` 仅出现在自定义模板读取处 `v2rayN: .../V2rayConfigTemplateService.cs:235` 和测试 JSON 中）。重写时应直接产出 Xray 原生结构，不要复刻这层扁平中间态。

按协议的字段映射（事实，`v2rayN: .../V2rayOutboundService.cs:66-178`）：

| 协议 | settings 字段 |
|---|---|
| VMess | `address`, `port`, `id`(=节点密码), `alterId`, `email`, `security`（在白名单内则用节点值，否则 `"auto"`） |
| Shadowsocks | `address`, `port`, `password`, `method`（白名单外降级 `"none"`）, `uot`, `ota=false`, `level=1` |
| SOCKS | `address`, `port`, 可选 `user`/`pass`/`level=1`/`email` |
| HTTP | `address`, `port`, 可选 `headers`(JSON 对象)、`user`/`pass`/`level=1`/`email` |
| VLESS | `address`, `port`, `id`(=节点密码), `email`, `encryption`；`flow` 为空时开 Mux，非空时写 `flow` 且禁用 Mux 的并发复用 |
| Trojan | `address`, `port`, `password`, `ota=false`, `level=1` |

`streamSettings`：`network` / `security` / `tlsSettings` / `realitySettings` / `wsSettings` / `grpcSettings` / `xhttpSettings` / `httpupgradeSettings` / `httpSettings` / `kcpSettings` / `quicSettings` / `hysteriaSettings` / `masqueSettings` / `rawSettings` / `sockopt` / `finalmask`（`v2rayN: .../V2rayConfig.cs:296-330`）。
`sockopt`：`domainStrategy` / `dialerProxy` / `interface` / `happyEyeballs`（`v2rayN: .../V2rayConfig.cs:556-566`）。
`mux`：`enabled` / `concurrency` / `xudpConcurrency` / `xudpProxyUDP443`（`v2rayN: .../V2rayConfig.cs:187-193`）。

**多节点与链式**（事实）：
- 策略组：tag = `"{base}-{i+1}-{节点备注}"`；只有一个子节点时 tag 直接用 base（`v2rayN: .../V2rayOutboundService.cs:741-773`）。
- 代理链：节点数组**反向**遍历（按实际流量方向），tag = `"chain-{base}-{i}-{备注}"`，用 `streamSettings.sockopt.dialerProxy` 串成链（`v2rayN: .../V2rayOutboundService.cs:775-800`）。

#### 1.3.3 routing 字段

`Routing4Ray`：`domainStrategy` / `rules[]` / `balancers[]`（`v2rayN: .../V2rayConfig.cs:222-229`）。
`RulesItem4Ray`：`type` / `port` / `network` / `inboundTag[]` / `outboundTag` / `balancerTag` / `ip[]` / `domain[]` / `protocol[]` / `process[]`（`v2rayN: .../V2rayConfig.cs:231-251`）。

生成规则（事实）：
- `domainStrategy` 先取路由基础配置，再允许路由项覆盖（`v2rayN: .../V2rayRoutingService.cs:40,45-48`）。
- 用户规则里 `ERuleType.DNS` 类型被跳过（DNS 规则交给 dns 段）（`v2rayN: .../V2rayRoutingService.cs:57-60`）。
- **一条用户规则会被拆成多条**：`domain`、`ip`、`process` 三个维度各自独立成规则，互相剔除对方字段，`type` 统一 `"field"`（`v2rayN: .../V2rayRoutingService.cs:124-160`）。这是为了匹配 Xray 的「同一规则内 domain 与 ip 组合语义」，重写时必须复刻该语义（或直接产出一条规则，但要确认内核行为一致）。
- 空数组字段归一为 `null`（`v2rayN: .../V2rayRoutingService.cs:95-122`）。
- 存在 balancer 时，原本指向某 outbound 的规则改写成 `balancerTag = outboundTag + "-balancer"`，并清空 `outboundTag`（`v2rayN: .../V2rayRoutingService.cs:66-76`）。
- TUN 场景额外注入：`{port:"53", process:[直连进程], outboundTag:"dns"}`、`{process:[直连进程], outboundTag:"direct"}`、`{inboundTag:["tun"], port:"53", outboundTag:"dns"}`（`v2rayN: .../V2rayRoutingService.cs:16-36`）。

#### 1.3.4 dns 字段

`Dns4Ray`：`hosts` / `servers[]` / `serveStale` / `enableParallelQuery` / `disableFallbackIfMatch` / `queryStrategy` / `tag`（`v2rayN: .../V2rayConfig.cs:200-209`）。
`DnsServer4Ray`：`address` / `port` / `domains[]` / `skipFallback` / `expectedIPs[]` / `finalQuery` / `tag`（`v2rayN: .../V2rayConfig.cs:211-220`）。

生成入口 `v2rayN: .../V2rayDnsService.cs:5-48`：自定义 DNS 开启时走 `GenDnsCustom()`（未填则回落到内嵌样板 `Global.DNSV2rayNormalFileName`），并把 `dns` 打上 `tag="dns-module"`，追加一条 `{inboundTag:["dns-module"], outboundTag:"proxy"}` 规则（仅当 `domainStrategy == IPIfNonMatch` 时）；否则走「简单 DNS」路径，并可按配置把 `strategy` 写进 freedom 出站的 `sockopt.domainStrategy`。**具体 servers 列表的构造细节本次未逐行展开（未查证）**。

### 1.4 从节点对象生成配置的完整流程（事实）

`v2rayN: v2rayN/ServiceLib/Services/CoreConfig/V2ray/CoreConfigV2rayService.cs:13-85`：

```
校验节点有效
  ↓ 拒绝 quic 传输（第 25-29 行）
反序列化内嵌样板 → V2rayConfig（33-45）
  ↓
GenLog()        47   → log
GenInbounds()   49   → inbounds
GenOutbounds()  51   → outbounds（代理插到最前）
GenRouting()    53   → routing
GenDns()        55   → dns（可能顺带追加规则）
GenStatistic()  57   → stats + metrics + policy
  ↓ 可选：出站分片 / 最终分片（59-66）
BuildFinalRule()68   → 兜底规则（有 balancerTag 才追加）
  ↓
ApplyFinalConfigModifiers()  76
   = ApplyOutboundBindInterface()      // 绑定网卡 → sockopt.interface
   + ApplyOutboundSendThrough()        // 出口源地址 → sendThrough
   + ApplyCustomOutboundReplace()      // 用户自定义出站模板替换
   + ApplyFullConfigTemplate()         // 用户整包模板合并
```

配置落盘在 `CoreConfigHandler.GenerateClientConfig`：按 `node.ConfigType == Custom` / `RunCoreType == sing_box` / 其它三分支选择生成器，成功后 `File.WriteAllTextAsync(fileName, result.Data)`（`v2rayN: v2rayN/ServiceLib/Handler/CoreConfigHandler.cs:10-37`）。自定义节点是**直接拷贝文件**而不是生成（同文件 `:39-70`）。

---

## 2. sing-box 配置

### 2.1 上游基线模板（事实）

`v2rayN: v2rayN/ServiceLib/Sample/SingboxSampleClientConfig`：

```json
{
  "log": { "level": "debug", "timestamp": true },
  "inbounds": [],
  "outbounds": [ { "type": "direct", "tag": "direct" } ],
  "route": { "rules": [] }
}
```

出站样板（`v2rayN: v2rayN/ServiceLib/Sample/SingboxSampleOutbound`）：

```json
{ "type": "vless", "tag": "proxy", "server": "", "server_port": 443 }
```

### 2.2 最小可用 config.json（拼接结果）

```json
{
  "log": { "level": "warn", "timestamp": true },
  "inbounds": [
    { "type": "mixed", "tag": "socks", "listen": "127.0.0.1", "listen_port": 10808 }
  ],
  "outbounds": [
    { "type": "<节点协议>", "tag": "proxy", "server": "<地址>", "server_port": 443 },
    { "type": "direct", "tag": "direct" }
  ],
  "route": { "rules": [] },
  "experimental": {
    "clash_api": { "external_controller": "127.0.0.1:10813" }
  }
}
```

出处：入站 `v2rayN: v2rayN/ServiceLib/Services/CoreConfig/Singbox/SingboxInboundService.cs:16-25`；出站插到最前 `v2rayN: v2rayN/ServiceLib/Services/CoreConfig/Singbox/SingboxOutboundService.cs`（与 Xray 侧同构，见 §1.3.2 的 `GenOutbounds` 语义）；`experimental.clash_api` 见 `v2rayN: v2rayN/ServiceLib/Services/CoreConfig/Singbox/SingboxStatisticService.cs:9-13`。

> 与 Xray 的差异：sing-box 基线模板**只有 `direct` 一个出站，没有 blackhole**（阻断用 `route.rules` 的 `action: "reject"` 表达）；`route` 里没有 `domainStrategy`，域名策略在 `dns.strategy` / 规则里的 `strategy` 字段上。

### 2.3 字段清单

模型：`v2rayN: v2rayN/ServiceLib/Models/CoreConfigs/SingboxConfig.cs:1-13`。

| 顶层字段 | 类型 | v2rayN 是否生成 | 来源 |
|---|---|---|---|
| `log` | `{disabled, level, output, timestamp}` | ✅ | `v2rayN: .../SingboxConfig.cs:15-21`、`SingboxLogService.cs` |
| `dns` | `{servers, rules, final, optimistic, strategy, disable_cache, disable_expire, independent_cache, cache_capacity, reverse_mapping, client_subnet}` | ✅ 条件 | `v2rayN: .../SingboxConfig.cs:23-36` |
| `inbounds` | array | ✅ | §2.3.1 |
| `outbounds` | array | ✅ | §2.3.2 |
| `endpoints` | array | ✅ 条件（如 WireGuard endpoint） | `v2rayN: .../SingboxConfig.cs:9`；`SingboxConfigTemplateService.cs:26-35` |
| `route` | `{default_domain_resolver, auto_detect_interface, rules[], rule_set[], final}` | ✅ | `v2rayN: .../SingboxConfig.cs:38-45` |
| `http_clients` | array | ⚠️ 模型保留 | `v2rayN: .../SingboxConfig.cs:11` |
| `experimental` | `{cache_file, v2ray_api, clash_api}` | ✅ | `v2rayN: .../SingboxConfig.cs:285-290` |

#### 2.3.1 inbounds

事实（`v2rayN: .../SingboxInboundService.cs:5-101`）：
- 主入站：`{type:"mixed", tag:"socks", listen:"127.0.0.1", listen_port: base+0}`。
- 追加端口时**复制**首个入站再改 `tag`（`socks2`/`socks3`）、`listen_port = base + 枚举值`、`type` 恒为 `mixed`（`:93-100`）。
- LAN 独立端口若配置了账号密码，写 `users: [{username, password}]`（`:44`）。
- TUN 入站从内嵌模板读，然后覆盖 `interface_name="singbox_tun"`（macOS 为 `null`）、`mtu`、`auto_route`、`strict_route`、`stack`、`dns_mode`、`address[]`、`route_exclude_address[]`（`:54-85`）。

#### 2.3.2 outbounds / route / dns / experimental

- `route.rules[]`（`Rule4Sbox`）可用字段极多，本项目相关的高频项：`outbound`、`action`、`inbound[]`、`protocol[]`、`network[]`、`port[]`、`port_range[]`、`domain[]`、`domain_suffix[]`、`domain_keyword[]`、`domain_regex[]`、`ip_cidr[]`、`source_ip_cidr[]`、`geosite[]`、`geoip[]`、`rule_set[]`、`process_name[]`、`process_path[]`、`clash_mode`、`invert`、`ip_is_private`（`v2rayN: .../SingboxConfig.cs:47-…`）。
- `route.rule_set[]` 由 `SingboxRulesetService` 生成，`ConvertGeo2Ruleset()` 负责把 geo 类规则转成 ruleset 引用（`v2rayN: .../CoreConfigSingboxService.cs:58`）。
- `experimental.clash_api`：只有 `external_controller` 与 `store_selected` 两个字段（`v2rayN: .../SingboxConfig.cs:298-302`）。**v2rayN 不设置 `secret`，也不设置 `external_ui`**。
- `experimental.cache_file`：`{enabled:true, path:"<bin>/cache.db", store_fakeip: <FakeIP 开关>}`（`v2rayN: .../SingboxStatisticService.cs:16-25`）。

### 2.4 生成流程

`v2rayN: v2rayN/ServiceLib/Services/CoreConfig/Singbox/CoreConfigSingboxService.cs:13-72`：

```
校验 → 拒绝 kcp / xhttp 传输（24-28）
反序列化 SingboxSampleClient（32-44）
GenLog()           46
GenInbounds()      48
GenOutbounds()     50
GenRouting()       52
GenDns()           54
GenExperimental()  56   → clash_api + cache_file
ConvertGeo2Ruleset() 58
ApplyFinalConfigModifiers() 63   // 与 Xray 侧同名同构
```

---

## 3. 流量统计

### 3.1 Xray：`metrics` + HTTP `/debug/vars`（v2rayN 的实际做法）

**结论：v2rayN 不使用 gRPC StatsService。** 它开启 Xray 的 `metrics` 端点，然后每秒 HTTP GET 一次 expvar JSON。

配置侧（`v2rayN: v2rayN/ServiceLib/Services/CoreConfig/V2ray/V2rayStatisticService.cs:5-23`）：

```csharp
if (_config.GuiItem.EnableStatistics || _config.GuiItem.DisplayRealTimeSpeed) {
    _coreConfig.stats = new Stats4Ray();                                        // "stats": {}
    metricsObj.listen = $"{Global.Loopback}:{AppManager.Instance.StatePort}";   // "metrics": {"listen": "127.0.0.1:10812"}
    _coreConfig.metrics = metricsObj;
    policySystemSetting.statsOutboundDownlink = true;
    policySystemSetting.statsOutboundUplink   = true;
    policyObj.system = policySystemSetting;                                     // "policy": {"system": {...}}
    _coreConfig.policy = policyObj;
}
```

读取侧（`v2rayN: v2rayN/ServiceLib/Services/Statistics/StatisticsXrayService.cs`）：

| 项 | 事实 | 行号 |
|---|---|---|
| URL | `http://127.0.0.1:{StatePort}/debug/vars` | `:10` |
| 频率 | `PeriodicTimer(1s)` | `:41` |
| 失败退避 | 3 秒 | `:66` |
| 数据结构 | 反序列化为 `V2rayMetricsVars`，读 `stats.outbound` 字典；每个 value 再解析为 `{uplink, downlink}`（单位：字节） | `:71-89` |
| 分类 | key 以 `"proxy"` 开头 → 累加到 ProxyUp/ProxyDown；key == `"direct"` → DirectUp/DirectDown | `:91-100` |
| 单位换算 | 字节 `/ 1024` → KB | `:93-99` |
| 回绕处理 | 若累计值变小，重置基准并丢弃本次结果 | `:103-107` |
| 输出 | **返回增量**（本次累计 − 上次累计），不是绝对值 | `:109-117` |

`V2rayMetricsVars` / `V2rayMetricsVarsLink` 模型见 `v2rayN: v2rayN/ServiceLib/Models/CoreConfigs/V2rayMetricsVars.cs`。

> 重写要点：这是**累计值 → 差分**的模型。UI 层若要「速率」需自己做滑动窗口；若要「总量」直接用累计值。另注意 `policy.system.statsOutbound*` 只覆盖出站维度，v2rayN 的 `SystemPolicy4Ray` **没有** `statsInboundUplink` / `statsInboundDownlink` 字段（`v2rayN: .../V2rayConfig.cs:32-36`）。

### 3.2 Xray：gRPC StatsService —— **未查证**

任务书要求的「StatsService 服务名、方法、请求/响应结构、端口怎么配、`stats`+`api`+`policy.system.statsInbound*` 开关组合」：

- **两份上游都没有使用 gRPC StatsService**。v2rayN 全仓 `grep StatsService` / `grep v2ray.core.app.stats` 无命中；其 `Grpc` 命中全部是 **gRPC 传输层**（`streamSettings.grpcSettings`），与统计无关。
- v2rayN 的 `V2rayConfig` 模型里**不存在 `api` 字段**，因此它从不生成 `"api": {"tag":"api","services":["StatsService"]}` 这一段（`v2rayN: v2rayN/ServiceLib/Models/CoreConfigs/V2rayConfig.cs:3-17`，全仓 `grep '"api"'` 无命中）。
- 因此：**服务名 / 方法名 / protobuf 请求响应结构 / `statsInbound*` 开关语义 —— 未查证**，不在本文编造。

可复用的替代路径（有据）：若 X-Matrix 需要按出站维度统计，v2rayN 的 `metrics` + `/debug/vars` 方案已被验证可用，且不需要引入 gRPC 客户端依赖（`v2rayN: .../StatisticsXrayService.cs:10`）。若确实需要 gRPC（例如要 `statsInbound*` 或在线连接数），必须另行查证 Xray 官方文档（本次离线，未查证）。

### 3.3 sing-box：Clash API

**开启方式**（`v2rayN: v2rayN/ServiceLib/Services/CoreConfig/Singbox/SingboxStatisticService.cs:5-14`）：

```json
"experimental": { "clash_api": { "external_controller": "127.0.0.1:10813" } }
```

注意：v2rayN 里这段是**无条件写入**的（`EnableStatistics` 判断被注释掉），即只要用 sing-box 内核就开 Clash API。模型只有 `external_controller` 与 `store_selected` 两个字段，**没有 `secret` / `external_ui`**（`v2rayN: .../SingboxConfig.cs:298-302`）——即上游默认无鉴权、仅监听回环。

**`/traffic`**：WebSocket，不是 HTTP 轮询。

| 项 | 事实 | 来源 |
|---|---|---|
| URL | `ws://127.0.0.1:{StatePort2}/traffic` | `v2rayN: .../Statistics/StatisticsSingboxService.cs:10` |
| 传输 | `System.Net.WebSockets.ClientWebSocket`，长连接循环读 | 同文件 `:50-85` |
| 消息结构 | 反序列化为 `TrafficItem`，取 `Up` / `Down`（**字节/秒**，不是累计值） | 同文件 `:104-109` |
| 单位换算 | `/ 1000` → KB | 同文件 `:81-82` |
| 断线 | 异常后 `Task.Delay(3000)` 重连 | 同文件 `:91-94` |

> 与 Xray 的关键差异：**sing-box `/traffic` 给的是瞬时速率，Xray `/debug/vars` 给的是累计值**。重写时两条链路的语义必须在抽象层统一（建议内部统一成「累计字节数」，速率层单独算）。

**`/proxies` 与 `/providers/proxies`**（`v2rayN: v2rayN/ServiceLib/Manager/ClashApiManager.cs`）：

| 端点 | 用途 | 事实 | 行号 |
|---|---|---|---|
| `GET /proxies` | 代理组与节点列表 | 与 `/providers/proxies` **并发**请求；反序列化为 `ClashProxies` / `ClashProviders`；失败重试 3 次、间隔 2 秒 | `:13-44` |
| `GET /providers/proxies` | 订阅 provider 内的节点 | provider 里的 proxy 用 `TryAdd` 合并进主表，并记录 `ProviderIndexMap[proxyName] = providerKey` | `:51-65` |
| `GET /proxies/{name}/delay?timeout=10000&url=...` | 单节点延迟测试 | `name` 需 URL 编码；`url` 取配置的测速地址 | `:82-87` |
| provider 内节点延迟 | 同上但走 provider 路径 | `:89+` |

**`/connections`**：见 §4。

**未使用的端点**：`/configs`、`/memory`、`/version`、`/logs`、`/rules`、`/providers/rules` 在 v2rayN 中未见调用（未查证其返回结构）。

---

## 4. 连接列表（当前连接）

### 4.1 数据来源

**只有 sing-box / mihomo 侧有连接列表，Xray 侧没有。**

| 内核 | 接口 | 事实 | 来源 |
|---|---|---|---|
| sing-box / mihomo | `GET http://127.0.0.1:{StatePort2}/connections` | 反序列化为 `ClashConnections` | `v2rayN: v2rayN/ServiceLib/Manager/ClashApiManager.cs:213-227` |
| sing-box / mihomo | `DELETE /connections/{id}` | 关闭单条连接（`id` 为空则全部关闭） | 同文件 `:229-239` |
| **Xray** | **无** | v2rayN 未实现 Xray 连接列表；`Handler/ConnectionHandler.cs` 只做可用性检查 / 真实延迟 / IP 信息，与连接枚举无关（`v2rayN: v2rayN/ServiceLib/Handler/ConnectionHandler.cs:10,61,80,113`）。Xray 内核是否提供连接列表接口 —— **未查证** | — |

### 4.2 `/connections` 响应结构（两处独立定义，字段一致）

v2rayN 的 DTO（`v2rayN: v2rayN/ServiceLib/Models/Dto/ClashConnections.cs`）：

```
ClashConnections { downloadTotal: u64, uploadTotal: u64, connections: [ConnectionItem] }

ConnectionItem {
  id: string
  metadata: {
    network, type, sourceIP, destinationIP, sourcePort, destinationPort,
    host, nsMode, uid, process, processPath, remoteDestination, sniffHost
  }
  upload: u64, download: u64
  start: DateTime
  chains: [string]
  rule: string
  rulePayload: string
}
```

clash-verge 前端类型（`clash-verge: src/types/global.d.ts:150-178`）：

```
IConnectionsItem {
  id, metadata { network, type, host, sourceIP, sourcePort, destinationPort,
                 destinationIP?, remoteDestination?, process?, processPath? },
  upload, download, start, chains[], rule, rulePayload,
  curUpload?, curDownload?      // 运行时计算的速度，不是接口字段
}
IConnections { downloadTotal, uploadTotal, connections[] }
```

### 4.3 字段与任务书要求的映射

| 任务书要求 | 接口字段 | 备注 |
|---|---|---|
| 目的地 | `metadata.host`，为空则 `metadata.destinationIP`，拼 `:destinationPort` | v2rayN 的展示逻辑：`v2rayN: .../ViewModels/ClashConnectionsViewModel.cs:66-79` |
| SNI 嗅探结果 | `metadata.sniffHost` | 与 host 不同时以括号附注（同上 `:75-78`） |
| 规则命中 | `rule` + `rulePayload` | v2rayN 拼成 `"{rule} , {chains joined by ->}"`（同上 `:98`） |
| 代理链 | `chains[]` | 同上 |
| 上行 / 下行 | `upload` / `download`（累计字节） | — |
| 时长 | `start`（起始时间） | **接口不返回时长**，由客户端算 `now - start`（同上 `:94-95`） |
| 进程 | `metadata.process` / `metadata.processPath` | — |

### 4.4 刷新与交互（事实）

- 轮询间隔 1 秒（`v2rayN: .../ClashConnectionsViewModel.cs:122-127`）。
- 仅在「自动刷新开启 + 窗口可见 + 正在跑 sing-box」时才拉取（同上 `:129-133`）。
- 关闭连接后立即重新拉取一次（同上 `:115-119`）。
- clash-verge 侧：前端订阅 WebSocket（`connect_connections()`），节流 500ms，保留最多 500 条已关闭连接（`clash-verge: src/hooks/use-connection-data.ts:1-10,400-408`）。注意该 WebSocket 客户端来自外部依赖 `tauri-plugin-mihomo`（`clash-verge: src-tauri/Cargo.toml:94`、`clash-verge: package.json:84`），**实现源码不在本仓库，其内部端点与消息格式未查证**。

---

## 5. 进程生命周期

### 5.1 v2rayN 的做法

**启动链**（`v2rayN: v2rayN/ServiceLib/Manager/CoreManager.cs:65-114`）：

```
生成配置写盘（Global.CoreConfigFileName）
  ↓
CoreStop()  ← 先停旧核
  ↓ Task.Delay(100)
Windows + TUN：WindowsUtils.RemoveTunDevice()
  ↓
CoreStart()  → RunProcess() → RunProcessNormal()
  ↓
（仅 TUN 场景）WaitForProxyPort(preContext)   ← 等代理端口真的能握手
  ↓
（可选）CoreStartPreService(preContext)      ← 第二内核（如 TUN 前置）
  ↓
AppManager.RunningCoreType = ...
```

**子进程启动细节**（`v2rayN: v2rayN/ServiceLib/Services/ProcessService.cs:13-71`）：

| 项 | 值 |
|---|---|
| `UseShellExecute` | `false` |
| `CreateNoWindow` | `true` |
| stdout/stderr | `displayLog` 时重定向，UTF-8，`OutputDataReceived`/`ErrorDataReceived` 逐行推给 UI 回调 |
| stdin | 仅 sudo 场景重定向，用于写密码 |
| 环境变量 | 逐个写入 `StartInfo.Environment`，值里的配置路径占位符先 `string.Format` 展开（`v2rayN: .../CoreManager.cs:362-366`） |
| 工作目录 | `Utils.GetBinConfigPath()` |
| 启动后存活检查 | `Task.Delay(100)` 后若 `HasExited` 直接抛「内核启动失败」（`v2rayN: .../CoreManager.cs:380-385`） |

**停止**（`v2rayN: v2rayN/ServiceLib/Services/ProcessService.cs:73-117`）：`CancelOutputRead` / `CancelErrorRead` → 非 Windows 先 `Kill(entireProcessTree: true)` → `Kill()` → `Task.Delay(100)`。没有优雅关闭信号（不发 SIGTERM / 不发 Ctrl-C）。

**「已就绪」判定 —— 只有 TUN 场景才有**（`v2rayN: v2rayN/ServiceLib/Manager/CoreManager.cs:245-312`）：

```
loop（受 Global.LocalFetch 预算约束）:
  TCP connect 127.0.0.1:<port>（单次超时 50ms）
  发送 SOCKS5 greeting: 0x05 0x01 0x00
  读 2 字节；若 buf[0] == 0x05  → 就绪，返回
  ConnectionRefused → Delay(50ms) 重试
```

即：**不是端口连通判定，也不是日志匹配，而是「真的完成一次 SOCKS5 方法协商」**。普通（非 TUN）模式**不做就绪等待**，启动后 100ms 就算成功。

**子进程随主进程退出**：Windows Job Object，`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`（`0x2000`）。
`v2rayN: v2rayN/ServiceLib/Services/WindowsJobService.cs:11-56`：

```
CreateJobObject(0, null)
JOBOBJECT_BASIC_LIMIT_INFORMATION { LimitFlags = 0x2000 }
  → 包进 JOBOBJECT_EXTENDED_LIMIT_INFORMATION
SetInformationJobObject(handle, ExtendedLimitInformation /* 9 */, ptr, len)
AssignProcessToJobObject(handle, processHandle)     // 每次 spawn 后调用
CloseHandle(handle)                                 // Dispose 时
```

接入点：`v2rayN: v2rayN/ServiceLib/Manager/CoreManager.cs:386,391-402`（`RunProcessNormal` 启动成功后 `AddProcessJob(procService.Handle)`）。

**内核可执行文件与命令行参数**（`v2rayN: v2rayN/ServiceLib/Manager/CoreInfoManager.cs`）：

| 内核 | exe 候选名 | 参数 | 版本参数 | 行号 |
|---|---|---|---|---|
| Xray | `xray` | **`run -c {0}`** | `-version` | `:149-171` |
| sing-box | `sing-box-client`, `sing-box` | **`run -c {0} --disable-color`** | `version` | `:216-235`（参数在 `:220`） |
| v2fly | `v2ray` | `{0}` | `-version` | `:121-133` |
| mihomo | 见 `GetMihomoCoreExes()` | `-f {0}` + PortableMode | `-v` | `:173-190` |

`{0}` 为配置文件绝对路径；`CoreInfo.AbsolutePath == true` 时用带引号的绝对路径（`v2rayN: .../CoreManager.cs:365,370`）。Xray 额外注入环境变量 `XrayLocalAsset` / `XrayLocalCert` 指向 bin 目录（`v2rayN: .../CoreInfoManager.cs:166-170`）。

### 5.2 clash-verge 的做法

**两种运行模式**：`RunningMode::{Service, Sidecar, NotRunning}`（`clash-verge: src-tauri/src/core/manager/state.rs:153-159`）。Windows 上优先用**系统服务**（提权跑内核，避免每次 UAC），服务不可用时回退 **sidecar**（普通子进程）。

**sidecar 启动**（`clash-verge: src-tauri/src/core/manager/state.rs:162-312`）：

```
等上一个 sidecar 完全退出
reserve_sidecar()（IPC 层占位，防并发双开）
生成运行配置（Windows 非管理员时强制关 TUN 后写盘）
shell().sidecar(core_name)
  .args(["-d", <app_home_dir>, "-f", <config_file>,
         <windows? "-ext-ctl-pipe" : "-ext-ctl-unix">, <ipc_path>])
  .env("LISTEN_NAMEDPIPE_SDDL", current_user_pipe_sddl())   // 仅 Windows
  .spawn()
记录 child pid → release_after_exit(pid)（退出时释放占位）
Windows: create_and_assign_sidecar_job(child.pid())  ← 失败则 kill 子进程并报错
起一个 task 持续 drain rx（stdout/stderr/Terminated），写入 sidecar 日志
轮询就绪（见下）
就绪失败 → 停 guard → 标记 core_stopped → kill 子进程
成功 → set_job_handle(job) → set_running_child_sidecar → core_started(Sidecar)
```

**就绪判定**（`clash-verge: src-tauri/src/core/manager/state.rs:46,99-100,285-303`）：

| 项 | 值 |
|---|---|
| 探测方式 | **调用内核 HTTP API**：`mihomo.get_version()` |
| 重试次数 | 30 |
| 重试间隔 | 100 ms |
| 单次探测超时 | 400 ms |
| 失败处理 | 停系统代理 guard → 标记停止 → kill 子进程 → 返回错误 |

即 clash-verge 的「已就绪」= **API 能应答**（比 v2rayN 的 SOCKS5 握手更严格：它同时验证了控制接口和配置加载）。

**Job Object**（`clash-verge: src-tauri/src/core/manager/state.rs:599-637`）：

```
CreateJobObjectW(null, null)
info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info, size_of::<...>)
OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE | PROCESS_QUERY_INFORMATION, 0, pid)
AssignProcessToJobObject(job, process_handle)
→ 返回 OwnedHandle，交由 CoreManager 持有
```

生命周期语义被显式写进注释：`set_job_handle(None)` 会 drop 掉 job handle，从而因 `KILL_ON_JOB_CLOSE` **终止所有已分配进程**（`clash-verge: src-tauri/src/core/manager/mod.rs:257-270`；`state.rs:320-326`）。

**顺序保证（值得照抄的设计）**：

| 场景 | 顺序 | 来源 |
|---|---|---|
| 启动 | 内核就绪 **之后** 才开系统代理（`run_core_start_transition(start_core, core_is_ready, apply_proxy_after_start)`） | `clash-verge: src-tauri/src/core/manager/lifecycle.rs:364-375` |
| 停止 | **先清系统代理**，再终止内核（`run_sidecar_termination_transition(proxy_control::clear, stop_core_by_sidecar_unprepared)`） | `clash-verge: src-tauri/src/core/manager/lifecycle.rs:525,537,763` |
| 重启 | 停 → 起 → 就绪 → 重开代理；失败回滚会再次清代理 | `clash-verge: src-tauri/src/core/manager/lifecycle.rs:793-812` |
| 并发控制 | 配置更新互斥锁 + 生命周期锁双层（`try_start_config_update()` / `lifecycle_lock`） | `clash-verge: src-tauri/src/core/manager/lifecycle.rs:354-365` |

### 5.3 对照与重写建议

| 维度 | v2rayN | clash-verge | 建议 |
|---|---|---|---|
| 提权方式 | 非 Windows 用 sudo；Windows 无服务模式 | Windows 系统服务 + sidecar 回退 | 采纳 clash-verge：服务优先、sidecar 兜底 |
| 就绪判定 | TUN 才有，SOCKS5 握手 | 始终，内核 API `get_version` | 采纳 clash-verge 的 API 探测；Xray 可用 `GET /debug/vars` 或 `metrics` 端点做等价探测（v2rayN 已证明该端点存在，`StatisticsXrayService.cs:10`） |
| 子进程托管 | Job Object（KILL_ON_JOB_CLOSE） | Job Object（KILL_ON_JOB_CLOSE） | 两家一致，Rust 侧用 `windows` crate 照做 |
| 停止方式 | `Kill()`，无优雅退出 | `child.kill()`（tauri shell），无优雅退出 | 同；注意先清代理再杀进程 |
| 代理与内核的顺序 | 未显式保证（代理开关是独立 UI 动作） | 强顺序：就绪→开代理，停→先关代理 | 采纳 clash-verge |
| 控制通道 | TCP 回环端口 | Windows 命名管道（`-ext-ctl-pipe`）+ 随机化 SDDL | 管道更安全（不占端口、不可被同机其他用户连接）；Xray 不支持管道，仍须回环端口 + 随机端口 |

---

## 6. 系统代理（Windows）

### 6.1 v2rayN：注册表 + WinINet 双写

**注册表键位**（`v2rayN: v2rayN/ServiceLib/Handler/SysProxy/ProxySettingWindows.cs:8`）：

```
HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings
  ProxyEnable     REG_DWORD
  ProxyServer     REG_SZ
  ProxyOverride  REG_SZ
  AutoConfigURL   REG_SZ
```

三种写入模式（同文件 `:10-34` `SetProxyFallback`）：

| 模式 | `ProxyEnable` | `ProxyServer` | `ProxyOverride` | `AutoConfigURL` |
|---|---|---|---|---|
| 关闭（type 1） | `0` | `""` | `""` | `""` |
| 命名代理（type 2） | `1` | `<代理串>` | `<例外列表>` | `""` |
| PAC（type 4） | `0` | `""` | `""` | `<PAC URL>` |

**主路径不是直接写注册表，而是 WinINet API**（`v2rayN: .../ProxySettingWindows.cs:58-198`）：

- `SetProxy()` 先设「LAN 连接」（`connectionName = null`），再 `EnumerateRasEntries()` 枚举拨号连接逐个设置（`:63-69`）。
- 每个连接调用 `InternetSetOption(NULL, INTERNET_OPTION_PER_CONNECTION_OPTION, ...)`，选项数组由 `INTERNET_PER_CONN_FLAGS` + `INTERNET_PER_CONN_PROXY_SERVER` / `INTERNET_PER_CONN_PROXY_BYPASS` / `INTERNET_PER_CONN_AUTOCONFIG_URL` 组成（`:93-163`）。
- 成功后必须再调两次刷新通知：`INTERNET_OPTION_SETTINGS_CHANGED` 与 `INTERNET_OPTION_REFRESH`（`:171-174`）。**漏掉这两步浏览器不会立即生效**，这是重写最容易踩的坑。
- 失败时回落为直接写注册表（`:72-76`）。
- Flag 常量：`PROXY_TYPE_DIRECT=0x1`、`PROXY_TYPE_PROXY=0x2`、`PROXY_TYPE_AUTO_PROXY_URL=0x4`、`PROXY_TYPE_AUTO_DETECT=0x8`（`:50-54` 注释）。

**代理串格式**（`v2rayN: v2rayN/ServiceLib/Handler/SysProxy/SysProxyHandler.cs:86-108`）：

- 默认：`127.0.0.1:<mixed 端口>`。
- 例外列表：配置里的 `SystemProxyExceptions` 去掉空格；`NotProxyLocalAddress` 为真时前缀 `"<local>;"`。
- 高级协议模板：支持 `{ip}` / `{http_port}` / `{socks_port}` 占位符替换。

**PAC 方案**（`v2rayN: .../SysProxyHandler.cs:110-117`）：

```
portPac = base + 3
PacManager.StartAsync(mixedPort, portPac)          // 本地起 HTTP 服务，动态生成 PAC
AutoConfigURL = "http://127.0.0.1:<portPac>/pac?t=<ticks>"   // 带时间戳防缓存
SetProxy(url, "", 4)                               // type 4 = PAC 模式
```

切到非 PAC 模式时会 `_pacManager.Stop()` 停掉本地 PAC 服务（`:58-61`）。

**⚠️ 事实纠正**：v2rayN **没有**「开启前快照、退出还原」。它只有 set / unset 两态，关闭时把四个键**清空**（`ProxyEnable=0`、其余空串），不会恢复用户原本的代理设置（`v2rayN: .../ProxySettingWindows.cs:14-18,40-43`）。

### 6.2 clash-verge：`sysproxy` crate + guard

clash-verge 的系统代理能力**全部委托给外部 crate**：

```toml
sysproxy = { git = "https://github.com/clash-verge-rev/sysproxy-rs.git",
             rev = "2c9d0a538052aac6e44ad413156a6e848da262b9", version = "0.7.2" }
```
（`clash-verge: src-tauri/Cargo.toml:70`）

> **该 crate 的 Windows 注册表/WinINet 实现源码不在本仓库 → 具体键位与 API 调用序列「未查证」**。本仓库只能确认它对外暴露的契约（见下）。

**观察到的 API 契约**（`clash-verge: src-tauri/src/core/sysopt.rs:18,84-93,427-461`）：

| 类型 | 方法 / 字段 | 语义 |
|---|---|---|
| `Sysproxy` | `.enable: bool`、`.bypass: String`、`.set_system_proxy()` | 全局命名代理 |
| `Autoproxy` | `.enable: bool`、`.set_auto_proxy()` | PAC / 自动配置脚本 |
| `ProxySnapshot` | `matches_global(&sys)`、`matches_pac(&auto)`、`bypass_matches(&str)`、`is_all_disabled()` | **读取当前系统状态并比对**，用于幂等判断 |
| `GuardMonitor` / `GuardType` | `new(GuardType, interval)`、`start()`、`shutdown()`、`set_guard_type()`、`set_interval()` | 周期性重申代理设置 |
| `GuardType` | `None` / `Sysproxy(..)` / `Autoproxy(..)` | guard 目标 |

**互斥模型**（`clash-verge: src-tauri/src/core/sysopt.rs:427-461`）：

```
enable_system_proxy 是总开关
├─ 关闭      → sys.enable=false, auto.enable=false, guard=None
├─ PAC 模式  → sys.enable=false, auto.enable=true,  guard=Autoproxy(或 None)
└─ 全局模式  → sys.enable=true,  auto.enable=false, guard=Sysproxy(或 None)
```

相关配置项（`clash-verge: src-tauri/src/config/verge.rs:74-93,347-371`）：`enable_system_proxy`、`proxy_auto_config`（PAC 开关）、`proxy_host`（默认 `"127.0.0.1"`）、`system_proxy_bypass`、`enable_proxy_guard`、`proxy_guard_duration`（默认 30 秒）。

**写入顺序**（`clash-verge: src-tauri/src/core/sysopt.rs:28-36` `proxy_apply_steps`）：开全局代理时**先关 PAC 再设命名代理**；开 PAC 时只设 PAC。每一步只有「真的写到了 OS」才算变更，被跳过的一步不算（`:494-521`）。写入前必须先 drain guard（最多等 10 秒，`GUARD_DRAIN_TIMEOUT`，`:39`），否则 OS 读到的状态不可信。部分失败时用「全关」做补偿（`:486-492`）。

**guard 的作用**：防止被其他软件改回去。`enable_proxy_guard` 打开后，`GuardMonitor` 每 `proxy_guard_duration` 秒重新把代理设置重申一遍（`clash-verge: .../sysopt.rs:323-366`）。默认关闭（`verge.rs:368`）。

**退出时**：`reset_sysproxy()` 把 `sys.enable` 与 `auto.enable` 都置 false 后统一关闭（`clash-verge: .../sysopt.rs:536-572`），调用链见 `clash-verge: src-tauri/src/core/proxy_control.rs:517-568` 与 `lifecycle.rs:525,537,763`。

**⚠️ 事实纠正（与任务书预设不同）**：clash-verge **没有「开启前快照 → 退出还原」**。

- `ProxySnapshot` 只用于**读-比对**（判断「目标状态是否已经在位」，实现幂等与 macOS 短路优化，`clash-verge: .../sysopt.rs:84-118`），不会被持久化，也不用于退出还原。
- 退出路径是「全部关闭」（`clear` / `reset_sysproxy`），不是「恢复原值」。
- 仓库里唯一叫 snapshot 的是**配置文件快照**，用于配置写入失败时回滚 `config.yaml` / `verge.yaml` / 运行时 YAML 三个文件（`clash-verge: src-tauri/src/config/snapshot.rs:1-50`），与系统代理无关。

> **结论**：如果 X-Matrix 要「开启前快照、退出还原」，这是一个**两家上游都没有、需要自行实现**的功能。最小实现：开启前读四个键（`ProxyEnable` / `ProxyServer` / `ProxyOverride` / `AutoConfigURL`）存盘 → 退出时按原值写回 → 再 `INTERNET_OPTION_SETTINGS_CHANGED` + `REFRESH`。注意要处理「快照存在但上次异常退出」的脏数据。

---

## 7. 订阅解析

### 7.1 v2rayN 支持的格式与识别入口

**总入口**：`ConfigDbHandler.AddBatchServers(strData, subid, isSub)`（`v2rayN: v2rayN/ServiceLib/Handler/Data/ConfigDbHandler.cs:1853-1955`）。
调用方：订阅更新 `v2rayN: v2rayN/ServiceLib/Handler/SubscriptionHandler.cs:108`。

**识别回退链（按顺序尝试，前一步成功即停；个别步骤可累加计数）**：

| # | 步骤 | 判定方式 | 行号 |
|---|---|---|---|
| 1 | Base64 解码后按行解析 | `Utils.IsBase64String(strData)` | `:1868-1871` |
| 2 | 原文直接按行解析 | 上一步 0 结果 | `:1872-1875` |
| 3 | 强制 Base64 解码再解析 | 仍 0 结果 | `:1876-1879` |
| 4 | Shadowsocks SIP008 JSON | `AddBatchServers4SsSIP008` | `:1881-1884` |
| 5 | WireGuard 配置文件 | `AddBatchServers4Wireguard` | `:1886-1890` |
| 6 | 内部 URI（可累加计数） | `AddBatchServers4InnerUri`，原文/base64 各一轮 | `:1892-1916` |
| 7 | **整包自定义配置**（Clash YAML / sing-box JSON / Xray JSON） | `AddBatchServers4Custom` | `:1918-1922` |
| 8 | 收尾 | 成功后删除旧订阅节点、恢复激活节点、迁移流量统计 | `:1924-1952` |

Base64 判定实现（`v2rayN: v2rayN/ServiceLib/Common/Utils.cs:305-314`）：`Convert.TryFromBase64String(plainText, buffer, out _)`，缓冲区长度取字符串长度。**注意这是「能解就算」，不做字符集白名单校验**——纯 ASCII 的普通文本也可能被误判，所以必须有第 2 步的原文重试兜底。

**逐行解析**（`v2rayN: .../ConfigDbHandler.cs:1375-1530`）：

- 按换行切分，订阅模式下先去重。
- 非订阅模式下，`http(s)://` 开头的行当作**子订阅 URL** 处理（`AddSubItem`），不当作节点。
- 其余行交给 `FmtHandler.ResolveConfig(str, out _)`。
- 订阅的 `Filter` 字段是正则，用来过滤节点备注（`:1433-1439`）。
- 订阅的 `PreResolver` 可指定 DNS 预解析：解析成功后把 IP 写回 `Address`，若原本没 SNI 则把原域名补到 `Sni`（`:1443-1470`）。
- 解析并行度 8（`Parallel.ForEachAsync`，`:1414-1417`）。

**URI 前缀分派**（`v2rayN: v2rayN/ServiceLib/Handler/Fmt/FmtHandler.cs:37-108`）：

| 前缀 | 解析器 | 行号 |
|---|---|---|
| `vmess://` | `VmessFmt.Resolve` | `:56` |
| `ss://` | `ShadowsocksFmt.Resolve` | `:60` |
| `socks://` / `socks5://` / `socks4://` | `SocksFmt.Resolve` | `:64-67` |
| `trojan://` | `TrojanFmt.Resolve` | `:69` |
| `vless://` | `VLESSFmt.Resolve` | `:73` |
| `hysteria2://` / `hy2://` | `Hysteria2Fmt.Resolve` | `:77-80` |
| `hysteria2+realm://` 等变体 | `Hysteria2Fmt.ResolveRealm` | `:82-85` |
| `tuic://` | `TuicFmt.Resolve` | `:87` |
| `wireguard://` | `WireguardFmt.Resolve` | `:91` |
| `anytls://` | `AnytlsFmt.Resolve` | `:95` |
| `naive+https://` / `naive+quic://` / `naive://` | `NaiveFmt.Resolve` | `:99-103` |
| 其它 | 返回 `null` + 错误消息 | `:105-107` |

### 7.2 Clash YAML：**整包接管，不做节点级解析**

`v2rayN: v2rayN/ServiceLib/Handler/Fmt/ClashFmt.cs:5-21`：

```csharp
public static ProfileItem? ResolveFull(string strData, string? subRemarks)
{
    if (Contains(strData, "rules", "-port", "proxies"))   // 三个关键词同时出现才算
    {
        var fileName = WriteAllText(strData, "yaml");     // 原样落盘
        return new ProfileItem {
            CoreType = ECoreType.mihomo,                  // 强制交给 mihomo 内核
            Address  = fileName,                          // 节点地址字段 = 配置文件路径
            Remarks  = subRemarks ?? "clash_custom"
        };
    }
    return null;
}
```

即：Clash 订阅被当作**一份完整的 mihomo 配置**，生成一个 `Custom` 节点，运行时直接让 mihomo 读这份 YAML（`v2rayN: v2rayN/ServiceLib/Handler/CoreConfigHandler.cs:20-23`）。**不做 proxies → 内部节点的转换**。

### 7.3 sing-box JSON / Xray JSON：同样整包接管

`v2rayN: v2rayN/ServiceLib/Handler/Fmt/SingboxFmt.cs:5-101`：

- `ResolveToCustom` / `ResolveToCustomOutbound` → `ResolveCommon`（数组则递归，逐元素处理）。
- `ResolveFull`（`:56-86`）判定条件：**同时**有 `inbounds` 与 `outbounds`，且 `outbounds` 里至少一个合法出站 → 原样落盘，产出 `{ConfigType: Custom, CoreType: sing_box, Address: <文件路径>, Remarks: "singbox_custom"}`。
- 否则 `ResolveFullToOutbound`（`:88+`）/ `ResolveOutbound` 尝试把单个出站对象转成节点。
- `V2rayFmt`（`v2rayN: v2rayN/ServiceLib/Handler/Fmt/V2rayFmt.cs:1-60`）与 SingboxFmt 结构完全同构，只是 `CoreType` 不同。

**自定义配置的优先级**（`v2rayN: .../ConfigDbHandler.cs:1549-1597`）：

```
V2rayFmt.ResolveToCustom
  → SingboxFmt.ResolveToCustom
  → V2rayFmt.ResolveToCustomOutbound
  → SingboxFmt.ResolveToCustomOutbound
  → 拒绝 HTML 页面（HtmlPageFmt.IsHtmlPage）
  → ClashFmt.ResolveFull ?? Hysteria2Fmt.ResolveFull2
```

若订阅显式指定了 `CustomCoreType`，则只走对应分支（Xray → `V2rayFmt`，sing-box → `SingboxFmt`，`:1599-1622`）。

### 7.4 clash-verge 的订阅

**只支持 Clash 生态的 YAML profile**，没有 base64 / v2ray URI / sing-box JSON 解析。

配置项模型 `PrfItem`（`clash-verge: src-tauri/src/config/prfitem.rs:32-65`）：

```
PrfItem {
  uid, type: "remote" | "local" | "script" | "merge", name, file,
  desc, url, selected[{name, now}], extra{upload, download, total, expire},
  updated, option{user_agent, with_proxy, self_proxy, update_interval, timeout_seconds, ...},
  home
}
```

- `type` 四态：远程订阅 / 本地文件 / 脚本增强 / 合并增强（`prfitem.rs:37-39`）。
- 订阅流量信息（`upload/download/total/expire`）来自响应头解析（`extra` 字段，`prfitem.rs:74-80`）。
- profile 存放于 `profiles.yaml`（`clash-verge: src-tauri/src/utils/dirs.rs:23,109-111`）。

### 7.5 对重写的判断

| 格式 | v2rayN | clash-verge | X-Matrix 建议 |
|---|---|---|---|
| base64(URI 列表) | ✅ 完整支持，十余种协议 | ❌ | 必须支持（这是机场主流格式） |
| Clash YAML | 整包交给 mihomo | ✅ 原生支持 | 支持「整包 → sing-box 转换」或「整包直跑 mihomo」（首发只有 Xray/sing-box，故需转换器或标注不支持） |
| sing-box JSON | 整包直跑 | ❌ | 整包直跑（最省事且零信息损失） |
| Xray JSON | 整包直跑 | ❌ | 整包直跑 |

---

## 8. 架构与 IPC 契约（clash-verge，供 Rust 侧参考）

### 8.1 工程结构

Cargo workspace（`clash-verge: Cargo.toml:1-12`）：

```
src-tauri/                      主 crate（Tauri 应用 + 全部业务逻辑）
crates/clash-verge-draft        配置草稿/事务
crates/clash-verge-i18n         国际化
crates/clash-verge-limiter      限速
crates/clash-verge-logging      日志
crates/clash-verge-media-unlock 流媒体解锁检测
crates/clash-verge-signal       进程间信号
crates/tauri-plugin-clash-verge-sysinfo  系统信息命令
```

`src-tauri/src` 模块（`clash-verge: src-tauri/src/`）：

| 模块 | 职责 |
|---|---|
| `cmd/` | 暴露给前端的 Tauri command（app/backup/clash/lightweight/listener/network/profile/proxy/runtime/service/system/uwp/validate/verge/webdav） |
| `config/` | 配置模型与持久化（clash/verge/profiles/runtime/dns/port/mixed_port/snapshot/encrypt/prfitem） |
| `core/` | 内核管理（manager/{config,lifecycle,state}、service、sysopt、proxy_control、proxy_view、runstate、notify、tray、logger、autostart、updater、network_watch、validate…） |
| `feat/` | 功能编排（profile/proxy/clash/tun/dns/executor/effects/window/icon…） |
| `enhance/` | 配置增强（profile 的 script/merge 链） |
| `process/` | 异步任务句柄（`AsyncHandler`） |
| `utils/` | 目录、网络、模板、单例等 |

### 8.2 IPC 契约

**生成方式**：`tauri-specta` 从 Rust 侧收集命令并生成 TS 绑定。

- Rust 侧注册：`clash-verge: src-tauri/src/ipc.rs:1-233`（`tauri_specta::Builder::new().types(...).commands(tauri_specta::collect_commands![...])`）。
- 前端侧产物：`clash-verge: src/services/bindings.ts`（类型自动生成，勿手改）。

**统一返回类型**（`clash-verge: src-tauri/src/cmd/mod.rs:4-17`）：

```rust
pub type CmdResult<T = ()> = Result<T, CommandFailure>;

pub struct CommandFailure {
    pub code: Option<String>,      // 稳定的前端可识别错误码
    pub detail: String,            // 人类可读诊断信息
    pub operation: Option<FailedOperation>,  // 失败的操作类别
}
```

**值得照搬的设计点**：
1. **错误不是字符串**：`code` 给前端做分支（弹特定对话框/重试），`detail` 给人看，`operation` 给「按操作类别归档的失败通知」用。
2. **命令集合即 API 清单**：`ipc.rs` 里一个数组就是全部前后端契约，配合 specta 自动生成 TS 类型，天然不会漂移。
3. **命令命名按域分组**：`get_*` / `set_*` / `take_*`（`take_*` 表示「读取并清除」的一次性通知，如 `take_service_fallback_notice`、`take_dns_override_notice`）。

### 8.3 配置文件布局

| 文件 | 用途 | 来源 |
|---|---|---|
| `<data_dir>/<APP_ID>/config.yaml` | Clash 配置（用户可编辑的主配置） | `clash-verge: src-tauri/src/utils/dirs.rs:21,101-103` |
| `<data_dir>/<APP_ID>/verge.yaml` | 应用设置（`IVerge`） | `dirs.rs:22,105-107` |
| `<data_dir>/<APP_ID>/profiles.yaml` | 订阅/配置项索引（`IProfiles`） | `dirs.rs:23,109-111` |
| `<data_dir>/<APP_ID>/clash-verge.yaml` | **运行时**最终 YAML（真正喂给内核的那份） | `clash-verge: src-tauri/src/constants.rs:64` |
| `<data_dir>/<APP_ID>/dns_config.yaml` | DNS 配置 | `constants.rs:66` |
| `<data_dir>/<APP_ID>/window_state.json` | 窗口状态 | `constants.rs:67` |

`APP_ID = "io.github.clash-verge-rev.clash-verge-rev"`（`dirs.rs:12`；dev 构建为 `...dev`，`dirs.rs:17`）。

> **关键设计**：**用户配置（config.yaml）与运行时配置（clash-verge.yaml）分离**。生成时把用户配置 + 增强链 + 运行时补丁合并写成 `clash-verge.yaml` 再启动内核。这样用户手改的配置不会被程序覆盖，程序注入的字段（端口、external-controller、TUN）也不会污染用户文件。X-Matrix 应采纳同样的分层。

### 8.4 默认端口（clash-verge 与 v2rayN 的差异）

`clash-verge: src-tauri/src/constants.rs:3-12`：`DEFAULT_EXTERNAL_CONTROLLER = "127.0.0.1:9097"`、`DEFAULT_MIXED = 7897`、`DEFAULT_SOCKS = 7898`、`DEFAULT_HTTP = 7899`、`DEFAULT_REDIR = 7895`（非 Windows）、`DEFAULT_TPROXY = 7896`（Linux）。

v2rayN 用 10808 系。X-Matrix 可任选，但**必须把「控制端口」与「业务端口」分开且都做占用探测**。

---

## 9. 对 Rust 重写的直接结论（建议，非上游事实）

1. **配置生成**：采用「内嵌最小模板 + 分段生成器」模式（v2rayN 已证明可行），但**不要复刻 v2rayN 的扁平 settings 中间态**，直接产出内核原生 JSON。每段生成器单独可测（v2rayN 有 `ServiceLib.Tests/CoreConfig/` 整套对照测试，这个做法值得学）。
2. **配置分层**：用户配置 / 运行时配置分离（clash-verge 模式），配合 `config/snapshot.rs` 式的写盘快照回滚。
3. **端口**：控制端口（Xray metrics / sing-box clash_api）与业务端口分开，都做 `GetFreePort` 探测并缓存；TUN 场景注意 +1 避让（v2rayN `AppManager.StatePort2`）。
4. **就绪判定**：用「内核 API 可应答」而不是端口连通（clash-verge 模式）。Xray 可用 `GET /debug/vars`；sing-box 用 `GET /version`（Clash API）。超时预算参考 30×100ms。
5. **顺序**：启动 = 内核就绪 → 开系统代理；停止 = 先关系统代理 → 再杀内核。失败回滚必须补一次「清代理」。
6. **子进程托管**：Windows Job Object `KILL_ON_JOB_CLOSE`，handle 由内核管理器持有，`drop` 即连带杀子进程。
7. **统计抽象**：内部统一为「累计字节数」，Xray 直接取 `/debug/vars` 累计值，sing-box 把 `/traffic` 的瞬时速率积分成累计（或两条链路分别暴露，UI 层再统一）。
8. **连接列表**：sing-box 走 `/connections`；**Xray 首发可能没有连接列表**，UI 需要按内核能力降级（这一点必须在产品层确认，见 §10-2）。
9. **系统代理**：两家上游都没有「快照还原」，若要此功能须自研（§6.2 末）。`INTERNET_OPTION_SETTINGS_CHANGED` + `REFRESH` 两次通知不能漏。
10. **IPC**：用 `tauri-specta` 生成 TS 绑定；错误用结构化 `{code?, detail, operation?}` 而非字符串。

---

## 10. 未查证清单（明确没有查到，不编造）

| # | 条目 | 原因 |
|---|---|---|
| 1 | **Xray gRPC StatsService**：服务名、方法名、protobuf 请求/响应结构、`api` 段配置写法、`policy.system.statsInboundUplink/statsInboundDownlink` 语义 | 两份上游均未使用 gRPC StatsService；v2rayN 全仓无 `StatsService` / `v2ray.core.app.stats` 引用，且 `V2rayConfig` 模型无 `api` 字段。需查 Xray 官方文档（本次离线） |
| 2 | **Xray 的「当前连接列表」接口** | v2rayN 未实现（`ConnectionHandler` 只做可用性/延迟/IP 查询）；Xray 内核是否提供连接枚举 —— 未查证 |
| 3 | **v2rayN 扁平 `settings` → Xray `settings.vnext` / `settings.servers` 的转换实现** | 全仓 `vnext` 仅出现在自定义模板读取与测试 JSON 中，转换逻辑不在可见代码里（可能在序列化器或内核兼容层） |
| 4 | **clash-verge 系统代理的 Windows 注册表/WinINet 实现细节** | 实现位于外部 crate `sysproxy` 0.7.2（git rev `2c9d0a5`），源码不在本仓库 |
| 5 | **`tauri-plugin-mihomo` 的内部端点与消息格式** | 外部 git 依赖（`Cargo.toml:94` / `package.json:84`），源码不在本仓库。文中 `/connections` 字段以 v2rayN 的 DTO 与 clash-verge 前端类型为准 |
| 6 | **sing-box Clash API 的权威 schema** | 仓库内只有消费方代码与 DTO，没有 schema 定义；`/configs`、`/memory`、`/rules`、`/providers/rules`、`/logs` 的返回结构未查证 |
| 7 | **v2rayN Xray DNS 段的 servers 列表构造细节** | `V2rayDnsService.cs` 共 569 行，本次只读了入口与分支骨架（`:5-48`、`:443-455`），未逐行展开 |
| 8 | **v2rayN sing-box DNS / 路由 / ruleset 的完整字段映射** | `SingboxDnsService.cs`(720) / `SingboxRoutingService.cs`(626) / `SingboxRulesetService.cs`(132) 未细读 |
| 9 | **clash-verge 的 mihomo 配置生成（增强链 script/merge 的具体语义）** | `src-tauri/src/enhance/` 未展开阅读 |
| 10 | **v2rayN 的 `CoreConfigContextBuilder` 上下文构建规则**（哪些开关会启用 TUN / 前置内核等） | `Handler/Builder/CoreConfigContextBuilder.cs`(562 行) 未细读 |

---

## 11. 关键源码索引（便于复核）

**v2rayN（`D:\GitHub-Warehouse\_refs\v2rayN\`）**

| 主题 | 文件 |
|---|---|
| Xray 配置生成主流程 | `v2rayN/ServiceLib/Services/CoreConfig/V2ray/CoreConfigV2rayService.cs` |
| Xray 入站 / 出站 / 路由 / 统计 / 日志 | `.../V2ray/V2rayInboundService.cs`、`V2rayOutboundService.cs`、`V2rayRoutingService.cs`、`V2rayStatisticService.cs`、`V2rayLogService.cs` |
| sing-box 配置生成主流程 | `v2rayN/ServiceLib/Services/CoreConfig/Singbox/CoreConfigSingboxService.cs` |
| sing-box 入站 / 统计(experimental) | `.../Singbox/SingboxInboundService.cs`、`SingboxStatisticService.cs` |
| 配置模型 | `v2rayN/ServiceLib/Models/CoreConfigs/V2rayConfig.cs`、`SingboxConfig.cs`、`CoreInfo.cs` |
| 内嵌样板 | `v2rayN/ServiceLib/Sample/{SampleClientConfig, SampleInbound, SingboxSampleClientConfig, SingboxSampleOutbound}` |
| 内核进程管理 | `v2rayN/ServiceLib/Manager/CoreManager.cs`、`Services/ProcessService.cs`、`Services/WindowsJobService.cs`、`Manager/CoreInfoManager.cs` |
| 流量统计 | `v2rayN/ServiceLib/Services/Statistics/StatisticsXrayService.cs`、`StatisticsSingboxService.cs` |
| Clash API 客户端 | `v2rayN/ServiceLib/Manager/ClashApiManager.cs` |
| 连接列表 DTO / 视图 | `v2rayN/ServiceLib/Models/Dto/ClashConnections.cs`、`v2rayN/ServiceLib/ViewModels/ClashConnectionsViewModel.cs` |
| 系统代理 | `v2rayN/ServiceLib/Handler/SysProxy/SysProxyHandler.cs`、`ProxySettingWindows.cs`、`Manager/PacManager.cs` |
| 订阅解析 | `v2rayN/ServiceLib/Handler/SubscriptionHandler.cs`、`Handler/Data/ConfigDbHandler.cs`、`Handler/Fmt/*.cs` |

**clash-verge（`D:\GitHub-Warehouse\_refs\clash-verge-rev\`）**

| 主题 | 文件 |
|---|---|
| 内核生命周期 | `src-tauri/src/core/manager/lifecycle.rs`、`core/manager/state.rs`、`core/manager/mod.rs` |
| 系统代理 | `src-tauri/src/core/sysopt.rs`、`core/proxy_control.rs` |
| 配置模型与持久化 | `src-tauri/src/config/{verge,clash,profiles,prfitem,snapshot}.rs` |
| IPC 契约 | `src-tauri/src/ipc.rs`、`src-tauri/src/cmd/mod.rs`、`src/services/bindings.ts` |
| 常量与端口 | `src-tauri/src/constants.rs`、`utils/dirs.rs` |
| 连接列表（前端） | `src/hooks/use-connection-data.ts`、`src/types/global.d.ts` |
| 流量（前端） | `src/hooks/use-traffic-data.ts`、`src/hooks/use-traffic-monitor.ts` |

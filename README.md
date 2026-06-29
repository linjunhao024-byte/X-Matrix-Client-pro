<div align="center">

<img src="https://capsule-render.vercel.app/api?type=waving&color=0:0f0f0f,50:3b82f6,100:6366f1&height=220&section=header&text=X-Matrix&fontSize=60&fontColor=ffffff&fontAlignY=35&desc=Advanced%20Xray%20Core%20Guardian&descSize=15&descColor=00f0ff&descAlignY=55&animation=twinkling" width="100%"/>

<br/>

![License](https://img.shields.io/badge/License-MIT-blue?style=flat-square)
![Python](https://img.shields.io/badge/Python-3.14-3776AB?style=flat-square&logo=python&logoColor=white)
![Alpine.js](https://img.shields.io/badge/Alpine.js-3.x-8BC0D0?style=flat-square&logo=alpine.js&logoColor=white)
![TailwindCSS](https://img.shields.io/badge/TailwindCSS-3.x-06B6D4?style=flat-square&logo=tailwindcss&logoColor=white)
![Xray](https://img.shields.io/badge/Xray-Core-FF6600?style=flat-square)
![Zero Deps](https://img.shields.io/badge/Backend-Zero%20External%20Deps-brightgreen?style=flat-square)

<br/>

**高性能 · 极客风 · Xray 核心网络代理守护程序与图形化客户端**

*A high-performance, geek-style desktop proxy guardian powered by Xray Core*

<br/>

</div>

---

## ⚡ 架构概览 | Architecture Overview

```text
╔══════════════════════════════════════════════════════════════╗
║                    X-Matrix Client v1.3.0                    ║
╠══════════════════════════════════════════════════════════════╣
║                                                              ║
║  ┌──────────────┐  pywebview JS Bridge   ┌──────────────┐   ║
║  │   Frontend    │◄═════════════════════►│   Backend     │   ║
║  │  HTML5 / JS   │  window.pywebview.api │  Python        │   ║
║  │  Alpine.js    │                       │  Zero Deps     │   ║
║  │  TailwindCSS  │                       │  ctypes / sock │   ║
║  │  ECharts      │                       │                │   ║
║  └───────┬───────┘                       └───────┬────────┘   ║
║          │                                       │            ║
║          ▼                                       ▼            ║
║  ┌──────────────┐                       ┌──────────────┐     ║
║  │   WebView2    │                       │ xmatrix-core │     ║
║  │  (Edge Eng)   │                       │  (Xray Bin)  │     ║
║  └──────────────┘                       └──────────────┘     ║
║                                                              ║
╚══════════════════════════════════════════════════════════════╝
```

---

## ✨ 核心特性 | Core Features

### 🔐 协议全覆盖与极速解析 | Full Protocol Coverage

支持 **VLESS**（REALITY / XTLS Vision）、**VMess**、**Trojan**、**Shadowsocks**、**SOCKS5**、**HTTP** 六大协议。传输层覆盖 TCP、WebSocket、gRPC、KCP（mKCP）。智能剪贴板 URI 捕获，`Ctrl+V` 秒级批量导入节点。

```text
╔════════════════════════════════════════════════╗
║  Protocol  │ Transport  │ Security             ║
╠════════════════════════════════════════════════╣
║  VLESS     │ TCP/WS/    │ REALITY / TLS / None ║
║  VMess     │ gRPC/KCP   │ TLS / None           ║
║  Trojan    │            │ TLS / None           ║
║  SS        │            │ None                 ║
║  SOCKS5    │            │ None                 ║
║  HTTP      │            │ None                 ║
╚════════════════════════════════════════════════╝
```

### 📊 零开销全链路监控 | Zero-Overhead Traffic Monitor

后端手写原生 Socket 解析 HTTP/2 + gRPC 帧，直接与 Xray StatsService 通信。**零子进程、零第三方库、零序列化开销**。毫秒级上/下行流量统计，前端 ECharts 实时渲染。

```
╔════════════════════════════════════════════════════════╗
║  gRPC StatsService Query Pipeline                      ║
╠════════════════════════════════════════════════════════╣
║                                                        ║
║  Python Socket                                         ║
║      |                                                 ║
║      +-> HTTP/2 PRI Magic                              ║
║      +-> SETTINGS Frame                                ║
║      +-> HEADERS Frame (HPACK Literal)                 ║
║      +-> DATA Frame (Protobuf QueryStats)              ║
║      +-> GOAWAY Frame Detection                        ║
║      |                                                 ║
║      <-> Protobuf Varint Decode -> Stats Map           ║
║      |                                                 ║
║      +-> { uplink, downlink, up_speed, down_speed }    ║
║                                                        ║
║  Latency: <5ms  |  Overhead: 0  |  Dependencies: 0    ║
╚════════════════════════════════════════════════════════╝
```

### 🔍 极客级网络溯源 | Deep IP Trace

内置硬核节点体检流水线：

```
╔═══════════════════════════════════════════════════════════╗
║  Deep IP Trace Pipeline                                   ║
╠═══════════════════════════════════════════════════════════╣
║                                                           ║
║  [1] IPPure Fraud Score --- 0-100 Risk Assessment         ║
║      +-> Residential IP Detection                         ║
║      +-> ASN / Org / ISP Fingerprinting                   ║
║      +-> Country + GeoIP Cross-Reference                  ║
║                                                           ║
║  [2] WebRTC Leak Probe --- STUN Candidate Parse           ║
║      +-> Real IP vs Proxy IP Comparison                   ║
║      +-> LAN / Public IP Separation                       ║
║                                                           ║
║  [3] Streaming Unlock --- Per-Service HTTP Test           ║
║      +-> Netflix / Disney+ / ChatGPT / ...                ║
║      +-> Custom URL Add/Remove (localStorage)             ║
║                                                           ║
║  [4] Global Latency --- AWS Backbone Ping                 ║
║      +-> Multi-Region Concurrent Probe                    ║
║      +-> Color-Coded: <100ms <200ms >200ms                ║
║                                                           ║
╚═══════════════════════════════════════════════════════════╝
```

### 🗺️ 高阶流量接管与路由 | Advanced Routing

内置 BGP 路由表可视化拓扑图（动态 SVG 连线动画），支持规则/全局/直连三模式热切换，路由规则实时持久化。

```
╔════════════════════════════════════════════════════════════╗
║  Routing Engine                                            ║
╠════════════════════════════════════════════════════════════╣
║                                                            ║
║  +-----------+     +-----------+     +------------------+  ║
║  | Inbound   |---->| Router    |---->| Proxy (VLESS)    |  ║
║  | :2077     |     | Engine    |---->| Direct           |  ║
║  | Mixed     |     |           |---->| Block            |  ║
║  +-----------+     +-----------+     +------------------+  ║
║                                                            ║
║  Mode: [Rule] --- domain / ip / geosite / geoip            ║
║                                                            ║
║  Features:                                                 ║
║  +-> System Proxy (WinINet Registry)                       ║
║  +-> TUN Mode (System Stack / Auto-Route)                  ║
║  +-> FakeDNS (198.18.0.0/15 Pool)                          ║
║  +-> DNS Strategy (AsIs / IPIfNonMatch / IPOnDemand)       ║
║  +-> UWP Loopback Exemption (PowerShell)                   ║
║                                                            ║
╚════════════════════════════════════════════════════════════╝
```

### 🌐 多数据源 IP 溯源 | Multi-Source IP Detection

支持 IPPure（欺诈分+住宅检测）、IPInfo Lite（地理+ASN）、ip-api.com（代理检测+ISP）三大数据源切换，UI 自动适配显示不同字段。

### 📡 实时连接监控雷达 | Connection Radar

捕获 Xray 核心流量流向，实时洞察进程级网络行为。支持 proxy/direct/block 出站类型过滤，PID 进程映射，一键清空。

### 📦 Clash YAML / Base64 订阅导入 | Subscription Import

支持 Clash YAML 格式和 Base64 编码的 URI 列表批量导入，自动识别协议类型（VMess/VLESS/Trojan/SS）。

### 🔐 TLS Fragment 包分片防封锁 | Anti-Censorship

支持 TLS 握手包分片发送（tlshello/100-200/10-20），绕过 SNI 深度检测。仅在 TLS/REALITY 安全层下生效。

### 🖥️ 系统托盘动态菜单 | Dynamic Tray Menu

右键托盘图标：节点快速切换（radio 单选）、路由模式子菜单（规则/全局/直连）、系统代理开关、剪贴板导入、复制代理命令。

### 🎨 暗色主题 + 多语言 | Dark Theme + i18n

支持浅色/深色/跟随系统三种主题模式。中英双语界面一键切换，所有设置通过 Alpine.js `$watch` 自动持久化到 localStorage。

### 🔧 TUN 高级配置 | TUN Advanced Config

MTU、网络栈（system/gvisor/mixed）、自动路由、严格路由、绕过地址（Route Exclude Address）全部可配置，对标 V2RayN。

### ✅ SHA256 核心签名校验 | Core Signature Verification

核心自动更新时下载 `.dgst` 签名文件，与下载包的 SHA256 哈希比对，防止供应链投毒。

### ⚡ 批量测速 + 延迟排序 | Batch Speed Test

一键并发测速所有节点，支持自定义并发数和取消。测速完成后可按延迟升序一键排列节点。

### 📋 V2RayN 风格日志系统 | V2RayN-Style Logging

完整复刻 V2RayN 的多层级日志输出：子系统标签着色（proxy/transport/app）、连接 ID 追踪、出站类型过滤（代理/直连/拦截）。

---

## 🚀 快速开始 | Quick Start

### 环境要求 | Prerequisites

```
╔══════════════════════════════════════╗
║  Requirement    │ Version            ║
╠══════════════════════════════════════╣
║  OS             │ Windows 10/11 x64  ║
║  Python         │ 3.10+ (3.14 rec.)  ║
║  WebView2       │ Edge Chromium      ║
║  xmatrix-core   │ Bundled (Xray)     ║
╚══════════════════════════════════════╝
```

### 安装与运行 | Install & Run

```bash
# 1. Clone the repository
git clone https://github.com/linjunhao024-byte/X-Matrix-Client.git
cd X-Matrix-Client

# 2. Install dependencies
pip install pywebview pystray Pillow

# 3. Run directly
python main.py

# 4. Or build standalone executable
build.bat
# Output: release/X-Matrix/X-Matrix.exe
```

---

## 🏗️ 项目结构 | Project Structure

```
X-Matrix-Client-pro/                     # 总大小: 286MB
│
├── 📁 .claude/                          # Claude AI配置
│
├── 📁 data/                             # 用户数据目录
│   ├── config.json                      # 主配置文件
│   ├── tunnels.json                     # 节点数据
│   ├── stats.json                       # 流量统计
│   └── xmatrix.db                       # SQLite数据库
│
├── 📁 modules/                          # 前端JS模块 (10个)
│   ├── config.js                        # 配置管理
│   ├── i18n.js                          # 国际化
│   ├── init.js                          # 初始化
│   ├── inspection.js                    # 检测功能
│   ├── monitor.js                       # 监控功能
│   ├── nodes.js                         # 节点管理
│   ├── routing.js                       # 路由管理
│   ├── settings.js                      # 设置功能
│   ├── state.js                         # 状态管理
│   └── utils.js                         # 工具函数
│
├── 📁 static/                           # 静态资源
│   └── app.js                           # Alpine.js主入口
│
├── 📁 xmatrix/                          # 后端Python代码
│   ├── api.py                           # API主类
│   ├── constants.py                     # 常量定义
│   ├── helpers.py                       # 辅助函数
│   ├── process.py                       # 进程管理
│   ├── core/                            # 核心管理
│   ├── nodes/                           # 节点CRUD
│   ├── routing/                         # 路由引擎
│   └── storage/                         # 数据存储
│
├── 📄 main.py                           # 主程序入口
├── 📄 index.html                        # 前端页面 (341KB)
├── 📄 package.json                      # npm配置
├── 📄 tailwind.config.js                # Tailwind配置
├── 📄 input.css                         # CSS输入
├── 📄 build.bat                         # 打包脚本
├── 📄 X-Matrix.spec                     # PyInstaller配置
├── 📄 validate_html.py                  # HTML验证工具
├── 📄 README.md                         # 项目文档
├── 📄 .gitignore                        # Git忽略规则
├── 📄 icon.ico                          # 应用图标
│
├── 🔧 tailwindcss-windows-x64.exe       # Tailwind CSS工具 (61MB)
│
├── 🌐 xmatrix-core.exe                  # Xray核心 (35MB)
├── 🌐 mihomo.exe                        # mihomo核心 (46MB)
├── 🌐 sing-box.exe                      # sing-box核心 (43MB)
├── 🌐 brook.exe                         # Brook核心 (30MB)
├── 🌐 hysteria.exe                      # Hysteria核心 (21MB)
├── 🌐 naive.exe                         # NaiveProxy核心 (9.5MB)
├── 🌐 tuic-client.exe                   # TUIC核心 (2MB)
│
├── 📊 geoip.dat                         # IP地理数据 (18MB)
└── 📊 geosite.dat                       # 域名地理数据 (10MB)
```

---

## 🔧 技术栈 | Tech Stack

```
╔═══════════════════════════════════════════════════════════╗
║  Layer          │ Technology                              ║
╠═══════════════════════════════════════════════════════════╣
║  Runtime        │ Python 3.14 + pywebview (WebView2)      ║
║  Frontend       │ HTML5 + TailwindCSS CDN + Alpine.js 3   ║
║  Charts         │ ECharts 5.5 (Realtime Traffic)          ║
║  Proxy Core     │ Xray Core (xmatrix-core.exe)            ║
║  IPC            │ pywebview JS Bridge (window.pywebview)  ║
║  Stats          │ Native Socket HTTP/2 + gRPC (0 deps)    ║
║  System Proxy   │ ctypes → WinINet Registry               ║
║  Tray           │ pystray + PIL (Icon Generation)         ║
║  Packaging      │ PyInstaller --onedir                    ║
╚═══════════════════════════════════════════════════════════╝
```

---

## 📡 API 接口 | Backend API Reference

X-Matrix 后端通过 `window.pywebview.api.*` 暴露 **40+ 个公开方法**，100% 前端覆盖率。

```
╔═══════════════════════════════════════════════════════════════════════╗
║  Category        │ Methods                                           ║
╠═══════════════════════════════════════════════════════════════════════╣
║  Node CRUD       │ get_tunnels, add_tunnel, delete_tunnel,           ║
║                  │ delete_tunnels_batch, update_tunnel,               ║
║                  │ reorder_tunnels, apply_tunnels_order,             ║
║                  │ import_config, import_uri, import_subscription,   ║
║                  │ import_routing_rules, export_uri, export_config,  ║
║                  │ get_routing_topology                              ║
╠═══════════════════════════════════════════════════════════════════════╣
║  Core Lifecycle  │ start_core, stop_core, activate_tunnel,           ║
║                  │ get_core_status, save_config, preview_config,     ║
║                  │ validate_config, save_raw_config, update_core     ║
╠═══════════════════════════════════════════════════════════════════════╣
║  System          │ toggle_system_proxy, toggle_auto_startup,         ║
║                  │ set_close_behavior, check_auto_startup,           ║
║                  │ exempt_uwp_loopback, get_sys_info                 ║
╠═══════════════════════════════════════════════════════════════════════╣
║  Monitoring      │ fetch_traffic_stats, fetch_logs,                  ║
║                  │ fetch_connections, clear_connections,              ║
║                  │ check_outbound_ip, check_ip_quality,              ║
║                  │ test_website_access, update_geo_data              ║
╠═══════════════════════════════════════════════════════════════════════╣
║  Speed Test      │ test_node_tcp_ping, test_node_real_delay,         ║
║                  │ test_download_speed, test_port                    ║
╚═══════════════════════════════════════════════════════════════════════╝
```

---

## ⚙️ 配置参数 | Configuration

### 核心入站配置 | Inbound Config

```
╔═══════════════════════════════════════════════════════╗
║  Parameter       │ Default    │ Description           ║
╠═══════════════════════════════════════════════════════╣
║  localPort       │ 2077       │ Mixed Proxy Port      ║
║  allowLan        │ false      │ LAN Access            ║
║  enableUdp       │ true       │ UDP Relay             ║
║  sniffing        │ true       │ Traffic Sniffing      ║
║  sniffTypes      │ [http,tls] │ Sniff Protocols       ║
║  dnsStrategy     │ AsIs       │ DNS Resolution        ║
║  enableFakeDns   │ false      │ FakeDNS Hijack        ║
║  proxyMode       │ rule       │ rule / global / direct║
║  tunMode         │ false      │ TUN Inbound           ║
╚═══════════════════════════════════════════════════════╝
```

### 运行时状态持久化 | State Persistence

所有用户配置通过 Alpine.js `$watch` 自动同步至 `localStorage`，刷新即恢复：

```
╔══════════════════════════════════════════════════════════════╗
║  Key                        │ Scope                          ║
╠══════════════════════════════════════════════════════════════╣
║  xmatrix_config_state       │ Port / LAN / UDP / Sniffing    ║
║  xmatrix_adv_dns            │ DNS Strategy                   ║
║  xmatrix_adv_fakedns        │ FakeDNS Toggle                 ║
║  xmatrix_adv_log            │ Log Level                      ║
║  xmatrix_proxy_mode         │ Rule / Global / Direct         ║
║  xmatrix_tun_mode           │ TUN Toggle                     ║
║  xmatrix_routing_rules      │ Custom Routing Rules           ║
║  xmatrix_theme              │ Light / Dark / System          ║
║  xmatrix_language           │ zh-CN / en-US                  ║
║  xmatrix_last_node          │ Last Active Node Index         ║
║  xmatrix_custom_sites       │ Custom Test URLs               ║
╚══════════════════════════════════════════════════════════════╝
```

---

## 🛡️ 安全说明 | Security Notes

```
╔══════════════════════════════════════════════════════════════╗
║  ⚠️  IMPORTANT                                              ║
╠══════════════════════════════════════════════════════════════╣
║                                                              ║
║  • 本项目仅用于学习与合法的网络代理用途                          ║
║  • This project is for educational and legal proxy use only  ║
║  • 请遵守您所在地区的相关法律法规                               ║
║  • Please comply with local laws and regulations             ║
║  • xmatrix-core.exe 基于 Xray Core 开源项目                   ║
║  • 系统代理修改仅限当前用户的 Windows 注册表                     ║
║                                                              ║
╚══════════════════════════════════════════════════════════════╝
```

---

## 📜 许可证 | License

本项目基于 [MIT License](LICENSE) 开源。

```
╔═══════════════════════════════════════════════════════╗
║  MIT License                                          ║
║  Copyright (c) 2026 Junhao Lin                        ║
║                                                       ║
║  Permission is hereby granted, free of charge, to     ║
║  any person obtaining a copy of this software...      ║
╚═══════════════════════════════════════════════════════╝
```

---

<div align="center">

**Built with 🔥 by [Junhao Lin](https://github.com/linjunhao024-byte)**

<img src="https://capsule-render.vercel.app/api?type=waving&color=0:0f0f0f,50:3b82f6,100:6366f1&height=120&section=footer&text=%20&fontSize=0" width="100%"/>

</div>

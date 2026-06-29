<div align="center">

<img src="https://capsule-render.vercel.app/api?type=rect&color=0:0f0f0f,100:1a1a1a&height=180&section=header&text=X-Matrix-Client-pro&fontSize=50&fontColor=fce300&fontAlignY=45&desc=Advanced%20Matrix%20Terminal%20Core&descSize=16&descColor=00f0ff&descAlignY=65" width="100%"/>

<br/>

![Language](https://img.shields.io/badge/Language-Python-3776AB?style=flat-square&logo=python&logoColor=white)
![Stage](https://img.shields.io/badge/Stage-Development-FCA121?style=flat-square)
![Contributions](https://img.shields.io/badge/Contributions-Welcome-00f0ff?style=flat-square)
![License](https://img.shields.io/badge/License-MIT-ff69b4?style=flat-square)
![Platform](https://img.shields.io/badge/Platform-Windows-0078D6?style=flat-square&logo=windows&logoColor=white)

<br/>

> 🛠️ **工程定位 | Project Status**
>
> 本项目目前定位为【纯开发/实验性项目】。核心架构正处于快速迭代期，尚未经过大规模真实用户的使用体验与多场景生产环境的严苛验证。
>
> 🤝 **开源协作 | Open Source Collaboration**
>
> 深度践行开源极客精神，防线的完善离不开社区。热烈欢迎全球开发者前来 **Fork** 仓库、通过 **Issues** 提交 Bug 或改进建议，并积极提交 **Pull Requests** 参与联合协同开发！

</div>

---

## ⚡ 架构概览 | Architecture Overview

```
╔══════════════════════════════════════════════════════════════╗
║                    X-Matrix Client v1.3.0                    ║
╠══════════════════════════════════════════════════════════════╣
║                                                              ║
║  ┌──────────────┐  pywebview JS Bridge   ┌──────────────┐   ║
║  │   Frontend    │◄═════════════════════►│   Backend     │   ║
║  │  HTML5 / JS   │  window.pywebview.api │  Python        ║
║  │  Alpine.js    │                       │  Zero Deps     ║
║  │  TailwindCSS  │                       │  ctypes / sock ║
║  │  ECharts      │                       │                ║
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

### 🔐 协议全覆盖 | Full Protocol Coverage

支持 **VLESS**（REALITY / XTLS Vision）、**VMess**、**Trojan**、**Shadowsocks**、**SOCKS5**、**HTTP** 六大协议。传输层覆盖 TCP、WebSocket、gRPC、KCP（mKCP）。智能剪贴板 URI 捕获，`Ctrl+V` 秒级批量导入节点。

```
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

### 📊 零开销监控 | Zero-Overhead Monitor

后端手写原生 Socket 解析 HTTP/2 + gRPC 帧，直接与 Xray StatsService 通信。**零子进程、零第三方库、零序列化开销**。毫秒级上/下行流量统计，前端 ECharts 实时渲染。

```
╔════════════════════════════════════════════════════════╗
║  gRPC StatsService Query Pipeline                      ║
╠════════════════════════════════════════════════════════╣
║                                                        ║
║  Python Socket ──> HTTP/2 PRI Magic                    ║
║               ──> SETTINGS Frame                      ║
║               ──> HEADERS Frame (HPACK Literal)       ║
║               ──> DATA Frame (Protobuf QueryStats)    ║
║               ──> GOAWAY Frame Detection              ║
║                                                        ║
║  <-> Protobuf Varint Decode -> Stats Map               ║
║  +-> { uplink, downlink, up_speed, down_speed }       ║
║                                                        ║
║  Latency: <5ms  |  Overhead: 0  |  Dependencies: 0    ║
╚════════════════════════════════════════════════════════╝
```

### 🔍 深度溯源 | Deep IP Trace

内置硬核节点体检流水线：IPPure 欺诈分、WebRTC 泄漏探测、流媒体解锁测试、全球延迟探测。

```
╔═══════════════════════════════════════════════════════════╗
║  Deep IP Trace Pipeline                                   ║
╠═══════════════════════════════════════════════════════════╣
║  [1] IPPure Fraud Score ─── 0-100 Risk Assessment         ║
║  [2] WebRTC Leak Probe ─── STUN Candidate Parse           ║
║  [3] Streaming Unlock  ─── Netflix / Disney+ / ChatGPT    ║
║  [4] Global Latency    ─── Multi-Region Concurrent Probe  ║
╚═══════════════════════════════════════════════════════════╝
```

### 🗺️ 高阶路由 | Advanced Routing

内置 BGP 路由表可视化拓扑图（动态 SVG 连线动画），支持规则/全局/直连三模式热切换，路由规则实时持久化。

```
╔════════════════════════════════════════════════════════════╗
║  Routing Engine                                            ║
╠════════════════════════════════════════════════════════════╣
║  Inbound(:2077) ──> Router Engine ──> Proxy / Direct / Block║
║                                                            ║
║  Features:                                                 ║
║  +-> System Proxy (WinINet Registry)                       ║
║  +-> TUN Mode (System Stack / Auto-Route)                  ║
║  +-> FakeDNS (198.18.0.0/15 Pool)                          ║
║  +-> DNS Strategy (AsIs / IPIfNonMatch / IPOnDemand)       ║
╚════════════════════════════════════════════════════════════╝
```

### 🌐 多数据源 | Multi-Source Detection

支持 IPPure（欺诈分+住宅检测）、IPInfo Lite（地理+ASN）、ip-api.com（代理检测+ISP）三大数据源切换。

### 📡 连接雷达 | Connection Radar

捕获 Xray 核心流量流向，实时洞察进程级网络行为。支持 proxy/direct/block 出站类型过滤，PID 进程映射，一键清空。

### 📦 订阅导入 | Subscription Import

支持 Clash YAML 格式和 Base64 编码的 URI 列表批量导入，自动识别协议类型（VMess/VLESS/Trojan/SS）。

### 🔐 防封锁 | Anti-Censorship

支持 TLS 握手包分片发送（tlshello/100-200/10-20），绕过 SNI 深度检测。

### 🖥️ 系统托盘 | System Tray

右键托盘图标：节点快速切换（radio 单选）、路由模式子菜单（规则/全局/直连）、系统代理开关、剪贴板导入。

### 🎨 暗色主题 | Dark Theme

支持浅色/深色/跟随系统三种主题模式。中英双语界面一键切换，所有设置通过 Alpine.js `$watch` 自动持久化到 localStorage。

### 🔧 TUN 高级配置 | TUN Config

MTU、网络栈（system/gvisor/mixed）、自动路由、严格路由、绕过地址全部可配置，对标 V2RayN。

### ✅ 核心签名校验 | Signature Verification

核心自动更新时下载 `.dgst` 签名文件，与下载包的 SHA256 哈希比对，防止供应链投毒。

### ⚡ 批量测速 | Batch Speed Test

一键并发测速所有节点，支持自定义并发数和取消。测速完成后可按延迟升序一键排列节点。

### 📋 日志系统 | Logging System

完整复刻 V2RayN 的多层级日志输出：子系统标签着色（proxy/transport/app）、连接 ID 追踪、出站类型过滤。

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
git clone https://github.com/linjunhao024-byte/X-Matrix-Client-pro.git
cd X-Matrix-Client-pro

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
│   ├── app.js                           # Alpine.js主入口
│   ├── alpine.min.js                    # Alpine.js框架
│   ├── echarts.min.js                   # ECharts图表库
│   └── css/                             # CSS样式目录
│
├── 📁 xmatrix/                          # 后端Python代码
│   ├── api.py                           # API主类 (40+方法)
│   ├── constants.py                     # 常量定义
│   ├── helpers.py                       # 辅助函数
│   ├── process.py                       # 进程管理
│   ├── core/                            # 核心管理
│   ├── nodes/                           # 节点CRUD
│   ├── routing/                         # 路由引擎
│   ├── storage/                         # 数据存储
│   ├── monitoring/                      # 监控模块
│   ├── network/                         # 网络工具
│   ├── geo/                             # Geo数据管理
│   ├── download/                        # 下载管理
│   └── backup/                          # 备份恢复
│
├── 📄 main.py                           # 主程序入口
├── 📄 index.html                        # 前端页面
├── 📄 package.json                      # npm配置
├── 📄 tailwind.config.js                # Tailwind配置
├── 📄 input.css                         # CSS输入
├── 📄 build.bat                         # 打包脚本
├── 📄 icon.ico                          # 应用图标
│
├── 🌐 xmatrix-core.exe                  # Xray核心 (35MB)
├── 🌐 mihomo.exe                        # mihomo核心 (46MB)
├── 🌐 sing-box.exe                      # sing-box核心 (43MB)
├── 🌐 brook.exe                         # Brook核心 (30MB)
├── 🌐 hysteria.exe                      # Hysteria核心 (21MB)
├── 🌐 naive.exe                         # NaiveProxy核心 (9.5MB)
└── 🌐 tuic-client.exe                   # TUIC核心 (2MB)
```

---

## 🌐 多核心支持 | Multi-Core Support

X-Matrix 支持 7 种代理核心，可在设置中一键切换：

```
╔═══════════════════════════════════════════════════════════════╗
║  Core          │ Protocols                    │ Features      ║
╠═══════════════════════════════════════════════════════════════╣
║  Xray (默认)   │ VLESS, VMess, Trojan, SS     │ REALITY 支持  ║
║  sing-box      │ VLESS, VMess, Trojan, SS, Hy2│ 新一代内核    ║
║  mihomo        │ VMess, Trojan, SS, Hy2       │ Clash Meta    ║
║  Hysteria 2    │ Hysteria2                    │ UDP 加速      ║
║  Brook         │ Brook                        │ 轻量级        ║
║  NaiveProxy    │ NaiveProxy                   │ 抗封锁        ║
║  TUIC          │ TUIC                         │ QUIC 低延迟   ║
╚═══════════════════════════════════════════════════════════════╝
```

---

## 🔧 技术栈 | Tech Stack

```
╔═══════════════════════════════════════════════════════════╗
║  Layer          │ Technology                              ║
╠═══════════════════════════════════════════════════════════╣
║  Runtime        │ Python 3.14 + pywebview (WebView2)      ║
║  Frontend       │ HTML5 + TailwindCSS + Alpine.js 3       ║
║  Charts         │ ECharts 5.5 (Realtime Traffic)          ║
║  Proxy Core     │ Xray / sing-box / mihomo / ...          ║
║  IPC            │ pywebview JS Bridge (window.pywebview)  ║
║  Stats          │ Native Socket HTTP/2 + gRPC (0 deps)    ║
║  System Proxy   │ ctypes → WinINet Registry               ║
║  Database       │ SQLite + JSON Hybrid Storage             ║
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
║                  │ update_tunnel, reorder_tunnels, clone_tunnel,     ║
║                  │ import_uri, export_uri, import_config             ║
╠═══════════════════════════════════════════════════════════════════════╣
║  Core Lifecycle  │ start_core, stop_core, activate_tunnel,           ║
║                  │ get_core_status, save_config, preview_config      ║
╠═══════════════════════════════════════════════════════════════════════╣
║  System          │ toggle_system_proxy, toggle_auto_startup,         ║
║                  │ set_close_behavior, get_sys_info                  ║
╠═══════════════════════════════════════════════════════════════════════╣
║  Monitoring      │ fetch_traffic_stats, fetch_logs,                  ║
║                  │ fetch_connections, check_outbound_ip              ║
╠═══════════════════════════════════════════════════════════════════════╣
║  Speed Test      │ test_node_tcp_ping, test_node_real_delay,         ║
║                  │ test_download_speed, test_port                    ║
╚═══════════════════════════════════════════════════════════════════════╝
```

---

## ⚙️ 配置参数 | Configuration

### 入站配置 | Inbound Config

```
╔═══════════════════════════════════════════════════════╗
║  Parameter       │ Default    │ Description           ║
╠═══════════════════════════════════════════════════════╣
║  localPort       │ 2077       │ Mixed Proxy Port      ║
║  allowLan        │ false      │ LAN Access            ║
║  enableUdp       │ true       │ UDP Relay             ║
║  sniffing        │ true       │ Traffic Sniffing      ║
║  dnsStrategy     │ AsIs       │ DNS Resolution        ║
║  proxyMode       │ rule       │ rule / global / direct║
║  tunMode         │ false      │ TUN Inbound           ║
╚═══════════════════════════════════════════════════════╝
```

### 状态持久化 | State Persistence

所有用户配置通过 Alpine.js `$watch` 自动同步至 `localStorage`，刷新即恢复。

---

## 🔨 开发指南 | Development Guide

### 环境搭建 | Setup

```bash
# 1. 克隆项目
git clone https://github.com/linjunhao024-byte/X-Matrix-Client-pro.git
cd X-Matrix-Client-pro

# 2. 安装 Python 依赖
pip install pywebview pystray Pillow

# 3. 安装 Node.js 依赖 (可选，用于 Tailwind CSS)
npm install
```

### 开发命令 | Commands

```bash
# 启动应用
python main.py

# 启动 Tailwind CSS 监听
./tailwindcss-windows-x64.exe -i input.css -o static/css/output.css --watch

# 或使用 npm 脚本
npm run dev

# 打包为独立可执行文件
build.bat
```

### 代码规范 | Code Style

- **Python**: 遵循 PEP 8，使用类型注解
- **JavaScript**: 使用 ES6+，模块化组织
- **HTML**: 单文件组件，Alpine.js 指令

---

## 🤝 贡献指南 | Contributing

欢迎提交 Issue 和 Pull Request！

### 提交 Issue

- 🐛 Bug 报告：请包含复现步骤、错误日志
- 💡 功能建议：请描述使用场景
- 📝 文档改进：欢迎直接提交 PR

### 提交 PR

```bash
# 1. Fork 本仓库
# 2. 创建特性分支
git checkout -b feature/your-feature

# 3. 提交更改
git commit -m 'feat: add your feature'

# 4. 推送分支
git push origin feature/your-feature

# 5. 提交 Pull Request
```

---

## ❓ 常见问题 | FAQ

```
╔═══════════════════════════════════════════════════════════════╗
║  Q: 启动后白屏怎么办？                                        ║
║  A: 确保系统已安装 WebView2 运行时（Windows 10/11 通常已内置）║
╠═══════════════════════════════════════════════════════════════╣
║  Q: 如何切换代理核心？                                        ║
║  A: 设置 → 核心管理 → 选择核心 → 点击切换                    ║
╠═══════════════════════════════════════════════════════════════╣
║  Q: 节点导入失败？                                            ║
║  A: 检查 URI 格式，支持 vmess:// vless:// trojan:// ss://    ║
╠═══════════════════════════════════════════════════════════════╣
║  Q: 如何开启 TUN 模式？                                       ║
║  A: 设置 → TUN 配置 → 开启（需要管理员权限）                 ║
╚═══════════════════════════════════════════════════════════════╝
```

---

## 📊 更新日志 | Changelog

### v1.3.0 (2026-06-29)

- ✨ 新增 7 种代理核心支持
- ✨ 新增 BGP 路由拓扑可视化
- ✨ 新增 TLS Fragment 防封锁
- ✨ 新增多语言支持 (中/英)
- 🐛 修复节点 ID 缺失问题
- 🐛 修复 JS 控制台错误
- 📦 优化项目结构，瘦身至 286MB

### v1.2.0

- ✨ 新增策略组负载均衡
- ✨ 新增订阅自动刷新
- ✨ 新增 WebDAV 备份

### v1.1.0

- ✨ 新增 TUN 模式
- ✨ 新增 FakeDNS 支持
- ✨ 新增批量测速

---

## 🛡️ 安全说明 | Security Notes

```
╔══════════════════════════════════════════════════════════════╗
║  ⚠️  IMPORTANT                                              ║
╠══════════════════════════════════════════════════════════════╣
║  • 本项目仅用于学习与合法的网络代理用途                       ║
║  • This project is for educational and legal proxy use only  ║
║  • 请遵守您所在地区的相关法律法规                             ║
║  • xmatrix-core.exe 基于 Xray Core 开源项目                  ║
║  • 系统代理修改仅限当前用户的 Windows 注册表                  ║
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

<img src="https://capsule-render.vercel.app/api?type=rect&color=0:0f0f0f,100:1a1a1a&height=80&section=footer&text=%20" width="100%"/>

</div>

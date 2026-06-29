"""
X-Matrix Client — 路径常量、端口常量、核心注册表、Geo 数据源、DNS 预设
"""
from __future__ import annotations

import os
import sys

# ── 路径常量 ─────────────────────────────────────────────────────────
if getattr(sys, "frozen", False):
    _BASE = os.path.dirname(sys.executable)
    _RES = sys._MEIPASS
else:
    _BASE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    _RES = _BASE

DATA_DIR: str = os.path.join(_BASE, "data")
TUNNELS_FILE: str = os.path.join(DATA_DIR, "tunnels.json")
CONFIG_FILE: str = os.path.join(DATA_DIR, "config.json")
# ── 自定义配置文件（优先级系统） ────────────────────────────────────────
# 注意：默认配置路径由 save_config() 根据核心类型动态计算（CONFIG_FILE + config_ext）
CONFIG_FILE_CUSTOM: str = os.path.join(DATA_DIR, "config.custom.json")
CONFIG_FILE_SINGBOX_CUSTOM: str = os.path.join(DATA_DIR, "config.singbox.custom.json")
CONFIG_FILE_MIHOMO_CUSTOM: str = os.path.join(DATA_DIR, "config.mihomo.custom.yaml")
SUBSCRIPTIONS_FILE: str = os.path.join(DATA_DIR, "subscriptions.json")
PROFILES_FILE: str = os.path.join(DATA_DIR, "profiles.json")
DB_FILE: str = os.path.join(DATA_DIR, "xmatrix.db")
HTML_FILE: str = os.path.join(_RES, "index.html")

# 常量
CREATE_NO_WINDOW: int = 0x08000000 if os.name == "nt" else 0
API_PORT: int = 20085  # Xray gRPC StatsService 端口
CLASH_API_PORT: int = 9090  # sing-box / mihomo Clash API 端口

# ── 多核心注册表（纯数据定义，不影响现有 Xray 流程） ────────────────
CORE_REGISTRY: dict[str, dict] = {
    "xray": {
        "name": "Xray",
        "exe_names": ["xmatrix-core.exe", "xray.exe"],
        "github_repo": "XTLS/Xray-core",
        "args_template": ["run", "-c", "{config}"],
        "config_ext": "json",
        "env_vars": {"XRAY_LOCATION_ASSET": DATA_DIR},
    },
    "singbox": {
        "name": "sing-box",
        "exe_names": ["sing-box.exe", "sing-box"],
        "github_repo": "SagerNet/sing-box",
        "args_template": ["run", "-c", "{config}", "--disable-color"],
        "config_ext": "json",
        "env_vars": {"SINGBOX_LOCATION_ASSET": DATA_DIR},
    },
    "mihomo": {
        "name": "mihomo",
        "exe_names": ["mihomo.exe", "mihomo-windows-amd64.exe", "mihomo"],
        "github_repo": "MetaCubeX/mihomo",
        "args_template": ["-f", "{config}", "-d", os.path.join(DATA_DIR, "mihomo")],
        "config_ext": "yaml",
        "env_vars": {},
    },
    "hysteria": {
        "name": "Hysteria 2",
        "exe_names": ["hysteria.exe", "hysteria-windows-amd64.exe"],
        "github_repo": "apernet/hysteria",
        "args_template": ["--config", "{config}", "--log-level", "warn"],
        "config_ext": "yaml",
        "env_vars": {},
        "release_match": lambda name: "windows" in name.lower() and "amd64" in name.lower() and name.lower().endswith(".exe"),
    },
    "naiveproxy": {
        "name": "naiveproxy",
        "exe_names": ["naive.exe", "naiveproxy.exe"],
        "github_repo": "klzgrad/naiveproxy",
        "args_template": ["--config", "{config}"],
        "config_ext": "json",
        "env_vars": {},
        "release_match": lambda name: "-win-" in name.lower() and "x64" in name.lower() and name.lower().endswith(".zip"),
    },
    "tuic": {
        "name": "TUIC-Client",
        "exe_names": ["tuic-client.exe"],
        "github_repo": "EAimTY/tuic",
        "args_template": ["-c", "{config}"],
        "config_ext": "json",
        "env_vars": {},
        "release_match": lambda name: ("windows" in name.lower() or "win" in name.lower()) and ("x86_64" in name.lower() or "amd64" in name.lower()) and (name.lower().endswith(".zip") or name.lower().endswith(".exe")),
    },
    "brook": {
        "name": "Brook",
        "exe_names": ["brook.exe"],
        "github_repo": "txthinking/brook",
        "args_template": ["client", "-c", "{config}"],
        "config_ext": "json",
        "env_vars": {},
        "release_match": lambda name: "windows" in name.lower() and "amd64" in name.lower() and name.lower().endswith(".exe"),
    },
}

# Geo 数据下载源配置
GEO_SOURCES: dict[str, dict] = {
    "loyalsoldier": {
        "name": "Loyalsoldier (v2ray-rules-dat)",
        "files": {
            "geoip.dat": "https://github.com/Loyalsoldier/v2ray-rules-dat/releases/latest/download/geoip.dat",
            "geosite.dat": "https://github.com/Loyalsoldier/v2ray-rules-dat/releases/latest/download/geosite.dat",
        },
    },
    "chocolate4u": {
        "name": "Chocolate4U (v2ray-rules-dat)",
        "files": {
            "geoip.dat": "https://github.com/chocolate4u/v2ray-rules-dat/releases/latest/download/geoip.dat",
            "geosite.dat": "https://github.com/chocolate4u/v2ray-rules-dat/releases/latest/download/geosite.dat",
        },
    },
    "singbox": {
        "name": "SagerNet (sing-box ruleset)",
        "files": {
            "geoip.db": "https://github.com/SagerNet/sing-geoip/releases/latest/download/geoip.db",
            "geosite.db": "https://github.com/SagerNet/sing-geosite/releases/latest/download/geosite.db",
        },
    },
    "mmdb": {
        "name": "GeoLite2 Country.mmdb",
        "files": {
            "country.mmdb": "https://github.com/Loyalsoldier/geoip/releases/latest/download/Country.mmdb",
        },
    },
    "srs": {
        "name": "sing-box 规则集 (.srs)",
        "files": {
            "geosite-cn.srs": "https://github.com/MetaCubeX/meta-rules-dat/releases/latest/download/sing-box/geosite-cn.srs",
            "geoip-cn.srs": "https://github.com/MetaCubeX/meta-rules-dat/releases/latest/download/sing-box/geoip-cn.srs",
            "geosite-geolocation-!cn.srs": "https://github.com/MetaCubeX/meta-rules-dat/releases/latest/download/sing-box/geosite-geolocation-!cn.srs",
        },
    },
    "mihomo_yaml": {
        "name": "mihomo 规则集 (YAML)",
        "files": {
            "geosite-cn.yaml": "https://github.com/MetaCubeX/meta-rules-dat/releases/latest/download/clash/geosite-cn.yaml",
            "geoip-cn.yaml": "https://github.com/MetaCubeX/meta-rules-dat/releases/latest/download/clash/geoip-cn.yaml",
        },
    },
}

DNS_PRESETS: dict[str, str] = {
    "cloudflare": "https://cloudflare-dns.com/dns-query",
    "google": "https://dns.google/dns-query",
    "quad9": "https://dns.quad9.net/dns-query",
    "alidns": "https://dns.alidns.com/dns-query",
    "opendns": "https://doh.opendns.com/dns-query",
    "114dns": "https://doh.114dns.com/dns-query",
    "dnspod": "https://doh.pub/dns-query",
    "tencent": "https://dns.tencent.com/dns-query",
    "baidu": "https://doh.baidu.com/dns-query",
}

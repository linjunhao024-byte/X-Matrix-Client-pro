"""
X-Matrix Client — 辅助函数：原子写入、版本解析、深度合并、URL 校验、配置加载等
"""
from __future__ import annotations

import functools
import json
import logging
import os
import socket
import urllib.parse
from typing import Any, Callable

from xmatrix.constants import DATA_DIR, API_PORT, CLASH_API_PORT


# ── 原子文件写入 ─────────────────────────────────────────────────────

def atomic_write_json(path: str, data: Any) -> None:
    """原子写入 JSON 文件：先写临时文件，再 os.replace 替换，防止进程崩溃导致数据损坏。"""
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False, indent=2)
    os.replace(tmp, path)


# ── 装饰器 ───────────────────────────────────────────────────────────

def parse_version(tag: str) -> tuple[int, int, int]:
    """解析语义化版本号 (vX.Y.Z / X.Y.Z) 为三段元组，用于版本比较。"""
    clean = tag.lstrip("vV").split("-")[0].split("+")[0]
    parts = clean.split(".")
    try:
        return (int(parts[0]), int(parts[1]) if len(parts) > 1 else 0, int(parts[2]) if len(parts) > 2 else 0)
    except (ValueError, IndexError):
        return (0, 0, 0)


def load_port_config() -> dict:
    """从 data/config.json 加载端口配置，返回含默认值的端口字典。"""
    defaults = {
        "api_port": API_PORT,
        "clash_api_port": CLASH_API_PORT,
        "tun_address": "10.0.0.2/30",
        "tun_ipv6": False,
        "tun_route_address": "",
        "tun_endpoint_independent_nat": False,
        "tun_sniff": True,
        "tun_sniff_override": {},
        "local_port": 2077,
        "pac_port": 2078,
        "speedtest_base_port": 20801,
    }
    cfg_path = os.path.join(DATA_DIR, "config.json")
    try:
        if os.path.isfile(cfg_path):
            with open(cfg_path, "r", encoding="utf-8") as f:
                saved = json.load(f)
            ports = saved.get("ports", {})
            for k in defaults:
                if k in ports:
                    defaults[k] = ports[k]
    except Exception as e:
        logging.warning(f"[配置] 端口配置加载失败，使用默认值: {e}")
    return defaults


def check_port_available(port: int) -> bool:
    """检查端口是否可用（未被占用）。"""
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
            s.settimeout(0.5)
            return s.connect_ex(("127.0.0.1", port)) != 0
    except Exception:
        return False


def load_node_defaults() -> dict:
    """从 data/config.json 加载全局节点默认值（指纹、UA），返回含默认值的字典。"""
    defaults = {
        "def_fingerprint": "",
        "def_user_agent": "",
    }
    cfg_path = os.path.join(DATA_DIR, "config.json")
    try:
        if os.path.isfile(cfg_path):
            with open(cfg_path, "r", encoding="utf-8") as f:
                saved = json.load(f)
            node_defs = saved.get("node_defaults", {})
            for k in defaults:
                if k in node_defs:
                    defaults[k] = node_defs[k]
    except Exception as e:
        logging.warning(f"[配置] 节点默认值加载失败，使用默认值: {e}")
    return defaults


def load_dns_config() -> dict:
    """从 data/config.json 加载 DNS 配置，返回含默认值的字典。"""
    defaults = {
        "bootstrap_dns": "8.8.8.8",
        "hosts": {
            "dns.google": ["8.8.8.8", "8.8.4.4"],
            "dns.cloudflare.com": ["1.1.1.1", "1.0.0.1"],
        },
        "fakeip_range": "198.18.0.0/15",
        "fakeip_pool_size": 65535,
        "fakeip_exclude": [],
        "local_dns_default": "223.5.5.5",
        "expected_ips": ["geoip:cn"],
        "strategy4proxy": "IPIfNonMatch",
        "strategy4freedom": "AsIs",
        "fallback_filter": {
            "geoip": True,
            "geoip_code": "CN",
            "ipcidr": ["240.0.0.0/4"],
            "domain": ["+.google.com", "+.facebook.com", "+.youtube.com"],
        },
    }
    cfg_path = os.path.join(DATA_DIR, "config.json")
    try:
        if os.path.isfile(cfg_path):
            with open(cfg_path, "r", encoding="utf-8") as f:
                saved = json.load(f)
            dns = saved.get("dns", {})
            for k in defaults:
                if k in dns:
                    defaults[k] = dns[k]
    except Exception as e:
        logging.warning(f"[配置] DNS 配置加载失败，使用默认值: {e}")
    return defaults


def deep_merge(base: dict, overlay: dict) -> dict:
    """深度合并两个 dict。overlay 中的键覆盖 base，列表直接替换，返回新 dict。"""
    result = base.copy()
    for key, value in overlay.items():
        if key in result and isinstance(result[key], dict) and isinstance(value, dict):
            result[key] = deep_merge(result[key], value)
        else:
            result[key] = value
    return result


def validate_custom_config(raw: str) -> dict | None:
    """校验并解析 custom_config JSON。返回解析后的 dict 或 None（无效）。"""
    try:
        ob = json.loads(raw)
    except (json.JSONDecodeError, TypeError):
        return None
    if not isinstance(ob, dict):
        return None
    # 必须包含 type 字段且在已知协议列表中
    ob_type = ob.get("type", "")
    valid_types = {"vmess", "vless", "trojan", "shadowsocks", "socks", "http",
                   "hysteria2", "tuic", "wireguard", "anytls", "naive",
                   "direct", "block", "freedom", "blackhole", "dns"}
    if ob_type not in valid_types:
        return None
    return ob


def read_system_hosts() -> dict[str, list[str]]:
    """读取系统 hosts 文件，解析为 domain → [IP, ...] 字典。
    Windows: C:\\Windows\\System32\\drivers\\etc\\hosts
    Linux/macOS: /etc/hosts
    合并优先级：用户自定义 hosts > 系统 hosts（调用方负责合并顺序）。
    """
    hosts_path = r"C:\Windows\System32\drivers\etc\hosts" if os.name == "nt" else "/etc/hosts"
    result: dict[str, list[str]] = {}
    try:
        if not os.path.isfile(hosts_path):
            return result
        with open(hosts_path, "r", encoding="utf-8-sig", errors="ignore") as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith("#"):
                    continue
                # 跳过非 IP 开头的行（注释残留或格式异常）
                parts = line.split()
                if len(parts) < 2:
                    continue
                ip = parts[0]
                # 校验 IP 格式基本合法性（必须以数字或字母开头，排除注释行）
                if not ip or not (ip[0].isdigit() or ip[0].isalpha()):
                    continue
                for domain in parts[1:]:
                    domain = domain.lower()
                    if domain == "localhost" or domain.startswith("#"):
                        continue
                    if domain not in result:
                        result[domain] = []
                    if ip not in result[domain]:
                        result[domain].append(ip)
    except Exception as e:
        logging.warning(f"[hosts] 读取系统 hosts 失败: {e}")
    return result


def validate_url(url: str) -> str | None:
    """校验 URL 是否安全（非内网地址）。返回错误消息或 None（安全）。"""
    try:
        parsed = urllib.parse.urlparse(url.strip())
    except Exception:
        return "URL 格式无效"
    if parsed.scheme not in ("http", "https"):
        return f"不支持的协议: {parsed.scheme}，仅允许 http/https"
    hostname = parsed.hostname or ""
    if not hostname:
        return "URL 缺少主机名"
    # 拒绝内网地址
    import ipaddress
    try:
        ip = ipaddress.ip_address(hostname)
        if ip.is_private or ip.is_loopback or ip.is_link_local or ip.is_reserved:
            return f"拒绝访问内网地址: {hostname}"
    except ValueError:
        # hostname 是域名，检查常见内网域名
        lower = hostname.lower()
        if lower in ("localhost",) or lower.endswith(".local"):
            return f"拒绝访问内网域名: {hostname}"
    return None


def api_response(fn: Callable) -> Callable:
    """统一捕获 API 方法中的未处理异常，返回标准错误字典。"""
    @functools.wraps(fn)
    def wrapper(*args: Any, **kwargs: Any) -> Any:
        try:
            return fn(*args, **kwargs)
        except Exception as exc:
            return {"success": False, "error": str(exc)}
    return wrapper

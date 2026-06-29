"""
X-Matrix Client — Xray 配置生成
注意：这是一个简化版本，完整实现需要从 main_original.py 中提取
"""
from __future__ import annotations

import json
import os
from typing import Any, TYPE_CHECKING

from xmatrix.constants import DATA_DIR, CORE_REGISTRY, CONFIG_FILE
from xmatrix.helpers import load_port_config, load_dns_config, load_node_defaults, read_system_hosts

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def build_config(api: XMatrixAPI, **kwargs) -> dict:
    """生成 Xray 配置。这是一个简化版本，完整实现请参考 main_original.py。"""
    # 基础配置
    config = {
        "log": {"loglevel": kwargs.get("log_level", "warning")},
        "stats": {},
        "api": {"tag": "api", "services": ["StatsService"]},
        "policy": {"system": {
            "statsInboundUplink": True, "statsInboundDownlink": True,
            "statsOutboundUplink": True, "statsOutboundDownlink": True,
        }},
        "inbounds": [],
        "outbounds": [],
        "routing": {"domainStrategy": "AsIs", "rules": []},
    }

    # 入站
    local_port = kwargs.get("local_port", 2077)
    config["inbounds"].append({
        "tag": "proxy-in",
        "port": local_port,
        "listen": "127.0.0.1",
        "protocol": "mixed",
        "sniffing": {"enabled": kwargs.get("sniffing", True), "destOverride": kwargs.get("sniff_types", ["http", "tls"])},
        "settings": {"udp": kwargs.get("enable_udp", True), "auth": "noauth"},
    })

    # API 入站
    api_port = load_port_config()["api_port"]
    config["inbounds"].insert(0, {
        "listen": "127.0.0.1", "port": api_port,
        "protocol": "dokodemo-door",
        "settings": {"address": "127.0.0.1"},
        "tag": "api",
    })

    # 出站
    if api.tunnels and api.active_index >= 0:
        idx = min(api.active_index, len(api.tunnels) - 1)
        t = api.tunnels[idx]
        if t.get("protocol") != "policy_group":
            outbound = build_outbound(t)
            config["outbounds"].append(outbound)

    config["outbounds"].append({"tag": "direct", "protocol": "freedom", "settings": {}})
    config["outbounds"].append({"tag": "block", "protocol": "blackhole", "settings": {}})

    # 路由规则
    from xmatrix.routing.engine import build_routing_rules
    routing_rules = kwargs.get("routing_rules")
    proxy_mode = kwargs.get("proxy_mode", "rule")
    proxy_outbound_tag = config["outbounds"][0].get("tag", "direct") if config["outbounds"] else "direct"
    config["routing"]["rules"] = build_routing_rules(routing_rules, proxy_outbound_tag, proxy_mode)

    return config


def build_outbound(t: dict, **kwargs) -> dict:
    """构建 Xray outbound。"""
    protocol = t.get("protocol", "vless")
    addr = t.get("server_addr", "")
    port = _safe_port(t.get("server_port"), 443)
    tag = t.get("out_tag", "")

    node_defs = load_node_defaults()
    def_fp = node_defs.get("def_fingerprint", "")
    def_ua = node_defs.get("def_user_agent", "")

    # 协议 settings
    if protocol == "vless":
        settings = {"vnext": [{"address": addr, "port": port, "users": [
            {"id": t.get("uuid", ""), "encryption": t.get("vless_encryption", "none"), "flow": t.get("flow", "xtls-rprx-vision")}
        ]}]}
    elif protocol == "vmess":
        settings = {"vnext": [{"address": addr, "port": port, "users": [
            {"id": t.get("uuid", ""), "alterId": int(t.get("alter_id", 0)), "security": t.get("vmess_security", "auto")}
        ]}]}
    elif protocol == "trojan":
        settings = {"servers": [{"address": addr, "port": port, "password": t.get("password", "")}]}
    elif protocol == "shadowsocks":
        ss_server = {"address": addr, "port": port, "password": t.get("password", ""),
                     "method": t.get("method", "aes-256-gcm")}
        if t.get("ss_plugin"):
            ss_server["plugin"] = t["ss_plugin"]
            if t.get("ss_plugin_opts"):
                ss_server["pluginOpts"] = t["ss_plugin_opts"]
        settings = {"servers": [ss_server]}
    elif protocol == "socks":
        settings = {"servers": [{"address": addr, "port": port,
                                 "users": [{"user": t.get("socks_user", ""), "pass": t.get("socks_pass", "")}]}]}
    elif protocol == "http":
        settings = {"servers": [{"address": addr, "port": port,
                                 "users": [{"user": t.get("http_user", ""), "pass": t.get("http_pass", "")}]}]}
    else:
        settings = {"vnext": [{"address": addr, "port": port, "users": [
            {"id": t.get("uuid", ""), "encryption": "none"}
        ]}]}

    # 传输层
    net = t.get("network", "tcp")
    stream: dict = {"network": net}

    if net == "ws":
        ws_settings = {"path": t.get("ws_path", ""), "headers": {"Host": t.get("ws_host", "")}}
        if t.get("default_ua") or def_ua:
            ws_settings["userAgent"] = t.get("default_ua") or def_ua
        stream["wsSettings"] = ws_settings
    elif net == "grpc":
        grpc_settings = {"serviceName": t.get("grpc_service_name", "")}
        if t.get("grpc_authority"):
            grpc_settings["authority"] = t["grpc_authority"]
        stream["grpcSettings"] = grpc_settings

    # 安全层
    sec = t.get("security", "none")
    if sec == "reality":
        stream["security"] = "reality"
        reality_settings = {
            "serverName": t.get("sni", ""),
            "fingerprint": t.get("fingerprint") or def_fp or "chrome",
            "publicKey": t.get("public_key", ""),
            "shortId": t.get("short_id", ""),
            "spiderX": t.get("spider_x", ""),
        }
        stream["realitySettings"] = reality_settings
    elif sec == "tls":
        tls_settings = {
            "serverName": t.get("sni", ""),
            "fingerprint": t.get("fingerprint") or def_fp or "chrome",
        }
        if t.get("allow_insecure"):
            tls_settings["allowInsecure"] = True
        if t.get("alpn"):
            tls_settings["alpn"] = [x.strip() for x in t.get("alpn", "").split(",") if x.strip()]
        stream["security"] = "tls"
        stream["tlsSettings"] = tls_settings
    else:
        stream["security"] = "none"

    outbound = {"tag": tag, "protocol": protocol, "settings": settings, "streamSettings": stream}
    return outbound


def _safe_port(val: Any, default: int = 443) -> int:
    """安全端口转换。"""
    try:
        p = int(val)
        return p if 1 <= p <= 65535 else default
    except (ValueError, TypeError):
        return default

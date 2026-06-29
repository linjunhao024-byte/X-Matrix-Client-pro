"""
X-Matrix Client — sing-box 配置生成
注意：这是一个简化版本，完整实现需要从 main_original.py 中提取
"""
from __future__ import annotations

import json
import os
from typing import Any, TYPE_CHECKING

from xmatrix.constants import DATA_DIR
from xmatrix.helpers import load_port_config, load_dns_config

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def build_config_singbox(api: XMatrixAPI, **kwargs) -> dict:
    """生成 sing-box 配置。这是一个简化版本。"""
    config = {
        "log": {"level": kwargs.get("log_level", "warn"), "timestamp": True},
        "inbounds": [],
        "outbounds": [],
        "route": {"rules": [], "final": "proxy"},
    }

    # 入站
    local_port = kwargs.get("local_port", 2077)
    config["inbounds"].append({
        "type": "mixed",
        "tag": "proxy-in",
        "listen": "127.0.0.1",
        "listen_port": local_port,
        "sniff": kwargs.get("sniffing", True),
    })

    # 出站
    if api.tunnels and api.active_index >= 0:
        idx = min(api.active_index, len(api.tunnels) - 1)
        t = api.tunnels[idx]
        if t.get("protocol") != "policy_group":
            outbound = build_outbound_singbox(t)
            if outbound:
                config["outbounds"].append(outbound)

    config["outbounds"].append({"type": "direct", "tag": "direct"})
    config["outbounds"].append({"type": "block", "tag": "block"})

    return config


def build_outbound_singbox(t: dict, **kwargs) -> dict | None:
    """构建 sing-box outbound。"""
    protocol = t.get("protocol", "")
    addr = t.get("server_addr", "")
    port = _safe_port(t.get("server_port"), 443)
    tag = t.get("out_tag", "")

    ob: dict = {"type": protocol, "tag": tag}

    if protocol == "vless":
        ob["server"] = addr
        ob["server_port"] = port
        ob["uuid"] = t.get("uuid", "")
        if t.get("flow"):
            ob["flow"] = t["flow"]
    elif protocol == "vmess":
        ob["server"] = addr
        ob["server_port"] = port
        ob["uuid"] = t.get("uuid", "")
        ob["alter_id"] = int(t.get("alter_id", 0))
        ob["security"] = t.get("vmess_security", "auto")
    elif protocol == "trojan":
        ob["server"] = addr
        ob["server_port"] = port
        ob["password"] = t.get("password", "")
    elif protocol == "shadowsocks":
        ob["server"] = addr
        ob["server_port"] = port
        ob["password"] = t.get("password", "")
        ob["method"] = t.get("method", "aes-256-gcm")
    elif protocol in ("hysteria2", "hy2"):
        ob["type"] = "hysteria2"
        ob["server"] = addr
        ob["server_port"] = port
        ob["password"] = t.get("password", "")
    elif protocol == "tuic":
        ob["server"] = addr
        ob["server_port"] = port
        ob["uuid"] = t.get("uuid", "")
        ob["password"] = t.get("password", "")
        ob["congestion_control"] = t.get("tuic_congestion", "bbr")
    elif protocol == "wireguard":
        ob["server"] = addr
        ob["server_port"] = port
        ob["local_address"] = [a.strip() for a in t.get("wg_address", "10.0.0.2/32").split(",") if a.strip()]
        ob["private_key"] = t.get("wg_secret_key", "")
        ob["peer_public_key"] = t.get("wg_public_key", "")
        ob["mtu"] = int(t.get("wg_mtu", 1420))
    elif protocol == "socks":
        ob["type"] = "socks"
        ob["server"] = addr
        ob["server_port"] = port
        ob["version"] = "5"
        if t.get("socks_user"):
            ob["username"] = t["socks_user"]
        if t.get("socks_pass"):
            ob["password"] = t["socks_pass"]
    elif protocol == "http":
        ob["server"] = addr
        ob["server_port"] = port
        if t.get("http_user"):
            ob["username"] = t["http_user"]
        if t.get("http_pass"):
            ob["password"] = t["http_pass"]
    else:
        return None

    # 传输层
    net = t.get("network", "tcp")
    if net == "ws":
        ob["transport"] = {"type": "ws", "path": t.get("ws_path", "")}
        if t.get("ws_host"):
            ob["transport"]["headers"] = {"Host": t["ws_host"]}
    elif net == "grpc":
        ob["transport"] = {"type": "grpc", "service_name": t.get("grpc_service_name", "")}

    # TLS
    sec = t.get("security", "none")
    if sec in ("tls", "reality"):
        tls_obj: dict = {"enabled": True}
        if t.get("sni"):
            tls_obj["server_name"] = t["sni"]
        if t.get("fingerprint"):
            tls_obj["utls"] = {"enabled": True, "fingerprint": t["fingerprint"]}
        if t.get("alpn"):
            tls_obj["alpn"] = [x.strip() for x in t["alpn"].split(",") if x.strip()]
        if t.get("allow_insecure"):
            tls_obj["insecure"] = True
        if sec == "reality":
            tls_obj["reality"] = {"enabled": True, "public_key": t.get("public_key", ""), "short_id": t.get("short_id", "")}
        ob["tls"] = tls_obj

    return ob


def _safe_port(val: Any, default: int = 443) -> int:
    """安全端口转换。"""
    try:
        p = int(val)
        return p if 1 <= p <= 65535 else default
    except (ValueError, TypeError):
        return default

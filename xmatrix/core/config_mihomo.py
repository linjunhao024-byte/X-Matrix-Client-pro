"""
X-Matrix Client — mihomo 配置生成
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


def build_config_mihomo(api: XMatrixAPI, **kwargs) -> dict:
    """生成 mihomo 配置。这是一个简化版本。"""
    local_port = kwargs.get("local_port", 2077)

    config = {
        "mixed-port": local_port,
        "allow-lan": kwargs.get("allow_lan", False),
        "mode": "rule",
        "log-level": kwargs.get("log_level", "warn"),
        "external-controller": f"127.0.0.1:{load_port_config()['clash_api_port']}",
        "proxies": [],
        "proxy-groups": [],
        "rules": [],
    }

    # 收集节点
    proxies = []
    proxy_names = []

    if api.tunnels and api.active_index >= 0:
        idx = min(api.active_index, len(api.tunnels) - 1)
        t = api.tunnels[idx]
        if t.get("protocol") == "policy_group":
            id_map = {nd.get("id"): nd for nd in api.tunnels}
            for cid in t.get("child_ids", []):
                child = id_map.get(cid)
                if child and child.get("protocol") != "policy_group":
                    p = clash_node(child)
                    if p:
                        proxies.append(p)
                        proxy_names.append(p["name"])
            strategy = t.get("group_strategy", "leastPing")
            clash_type = {"leastPing": "url-test", "leastLoad": "fallback", "random": "random", "roundRobin": "round-robin", "fallback": "fallback"}.get(strategy, "url-test")
            config["proxy-groups"] = [{"name": "PROXY", "type": clash_type, "proxies": proxy_names}]
        else:
            p = clash_node(t)
            if p:
                proxies.append(p)
                proxy_names.append(p["name"])
            config["proxy-groups"] = [{"name": "PROXY", "type": "select", "proxies": proxy_names or ["DIRECT"]}]
    else:
        config["proxy-groups"] = [{"name": "PROXY", "type": "select", "proxies": ["DIRECT"]}]

    config["proxies"] = proxies

    # 路由规则
    rules = []
    routing_rules = kwargs.get("routing_rules")
    if routing_rules:
        for r in routing_rules:
            if not r.get("enabled", True):
                continue
            outbound = r.get("outbound", "DIRECT")
            if outbound == "proxy":
                outbound = "PROXY"
            elif outbound == "direct":
                outbound = "DIRECT"
            content = r.get("content", "").strip()
            if not content:
                continue
            rt = r.get("type", "domain")
            for c in content.split(","):
                c = c.strip()
                if not c:
                    continue
                if rt == "domain":
                    rules.append(f"DOMAIN-SUFFIX,{c},{outbound}")
                elif rt == "ip":
                    rules.append(f"IP-CIDR,{c},{outbound}")
                elif rt == "geosite":
                    rules.append(f"DOMAIN-SUFFIX,{c},{outbound}")
                elif rt == "geoip":
                    rules.append(f"IP-CIDR,{c},{outbound}")

    rules.append("MATCH,PROXY")
    config["rules"] = rules

    # DNS
    if kwargs.get("enable_custom_dns"):
        dns_conf = {
            "enable": True,
            "enhanced-mode": "fake-ip" if kwargs.get("enable_fake_dns") else "redir-host",
            "nameserver": [kwargs.get("local_dns", "223.5.5.5")],
        }
        if kwargs.get("enable_fake_dns"):
            dns_cfg = load_dns_config()
            dns_conf["fake-ip-range"] = dns_cfg["fakeip_range"]
        config["dns"] = dns_conf

    return config


def clash_node(t: dict) -> dict | None:
    """将节点转为 mihomo 格式。"""
    proto = t.get("protocol", "")
    addr = t.get("server_addr", "")
    port = _safe_port(t.get("server_port"), 443)
    tag = t.get("out_tag", "")

    p: dict = {"name": tag, "type": proto, "server": addr, "port": port}

    if proto == "vless":
        p["uuid"] = t.get("uuid", "")
        p["cipher"] = "auto"
        if t.get("flow"):
            p["flow"] = t["flow"]
    elif proto == "vmess":
        p["uuid"] = t.get("uuid", "")
        p["alterId"] = int(t.get("alter_id", 0))
        p["cipher"] = t.get("vmess_security", "auto")
    elif proto == "trojan":
        p["password"] = t.get("password", "")
    elif proto == "shadowsocks":
        p["password"] = t.get("password", "")
        p["cipher"] = t.get("method", "aes-256-gcm")
    elif proto in ("hysteria2", "hy2"):
        p["type"] = "hysteria2"
        p["password"] = t.get("password", "")
    elif proto == "tuic":
        p["uuid"] = t.get("uuid", "")
        p["password"] = t.get("password", "")
        p["congestion-control"] = t.get("tuic_congestion", "bbr")
    elif proto == "wireguard":
        p["private-key"] = t.get("wg_secret_key", "")
        p["public-key"] = t.get("wg_public_key", "")
        p["ip"] = t.get("wg_address", "10.0.0.2/32")
        p["mtu"] = int(t.get("wg_mtu", 1420))
    else:
        return None

    # 传输层
    net = t.get("network", "tcp")
    if net == "ws":
        p["network"] = "ws"
        p["ws-opts"] = {"path": t.get("ws_path", "")}
        if t.get("ws_host"):
            p["ws-opts"]["headers"] = {"Host": t["ws_host"]}
    elif net == "grpc":
        p["network"] = "grpc"
        p["grpc-opts"] = {"grpc-service-name": t.get("grpc_service_name", "")}

    # TLS
    sec = t.get("security", "none")
    if sec == "tls":
        p["tls"] = True
        if t.get("sni"):
            p["sni"] = t["sni"]
        if t.get("fingerprint"):
            p["client-fingerprint"] = t["fingerprint"]
        if t.get("alpn"):
            p["alpn"] = [a.strip() for a in t["alpn"].split(",") if a.strip()]
        if t.get("allow_insecure"):
            p["skip-cert-verify"] = True
    elif sec == "reality":
        p["tls"] = True
        p["reality-opts"] = {"public-key": t.get("public_key", ""), "short-id": t.get("short_id", "")}
        if t.get("sni"):
            p["sni"] = t["sni"]
        if t.get("fingerprint"):
            p["client-fingerprint"] = t["fingerprint"]

    return p


def _safe_port(val: Any, default: int = 443) -> int:
    """安全端口转换。"""
    try:
        p = int(val)
        return p if 1 <= p <= 65535 else default
    except (ValueError, TypeError):
        return default

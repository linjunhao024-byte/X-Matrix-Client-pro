"""
X-Matrix Client — 路由引擎
"""
from __future__ import annotations

from typing import Any, TYPE_CHECKING

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def build_routing_rules(routing_rules: list[dict] | None, proxy_outbound_tag: str = "direct", proxy_mode: str = "rule") -> list[dict]:
    """组装并翻译前端传入的路由规则为 Xray 标准格式。"""
    is_balancer = proxy_outbound_tag.endswith("-balancer")

    def _proxy_rule(**extra: Any) -> dict:
        rule: dict = {"type": "field", **extra}
        if is_balancer:
            rule["balancerTag"] = proxy_outbound_tag
        else:
            rule["outboundTag"] = proxy_outbound_tag
        return rule

    rules: list[dict] = [
        {"type": "field", "inboundTag": ["api"], "outboundTag": "api"},
        {"type": "field", "outboundTag": "block", "port": "443", "network": "udp"},
    ]

    if proxy_mode == "global":
        rules.append(_proxy_rule(inboundTag=["proxy-in", "lan-in", "tun-inbound"]))
        return rules
    elif proxy_mode == "direct":
        rules.append({"type": "field", "inboundTag": ["proxy-in", "lan-in", "tun-inbound"], "outboundTag": "direct"})
        return rules
    elif proxy_mode == "route_only":
        pass

    if routing_rules:
        for r in routing_rules:
            outbound_tag = r.get("outbound", "direct")
            if outbound_tag == "proxy":
                content_list = [c.strip() for c in r.get("content", "").split(",") if c.strip()]
                rule = _proxy_rule()
            else:
                rule = {"type": "field", "outboundTag": outbound_tag}
                content_list = [c.strip() for c in r.get("content", "").split(",") if c.strip()]

            has_negation = any(c.startswith("!") for c in content_list)

            if r.get("type") == "domain":
                if has_negation:
                    rule["domain"] = [f"geosite:{c}" if c.startswith("!") and not c.startswith("geosite:") else c for c in content_list]
                else:
                    rule["domain"] = content_list
            elif r.get("type") == "ip":
                if has_negation:
                    rule["ip"] = [f"geoip:{c}" if c.startswith("!") and not c.startswith("geoip:") else c for c in content_list]
                else:
                    rule["ip"] = content_list
            elif r.get("type") == "port":
                rule["port"] = ",".join(content_list)
            elif r.get("type") == "network":
                rule["network"] = ",".join(content_list)
            elif r.get("type") == "process":
                rule["processName"] = content_list
            elif r.get("type") == "geosite":
                rule["domain"] = [f"geosite:{c.strip()}" for c in content_list]
            elif r.get("type") == "geoip":
                rule["ip"] = [f"geoip:{c.strip()}" for c in content_list]
            elif r.get("type") == "protocol":
                rule["protocol"] = content_list
            elif r.get("type") == "source":
                rule["source"] = content_list
            elif r.get("type") == "sourcePort":
                rule["sourcePort"] = ",".join(content_list)
            elif r.get("type") == "domain_regex":
                rule["domain"] = [f"regexp:{c.strip()}" for c in content_list]
            elif r.get("type") == "inboundTag":
                rule["inboundTag"] = content_list

            rules.append(rule)

    rules.append(_proxy_rule(inboundTag=["proxy-in", "lan-in", "tun-inbound"]))
    return rules


def get_routing_topology(routing_rules: list[dict] | None = None, local_port: int = 2077) -> dict:
    """获取路由拓扑。"""
    nodes = [
        {"id": "inbound", "type": "in", "label": f"入站 ({local_port})", "x": 100, "y": 240},
        {"id": "router", "type": "router", "label": "路由引擎", "x": 320, "y": 240},
        {"id": "proxy", "type": "out", "label": "🚀 代理 (Proxy)", "x": 550, "y": 120, "color": "blue"},
        {"id": "direct", "type": "out", "label": "🌐 直连 (Direct)", "x": 550, "y": 240, "color": "emerald"},
        {"id": "block", "type": "out", "label": "⛔ 拦截 (Block)", "x": 550, "y": 360, "color": "rose"}
    ]

    edges_map = {
        "proxy": {"from": "router", "to": "proxy", "rules": []},
        "direct": {"from": "router", "to": "direct", "rules": []},
        "block": {"from": "router", "to": "block", "rules": []}
    }

    rules_count = 0
    if routing_rules:
        for r in routing_rules:
            if not r.get("enabled", False):
                continue
            outbound = r.get("outbound", "proxy")
            if outbound in edges_map:
                edges_map[outbound]["rules"].append(r.get("content", ""))
                rules_count += 1

    edges = [{"from": "inbound", "to": "router", "rules": ["所有本地流量"]}]

    for k, v in edges_map.items():
        if v["rules"] or k == "proxy":
            if not v["rules"] and k == "proxy":
                v["rules"] = ["默认兜底流量"]
            edges.append(v)

    return {"success": True, "nodes": nodes, "edges": edges, "rules_count": rules_count}

"""
X-Matrix Client — 导入导出功能
"""
from __future__ import annotations

import base64
import json
import os
import re
import time
import urllib.parse
import urllib.request
from typing import TYPE_CHECKING

import yaml

from xmatrix.constants import DATA_DIR, TUNNELS_FILE
from xmatrix.helpers import validate_url
from xmatrix.nodes.parser import parse_vmess_uri, parse_vless_uri, parse_trojan_uri, parse_ss_uri, parse_hy2_uri

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def import_config(api: XMatrixAPI, file_path: str = "") -> dict:
    """导入配置文件。"""
    import webview
    if not file_path:
        file_path = api._pick_file(webview.OPEN_DIALOG, allow_multiple=False,
                                    file_types=("配置文件 (*.json;*.yaml;*.yml)",))
    if not file_path:
        return {"success": False, "error": "用户取消选择"}

    try:
        with open(file_path, "r", encoding="utf-8") as f:
            content = f.read()
    except Exception as e:
        return {"success": False, "error": f"读取文件失败: {e}"}

    # 尝试 JSON
    try:
        data = json.loads(content)
        if isinstance(data, dict) and "outbounds" in data:
            return _import_xray_config(api, data)
        if isinstance(data, list):
            return _import_uri_list(api, data)
    except json.JSONDecodeError:
        pass

    # 尝试 YAML
    try:
        data = yaml.safe_load(content)
        if isinstance(data, dict) and "proxies" in data:
            from xmatrix.nodes.parser import parse_clash_proxies
            results = parse_clash_proxies(data["proxies"], api._next_tag)
            if results:
                with api._tunnels_lock:
                    api.tunnels.extend(results)
                api._save_tunnels()
                return {"success": True, "tunnels": api.tunnels, "count": len(results)}
    except Exception:
        pass

    # 尝试 URI 列表
    if "://" in content:
        return import_uri(api, content)

    return {"success": False, "error": "无法识别的配置格式"}


def import_uri(api: XMatrixAPI, text: str) -> dict:
    """导入 URI。"""
    lines = text.strip().split("\n")
    results = []
    for line in lines:
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        try:
            if line.startswith("vmess://"):
                entry = parse_vmess_uri(line)
            elif line.startswith("vless://"):
                entry = parse_vless_uri(line)
            elif line.startswith("trojan://"):
                entry = parse_trojan_uri(line)
            elif line.startswith("ss://"):
                entry = parse_ss_uri(line)
            elif line.startswith(("hy2://", "hysteria2://")):
                entry = parse_hy2_uri(line)
            else:
                continue
            if entry:
                if not entry.get("in_tag"):
                    entry["in_tag"] = api._next_tag("in")
                if not entry.get("out_tag"):
                    entry["out_tag"] = f"{entry.get('protocol', 'unknown')} Node"
                entry["id"] = f"{int(time.time() * 1000)}-{len(results)}"
                results.append(entry)
        except Exception as e:
            continue

    if not results:
        return {"success": False, "error": "未识别到有效的节点链接"}

    with api._tunnels_lock:
        api.tunnels.extend(results)
    api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels, "count": len(results)}


def import_subscription(api: XMatrixAPI, url: str) -> dict:
    """导入订阅。"""
    if not url or not url.strip():
        return {"success": False, "error": "订阅链接为空"}
    ssrf_err = validate_url(url)
    if ssrf_err:
        return {"success": False, "error": ssrf_err}

    try:
        req = urllib.request.Request(url.strip(), headers={
            "User-Agent": "ClashForAndroid/2.5.12",
            "Accept": "*/*",
        })
        with urllib.request.urlopen(req, timeout=15) as resp:
            raw_text = resp.read().decode("utf-8", errors="ignore")
    except Exception as e:
        return {"success": False, "error": f"拉取订阅失败: {str(e)}"}

    # Clash YAML
    try:
        data = yaml.safe_load(raw_text)
        if isinstance(data, dict) and "proxies" in data:
            from xmatrix.nodes.parser import parse_clash_proxies
            results = parse_clash_proxies(data["proxies"], api._next_tag)
            if results:
                with api._tunnels_lock:
                    api.tunnels.extend(results)
                api._save_tunnels()
                api.dedup_server_list(True)
                return {"success": True, "tunnels": api.tunnels, "count": len(results)}
    except Exception:
        pass

    # Base64
    try:
        cleaned = raw_text.strip().replace("\n", "").replace("\r", "")
        for decoder in [base64.b64decode, base64.urlsafe_b64decode]:
            try:
                decoded = decoder(cleaned).decode("utf-8", errors="ignore")
                if "://" in decoded:
                    result = import_uri(api, decoded)
                    if result.get("success"):
                        api.dedup_server_list(True)
                    return result
            except Exception:
                continue
    except Exception:
        pass

    # 纯文本 URI
    if "://" in raw_text:
        result = import_uri(api, raw_text)
        if result.get("success"):
            api.dedup_server_list(True)
        return result

    return {"success": False, "error": "无法识别订阅格式"}


def export_uri(api: XMatrixAPI, index: int) -> dict:
    """导出 URI。"""
    if not (0 <= index < len(api.tunnels)):
        return {"success": False, "error": "索引越界"}
    t = api.tunnels[index]
    proto = t.get("protocol", "")
    if proto == "policy_group":
        return {"success": False, "error": "负载均衡组不支持 URI 导出"}

    try:
        if proto == "vmess":
            data = {
                "v": "2", "ps": t.get("out_tag", ""),
                "add": t.get("server_addr", ""),
                "port": str(t.get("server_port", 443)),
                "id": t.get("uuid", ""), "aid": str(t.get("alter_id", "0")),
                "scy": t.get("vmess_security", "auto"), "net": t.get("network", "tcp"),
                "type": "none", "host": t.get("ws_host", ""),
                "path": t.get("ws_path", "/"), "tls": t.get("security", ""),
                "sni": t.get("sni", ""), "alpn": "", "fp": t.get("fingerprint", "")
            }
            b64 = base64.b64encode(json.dumps(data).encode('utf-8')).decode('utf-8')
            return {"success": True, "uri": f"vmess://{b64}"}

        elif proto == "vless":
            userinfo = urllib.parse.quote(t.get("uuid", ""))
            scheme = "vless"
        elif proto == "trojan":
            userinfo = urllib.parse.quote(t.get("password", ""))
            scheme = "trojan"
        elif proto == "shadowsocks":
            userinfo = base64.b64encode(f"{t.get('method', 'aes-256-gcm')}:{t.get('password', '')}".encode('utf-8')).decode('utf-8')
            scheme = "ss"
        elif proto in ("hysteria2", "hy2"):
            userinfo = urllib.parse.quote(t.get("password", ""))
            scheme = "hy2"
        else:
            return {"success": False, "error": "暂不支持导出该协议"}

        query = {"type": t.get("network", "tcp")}
        if t.get("security") and t.get("security") != "none":
            query["security"] = t.get("security")
        if t.get("sni"):
            query["sni"] = t.get("sni")
        if t.get("fingerprint"):
            query["fp"] = t.get("fingerprint")

        query_str = urllib.parse.urlencode(query, safe="")
        tag = urllib.parse.quote(t.get("out_tag", ""))
        uri = f"{scheme}://{userinfo}@{t.get('server_addr', '')}:{t.get('server_port', 443)}?{query_str}#{tag}"
        return {"success": True, "uri": uri}
    except Exception as e:
        return {"success": False, "error": f"导出失败: {str(e)}"}


def export_config(api: XMatrixAPI, json_str: str) -> dict:
    """导出配置。"""
    import webview
    file_path = api._pick_file(webview.SAVE_DIALOG, save_filename="config.json",
                                file_types=("JSON 文件 (*.json)",))
    if not file_path:
        return {"success": False, "error": "用户取消"}
    if not file_path.lower().endswith(".json"):
        file_path += ".json"
    with open(file_path, "w", encoding="utf-8") as f:
        f.write(json_str)
    return {"success": True, "path": file_path}


def export_xmatrix_uri(api: XMatrixAPI, index: int) -> dict:
    """导出 xmatrix:// URI。"""
    if not (0 <= index < len(api.tunnels)):
        return {"success": False, "error": "索引越界"}
    t = api.tunnels[index]
    proto = t.get("protocol", "")
    if proto == "policy_group":
        return {"success": False, "error": "负载均衡组不支持 URI 导出"}
    payload = json.dumps(t, ensure_ascii=False)
    b64 = base64.urlsafe_b64encode(payload.encode("utf-8")).decode("utf-8")
    return {"success": True, "uri": f"xmatrix://{proto}/{b64}"}


def import_routing_rules(api: XMatrixAPI) -> dict:
    """导入路由规则。"""
    import webview
    file_path = api._pick_file(webview.OPEN_DIALOG, allow_multiple=False,
                                file_types=("JSON 文件 (*.json)",))
    if not file_path:
        return {"success": False, "error": "用户取消选择"}
    with open(file_path, "r", encoding="utf-8") as f:
        data = json.load(f)
    rules = data if isinstance(data, list) else data.get("rules", [])
    if not isinstance(rules, list) or not rules:
        return {"success": False, "error": "未找到有效的路由规则列表"}
    valid_rules = []
    for r in rules:
        if isinstance(r, dict) and "type" in r and "content" in r and "outbound" in r:
            if "enabled" not in r:
                r["enabled"] = True
            if "id" not in r:
                r["id"] = int(time.time() * 1000) + len(valid_rules)
            valid_rules.append(r)
    if not valid_rules:
        return {"success": False, "error": "JSON 中没有符合格式的规则"}
    return {"success": True, "rules": valid_rules, "count": len(valid_rules)}


def import_routing_rules_from_url(url: str) -> dict:
    """从 URL 导入路由规则。"""
    if not url or not url.strip():
        return {"success": False, "error": "URL 不能为空"}
    ssrf_err = validate_url(url)
    if ssrf_err:
        return {"success": False, "error": ssrf_err}
    try:
        req = urllib.request.Request(url.strip(), headers={"User-Agent": "X-Matrix/1.0", "Accept": "application/json,*/*"})
        with urllib.request.urlopen(req, timeout=15) as resp:
            raw = resp.read().decode("utf-8", errors="ignore")
        data = json.loads(raw)
    except Exception as e:
        return {"success": False, "error": f"拉取远程规则失败: {e}"}
    rules = data if isinstance(data, list) else data.get("rules", [])
    if not isinstance(rules, list) or not rules:
        return {"success": False, "error": "远程规则集为空或格式无效"}
    valid_rules = []
    for r in rules:
        if isinstance(r, dict) and "type" in r and "content" in r and "outbound" in r:
            if "enabled" not in r:
                r["enabled"] = True
            if "id" not in r:
                r["id"] = int(time.time() * 1000) + len(valid_rules)
            r["remote_url"] = url
            valid_rules.append(r)
    return {"success": True, "rules": valid_rules, "count": len(valid_rules)}


def export_routing_rules(api: XMatrixAPI, rules_json: str) -> dict:
    """导出路由规则。"""
    import webview
    file_path = api._pick_file(webview.SAVE_DIALOG, save_filename="routing-template.json",
                                file_types=("JSON 文件 (*.json)",))
    if not file_path:
        return {"success": False, "error": "用户取消"}
    if not file_path.lower().endswith(".json"):
        file_path += ".json"
    try:
        rules = json.loads(rules_json) if isinstance(rules_json, str) else rules_json
    except (json.JSONDecodeError, TypeError):
        return {"success": False, "error": "规则数据格式错误"}
    export_data = {
        "name": "X-Matrix Routing Template",
        "version": 1,
        "rules": rules
    }
    with open(file_path, "w", encoding="utf-8") as f:
        json.dump(export_data, f, ensure_ascii=False, indent=2)
    return {"success": True, "path": file_path, "count": len(rules)}


def import_routing_template_from_url(url: str) -> dict:
    """从 URL 导入路由模板。"""
    if not url or not url.strip():
        return {"success": False, "error": "URL 为空"}
    ssrf_err = validate_url(url)
    if ssrf_err:
        return {"success": False, "error": ssrf_err}
    try:
        req = urllib.request.Request(url.strip(), headers={
            "User-Agent": "X-Matrix/1.0",
            "Accept": "application/json, text/plain, */*",
        })
        with urllib.request.urlopen(req, timeout=15) as resp:
            raw = resp.read().decode("utf-8", errors="ignore")
    except Exception as e:
        return {"success": False, "error": f"下载失败: {str(e)}"}
    try:
        data = json.loads(raw)
        rules = data if isinstance(data, list) else data.get("rules", [])
    except json.JSONDecodeError:
        rules = []
        lower_url = url.lower()
        if "reject" in lower_url or "block" in lower_url or "ads" in lower_url:
            outbound = "block"
        elif "direct" in lower_url or "direct-list" in lower_url:
            outbound = "direct"
        else:
            outbound = "proxy"
        for line in raw.splitlines():
            line = line.strip()
            if not line or line.startswith("#") or line.startswith("//"):
                continue
            if line.startswith("geosite:"):
                rules.append({"type": "geosite", "content": line, "outbound": outbound})
            elif line.startswith("geoip:"):
                rules.append({"type": "geoip", "content": line, "outbound": outbound})
            elif line.startswith("domain:") or line.startswith("domain-suffix:"):
                rules.append({"type": "domain", "content": line.split(":", 1)[1], "outbound": outbound})
            else:
                rules.append({"type": "domain", "content": line, "outbound": outbound})
    if not isinstance(rules, list) or not rules:
        return {"success": False, "error": "未找到有效的路由规则列表"}
    valid_rules = []
    for r in rules:
        if isinstance(r, dict) and "type" in r and "content" in r and "outbound" in r:
            if "enabled" not in r:
                r["enabled"] = True
            if "id" not in r:
                r["id"] = int(time.time() * 1000) + len(valid_rules)
            valid_rules.append(r)
    if not valid_rules:
        return {"success": False, "error": "JSON 中没有符合格式的规则"}
    return {"success": True, "rules": valid_rules, "count": len(valid_rules)}


def _import_xray_config(api: XMatrixAPI, data: dict) -> dict:
    """导入 Xray 配置。"""
    outbounds = data.get("outbounds", [])
    results = []
    for ob in outbounds:
        if ob.get("tag") in ("direct", "block", "api"):
            continue
        protocol = ob.get("protocol", "")
        if protocol not in ("vless", "vmess", "trojan", "shadowsocks", "socks", "http"):
            continue
        entry = {
            "protocol": protocol,
            "in_tag": api._next_tag("in"),
            "out_tag": ob.get("tag", f"{protocol} Node"),
            "server_addr": "",
            "server_port": 443,
            "uuid": "", "password": "", "method": "",
        }
        settings = ob.get("settings", {})
        if protocol in ("vless", "vmess"):
            vnext = settings.get("vnext", [{}])[0]
            entry["server_addr"] = vnext.get("address", "")
            entry["server_port"] = vnext.get("port", 443)
            users = vnext.get("users", [{}])[0]
            entry["uuid"] = users.get("id", "")
        elif protocol in ("trojan", "shadowsocks"):
            servers = settings.get("servers", [{}])[0]
            entry["server_addr"] = servers.get("address", "")
            entry["server_port"] = servers.get("port", 443)
            entry["password"] = servers.get("password", "")
            if protocol == "shadowsocks":
                entry["method"] = servers.get("method", "aes-256-gcm")
        entry["id"] = f"{int(time.time() * 1000)}-{len(results)}"
        results.append(entry)

    if results:
        with api._tunnels_lock:
            api.tunnels.extend(results)
        api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels, "count": len(results)}


def _import_uri_list(api: XMatrixAPI, data: list) -> dict:
    """导入 URI 列表。"""
    text = "\n".join(str(item) for item in data if isinstance(item, str))
    return import_uri(api, text)

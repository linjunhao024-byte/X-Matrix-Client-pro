"""
X-Matrix Client — URI 解析器
"""
from __future__ import annotations

import base64
import json
import logging
import urllib.parse
from typing import Callable


def parse_vmess_uri(uri: str) -> dict | None:
    """解析 vmess:// 链接。"""
    try:
        b64 = uri.replace("vmess://", "")
        padding = 4 - len(b64) % 4
        if padding != 4:
            b64 += "=" * padding
        data = json.loads(base64.b64decode(b64).decode("utf-8"))
        return {
            "protocol": "vmess",
            "server_addr": data.get("add", ""),
            "server_port": int(data.get("port", 443)),
            "uuid": data.get("id", ""),
            "alter_id": str(data.get("aid", "0")),
            "vmess_security": data.get("scy", "auto"),
            "out_tag": data.get("ps", "VMess Node"),
            "network": data.get("net", "tcp"),
            "ws_path": data.get("path", ""),
            "ws_host": data.get("host", ""),
            "security": data.get("tls", "none"),
            "sni": data.get("sni", ""),
            "fingerprint": data.get("fp", "chrome"),
        }
    except Exception as e:
        logging.warning(f"[解析] vmess URI 解析失败: {e}")
        return None


def parse_vless_uri(uri: str) -> dict | None:
    """解析 vless:// 链接。"""
    try:
        rest = uri.replace("vless://", "")
        userinfo, hostinfo = rest.split("@", 1)
        uuid = urllib.parse.unquote(userinfo)
        if "?" in hostinfo:
            hostport, query_str = hostinfo.split("?", 1)
        else:
            hostport, query_str = hostinfo, ""
        if "#" in hostport:
            hostport = hostport.split("#")[0]
        if ":" in hostport:
            addr, port = hostport.rsplit(":", 1)
            port = int(port)
        else:
            addr, port = hostport, 443
        query = dict(urllib.parse.parse_qsl(query_str))
        tag = ""
        if "#" in rest:
            tag = urllib.parse.unquote(rest.split("#")[1])
        return {
            "protocol": "vless",
            "server_addr": addr,
            "server_port": port,
            "uuid": uuid,
            "out_tag": tag or f"VLESS {addr}",
            "network": query.get("type", "tcp"),
            "security": query.get("security", "none"),
            "sni": query.get("sni", ""),
            "fingerprint": query.get("fp", "chrome"),
            "flow": query.get("flow", ""),
            "public_key": query.get("pbk", ""),
            "short_id": query.get("sid", ""),
            "ws_path": query.get("path", ""),
            "ws_host": query.get("host", ""),
            "grpc_service_name": query.get("serviceName", ""),
        }
    except Exception as e:
        logging.warning(f"[解析] vless URI 解析失败: {e}")
        return None


def parse_trojan_uri(uri: str) -> dict | None:
    """解析 trojan:// 链接。"""
    try:
        rest = uri.replace("trojan://", "")
        userinfo, hostinfo = rest.split("@", 1)
        password = urllib.parse.unquote(userinfo)
        if "?" in hostinfo:
            hostport, query_str = hostinfo.split("?", 1)
        else:
            hostport, query_str = hostinfo, ""
        if "#" in hostport:
            hostport = hostport.split("#")[0]
        if ":" in hostport:
            addr, port = hostport.rsplit(":", 1)
            port = int(port)
        else:
            addr, port = hostport, 443
        query = dict(urllib.parse.parse_qsl(query_str))
        tag = ""
        if "#" in rest:
            tag = urllib.parse.unquote(rest.split("#")[1])
        return {
            "protocol": "trojan",
            "server_addr": addr,
            "server_port": port,
            "password": password,
            "out_tag": tag or f"Trojan {addr}",
            "network": query.get("type", "tcp"),
            "security": query.get("security", "tls"),
            "sni": query.get("sni", ""),
            "fingerprint": query.get("fp", "chrome"),
        }
    except Exception as e:
        logging.warning(f"[解析] trojan URI 解析失败: {e}")
        return None


def parse_ss_uri(uri: str) -> dict | None:
    """解析 ss:// 链接。"""
    try:
        rest = uri.replace("ss://", "")
        if "@" in rest:
            userinfo, hostinfo = rest.split("@", 1)
            if "#" in hostinfo:
                hostinfo = hostinfo.split("#")[0]
            if ":" in hostinfo:
                addr, port = hostinfo.rsplit(":", 1)
                port = int(port)
            else:
                addr, port = hostinfo, 443
            try:
                decoded = base64.b64decode(userinfo + "==").decode("utf-8")
                method, password = decoded.split(":", 1)
            except Exception:
                method, password = "aes-256-gcm", userinfo
        else:
            if "#" in rest:
                rest = rest.split("#")[0]
            decoded = base64.b64decode(rest + "==").decode("utf-8")
            method_password, hostport = decoded.split("@", 1)
            method, password = method_password.split(":", 1)
            if ":" in hostport:
                addr, port = hostport.rsplit(":", 1)
                port = int(port)
            else:
                addr, port = hostport, 443
        tag = ""
        if "#" in uri:
            tag = urllib.parse.unquote(uri.split("#")[1])
        return {
            "protocol": "shadowsocks",
            "server_addr": addr,
            "server_port": port,
            "password": password,
            "method": method,
            "out_tag": tag or f"SS {addr}",
        }
    except Exception as e:
        logging.warning(f"[解析] ss URI 解析失败: {e}")
        return None


def parse_hy2_uri(uri: str) -> dict | None:
    """解析 hy2:// 链接。"""
    try:
        rest = uri.replace("hy2://", "").replace("hysteria2://", "")
        if "@" in rest:
            password, hostinfo = rest.split("@", 1)
        else:
            return None
        if "?" in hostinfo:
            hostport, query_str = hostinfo.split("?", 1)
        else:
            hostport, query_str = hostinfo, ""
        if "#" in hostport:
            hostport = hostport.split("#")[0]
        if ":" in hostport:
            addr, port = hostport.rsplit(":", 1)
            port = int(port)
        else:
            addr, port = hostport, 443
        query = dict(urllib.parse.parse_qsl(query_str))
        tag = ""
        if "#" in uri:
            tag = urllib.parse.unquote(uri.split("#")[1])
        return {
            "protocol": "hysteria2",
            "server_addr": addr,
            "server_port": port,
            "password": urllib.parse.unquote(password),
            "out_tag": tag or f"HY2 {addr}",
            "sni": query.get("sni", ""),
        }
    except Exception as e:
        logging.warning(f"[解析] hy2 URI 解析失败: {e}")
        return None


def parse_clash_proxies(proxies: list, next_tag_fn: Callable) -> list[dict]:
    """解析 Clash YAML proxies 列表。"""
    protocol_map = {
        "vmess": "vmess", "vless": "vless",
        "trojan": "trojan", "ss": "shadowsocks",
        "shadowsocks": "shadowsocks",
        "hysteria2": "hysteria2", "hy2": "hysteria2",
        "tuic": "tuic", "wireguard": "wireguard", "wg": "wireguard",
        "socks5": "socks", "socks": "socks", "http": "http",
        "anytls": "anytls", "naiveproxy": "naive", "naive": "naive",
    }
    results: list[dict] = []
    for p in proxies:
        if not isinstance(p, dict):
            continue
        ptype = str(p.get("type", "")).lower()
        protocol = protocol_map.get(ptype)
        if not protocol:
            continue

        entry = {
            "protocol": protocol,
            "in_tag": next_tag_fn("in"),
            "out_tag": str(p.get("name", "")) or f"{protocol.upper()} Node",
            "server_addr": str(p.get("server", "")),
            "server_port": int(p.get("port", 443)),
            "uuid": "", "alter_id": "0", "password": "", "method": "",
            "socks_user": "", "socks_pass": "", "http_user": "", "http_pass": "",
            "network": "tcp", "ws_path": "", "ws_host": "",
            "grpc_service_name": "", "kcp_header": "none", "kcp_seed": "",
            "security": "none", "sni": "", "fingerprint": "chrome",
            "public_key": "", "short_id": "", "flow": "", "alpn": "",
        }

        if protocol in ("vless", "vmess"):
            entry["uuid"] = str(p.get("uuid", ""))
            if protocol == "vmess":
                entry["alter_id"] = str(p.get("alterId", 0))
        elif protocol == "trojan":
            entry["password"] = str(p.get("password", ""))
        elif protocol in ("shadowsocks",):
            entry["password"] = str(p.get("password", ""))
            entry["method"] = str(p.get("cipher", p.get("method", "aes-256-gcm")))
        elif protocol == "hysteria2":
            entry["password"] = str(p.get("password", ""))
        elif protocol == "tuic":
            entry["uuid"] = str(p.get("uuid", ""))
            entry["password"] = str(p.get("password", ""))
        elif protocol == "wireguard":
            entry["wg_secret_key"] = str(p.get("private-key", ""))
            entry["wg_public_key"] = str(p.get("public-key", ""))
            entry["wg_address"] = str(p.get("ip", "10.0.0.2/32"))
        elif protocol in ("socks", "http"):
            if p.get("username"):
                entry[f"{protocol}_user"] = str(p["username"])
            if p.get("password"):
                entry[f"{protocol}_pass"] = str(p["password"])
        elif protocol == "anytls":
            entry["anytls_password"] = str(p.get("password", ""))
        elif protocol == "naive":
            entry["naive_user"] = str(p.get("username", ""))
            entry["password"] = str(p.get("password", ""))

        net = str(p.get("network", "tcp")).lower()
        entry["network"] = net

        ws_opts = p.get("ws-opts", {}) or {}
        if net == "ws" and ws_opts:
            entry["ws_path"] = str(ws_opts.get("path", ""))
            headers = ws_opts.get("headers", {}) or {}
            entry["ws_host"] = str(headers.get("Host", ""))

        grpc_opts = p.get("grpc-opts", {}) or {}
        if net == "grpc" and grpc_opts:
            entry["grpc_service_name"] = str(grpc_opts.get("grpc-service-name", ""))

        tls_enabled = p.get("tls", False)
        if tls_enabled:
            entry["security"] = "tls"
            entry["sni"] = str(p.get("sni", p.get("servername", "")))
            entry["fingerprint"] = str(p.get("client-fingerprint", "chrome"))
            alpn = p.get("alpn", [])
            if isinstance(alpn, list) and alpn:
                entry["alpn"] = ",".join(str(a) for a in alpn)

        results.append(entry)

    return results

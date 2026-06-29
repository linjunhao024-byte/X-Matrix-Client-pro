"""
X-Matrix Client — 系统代理管理
"""
from __future__ import annotations

import ctypes
import json
import logging
import os
import sys
import time
import winreg
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import TYPE_CHECKING

from xmatrix.process import run_hidden

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def generate_pac_script(api: XMatrixAPI) -> str:
    """根据当前路由规则动态生成 PAC 脚本。"""
    proxy_addr = f"PROXY 127.0.0.1:{api.config_local_port}"
    direct_suffixes: list[str] = []
    proxy_suffixes: list[str] = []
    has_cn_direct = False
    routing_rules = getattr(api, '_last_routing_rules', None) or []
    for r in routing_rules:
        if not r.get("enabled", True):
            continue
        outbound = r.get("outbound", "direct")
        rtype = r.get("type", "domain")
        content = r.get("content", "")
        if rtype in ("domain", "geosite"):
            entries = [c.strip().lstrip("!") for c in content.split(",") if c.strip()]
            if outbound == "direct":
                direct_suffixes.extend(entries)
                if any(e.lower() in ("cn", ".cn") or e.lower().endswith(".cn") for e in entries):
                    has_cn_direct = True
            elif outbound == "proxy":
                proxy_suffixes.extend(entries)
    if not has_cn_direct:
        cn_domains = [
            ".cn", ".com.cn", ".net.cn", ".org.cn", ".gov.cn", ".edu.cn",
            ".baidu.com", ".qq.com", ".taobao.com", ".tmall.com", ".jd.com",
            ".alibaba.com", ".alipay.com", ".weibo.com", ".zhihu.com", ".bilibili.com",
            ".douyin.com", ".bytedance.com", ".xiaomi.com", ".huawei.com", ".oppo.com",
            ".vivo.com", ".meituan.com", ".dianping.com", ".ele.me", ".pinduoduo.com",
            ".163.com", ".126.com", ".sohu.com", ".sina.com", ".ifeng.com",
            ".csdn.net", ".cnblogs.com", ".aliyun.com", ".tencent.com",
        ]
        direct_suffixes.extend(cn_domains)
    try:
        from xmatrix.constants import DATA_DIR
        cfg_path = os.path.join(DATA_DIR, "config.json")
        if os.path.isfile(cfg_path):
            with open(cfg_path, "r", encoding="utf-8") as f:
                _cfg = json.load(f)
            _exceptions = _cfg.get("system_proxy_exceptions", "<local>") or "<local>"
            for exc in _exceptions.split(";"):
                exc = exc.strip()
                if exc and exc != "<local>":
                    direct_suffixes.append(exc)
    except Exception:
        pass
    direct_suffixes = list(set(direct_suffixes))
    proxy_suffixes = list(set(proxy_suffixes))

    def _js_escape(s: str) -> str:
        return s.replace("\\", "\\\\").replace('"', '\\"').replace("'", "\\'").replace("\n", "\\n").replace("\r", "\\r").replace("</", "<\\/")

    direct_js = ", ".join(f'"{_js_escape(s)}"' for s in direct_suffixes)
    proxy_js = ", ".join(f'"{_js_escape(s)}"' for s in proxy_suffixes) if proxy_suffixes else ""
    pac = f"""function FindProxyForURL(url, host) {{
    if (isPlainHostName(host) || shExpMatch(host, "127.0.0.1") || shExpMatch(host, "localhost") ||
        shExpMatch(host, "192.168.*") || shExpMatch(host, "10.*") || shExpMatch(host, "172.16.*") ||
        shExpMatch(host, "*.local") || shExpMatch(host, "*.localhost"))
        return "DIRECT";
    var DIRECT_SUFFIXES = [{direct_js}];
    for (var i = 0; i < DIRECT_SUFFIXES.length; i++) {{
        if (shExpMatch(host, "*" + DIRECT_SUFFIXES[i]) || host === DIRECT_SUFFIXES[i].replace(/^\\./, ""))
            return "DIRECT";
    }}
    return "{_js_escape(proxy_addr)}; DIRECT";
}}"""
    return pac


def start_pac_server(api: XMatrixAPI) -> None:
    """启动 PAC 服务器。"""
    api_self = api
    class PACRequestHandler(BaseHTTPRequestHandler):
        def do_GET(req) -> None:
            req.send_response(200)
            req.send_header('Content-Type', 'application/x-ns-proxy-autoconfig')
            req.end_headers()
            pac_js = generate_pac_script(api_self)
            req.wfile.write(pac_js.encode('utf-8'))
        def log_message(req, format, *args) -> None:
            pass
    try:
        api.pac_server = HTTPServer(('127.0.0.1', api.pac_port), PACRequestHandler)
        import threading
        threading.Thread(target=api.pac_server.serve_forever, daemon=True).start()
    except OSError as e:
        api.log_queue.put(f"[PAC] 端口 {api.pac_port} 绑定失败: {e}\n")


def toggle_system_proxy(api: XMatrixAPI, enable: bool, port: str = "2077", use_pac: bool = True) -> dict:
    """切换系统代理。"""
    api.sys_proxy_enabled = enable
    api.sys_proxy_pac = use_pac
    proxy_addr = f"127.0.0.1:{port}"

    _exceptions = "<local>"
    try:
        from xmatrix.constants import DATA_DIR
        cfg_path = os.path.join(DATA_DIR, "config.json")
        if os.path.isfile(cfg_path):
            with open(cfg_path, "r", encoding="utf-8") as f:
                _cfg = json.load(f)
            _exceptions = _cfg.get("system_proxy_exceptions", "<local>") or "<local>"
    except Exception:
        pass

    if os.name == "nt":
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER,
                            r"Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                            0, winreg.KEY_ALL_ACCESS) as internet_settings:
            if enable:
                winreg.SetValueEx(internet_settings, "ProxyOverride", 0, winreg.REG_SZ, _exceptions)
                if use_pac:
                    winreg.SetValueEx(internet_settings, "ProxyEnable", 0, winreg.REG_DWORD, 0)
                    winreg.SetValueEx(internet_settings, "AutoConfigURL", 0, winreg.REG_SZ, f"http://127.0.0.1:{api.pac_port}/pac.js?t={int(time.time())}")
                else:
                    try:
                        winreg.DeleteValue(internet_settings, "AutoConfigURL")
                    except FileNotFoundError:
                        pass
                    winreg.SetValueEx(internet_settings, "ProxyEnable", 0, winreg.REG_DWORD, 1)
                    winreg.SetValueEx(internet_settings, "ProxyServer", 0, winreg.REG_SZ, proxy_addr)
            else:
                winreg.SetValueEx(internet_settings, "ProxyEnable", 0, winreg.REG_DWORD, 0)
                try:
                    winreg.DeleteValue(internet_settings, "AutoConfigURL")
                except FileNotFoundError:
                    pass
        internet_set_option = ctypes.windll.wininet.InternetSetOptionW
        internet_set_option(0, 39, 0, 0)
        internet_set_option(0, 37, 0, 0)

    elif sys.platform == "linux":
        proxy_mode = "manual" if enable else "none"
        run_hidden("gsettings", "set", "org.gnome.system.proxy", "mode", proxy_mode)
        if enable:
            run_hidden("gsettings", "set", "org.gnome.system.proxy.http", "host", "127.0.0.1")
            run_hidden("gsettings", "set", "org.gnome.system.proxy.http", "port", port)
            run_hidden("gsettings", "set", "org.gnome.system.proxy.https", "host", "127.0.0.1")
            run_hidden("gsettings", "set", "org.gnome.system.proxy.https", "port", port)
            run_hidden("gsettings", "set", "org.gnome.system.proxy.socks", "host", "127.0.0.1")
            run_hidden("gsettings", "set", "org.gnome.system.proxy.socks", "port", port)

    elif sys.platform == "darwin":
        result = run_hidden("networksetup", "-listallnetworkservices", text=True, timeout=5)
        services = [s.strip() for s in (result.stdout if result else "").splitlines() if s.strip() and not s.startswith("*")]
        active_service = services[0] if services else "Wi-Fi"
        if enable:
            run_hidden("networksetup", "-setwebproxy", active_service, "127.0.0.1", port)
            run_hidden("networksetup", "-setsecurewebproxy", active_service, "127.0.0.1", port)
            run_hidden("networksetup", "-setsocksfirewallproxy", active_service, "127.0.0.1", port)
            run_hidden("networksetup", "-setwebproxystate", active_service, "on")
            run_hidden("networksetup", "-setsecurewebproxystate", active_service, "on")
            run_hidden("networksetup", "-setsocksfirewallproxystate", active_service, "on")
        else:
            run_hidden("networksetup", "-setwebproxystate", active_service, "off")
            run_hidden("networksetup", "-setsecurewebproxystate", active_service, "off")
            run_hidden("networksetup", "-setsocksfirewallproxystate", active_service, "off")

    return {"success": True, "enabled": enable, "mode": "PAC" if use_pac else "Global", "platform": sys.platform}

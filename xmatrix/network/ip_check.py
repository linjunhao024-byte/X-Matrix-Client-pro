"""
X-Matrix Client — IP 检测功能
"""
from __future__ import annotations

import json
import logging
import os
import socket
import time
import urllib.parse
import urllib.request
from typing import TYPE_CHECKING

from xmatrix.constants import DATA_DIR

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def check_outbound_ip(api: XMatrixAPI, port: int = 2077) -> dict:
    """检测出口 IP。"""
    def fetch_ip(use_proxy: bool) -> dict:
        if use_proxy:
            proxy_handler = urllib.request.ProxyHandler({
                "http": f"http://127.0.0.1:{port}",
                "https": f"http://127.0.0.1:{port}",
            })
            opener = urllib.request.build_opener(proxy_handler)
        else:
            opener = urllib.request.build_opener()
        req = urllib.request.Request(
            "http://ip-api.com/json/?fields=status,country,countryCode,regionName,city,timezone,isp,org,as,query",
            headers={"User-Agent": "Mozilla/5.0 X-Matrix/1.0"},
        )
        with opener.open(req, timeout=3) as response:
            return json.loads(response.read().decode("utf-8"))

    try:
        data = fetch_ip(use_proxy=True)
    except Exception:
        try:
            data = fetch_ip(use_proxy=False)
        except Exception:
            return {"success": False, "error": "网络离线"}

    if data.get("status") == "success":
        org = data.get("org", "") or data.get("isp", "")
        return {
            "success": True,
            "ip": data.get("query", ""),
            "country": data.get("country", ""),
            "countryCode": data.get("countryCode", "").lower(),
            "asn": data.get("as", ""),
            "isp": data.get("isp", ""),
            "org": org,
            "location": f"{data.get('city', '')}, {data.get('regionName', '')}",
            "timezone": data.get("timezone", "")
        }
    return {"success": False, "error": "API返回异常"}


def check_ip_quality(api: XMatrixAPI, port: int = 2077, source: str = "ippure", token: str = "") -> dict:
    """IP 质量检测。"""
    ipinfo_token = token or _load_ipinfo_token()
    ipinfo_url = f"https://api.ipinfo.io/lite/me?token={ipinfo_token}" if ipinfo_token else ""
    api_map = {
        "ippure": ("https://my.ippure.com/v1/info", _adapter_ippure),
        "ipinfo": (ipinfo_url, _adapter_ipinfo),
        "ipapi": ("http://ip-api.com/json/?fields=status,country,countryCode,region,regionName,city,isp,org,as,mobile,proxy,hosting,query", _adapter_ipapi),
    }

    if source not in api_map:
        return {"success": False, "error": f"不支持的数据源: {source}"}

    url, adapter = api_map[source]
    if source == "ipinfo" and not url:
        logging.warning("[IP检测] ipinfo.io 未配置 token，自动降级为 ipapi")
        url, adapter = api_map["ipapi"]

    try:
        raw = _fetch_via_proxy(api, url, port)
    except Exception as e:
        err_str = str(e).lower()
        if "timed out" in err_str or "timeout" in err_str:
            return {"success": False, "error": "代理节点响应超时，请检查节点质量或更换节点。"}
        return {"success": False, "error": f"探测发生异常: {str(e)}"}

    result = adapter(raw)
    return {"success": True, "data": result}


def test_website_access(api: XMatrixAPI, url: str, port: int = 2077) -> dict:
    """测试网站访问。"""
    def do_request(use_proxy: bool):
        if use_proxy:
            proxy_handler = urllib.request.ProxyHandler({
                "http": f"http://127.0.0.1:{port}", "https": f"http://127.0.0.1:{port}",
            })
            opener = urllib.request.build_opener(proxy_handler)
        else:
            opener = urllib.request.build_opener()
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 X-Matrix/1.0"})
        with opener.open(req, timeout=5) as response:
            return response.status

    start_time = time.perf_counter()
    try:
        try:
            status = do_request(use_proxy=True)
        except Exception as e:
            if "10061" in str(e) or "refused" in str(e).lower():
                start_time = time.perf_counter()
                status = do_request(use_proxy=False)
            else:
                raise e
        ms = int((time.perf_counter() - start_time) * 1000)
        return {"success": True, "ms": ms, "status": status}
    except urllib.error.HTTPError as e:
        ms = int((time.perf_counter() - start_time) * 1000)
        return {"success": True, "ms": ms, "status": e.code}
    except Exception:
        return {"success": False, "error": "Timeout"}


def probe_single_node(api: XMatrixAPI, index: int) -> dict:
    """探测单个节点。"""
    if not (0 <= index < len(api.tunnels)):
        return {"success": False, "error": "索引越界"}
    t = api.tunnels[index]
    addr = t.get("server_addr", "")
    port = api._safe_port(t.get("server_port"), 443)
    tag = t.get("out_tag", f"节点 {index}")

    # TCP Ping
    latency_a = -1
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(3)
            start = time.perf_counter()
            result = sock.connect_ex((addr, port))
            latency_a = int((time.perf_counter() - start) * 1000) if result == 0 else -1
    except Exception:
        latency_a = -1

    return {
        "success": True,
        "index": index,
        "tag": tag,
        "latency_a": latency_a,
        "addr": addr,
        "port": port,
    }


def _fetch_via_proxy(api: XMatrixAPI, url: str, port: int = 2077, timeout: int = 8) -> dict:
    """通过代理隧道发起 HTTP 请求。"""
    def _do_fetch(use_proxy: bool) -> dict:
        if use_proxy:
            proxy_handler = urllib.request.ProxyHandler({
                "http": f"http://127.0.0.1:{port}", "https": f"http://127.0.0.1:{port}",
            })
            opener = urllib.request.build_opener(proxy_handler)
        else:
            opener = urllib.request.build_opener()
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 X-Matrix/1.0"})
        with opener.open(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode("utf-8"))

    try:
        return _do_fetch(use_proxy=True)
    except Exception as e:
        if "10061" in str(e) or "refused" in str(e).lower():
            return _do_fetch(use_proxy=False)
        raise


def _load_ipinfo_token() -> str:
    """从 data/config.json 读取 ipinfo.io API token。"""
    cfg_path = os.path.join(DATA_DIR, "config.json")
    try:
        if os.path.isfile(cfg_path):
            with open(cfg_path, "r", encoding="utf-8") as f:
                return json.load(f).get("ipinfo_token", "")
    except Exception:
        pass
    return ""


def _adapter_ippure(data: dict) -> dict:
    """IPPure 数据归一化。"""
    return {
        "ip": data.get("ip", ""),
        "asn": f"AS{data.get('asn', '')}" if data.get("asn") else "-",
        "org": data.get("asOrganization", ""),
        "country": data.get("country", ""),
        "countryCode": data.get("countryCode", "un").lower(),
        "city": None,
        "region": None,
        "timezone": None,
        "isp": None,
        "continent": None,
        "as_domain": None,
        "asname": None,
        "isResidential": data.get("isResidential", False),
        "fraudScore": data.get("fraudScore", 0),
        "isProxy": None,
        "isHosting": None,
        "isMobile": None,
        "source": "ippure",
    }


def _adapter_ipinfo(data: dict) -> dict:
    """IPInfo Lite 数据归一化。"""
    org_raw = data.get("as_name", "") or data.get("org", "")
    asn = data.get("asn", "-")
    return {
        "ip": data.get("ip", ""),
        "asn": asn if asn.startswith("AS") else f"AS{asn}",
        "org": org_raw,
        "country": data.get("country", ""),
        "countryCode": data.get("country_code", "un").lower(),
        "city": data.get("city"),
        "region": data.get("region"),
        "timezone": None,
        "isp": None,
        "continent": data.get("continent"),
        "as_domain": data.get("as_domain"),
        "isResidential": None,
        "fraudScore": None,
        "isProxy": None,
        "isHosting": None,
        "isMobile": None,
        "source": "ipinfo",
    }


def _adapter_ipapi(data: dict) -> dict:
    """ip-api.com 数据归一化。"""
    asn_raw = data.get("as", "")
    asn = asn_raw.split(" ")[0] if asn_raw else "-"
    return {
        "ip": data.get("query", ""),
        "asn": asn,
        "org": data.get("org", "") or data.get("isp", ""),
        "country": data.get("country", ""),
        "countryCode": data.get("countryCode", "un").lower(),
        "city": data.get("city"),
        "region": data.get("regionName"),
        "timezone": data.get("timezone"),
        "isp": data.get("isp"),
        "continent": None,
        "as_domain": None,
        "asname": data.get("asname"),
        "isResidential": None,
        "fraudScore": None,
        "isProxy": data.get("proxy", False),
        "isHosting": data.get("hosting", False),
        "isMobile": data.get("mobile", False),
        "source": "ipapi",
    }

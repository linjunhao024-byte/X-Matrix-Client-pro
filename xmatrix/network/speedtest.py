"""
X-Matrix Client — 测速功能
"""
from __future__ import annotations

import json
import os
import socket
import ssl
import struct
import time
import urllib.parse
import urllib.request
from typing import TYPE_CHECKING

from xmatrix.constants import DATA_DIR, CORE_REGISTRY
from xmatrix.process import popen_hidden

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def test_node_tcp_ping(api: XMatrixAPI, index: int) -> dict:
    """TCP Ping 测试。"""
    if not (0 <= index < len(api.tunnels)):
        return {"success": False, "error": "索引越界"}
    t = api.tunnels[index]
    addr = t.get("server_addr", "")
    try:
        port = int(t.get("server_port", 443))
    except ValueError:
        return {"success": False, "error": "端口无效"}
    start_time = time.perf_counter()
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.settimeout(3)
        result = sock.connect_ex((addr, port))
        elapsed_ms = int((time.perf_counter() - start_time) * 1000)
        if result == 0:
            return {"success": True, "delay": elapsed_ms}
        return {"success": True, "delay": -1}


def test_node_real_delay(api: XMatrixAPI, index: int, test_url: str = "", timeout: int = 5, local_port: int = 2077) -> dict:
    """真实延迟测试。"""
    if not (0 <= index < len(api.tunnels)):
        return {"success": False, "error": "索引越界"}
    if not test_url:
        try:
            cfg_path = os.path.join(DATA_DIR, "config.json")
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    test_url = json.load(f).get("speed_ping_test_url", "")
        except Exception:
            pass
        if not test_url:
            test_url = "https://www.google.com/generate_204"

    parsed = urllib.parse.urlparse(test_url)
    host = parsed.hostname
    target_port = parsed.port or (443 if parsed.scheme == 'https' else 80)
    path = parsed.path or "/"
    if parsed.query:
        path += f"?{parsed.query}"

    def do_request(proxy_port: int) -> int:
        try:
            timeout_sec = float(timeout) / 1000.0
            s = socket.create_connection(("127.0.0.1", proxy_port), timeout=timeout_sec)
            s.settimeout(timeout_sec)
            s.sendall(b"\x05\x01\x00")
            if s.recv(2) != b"\x05\x00":
                return -1
            host_bytes = host.encode('utf-8')
            req = b"\x05\x01\x00\x03" + bytes([len(host_bytes)]) + host_bytes + struct.pack(">H", target_port)
            s.sendall(req)
            resp = s.recv(10)
            if len(resp) < 2 or resp[1] != 0x00:
                return -1
            if parsed.scheme == 'https':
                ctx = ssl._create_unverified_context()
                s = ctx.wrap_socket(s, server_hostname=host)
            start_time = time.perf_counter()
            http_req = f"GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: v2rayN/3.0\r\nConnection: close\r\n\r\n"
            s.sendall(http_req.encode('utf-8'))
            data = s.recv(4096)
            elapsed_ms = int((time.perf_counter() - start_time) * 1000)
            s.close()
            if b"HTTP/1.1 204" in data or b"HTTP/1.1 200" in data:
                return elapsed_ms
            return -1
        except Exception:
            return -1

    with api._process_lock:
        _proc_alive = api.xray_process is not None and api.xray_process.poll() is None
    if index == api.active_index and _proc_alive:
        delay1 = do_request(local_port)
        delay2 = do_request(local_port) if delay1 > 0 else -1
        delays = [d for d in (delay1, delay2) if d > 0]
        if delays:
            return {"success": True, "delay": min(delays), "proxy_port": local_port}
        return {"success": True, "delay": -1, "proxy_port": local_port}
    return {"success": True, "delay": -1, "proxy_port": local_port}


def test_download_speed(api: XMatrixAPI, url: str, port: int = 2077) -> dict:
    """下载速度测试。"""
    proxy_handler = urllib.request.ProxyHandler({
        "http": f"http://127.0.0.1:{port}",
        "https": f"http://127.0.0.1:{port}",
    })
    opener = urllib.request.build_opener(proxy_handler)
    try:
        start_time = time.perf_counter()
        req = urllib.request.Request(url, headers={"User-Agent": "X-Matrix/1.0"})
        with opener.open(req, timeout=15) as resp:
            total_bytes = 0
            while True:
                chunk = resp.read(8192)
                if not chunk:
                    break
                total_bytes += len(chunk)
        elapsed = time.perf_counter() - start_time
        speed_mbps = (total_bytes * 8) / (elapsed * 1_000_000) if elapsed > 0 else 0
        return {"success": True, "speed_mbps": round(speed_mbps, 2), "size_mb": round(total_bytes / 1_048_576, 2), "elapsed": round(elapsed, 2)}
    except Exception as e:
        return {"success": False, "error": str(e)}


def test_node_udp_ping(api: XMatrixAPI, index: int) -> dict:
    """UDP Ping 测试。"""
    if not (0 <= index < len(api.tunnels)):
        return {"success": False, "error": "索引越界"}
    t = api.tunnels[index]
    addr = t.get("server_addr", "")
    try:
        port = int(t.get("server_port", 443))
    except ValueError:
        return {"success": False, "error": "端口无效"}
    try:
        start_time = time.perf_counter()
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
            sock.settimeout(3)
            sock.sendto(b"\x00" * 4, (addr, port))
            try:
                sock.recvfrom(64)
                elapsed_ms = int((time.perf_counter() - start_time) * 1000)
                return {"success": True, "delay": elapsed_ms}
            except socket.timeout:
                elapsed_ms = int((time.perf_counter() - start_time) * 1000)
                return {"success": True, "delay": elapsed_ms, "note": "no_response"}
    except Exception as e:
        return {"success": True, "delay": -1, "error": str(e)}


def test_node_mixed(api: XMatrixAPI, index: int, test_url: str = "", timeout: int = 5, local_port: int = 2077) -> dict:
    """混合测速。"""
    if not (0 <= index < len(api.tunnels)):
        return {"success": False, "error": "索引越界"}
    if not test_url:
        try:
            cfg_path = os.path.join(DATA_DIR, "config.json")
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    test_url = json.load(f).get("speed_ping_test_url", "")
        except Exception:
            pass
        if not test_url:
            test_url = "https://www.google.com/generate_204"
    tcp_result = test_node_tcp_ping(api, index)
    tcp_delay = tcp_result.get("delay", -1) if tcp_result.get("success") else -1
    real_result = test_node_real_delay(api, index, test_url, timeout, local_port)
    real_delay = real_result.get("delay", -1) if real_result.get("success") else -1
    return {
        "success": True,
        "tcp_delay": tcp_delay,
        "real_delay": real_delay,
        "index": index,
        "tag": api.tunnels[index].get("out_tag", ""),
    }


def test_node_bandwidth(api: XMatrixAPI, index: int, test_url: str = "", test_bytes: int = 10000000, timeout: int = 30) -> dict:
    """带宽测试。"""
    if not (0 <= index < len(api.tunnels)):
        return {"success": False, "error": "索引越界"}
    if not test_url:
        try:
            cfg_path = os.path.join(DATA_DIR, "config.json")
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    test_url = json.load(f).get("speed_bandwidth_test_url", "")
        except Exception:
            pass
        if not test_url:
            test_url = f"https://speed.cloudflare.com/__down?bytes={test_bytes}"

    t = api.tunnels[index]
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(('127.0.0.1', 0))
        test_port = s.getsockname()[1]

    proxy_handler = urllib.request.ProxyHandler({
        "http": f"http://127.0.0.1:{test_port}",
        "https": f"http://127.0.0.1:{test_port}",
    })
    opener = urllib.request.build_opener(proxy_handler)
    try:
        start_time = time.perf_counter()
        total_bytes = 0
        req = urllib.request.Request(test_url, headers={"User-Agent": "X-Matrix/1.0"})
        with opener.open(req, timeout=timeout) as resp:
            while True:
                chunk = resp.read(65536)
                if not chunk:
                    break
                total_bytes += len(chunk)
        elapsed = time.perf_counter() - start_time
        bandwidth_mbps = (total_bytes * 8) / (elapsed * 1000000) if elapsed > 0 else 0
        api.update_profile(t.get("id", ""), {"speed": round(bandwidth_mbps, 2)})
        return {
            "success": True,
            "index": index,
            "bandwidth_mbps": round(bandwidth_mbps, 2),
            "latency_ms": -1,
            "total_bytes": total_bytes,
            "elapsed_seconds": round(elapsed, 2),
        }
    except Exception as e:
        return {"success": False, "error": str(e)}


def batch_test_real_delay(api: XMatrixAPI, node_indices: list[int], concurrency: int = 5) -> dict:
    """批量延迟测试。"""
    import concurrent.futures

    def _test_one(idx: int) -> dict:
        try:
            result = test_node_real_delay(api, idx)
            if isinstance(result, dict) and result.get("success"):
                return {"index": idx, "delay": result.get("delay", -1), "success": True}
        except Exception:
            pass
        return {"index": idx, "delay": -1, "success": False}

    all_results: list[dict] = []
    remaining = list(node_indices)
    max_retries = 2

    for retry_round in range(max_retries + 1):
        if not remaining:
            break
        round_results: list[dict] = []
        with concurrent.futures.ThreadPoolExecutor(max_workers=concurrency) as pool:
            futures = {pool.submit(_test_one, idx): idx for idx in remaining}
            for fut in concurrent.futures.as_completed(futures):
                round_results.append(fut.result())
        failed = [r["index"] for r in round_results if not r["success"]]
        passed = [r for r in round_results if r["success"]]
        all_results.extend(passed)
        if not failed:
            break
        concurrency = max(1, concurrency // 2)
        remaining = failed

    for idx in remaining:
        all_results.append({"index": idx, "delay": -1, "success": False})

    return {"success": True, "results": all_results, "total": len(node_indices),
            "passed": len([r for r in all_results if r["success"]]),
            "failed": len([r for r in all_results if not r["success"]])}


def batch_test_bandwidth(api: XMatrixAPI, node_indices: list[int], max_workers: int = 3, test_url: str = "", test_bytes: int = 10000000) -> dict:
    """批量带宽测试。"""
    import concurrent.futures

    def _test_one(idx: int) -> dict:
        try:
            result = test_node_bandwidth(api, idx, test_url=test_url, test_bytes=test_bytes)
            if isinstance(result, dict) and result.get("success"):
                return {
                    "index": idx,
                    "bandwidth_mbps": result.get("bandwidth_mbps", 0),
                    "latency_ms": result.get("latency_ms", -1),
                    "success": True,
                }
        except Exception:
            pass
        return {"index": idx, "bandwidth_mbps": 0, "latency_ms": -1, "success": False}

    all_results: list[dict] = []
    remaining = list(node_indices)
    max_retries = 2

    for retry_round in range(max_retries + 1):
        if not remaining:
            break
        round_results: list[dict] = []
        with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as pool:
            futures = {pool.submit(_test_one, idx): idx for idx in remaining}
            for fut in concurrent.futures.as_completed(futures):
                round_results.append(fut.result())
        failed = [r["index"] for r in round_results if not r["success"]]
        passed = [r for r in round_results if r["success"]]
        all_results.extend(passed)
        if not failed:
            break
        max_workers = max(1, max_workers // 2)
        remaining = failed

    for idx in remaining:
        all_results.append({"index": idx, "bandwidth_mbps": 0, "latency_ms": -1, "success": False})

    return {
        "success": True,
        "results": all_results,
        "total": len(node_indices),
        "passed": len([r for r in all_results if r["success"]]),
        "failed": len([r for r in all_results if not r["success"]]),
    }


def build_speedtest_config(api: XMatrixAPI, node_indices: list[int], base_port: int = 20801) -> dict:
    """生成测速配置。"""
    configs: list[dict] = []
    temp_dir = os.path.join(DATA_DIR, "speedtest")
    os.makedirs(temp_dir, exist_ok=True)
    for i, idx in enumerate(node_indices):
        if not (0 <= idx < len(api.tunnels)):
            continue
        t = api.tunnels[idx]
        if t.get("protocol") == "policy_group":
            continue
        port = base_port + i
        tag = t.get("out_tag", f"node-{idx}")
        configs.append({"port": port, "index": idx, "tag": tag})
    return {"success": True, "configs": configs, "temp_dir": temp_dir}


def cleanup_speedtest_configs() -> dict:
    """清理测速配置。"""
    import shutil
    temp_dir = os.path.join(DATA_DIR, "speedtest")
    if os.path.exists(temp_dir):
        shutil.rmtree(temp_dir, ignore_errors=True)
    return {"success": True}


def test_port(server_addr: str, server_port: str) -> dict:
    """测试端口。"""
    try:
        port = int(server_port)
    except (ValueError, TypeError):
        return {"success": False, "error": f"无效端口: {server_port}"}
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.settimeout(5)
        result = sock.connect_ex((server_addr, port))
        return {"success": True, "status": "open" if result == 0 else "closed"}

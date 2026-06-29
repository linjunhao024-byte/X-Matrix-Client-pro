"""
X-Matrix Client — Clash 兼容 API 客户端
通过 Clash 兼容 API 轮询 sing-box / mihomo 流量统计（端口可配置）。
"""
from __future__ import annotations

import json
import socket

from xmatrix.helpers import load_port_config


class SingboxStatsClient:
    """通过 Clash 兼容 API 轮询 sing-box / mihomo 流量统计。"""

    _API_ADDR = "127.0.0.1"

    @classmethod
    def _get_api_port(cls) -> int:
        return load_port_config()["clash_api_port"]

    @classmethod
    def query(cls) -> dict[str, int]:
        """查询 Clash API /connections，返回聚合的流量统计。"""
        api_port = cls._get_api_port()
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(3)
        try:
            sock.connect((cls._API_ADDR, api_port))
            request = (
                "GET /connections HTTP/1.1\r\n"
                f"Host: {cls._API_ADDR}:{api_port}\r\n"
                "Connection: close\r\n\r\n"
            )
            sock.sendall(request.encode())

            resp = b""
            while True:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                resp += chunk
                if len(resp) > 131072:
                    break

            # 解析 HTTP 响应体
            body_start = resp.find(b"\r\n\r\n")
            if body_start < 0:
                return {}
            body = resp[body_start + 4:]

            # 处理 chunked encoding
            if b"Transfer-Encoding: chunked" in resp[:body_start]:
                decoded = b""
                while body:
                    nl = body.find(b"\r\n")
                    if nl < 0:
                        break
                    size_str = body[:nl].decode("utf-8", errors="ignore").strip()
                    try:
                        chunk_size = int(size_str, 16)
                    except ValueError:
                        break
                    if chunk_size == 0:
                        break
                    decoded += body[nl + 2:nl + 2 + chunk_size]
                    body = body[nl + 2 + chunk_size + 2:]
                body = decoded

            if not body:
                return {}

            data = json.loads(body.decode("utf-8", errors="ignore"))
            connections = data.get("connections", [])

            # 聚合流量统计
            stats: dict[str, int] = {}
            for conn in connections:
                upload = conn.get("upload", 0)
                download = conn.get("download", 0)
                chain = conn.get("chains", [])
                out_tag = chain[0] if chain else "direct"

                up_key = f"outbound>>>{out_tag}>>>uplink"
                down_key = f"outbound>>>{out_tag}>>>downlink"
                stats[up_key] = stats.get(up_key, 0) + upload
                stats[down_key] = stats.get(down_key, 0) + download

            return stats
        except (OSError, json.JSONDecodeError, ValueError):
            return {}
        finally:
            sock.close()

    @classmethod
    def query_connections(cls) -> list[dict]:
        """查询当前活跃连接列表（用于连接雷达）。"""
        api_port = cls._get_api_port()
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(3)
        try:
            sock.connect((cls._API_ADDR, api_port))
            request = (
                "GET /connections HTTP/1.1\r\n"
                f"Host: {cls._API_ADDR}:{api_port}\r\n"
                "Connection: close\r\n\r\n"
            )
            sock.sendall(request.encode())

            resp = b""
            while True:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                resp += chunk
                if len(resp) > 131072:
                    break

            body_start = resp.find(b"\r\n\r\n")
            if body_start < 0:
                return []
            body = resp[body_start + 4:]

            if b"Transfer-Encoding: chunked" in resp[:body_start]:
                decoded = b""
                while body:
                    nl = body.find(b"\r\n")
                    if nl < 0:
                        break
                    size_str = body[:nl].decode("utf-8", errors="ignore").strip()
                    try:
                        chunk_size = int(size_str, 16)
                    except ValueError:
                        break
                    if chunk_size == 0:
                        break
                    decoded += body[nl + 2:nl + 2 + chunk_size]
                    body = body[nl + 2 + chunk_size + 2:]
                body = decoded

            if not body:
                return []

            data = json.loads(body.decode("utf-8", errors="ignore"))
            return data.get("connections", [])
        except (OSError, json.JSONDecodeError, ValueError):
            return []
        finally:
            sock.close()

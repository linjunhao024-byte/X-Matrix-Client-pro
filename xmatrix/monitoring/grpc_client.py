"""
X-Matrix Client — gRPC / HTTP2 原生客户端
通过原生 socket 实现 HTTP/2 + gRPC 调用 Xray StatsService。
不依赖任何第三方库，零子进程开销。
"""
from __future__ import annotations

import socket
import struct

from xmatrix.helpers import load_port_config


class GrpcStatsClient:
    """通过原生 socket 实现 HTTP/2 + gRPC 调用 Xray StatsService。"""

    _GRPC_PATH = b"/xray.app.stats.command.StatsService/QueryStats"

    @classmethod
    def _get_grpc_addr(cls) -> tuple[str, int]:
        return ("127.0.0.1", load_port_config()["api_port"])

    # ── varint 编解码 ────────────────────────────────────────────────

    @staticmethod
    def _encode_varint(value: int) -> bytes:
        result = b""
        while value > 0x7F:
            result += bytes([(value & 0x7F) | 0x80])
            value >>= 7
        return result + bytes([value & 0x7F])

    @staticmethod
    def _decode_varint(data: bytes, offset: int) -> tuple[int, int]:
        result, shift = 0, 0
        while offset < len(data):
            b = data[offset]
            result |= (b & 0x7F) << shift
            offset += 1
            if (b & 0x80) == 0:
                break
            shift += 7
        return result, offset

    # ── protobuf 解析 ────────────────────────────────────────────────

    @classmethod
    def _parse_stats_message(cls, data: bytes) -> dict[str, int]:
        stats: dict[str, int] = {}
        offset = 0
        while offset < len(data):
            tag_val, offset = cls._decode_varint(data, offset)
            field_number, wire_type = tag_val >> 3, tag_val & 0x07
            if wire_type == 2:
                length, offset = cls._decode_varint(data, offset)
                if offset + length > len(data):
                    break
                msg_bytes = data[offset : offset + length]
                offset += length
                if field_number == 1:
                    name, value, moff = "", 0, 0
                    while moff < len(msg_bytes):
                        mt, moff = cls._decode_varint(msg_bytes, moff)
                        mw = mt & 0x07
                        if mw == 2:
                            sl, moff = cls._decode_varint(msg_bytes, moff)
                            name = msg_bytes[moff : moff + sl].decode("utf-8", errors="ignore")
                            moff += sl
                        elif mw == 0:
                            value, moff = cls._decode_varint(msg_bytes, moff)
                        else:
                            break
                    if name:
                        stats[name] = value
            elif wire_type == 0:
                _, offset = cls._decode_varint(data, offset)
            else:
                break
        return stats

    # ── HTTP/2 + gRPC 帧构造 ────────────────────────────────────────

    @staticmethod
    def _hpack_literal(name: bytes, value: bytes) -> bytes:
        """RFC 7541 Literal Header Field without Indexing"""
        return b"\x00" + bytes([len(name)]) + name + bytes([len(value)]) + value

    @classmethod
    def _build_grpc_request(cls) -> bytes:
        grpc_payload = b"\x00" + struct.pack(">I", 0)
        hdr_block = b""
        for n, v in [
            (b":method", b"POST"), (b":scheme", b"http"),
            (b":path", cls._GRPC_PATH), (b":authority", f"127.0.0.1:{load_port_config()['api_port']}".encode()),
            (b"content-type", b"application/grpc"), (b"te", b"trailers"),
        ]:
            hdr_block += cls._hpack_literal(n, v)
        hdr_frame = struct.pack(">I", len(hdr_block))[1:] + b"\x01\x04" + struct.pack(">I", 1) + hdr_block
        data_frame = struct.pack(">I", len(grpc_payload))[1:] + b"\x00\x01" + struct.pack(">I", 1) + grpc_payload
        return hdr_frame + data_frame

    # ── 公开接口 ─────────────────────────────────────────────────────

    @classmethod
    def query(cls) -> dict[str, int]:
        """执行一次 gRPC StatsQuery，返回 name→value 映射。"""
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(3)
        try:
            sock.connect(cls._get_grpc_addr())
            sock.sendall(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
            sock.sendall(b"\x00\x00\x00\x04\x00\x00\x00\x00\x00")
            sock.sendall(cls._build_grpc_request())

            resp = b""
            while True:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                resp += chunk
                if b"\x00\x00\x00\x07\x00\x00\x00\x00\x00" in resp:
                    break
                if len(resp) > 65536:
                    break

            offset = 0
            while offset + 9 <= len(resp):
                frame_len = (resp[offset] << 16) | (resp[offset + 1] << 8) | resp[offset + 2]
                frame_type = resp[offset + 3]
                stream_id_int = struct.unpack(">I", b"\x00" + resp[offset + 5 : offset + 9])[0] & 0x7FFFFFFF
                payload = resp[offset + 9 : offset + 9 + frame_len]
                offset += 9 + frame_len
                if frame_type == 0 and stream_id_int == 1 and len(payload) > 5:
                    return cls._parse_stats_message(payload[5:])
            return {}
        finally:
            sock.close()

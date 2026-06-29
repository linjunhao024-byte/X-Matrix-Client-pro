"""
X-Matrix Client — 节点扩展数据 (ProfileEx) 持久化
"""
from __future__ import annotations

import json
import logging
from typing import TYPE_CHECKING

from xmatrix.storage.db import get_db

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def load_profiles() -> dict:
    """从 SQLite node_ext 表读取所有节点扩展数据。"""
    conn = get_db()
    try:
        rows = conn.execute(
            "SELECT node_id, delay, speed, ip_info, sort_order, message FROM node_ext"
        ).fetchall()
        profiles: dict[str, dict] = {}
        for row in rows:
            node_id = row["node_id"]
            ip_info_raw = row["ip_info"]
            try:
                ip_info = json.loads(ip_info_raw) if ip_info_raw else {}
            except (json.JSONDecodeError, TypeError):
                ip_info = {}
            profiles[node_id] = {
                "delay": row["delay"] if row["delay"] is not None else -1,
                "speed": row["speed"] if row["speed"] is not None else 0,
                "ip_info": ip_info,
                "sort_order": row["sort_order"] if row["sort_order"] is not None else 0,
                "message": row["message"] or "",
            }
        return profiles
    except Exception as e:
        logging.warning(f"[ProfileEx] 从 SQLite 加载失败: {e}")
        return {}
    finally:
        conn.close()


def get_profiles() -> dict:
    """返回所有节点的扩展数据。"""
    return {"success": True, "profiles": load_profiles()}


def update_profile(node_id: str, data: dict) -> dict:
    """更新单个节点的扩展数据（delay / speed / ip_info / sort_order / message 等）。"""
    if not node_id:
        return {"success": False, "error": "node_id 不能为空"}
    conn = get_db()
    try:
        # 读取现有数据
        row = conn.execute(
            "SELECT delay, speed, ip_info, sort_order, message FROM node_ext WHERE node_id = ?",
            (node_id,)
        ).fetchone()
        if row:
            # 合并更新
            delay = data.get("delay", row["delay"] if row["delay"] is not None else -1)
            speed = data.get("speed", row["speed"] if row["speed"] is not None else 0)
            sort_order = data.get("sort_order", row["sort_order"] if row["sort_order"] is not None else 0)
            message = data.get("message", row["message"] or "")
            # ip_info 需要合并
            ip_info_raw = row["ip_info"]
            try:
                existing_ip_info = json.loads(ip_info_raw) if ip_info_raw else {}
            except (json.JSONDecodeError, TypeError):
                existing_ip_info = {}
            if "ip_info" in data and isinstance(data["ip_info"], dict):
                existing_ip_info.update(data["ip_info"])
            ip_info_str = json.dumps(existing_ip_info, ensure_ascii=False)
        else:
            # 新记录
            delay = data.get("delay", -1)
            speed = data.get("speed", 0)
            sort_order = data.get("sort_order", 0)
            message = data.get("message", "")
            ip_info = data.get("ip_info", {})
            ip_info_str = json.dumps(ip_info, ensure_ascii=False) if isinstance(ip_info, dict) else "{}"
        conn.execute(
            """INSERT OR REPLACE INTO node_ext (node_id, delay, speed, ip_info, sort_order, message, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)""",
            (node_id, delay, speed, ip_info_str, sort_order, message)
        )
        conn.commit()
        return {"success": True}
    except Exception as e:
        logging.warning(f"[ProfileEx] 更新失败 node_id={node_id}: {e}")
        return {"success": False, "error": str(e)}
    finally:
        conn.close()


def migrate_profiles_on_refresh(old_tunnels: list[dict], new_tunnels: list[dict]) -> None:
    """订阅刷新后，通过 server_addr:port:protocol 匹配迁移扩展数据（SQLite 版）。"""
    conn = get_db()
    try:
        # 构建旧节点 identity → node_id 映射
        old_identity_map: dict[str, str] = {}
        for t in old_tunnels:
            if t.get("protocol") == "policy_group":
                continue
            identity = f"{t.get('server_addr', '')}:{t.get('server_port', 443)}:{t.get('protocol', '')}"
            old_id = t.get("id", "")
            if old_id:
                old_identity_map[identity] = old_id
        # 匹配新节点并迁移
        migrated = 0
        for t in new_tunnels:
            if t.get("protocol") == "policy_group":
                continue
            identity = f"{t.get('server_addr', '')}:{t.get('server_port', 443)}:{t.get('protocol', '')}"
            new_id = t.get("id", "")
            if identity in old_identity_map and new_id:
                old_id = old_identity_map[identity]
                # 从旧节点复制扩展数据到新节点
                conn.execute(
                    """INSERT OR REPLACE INTO node_ext (node_id, delay, speed, ip_info, sort_order, message, updated_at)
                       SELECT ?, delay, speed, ip_info, sort_order, message, CURRENT_TIMESTAMP
                       FROM node_ext WHERE node_id = ?""",
                    (new_id, old_id)
                )
                migrated += 1
        if migrated:
            conn.commit()
            logging.info(f"[ProfileEx] 订阅刷新迁移完成，迁移 {migrated} 个节点")
    except Exception as e:
        logging.warning(f"[ProfileEx] 订阅刷新迁移失败: {e}")
    finally:
        conn.close()

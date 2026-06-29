"""
X-Matrix Client — 流量统计持久化
"""
from __future__ import annotations

import json
import logging
import os
import time
from typing import TYPE_CHECKING

from xmatrix.constants import DATA_DIR
from xmatrix.helpers import atomic_write_json
from xmatrix.storage.db import get_db

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI

STATS_FILE = os.path.join(DATA_DIR, "stats.json")


def load_stats() -> dict:
    """加载流量统计：优先从 SQLite 读取，fallback 到 JSON 文件。"""
    # 优先从 SQLite node_stats 表读取
    conn = get_db()
    try:
        rows = conn.execute(
            "SELECT node_id, today_up, today_down, total_up, total_down, last_reset FROM node_stats"
        ).fetchall()
        if rows:
            per_node: dict[str, dict] = {}
            for row in rows:
                per_node[row["node_id"]] = {
                    "today_up": row["today_up"] or 0,
                    "today_down": row["today_down"] or 0,
                    "total_up": row["total_up"] or 0,
                    "total_down": row["total_down"] or 0,
                    "date": row["last_reset"] or "",
                }
            # 计算全局总计
            total_up = sum(v.get("total_up", 0) for v in per_node.values())
            total_down = sum(v.get("total_down", 0) for v in per_node.values())
            return {"up": total_up, "down": total_down, "per_node": per_node}
    except Exception as e:
        logging.warning(f"[统计] SQLite 加载失败，fallback 到 JSON: {e}")
    finally:
        conn.close()
    # Fallback: 从 JSON 文件读取
    try:
        if os.path.exists(STATS_FILE):
            with open(STATS_FILE, "r", encoding="utf-8") as f:
                data = json.load(f)
                return {
                    "up": int(data.get("total_up", 0)),
                    "down": int(data.get("total_down", 0)),
                    "per_node": data.get("per_node", {}),
                }
    except Exception as e:
        logging.warning(f"[统计] JSON 加载失败: {e}")
    return {"up": 0, "down": 0, "per_node": {}}


def save_stats(stats_offset: dict) -> None:
    """保存流量统计：同时写入 SQLite 和 JSON 备份。"""
    try:
        total_up = stats_offset.get("up", 0)
        total_down = stats_offset.get("down", 0)
        per_node = stats_offset.get("per_node", {})
        today = time.strftime("%Y-%m-%d")
        # 每日重置 today 字段
        for nid, stats in per_node.items():
            if stats.get("date") != today:
                stats["today_up"] = 0
                stats["today_down"] = 0
                stats["date"] = today
        # 写入 SQLite node_stats 表
        conn = get_db()
        try:
            for nid, stats in per_node.items():
                conn.execute(
                    """INSERT OR REPLACE INTO node_stats
                       (node_id, today_up, today_down, total_up, total_down, last_reset, updated_at)
                       VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)""",
                    (nid, stats.get("today_up", 0), stats.get("today_down", 0),
                     stats.get("total_up", 0), stats.get("total_down", 0),
                     stats.get("date", today))
                )
            conn.commit()
        except Exception as e:
            logging.warning(f"[统计] SQLite 保存失败: {e}")
        finally:
            conn.close()
        # 保留 JSON 备份
        atomic_write_json(STATS_FILE, {
            "total_up": total_up,
            "total_down": total_down,
            "per_node": per_node,
            "updated": time.strftime("%Y-%m-%d %H:%M:%S"),
        })
    except Exception as e:
        logging.warning(f"[统计] 流量统计数据保存失败: {e}")


def record_node_traffic(stats_offset: dict, node_id: str, up_bytes: int, down_bytes: int) -> None:
    """记录单个节点的流量增量。"""
    if not node_id:
        return
    per_node = stats_offset.get("per_node", {})
    today = time.strftime("%Y-%m-%d")
    if node_id not in per_node:
        per_node[node_id] = {"today_up": 0, "today_down": 0, "total_up": 0, "total_down": 0, "date": today}
    entry = per_node[node_id]
    if entry.get("date") != today:
        entry["today_up"] = 0
        entry["today_down"] = 0
        entry["date"] = today
    entry["today_up"] += up_bytes
    entry["today_down"] += down_bytes
    entry["total_up"] += up_bytes
    entry["total_down"] += down_bytes
    stats_offset["per_node"] = per_node


def get_node_stats(stats_offset: dict, node_id: str = "") -> dict:
    """返回节点流量统计。node_id 为空时返回所有节点。"""
    per_node = stats_offset.get("per_node", {})
    if node_id:
        stats = per_node.get(node_id, {"today_up": 0, "today_down": 0, "total_up": 0, "total_down": 0})
        return {"success": True, "node_id": node_id, "stats": stats}
    return {"success": True, "per_node": per_node}

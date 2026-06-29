"""
X-Matrix Client — SQLite 数据库初始化与迁移
"""
from __future__ import annotations

import json
import logging
import os
import sqlite3
from typing import TYPE_CHECKING

from xmatrix.constants import DB_FILE, TUNNELS_FILE, SUBSCRIPTIONS_FILE, PROFILES_FILE

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def get_db() -> sqlite3.Connection:
    """获取 SQLite 连接（每线程一个连接）。"""
    conn = sqlite3.connect(DB_FILE, timeout=10)
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA foreign_keys=ON")
    conn.row_factory = sqlite3.Row
    return conn


def init_db(api: XMatrixAPI) -> None:
    """初始化 SQLite 数据库，创建 7 张表。"""
    conn = get_db()
    try:
        conn.executescript("""
            CREATE TABLE IF NOT EXISTS tunnels (
                id TEXT PRIMARY KEY,
                protocol TEXT NOT NULL,
                server_addr TEXT,
                server_port INTEGER,
                uuid TEXT,
                password TEXT,
                out_tag TEXT,
                in_tag TEXT,
                data TEXT NOT NULL,
                sort_order INTEGER DEFAULT 0,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS subscriptions (
                id TEXT PRIMARY KEY,
                name TEXT,
                url TEXT,
                ua TEXT,
                enabled INTEGER DEFAULT 1,
                interval_hours REAL DEFAULT 0,
                last_update TEXT,
                last_count INTEGER DEFAULT 0,
                filter_regex TEXT,
                subconverter_url TEXT,
                target_format TEXT DEFAULT 'clash',
                data TEXT NOT NULL,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS routing_rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                type TEXT NOT NULL,
                content TEXT NOT NULL,
                outbound TEXT DEFAULT 'proxy',
                enabled INTEGER DEFAULT 1,
                sort_order INTEGER DEFAULT 0,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS node_stats (
                node_id TEXT PRIMARY KEY,
                today_up INTEGER DEFAULT 0,
                today_down INTEGER DEFAULT 0,
                total_up INTEGER DEFAULT 0,
                total_down INTEGER DEFAULT 0,
                last_reset TEXT,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS node_ext (
                node_id TEXT PRIMARY KEY,
                delay INTEGER DEFAULT -1,
                speed REAL DEFAULT 0,
                ip_info TEXT,
                sort_order INTEGER DEFAULT 0,
                message TEXT,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS dns_config (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                remote_dns TEXT,
                local_dns TEXT,
                direct_dns TEXT,
                enable_fake_dns INTEGER DEFAULT 0,
                dns_rules TEXT,
                is_default INTEGER DEFAULT 0,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS config_templates (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                description TEXT,
                config_data TEXT NOT NULL,
                is_default INTEGER DEFAULT 0,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );

            CREATE INDEX IF NOT EXISTS idx_tunnels_protocol ON tunnels(protocol);
            CREATE INDEX IF NOT EXISTS idx_tunnels_sort ON tunnels(sort_order);
            CREATE INDEX IF NOT EXISTS idx_node_stats_id ON node_stats(node_id);
            CREATE INDEX IF NOT EXISTS idx_node_ext_id ON node_ext(node_id);
        """)
        conn.commit()
        # 从 JSON 迁移数据到 SQLite（仅首次）
        migrate_json_to_db(conn)
    finally:
        conn.close()


def migrate_json_to_db(conn: sqlite3.Connection) -> None:
    """将现有 JSON 数据迁移到 SQLite（幂等：跳过已有数据）。"""
    # 迁移 tunnels
    count = conn.execute("SELECT COUNT(*) FROM tunnels").fetchone()[0]
    if count == 0 and os.path.exists(TUNNELS_FILE):
        try:
            with open(TUNNELS_FILE, "r", encoding="utf-8") as f:
                tunnels = json.load(f)
            for i, t in enumerate(tunnels):
                tid = t.get("id", f"migrated-{i}")
                conn.execute(
                    "INSERT OR IGNORE INTO tunnels (id, protocol, server_addr, server_port, uuid, password, out_tag, in_tag, data, sort_order) VALUES (?,?,?,?,?,?,?,?,?,?)",
                    (tid, t.get("protocol", ""), t.get("server_addr", ""), t.get("server_port", 443),
                     t.get("uuid", ""), t.get("password", ""), t.get("out_tag", ""), t.get("in_tag", ""),
                     json.dumps(t, ensure_ascii=False), i)
                )
        except Exception as e:
            logging.warning(f"[迁移] tunnels 迁移失败: {e}")
    # 迁移 subscriptions
    count = conn.execute("SELECT COUNT(*) FROM subscriptions").fetchone()[0]
    if count == 0 and os.path.exists(SUBSCRIPTIONS_FILE):
        try:
            with open(SUBSCRIPTIONS_FILE, "r", encoding="utf-8") as f:
                subs = json.load(f)
            for s in subs:
                sid = s.get("id", "")
                conn.execute(
                    "INSERT OR IGNORE INTO subscriptions (id, name, url, ua, enabled, interval_hours, last_update, last_count, filter_regex, subconverter_url, target_format, data) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
                    (sid, s.get("name", ""), s.get("url", ""), s.get("ua", ""),
                     1 if s.get("enabled") else 0, s.get("interval_hours", 0),
                     s.get("last_update", ""), s.get("last_count", 0),
                     s.get("filter_regex", ""), s.get("subconverter_url", ""),
                     s.get("target_format", "clash"), json.dumps(s, ensure_ascii=False))
                )
        except Exception as e:
            logging.warning(f"[迁移] subscriptions 迁移失败: {e}")
    # 迁移 profiles → node_ext
    count = conn.execute("SELECT COUNT(*) FROM node_ext").fetchone()[0]
    if count == 0 and os.path.exists(PROFILES_FILE):
        try:
            with open(PROFILES_FILE, "r", encoding="utf-8") as f:
                profiles = json.load(f)
            for node_id, ext in profiles.items():
                if isinstance(ext, dict):
                    conn.execute(
                        "INSERT OR IGNORE INTO node_ext (node_id, delay, speed, ip_info, sort_order, message) VALUES (?,?,?,?,?,?)",
                        (node_id, ext.get("delay", -1), ext.get("speed", 0),
                         json.dumps(ext.get("ip_info", {}), ensure_ascii=False),
                         ext.get("sort_order", 0), ext.get("message", ""))
                    )
        except Exception as e:
            logging.warning(f"[迁移] profiles 迁移失败: {e}")
    conn.commit()

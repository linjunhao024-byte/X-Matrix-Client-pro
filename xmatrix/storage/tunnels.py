"""
X-Matrix Client — 节点隧道持久化
"""
from __future__ import annotations

import json
import os
import threading
import time
from typing import TYPE_CHECKING

from xmatrix.constants import TUNNELS_FILE
from xmatrix.helpers import atomic_write_json

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def load_tunnels() -> list[dict]:
    """从 tunnels.json 加载节点列表，为缺少 id 的节点自动生成唯一ID。"""
    if os.path.exists(TUNNELS_FILE):
        try:
            with open(TUNNELS_FILE, "r", encoding="utf-8") as f:
                tunnels = json.load(f)
            # 为缺少 id 的节点自动生成唯一ID
            need_save = False
            existing_ids = {t.get("id") for t in tunnels if t.get("id")}
            for i, t in enumerate(tunnels):
                if not t.get("id"):
                    # 生成唯一ID
                    new_id = f"migrated-{i}"
                    counter = 0
                    while new_id in existing_ids:
                        counter += 1
                        new_id = f"migrated-{i}-{counter}"
                    t["id"] = new_id
                    existing_ids.add(new_id)
                    need_save = True
            if need_save:
                atomic_write_json(TUNNELS_FILE, tunnels)
            return tunnels
        except (json.JSONDecodeError, IOError):
            pass
    return []


def save_tunnels(tunnels: list[dict], lock: threading.Lock) -> None:
    """保存节点列表到 tunnels.json。"""
    with lock:
        atomic_write_json(TUNNELS_FILE, tunnels)


def get_tunnels(api: XMatrixAPI) -> list[dict]:
    """获取节点列表，为 policy_group 节点附加子节点摘要。"""
    id_map = {t.get("id"): t for t in api.tunnels}
    result = []
    for t in api.tunnels:
        if t.get("protocol") == "policy_group":
            t = dict(t)  # 浅拷贝，不污染原数据
            child_ids = t.get("child_ids", [])
            children = [id_map[cid] for cid in child_ids if cid in id_map]
            t["child_count"] = len(children)
            t["child_summary"] = ", ".join(c.get("out_tag", "?")[:15] for c in children[:5])
            if len(children) > 5:
                t["child_summary"] += f" +{len(children)-5}"
        result.append(t)
    return result

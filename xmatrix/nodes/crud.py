"""
X-Matrix Client — 节点 CRUD 操作
"""
from __future__ import annotations

import copy
import re
import time
from typing import Any, TYPE_CHECKING

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI

# 策略组有效策略列表
VALID_STRATEGIES = ("leastPing", "fallback", "random", "roundRobin", "leastLoad")

# 地区关键词模式
REGION_PATTERNS: list[tuple[list[str], str, str]] = [
    (["🇭🇰", "香港", "hong kong", "hk"], "🇭🇰 香港", "香港"),
    (["🇯🇵", "日本", "japan", "jp"], "🇯🇵 日本", "日本"),
    (["🇺🇸", "美国", "usa", "us ", " us"], "🇺🇸 美国", "美国"),
    (["🇸🇬", "新加坡", "singapore", "sg"], "🇸🇬 新加坡", "新加坡"),
    (["🇹🇼", "台湾", "taiwan", "tw"], "🇹🇼 台湾", "台湾"),
    (["🇰🇷", "韩国", "korea", "kr"], "🇰🇷 韩国", "韩国"),
    (["🇬🇧", "英国", "uk ", " uk", "united kingdom", "britain"], "🇬🇧 英国", "英国"),
    (["🇩🇪", "德国", "germany", "de "], "🇩🇪 德国", "德国"),
    (["🇫🇷", "法国", "france", "fr "], "🇫🇷 法国", "法国"),
    (["🇦🇺", "澳大利亚", "australia", "au "], "🇦🇺 澳大利亚", "澳大利亚"),
    (["🇨🇦", "加拿大", "canada", "ca "], "🇨🇦 加拿大", "加拿大"),
    (["🇮🇳", "印度", "india", "in "], "🇮🇳 印度", "印度"),
    (["🇷🇺", "俄罗斯", "russia", "ru "], "🇷🇺 俄罗斯", "俄罗斯"),
    (["🇳🇱", "荷兰", "netherlands", "nl "], "🇳🇱 荷兰", "荷兰"),
    (["🇧🇷", "巴西", "brazil", "br "], "🇧🇷 巴西", "巴西"),
    (["🇹🇭", "泰国", "thailand", "th "], "🇹🇭 泰国", "泰国"),
    (["🇲🇾", "马来西亚", "malaysia", "my "], "🇲🇾 马来西亚", "马来西亚"),
    (["🇵🇭", "菲律宾", "philippines", "ph "], "🇵🇭 菲律宾", "菲律宾"),
    (["🇮🇩", "印度尼西亚", "indonesia", "id "], "🇮🇩 印度尼西亚", "印度尼西亚"),
    (["🇹🇷", "土耳其", "turkey", "turkiye", "tr "], "🇹🇷 土耳其", "土耳其"),
    (["🇦🇷", "阿根廷", "argentina"], "🇦🇷 阿根廷", "阿根廷"),
    (["🇨🇱", "智利", "chile"], "🇨🇱 智利", "智利"),
    (["🇵🇱", "波兰", "poland", "pl "], "🇵🇱 波兰", "波兰"),
    (["🇺🇦", "乌克兰", "ukraine", "ua "], "🇺🇦 乌克兰", "乌克兰"),
]


def safe_port(val: Any, default: int = 443) -> int:
    """安全端口转换。"""
    try:
        p = int(val)
        return p if 1 <= p <= 65535 else default
    except (ValueError, TypeError):
        return default


def _next_tag(api: XMatrixAPI, prefix: str) -> str:
    """生成不与现有标签冲突的唯一 tag，如 in-1, in-2, ..."""
    existing = {t.get(f"{prefix}_tag", "") for t in api.tunnels}
    n = 1
    while f"{prefix}-{n}" in existing:
        n += 1
    return f"{prefix}-{n}"


def _check_group_cycle(api: XMatrixAPI, group_id: str, child_ids: list[str]) -> bool:
    """DFS 检测组环路：group_id → child_ids 中是否有间接引用回 group_id。返回 True 表示有环。"""
    # 构建 group_id → child_ids 映射
    group_children: dict[str, list[str]] = {}
    for t in api.tunnels:
        if t.get("protocol") == "policy_group":
            gid = t.get("id", "")
            if gid != group_id:
                group_children[gid] = t.get("child_ids", [])
    # 从 group_id 出发，看能否通过 child → group → child 链回到 group_id
    visited: set[str] = set()
    stack = list(child_ids)
    while stack:
        cid = stack.pop()
        if cid == group_id:
            return True  # 环路！
        if cid in visited:
            continue
        visited.add(cid)
        # 如果 cid 是一个组节点，展开其子节点
        if cid in group_children:
            stack.extend(group_children[cid])
    return False


def add_tunnel(api: XMatrixAPI, tunnel: dict) -> dict:
    """添加节点。"""
    if not tunnel.get("id"):
        tunnel["id"] = f"{int(time.time() * 1000)}-{len(api.tunnels)}"
    if not tunnel.get("in_tag"):
        tunnel["in_tag"] = _next_tag(api, "in")
    if not tunnel.get("out_tag"):
        tunnel["out_tag"] = _next_tag(api, "out")
    with api._tunnels_lock:
        api.tunnels.append(tunnel)
    api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels}


def delete_tunnel(api: XMatrixAPI, index: int) -> dict:
    """删除节点。"""
    if not (0 <= index < len(api.tunnels)):
        return {"success": False, "error": "索引越界"}
    with api._tunnels_lock:
        api.tunnels.pop(index)
    api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels}


def delete_tunnels_batch(api: XMatrixAPI, indices: list[int]) -> dict:
    """批量删除多个节点（倒序删除避免索引偏移）。"""
    if not indices:
        return {"success": False, "error": "未选择任何节点"}
    with api._tunnels_lock:
        for i in sorted(indices, reverse=True):
            if 0 <= i < len(api.tunnels):
                api.tunnels.pop(i)
    api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels}


def clone_tunnel(api: XMatrixAPI, index: int) -> dict:
    """深拷贝指定节点并追加 -clone 后缀到 out_tag。"""
    if not (0 <= index < len(api.tunnels)):
        return {"success": False, "error": "索引越界"}
    cloned = copy.deepcopy(api.tunnels[index])
    cloned["id"] = f"{int(time.time() * 1000)}-{len(api.tunnels)}"
    orig_tag = cloned.get("out_tag", "")
    cloned["out_tag"] = f"{orig_tag}-clone" if orig_tag else "clone"
    cloned["in_tag"] = _next_tag(api, "in")
    with api._tunnels_lock:
        api.tunnels.insert(index + 1, cloned)
    api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels, "cloned_index": index + 1}


def update_tunnel(api: XMatrixAPI, index: int, data: dict) -> dict:
    """更新节点。合并更新而非完全替换，保留原有字段如id。"""
    if not (0 <= index < len(api.tunnels)):
        return {"success": False, "error": "索引越界"}
    # 保留原有节点数据，合并新数据
    existing = api.tunnels[index]
    if not data.get("in_tag"):
        data["in_tag"] = existing.get("in_tag") or _next_tag(api, "in")
    if not data.get("out_tag"):
        data["out_tag"] = existing.get("out_tag") or _next_tag(api, "out")
    # 合并更新：保留原有字段，覆盖新字段
    merged = {**existing, **data}
    api.tunnels[index] = merged
    api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels}


def reorder_tunnels(api: XMatrixAPI, from_index: int, to_index: int) -> dict:
    """将节点从 from_index 移动到 to_index 位置，并同步维持运行中的活跃索引。"""
    if not (0 <= from_index < len(api.tunnels)) or not (0 <= to_index < len(api.tunnels)):
        return {"success": False, "error": "索引越界"}

    # 同步修正 active_index 防止内核重启时连错节点
    if api.active_index == from_index:
        api.active_index = to_index
    elif from_index < api.active_index <= to_index:
        api.active_index -= 1
    elif to_index <= api.active_index < from_index:
        api.active_index += 1

    with api._tunnels_lock:
        node = api.tunnels.pop(from_index)
        api.tunnels.insert(to_index, node)
    api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels}


def apply_tunnels_order(api: XMatrixAPI, original_indices: list[int]) -> dict:
    """根据前端传入的新索引顺序重排所有节点（用于一键延迟排序）。"""
    if len(original_indices) != len(api.tunnels):
        return {"success": False, "error": "数据流不匹配"}

    with api._tunnels_lock:
        new_tunnels = [api.tunnels[i] for i in original_indices]
    if 0 <= api.active_index < len(api.tunnels):
        try:
            api.active_index = original_indices.index(api.active_index)
        except ValueError:
            pass

    with api._tunnels_lock:
        api.tunnels = new_tunnels
    api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels}


def sort_tunnels(api: XMatrixAPI, sort_key: str = "delay", ascending: bool = True) -> dict:
    """多列排序节点列表。支持 14 个排序键。"""
    valid_keys = ("delay", "speed", "name", "addr", "port", "protocol", "network", "security",
                  "today_up", "today_down", "total_up", "total_down", "ip_info", "index")
    if sort_key not in valid_keys:
        return {"success": False, "error": f"无效排序键: {sort_key}，可选: {', '.join(valid_keys)}"}

    profiles = api._load_profiles()
    stats = api._load_stats()
    per_node = stats.get("per_node", {})

    def _sort_key(t: dict) -> tuple:
        nid = t.get("id", "")
        ext = profiles.get(nid, {})
        node_traffic = per_node.get(nid, {})

        if sort_key == "delay":
            val = ext.get("delay", -1)
            return (1 if val <= 0 else 0, val if ascending else -val)
        elif sort_key == "speed":
            val = ext.get("speed", 0)
            return (1 if val <= 0 else 0, val if ascending else -val)
        elif sort_key == "name":
            return (t.get("out_tag", "").lower(),)
        elif sort_key == "addr":
            return (t.get("server_addr", ""),)
        elif sort_key == "port":
            return (safe_port(t.get("server_port"), 0),)
        elif sort_key == "protocol":
            return (t.get("protocol", ""),)
        elif sort_key == "network":
            return (t.get("network", "tcp"),)
        elif sort_key == "security":
            return (t.get("security", "none"),)
        elif sort_key == "today_up":
            return (node_traffic.get("today_up", 0),)
        elif sort_key == "today_down":
            return (node_traffic.get("today_down", 0),)
        elif sort_key == "total_up":
            return (node_traffic.get("total_up", 0),)
        elif sort_key == "total_down":
            return (node_traffic.get("total_down", 0),)
        elif sort_key == "ip_info":
            return (ext.get("ip_info", {}).get("country", ""),)
        else:  # index
            return (0,)

    # 记录活跃节点 ID
    active_id = ""
    if 0 <= api.active_index < len(api.tunnels):
        active_id = api.tunnels[api.active_index].get("id", "")

    # 分离 policy_group 和普通节点
    with api._tunnels_lock:
        groups = [t for t in api.tunnels if t.get("protocol") == "policy_group"]
        nodes = [t for t in api.tunnels if t.get("protocol") != "policy_group"]
    nodes.sort(key=_sort_key, reverse=not ascending)
    with api._tunnels_lock:
        api.tunnels = groups + nodes

    # 恢复 active_index
    if active_id:
        for i, t in enumerate(api.tunnels):
            if t.get("id") == active_id:
                api.active_index = i
                break

    api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels, "sort_key": sort_key, "ascending": ascending}


def dedup_server_list(api: XMatrixAPI, keep_older: bool = True) -> dict:
    """根据 server_addr + server_port 去重。keep_older=True 保留先导入的，False 保留后导入的。"""
    before = len(api.tunnels)
    if before <= 1:
        return {"success": True, "tunnels": api.tunnels, "before": before, "after": before}

    seen: dict[str, int] = {}  # "addr:port" -> 第一次出现的索引
    remove_indices: set[int] = set()

    indices = range(before) if keep_older else range(before - 1, -1, -1)
    for i in indices:
        t = api.tunnels[i]
        key = f"{t.get('server_addr', '')}:{t.get('server_port', 443)}"
        if key in seen:
            # 重复节点：保留 seen 中记录的那个，标记当前为删除
            if keep_older:
                remove_indices.add(i)
            else:
                remove_indices.add(seen[key])
                seen[key] = i
        else:
            seen[key] = i

    if not remove_indices:
        return {"success": True, "tunnels": api.tunnels, "before": before, "after": before}

    # 调整 active_index
    sorted_remove = sorted(remove_indices)
    new_active = api.active_index
    for r in sorted_remove:
        if r < api.active_index:
            new_active -= 1
        elif r == api.active_index:
            new_active = -1  # 被删的正好是活跃节点，重置
            break
    api.active_index = max(new_active, -1)

    with api._tunnels_lock:
        api.tunnels = [t for i, t in enumerate(api.tunnels) if i not in remove_indices]
    api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels, "before": before, "after": len(api.tunnels)}


def remove_timeout_nodes(api: XMatrixAPI) -> dict:
    """遍历删除 delay == -1 的超时节点。"""
    before = len(api.tunnels)
    # 从 profiles.json 读取 delay 数据
    profiles = api._load_profiles()
    timeout_indices: set[int] = set()
    for i, t in enumerate(api.tunnels):
        if t.get("protocol") == "policy_group":
            continue
        node_id = t.get("id", "")
        ext = profiles.get(node_id, {})
        if ext.get("delay", -1) == -1:
            timeout_indices.add(i)
    if not timeout_indices:
        return {"success": True, "tunnels": api.tunnels, "removed": 0}
    # 调整 active_index
    sorted_remove = sorted(timeout_indices)
    new_active = api.active_index
    for r in sorted_remove:
        if r < api.active_index:
            new_active -= 1
        elif r == api.active_index:
            new_active = -1
            break
    api.active_index = max(new_active, -1)
    with api._tunnels_lock:
        api.tunnels = [t for i, t in enumerate(api.tunnels) if i not in timeout_indices]
    api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels, "removed": before - len(api.tunnels)}


def add_policy_group(api: XMatrixAPI, name: str, strategy: str, child_ids: list[str], filter_regex: str = "") -> dict:
    """创建 PolicyGroup 负载均衡组节点。"""
    if not name:
        return {"success": False, "error": "组名称不能为空"}
    if strategy not in VALID_STRATEGIES:
        return {"success": False, "error": f"无效策略: {strategy}，可选: {', '.join(VALID_STRATEGIES)}"}
    # 校验 child_ids 存在且非组节点
    valid_ids = {t.get("id") for t in api.tunnels if t.get("protocol") != "policy_group"}
    invalid = [cid for cid in child_ids if cid not in valid_ids]
    if invalid:
        return {"success": False, "error": f"子节点不存在或是组节点: {invalid[:3]}"}
    # 环路检测
    new_group_id = f"pg-{int(time.time()*1000)}"
    if _check_group_cycle(api, new_group_id, child_ids):
        return {"success": False, "error": "检测到组环路：子节点中包含引用当前组的节点"}

    group = {
        "protocol": "policy_group",
        "id": new_group_id,
        "in_tag": _next_tag(api, "in"),
        "out_tag": name,
        "group_strategy": strategy,
        "child_ids": child_ids,
        "filter": filter_regex,
    }
    with api._tunnels_lock:
        api.tunnels.append(group)
    api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels}


def update_policy_group(api: XMatrixAPI, group_id: str, name: str = "", strategy: str = "", child_ids: list[str] | None = None, filter_regex: str | None = None) -> dict:
    """更新已有 PolicyGroup 组节点的配置。"""
    group = next((t for t in api.tunnels if t.get("id") == group_id and t.get("protocol") == "policy_group"), None)
    if not group:
        return {"success": False, "error": "未找到该负载均衡组"}
    if name:
        group["out_tag"] = name
    if strategy:
        if strategy not in VALID_STRATEGIES:
            return {"success": False, "error": f"无效策略: {strategy}"}
        group["group_strategy"] = strategy
    if child_ids is not None:
        valid_ids = {t.get("id") for t in api.tunnels if t.get("protocol") != "policy_group"}
        invalid = [cid for cid in child_ids if cid not in valid_ids]
        if invalid:
            return {"success": False, "error": f"子节点不存在: {invalid[:3]}"}
        # 环路检测
        if _check_group_cycle(api, group_id, child_ids):
            return {"success": False, "error": "检测到组环路：子节点中包含引用当前组的节点"}
        group["child_ids"] = child_ids
    if filter_regex is not None:
        group["filter"] = filter_regex
    api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels}


def get_group_preview(api: XMatrixAPI, child_ids: list[str] | None = None, filter_regex: str = "") -> dict:
    """返回匹配的子节点列表预览（ID + 名称 + 协议 + 延迟）。"""
    candidates = [t for t in api.tunnels if t.get("protocol") != "policy_group"]
    if child_ids:
        id_set = set(child_ids)
        candidates = [t for t in candidates if t.get("id") in id_set]
    if filter_regex:
        try:
            pat = re.compile(filter_regex, re.IGNORECASE)
            candidates = [t for t in candidates if pat.search(t.get("out_tag", ""))]
        except re.error as e:
            return {"success": False, "error": f"正则表达式错误: {e}"}
    preview = [{
        "id": t.get("id", ""),
        "name": t.get("out_tag", ""),
        "protocol": t.get("protocol", ""),
        "addr": t.get("server_addr", ""),
    } for t in candidates]
    return {"success": True, "count": len(preview), "nodes": preview}


def auto_group_by_region(api: XMatrixAPI, strategy: str = "leastPing") -> dict:
    """根据节点名称中的地区关键词，自动创建多个 PolicyGroup。"""
    if strategy not in VALID_STRATEGIES:
        return {"success": False, "error": f"无效策略: {strategy}，可选: {', '.join(VALID_STRATEGIES)}"}

    # 仅收集普通节点（排除已有组节点）
    candidates = [(i, t) for i, t in enumerate(api.tunnels) if t.get("protocol") != "policy_group"]
    if not candidates:
        return {"success": False, "error": "没有可用的普通节点"}

    # 按地区分桶
    region_buckets: dict[str, list[str]] = {}  # region_suffix → [child_id, ...]
    for idx, t in candidates:
        name = (t.get("out_tag", "") + " " + t.get("server_addr", "")).lower()
        for patterns, display_name, suffix in REGION_PATTERNS:
            if any(p in name for p in patterns):
                region_buckets.setdefault(suffix, [])
                region_buckets[suffix].append(t.get("id", ""))
                break  # 每个节点只归入第一个匹配的地区

    if not region_buckets:
        return {"success": False, "error": "未从节点名称中识别到任何地区关键词"}

    # 为每个地区创建组（跳过已有同名组的地区）
    existing_names = {t.get("out_tag", "") for t in api.tunnels if t.get("protocol") == "policy_group"}
    created = []
    for suffix, child_ids in region_buckets.items():
        group_name = f"⚖ {suffix}"
        if group_name in existing_names:
            continue
        if len(child_ids) < 2:
            continue  # 少于 2 个节点不值得成组
        result = add_policy_group(api, group_name, strategy, child_ids)
        if result.get("success"):
            created.append({"name": group_name, "count": len(child_ids)})

    if not created:
        return {"success": False, "error": "所有地区节点数不足或组已存在"}
    return {"success": True, "created": created, "total": len(created)}


def set_proxy_chain(api: XMatrixAPI, node_id: str, via_id: str = "") -> dict:
    """为节点设置链式代理。via_id 为空则清除链式代理。"""
    node = next((t for t in api.tunnels if t.get("id") == node_id), None)
    if not node:
        return {"success": False, "error": "未找到该节点"}
    if node.get("protocol") == "policy_group":
        return {"success": False, "error": "组节点不支持链式代理"}
    if via_id:
        via = next((t for t in api.tunnels if t.get("id") == via_id), None)
        if not via:
            return {"success": False, "error": "未找到上游节点"}
        if via.get("protocol") == "policy_group":
            return {"success": False, "error": "上游节点不能是组节点"}
        if via_id == node_id:
            return {"success": False, "error": "不能将自己设为上游"}
        node["chain_id"] = via_id
    else:
        node.pop("chain_id", None)
    api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels}


def get_chain_preview(api: XMatrixAPI, node_id: str) -> dict:
    """返回节点的链式代理路径预览。"""
    chain = []
    visited = set()
    current_id = node_id
    while current_id and current_id not in visited:
        visited.add(current_id)
        node = next((t for t in api.tunnels if t.get("id") == current_id), None)
        if not node:
            break
        chain.append({"id": node.get("id", ""), "name": node.get("out_tag", ""), "protocol": node.get("protocol", ""), "addr": node.get("server_addr", "")})
        current_id = node.get("chain_id", "")
    if len(chain) <= 1:
        return {"success": True, "chain": chain, "has_chain": False}
    return {"success": True, "chain": chain, "has_chain": True}

"""
X-Matrix Client — 订阅管理模块
"""
from __future__ import annotations

import base64
import json
import logging
import os
import re
import time
import traceback
import urllib.parse
import urllib.request
from datetime import datetime
from typing import TYPE_CHECKING

import yaml

from xmatrix.constants import DATA_DIR, SUBSCRIPTIONS_FILE, TUNNELS_FILE
from xmatrix.helpers import atomic_write_json, validate_url

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def _load_subscriptions() -> list[dict]:
    """加载订阅列表。"""
    if os.path.exists(SUBSCRIPTIONS_FILE):
        try:
            with open(SUBSCRIPTIONS_FILE, "r", encoding="utf-8") as f:
                return json.load(f)
        except (json.JSONDecodeError, IOError):
            pass
    return []


def _save_subscriptions(subs: list[dict]) -> None:
    """保存订阅列表。"""
    atomic_write_json(SUBSCRIPTIONS_FILE, subs)


def get_subscriptions(api: XMatrixAPI) -> dict:
    """获取订阅列表。"""
    subs = _load_subscriptions()
    # G-9 修复：将嵌套 traffic 字典平铺为前端期望的顶层字段
    for s in subs:
        t = s.pop("traffic", None) or {}
        s["traffic_upload"] = t.get("upload", 0)
        s["traffic_download"] = t.get("download", 0)
        s["traffic_total"] = t.get("total", 0)
        s["traffic_expire"] = t.get("expire", 0)
    return {"success": True, "subscriptions": subs}


def add_subscription(api: XMatrixAPI, name: str, url: str, ua: str = "", interval_hours: int = 0, filter_regex: str = "", subconverter_url: str = "", target_format: str = "clash", memo: str = "") -> dict:
    """添加订阅。"""
    if not url or not url.strip():
        return {"success": False, "error": "订阅链接不能为空"}
    subs = _load_subscriptions()
    sub = {
        "id": f"sub-{int(time.time()*1000)}",
        "name": name or url[:50],
        "url": url.strip(),
        "ua": ua or "ClashForAndroid/2.5.12",
        "interval_hours": max(0, int(interval_hours)),
        "filter_regex": filter_regex,
        "subconverter_url": subconverter_url,
        "target_format": target_format,
        "enabled": True,
        "memo": memo,
        "last_update": "",
        "last_count": 0,
    }
    subs.append(sub)
    _save_subscriptions(subs)
    return {"success": True, "subscription": sub, "subscriptions": subs}


def update_subscription(api: XMatrixAPI, sub_id: str, **kwargs) -> dict:
    """更新订阅。"""
    subs = _load_subscriptions()
    sub = next((s for s in subs if s.get("id") == sub_id), None)
    if not sub:
        return {"success": False, "error": "未找到该订阅"}
    for key in ("name", "url", "ua", "filter_regex", "enabled", "memo", "subconverter_url", "target_format"):
        if key in kwargs and kwargs[key] is not None:
            sub[key] = kwargs[key]
    if "interval_hours" in kwargs and kwargs["interval_hours"] >= 0:
        sub["interval_hours"] = int(kwargs["interval_hours"])
    _save_subscriptions(subs)
    return {"success": True, "subscription": sub, "subscriptions": subs}


def delete_subscription(api: XMatrixAPI, sub_id: str) -> dict:
    """删除订阅。"""
    subs = _load_subscriptions()
    before = len(subs)
    subs = [s for s in subs if s.get("id") != sub_id]
    if len(subs) == before:
        return {"success": False, "error": "未找到该订阅"}
    _save_subscriptions(subs)
    return {"success": True, "subscriptions": subs}


def refresh_subscription(api: XMatrixAPI, sub_id: str = "") -> dict:
    """刷新订阅。"""
    subs = _load_subscriptions()
    targets = [s for s in subs if s.get("id") == sub_id] if sub_id else [s for s in subs if s.get("enabled")]
    if not targets:
        return {"success": False, "error": "未找到目标订阅"}

    results = []
    for sub in targets:
        urls_raw = sub.get("url", "")
        if not urls_raw:
            results.append({"id": sub.get("id"), "success": False, "error": "URL 为空"})
            continue

        # 多 URL 支持：换行分隔
        urls = [u.strip() for u in urls_raw.replace("\r", "").split("\n") if u.strip()]
        ua = sub.get("ua", "ClashForAndroid/2.5.12") or "ClashForAndroid/2.5.12"
        subconverter = sub.get("subconverter_url", "")
        target_fmt = sub.get("target_format", "clash")
        total_count = 0
        any_success = False

        for url in urls:
            try:
                raw_text = _fetch_url(url, ua, subconverter, target_fmt)
                import_result = _parse_subscription_content(api, raw_text)
                if import_result and import_result.get("success"):
                    count = import_result.get("count", 0)
                    total_count += count
                    any_success = True
                else:
                    results.append({"id": sub.get("id"), "success": False, "error": "无法解析订阅内容", "url": url[:50]})
            except Exception as e:
                results.append({"id": sub.get("id"), "success": False, "error": f"拉取失败: {e}", "url": url[:50]})

        # 应用过滤正则
        if any_success and sub.get("filter_regex"):
            try:
                pat = re.compile(sub["filter_regex"], re.IGNORECASE)
                api.tunnels = [t for t in api.tunnels if t.get("protocol") == "policy_group" or pat.search(t.get("out_tag", ""))]
                api._save_tunnels()
            except re.error:
                pass

        # 更新元数据
        sub["last_update"] = time.strftime("%Y-%m-%d %H:%M:%S")
        sub["last_count"] = total_count
        results.append({"id": sub.get("id"), "success": any_success, "count": total_count})

    _save_subscriptions(subs)
    if any(r.get("success") for r in results):
        api.dedup_server_list(True)

    return {"success": True, "results": results, "tunnels": api.tunnels}


def _fetch_url(url: str, ua: str = "ClashForAndroid/2.5.12", subconverter_url: str = "", target_format: str = "clash") -> str:
    """拉取 URL 内容。"""
    if subconverter_url and "://" in url:
        converter = subconverter_url.rstrip("/")
        encoded_url = urllib.parse.quote(url, safe="")
        url = f"{converter}/sub?target={target_format}&url={encoded_url}&config={converter}/subconverter/config"
    req = urllib.request.Request(url, headers={"User-Agent": ua, "Accept": "*/*"})
    with urllib.request.urlopen(req, timeout=15) as resp:
        return resp.read().decode("utf-8", errors="ignore")


def _parse_subscription_content(api: XMatrixAPI, raw_text: str) -> dict | None:
    """尝试解析订阅内容为节点列表。"""
    # Clash YAML
    try:
        data = yaml.safe_load(raw_text)
        if isinstance(data, dict) and "proxies" in data:
            result = _parse_clash_yaml(api, data["proxies"])
            if result.get("success"):
                return result
    except Exception:
        pass
    # Base64
    try:
        cleaned = raw_text.strip().replace("\n", "").replace("\r", "")
        for decoder in [base64.b64decode, base64.urlsafe_b64decode]:
            try:
                decoded = decoder(cleaned).decode("utf-8", errors="ignore")
                if "://" in decoded:
                    result = api.import_uri(decoded)
                    if result.get("success"):
                        return result
            except Exception:
                continue
    except Exception:
        pass
    # 纯文本 URI
    if "://" in raw_text:
        result = api.import_uri(raw_text)
        if result.get("success"):
            return result
    return None


def _parse_clash_yaml(api: XMatrixAPI, proxies: list) -> dict:
    """解析 Clash YAML 的 proxies 列表。"""
    from xmatrix.nodes.parser import parse_clash_proxies
    results = parse_clash_proxies(proxies, api._next_tag)
    if not results:
        return {"success": False, "error": "订阅中未找到支持的节点"}

    # 备份旧数据
    if api.tunnels:
        ts = datetime.now().strftime("%Y%m%d_%H%M%S")
        backup = os.path.join(DATA_DIR, f"tunnels.json.bak_sub_{ts}")
        try:
            with open(TUNNELS_FILE, "r", encoding="utf-8") as f:
                with open(backup, "w", encoding="utf-8") as bf:
                    bf.write(f.read())
        except Exception:
            pass

    with api._tunnels_lock:
        api.tunnels.extend(results)
    api._save_tunnels()
    return {"success": True, "tunnels": api.tunnels, "count": len(results)}


def subscription_scheduler(api: XMatrixAPI) -> None:
    """后台定时任务调度器。"""
    last_geo_update: float = 0
    last_save: float = time.time()
    last_cleanup: float = time.time()

    while True:
        time.sleep(60)
        try:
            # 订阅自动刷新
            subs = _load_subscriptions()
            now = time.time()
            for sub in subs:
                if not sub.get("enabled") or not sub.get("interval_hours"):
                    continue
                interval_sec = sub["interval_hours"] * 3600
                last = sub.get("last_update", "")
                if last:
                    try:
                        last_ts = time.mktime(time.strptime(last, "%Y-%m-%d %H:%M:%S"))
                    except ValueError:
                        last_ts = 0
                else:
                    last_ts = 0
                if now - last_ts >= interval_sec:
                    api.log_queue.put(f"[订阅] 自动刷新: {sub.get('name', sub.get('url', '')[:30])}\n")
                    refresh_subscription(api, sub.get("id", ""))

            # Geo 文件自动更新
            if now - last_geo_update >= 86400:
                try:
                    api.auto_update_geo()
                    last_geo_update = now
                except Exception as e:
                    logging.warning(f"[调度器] Geo 文件自动更新失败: {e}")

            # 定期自动保存
            if now - last_save >= 1200:
                api._save_tunnels()
                last_save = now

            # 过期清理
            if now - last_cleanup >= 3600:
                api._cleanup_temp_files()
                last_cleanup = now

        except Exception as e:
            logging.warning(f"[调度器] 后台任务异常: {e}")
            logging.debug(traceback.format_exc())

"""
X-Matrix Client — Geo 数据更新
"""
from __future__ import annotations

import json
import os
import time
from typing import TYPE_CHECKING

from xmatrix.constants import DATA_DIR, GEO_SOURCES

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def update_geo_data(api: XMatrixAPI, source: str = "loyalsoldier") -> dict:
    """更新 Geo 数据。"""
    custom_urls = {}
    try:
        cfg_path = os.path.join(DATA_DIR, "config.json")
        if os.path.isfile(cfg_path):
            with open(cfg_path, "r", encoding="utf-8") as f:
                custom_urls = json.load(f).get("geo_source_urls", {})
    except Exception:
        pass

    sources_to_update = []
    if source == "all":
        sources_to_update = list(GEO_SOURCES.keys())
    elif source in GEO_SOURCES:
        sources_to_update = [source]
    else:
        return {"success": False, "error": f"未知数据源: {source}，可选: {', '.join(GEO_SOURCES.keys())} / all"}

    file_urls: dict[str, list[str]] = {}
    for src_key in sources_to_update:
        src = GEO_SOURCES[src_key]
        for filename, default_url in src["files"].items():
            if filename not in file_urls:
                file_urls[filename] = []
            custom_url = custom_urls.get(filename, "")
            if custom_url:
                file_urls[filename].append(custom_url)
            file_urls[filename].append(default_url)

    results: list[str] = []
    for filename, urls in file_urls.items():
        seen = set()
        unique_urls = []
        for u in urls:
            if u not in seen:
                seen.add(u)
                unique_urls.append(u)
        dest = os.path.join(DATA_DIR, filename)
        dl = api._download_file_multi_source(unique_urls, dest, retries=2, timeout=60)
        if dl["success"]:
            results.append(f"{filename} ✅ ({dl['size_bytes']//1024}KB)")
        else:
            results.append(f"{filename} ❌ {dl['error']}")

    return {"success": True, "source": source, "message": "; ".join(results)}


def get_geo_status() -> dict:
    """获取 Geo 数据状态。"""
    geo_files = [
        "geoip.dat", "geosite.dat", "geoip.db", "geosite.db", "country.mmdb",
        "geosite-cn.srs", "geoip-cn.srs", "geosite-geolocation-!cn.srs",
        "geosite-cn.yaml", "geoip-cn.yaml",
    ]
    status: list[dict] = []
    for fn in geo_files:
        path = os.path.join(DATA_DIR, fn)
        if os.path.isfile(path):
            stat = os.stat(path)
            age_days = (time.time() - stat.st_mtime) / 86400
            if age_days < 7:
                file_status = "最新"
            else:
                file_status = "可更新"
            status.append({
                "name": fn, "exists": True, "size_kb": round(stat.st_size / 1024),
                "modified": time.strftime("%Y-%m-%d %H:%M", time.localtime(stat.st_mtime)),
                "age_days": round(age_days, 1), "status": file_status,
            })
        else:
            status.append({"name": fn, "exists": False, "size_kb": 0, "modified": "", "age_days": -1, "status": "缺失"})
    return {"success": True, "files": status}


def get_geo_presets() -> dict:
    """获取 Geo 预设。"""
    presets: list[dict] = [
        {
            "name": "🇨🇳 中国大陆直连",
            "description": "国内域名和 IP 直连，其余走代理",
            "rules": [
                {"type": "geosite", "content": "cn", "outbound": "direct", "enabled": True},
                {"type": "geoip", "content": "cn", "outbound": "direct", "enabled": True},
            ],
        },
        {
            "name": "📢 广告拦截",
            "description": "屏蔽常见广告域名",
            "rules": [
                {"type": "domain", "content": "ads.google.com,pagead2.googlesad,syndication.twitter.com,analytics.tiktok.com", "outbound": "block", "enabled": True},
                {"type": "geosite", "content": "category-ads-all", "outbound": "block", "enabled": True},
            ],
        },
        {
            "name": "🎬 流媒体解锁",
            "description": "流媒体流量走代理",
            "rules": [
                {"type": "domain", "content": "netflix.com,nflxvideo.net,dssott.com,disneyplus.com,hulu.com,hbo.com,spotify.com,youtube.com,googlevideo.com", "outbound": "proxy", "enabled": True},
            ],
        },
    ]
    return {"success": True, "presets": presets}


def auto_update_geo(api: XMatrixAPI) -> None:
    """自动更新 Geo 数据。"""
    for source in ("loyalsoldier", "srs", "mihomo_yaml"):
        try:
            src = GEO_SOURCES.get(source, {})
            for fname, url in src.get("files", {}).items():
                dest = os.path.join(DATA_DIR, fname)
                if os.path.isfile(dest):
                    age_days = (time.time() - os.path.getmtime(dest)) / 86400
                    if age_days < 7:
                        continue
                api._download_file(url, dest)
        except Exception:
            pass

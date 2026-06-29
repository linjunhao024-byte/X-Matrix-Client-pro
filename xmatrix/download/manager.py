"""
X-Matrix Client — 下载管理
"""
from __future__ import annotations

import io
import json
import logging
import os
import time
import zipfile
from typing import TYPE_CHECKING

from xmatrix.constants import CORE_REGISTRY, DATA_DIR, _BASE

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def urlopen(api: XMatrixAPI, url: str, timeout: int = 30, headers: dict | None = None):
    """打开 URL，依次尝试本地代理端口。"""
    import urllib.request
    hdrs = {"User-Agent": "X-Matrix/1.0"}
    if headers:
        hdrs.update(headers)

    ports = [api.config_local_port]
    for p in range(10808, 10821):
        if p not in ports:
            ports.append(p)

    def _open_with_port(port: int):
        proxy_handler = urllib.request.ProxyHandler({
            "http": f"http://127.0.0.1:{port}",
            "https": f"http://127.0.0.1:{port}",
        })
        opener = urllib.request.build_opener(proxy_handler)
        req = urllib.request.Request(url, headers=hdrs)
        return opener.open(req, timeout=timeout)

    def _open_direct():
        opener = urllib.request.build_opener()
        req = urllib.request.Request(url, headers=hdrs)
        return opener.open(req, timeout=timeout)

    last_err = None
    for port in ports:
        try:
            return _open_with_port(port)
        except Exception as e:
            last_err = e
            err = str(e).lower()
            if "10061" in err or "refused" in err or "10060" in err or "timed out" in err:
                continue
            raise
    return _open_direct()


def download_file(api: XMatrixAPI, url: str, dest: str, retries: int = 3, timeout: int = 120) -> dict:
    """下载文件。"""
    filename = os.path.basename(dest)
    last_err = None
    for attempt in range(retries):
        try:
            api.download_progress = {
                "active": True, "percent": 0, "total": 0, "downloaded": 0,
                "message": f"下载 {filename}...", "done": False, "paused": False,
                "speed_bytes_per_sec": 0, "eta_seconds": 0,
            }
            os.makedirs(os.path.dirname(dest) or ".", exist_ok=True)
            existing_size = 0
            if os.path.exists(dest):
                existing_size = os.path.getsize(dest)
            headers = {}
            if existing_size > 0:
                headers["Range"] = f"bytes={existing_size}-"
            with urlopen(api, url, timeout=timeout, headers=headers if headers else None) as resp:
                if resp.status == 206:
                    downloaded = existing_size
                    total = existing_size + int(resp.headers.get("Content-Length", 0))
                    mode = "ab"
                else:
                    downloaded = 0
                    total = int(resp.headers.get("Content-Length", 0))
                    mode = "wb"
                    existing_size = 0
                api.download_progress["total"] = total
                api.download_progress["downloaded"] = downloaded
                speed_window: list[tuple[float, int]] = []
                with open(dest, mode) as f:
                    while True:
                        while api.download_progress.get("paused", False):
                            time.sleep(0.5)
                        chunk = resp.read(65536)
                        if not chunk:
                            break
                        f.write(chunk)
                        downloaded += len(chunk)
                        now = time.time()
                        speed_window.append((now, len(chunk)))
                        cutoff = now - 5
                        speed_window = [(t, b) for t, b in speed_window if t >= cutoff]
                        if len(speed_window) >= 2:
                            window_bytes = sum(b for _, b in speed_window)
                            window_time = speed_window[-1][0] - speed_window[0][0]
                            speed = int(window_bytes / window_time) if window_time > 0 else 0
                        else:
                            speed = 0
                        api.download_progress["downloaded"] = downloaded
                        api.download_progress["percent"] = int(downloaded * 100 / total) if total else 0
                        api.download_progress["speed_bytes_per_sec"] = speed
                        api.download_progress["eta_seconds"] = int((total - downloaded) / speed) if speed > 0 and total > 0 else 0
            api.download_progress = {
                "active": False, "percent": 100, "total": downloaded, "downloaded": downloaded,
                "message": f"{filename} 完成", "done": True, "paused": False,
                "speed_bytes_per_sec": 0, "eta_seconds": 0,
            }
            return {"success": True, "size_bytes": downloaded, "path": dest}
        except Exception as e:
            last_err = e
            if attempt < retries - 1:
                time.sleep(2 ** attempt)
    api.download_progress = {
        "active": False, "percent": 0, "total": 0, "downloaded": 0,
        "message": f"下载失败: {last_err}", "done": True, "paused": False,
        "speed_bytes_per_sec": 0, "eta_seconds": 0,
    }
    return {"success": False, "error": str(last_err)}


def download_file_multi_source(api: XMatrixAPI, urls: list[str], dest: str, retries: int = 2, timeout: int = 120) -> dict:
    """多源下载。"""
    if not urls:
        return {"success": False, "error": "无下载源"}
    last_err = None
    for i, url in enumerate(urls):
        try:
            api.download_progress["message"] = f"尝试源 {i+1}/{len(urls)}: {os.path.basename(dest)}..."
            result = download_file(api, url, dest, retries=retries, timeout=timeout)
            if result["success"]:
                return result
            last_err = result.get("error", "未知错误")
        except Exception as e:
            last_err = str(e)
    return {"success": False, "error": f"所有源均失败: {last_err}"}


def pause_download(api: XMatrixAPI) -> dict:
    """暂停下载。"""
    if not api.download_progress.get("active", False):
        return {"success": False, "error": "没有正在进行的下载"}
    api.download_progress["paused"] = True
    api.download_progress["message"] = "下载已暂停"
    return {"success": True}


def resume_download(api: XMatrixAPI) -> dict:
    """恢复下载。"""
    if not api.download_progress.get("paused", False):
        return {"success": False, "error": "下载未暂停"}
    api.download_progress["paused"] = False
    api.download_progress["message"] = "下载中..."
    return {"success": True}


def download_core(api: XMatrixAPI, core_type: str) -> dict:
    """下载核心。"""
    info = CORE_REGISTRY.get(core_type)
    if not info:
        return {"success": False, "error": f"未知核心类型: {core_type}"}

    repo = info["github_repo"]
    try:
        with urlopen(
            api,
            f"https://api.github.com/repos/{repo}/releases/latest",
            timeout=15,
            headers={"Accept": "application/vnd.github.v3+json"},
        ) as resp:
            release = json.loads(resp.read().decode("utf-8"))
    except Exception as e:
        return {"success": False, "error": f"获取版本信息失败: {e}"}

    tag = release.get("tag_name", "unknown")
    assets = release.get("assets", [])

    download_urls: list[str] = []
    release_match = info.get("release_match")
    for asset in assets:
        name = asset.get("name", "")
        name_lower = name.lower()
        if release_match and release_match(name):
            download_urls.append(asset.get("browser_download_url", ""))
        elif not release_match and "windows" in name_lower and "64" in name_lower and name_lower.endswith(".zip"):
            download_urls.append(asset.get("browser_download_url", ""))

    if not download_urls:
        return {"success": False, "error": f"未找到 {info['name']} Windows 64 位包"}

    temp_dest = os.path.join(DATA_DIR, f"core_download_{core_type}.tmp")
    try:
        dl = download_file_multi_source(api, download_urls, temp_dest, retries=2, timeout=120)
        if not dl["success"]:
            return {"success": False, "error": f"下载失败: {dl.get('error', '未知错误')}"}

        with open(temp_dest, "rb") as f:
            file_data = f.read()

        target = os.path.join(_BASE, info["exe_names"][0])

        is_zip = download_urls[0].lower().endswith(".zip")

        if not is_zip:
            try:
                with open(target, "wb") as f:
                    f.write(file_data)
                return {"success": True, "tag": tag, "message": f"{info['name']} 已更新至 {tag}"}
            except Exception as e:
                return {"success": False, "error": f"写入失败: {e}"}

        try:
            with zipfile.ZipFile(io.BytesIO(file_data)) as zf:
                matched_entry = None
                for entry in zf.namelist():
                    basename = os.path.basename(entry).lower()
                    if any(basename == exe_name.lower() for exe_name in info["exe_names"]):
                        matched_entry = entry
                        break
                if not matched_entry:
                    for entry in zf.namelist():
                        if entry.lower().endswith(".exe") and os.path.basename(entry):
                            matched_entry = entry
                            break
                if matched_entry:
                    with zf.open(matched_entry) as src:
                        with open(target, "wb") as dst:
                            dst.write(src.read())
                    return {"success": True, "tag": tag, "message": f"{info['name']} 已更新至 {tag}"}
            return {"success": False, "error": f"ZIP 中未找到 {info['name']} 可执行文件"}
        except Exception as e:
            return {"success": False, "error": f"解压失败: {e}"}
    finally:
        if os.path.exists(temp_dest):
            try:
                os.remove(temp_dest)
            except Exception:
                pass

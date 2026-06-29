"""
X-Matrix Client — 核心注册表管理
"""
from __future__ import annotations

import json
import logging
import os
import shutil
import zipfile
import io
import hashlib
import re
from typing import TYPE_CHECKING

from xmatrix.constants import CORE_REGISTRY, DATA_DIR, _BASE
from xmatrix.helpers import parse_version
from xmatrix.process import run_hidden
from xmatrix.core.lifecycle import find_core_exe, get_core_version

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def get_core_types(api: XMatrixAPI) -> dict:
    """返回所有可用核心类型及其状态。"""
    cores = []
    for core_type, info in CORE_REGISTRY.items():
        exe_path = find_core_exe(core_type)
        cores.append({
            "type": core_type,
            "name": info["name"],
            "available": exe_path is not None,
            "active": api.active_core == core_type,
            "exe_path": exe_path or "",
        })
    return {"success": True, "cores": cores, "active": api.active_core}


def set_active_core(api: XMatrixAPI, core_type: str) -> dict:
    """切换当前激活的核心类型。"""
    if core_type not in CORE_REGISTRY:
        return {"success": False, "error": f"未知核心类型: {core_type}"}
    exe_path = find_core_exe(core_type)
    if not exe_path:
        return {"success": False, "error": f"{CORE_REGISTRY[core_type]['name']} 未安装，请先下载"}
    api.active_core = core_type
    return {"success": True, "active": core_type}


def check_core_update(api: XMatrixAPI, check_prerelease: bool = False) -> dict:
    """检查所有核心的当前版本和 GitHub 最新版本。"""
    if not check_prerelease:
        try:
            cfg_path = os.path.join(DATA_DIR, "config.json")
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    check_prerelease = json.load(f).get("check_prerelease", False)
        except Exception:
            pass

    results = []
    for core_type, info in CORE_REGISTRY.items():
        current = get_core_version(core_type)
        latest = "unknown"
        is_prerelease = False
        repo = info.get("github_repo", "")
        if repo:
            try:
                if check_prerelease:
                    with api._urlopen(
                        f"https://api.github.com/repos/{repo}/releases",
                        timeout=10,
                        headers={"Accept": "application/vnd.github.v3+json"},
                    ) as resp:
                        releases = json.loads(resp.read().decode("utf-8"))
                        if releases and isinstance(releases, list):
                            latest = releases[0].get("tag_name", "unknown")
                            is_prerelease = releases[0].get("prerelease", False)
                else:
                    with api._urlopen(
                        f"https://api.github.com/repos/{repo}/releases/latest",
                        timeout=10,
                        headers={"Accept": "application/vnd.github.v3+json"},
                    ) as resp:
                        release = json.loads(resp.read().decode("utf-8"))
                        latest = release.get("tag_name", "unknown")
            except Exception:
                latest = "check_failed"
        has_update = False
        if current not in ("unknown", "not_installed", "check_failed") and latest not in ("unknown", "check_failed"):
            has_update = parse_version(latest) > parse_version(current)
        results.append({
            "type": core_type,
            "name": info["name"],
            "current": current,
            "latest": latest,
            "has_update": has_update,
            "available": current != "not_installed",
            "active": api.active_core == core_type,
            "is_prerelease": is_prerelease,
        })
    return {"success": True, "cores": results, "check_prerelease": check_prerelease}


def update_core(api: XMatrixAPI) -> dict:
    """从 GitHub 下载最新 Xray-core 并热替换 xmatrix-core.exe。"""
    with api._process_lock:
        was_running = api.xray_process is not None and api.xray_process.poll() is None

    try:
        with api._urlopen(
            "https://api.github.com/repos/XTLS/Xray-core/releases/latest",
            timeout=15,
            headers={"Accept": "application/vnd.github.v3+json"},
        ) as resp:
            release = json.loads(resp.read().decode("utf-8"))
    except Exception as e:
        return {"success": False, "error": f"获取版本信息失败: {str(e)}"}

    tag_name = release.get("tag_name", "unknown")
    assets = release.get("assets", [])

    download_url = None
    dgst_url = None
    for asset in assets:
        name = asset.get("name", "").lower()
        if "windows-64" in name and "32" not in name:
            if name.endswith(".zip"):
                download_url = asset.get("browser_download_url")
            elif name.endswith(".zip.dgst"):
                dgst_url = asset.get("browser_download_url")

    if not download_url:
        return {"success": False, "error": "未找到 Windows 64 位核心包"}

    try:
        with api._urlopen(download_url, timeout=120) as resp:
            zip_data = io.BytesIO(resp.read())
    except Exception as e:
        return {"success": False, "error": f"核心下载失败: {str(e)}"}

    if dgst_url:
        try:
            with api._urlopen(dgst_url, timeout=15) as resp:
                dgst_text = resp.read().decode("utf-8")
            match = re.search(r'([a-fA-F0-9]{64})', dgst_text)
            if match:
                expected_hash = match.group(1).lower()
                actual_hash = hashlib.sha256(zip_data.getvalue()).hexdigest().lower()
                if expected_hash != actual_hash:
                    return {
                        "success": False,
                        "error": f"安全拦截：核心签名校验失败，文件可能被篡改！\n预期: {expected_hash[:16]}...\n实际: {actual_hash[:16]}..."
                    }
        except Exception as e:
            return {"success": False, "error": f"获取官方安全签名失败，为确保安全已终止更新: {str(e)}"}

    if was_running:
        api.stop_core()
        import time
        time.sleep(1)

    try:
        with zipfile.ZipFile(zip_data) as zf:
            xray_entry = None
            for name in zf.namelist():
                if name.endswith("xray.exe") and "/" not in name:
                    xray_entry = name
                    break
            if not xray_entry:
                for name in zf.namelist():
                    if name.endswith("xray.exe"):
                        xray_entry = name
                        break
            if not xray_entry:
                return {"success": False, "error": "ZIP 中未找到 xray.exe"}

            target_path = os.path.join(_BASE, "xmatrix-core.exe")
            with zf.open(xray_entry) as src:
                with open(target_path, "wb") as dst:
                    dst.write(src.read())
    except Exception as e:
        return {"success": False, "error": f"解压失败: {str(e)}"}

    if was_running:
        import time
        time.sleep(0.5)
        api.start_core()

    return {"success": True, "tag_name": tag_name, "message": f"核心已更新至 {tag_name}"}

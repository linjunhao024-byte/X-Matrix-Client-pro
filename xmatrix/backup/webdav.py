"""
X-Matrix Client — WebDAV 备份恢复
"""
from __future__ import annotations

import io
import json
import os
import shutil
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from typing import TYPE_CHECKING

from xmatrix.constants import DATA_DIR

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def webdav_request(method: str, url: str, body: bytes | None = None, timeout: int = 30) -> tuple[int, bytes]:
    """发送 WebDAV 请求。"""
    req = urllib.request.Request(url, data=body, method=method)
    req.add_header("User-Agent", "X-Matrix-WebDAV/1.0")
    if body is not None:
        req.add_header("Content-Type", "application/octet-stream")
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return resp.status, resp.read()


def webdav_test(url: str) -> dict:
    """测试 WebDAV 连接。"""
    if not url:
        return {"success": False, "error": "WebDAV 地址不能为空"}
    try:
        parsed = urllib.parse.urlparse(url.strip())
        if parsed.scheme not in ("http", "https"):
            return {"success": False, "error": f"不支持的协议: {parsed.scheme}，仅允许 http/https"}
        if not (parsed.hostname or ""):
            return {"success": False, "error": "URL 缺少主机名"}
    except Exception:
        return {"success": False, "error": "URL 格式无效"}
    try:
        req = urllib.request.Request(url, method="OPTIONS")
        req.add_header("User-Agent", "X-Matrix-WebDAV/1.0")
        with urllib.request.urlopen(req, timeout=10) as resp:
            dav_header = resp.getheader("DAV", "")
            return {"success": True, "status": resp.status, "dav": dav_header or "unknown"}
    except urllib.error.HTTPError as e:
        if e.code in (401, 403):
            return {"success": True, "status": e.code, "dav": "auth_required"}
        return {"success": False, "error": f"HTTP {e.code}: {e.reason}"}
    except Exception as e:
        return {"success": False, "error": str(e)}


def webdav_backup(api: XMatrixAPI, url: str, remote_path: str = "xmatrix-backup.zip") -> dict:
    """WebDAV 备份。"""
    if not url:
        return {"success": False, "error": "WebDAV 地址不能为空"}
    base_url = url.rstrip("/")
    full_url = f"{base_url}/{remote_path.lstrip('/')}"

    zip_buf = io.BytesIO()
    try:
        with zipfile.ZipFile(zip_buf, "w", zipfile.ZIP_DEFLATED) as zf:
            for root, dirs, files in os.walk(DATA_DIR):
                for fn in files:
                    full = os.path.join(root, fn)
                    arcname = os.path.relpath(full, DATA_DIR)
                    zf.write(full, arcname)
    except Exception as e:
        return {"success": False, "error": f"打包失败: {e}"}

    parent_url = full_url.rsplit("/", 1)[0] + "/"
    try:
        webdav_request("MKCOL", parent_url)
    except Exception:
        pass

    try:
        webdav_request("PUT", full_url, body=zip_buf.getvalue(), timeout=60)
        return {"success": True, "url": full_url, "size_kb": round(zip_buf.tell() / 1024, 1)}
    except Exception as e:
        return {"success": False, "error": f"上传失败: {e}"}


def webdav_restore(api: XMatrixAPI, url: str, remote_path: str = "xmatrix-backup.zip") -> dict:
    """WebDAV 恢复。"""
    if not url:
        return {"success": False, "error": "WebDAV 地址不能为空"}
    base_url = url.rstrip("/")
    full_url = f"{base_url}/{remote_path.lstrip('/')}"

    try:
        status, zip_data = webdav_request("GET", full_url, timeout=60)
        if not zip_data:
            return {"success": False, "error": "下载的文件为空"}
    except Exception as e:
        return {"success": False, "error": f"下载失败: {e}"}

    try:
        backup_dir = DATA_DIR + "_backup_" + time.strftime("%Y%m%d%H%M%S")
        if os.path.exists(DATA_DIR):
            shutil.copytree(DATA_DIR, backup_dir)
    except Exception as e:
        return {"success": False, "error": f"备份当前数据失败: {e}"}

    try:
        with zipfile.ZipFile(io.BytesIO(zip_data)) as zf:
            for member in zf.namelist():
                member_path = os.path.realpath(os.path.join(DATA_DIR, member))
                if not member_path.startswith(os.path.realpath(DATA_DIR) + os.sep) and member_path != os.path.realpath(DATA_DIR):
                    continue
                zf.extract(member, DATA_DIR)
        api.tunnels = api._load_tunnels()
        api.stats_offset = api._load_stats()
        return {"success": True, "message": f"已从 WebDAV 恢复，旧数据备份至 {backup_dir}"}
    except Exception as e:
        return {"success": False, "error": f"解压恢复失败: {e}"}


def export_backup(api: XMatrixAPI) -> dict:
    """导出备份。"""
    import webview
    file_path = api._pick_file(webview.SAVE_DIALOG, save_filename="xmatrix-backup.zip",
                                file_types=("ZIP 文件 (*.zip)",))
    if not file_path:
        return {"success": False, "error": "用户取消"}
    if not file_path.lower().endswith(".zip"):
        file_path += ".zip"
    try:
        with zipfile.ZipFile(file_path, "w", zipfile.ZIP_DEFLATED) as zf:
            for root, dirs, files in os.walk(DATA_DIR):
                for fn in files:
                    full = os.path.join(root, fn)
                    arcname = os.path.relpath(full, DATA_DIR)
                    zf.write(full, arcname)
        return {"success": True, "path": file_path}
    except Exception as e:
        return {"success": False, "error": f"备份失败: {str(e)}"}


def import_backup(api: XMatrixAPI) -> dict:
    """导入备份。"""
    import webview
    file_path = api._pick_file(webview.OPEN_DIALOG, allow_multiple=False,
                                file_types=("ZIP 文件 (*.zip)",))
    if not file_path:
        return {"success": False, "error": "用户取消"}
    try:
        backup_dir = DATA_DIR + "_backup_" + time.strftime("%Y%m%d%H%M%S")
        if os.path.exists(DATA_DIR):
            shutil.copytree(DATA_DIR, backup_dir)
        with zipfile.ZipFile(file_path, "r") as zf:
            for member in zf.namelist():
                member_path = os.path.realpath(os.path.join(DATA_DIR, member))
                if not member_path.startswith(os.path.realpath(DATA_DIR) + os.sep) and member_path != os.path.realpath(DATA_DIR):
                    continue
                zf.extract(member, DATA_DIR)
        api.tunnels = api._load_tunnels()
        api.stats_offset = api._load_stats()
        return {"success": True, "message": f"已恢复备份，旧数据已备份至 {backup_dir}"}
    except Exception as e:
        return {"success": False, "error": f"恢复失败: {str(e)}"}

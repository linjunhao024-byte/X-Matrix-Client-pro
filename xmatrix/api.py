"""
X-Matrix Client — XMatrixAPI 薄委托层
所有公开方法委托到各子模块，保持 window.pywebview.api.* 签名不变。
"""
from __future__ import annotations

import json
import logging
import os
import queue
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import Any

from xmatrix.constants import (
    DATA_DIR, CONFIG_FILE, CONFIG_FILE_CUSTOM, CONFIG_FILE_SINGBOX_CUSTOM,
    CONFIG_FILE_MIHOMO_CUSTOM, CORE_REGISTRY, _BASE,
)
from xmatrix.helpers import (
    load_port_config, load_node_defaults, load_dns_config, atomic_write_json,
    api_response, deep_merge, validate_url,
)
from xmatrix.storage.db import get_db, init_db
from xmatrix.storage.tunnels import load_tunnels, save_tunnels, get_tunnels
from xmatrix.storage.stats import load_stats, save_stats, record_node_traffic, get_node_stats
from xmatrix.storage.profiles import load_profiles, update_profile, migrate_profiles_on_refresh


class XMatrixAPI:
    """X-Matrix 后端 API 主类 — 薄委托层，所有方法委托到各子模块。"""

    def __init__(self) -> None:
        os.makedirs(DATA_DIR, exist_ok=True)
        init_db(self)
        self.tunnels: list[dict] = load_tunnels()
        self.xray_process: subprocess.Popen | None = None
        self.pre_service_proc: subprocess.Popen | None = None  # 副核心进程
        self._process_lock = threading.Lock()
        self._tunnels_lock = threading.Lock()
        self._js_log_file = os.path.join(_BASE, "js_console.log")
        self.log_queue: queue.Queue[str] = queue.Queue()
        self.active_index: int = -1
        self.stats_offset: dict[str, int] = load_stats()
        self.current_stats: dict[str, int] = {"up": 0, "down": 0}
        self.current_speeds: dict[str, int] = {"up_speed": 0, "down_speed": 0}
        self.per_outbound_stats: dict[str, dict] = {}  # tag → {"up": int, "down": int}
        self._last_stats_snapshot: dict[str, dict] = {}  # tag → {"up": int, "down": int} 上次快照，用于增量计算
        self.download_progress: dict = {"active": False, "percent": 0, "total": 0, "downloaded": 0, "message": "", "done": False, "paused": False, "speed_bytes_per_sec": 0, "eta_seconds": 0}
        self.current_connections: int = 0
        self.current_memory: float = 0.0
        self.connections: list[dict] = []
        self.port_process_map: dict[str, str] = {}
        self.is_quitting: bool = False
        self.close_behavior: str = "ask"
        self.sys_proxy_enabled: bool = False
        self.sys_proxy_pac: bool = True
        self.proxy_mode: str = "rule"
        self.active_core: str = "xray"
        self.tray_limit: int = 100  # 托盘菜单节点上限，0=不限制
        _ports = load_port_config()
        self.config_local_port: int = _ports["local_port"]
        self.pac_port: int = _ports["pac_port"]
        self.pac_server: HTTPServer | None = None
        self.window_hidden: bool = False
        self._job_handle: int | None = None  # Windows Job Object handle
        self.config_priority: str = "smart"  # 配置优先级: smart / nodelist / custom
        self.tun_mode: bool = False  # 当前是否启用 TUN 模式
        self._conn_refresh_stop: threading.Event = threading.Event()  # B-B3 连接自动刷新停止标志
        self._delay_test_stop: threading.Event = threading.Event()   # B-B4 自动延迟测试停止标志
        self._window: Any = None  # pywebview 窗口引用
        self._hotkey_hooks: list = []
        # 启动订阅定时刷新调度器（延迟导入避免循环）
        from xmatrix.storage.subscriptions import subscription_scheduler
        threading.Thread(target=subscription_scheduler, args=(self,), daemon=True).start()

    # ── JS 日志 ──────────────────────────────────────────────────────

    def js_log(self, level: str, message: str) -> None:
        """接收前端 JS 日志并写入文件。"""
        from datetime import datetime
        ts = datetime.now().strftime("%H:%M:%S.%f")[:-3]
        with open(self._js_log_file, "a", encoding="utf-8") as f:
            f.write(f"[{ts}] [{level}] {message}\n")

    # ── 持久化委托 ──────────────────────────────────────────────────

    def _get_db(self):
        """获取 SQLite 连接。"""
        return get_db()

    def _init_db(self) -> None:
        """初始化 SQLite 数据库。"""
        init_db(self)

    def _load_tunnels(self) -> list[dict]:
        """加载节点列表。"""
        return load_tunnels()

    def _save_tunnels(self) -> None:
        """保存节点列表。"""
        save_tunnels(self.tunnels, self._tunnels_lock)

    def _load_stats(self) -> dict:
        """加载流量统计。"""
        return load_stats()

    def _save_stats(self) -> None:
        """保存流量统计。"""
        save_stats(self.stats_offset)

    def _record_node_traffic(self, node_id: str, up_bytes: int, down_bytes: int) -> None:
        """记录节点流量增量。"""
        record_node_traffic(self.stats_offset, node_id, up_bytes, down_bytes)

    # ── 节点 CRUD 委托 ──────────────────────────────────────────────

    def get_tunnels(self) -> list[dict]:
        """获取节点列表。"""
        return get_tunnels(self)

    # ── 节点扩展数据委托 ────────────────────────────────────────────

    def _load_profiles(self) -> dict:
        """加载节点扩展数据。"""
        return load_profiles()

    @api_response
    def get_profiles(self) -> dict:
        """返回所有节点的扩展数据。"""
        return load_profiles()

    @api_response
    def update_profile(self, node_id: str, data: dict) -> dict:
        """更新节点扩展数据。"""
        return update_profile(node_id, data)

    def _migrate_profiles_on_refresh(self, old_tunnels: list[dict], new_tunnels: list[dict]) -> None:
        """订阅刷新后迁移扩展数据。"""
        migrate_profiles_on_refresh(old_tunnels, new_tunnels)

    # ── 流量统计委托 ────────────────────────────────────────────────

    @api_response
    def get_node_stats(self, node_id: str = "") -> dict:
        """返回节点流量统计。"""
        return get_node_stats(self.stats_offset, node_id)

    def fetch_traffic_stats(self) -> dict:
        """返回实时流量统计。"""
        return {
            "up": self.stats_offset.get("up", 0) + self.current_stats.get("up", 0),
            "down": self.stats_offset.get("down", 0) + self.current_stats.get("down", 0),
            **self.current_speeds, "connections": self.current_connections, "memory": self.current_memory,
        }

    @api_response
    def get_outbound_stats(self, node_id: str = "") -> dict:
        """返回指定节点的 per-outbound 实时流量统计。"""
        node = next((t for t in self.tunnels if t.get("id") == node_id), None)
        if not node:
            return {"success": False, "error": "未找到该节点"}
        tag = node.get("out_tag", "")
        stats = self.per_outbound_stats.get(tag, {"up": 0, "down": 0})
        return {"success": True, "up": stats["up"], "down": stats["down"], "tag": tag}

    @api_response
    def get_download_progress(self) -> dict:
        """返回当前下载进度。"""
        return {"success": True, **self.download_progress}

    # ── 连接和日志委托 ──────────────────────────────────────────────

    @api_response
    def fetch_connections(self) -> dict:
        """获取连接列表。"""
        from xmatrix.monitoring.clash_client import SingboxStatsClient
        # sing-box / mihomo: 从 Clash API 获取结构化连接数据
        if self.active_core in ("singbox", "mihomo"):
            try:
                raw = SingboxStatsClient.query_connections()
                conns: list[dict] = []
                for c in raw:
                    chain = c.get("chains", [])
                    conns.append({
                        "id": c.get("id", ""),
                        "time": c.get("start", "")[:19].replace("T", " "),
                        "process": c.get("metadata", {}).get("process", ""),
                        "network": c.get("metadata", {}).get("network", "tcp").upper(),
                        "target": f"{c.get('metadata', {}).get('host', '')}:{c.get('metadata', {}).get('destinationPort', '')}",
                        "outbound": chain[0] if chain else "direct",
                    })
                return {"success": True, "connections": conns}
            except Exception:
                pass
        return {"success": True, "connections": self.connections}

    @api_response
    def clear_connections(self) -> dict:
        """清空连接列表。"""
        self.connections.clear()
        return {"success": True}

    def fetch_logs(self) -> list[str]:
        """获取日志队列。"""
        logs: list[str] = []
        while not self.log_queue.empty():
            try:
                logs.append(self.log_queue.get_nowait())
            except queue.Empty:
                break
        return logs

    # ── 窗口控制 ────────────────────────────────────────────────────

    def set_close_behavior(self, behavior: str) -> dict:
        """设置关闭行为。"""
        self.close_behavior = behavior
        return {"success": True}

    @api_response
    def set_tray_limit(self, limit: int) -> dict:
        """设置托盘菜单节点上限。"""
        self.tray_limit = max(0, int(limit))
        return {"success": True, "limit": self.tray_limit}

    def hide_window(self) -> dict:
        """隐藏窗口。"""
        if self._window:
            self._window.hide()
        return {"success": True}

    def force_quit(self) -> dict:
        """强制退出。"""
        self.is_quitting = True
        self._save_tunnels()
        self._save_stats()
        try:
            self.toggle_system_proxy(False)
        except Exception:
            pass
        self.stop_core()
        os._exit(0)

    # ── 热键委托 ────────────────────────────────────────────────────

    @api_response
    def set_hotkeys(self, window_key: str, proxy_key: str) -> dict:
        """动态注册系统级全局快捷键。"""
        import keyboard
        try:
            # 只移除自己注册的热键，不影响其他钩子
            if hasattr(self, '_hotkey_hooks'):
                for hook in self._hotkey_hooks:
                    try:
                        keyboard.remove_hotkey(hook)
                    except Exception:
                        pass
            self._hotkey_hooks = []
            if window_key:
                self._hotkey_hooks.append(keyboard.add_hotkey(window_key, self._hotkey_toggle_window))
            if proxy_key:
                self._hotkey_hooks.append(keyboard.add_hotkey(proxy_key, self._hotkey_toggle_proxy))
            return {"success": True}
        except Exception as e:
            return {"success": False, "error": str(e)}

    def _hotkey_toggle_window(self) -> None:
        """全局热键回调：切换窗口可见性。"""
        if self._window is None:
            return
        if self.window_hidden:
            self._window.show()
            self._window.restore()
            self.window_hidden = False
        else:
            self._window.hide()
            self.window_hidden = True

    def _hotkey_toggle_proxy(self) -> None:
        """全局热键回调：切换系统代理。"""
        if self._window is None:
            return
        try:
            self._window.evaluate_js("(function(){try{var el=document.querySelector('[x-data]');if(el&&el.__x)el.__x.$data.toggleSysProxyFromTray()}catch(e){}})()")
        except Exception:
            pass

    # ── PAC 服务器 ──────────────────────────────────────────────────

    def _generate_pac_script(self) -> str:
        """根据当前路由规则动态生成 PAC 脚本。"""
        from xmatrix.network.proxy import generate_pac_script
        return generate_pac_script(self)

    def _start_pac_server(self) -> None:
        """启动 PAC 服务器。"""
        from xmatrix.network.proxy import start_pac_server
        start_pac_server(self)

    # ── 核心控制委托 ────────────────────────────────────────────────

    @api_response
    def start_core(self) -> dict:
        """启动核心。"""
        from xmatrix.core.lifecycle import start_core
        return start_core(self)

    @api_response
    def stop_core(self) -> dict:
        """停止核心。"""
        from xmatrix.core.lifecycle import stop_core
        return stop_core(self)

    def get_core_status(self) -> dict:
        """获取核心状态。"""
        from xmatrix.core.lifecycle import get_core_status
        return get_core_status(self)

    # ── 配置生成委托 ────────────────────────────────────────────────

    def _build_config(self, *args, **kwargs) -> dict:
        """生成 Xray 配置。"""
        from xmatrix.core.config_xray import build_config
        return build_config(self, *args, **kwargs)

    def _build_outbound(self, *args, **kwargs) -> dict:
        """构建 Xray outbound。"""
        from xmatrix.core.config_xray import build_outbound
        return build_outbound(*args, **kwargs)

    def _build_routing_rules(self, *args, **kwargs) -> list[dict]:
        """构建路由规则。"""
        from xmatrix.routing.engine import build_routing_rules
        return build_routing_rules(*args, **kwargs)

    def _build_config_singbox(self, *args, **kwargs) -> dict:
        """生成 sing-box 配置。"""
        from xmatrix.core.config_singbox import build_config_singbox
        return build_config_singbox(self, *args, **kwargs)

    def _build_config_mihomo(self, *args, **kwargs) -> dict:
        """生成 mihomo 配置。"""
        from xmatrix.core.config_mihomo import build_config_mihomo
        return build_config_mihomo(self, *args, **kwargs)

    # ── 系统代理委托 ────────────────────────────────────────────────

    @api_response
    def toggle_system_proxy(self, enable: bool, port: str = "2077", use_pac: bool = True) -> dict:
        """切换系统代理。"""
        from xmatrix.network.proxy import toggle_system_proxy
        return toggle_system_proxy(self, enable, port, use_pac)

    # ── 测速委托 ────────────────────────────────────────────────────

    @api_response
    def test_node_tcp_ping(self, index: int) -> dict:
        """TCP Ping 测试。"""
        from xmatrix.network.speedtest import test_node_tcp_ping
        return test_node_tcp_ping(self, index)

    @api_response
    def test_node_real_delay(self, index: int, test_url: str = "", timeout: int = 5, local_port: int = 2077) -> dict:
        """真实延迟测试。"""
        from xmatrix.network.speedtest import test_node_real_delay
        return test_node_real_delay(self, index, test_url, timeout, local_port)

    @api_response
    def test_download_speed(self, url: str, port: int = 2077) -> dict:
        """下载速度测试。"""
        from xmatrix.network.speedtest import test_download_speed
        return test_download_speed(self, url, port)

    # ── IP 检测委托 ────────────────────────────────────────────────

    @api_response
    def check_outbound_ip(self, port: int = 2077) -> dict:
        """检测出口 IP。"""
        from xmatrix.network.ip_check import check_outbound_ip
        return check_outbound_ip(self, port)

    @api_response
    def check_ip_quality(self, port: int = 2077, source: str = "ippure", token: str = "") -> dict:
        """IP 质量检测。"""
        from xmatrix.network.ip_check import check_ip_quality
        return check_ip_quality(self, port, source, token)

    @api_response
    def test_website_access(self, url: str, port: int = 2077) -> dict:
        """测试网站访问。"""
        from xmatrix.network.ip_check import test_website_access
        return test_website_access(self, url, port)

    # ── 核心管理委托 ────────────────────────────────────────────────

    @api_response
    def get_core_types(self) -> dict:
        """获取核心类型列表。"""
        from xmatrix.core.registry import get_core_types
        return get_core_types(self)

    def _find_core_exe(self, core_type: str) -> str | None:
        """查找核心可执行文件。"""
        from xmatrix.core.registry import find_core_exe
        return find_core_exe(core_type)

    def _find_xray(self) -> str | None:
        """查找 Xray 核心。"""
        from xmatrix.core.registry import find_core_exe
        return find_core_exe("xray")

    @api_response
    def set_active_core(self, core_type: str) -> dict:
        """设置活跃核心。"""
        from xmatrix.core.registry import set_active_core
        return set_active_core(self, core_type)

    @api_response
    def check_core_update(self, check_prerelease: bool = False) -> dict:
        """检查核心更新。"""
        from xmatrix.core.registry import check_core_update
        return check_core_update(self, check_prerelease)

    # ── Geo 数据委托 ────────────────────────────────────────────────

    @api_response
    def update_geo_data(self, source: str = "loyalsoldier") -> dict:
        """更新 Geo 数据。"""
        from xmatrix.geo.updater import update_geo_data
        return update_geo_data(self, source)

    @api_response
    def get_geo_status(self) -> dict:
        """获取 Geo 数据状态。"""
        from xmatrix.geo.updater import get_geo_status
        return get_geo_status()

    @api_response
    def get_geo_presets(self) -> dict:
        """获取 Geo 预设。"""
        from xmatrix.geo.updater import get_geo_presets
        return get_geo_presets()

    def auto_update_geo(self) -> None:
        """自动更新 Geo 数据。"""
        from xmatrix.geo.updater import auto_update_geo
        auto_update_geo(self)

    # ── 备份恢复委托 ────────────────────────────────────────────────

    @api_response
    def export_backup(self) -> dict:
        """导出备份。"""
        from xmatrix.backup.webdav import export_backup
        return export_backup(self)

    @api_response
    def import_backup(self) -> dict:
        """导入备份。"""
        from xmatrix.backup.webdav import import_backup
        return import_backup(self)

    @api_response
    def webdav_test(self, url: str) -> dict:
        """测试 WebDAV 连接。"""
        from xmatrix.backup.webdav import webdav_test
        return webdav_test(url)

    @api_response
    def webdav_backup(self, url: str, remote_path: str = "xmatrix-backup.zip") -> dict:
        """WebDAV 备份。"""
        from xmatrix.backup.webdav import webdav_backup
        return webdav_backup(self, url, remote_path)

    @api_response
    def webdav_restore(self, url: str, remote_path: str = "xmatrix-backup.zip") -> dict:
        """WebDAV 恢复。"""
        from xmatrix.backup.webdav import webdav_restore
        return webdav_restore(self, url, remote_path)

    # ── 下载管理委托 ────────────────────────────────────────────────

    @api_response
    def download_core(self, core_type: str) -> dict:
        """下载核心。"""
        from xmatrix.download.manager import download_core
        return download_core(self, core_type)

    def pause_download(self) -> dict:
        """暂停下载。"""
        from xmatrix.download.manager import pause_download
        return pause_download(self)

    def resume_download(self) -> dict:
        """恢复下载。"""
        from xmatrix.download.manager import resume_download
        return resume_download(self)

    # ── 辅助方法 ────────────────────────────────────────────────────

    @staticmethod
    def _safe_port(val: Any, default: int = 443) -> int:
        """安全端口转换。"""
        try:
            p = int(val)
            return p if 1 <= p <= 65535 else default
        except (ValueError, TypeError):
            return default

    def _next_tag(self, prefix: str) -> str:
        """生成不与现有标签冲突的唯一 tag。"""
        existing = {t.get(f"{prefix}_tag", "") for t in self.tunnels}
        n = 1
        while f"{prefix}-{n}" in existing:
            n += 1
        return f"{prefix}-{n}"

    def _get_custom_config_path(self) -> str:
        """根据当前活跃核心返回对应的 custom 配置文件路径。"""
        if self.active_core == "singbox":
            return CONFIG_FILE_SINGBOX_CUSTOM
        if self.active_core == "mihomo":
            return CONFIG_FILE_MIHOMO_CUSTOM
        return CONFIG_FILE_CUSTOM

    def _attach_job_object(self, proc: subprocess.Popen) -> None:
        """将进程绑定到 Windows Job Object。"""
        from xmatrix.core.lifecycle import attach_job_object
        attach_job_object(self, proc)

    def _cleanup_job_object(self) -> None:
        """清理 Windows Job Object handle。"""
        from xmatrix.core.lifecycle import cleanup_job_object
        cleanup_job_object(self)

    def _urlopen(self, url: str, timeout: int = 30, headers: dict | None = None):
        """打开 URL，依次尝试本地代理端口。"""
        from xmatrix.download.manager import urlopen
        return urlopen(self, url, timeout, headers)

    def _download_file(self, url: str, dest: str, retries: int = 3, timeout: int = 120) -> dict:
        """下载文件。"""
        from xmatrix.download.manager import download_file
        return download_file(self, url, dest, retries, timeout)

    def _download_file_multi_source(self, urls: list[str], dest: str, retries: int = 2, timeout: int = 120) -> dict:
        """多源下载。"""
        from xmatrix.download.manager import download_file_multi_source
        return download_file_multi_source(self, urls, dest, retries, timeout)

    def _cleanup_temp_files(self) -> None:
        """清理临时文件。"""
        import glob
        now = time.time()
        for fname in os.listdir(DATA_DIR):
            if fname.endswith(".tmp") or ".bak_" in fname:
                fpath = os.path.join(DATA_DIR, fname)
                try:
                    if os.path.isfile(fpath) and (now - os.path.getmtime(fpath)) > 86400:
                        os.unlink(fpath)
                except OSError:
                    pass

    # ── 以下方法需要从 main.py 搬移（Phase 4-7） ────────────────────
    # 注意：这些方法暂时保留空实现或简单委托，后续 Phase 会填充完整逻辑

    @api_response
    def add_tunnel(self, tunnel: dict) -> dict:
        """添加节点。"""
        from xmatrix.nodes.crud import add_tunnel
        return add_tunnel(self, tunnel)

    @api_response
    def delete_tunnel(self, index) -> dict:
        """删除节点。支持索引(int)或节点ID(str)。"""
        from xmatrix.nodes.crud import delete_tunnel
        # 如果传入的是字符串ID，转换为索引
        if isinstance(index, str):
            idx = next((i for i, t in enumerate(self.tunnels) if t.get("id") == index), -1)
            if idx == -1:
                return {"success": False, "error": f"未找到节点: {index}"}
            index = idx
        return delete_tunnel(self, index)

    @api_response
    def delete_tunnels_batch(self, indices: list[int]) -> dict:
        """批量删除节点。"""
        from xmatrix.nodes.crud import delete_tunnels_batch
        return delete_tunnels_batch(self, indices)

    @api_response
    def update_tunnel(self, index, data: dict) -> dict:
        """更新节点。支持索引(int)或节点ID(str)。"""
        from xmatrix.nodes.crud import update_tunnel
        # 如果传入的是字符串ID，转换为索引
        if isinstance(index, str):
            idx = next((i for i, t in enumerate(self.tunnels) if t.get("id") == index), -1)
            if idx == -1:
                return {"success": False, "error": f"未找到节点: {index}"}
            index = idx
        return update_tunnel(self, index, data)

    @api_response
    def reorder_tunnels(self, from_index: int, to_index: int) -> dict:
        """重排节点。"""
        from xmatrix.nodes.crud import reorder_tunnels
        return reorder_tunnels(self, from_index, to_index)

    @api_response
    def apply_tunnels_order(self, ordered_ids: list[str]) -> dict:
        """应用节点排序。"""
        from xmatrix.nodes.crud import apply_tunnels_order
        return apply_tunnels_order(self, ordered_ids)

    @api_response
    def sort_tunnels(self, key: str = "delay", reverse: bool = False) -> dict:
        """排序节点。"""
        from xmatrix.nodes.crud import sort_tunnels
        return sort_tunnels(self, key, reverse)

    @api_response
    def dedup_server_list(self, silent: bool = False) -> dict:
        """去重节点列表。"""
        from xmatrix.nodes.crud import dedup_server_list
        return dedup_server_list(self, silent)

    @api_response
    def remove_timeout_nodes(self, timeout_seconds: int = 300) -> dict:
        """移除超时节点。"""
        from xmatrix.nodes.crud import remove_timeout_nodes
        return remove_timeout_nodes(self, timeout_seconds)

    @api_response
    def clone_tunnel(self, index) -> dict:
        """克隆节点。支持索引(int)或节点ID(str)。"""
        from xmatrix.nodes.crud import clone_tunnel
        # 如果传入的是字符串ID，转换为索引
        if isinstance(index, str):
            idx = next((i for i, t in enumerate(self.tunnels) if t.get("id") == index), -1)
            if idx == -1:
                return {"success": False, "error": f"未找到节点: {index}"}
            index = idx
        return clone_tunnel(self, index)

    @api_response
    def import_config(self, file_path: str = "") -> dict:
        """导入配置文件。"""
        from xmatrix.nodes.import_export import import_config
        return import_config(self, file_path)

    @api_response
    def import_uri(self, text: str) -> dict:
        """导入 URI。"""
        from xmatrix.nodes.import_export import import_uri
        return import_uri(self, text)

    @api_response
    def import_subscription(self, url: str) -> dict:
        """导入订阅。"""
        from xmatrix.nodes.import_export import import_subscription
        return import_subscription(self, url)

    @api_response
    def export_uri(self, index: int) -> dict:
        """导出 URI。"""
        from xmatrix.nodes.import_export import export_uri
        return export_uri(self, index)

    @api_response
    def export_config(self, json_str: str) -> dict:
        """导出配置。"""
        from xmatrix.nodes.import_export import export_config
        return export_config(self, json_str)

    @api_response
    def add_policy_group(self, name: str, strategy: str, child_ids: list[str], filter_regex: str = "") -> dict:
        """添加策略组。"""
        from xmatrix.nodes.crud import add_policy_group
        return add_policy_group(self, name, strategy, child_ids, filter_regex)

    @api_response
    def update_policy_group(self, group_id: str, name: str = "", strategy: str = "", child_ids: list[str] | None = None, filter_regex: str | None = None) -> dict:
        """更新策略组。"""
        from xmatrix.nodes.crud import update_policy_group
        return update_policy_group(self, group_id, name, strategy, child_ids, filter_regex)

    @api_response
    def get_group_preview(self, child_ids: list[str] | None = None, filter_regex: str = "") -> dict:
        """获取策略组预览。"""
        from xmatrix.nodes.crud import get_group_preview
        return get_group_preview(self, child_ids, filter_regex)

    @api_response
    def auto_group_by_region(self) -> dict:
        """按地区自动分组。"""
        from xmatrix.nodes.crud import auto_group_by_region
        return auto_group_by_region(self)

    @api_response
    def activate_tunnel(self, index: int, *args, **kwargs) -> dict:
        """激活节点。"""
        from xmatrix.core.lifecycle import activate_tunnel
        # 前端通过 _buildCoreParams 展开为位置参数，需要映射为关键字参数
        if args:
            keys = [
                'active_rules', 'log_level', 'tun_mode', 'local_port', 'allow_lan',
                'enable_udp', 'sniffing', 'sniff_types', 'dns_strategy', 'enable_fake_dns',
                'proxy_mode', 'enable_custom_dns', 'remote_dns', 'local_dns',
                'enable_fragment', 'fragment_packets', 'fragment_length', 'fragment_interval',
                'tun_mtu', 'tun_stack', 'tun_auto_route', 'tun_strict_route',
                'tun_exclude_address', 'enable_mux', 'mux_concurrency',
                'inbound_auth', 'inbound_user', 'inbound_pass', 'lan_port', 'http_port',
                'second_port', 'direct_dns', 'dns_rules', 'tun_exclude_apps',
                'tun_include_apps', 'use_system_hosts',
            ]
            for i, v in enumerate(args):
                if i < len(keys):
                    kwargs[keys[i]] = v
        return activate_tunnel(self, index, **kwargs)

    @api_response
    def preview_config(self, *args, **kwargs) -> str:
        """预览配置。"""
        from xmatrix.core.config_xray import build_config
        if args:
            keys = [
                'index', 'active_rules', 'log_level', 'tun_mode', 'local_port', 'allow_lan',
                'enable_udp', 'sniffing', 'sniff_types', 'dns_strategy', 'enable_fake_dns',
                'proxy_mode', 'enable_custom_dns', 'remote_dns', 'local_dns',
                'enable_fragment', 'fragment_packets', 'fragment_length', 'fragment_interval',
                'tun_mtu', 'tun_stack', 'tun_auto_route', 'tun_strict_route',
                'tun_exclude_address', 'enable_mux', 'mux_concurrency',
                'inbound_auth', 'inbound_user', 'inbound_pass', 'lan_port', 'http_port',
                'second_port', 'direct_dns', 'dns_rules', 'tun_exclude_apps',
                'tun_include_apps', 'use_system_hosts',
            ]
            for i, v in enumerate(args):
                if i < len(keys):
                    kwargs[keys[i]] = v
        config = build_config(self, **kwargs)
        return json.dumps(config, ensure_ascii=False, indent=2)

    @api_response
    def save_config(self, *args, **kwargs) -> dict:
        """保存配置。"""
        from xmatrix.core.lifecycle import save_config
        return save_config(self, *args, **kwargs)

    @api_response
    def validate_config(self, json_str: str) -> dict:
        """校验配置。"""
        from xmatrix.core.lifecycle import validate_config
        return validate_config(self, json_str)

    @api_response
    def save_raw_config(self, json_str: str) -> dict:
        """保存原始配置。"""
        from xmatrix.core.lifecycle import save_raw_config
        return save_raw_config(self, json_str)

    @api_response
    def get_routing_topology(self, routing_rules: list[dict] | None = None, local_port: int = 2077) -> dict:
        """获取路由拓扑。"""
        from xmatrix.routing.engine import get_routing_topology
        return get_routing_topology(routing_rules, local_port)

    @api_response
    def get_port_config(self) -> dict:
        """获取端口配置。"""
        return {"success": True, "ports": load_port_config()}

    @api_response
    def save_port_config(self, ports: dict) -> dict:
        """保存端口配置。"""
        from xmatrix.core.lifecycle import save_port_config
        return save_port_config(self, ports)

    @api_response
    def check_ports(self, ports: list[int] | None = None) -> dict:
        """检测端口。"""
        from xmatrix.helpers import check_port_available
        from xmatrix.constants import API_PORT, CLASH_API_PORT
        if ports is None:
            ports_cfg = load_port_config()
            ports = [ports_cfg["api_port"], ports_cfg["clash_api_port"]]
        results = [{"port": p, "available": check_port_available(p)} for p in ports if isinstance(p, int) and p > 0]
        return {"success": True, "results": results}

    @api_response
    def get_sys_info(self) -> dict:
        """获取系统信息。"""
        import platform
        import ctypes
        os_info = f"{platform.system()} {platform.release()}"
        is_admin = False
        if os.name == "nt":
            try:
                is_admin = ctypes.windll.shell32.IsUserAnAdmin() != 0
            except Exception:
                pass
        return {"success": True, "os": os_info, "mode": "管理员模式" if is_admin else "用户模式", "version": "V1.0.0"}

    @api_response
    def toggle_auto_startup(self, enable: bool) -> dict:
        """切换开机自启。"""
        from xmatrix.core.lifecycle import toggle_auto_startup
        return toggle_auto_startup(enable)

    def check_auto_startup(self) -> bool:
        """检查是否开机自启。"""
        import winreg
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER,
                                r"Software\Microsoft\Windows\CurrentVersion\Run",
                                0, winreg.KEY_READ) as key:
                winreg.QueryValueEx(key, "X-Matrix")
                return True
        except (FileNotFoundError, OSError):
            return False

    @api_response
    def exempt_uwp_loopback(self) -> dict:
        """解除 UWP 回环限制。"""
        from xmatrix.process import run_hidden
        proc = run_hidden(
            "powershell", "-Command",
            'ForEach ($app in Get-AppxPackage) { CheckNetIsolation.exe LoopbackExempt -a -n="$($app.PackageFamilyName)" }',
            text=True, timeout=30,
        )
        if proc.returncode == 0:
            return {"success": True, "message": "UWP 回环限制已解除"}
        return {"success": False, "error": proc.stderr.strip() or proc.stdout.strip() or "执行失败"}

    @api_response
    def get_server_cert(self, addr: str, port: int = 443) -> dict:
        """获取服务器证书。"""
        import ssl
        import socket
        ctx = ssl.create_default_context()
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
        with socket.create_connection((addr, port), timeout=10) as sock:
            with ctx.wrap_socket(sock, server_hostname=addr) as ssock:
                cert_der = ssock.getpeercert(binary_form=True)
                cert_pem = ssl.DER_cert_to_PEM_cert(cert_der)
                cert_dict = ssock.getpeercert()
                subject = dict(x[0] for x in cert_dict.get("subject", ()))
                issuer = dict(x[0] for x in cert_dict.get("issuer", ()))
                return {
                    "success": True, "pem": cert_pem,
                    "subject_cn": subject.get("commonName", ""),
                    "issuer_cn": issuer.get("commonName", ""),
                    "not_before": cert_dict.get("notBefore", ""),
                    "not_after": cert_dict.get("notAfter", ""),
                    "serial": cert_dict.get("serialNumber", ""),
                    "san": [entry[1] for entry in cert_dict.get("subjectAltName", ())],
                }

    @api_response
    def test_port(self, server_addr: str, server_port: str) -> dict:
        """测试端口。"""
        import socket
        try:
            port = int(server_port)
        except (ValueError, TypeError):
            return {"success": False, "error": f"无效端口: {server_port}"}
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(5)
            result = sock.connect_ex((server_addr, port))
            return {"success": True, "status": "open" if result == 0 else "closed"}

    @api_response
    def select_wallpaper(self) -> dict:
        """选择壁纸。"""
        import base64
        import webview
        file_path = self._pick_file(webview.OPEN_DIALOG, allow_multiple=False,
                                    file_types=("图片文件 (*.png;*.jpg;*.jpeg;*.webp;*.gif)",))
        if not file_path:
            return {"success": False, "error": "用户取消选择"}
        try:
            with open(file_path, "rb") as f:
                encoded = base64.b64encode(f.read()).decode('utf-8')
            ext = os.path.splitext(file_path)[1][1:].lower()
            if ext == "jpg":
                ext = "jpeg"
            mime_type = f"image/{ext}"
            return {"success": True, "wallpaper": f"data:{mime_type};base64,{encoded}"}
        except Exception as e:
            return {"success": False, "error": f"壁纸读取失败: {str(e)}"}

    def _pick_file(self, dialog_type: str, allow_multiple: bool = False, file_types: tuple = ()) -> str | list[str] | None:
        """调用文件选择对话框。"""
        if self._window:
            return self._window.create_file_dialog(dialog_type, allow_multiple=allow_multiple, file_types=file_types)
        return None

    # ── 订阅管理委托 ────────────────────────────────────────────────

    @api_response
    def get_subscriptions(self) -> dict:
        """获取订阅列表。"""
        from xmatrix.storage.subscriptions import get_subscriptions
        return get_subscriptions(self)

    @api_response
    def add_subscription(self, name: str, url: str, ua: str = "", interval_hours: int = 0, filter_regex: str = "", subconverter_url: str = "", target_format: str = "clash", memo: str = "") -> dict:
        """添加订阅。"""
        from xmatrix.storage.subscriptions import add_subscription
        return add_subscription(self, name, url, ua, interval_hours, filter_regex, subconverter_url, target_format, memo)

    @api_response
    def update_subscription(self, sub_id: str, **kwargs) -> dict:
        """更新订阅。"""
        from xmatrix.storage.subscriptions import update_subscription
        return update_subscription(self, sub_id, **kwargs)

    @api_response
    def delete_subscription(self, sub_id: str) -> dict:
        """删除订阅。"""
        from xmatrix.storage.subscriptions import delete_subscription
        return delete_subscription(self, sub_id)

    @api_response
    def refresh_subscription(self, sub_id: str = "") -> dict:
        """刷新订阅。"""
        from xmatrix.storage.subscriptions import refresh_subscription
        return refresh_subscription(self, sub_id)

    # ── 配置优先级 ──────────────────────────────────────────────────

    @api_response
    def set_config_priority(self, mode: str) -> dict:
        """设置配置优先级。"""
        if mode not in ("smart", "nodelist", "custom", "merge"):
            return {"success": False, "error": f"无效模式: {mode}"}
        self.config_priority = mode
        return {"success": True, "mode": mode}

    @api_response
    def get_config_priority(self) -> dict:
        """获取配置优先级。"""
        any_custom = any(os.path.isfile(p) for p in [CONFIG_FILE_CUSTOM, CONFIG_FILE_SINGBOX_CUSTOM, CONFIG_FILE_MIHOMO_CUSTOM])
        custom_path = self._get_custom_config_path()
        return {
            "success": True, "mode": self.config_priority,
            "custom_exists": any_custom, "custom_path": custom_path, "default_path": CONFIG_FILE,
        }

    @api_response
    def save_custom_config(self, json_str: str) -> dict:
        """保存自定义配置。"""
        import re
        clean = re.sub(r'(?<!:)//.*', '', json_str)
        clean = re.sub(r'/\*.*?\*/', '', clean, flags=re.DOTALL)
        try:
            parsed = json.loads(clean)
        except json.JSONDecodeError as e:
            return {"success": False, "error": f"JSON 解析失败: {e}"}
        custom_path = self._get_custom_config_path()
        with open(custom_path, "w", encoding="utf-8") as f:
            json.dump(parsed, f, ensure_ascii=False, indent=2)
        return {"success": True, "path": custom_path}

    @api_response
    def delete_custom_config(self) -> dict:
        """删除自定义配置。"""
        removed = []
        for path in [CONFIG_FILE_CUSTOM, CONFIG_FILE_SINGBOX_CUSTOM, CONFIG_FILE_MIHOMO_CUSTOM]:
            if os.path.exists(path):
                os.remove(path)
                removed.append(os.path.basename(path))
        return {"success": True, "removed": removed}

    # ── 其他设置 API ────────────────────────────────────────────────

    @api_response
    def save_frontend_settings(self, settings: dict) -> dict:
        """保存前端设置。"""
        from xmatrix.core.lifecycle import save_frontend_settings
        return save_frontend_settings(self, settings)

    @api_response
    def get_connections_auto_refresh(self) -> dict:
        """获取连接自动刷新配置。"""
        cfg = {"connections_auto_refresh": False, "connections_refresh_interval": 2}
        try:
            cfg_path = os.path.join(DATA_DIR, "config.json")
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    saved = json.load(f)
                cfg["connections_auto_refresh"] = saved.get("connections_auto_refresh", False)
                cfg["connections_refresh_interval"] = saved.get("connections_refresh_interval", 2)
        except Exception:
            pass
        return {"success": True, **cfg}

    @api_response
    def set_connections_auto_refresh(self, enabled: bool, interval: int = 2) -> dict:
        """设置连接自动刷新。"""
        if interval < 1:
            interval = 1
        cfg_path = os.path.join(DATA_DIR, "config.json")
        existing = {}
        try:
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    existing = json.load(f)
        except Exception:
            pass
        existing["connections_auto_refresh"] = bool(enabled)
        existing["connections_refresh_interval"] = int(interval)
        atomic_write_json(cfg_path, existing)
        return {"success": True, "connections_auto_refresh": bool(enabled), "connections_refresh_interval": int(interval)}

    @api_response
    def get_auto_delay_test_config(self) -> dict:
        """获取自动延迟测试配置。"""
        interval = 0
        try:
            cfg_path = os.path.join(DATA_DIR, "config.json")
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    interval = json.load(f).get("auto_delay_test_interval", 0)
        except Exception:
            pass
        return {"success": True, "auto_delay_test_interval": interval}

    @api_response
    def set_auto_delay_test_config(self, interval_minutes: int) -> dict:
        """设置自动延迟测试间隔。"""
        if interval_minutes < 0:
            interval_minutes = 0
        cfg_path = os.path.join(DATA_DIR, "config.json")
        existing = {}
        try:
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    existing = json.load(f)
        except Exception:
            pass
        existing["auto_delay_test_interval"] = int(interval_minutes)
        atomic_write_json(cfg_path, existing)
        return {"success": True, "auto_delay_test_interval": int(interval_minutes)}

    @api_response
    def get_prerelease_config(self) -> dict:
        """获取预发布配置。"""
        check_prerelease = False
        try:
            cfg_path = os.path.join(DATA_DIR, "config.json")
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    check_prerelease = json.load(f).get("check_prerelease", False)
        except Exception:
            pass
        return {"success": True, "check_prerelease": check_prerelease}

    @api_response
    def set_prerelease_config(self, enabled: bool) -> dict:
        """设置预发布配置。"""
        cfg_path = os.path.join(DATA_DIR, "config.json")
        existing = {}
        try:
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    existing = json.load(f)
        except Exception:
            pass
        existing["check_prerelease"] = bool(enabled)
        atomic_write_json(cfg_path, existing)
        return {"success": True, "check_prerelease": bool(enabled)}

    @api_response
    def get_system_hosts_config(self) -> dict:
        """获取系统 hosts 配置。"""
        use_sys = True
        try:
            cfg_path = os.path.join(DATA_DIR, "config.json")
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    use_sys = json.load(f).get("use_system_hosts", True)
        except Exception:
            pass
        return {"success": True, "use_system_hosts": use_sys}

    @api_response
    def set_system_hosts_config(self, enabled: bool) -> dict:
        """设置系统 hosts 配置。"""
        cfg_path = os.path.join(DATA_DIR, "config.json")
        existing = {}
        try:
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    existing = json.load(f)
        except Exception:
            pass
        existing["use_system_hosts"] = bool(enabled)
        atomic_write_json(cfg_path, existing)
        return {"success": True, "use_system_hosts": bool(enabled)}

    @api_response
    def get_dns_presets(self) -> dict:
        """获取 DNS 预设。"""
        from xmatrix.constants import DNS_PRESETS
        return {"success": True, "presets": DNS_PRESETS}

    @api_response
    def get_config_presets(self) -> dict:
        """获取配置预设。"""
        presets: list[dict] = [
            {
                "name": "⬜ 白名单模式",
                "description": "国内直连，其余走代理（推荐）",
                "proxy_mode": "rule",
                "enable_custom_dns": True,
                "remote_dns": "https+local://1.1.1.1/dns-query",
                "local_dns": "223.5.5.5",
                "enable_fake_dns": True,
                "rules": [
                    {"type": "geosite", "content": "cn", "outbound": "direct", "enabled": True},
                    {"type": "geoip", "content": "cn", "outbound": "direct", "enabled": True},
                ],
            },
            {
                "name": "⬛ 黑名单模式",
                "description": "指定域名走代理，其余直连",
                "proxy_mode": "rule",
                "enable_custom_dns": True,
                "remote_dns": "https+local://1.1.1.1/dns-query",
                "local_dns": "223.5.5.5",
                "enable_fake_dns": False,
                "rules": [
                    {"type": "domain", "content": "google.com,googleapis.com,gstatic.com,youtube.com,facebook.com,twitter.com", "outbound": "proxy", "enabled": True},
                ],
            },
            {
                "name": "🟦 全局代理",
                "description": "所有流量走代理（慎用）",
                "proxy_mode": "global",
                "enable_custom_dns": True,
                "remote_dns": "https+local://1.1.1.1/dns-query",
                "local_dns": "223.5.5.5",
                "enable_fake_dns": True,
                "rules": [],
            },
        ]
        return {"success": True, "presets": presets}

    @api_response
    def apply_config_preset(self, preset_name: str) -> dict:
        """应用配置预设。"""
        presets_result = self.get_config_presets()
        if not presets_result.get("success"):
            return presets_result
        presets = presets_result.get("presets", [])
        preset = next((p for p in presets if p.get("name") == preset_name), None)
        if not preset:
            names = [p.get("name", "") for p in presets]
            return {"success": False, "error": f"未找到模板: {preset_name}，可选: {', '.join(names)}"}
        self.proxy_mode = preset.get("proxy_mode", "rule")
        return {"success": True, "preset": preset, "message": f"已应用模板: {preset_name}"}

    @api_response
    def get_core_type_map(self) -> dict:
        """获取核心类型映射。"""
        cfg_path = os.path.join(DATA_DIR, "config.json")
        try:
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    saved = json.load(f)
                return {"success": True, "core_type_map": saved.get("core_type_map", {})}
        except Exception:
            pass
        return {"success": True, "core_type_map": {}}

    @api_response
    def save_core_type_map(self, core_type_map: dict) -> dict:
        """保存核心类型映射。"""
        cfg_path = os.path.join(DATA_DIR, "config.json")
        try:
            saved = {}
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    saved = json.load(f)
            valid_map = {}
            for proto, core_id in core_type_map.items():
                if isinstance(proto, str) and isinstance(core_id, str) and core_id in CORE_REGISTRY:
                    valid_map[proto.lower()] = core_id
            saved["core_type_map"] = valid_map
            with open(cfg_path, "w", encoding="utf-8") as f:
                json.dump(saved, f, ensure_ascii=False, indent=2)
            return {"success": True, "core_type_map": valid_map}
        except Exception as e:
            return {"success": False, "error": str(e)}

    @api_response
    def get_pre_service_config(self) -> dict:
        """获取副核心配置。"""
        cfg_path = os.path.join(DATA_DIR, "config.json")
        try:
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    saved = json.load(f)
                return {
                    "success": True,
                    "pre_service_enabled": saved.get("pre_service_enabled", False),
                    "pre_service_core": saved.get("pre_service_core", ""),
                    "pre_service_port": saved.get("pre_service_port", 2076),
                }
        except Exception:
            pass
        return {"success": True, "pre_service_enabled": False, "pre_service_core": "", "pre_service_port": 2076}

    @api_response
    def save_pre_service_config(self, enabled: bool = False, core: str = "", port: int = 2076) -> dict:
        """保存副核心配置。"""
        cfg_path = os.path.join(DATA_DIR, "config.json")
        try:
            saved = {}
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    saved = json.load(f)
            saved["pre_service_enabled"] = bool(enabled)
            saved["pre_service_core"] = core if core in CORE_REGISTRY else ""
            saved["pre_service_port"] = int(port) if 1024 <= port <= 65535 else 2076
            with open(cfg_path, "w", encoding="utf-8") as f:
                json.dump(saved, f, ensure_ascii=False, indent=2)
            return {"success": True, "pre_service_enabled": saved["pre_service_enabled"], "pre_service_core": saved["pre_service_core"], "pre_service_port": saved["pre_service_port"]}
        except Exception as e:
            return {"success": False, "error": str(e)}

    @api_response
    def update_core(self) -> dict:
        """更新核心。"""
        from xmatrix.core.registry import update_core
        return update_core(self)

    @api_response
    def get_available_ports(self) -> dict:
        """获取可用端口列表。"""
        from xmatrix.constants import API_PORT, CLASH_API_PORT
        ports: list[int] = []
        seen: set[int] = set()

        def _add(p: int) -> None:
            if p not in seen:
                seen.add(p)
                ports.append(p)

        _add(self.config_local_port)
        for t in self.tunnels:
            p = t.get("server_port")
            if p:
                try:
                    _add(int(p))
                except (ValueError, TypeError):
                    pass
        _add(load_port_config().get("api_port", API_PORT))
        _add(load_port_config().get("clash_api_port", CLASH_API_PORT))
        return {"success": True, "ports": ports, "priority": self.config_priority}

    @api_response
    def import_routing_rules(self) -> dict:
        """导入路由规则。"""
        from xmatrix.nodes.import_export import import_routing_rules
        return import_routing_rules(self)

    @api_response
    def import_routing_rules_from_url(self, url: str) -> dict:
        """从 URL 导入路由规则。"""
        from xmatrix.nodes.import_export import import_routing_rules_from_url
        return import_routing_rules_from_url(url)

    @api_response
    def export_routing_rules(self, rules_json: str) -> dict:
        """导出路由规则。"""
        from xmatrix.nodes.import_export import export_routing_rules
        return export_routing_rules(self, rules_json)

    @api_response
    def import_routing_template_from_url(self, url: str) -> dict:
        """从 URL 导入路由模板。"""
        from xmatrix.nodes.import_export import import_routing_template_from_url
        return import_routing_template_from_url(url)

    @api_response
    def export_xmatrix_uri(self, index: int) -> dict:
        """导出 xmatrix:// URI。"""
        from xmatrix.nodes.import_export import export_xmatrix_uri
        return export_xmatrix_uri(self, index)

    @api_response
    def batch_test_real_delay(self, node_indices: list[int], concurrency: int = 5) -> dict:
        """批量延迟测试。"""
        from xmatrix.network.speedtest import batch_test_real_delay
        return batch_test_real_delay(self, node_indices, concurrency)

    @api_response
    def test_node_bandwidth(self, index: int, test_url: str = "", test_bytes: int = 10000000, timeout: int = 30) -> dict:
        """带宽测试。"""
        from xmatrix.network.speedtest import test_node_bandwidth
        return test_node_bandwidth(self, index, test_url, test_bytes, timeout)

    @api_response
    def batch_test_bandwidth(self, node_indices: list[int], max_workers: int = 3, test_url: str = "", test_bytes: int = 10000000) -> dict:
        """批量带宽测试。"""
        from xmatrix.network.speedtest import batch_test_bandwidth
        return batch_test_bandwidth(self, node_indices, max_workers, test_url, test_bytes)

    @api_response
    def build_speedtest_config(self, node_indices: list[int], base_port: int = 20801) -> dict:
        """生成测速配置。"""
        from xmatrix.network.speedtest import build_speedtest_config
        return build_speedtest_config(self, node_indices, base_port)

    @api_response
    def cleanup_speedtest_configs(self) -> dict:
        """清理测速配置。"""
        from xmatrix.network.speedtest import cleanup_speedtest_configs
        return cleanup_speedtest_configs()

    @api_response
    def test_node_mixed(self, index: int, test_url: str = "", timeout: int = 5, local_port: int = 2077) -> dict:
        """混合测速。"""
        from xmatrix.network.speedtest import test_node_mixed
        return test_node_mixed(self, index, test_url, timeout, local_port)

    @api_response
    def test_node_udp_ping(self, index: int) -> dict:
        """UDP Ping 测试。"""
        from xmatrix.network.speedtest import test_node_udp_ping
        return test_node_udp_ping(self, index)

    def _probe_single_node(self, index: int) -> dict:
        """探测单个节点。"""
        from xmatrix.network.ip_check import probe_single_node
        return probe_single_node(self, index)

    # ── 代理链 & BGP 拓扑 ──────────────────────────────────────────

    @api_response
    def set_proxy_chain(self, node_id: str, chain_id: str = "") -> dict:
        """设置节点的代理链。"""
        for t in self.tunnels:
            if t.get("id") == node_id:
                t["chain_id"] = chain_id
                self._save_tunnels()
                return {"success": True, "node_id": node_id, "chain_id": chain_id}
        return {"success": False, "error": f"未找到节点: {node_id}"}

    @api_response
    def test_bgp_topology(self) -> dict:
        """BGP 拓扑测试。"""
        # 简单实现：返回所有节点的连通性拓扑
        results = []
        for i, t in enumerate(self.tunnels):
            if t.get("protocol") == "policy_group":
                continue
            addr = t.get("server_addr", "")
            port = t.get("server_port", 443)
            try:
                port_int = self._safe_port(port)
                import socket
                with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
                    sock.settimeout(3)
                    result = sock.connect_ex((addr, port_int))
                    results.append({
                        "index": i,
                        "tag": t.get("out_tag", ""),
                        "addr": f"{addr}:{port}",
                        "reachable": result == 0,
                    })
            except Exception:
                results.append({
                    "index": i,
                    "tag": t.get("out_tag", ""),
                    "addr": f"{addr}:{port}",
                    "reachable": False,
                })
        return {"success": True, "results": results}

"""
X-Matrix Client — 入口文件
职责：pywebview 窗口创建、系统托盘、热键、PAC 服务器、atexit 注册。
"""
from __future__ import annotations

import atexit
import ctypes
import json
import logging
import os
import sys
import threading

import pystray
import webview
from PIL import Image, ImageDraw
from pystray import MenuItem as item

# ── 路径常量 ─────────────────────────────────────────────────────────
if getattr(sys, "frozen", False):
    _BASE = os.path.dirname(sys.executable)
    _RES = sys._MEIPASS
else:
    _BASE = os.path.dirname(os.path.abspath(__file__))
    _RES = _BASE

DATA_DIR: str = os.path.join(_BASE, "data")
HTML_FILE: str = os.path.join(_RES, "index.html")

from xmatrix.api import XMatrixAPI


def _setup_logging() -> None:
    """把日志同时写到控制台与 data/logs/app.log（滚动，单文件 2MB / 保留 3 份）。"""
    from logging.handlers import RotatingFileHandler
    log_dir = os.path.join(DATA_DIR, "logs")
    os.makedirs(log_dir, exist_ok=True)
    fmt = logging.Formatter("[%(asctime)s] [%(levelname)s] %(message)s", "%Y-%m-%d %H:%M:%S")
    root = logging.getLogger()
    root.setLevel(logging.INFO)
    if root.handlers:
        return
    fh = RotatingFileHandler(os.path.join(log_dir, "app.log"), maxBytes=2 * 1024 * 1024, backupCount=3, encoding="utf-8")
    fh.setFormatter(fmt)
    root.addHandler(fh)
    sh = logging.StreamHandler()
    sh.setFormatter(fmt)
    root.addHandler(sh)


def _create_tray_icon() -> Image.Image:
    """绘制系统托盘图标：64×64 靛蓝底色 + 白色 X。"""
    img = Image.new("RGBA", (64, 64), (99, 102, 241, 255))
    draw = ImageDraw.Draw(img)
    draw.line([(16, 16), (48, 48)], fill=(255, 255, 255, 255), width=6)
    draw.line([(48, 16), (16, 48)], fill=(255, 255, 255, 255), width=6)
    return img


def _validate_index_html() -> None:
    """启动前校验 index.html，拦截常见 JS 语法陷阱。"""
    import subprocess
    script = os.path.join(_BASE, "validate_html.py")
    if not os.path.isfile(script):
        return
    result = subprocess.run(
        [sys.executable, script],
        capture_output=True, text=True, cwd=_BASE,
    )
    if result.returncode != 0:
        print(result.stdout)
        logging.warning("[启动] index.html 校验失败:\n%s", result.stdout)


# ── 单实例锁 ─────────────────────────────────────────────────────────
_SINGLETON_MUTEX_NAME = r"Global\XMatrix-Client-pro"
_singleton_handle = None
_singleton_lock_file = None


def _acquire_windows_mutex(name: str = _SINGLETON_MUTEX_NAME) -> str:
    """返回 ok / busy / error；busy 表示已有实例。"""
    global _singleton_handle
    try:
        kernel32 = ctypes.windll.kernel32
        kernel32.CreateMutexW.restype = ctypes.c_void_p
        handle = kernel32.CreateMutexW(None, False, name)
        if not handle:
            return "error"
        # ERROR_ALREADY_EXISTS = 183
        if kernel32.GetLastError() == 183:
            kernel32.CloseHandle(handle)
            return "busy"
        _singleton_handle = handle
        return "ok"
    except Exception as e:
        logging.warning("[单例] 创建命名 Mutex 失败: %s", e)
        return "error"


def _acquire_file_lock() -> bool:
    """非 Windows（以及 Mutex 不可用时的兜底）文件锁。"""
    global _singleton_lock_file
    lock_path = os.path.join(DATA_DIR, "xmatrix.lock")
    try:
        f = open(lock_path, "a+", encoding="utf-8")
        f.write("x")
        f.flush()
        f.seek(0)
        if os.name == "nt":
            import msvcrt
            msvcrt.locking(f.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(f.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        _singleton_lock_file = f
        return True
    except OSError as e:
        logging.warning("[单例] 获取文件锁失败: %s", e)
        try:
            f.close()
        except Exception:
            pass
        return False
    except Exception as e:
        logging.warning("[单例] 文件锁异常: %s", e)
        return False


def _acquire_single_instance() -> bool:
    if os.name == "nt":
        state = _acquire_windows_mutex()
        if state == "ok":
            return True
        if state == "busy":
            return False
        logging.warning("[单例] 命名 Mutex 不可用，改用文件锁兜底")
    return _acquire_file_lock()


def _release_single_instance() -> None:
    global _singleton_handle, _singleton_lock_file
    if _singleton_handle is not None:
        try:
            ctypes.windll.kernel32.CloseHandle(_singleton_handle)
        except Exception:
            pass
        _singleton_handle = None
    if _singleton_lock_file is not None:
        try:
            if os.name == "nt":
                import msvcrt
                _singleton_lock_file.seek(0)
                msvcrt.locking(_singleton_lock_file.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                import fcntl
                fcntl.flock(_singleton_lock_file.fileno(), fcntl.LOCK_UN)
        except Exception:
            pass
        try:
            _singleton_lock_file.close()
        except Exception:
            pass
        _singleton_lock_file = None


def main() -> None:
    """主入口函数。"""
    if not _acquire_single_instance():
        if os.name == "nt":
            try:
                ctypes.windll.user32.MessageBoxW(0, "X-Matrix 已在运行", "X-Matrix", 0x40)
            except Exception:
                pass
        return
    atexit.register(_release_single_instance)

    _setup_logging()
    _validate_index_html()
    api = XMatrixAPI()
    atexit.register(api.stop_core)

    window = webview.create_window(
        title="X-Matrix",
        url=HTML_FILE, js_api=api,
        width=1440, height=900, min_size=(1024, 720),
        resizable=True, frameless=False, easy_drag=False,
        background_color='#F9FAFB'
    )
    api._window = window

    def on_closing() -> bool:
        if api.is_quitting:
            return True
        if api.close_behavior == "quit":
            api._save_tunnels()
            api._save_stats()
            try:
                api.toggle_system_proxy(False)
            except Exception as e:
                logging.warning("[退出] 还原系统代理失败: %s", e)
            api.stop_core()
            _release_single_instance()
            os._exit(0)
        if hasattr(api, "_window") and api._window:
            api._window.hide()
        return False

    window.events.closing += on_closing

    def show_window(icon: pystray.Icon, _item: item) -> None:
        window.show()

    def quit_app(icon: pystray.Icon, _item: item) -> None:
        api.is_quitting = True
        api.stop_core()
        _release_single_instance()
        icon.stop()
        window.destroy()

    def _safe_js(js_expr):
        """安全执行 JS：兼容 Alpine 3（Alpine.$data）与 Alpine 2（el.__x），未就绪时静默失败。"""
        try:
            api._window.evaluate_js(
                "(function(){try{var el=document.querySelector('[x-data]');"
                "var d=null;"
                "if(el){ if(window.Alpine&&window.Alpine.$data){d=window.Alpine.$data(el);} else if(el.__x){d=el.__x.$data;} }"
                "if(d){ window.__xm=d;" + js_expr + " }}catch(e){}})()"
            )
        except Exception as e:
            logging.debug("[托盘] 执行 JS 失败: %s", e)

    def menu_generator():
        yield item("显示主窗口", show_window, default=True)
        yield pystray.Menu.SEPARATOR

        if api.tunnels:
            show_tunnels = api.tunnels if api.tray_limit <= 0 else api.tunnels[:api.tray_limit]
            for i, t in enumerate(show_tunnels):
                def make_action(idx):
                    return lambda icon, it: _safe_js("window.__xm.activateNode(" + str(idx) + ")")
                def make_checked(idx):
                    return lambda it: api.active_index == idx

                name = t.get("out_tag") or f"节点 {i+1}"
                if t.get("protocol") == "policy_group":
                    name = f"⚖ {name}"
                if len(name) > 30:
                    name = name[:27] + "..."
                yield item(name, make_action(i), checked=make_checked(i), radio=True)
            yield pystray.Menu.SEPARATOR

        def make_mode_action(mode):
            return lambda icon, it: _safe_js(f"window.__xm.setProxyModeFromTray('{mode}')")

        yield item("路由模式", pystray.Menu(
            item("规则 (Rule)", make_mode_action("rule"), checked=lambda it: api.proxy_mode == "rule", radio=True),
            item("全局 (Global)", make_mode_action("global"), checked=lambda it: api.proxy_mode == "global", radio=True),
            item("仅路由 (Route Only)", make_mode_action("route_only"), checked=lambda it: api.proxy_mode == "route_only", radio=True),
            item("直连 (Direct)", make_mode_action("direct"), checked=lambda it: api.proxy_mode == "direct", radio=True),
        ))
        yield item("代理接管模式", pystray.Menu(
            item("智能 PAC", lambda icon, it: _safe_js("window.__xm.setSysProxyModeFromTray('pac')"), checked=lambda it: getattr(api, 'sys_proxy_pac', True), radio=True),
            item("强制全局", lambda icon, it: _safe_js("window.__xm.setSysProxyModeFromTray('global')"), checked=lambda it: not getattr(api, 'sys_proxy_pac', True), radio=True),
        ))
        yield item("开启系统代理",
                   lambda icon, it: _safe_js("window.__xm.toggleSysProxyFromTray()"),
                   checked=lambda it: getattr(api, 'sys_proxy_enabled', False))
        yield pystray.Menu.SEPARATOR
        yield item("从剪贴板导入节点",
                   lambda icon, it: _safe_js("window.__xm.importFromClipboard()"))
        yield item("复制代理命令",
                   lambda icon, it: _safe_js(
                       f"navigator.clipboard.writeText('set http_proxy=http://127.0.0.1:{api.config_local_port} && set https_proxy=http://127.0.0.1:{api.config_local_port}').then(function(){{ window.__xm.showToast('代理命令已复制', 'success') }})"
                   ))
        yield pystray.Menu.SEPARATOR
        yield item("完全退出", quit_app)

    tray_menu = pystray.Menu(menu_generator)
    tray_icon = pystray.Icon("X-Matrix", _create_tray_icon(), "X-Matrix Proxy", tray_menu)
    threading.Thread(target=tray_icon.run, daemon=True).start()

    # ── 硬件加速开关（从 config.json 读取，默认开启） ──
    try:
        with open(os.path.join(DATA_DIR, "config.json"), "r", encoding="utf-8") as _f:
            _hwa_cfg = json.load(_f)
        _enable_hwa = _hwa_cfg.get("enable_hwa", True)
    except Exception:
        _enable_hwa = True
    if not _enable_hwa:
        os.environ["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = "--disable-gpu"
        logging.info("[启动] 硬件加速已关闭，使用软件渲染模式")

    # 方案 C：固定缓存目录，后续启动走磁盘缓存加速
    user_data_dir = os.path.join(_BASE, "webview_cache")
    # 调试模式：默认关闭；可用环境变量 XMATRIX_DEBUG=1 或 data/config.json 的 debug_mode=true 打开
    debug_mode = os.environ.get("XMATRIX_DEBUG", "").strip().lower() in ("1", "true", "yes", "on")
    if not debug_mode:
        try:
            with open(os.path.join(DATA_DIR, "config.json"), "r", encoding="utf-8") as _f:
                debug_mode = bool(json.load(_f).get("debug_mode", False))
        except Exception:
            debug_mode = False
    if debug_mode:
        logging.info("[启动] 调试模式已开启（开发者工具将自动打开）")
    webview.start(debug=debug_mode, private_mode=False, storage_path=user_data_dir)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        logging.exception("[致命] 启动失败")
        raise

"""
X-Matrix Client — 入口文件
职责：pywebview 窗口创建、系统托盘、热键、PAC 服务器、atexit 注册。
"""
from __future__ import annotations

import atexit
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


def main() -> None:
    """主入口函数。"""
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
            except Exception:
                pass
            api.stop_core()
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
        icon.stop()
        window.destroy()

    def _safe_js(js_expr):
        """安全执行 JS：Alpine 未就绪时静默失败，不抛异常。"""
        try:
            api._window.evaluate_js(
                "(function(){try{var el=document.querySelector('[x-data]');if(el&&el.__x){" + js_expr + "}}catch(e){}})()"
            )
        except Exception:
            pass

    def menu_generator():
        yield item("显示主窗口", show_window, default=True)
        yield pystray.Menu.SEPARATOR

        if api.tunnels:
            show_tunnels = api.tunnels if api.tray_limit <= 0 else api.tunnels[:api.tray_limit]
            for i, t in enumerate(show_tunnels):
                def make_action(idx):
                    return lambda icon, it: _safe_js("el.__x.$data.activateNode(" + str(idx) + ")")
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
            return lambda icon, it: _safe_js(f"el.__x.$data.setProxyModeFromTray('{mode}')")

        yield item("路由模式", pystray.Menu(
            item("规则 (Rule)", make_mode_action("rule"), checked=lambda it: api.proxy_mode == "rule", radio=True),
            item("全局 (Global)", make_mode_action("global"), checked=lambda it: api.proxy_mode == "global", radio=True),
            item("仅路由 (Route Only)", make_mode_action("route_only"), checked=lambda it: api.proxy_mode == "route_only", radio=True),
            item("直连 (Direct)", make_mode_action("direct"), checked=lambda it: api.proxy_mode == "direct", radio=True),
        ))
        yield item("代理接管模式", pystray.Menu(
            item("智能 PAC", lambda icon, it: _safe_js("el.__x.$data.setSysProxyModeFromTray('pac')"), checked=lambda it: getattr(api, 'sys_proxy_pac', True), radio=True),
            item("强制全局", lambda icon, it: _safe_js("el.__x.$data.setSysProxyModeFromTray('global')"), checked=lambda it: not getattr(api, 'sys_proxy_pac', True), radio=True),
        ))
        yield item("开启系统代理",
                   lambda icon, it: _safe_js("el.__x.$data.toggleSysProxyFromTray()"),
                   checked=lambda it: getattr(api, 'sys_proxy_enabled', False))
        yield pystray.Menu.SEPARATOR
        yield item("从剪贴板导入节点",
                   lambda icon, it: _safe_js("el.__x.$data.importFromClipboard()"))
        yield item("复制代理命令",
                   lambda icon, it: _safe_js(
                       f"navigator.clipboard.writeText('set http_proxy=http://127.0.0.1:{api.config_local_port} && set https_proxy=http://127.0.0.1:{api.config_local_port}').then(function(){{ el.__x.$data.showToast('代理命令已复制', 'success') }})"
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
    debug_mode = True  # 调试模式：强制开启开发者工具
    webview.start(debug=debug_mode, private_mode=False, storage_path=user_data_dir)


if __name__ == "__main__":
    main()

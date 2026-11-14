"""X-Matrix-Client-pro 本地离线测试（纯标准库，不联网、不启动 GUI）。"""
from __future__ import annotations

import io
import json
import logging
import os
import sys
import tempfile
import threading
import types
import unittest
import zipfile
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from xmatrix.network import proxy as proxy_mod  # noqa: E402
from xmatrix.core import registry as registry_mod  # noqa: E402
from xmatrix.core import config_mihomo as mihomo_mod  # noqa: E402
from xmatrix import constants as constants_mod  # noqa: E402


class FakeKey:
    def __init__(self, store: dict):
        self.store = store

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False


class FakeWinreg:
    HKEY_CURRENT_USER = object()
    KEY_ALL_ACCESS = 1
    REG_SZ = 1
    REG_DWORD = 4

    def __init__(self, store: dict | None = None):
        self.store = dict(store or {})

    def OpenKey(self, root, path, reserved, access):
        return FakeKey(self.store)

    def QueryValueEx(self, key, name):
        if name not in key.store:
            raise FileNotFoundError(name)
        return key.store[name]

    def SetValueEx(self, key, name, reserved, reg_type, value):
        key.store[name] = (value, reg_type)

    def DeleteValue(self, key, name):
        if name not in key.store:
            raise FileNotFoundError(name)
        del key.store[name]


class ProxyApi:
    def __init__(self, pac_port=2078):
        self.sys_proxy_enabled = False
        self.sys_proxy_pac = True
        self.pac_port = pac_port


class ProxyBackupTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.data_patch = mock.patch.object(constants_mod, "DATA_DIR", self.tmp.name)
        self.data_patch.start()
        self.addCleanup(self.data_patch.stop)
        self.notify_patch = mock.patch.object(proxy_mod, "_notify_wininet", lambda: None)
        self.notify_patch.start()
        self.addCleanup(self.notify_patch.stop)
        self.backup_path = os.path.join(self.tmp.name, "proxy_backup.json")

    def _run(self, store, enable=True, use_pac=True):
        fake = FakeWinreg(store)
        old = proxy_mod.winreg
        proxy_mod.winreg = fake
        try:
            api = ProxyApi()
            result = proxy_mod.toggle_system_proxy(api, enable, "2077", use_pac)
        finally:
            proxy_mod.winreg = old
        return fake, result

    def test_a_restore_all_values_and_types(self):
        store = {
            "ProxyEnable": (1, FakeWinreg.REG_DWORD),
            "ProxyServer": ("old.proxy:8080", FakeWinreg.REG_SZ),
            "ProxyOverride": ("old;exceptions", FakeWinreg.REG_SZ),
        }
        fake, result = self._run(store, enable=True)
        self.assertTrue(result["success"])
        self.assertEqual(fake.store["ProxyEnable"], (0, FakeWinreg.REG_DWORD))
        self.assertIn("AutoConfigURL", fake.store)

        fake2, result2 = self._run(fake.store, enable=False)
        self.assertTrue(result2["success"])
        self.assertEqual(fake2.store["ProxyEnable"], (1, FakeWinreg.REG_DWORD))
        self.assertEqual(fake2.store["ProxyServer"], ("old.proxy:8080", FakeWinreg.REG_SZ))
        self.assertEqual(fake2.store["ProxyOverride"], ("old;exceptions", FakeWinreg.REG_SZ))
        self.assertNotIn("AutoConfigURL", fake2.store)
        with open(self.backup_path, encoding="utf-8") as f:
            backup = json.load(f)
        self.assertTrue(backup["restored"])

    def test_b_missing_key_stays_missing(self):
        store = {
            "ProxyEnable": (0, FakeWinreg.REG_DWORD),
            "ProxyServer": ("old.proxy:8080", FakeWinreg.REG_SZ),
            "ProxyOverride": ("old", FakeWinreg.REG_SZ),
        }
        fake, _ = self._run(store, enable=True)
        self.assertIn("AutoConfigURL", fake.store)
        fake2, _ = self._run(fake.store, enable=False)
        self.assertNotIn("AutoConfigURL", fake2.store)

    def test_c_unrestored_backup_is_not_overwritten(self):
        store = {"ProxyEnable": (1, FakeWinreg.REG_DWORD)}
        fake, _ = self._run(store, enable=True)
        with open(self.backup_path, "rb") as f:
            first = f.read()
        # 模拟上次异常退出：restored 仍为 False；再次 enable 不得覆盖快照。
        fake2, _ = self._run(fake.store, enable=True)
        with open(self.backup_path, "rb") as f:
            second = f.read()
        self.assertEqual(first, second)
        backup = json.loads(second.decode("utf-8"))
        self.assertFalse(backup["restored"])
        self.assertIn("ProxyServer", backup["values"])


class FakeResp:
    def __init__(self, data: bytes):
        self.data = data

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False

    def read(self):
        return self.data


class RegistryApi:
    def __init__(self, release: dict, zip_bytes: bytes = b"", dgst_text: str | None = None):
        self.release = release
        self.zip_bytes = zip_bytes
        self.dgst_text = dgst_text
        self.calls: list[str] = []
        self._process_lock = threading.Lock()
        self.xray_process = None
        self.download_url = "https://example.test/xray.zip"
        self.dgst_url = "https://example.test/xray.zip.dgst"

    def _urlopen(self, url, timeout=15, headers=None):
        self.calls.append(url)
        if "api.github.com" in url:
            return FakeResp(json.dumps(self.release).encode("utf-8"))
        if url == self.download_url:
            if not self.zip_bytes:
                raise AssertionError("fail-closed 路径不应下载 zip")
            return FakeResp(self.zip_bytes)
        if url == self.dgst_url and self.dgst_text is not None:
            return FakeResp(self.dgst_text.encode("utf-8"))
        raise AssertionError(f"unexpected url: {url}")

    def stop_core(self):
        pass

    def start_core(self):
        pass


def make_zip() -> bytes:
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as zf:
        zf.writestr("xray.exe", b"fake-xray")
    return buf.getvalue()


class RegistryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.data_patch = mock.patch.object(registry_mod, "DATA_DIR", self.tmp.name)
        self.data_patch.start()
        self.addCleanup(self.data_patch.stop)
        self.base_patch = mock.patch.object(registry_mod, "_BASE", self.tmp.name)
        self.base_patch.start()
        self.addCleanup(self.base_patch.stop)
        self.release = {
            "tag_name": "v1.2.3",
            "assets": [{
                "name": "Xray-windows-64.zip",
                "browser_download_url": "https://example.test/xray.zip",
            }],
        }

    def _write_config(self, allow: bool):
        os.makedirs(self.tmp.name, exist_ok=True)
        with open(os.path.join(self.tmp.name, "config.json"), "w", encoding="utf-8") as f:
            json.dump({"security": {"allow_unsigned_core_update": allow}}, f)

    def test_d_no_dgst_fail_closed_without_download(self):
        api = RegistryApi(self.release, zip_bytes=b"")
        result = registry_mod.update_core(api)
        self.assertFalse(result["success"])
        self.assertIn("fail-closed", result["error"])
        self.assertEqual(api.calls, ["https://api.github.com/repos/XTLS/Xray-core/releases/latest"])
        self.assertFalse(os.path.exists(os.path.join(self.tmp.name, "xmatrix-core.exe")))

    def test_e_allow_unsigned_true_warns_and_updates(self):
        self._write_config(True)
        api = RegistryApi(self.release, zip_bytes=make_zip())
        result = registry_mod.update_core(api)
        self.assertTrue(result["success"], result)
        self.assertIn("warning", result)
        self.assertIn("allow_unsigned_core_update", result["warning"])
        self.assertTrue(os.path.exists(os.path.join(self.tmp.name, "xmatrix-core.exe")))
        self.assertIn(api.download_url, api.calls)

    def test_default_config_missing_is_false(self):
        self.assertFalse(registry_mod._allow_unsigned_core_update())


class MihomoTests(unittest.TestCase):
    def test_warns_for_tun_and_rule_sets(self):
        warnings = mihomo_mod._mihomo_warnings({
            "tun_mode": True,
            "rule_sets": [{"name": "cn"}],
            "dns_rules": [{"domain": "example.com"}],
        })
        joined = "\n".join(warnings)
        self.assertIn("TUN", joined)
        self.assertIn("规则集", joined)
        self.assertIn("DNS", joined)

    def test_build_logs_warning(self):
        class Api:
            tunnels = []
            active_index = -1

        with self.assertLogs(level="WARNING") as cm:
            cfg = mihomo_mod.build_config_mihomo(Api(), tun_mode=True)
        self.assertTrue(any("mihomo" in line for line in cm.output))
        self.assertIn("mixed-port", cfg)

    def test_constants_marks_mihomo_experimental(self):
        self.assertTrue(constants_mod.CORE_REGISTRY["mihomo"]["experimental"])

    def test_get_core_types_exposes_experimental(self):
        class Api:
            active_core = "xray"

        with mock.patch.object(registry_mod, "find_core_exe", lambda _t: None):
            data = registry_mod.get_core_types(Api())
        mihomo = next(c for c in data["cores"] if c["type"] == "mihomo")
        self.assertTrue(mihomo["experimental"])

    def test_settings_js_marks_experimental(self):
        text = (ROOT / "modules" / "settings.js").read_text(encoding="utf-8")
        self.assertIn("core.experimental", text)
        self.assertIn("实验性", text)


class MainSingletonTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # main.py 顶部 import GUI 依赖；测试只加载锁函数，用轻量 stub 避免弹窗/装依赖。
        pystray = types.ModuleType("pystray")
        pystray.Icon = object
        pystray.Menu = object
        pystray.MenuItem = object
        webview = types.ModuleType("webview")
        webview.create_window = lambda *a, **k: None
        webview.start = lambda *a, **k: None
        pil = types.ModuleType("PIL")
        pil.Image = types.SimpleNamespace(new=lambda *a, **k: None, Draw=lambda *a, **k: None)
        pil.ImageDraw = types.SimpleNamespace(Draw=lambda *a, **k: None)
        cls._saved = {k: sys.modules.get(k) for k in ("pystray", "webview", "PIL")}
        sys.modules["pystray"] = pystray
        sys.modules["webview"] = webview
        sys.modules["PIL"] = pil
        import main as main_mod
        cls.main = main_mod

    @classmethod
    def tearDownClass(cls):
        for k, v in cls._saved.items():
            if v is None:
                sys.modules.pop(k, None)
            else:
                sys.modules[k] = v

    def test_f_second_mutex_acquire_fails(self):
        name = f"Local\\XMatrix-Client-pro-test-{os.getpid()}"
        self.main._release_single_instance()
        self.assertEqual(self.main._acquire_windows_mutex(name), "ok")
        self.assertEqual(self.main._acquire_windows_mutex(name), "busy")
        self.main._release_single_instance()
        self.assertEqual(self.main._acquire_windows_mutex(name), "ok")
        self.main._release_single_instance()


if __name__ == "__main__":
    unittest.main(verbosity=2)

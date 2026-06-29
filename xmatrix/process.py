"""
X-Matrix Client — 子进程辅助函数
"""
from __future__ import annotations

import subprocess
from typing import Any

from xmatrix.constants import CREATE_NO_WINDOW


def run_hidden(*args: str, **kwargs: Any) -> subprocess.CompletedProcess:
    """subprocess.run 的封装，自动注入 CREATE_NO_WINDOW。"""
    kwargs.setdefault("creationflags", CREATE_NO_WINDOW)
    kwargs.setdefault("capture_output", True)
    return subprocess.run(list(args), **kwargs)


def popen_hidden(cmd: list[str], **kwargs: Any) -> subprocess.Popen:
    """subprocess.Popen 的封装，自动注入 CREATE_NO_WINDOW。"""
    kwargs.setdefault("creationflags", CREATE_NO_WINDOW)
    return subprocess.Popen(cmd, **kwargs)

"""
X-Matrix Client — 核心生命周期管理
"""
from __future__ import annotations

import ctypes
import json
import logging
import os
import queue
import re
import shutil
import subprocess
import time
import threading
from typing import TYPE_CHECKING

from xmatrix.constants import (
    DATA_DIR, CONFIG_FILE, CONFIG_FILE_CUSTOM, CONFIG_FILE_SINGBOX_CUSTOM,
    CONFIG_FILE_MIHOMO_CUSTOM, CORE_REGISTRY, _BASE,
)
from xmatrix.helpers import deep_merge, load_port_config, atomic_write_json
from xmatrix.process import run_hidden, popen_hidden

if TYPE_CHECKING:
    from xmatrix.api import XMatrixAPI


def find_core_exe(core_type: str) -> str | None:
    """查找指定核心的可执行文件路径。"""
    info = CORE_REGISTRY.get(core_type)
    if not info:
        return None
    for name in info["exe_names"]:
        path = os.path.join(_BASE, name)
        if os.path.isfile(path):
            return path
        found = shutil.which(name)
        if found and os.path.isfile(found):
            return found
    return None


def get_core_version(core_type: str) -> str:
    """获取已安装核心的版本号。返回版本字符串或 'unknown'。"""
    exe_path = find_core_exe(core_type)
    if not exe_path:
        return "not_installed"
    try:
        result = run_hidden(exe_path, "version", text=True, timeout=5)
        if result.returncode == 0:
            output = result.stdout.strip()
            match = re.search(r'v?(\d+\.\d+\.\d+)', output)
            return match.group(0) if match else output[:30]
    except Exception:
        pass
    try:
        result = run_hidden(exe_path, "--version", text=True, timeout=5)
        if result.returncode == 0:
            output = result.stdout.strip()
            match = re.search(r'v?(\d+\.\d+\.\d+)', output)
            return match.group(0) if match else output[:30]
    except Exception:
        pass
    return "unknown"


def attach_job_object(api: XMatrixAPI, proc: subprocess.Popen) -> None:
    """将进程绑定到 Windows Job Object，主进程退出时自动终止子进程。"""
    if os.name != "nt":
        return
    try:
        kernel32 = ctypes.windll.kernel32
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000
        JobObjectExtendedLimitInformation = 9

        class JOBOBJECT_BASIC_LIMIT_INFORMATION(ctypes.Structure):
            _fields_ = [
                ("PerProcessUserTimeLimit", ctypes.c_int64),
                ("PerJobUserTimeLimit", ctypes.c_int64),
                ("LimitFlags", ctypes.c_uint32),
                ("MinimumWorkingSetSize", ctypes.c_size_t),
                ("MaximumWorkingSetSize", ctypes.c_size_t),
                ("ActiveProcessLimit", ctypes.c_uint32),
                ("Affinity", ctypes.c_size_t),
                ("PriorityClass", ctypes.c_uint32),
                ("SchedulingClass", ctypes.c_uint32),
            ]

        class IO_COUNTERS(ctypes.Structure):
            _fields_ = [
                ("ReadOperationCount", ctypes.c_uint64),
                ("WriteOperationCount", ctypes.c_uint64),
                ("OtherOperationCount", ctypes.c_uint64),
                ("ReadTransferCount", ctypes.c_uint64),
                ("WriteTransferCount", ctypes.c_uint64),
                ("OtherTransferCount", ctypes.c_uint64),
            ]

        class JOBOBJECT_EXTENDED_LIMIT_INFORMATION(ctypes.Structure):
            _fields_ = [
                ("BasicLimitInformation", JOBOBJECT_BASIC_LIMIT_INFORMATION),
                ("IoInfo", IO_COUNTERS),
                ("ProcessMemoryLimit", ctypes.c_size_t),
                ("JobMemoryLimit", ctypes.c_size_t),
                ("PeakProcessMemoryUsed", ctypes.c_size_t),
                ("PeakJobMemoryUsed", ctypes.c_size_t),
            ]

        cleanup_job_object(api)

        job = kernel32.CreateJobObjectW(None, None)
        if not job:
            logging.warning(f"[JobObject] 创建失败 PID={proc.pid}")
            return

        info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION()
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE

        ret = kernel32.SetInformationJobObject(
            job, JobObjectExtendedLimitInformation,
            ctypes.byref(info), ctypes.sizeof(info),
        )
        if not ret:
            kernel32.CloseHandle(job)
            logging.warning(f"[JobObject] SetInformationJobObject 失败 PID={proc.pid}")
            return

        kernel32.AssignProcessToJobObject(job, proc._handle)
        api._job_handle = job
        logging.info(f"[JobObject] 已创建并绑定 PID={proc.pid}, handle={job}")
    except Exception as e:
        logging.warning(f"[JobObject] 绑定失败: {e}")


def cleanup_job_object(api: XMatrixAPI) -> None:
    """清理 Windows Job Object handle。"""
    if os.name != "nt":
        return
    if not api._job_handle:
        return
    try:
        kernel32 = ctypes.windll.kernel32
        kernel32.CloseHandle(api._job_handle)
        logging.info(f"[JobObject] 已关闭 handle={api._job_handle}")
    except Exception as e:
        logging.warning(f"[JobObject] 关闭失败: {e}")
    finally:
        api._job_handle = None


def cleanup_tun_device(api: XMatrixAPI) -> None:
    """清理 TUN 设备残留（仅在 tun_mode=True 时执行）。"""
    if not api.tun_mode:
        return
    try:
        if os.name == "nt":
            run_hidden("taskkill", "/f", "/im", "wintun.dll")
            logging.info("[TUN] 已清理 Windows TUN 适配器")
        else:
            result = run_hidden("ip", "link", "delete", "tun0", timeout=5)
            if result.returncode == 0:
                logging.info("[TUN] 已删除 Linux tun0 设备")
    except Exception as e:
        logging.warning(f"[TUN] 设备清理失败: {e}")


def stop_pre_service(api: XMatrixAPI) -> None:
    """停止副核心进程。"""
    if not api.pre_service_proc:
        return
    try:
        if api.pre_service_proc.poll() is None:
            api.pre_service_proc.terminate()
            try:
                api.pre_service_proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                api.pre_service_proc.kill()
                api.pre_service_proc.wait(timeout=3)
        api.pre_service_proc = None
        api.log_queue.put("[副核心] 已停止\n")
    except Exception as e:
        logging.warning(f"[副核心] 停止失败: {e}")
        api.pre_service_proc = None


def get_core_status(api: XMatrixAPI) -> dict:
    """获取核心状态。"""
    with api._process_lock:
        if api.xray_process and api.xray_process.poll() is None:
            return {"running": True, "pid": api.xray_process.pid}
        if api.xray_process:
            api.xray_process = None
    return {"running": False}


def stop_core(api: XMatrixAPI) -> dict:
    """停止核心。"""
    # B-B3/B-B4: 信号停止后台线程
    api._conn_refresh_stop.set()
    api._delay_test_stop.set()
    # 累加当前运行的流量到偏移量，然后保存
    api.stats_offset["up"] = api.stats_offset.get("up", 0) + api.current_stats.get("up", 0)
    api.stats_offset["down"] = api.stats_offset.get("down", 0) + api.current_stats.get("down", 0)
    api.current_stats = {"up": 0, "down": 0}
    api._save_stats()
    with api._process_lock:
        if not api.xray_process:
            # 即使主核心未运行，也尝试停止副核心
            stop_pre_service(api)
            cleanup_job_object(api)
            return {"success": True, "message": "核心未在运行"}
        try:
            if api.xray_process.poll() is None:
                pid = api.xray_process.pid
                # Graceful shutdown: 先 terminate，等待 2 秒，再 force kill
                api.xray_process.terminate()
                try:
                    api.xray_process.wait(timeout=2)
                    logging.info(f"[核心] PID={pid} 已优雅退出")
                except subprocess.TimeoutExpired:
                    logging.info(f"[核心] PID={pid} 未响应 terminate，强制 kill")
                    api.xray_process.kill()
                    api.xray_process.wait(timeout=3)
            api.xray_process = None
            # 先停主核心，再停副核心
            stop_pre_service(api)
            api.toggle_system_proxy(False)
            api.tun_mode = False
            return {"success": True, "message": f"{CORE_REGISTRY.get(api.active_core, {}).get('name', '核心')} 已停止"}
        except Exception:
            api.xray_process = None
            stop_pre_service(api)
            api.toggle_system_proxy(False)
            api.tun_mode = False
            raise
        finally:
            cleanup_job_object(api)
            cleanup_tun_device(api)


def start_core(api: XMatrixAPI) -> dict:
    """启动核心。"""
    # 确保配置文件存在
    core_info = CORE_REGISTRY.get(api.active_core, CORE_REGISTRY["xray"])
    config_ext = core_info.get("config_ext", "json")
    default_config = CONFIG_FILE if config_ext == "json" else os.path.join(DATA_DIR, f"config.{config_ext}")
    if not os.path.exists(default_config):
        api.save_config()

    # 配置优先级选择
    if api.active_core == "singbox":
        custom_path = CONFIG_FILE_SINGBOX_CUSTOM
    elif api.active_core == "mihomo":
        custom_path = CONFIG_FILE_MIHOMO_CUSTOM
    else:
        custom_path = CONFIG_FILE_CUSTOM

    use_custom = False
    if api.config_priority == "custom":
        if os.path.isfile(custom_path):
            use_custom = True
        else:
            return {"success": False, "error": f"自定义配置文件不存在: {os.path.basename(custom_path)}"}
    elif api.config_priority == "smart":
        if os.path.isfile(custom_path):
            use_custom = True

    if use_custom:
        config_file = custom_path
        api.log_queue.put(f"[系统] 使用自定义配置: {os.path.basename(custom_path)}\n")
    else:
        config_file = default_config

    with api._process_lock:
        if api.xray_process and api.xray_process.poll() is None:
            stop_core(api)

    # 查找核心可执行文件
    core_path = find_core_exe(api.active_core)
    if not core_path:
        exe_names = ", ".join(core_info.get("exe_names", []))
        return {"success": False, "error": f"未找到 {core_info['name']} 核心。已搜索：{_BASE}、PATH。需要：{exe_names}"}

    # 杀残留进程
    exe_name = os.path.basename(core_path)
    if os.name == "nt":
        run_hidden("taskkill", "/f", "/im", exe_name)
        if api.tun_mode:
            for tun_name in ("wintun.dll", "wireguard.exe", "sing-box-tun"):
                run_hidden("taskkill", "/f", "/im", tun_name)

    # 清空队列残留
    while not api.log_queue.empty():
        try:
            api.log_queue.get_nowait()
        except queue.Empty:
            break

    # 启动副核心
    _start_pre_service(api)

    # 构建启动命令
    args = [core_path] + [a.format(config=config_file) for a in core_info.get("args_template", ["run", "-c", "{config}"])]

    # 注入核心环境变量
    core_env = os.environ.copy()
    env_vars = core_info.get("env_vars", {})
    if env_vars:
        for k, v in env_vars.items():
            core_env[k] = v
    cwd = os.path.dirname(core_path)
    if api.active_core == "mihomo":
        mihomo_dir = os.path.join(DATA_DIR, "mihomo")
        os.makedirs(mihomo_dir, exist_ok=True)
        cwd = mihomo_dir

    with api._process_lock:
        api.xray_process = popen_hidden(
            args,
            cwd=cwd,
            env=core_env,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1,
        )
        proc = api.xray_process

    # Windows Job Object：确保子进程随主进程退出
    if os.name == "nt":
        attach_job_object(api, proc)

    threading.Thread(target=_log_reader_thread, args=(api, proc), daemon=True).start()
    api.current_stats = {"up": 0, "down": 0}
    threading.Thread(target=_stats_reader_thread, args=(api,), daemon=True).start()

    # B-B3: 连接自动刷新后台线程
    api._conn_refresh_stop.clear()
    threading.Thread(target=_connection_auto_refresh_thread, args=(api,), daemon=True).start()
    # B-B4: 自动延迟测试后台线程
    api._delay_test_stop.clear()
    threading.Thread(target=_auto_delay_test_thread, args=(api,), daemon=True).start()

    return {"success": True, "message": f"{core_info['name']} 核心已启动", "pid": proc.pid}


def _start_pre_service(api: XMatrixAPI) -> None:
    """启动副核心前置代理。"""
    try:
        cfg_path = os.path.join(DATA_DIR, "config.json")
        if not os.path.isfile(cfg_path):
            return
        with open(cfg_path, "r", encoding="utf-8") as f:
            saved = json.load(f)
        if not saved.get("pre_service_enabled", False):
            return
        pre_core = saved.get("pre_service_core", "")
        pre_port = int(saved.get("pre_service_port", 2076))
        if not pre_core or pre_core not in CORE_REGISTRY:
            return
        main_port = api.config_local_port
        if pre_port == main_port or pre_port == load_port_config()["api_port"]:
            logging.warning(f"[副核心] 端口 {pre_port} 与主核心/API 端口冲突，跳过启动")
            return
        pre_exe = find_core_exe(pre_core)
        if not pre_exe:
            logging.warning(f"[副核心] {CORE_REGISTRY[pre_core]['name']} 可执行文件不存在，跳过启动")
            return
        # 生成副核心配置
        pre_config = _build_preservice_config(pre_core, pre_port)
        pre_info = CORE_REGISTRY[pre_core]
        pre_args = [pre_exe] + [a.format(config=pre_config) for a in pre_info.get("args_template", ["run", "-c", "{config}"])]
        pre_env = os.environ.copy()
        for k, v in pre_info.get("env_vars", {}).items():
            pre_env[k] = v
        pre_cwd = os.path.dirname(pre_exe)
        if pre_core == "mihomo":
            mihomo_dir = os.path.join(DATA_DIR, "mihomo")
            os.makedirs(mihomo_dir, exist_ok=True)
            pre_cwd = mihomo_dir
        pre_exe_name = os.path.basename(pre_exe)
        if os.name == "nt":
            run_hidden("taskkill", "/f", "/im", pre_exe_name)
        api.pre_service_proc = popen_hidden(
            pre_args, cwd=pre_cwd, env=pre_env,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1,
        )
        if os.name == "nt":
            attach_job_object(api, api.pre_service_proc)
        threading.Thread(target=_log_reader_thread, args=(api, api.pre_service_proc), daemon=True).start()
        api.log_queue.put(f"[副核心] {pre_info['name']} 已启动，SOCKS 端口: {pre_port}\n")
    except Exception as e:
        logging.warning(f"[副核心] 启动失败: {e}")
        cleanup_job_object(api)


def _build_preservice_config(core_id: str, port: int) -> str:
    """生成副核心配置文件。"""
    config_path = os.path.join(DATA_DIR, "config.preservice.json")
    core_info = CORE_REGISTRY.get(core_id, CORE_REGISTRY["xray"])
    config_ext = core_info.get("config_ext", "json")

    if core_id == "singbox":
        config = {
            "log": {"level": "warn", "timestamp": True},
            "inbounds": [{"type": "mixed", "tag": "preservice-in", "listen": "127.0.0.1", "listen_port": port, "sniff": True}],
            "outbounds": [{"type": "direct", "tag": "direct"}],
            "route": {"rules": [], "final": "direct"},
        }
    elif core_id == "mihomo":
        config = {
            "mixed-port": port,
            "allow-lan": False,
            "mode": "rule",
            "log-level": "warn",
            "external-controller": "",
        }
    else:
        config = {
            "log": {"loglevel": "warning"},
            "inbounds": [{
                "tag": "preservice-in",
                "port": port,
                "listen": "127.0.0.1",
                "protocol": "mixed",
                "sniffing": {"enabled": True, "destOverride": ["http", "tls"]},
                "settings": {"udp": True, "auth": "noauth"},
            }],
            "outbounds": [
                {"tag": "direct", "protocol": "freedom", "settings": {}},
                {"tag": "block", "protocol": "blackhole", "settings": {}},
            ],
            "routing": {"domainStrategy": "AsIs", "rules": []},
        }

    if config_ext == "yaml":
        import yaml
        with open(config_path, "w", encoding="utf-8") as f:
            yaml.dump(config, f, allow_unicode=True, default_flow_style=False, sort_keys=False)
    else:
        with open(config_path, "w", encoding="utf-8") as f:
            json.dump(config, f, ensure_ascii=False, indent=2)
    return config_path


def _log_reader_thread(api: XMatrixAPI, proc: subprocess.Popen) -> None:
    """日志读取线程。"""
    try:
        for line in iter(proc.stdout.readline, ""):
            if not line:
                break
            api.log_queue.put(line)
    except (ValueError, OSError):
        pass
    finally:
        if api.xray_process is proc:
            api.log_queue.put("[系统] 日志管道已断开，核心已退出。\n")


def _stats_reader_thread(api: XMatrixAPI) -> None:
    """统计读取线程。"""
    from xmatrix.monitoring.grpc_client import GrpcStatsClient
    from xmatrix.monitoring.clash_client import SingboxStatsClient

    prev_up, prev_down = 0, 0
    while True:
        with api._process_lock:
            proc = api.xray_process
            if not proc or proc.poll() is not None:
                break
        time.sleep(3)
        use_clash_api = api.active_core in ("singbox", "mihomo")
        try:
            if use_clash_api:
                stats = SingboxStatsClient.query()
            else:
                stats = GrpcStatsClient.query()
            if not stats:
                continue
            up_total = sum(v for n, v in stats.items() if "outbound>>>" in n and n.endswith(">>>uplink"))
            down_total = sum(v for n, v in stats.items() if "outbound>>>" in n and n.endswith(">>>downlink"))
            api.current_speeds = {"up_speed": max(0, up_total - prev_up), "down_speed": max(0, down_total - prev_down)}
            prev_up, prev_down = up_total, down_total
            api.current_stats = {"up": up_total, "down": down_total}
        except (OSError, ValueError):
            time.sleep(1)


def _connection_auto_refresh_thread(api: XMatrixAPI) -> None:
    """连接自动刷新线程。"""
    while not api._conn_refresh_stop.is_set():
        auto_refresh = False
        interval = 2
        try:
            cfg_path = os.path.join(DATA_DIR, "config.json")
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    saved = json.load(f)
                auto_refresh = saved.get("connections_auto_refresh", False)
                interval = saved.get("connections_refresh_interval", 2)
        except Exception:
            pass
        if not auto_refresh:
            api._conn_refresh_stop.wait(5)
            continue
        with api._process_lock:
            proc = api.xray_process
            if not proc or proc.poll() is not None:
                break
        api._conn_refresh_stop.wait(max(1, interval))


def _auto_delay_test_thread(api: XMatrixAPI) -> None:
    """自动延迟测试线程。"""
    while not api._delay_test_stop.is_set():
        interval_min = 0
        try:
            cfg_path = os.path.join(DATA_DIR, "config.json")
            if os.path.isfile(cfg_path):
                with open(cfg_path, "r", encoding="utf-8") as f:
                    interval_min = json.load(f).get("auto_delay_test_interval", 0)
        except Exception:
            pass
        if interval_min <= 0:
            api._delay_test_stop.wait(30)
            continue
        with api._process_lock:
            proc = api.xray_process
            if not proc or proc.poll() is not None:
                break
        api._delay_test_stop.wait(max(60, interval_min * 60))


def activate_tunnel(api: XMatrixAPI, index: int, **kwargs) -> dict:
    """激活节点。"""
    if index != -1 and not api.tunnels:
        return {"success": False, "error": "没有通道数据"}
    if index != -1 and not (0 <= index < len(api.tunnels)):
        return {"success": False, "error": "索引越界"}
    api.active_index = index
    api.proxy_mode = kwargs.get("proxy_mode", "rule")
    api.config_local_port = kwargs.get("local_port", 2077)
    api.tun_mode = kwargs.get("tun_mode", False)
    api.save_config(**kwargs)
    result = start_core(api)
    result["active_index"] = index
    return result


def save_config(api: XMatrixAPI, *args, **kwargs) -> dict:
    """保存配置。"""
    config = api._build_config(*args, **kwargs)
    core_info = CORE_REGISTRY.get(api.active_core, CORE_REGISTRY["xray"])
    config_ext = core_info.get("config_ext", "json")
    config_file = CONFIG_FILE if config_ext == "json" else os.path.join(DATA_DIR, f"config.{config_ext}")

    if config_ext == "yaml":
        import yaml
        with open(config_file, "w", encoding="utf-8") as f:
            yaml.dump(config, f, allow_unicode=True, default_flow_style=False, sort_keys=False)
    else:
        with open(config_file, "w", encoding="utf-8") as f:
            json.dump(config, f, ensure_ascii=False, indent=2)
    return {"success": True, "path": config_file}


def validate_config(api: XMatrixAPI, json_str: str) -> dict:
    """校验配置。"""
    xray_path = find_core_exe("xray")
    if not xray_path:
        return {"success": False, "error": "未找到 xmatrix-core.exe"}

    import tempfile
    tmp = None
    try:
        try:
            json.loads(json_str)
            clean_json = json_str
        except json.JSONDecodeError:
            clean_json = re.sub(r'(?<!:)//.*', '', json_str)
            clean_json = re.sub(r'/\*.*?\*/', '', clean_json, flags=re.DOTALL)

        tmp = tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False, encoding="utf-8")
        tmp.write(clean_json)
        tmp.close()

        proc = run_hidden(
            xray_path, "run", "-test", "-c", tmp.name,
            text=True, timeout=10, cwd=os.path.dirname(xray_path),
        )
        if proc.returncode == 0:
            return {"success": True, "valid": True, "message": "配置校验通过 ✓"}
        return {"success": True, "valid": False,
                "message": proc.stderr.strip() or proc.stdout.strip() or "校验失败（未知错误）"}
    except subprocess.TimeoutExpired:
        return {"success": False, "error": "校验超时（>10s）"}
    finally:
        if tmp and os.path.exists(tmp.name):
            os.unlink(tmp.name)


def save_raw_config(api: XMatrixAPI, json_str: str) -> dict:
    """保存原始配置。"""
    try:
        json.loads(json_str)
        clean_json = json_str
    except json.JSONDecodeError:
        clean_json = re.sub(r'(?<!:)//.*', '', json_str)
        clean_json = re.sub(r'/\*.*?\*/', '', clean_json, flags=re.DOTALL)

    try:
        parsed = json.loads(clean_json)
        with open(CONFIG_FILE, "w", encoding="utf-8") as f:
            json.dump(parsed, f, ensure_ascii=False, indent=2)
        return {"success": True}
    except Exception as e:
        return {"success": False, "error": f"JSON 解析失败: {str(e)}"}


def save_port_config(api: XMatrixAPI, ports: dict) -> dict:
    """保存端口配置。"""
    valid_keys = {"api_port", "clash_api_port", "tun_address", "local_port", "pac_port", "speedtest_base_port"}
    filtered = {k: v for k, v in ports.items() if k in valid_keys}
    for key in ("api_port", "clash_api_port", "local_port", "pac_port", "speedtest_base_port"):
        if key in filtered:
            val = int(filtered[key])
            if not (1024 <= val <= 65535):
                return {"success": False, "error": f"{key} 端口必须在 1024-65535 范围内"}
            filtered[key] = val
    cfg_path = os.path.join(DATA_DIR, "config.json")
    existing = {}
    try:
        if os.path.isfile(cfg_path):
            with open(cfg_path, "r", encoding="utf-8") as f:
                existing = json.load(f)
    except Exception:
        pass
    existing.setdefault("ports", {}).update(filtered)
    atomic_write_json(cfg_path, existing)
    if "local_port" in filtered:
        api.config_local_port = filtered["local_port"]
    if "pac_port" in filtered:
        api.pac_port = filtered["pac_port"]
    return {"success": True, "ports": load_port_config()}


def save_frontend_settings(api: XMatrixAPI, settings: dict) -> dict:
    """保存前端设置。"""
    cfg_path = os.path.join(DATA_DIR, "config.json")
    existing: dict = {}
    try:
        if os.path.isfile(cfg_path):
            with open(cfg_path, "r", encoding="utf-8") as f:
                existing = json.load(f)
    except Exception:
        pass

    nd = existing.setdefault("node_defaults", {})
    if "def_fingerprint" in settings:
        nd["def_fingerprint"] = settings["def_fingerprint"]
    if "def_user_agent" in settings:
        nd["def_user_agent"] = settings["def_user_agent"]

    if "mux_xudp_concurrency" in settings:
        existing["mux_xudp_concurrency"] = settings["mux_xudp_concurrency"]
    if "mux_xudp_proxy_udp443" in settings:
        existing["mux_xudp_proxy_udp443"] = settings["mux_xudp_proxy_udp443"]

    if "system_proxy_exceptions" in settings:
        existing["system_proxy_exceptions"] = settings["system_proxy_exceptions"]

    dns = existing.setdefault("dns", {})
    if "strategy4proxy" in settings:
        dns["strategy4proxy"] = settings["strategy4proxy"]
    if "strategy4freedom" in settings:
        dns["strategy4freedom"] = settings["strategy4freedom"]

    if "tun_ipv6" in settings:
        existing.setdefault("ports", {})["tun_ipv6"] = settings["tun_ipv6"]

    if "geo_source_urls" in settings:
        existing["geo_source_urls"] = settings["geo_source_urls"]

    if "core_type_map" in settings:
        existing["core_type_map"] = settings["core_type_map"]

    if "pre_service_enabled" in settings:
        existing["pre_service_enabled"] = settings["pre_service_enabled"]
    if "pre_service_core" in settings:
        existing["pre_service_core"] = settings["pre_service_core"]

    if "connections_auto_refresh" in settings:
        existing["connections_auto_refresh"] = settings["connections_auto_refresh"]
    if "connections_refresh_interval" in settings:
        existing["connections_refresh_interval"] = settings["connections_refresh_interval"]

    if "auto_delay_test_interval" in settings:
        existing["auto_delay_test_interval"] = settings["auto_delay_test_interval"]

    if "speed_ping_test_url" in settings:
        existing["speed_ping_test_url"] = settings["speed_ping_test_url"]

    if "enable_hwa" in settings:
        existing["enable_hwa"] = settings["enable_hwa"]

    ports = existing.setdefault("ports", {})
    if "api_port" in settings:
        ports["api_port"] = int(settings["api_port"])
    if "clash_api_port" in settings:
        ports["clash_api_port"] = int(settings["clash_api_port"])
    if "tun_address" in settings:
        ports["tun_address"] = settings["tun_address"]

    if "second_socks_enabled" in settings:
        existing["second_socks_enabled"] = bool(settings["second_socks_enabled"])
    if "second_port" in settings:
        ports["second_port"] = int(settings["second_port"])
    if "use_system_hosts" in settings:
        existing["use_system_hosts"] = bool(settings["use_system_hosts"])
    if "check_prerelease" in settings:
        existing["check_prerelease"] = bool(settings["check_prerelease"])
    if "tun_include_apps" in settings:
        existing["tun_include_apps"] = settings["tun_include_apps"]

    atomic_write_json(cfg_path, existing)
    return {"success": True}


def toggle_auto_startup(enable: bool) -> dict:
    """切换开机自启。"""
    import sys
    import winreg
    exe_path = sys.executable if getattr(sys, "frozen", False) else os.path.abspath(sys.argv[0])

    if os.name == "nt":
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER,
                            r"Software\Microsoft\Windows\CurrentVersion\Run",
                            0, winreg.KEY_ALL_ACCESS) as key:
            if enable:
                winreg.SetValueEx(key, "X-Matrix", 0, winreg.REG_SZ, f'"{exe_path}"')
            else:
                try:
                    winreg.DeleteValue(key, "X-Matrix")
                except FileNotFoundError:
                    pass
    return {"success": True, "enabled": enable, "platform": sys.platform}

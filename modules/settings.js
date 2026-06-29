/**
 * X-Matrix — 设置模块
 * 设置面板、核心管理、备份恢复、订阅管理、策略组、WebDAV
 */
window.XMatrixSettings = {
        sysInfo: {
          os: '正在读取...',
          autoStart: false,
          mode: '-',
          version: '...',
          isUpdatingCore: false
        },

  downloadingCore: null,
  dlProgress: { active: false, percent: 0, total: 0, downloaded: 0, message: '', done: false },

  // 策略组编辑器
  isGroupModalOpen: false,
  groupModalMode: 'add',
  groupEditorId: null,
  groupEditor: { name: '', strategy: 'leastPing', childIds: [], filterRegex: '' },
  filteredGroupNodes: [],

  // 订阅管理
  isSubModalOpen: false,
  subscriptions: [],
  subEditor: { name: '', urls: '', ua: '', interval_hours: 24, filter: '', subconverter: '', targetFormat: 'clash', editingIdx: -1 },

openGroupModal(mode, group = null) {
          this.groupModalMode = mode;
          if (mode === 'edit' && group) {
            this.groupEditorId = group.id;
            this.groupEditor = {
              name: group.out_tag || '',
              strategy: group.group_strategy || 'leastPing',
              childIds: [...(group.child_ids || [])],
              filterRegex: group.filter || ''
            };
          } else {
            this.groupEditorId = null;
            this.groupEditor = { name: '', strategy: 'leastPing', childIds: [], filterRegex: '' };
          }
          this.filterGroupNodes();
          this.isGroupModalOpen = true;
        },
        filterGroupNodes() {
          const regex = this.groupEditor.filterRegex;
          const all = this.tunnels.filter(t => t.protocol !== 'policy_group');
          if (!regex) { this.filteredGroupNodes = all; return; }
          try {
            const re = new RegExp(regex, 'i');
            this.filteredGroupNodes = all.filter(t => re.test(t.out_tag || '') || re.test(t.server_addr || ''));
          } catch { this.filteredGroupNodes = all; }
        },
        groupSelectAll() { this.groupEditor.childIds = this.filteredGroupNodes.map(n => n.id); },
        groupSelectNone() { this.groupEditor.childIds = []; },
        groupSelectInvert() {
          const ids = new Set(this.groupEditor.childIds);
          this.groupEditor.childIds = this.filteredGroupNodes.filter(n => !ids.has(n.id)).map(n => n.id);
        },
        async saveGroupModal() {
          const { name, strategy, childIds, filterRegex } = this.groupEditor;
          if (!name.trim()) return this.showToast('请填写组名称', 'warning');
          if (childIds.length < 2) return this.showToast('请至少选择 2 个节点', 'warning');
          try {
            let res;
            if (this.groupModalMode === 'add') {
              res = await window.pywebview.api.add_policy_group(name, strategy, childIds, filterRegex);
            } else {
              res = await window.pywebview.api.update_policy_group(this.groupEditorId, name, strategy, childIds, filterRegex);
            }
            if (res && res.success) {
              this.syncTunnels(res.tunnels);
              this.isGroupModalOpen = false;
              this.showToast('✅ 策略组已保存', 'success');
            } else {
              this.showToast(('失败: ') + (res ? res.error : ''), 'error');
            }
          } catch (e) { this.showToast(e.message, 'error'); }
        },
        // ── 订阅管理 ──
        async loadSubscriptions() {
          try {
            const res = await window.pywebview.api.get_subscriptions();
            if (res && res.success) this.subscriptions = res.subscriptions || [];
          } catch (e) { console.warn('[订阅] 加载订阅列表失败:', e); }
        },
        async openSubModal() {
          await this.loadSubscriptions();
          this.isSubModalOpen = true;
        },
        async addSubscription() {
          const urls = (this.subEditor.urls || '').split('\n').map(u => u.trim()).filter(Boolean);
          if (!urls.length) return this.showToast('请填写至少一个链接', 'warning');
          let added = 0;
          for (const url of urls) {
            try {
              const res = await window.pywebview.api.add_subscription(
                this.subEditor.name || '', url,
                this.subEditor.ua || '', parseInt(this.subEditor.interval_hours) || 0,
                this.subEditor.filter || '', this.subEditor.subconverter || '',
                this.subEditor.targetFormat || 'clash'
              );
              if (res && res.success) { this.subscriptions = res.subscriptions || []; added++; }
            } catch (e) { console.warn('[订阅] 添加订阅失败:', e); }
          }
          this.subEditor = { name: '', urls: '', ua: '', interval_hours: 24, filter: '', subconverter: '', targetFormat: 'clash', editingIdx: -1 };
          this.showToast(('✅ 已添加 ') + added + (' 个订阅'), 'success');
        },
        editSubscription(idx) {
          const s = this.subscriptions[idx];
          this.subEditor = { name: s.name || '', urls: s.url || '', ua: s.ua || '', interval_hours: s.interval_hours || 24, filter: s.filter_regex || '', subconverter: s.subconverter_url || '', targetFormat: s.target_format || 'clash', editingIdx: idx };
        },
        async saveSubscription() {
          const idx = this.subEditor.editingIdx;
          if (idx < 0 || idx >= this.subscriptions.length) return;
          const sub = this.subscriptions[idx];
          const url = (this.subEditor.urls || '').split('\n')[0].trim();
          try {
            const res = await window.pywebview.api.update_subscription(
              sub.id, this.subEditor.name, url,
              this.subEditor.ua, parseInt(this.subEditor.interval_hours) || 0,
              this.subEditor.filter, null, this.subEditor.subconverter || '',
              this.subEditor.targetFormat || 'clash'
            );
            if (res && res.success) {
              this.subscriptions = res.subscriptions || [];
              this.showToast('✅ 已更新', 'success');
            }
          } catch (e) { this.showToast(e.message, 'error'); }
          this.subEditor = { name: '', urls: '', ua: '', interval_hours: 24, filter: '', subconverter: '', targetFormat: 'clash', editingIdx: -1 };
        },
        async deleteSubscription(idx) {
          const sub = this.subscriptions[idx];
          if (!sub) return;
          try {
            const res = await window.pywebview.api.delete_subscription(sub.id);
            if (res && res.success) {
              this.subscriptions = res.subscriptions || [];
              this.showToast('✅ 已删除', 'success');
            }
          } catch (e) { this.showToast(e.message, 'error'); }
        },
        async toggleSubscription(idx) {
          const sub = this.subscriptions[idx];
          if (!sub) return;
          const newState = sub.enabled === false;
          try {
            const res = await window.pywebview.api.update_subscription(sub.id, '', '', '', -1, null, newState);
            if (res && res.success) {
              this.subscriptions = res.subscriptions || [];
            }
          } catch (e) { this.showToast(e.message, 'error'); }
        },
        async refreshSubscription(idx) {
          if (idx === -1) {
            // 全部刷新
            this.showToast('正在刷新全部订阅...', 'info');
            try {
              const res = await window.pywebview.api.refresh_subscription();
              if (res && res.success) {
                await this.loadSubscriptions();
                this.showToast('✅ 全部刷新完成', 'success');
              } else {
                this.showToast(('刷新失败: ') + (res ? res.error : ''), 'error');
              }
            } catch (e) { this.showToast(e.message, 'error'); }
            return;
          }
          const sub = this.subscriptions[idx];
          if (!sub) return;
          this.showToast('正在刷新...', 'info');
          try {
            const res = await window.pywebview.api.refresh_subscription(sub.id);
            if (res && res.success) {
              await this.loadSubscriptions();
              this.showToast('✅ 已刷新', 'success');
            } else {
              this.showToast(('刷新失败: ') + (res ? res.error : ''), 'error');
            }
          } catch (e) { this.showToast(e.message, 'error'); }
        },
        // ── WebDAV ──
        webdavConfig: _safeJsonParse('xmatrix_webdav', {url: '', user: '', pass: ''}),
        saveWebdavConfig() {
          localStorage.setItem('xmatrix_webdav', JSON.stringify(this.webdavConfig));
        },
        _buildWebdavUrl() {
          const { url, user, pass } = this.webdavConfig;
          if (!url) return '';
          if (!user) return url;
          // 把 user:pass 嵌入 URL: https://host/path → https://user:pass@host/path
          try {
            const u = new URL(url);
            u.username = user;
            u.password = pass;
            return u.toString();
          } catch { return url; }
        },
        async webdavBackup() {
          if (!this.webdavConfig.url) return this.showToast('请填写 WebDAV 地址', 'warning');
          this.saveWebdavConfig();
          try {
            const res = await window.pywebview.api.webdav_backup(this._buildWebdavUrl());
            if (res && res.success) this.showToast('✅ 已备份到 WebDAV', 'success');
            else this.showToast(('备份失败: ') + (res ? res.error : ''), 'error');
          } catch (e) { this.showToast(e.message, 'error'); }
        },
        async webdavRestore() {
          if (!this.webdavConfig.url) return this.showToast('请填写 WebDAV 地址', 'warning');
          if (!confirm('确定从 WebDAV 恢复？当前数据将被覆盖。')) return;
          this.saveWebdavConfig();
          try {
            const res = await window.pywebview.api.webdav_restore(this._buildWebdavUrl());
            if (res && res.success) {
              this.showToast('✅ 已从 WebDAV 恢复，正在重载...', 'success');
              setTimeout(() => location.reload(), 1500);
            } else {
              this.showToast(('恢复失败: ') + (res ? res.error : ''), 'error');
            }
          } catch (e) { this.showToast(e.message, 'error'); }

        },
        saveSettings() {
          localStorage.setItem('xmatrix_auto_activate', JSON.stringify(this.settingsState.autoActivateNode));
          localStorage.setItem('xmatrix_silent_start', JSON.stringify(this.settingsState.silentStart));
          localStorage.setItem('xmatrix_close_behavior', this.settingsState.closeBehavior);
          localStorage.setItem('xmatrix_keep_older_dedup', JSON.stringify(this.settingsState.keepOlderDedup));
          localStorage.setItem('xmatrix_delay_test_url', this.settingsState.delayTestUrl);
          localStorage.setItem('xmatrix_download_test_url', this.settingsState.downloadTestUrl);
          localStorage.setItem('xmatrix_test_timeout', this.settingsState.testTimeout.toString());
          localStorage.setItem('xmatrix_test_concurrency', this.settingsState.testConcurrency.toString());
          localStorage.setItem('xmatrix_theme', this.settingsState.theme);
          localStorage.setItem('xmatrix_tray_limit', this.settingsState.trayLimit.toString());
          localStorage.setItem('xmatrix_ipinfo_token', this.settingsState.ipinfoToken);
          localStorage.setItem('xmatrix_def_fingerprint', this.settingsState.defFingerprint);
          localStorage.setItem('xmatrix_def_user_agent', this.settingsState.defUserAgent);
          localStorage.setItem('xmatrix_proxy_exceptions', this.settingsState.proxyExceptions);
          localStorage.setItem('xmatrix_enable_hwa', JSON.stringify(this.settingsState.enableHwa));
          localStorage.setItem('xmatrix_config_priority', this.settingsState.configPriority);
          localStorage.setItem('xmatrix_core_type_map', JSON.stringify(this.settingsState.coreTypeMap));
          localStorage.setItem('xmatrix_pre_service_enabled', JSON.stringify(this.settingsState.preServiceEnabled));
          localStorage.setItem('xmatrix_pre_service_core', this.settingsState.preServiceCore);
          localStorage.setItem('xmatrix_conn_auto_refresh', JSON.stringify(this.settingsState.connectionsAutoRefresh));
          localStorage.setItem('xmatrix_conn_refresh_interval', this.settingsState.connectionsRefreshInterval.toString());
          localStorage.setItem('xmatrix_auto_delay_test_interval', this.settingsState.autoDelayTestInterval);
          localStorage.setItem('xmatrix_check_prerelease', JSON.stringify(this.settingsState.checkPrerelease));
          this.applyTheme();
          if (window.pywebview && window.pywebview.api) {
            window.pywebview.api.save_frontend_settings({
              def_fingerprint: this.settingsState.defFingerprint || '',
              def_user_agent: this.settingsState.defUserAgent || '',
              system_proxy_exceptions: this.settingsState.proxyExceptions || '',
              enable_hwa: !!this.settingsState.enableHwa,
              core_type_map: this.settingsState.coreTypeMap || {},
              pre_service_enabled: !!this.settingsState.preServiceEnabled,
              pre_service_core: this.settingsState.preServiceCore || '',
              connections_auto_refresh: !!this.settingsState.connectionsAutoRefresh,
              connections_refresh_interval: parseInt(this.settingsState.connectionsRefreshInterval) || 2,
              auto_delay_test_interval: parseInt(this.settingsState.autoDelayTestInterval) || 0,
              speed_ping_test_url: this.settingsState.delayTestUrl || '',
              check_prerelease: !!this.settingsState.checkPrerelease,
              geo_source_urls: { 'geoip.dat': this.settingsState.geoipUrl || '', 'geosite.dat': this.settingsState.geositeUrl || '', 'geosite-geolocation-!cn.srs': this.settingsState.srsUrl || '' },
              strategy4proxy: this.configState.strategy4proxy || '',
              strategy4freedom: this.configState.strategy4freedom || '',
              tun_ipv6: !!this.configState.tunIpv6,
              tun_include_apps: this.configState.tunIncludeApps || '',
              use_system_hosts: !!this.configState.useSystemHosts,
              mux_xudp_concurrency: parseInt(this.configState.muxXudpConcurrency) || 8,
              mux_xudp_proxy_udp443: this.configState.muxXudpProxyUdp443 || 'reject',
              second_socks_enabled: !!this.configState.secondSocksEnabled,
              second_port: parseInt(this.configState.secondPort) || 0
            }).catch(() => {});
          }
        },
        applyTheme() {
          const theme = this.settingsState.theme;
          const mq = window.matchMedia('(prefers-color-scheme: dark)');
          const apply = () => {
            const isDark = theme === 'dark' || (theme === 'system' && mq.matches);
            document.documentElement.classList.toggle('dark', isDark);
          };
          mq.removeEventListener('change', this._themeHandler);
          if (theme === 'system') {
            this._themeHandler = apply;
            mq.addEventListener('change', this._themeHandler);
          }
          apply();
        },
        async testDownloadSpeed() {
          if (!this.settingsState.downloadTestUrl) return this.showToast('请先填写测速文件 URL', 'warning');
          this.showToast('正在测速，请稍候...', 'info');
          try {
            const res = await window.pywebview.api.test_download_speed(this.settingsState.downloadTestUrl, this.configState.localPort);
            if (res && res.success) {
              this.showToast(`✅ 下载速度: ${res.speed_mbps} Mbps (${res.size_mb} MB / ${res.elapsed}s)`, 'success');
            } else {
              this.showToast(('测速失败: ') + ((res && res.error) || ('未知错误')), 'error');
            }
          } catch (e) {
            this.showToast(('通信异常: ') + e.message, 'error');
          }
        },
        async selectWallpaper() {
          try {
            const res = await window.pywebview.api.select_wallpaper();
            if (res && res.success) {
              this.wallpaperPath = res.wallpaper;
              this.showToast('✨ 幻彩壁纸已应用', 'success');
            }
          } catch (e) {
            this.showToast('选择壁纸异常', 'error');
          }
        },
        clearWallpaper() {
          this.wallpaperPath = '';
          this.showToast('壁纸已清除', 'info');
        },
        clearLocalCache() {
          if (!confirm('确定要清理本地缓存吗？\n将清除：自定义测试网站、路由规则持久化、设置项缓存。\n不会清除：节点数据（由后端管理）。')) return;
          const keysToKeep = ['xmatrix_last_node', 'xmatrix_proxy_mode', 'xmatrix_tun_mode', 'xmatrix_config_state', 'xmatrix_adv_dns', 'xmatrix_adv_fakedns', 'xmatrix_adv_log', 'xmatrix_auto_activate', 'xmatrix_silent_start', 'xmatrix_close_behavior', 'xmatrix_delay_test_url', 'xmatrix_download_test_url', 'xmatrix_test_timeout', 'xmatrix_test_concurrency', 'xmatrix_theme', 'xmatrix_language', 'xmatrix_display'];
          const allKeys = Object.keys(localStorage).filter(k => k.startsWith('xmatrix_'));
          allKeys.forEach(k => { if (!keysToKeep.includes(k)) localStorage.removeItem(k); });
          this.showToast('✅ 本地缓存已清理，页面将刷新', 'success');
          setTimeout(() => location.reload(), 1200);
        },
        async exportBackup() {
          try {
            const res = await window.pywebview.api.export_backup();
            if (res && res.success) {
              this.showToast('✅ 备份已导出！', 'success');
            } else {
              this.showToast(('备份失败：') + ((res && res.error) || '未知错误'), 'error');
            }
          } catch (e) {
            this.showToast(('通信异常：') + e.message, 'error');
          }
        },
        async importBackup() {
          if (!confirm('确定要从备份恢复吗？\n当前数据会在恢复前自动备份。')) return;
          try {
            const res = await window.pywebview.api.import_backup();
            if (res && res.success) {
              this.showToast('✅ 备份已恢复，正在重新加载...', 'success');
              setTimeout(() => location.reload(), 1500);
            } else {
              this.showToast(('恢复失败：') + ((res && res.error) || '未知错误'), 'error');
            }
          } catch (e) {
            this.showToast(('通信异常：') + e.message, 'error');
          }
        },
        async updateGeoData() {
          this.showToast('正在下载最新 Geo 数据库...', 'info');
          const pollId = setInterval(async () => {
            try { const p = await window.pywebview.api.get_download_progress(); if (p && p.success) this.dlProgress = p; } catch (e) { /* 轮询静默 */ }
          }, 500);
          try {
            const res = await window.pywebview.api.update_geo_data();
            clearInterval(pollId);
            try { const p = await window.pywebview.api.get_download_progress(); if (p && p.success) this.dlProgress = p; } catch (e) { /* 最终刷新静默 */ }
            if (res && res.success) {
              this.showToast('✅ Geo 数据库更新完成！', 'success');
            } else {
              this.showToast(('更新失败：') + ((res && res.error) || ('未知错误')), 'error');
            }
          } catch (e) {
            clearInterval(pollId);
            this.showToast(('通信异常：') + e.message, 'error');
          } finally {
            setTimeout(() => { this.dlProgress = { active: false, percent: 0, total: 0, downloaded: 0, message: '', done: false }; }, 3000);
          }
        },
        async updateCore() {
          this.sysInfo.isUpdatingCore = true;
          try {
            const res = await window.pywebview.api.check_core_update();
            if (res && res.success && res.cores) {
              const active = res.cores.find(c => c.has_update);
              if (active) {
                this.showToast('正在更新 ' + active.type + '...', 'info');
                await this.downloadCore(active.type);
              } else {
                this.showToast('✅ 当前核心已是最新版本', 'success');
              }
            } else {
              this.showToast('检查更新失败: ' + ((res && res.error) || '未知错误'), 'error');
            }
          } catch (e) {
            this.showToast('更新异常: ' + e.message, 'error');
          } finally {
            this.sysInfo.isUpdatingCore = false;
          }
        },
        async toggleCore() {
          if (this.coreState.isRunning) {
            const res = await window.pywebview.api.stop_core();
            if (res && res.success) {
              this.coreState.isRunning = false;
              this.addLog('warning', '手动触发：停止核心');
              this.tunnels.forEach(t => t.active = false);
              this.refreshIpInfo();
            } else {
              this.showToast('停止核心失败: ' + ((res && res.error) || '未知错误'), 'error');
            }
          } else {
            const activeRules = this.getActiveRules();
            let targetIdx = -1;
            if (this.settingsState.autoActivateNode && this.tunnels.length > 0) {
              const savedIdx = localStorage.getItem('xmatrix_last_node');
              targetIdx = savedIdx !== null ? parseInt(savedIdx) : 0;
              if (targetIdx >= this.tunnels.length) targetIdx = 0;
            }
            const res = await window.pywebview.api.activate_tunnel(...this._buildCoreParams(targetIdx, activeRules));
            if (res && res.success) {
              this.coreState.isRunning = true;
              this.tunnels.forEach((t, i) => t.active = (i === targetIdx));
              this.addLog('info', targetIdx === -1 ? ('核心已启动 (全局直连模式待命)') : ('核心已启动并连接至代理节点'));
              this.refreshIpInfo();
            } else {
              this.showToast('启动核心失败: ' + ((res && res.error) || '未知错误'), 'error');
            }
          }
        },
        async setProxyModeFromTray(mode) {
          this.proxyMode = mode;
          if (this.coreState.isRunning) {
            await this.activateNode(-2);
          }
        },
        async setSysProxyModeFromTray(mode) {
          this.settingsState.sysProxyMode = mode;
          if (this.systemProxy) {
            const res = await window.pywebview.api.toggle_system_proxy(true, this.configState.localPort.toString(), mode === 'pac');
            if (res && res.success) {
              this.addLog('info', `[代理] 代理接管模式热切换为 ${res.mode}`);
            }
          }
          this.showToast(`代理模式已切换为 ${mode === 'pac' ? '智能 PAC' : '强制全局'}`, 'success');
        },
        async toggleSystemProxy() {
          if (this.systemProxy && !this.coreState.isRunning) {
            await this.toggleCore();
          }
          const res = await window.pywebview.api.toggle_system_proxy(this.systemProxy, this.configState.localPort.toString(), this.settingsState.sysProxyMode === 'pac');
          if (!res || !res.success) {
            this.systemProxy = !this.systemProxy;
            this.showToast('系统代理切换失败', 'error');
          } else {
            this.addLog('info', this.systemProxy ? '[代理] 系统代理已开启 (' + res.mode + ' 模式)' : '[代理] 系统代理已关闭');
          }
        },
        async toggleSysProxyFromTray() {
          this.systemProxy = !this.systemProxy;
          await this.toggleSystemProxy();
        },
        captureHotkey(e, field) {
          e.preventDefault();
          const keys = [];
          if (e.ctrlKey) keys.push('ctrl');
          if (e.altKey) keys.push('alt');
          if (e.shiftKey) keys.push('shift');
          if (e.metaKey) keys.push('meta');
          const key = e.key.toLowerCase();
          if (!['control', 'alt', 'shift', 'meta'].includes(key)) keys.push(key);
          const combo = keys.join('+');
          if (field === 'window') {
            this.settingsState.hotkeyWindow = combo;
          } else if (field === 'proxy') {
            this.settingsState.hotkeyProxy = combo;
          }
          this.saveSettings();
        },
        // ── 核心管理方法 ──
        async loadCoreTypes() {
          try {
            console.log('[CoreTypes] 开始加载核心列表...');
            var res = await window.pywebview.api.get_core_types();
            console.log('[CoreTypes] 后端返回:', JSON.stringify(res));
            if (res && res.success) {
              this.coreTypes = res.cores;
              console.log('[CoreTypes] coreTypes 已更新, 长度:', this.coreTypes.length);
            } else {
              console.warn('[CoreTypes] 后端返回失败或格式不对:', res);
              // fallback：用静态数据兜底，保证卡片可见
              if (!this.coreTypes.length) {
                this.coreTypes = [
                  { type: 'xray', name: 'Xray', available: false, active: true },
                  { type: 'singbox', name: 'sing-box', available: false, active: false },
                  { type: 'mihomo', name: 'mihomo', available: false, active: false },
                  { type: 'clash', name: 'Clash', available: false, active: false },
                  { type: 'v2ray', name: 'V2Ray', available: false, active: false },
                  { type: 'hysteria', name: 'Hysteria', available: false, active: false },
                  { type: 'naiveproxy', name: 'NaiveProxy', available: false, active: false },
                  { type: 'tuic', name: 'TUIC', available: false, active: false },
                  { type: 'brook', name: 'Brook', available: false, active: false },
                ];
              }
            }
          } catch (e) {
            console.error('[CoreTypes] 加载核心列表异常:', e);
            // fallback
            if (!this.coreTypes.length) {
              this.coreTypes = [
                { type: 'xray', name: 'Xray', available: false, active: true },
                { type: 'singbox', name: 'sing-box', available: false, active: false },
                { type: 'mihomo', name: 'mihomo', available: false, active: false },
              ];
            }
          }
        },
        async checkCoreUpdates() {
          try {
            const res = await window.pywebview.api.check_core_update();
            if (res && res.success && res.cores) {
              for (const info of res.cores) {
                const core = this.coreTypes.find(c => c.type === info.type);
                if (core) {
                  core.version = info.current || '';
                  core.latest = info.latest || '';
                  core.has_update = info.has_update || false;
                }
              }
            }
          } catch (e) { console.warn('[CoreUpdate] check failed:', e); }
        },
        async downloadCore(coreType) {
          this.downloadingCore = coreType;
          var coreName = this.coreTypes.find(c => c.type === coreType);
          this.showToast('正在下载 ' + (coreName ? coreName.name : coreType) + '，请稍候…', 'info');
          // 启动进度轮询
          const pollId = setInterval(async () => {
            try { const p = await window.pywebview.api.get_download_progress(); if (p && p.success) this.dlProgress = p; } catch (e) { /* 轮询静默 */ }
          }, 500);
          try {
            var res = await window.pywebview.api.download_core(coreType);
            clearInterval(pollId);
            // 最终刷新一次进度
            try { const p = await window.pywebview.api.get_download_progress(); if (p && p.success) this.dlProgress = p; } catch (e) { /* 最终刷新静默 */ }
            if (res && res.success) {
              this.showToast('✅ 下载完成', 'success');
              await this.loadCoreTypes();
            } else {
              this.showToast(('下载失败: ') + (res ? res.error : ''), 'error');
            }
          } catch (e) {
            clearInterval(pollId);
            this.showToast(('下载异常: ') + e.message, 'error');
          } finally {
            this.downloadingCore = null;
            setTimeout(() => { this.dlProgress = { active: false, percent: 0, total: 0, downloaded: 0, message: '', done: false }; }, 3000);
          }
        },
        async switchCore(coreType) {
          const core = this.coreTypes.find(c => c.type === coreType);
          // 未安装 → 弹确认框提示下载
          if (core && !core.available) {
            const msg = `${core.name} 尚未安装，是否立即下载？`;
            if (!confirm(msg)) return;
            await this.downloadCore(coreType);
            // 下载后重新检查
            const updated = this.coreTypes.find(c => c.type === coreType);
            if (!updated || !updated.available) return;
          }
          try {
            var res = await window.pywebview.api.set_active_core(coreType);
            if (res && res.success) {
              this.showToast('✅ 已切换至 ' + coreType, 'success');
              await this.loadCoreTypes();
              // 刷新高级视图配置预览
              try {
                const activeRules = this.getActiveRules();
                const preview = await window.pywebview.api.preview_config(...this._buildCoreParams(-1, activeRules));
                if (preview) this.advancedState.codeOutbounds = preview;
              } catch (e) { console.warn('[核心] 切换后刷新配置预览失败:', e); }
            } else {
              this.showToast(('切换失败: ') + (res ? res.error : ''), 'error');
            }
          } catch (e) {
            this.showToast(('切换异常: ') + e.message, 'error');
          }
        },

        // ── 下载控制方法 ──
        async pauseDownload() {
          try {
            const res = await window.pywebview.api.pause_download();
            if (res && res.success) {
              this.dlProgress.paused = true;
              this.showToast('下载已暂停', 'info');
            }
          } catch (e) {
            this.showToast('暂停失败: ' + e.message, 'error');
          }
        },
        async resumeDownload() {
          try {
            const res = await window.pywebview.api.resume_download();
            if (res && res.success) {
              this.dlProgress.paused = false;
              this.showToast('下载已恢复', 'info');
            }
          } catch (e) {
            this.showToast('恢复失败: ' + e.message, 'error');
          }
        },
        async autoUpdateGeo() {
          this.showToast('正在自动更新 Geo 数据...', 'info');
          try {
            const res = await window.pywebview.api.auto_update_geo();
            this.showToast('✅ Geo 数据更新完成', 'success');
          } catch (e) {
            this.showToast('Geo 更新异常: ' + e.message, 'error');
          }
        }
};

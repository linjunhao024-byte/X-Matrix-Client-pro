/**
 * X-Matrix — 初始化模块
 * $watch 注册、pywebviewready 事件、剪贴板监听、键盘快捷键
 */
window.XMatrixInit = {

        init() {
          // 页面卸载时清理资源，防止内存泄漏
          window.addEventListener('beforeunload', () => {
            this._cleanupTrafficChart();
          });

          // 核心配置状态自动监控与持久化写入
          this.$watch('proxyMode', val => localStorage.setItem('xmatrix_proxy_mode', val));
          
          this.$watch('tunMode', val => localStorage.setItem('xmatrix_tun_mode', JSON.stringify(val)));
          this.$watch('configState', val => localStorage.setItem('xmatrix_config_state', JSON.stringify(val)), { deep: true });
          this.$watch('advancedState.dnsStrategy', val => localStorage.setItem('xmatrix_adv_dns', val));
          this.$watch('advancedState.enableFakeDns', val => localStorage.setItem('xmatrix_adv_fakedns', JSON.stringify(val)));
          this.$watch('advancedState.logLevel', val => localStorage.setItem('xmatrix_adv_log', val));
          this.$watch('settingsState.sysProxyMode', val => localStorage.setItem('xmatrix_sys_proxy_mode', val));
          this.$watch('settingsState.hotkeyWindow', val => { localStorage.setItem('xmatrix_hk_win', val); if (window.pywebview?.api) window.pywebview.api.set_hotkeys(val, this.settingsState.hotkeyProxy); });
          this.$watch('settingsState.hotkeyProxy', val => { localStorage.setItem('xmatrix_hk_proxy', val); if (window.pywebview?.api) window.pywebview.api.set_hotkeys(this.settingsState.hotkeyWindow, val); });
          this.$watch('settingsState.trayLimit', val => { localStorage.setItem('xmatrix_tray_limit', val.toString()); if (window.pywebview?.api) window.pywebview.api.set_tray_limit(val); });
          // isDark 已改为 getter，主题由 settingsState.theme → applyTheme() 统一管理
          this.$watch('settingsState.theme', () => { this.applyTheme(); });
          this.$watch('wallpaperPath', val => {
            if (val) localStorage.setItem('xmatrix_wallpaper', val);
            else localStorage.removeItem('xmatrix_wallpaper');
          });
          this.$watch('glassOpacity', val => localStorage.setItem('xmatrix_glass_opacity', val));

          // Tab 切换时重绘依赖 DOM 尺寸的组件
          this.$watch('currentTab', tab => {
            if (tab === 'routing-table' && this.lastBgpResults && this.lastBgpResults.length > 0) {
              this.$nextTick(() => { this.buildBgpTopology(this.lastBgpResults); });
            }
            if (tab === 'home') {
              this.$nextTick(() => {
                if (this._trafficChart) this._trafficChart.resize();
              });
            }
          });

          // 全局监听 Ctrl+V / Cmd+V 进行极速无感导入
          window.addEventListener('keydown', async (e) => {
            if (e.key === 'Escape' && this.isBatchTesting) {
              this.isBatchTesting = false;
              this.showToast('⛔ 批量测速已取消', 'warning');
              return;
            }
            if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'v') {
              if (['INPUT', 'TEXTAREA', 'SELECT'].includes(document.activeElement.tagName)) return;
              await this.importFromClipboard();
            }
          });

          // 剪贴板自动检测引擎 (非阻塞版)
          window.addEventListener('focus', async () => {
            try {
              const text = await navigator.clipboard.readText();
              if (text && /vmess:\/\/|vless:\/\/|trojan:\/\/|ss:\/\//.test(text)) {
                if (text !== this.lastClipboardText) {
                  this.lastClipboardText = text;
                  this.clipboardPrompt.text = text;
                  this.clipboardPrompt.show = true;
                }
              }
            } catch(e) { /* 忽略读取限制 */ }
          });

          window.addEventListener('pywebviewready', async () => {
            // 推送用户自定义热键配置到后端
            window.pywebview.api.set_hotkeys(this.settingsState.hotkeyWindow, this.settingsState.hotkeyProxy);

            try {
            this.addLog('info', '[系统] X-Matrix 客户端启动中...');

            const res = await window.pywebview.api.get_tunnels();
            if (res) {
              this.syncTunnels(res);
              this.addLog('info', `[系统] 已加载 ${this.tunnels.length} 个节点配置`);
            }
            } catch (e) {
              console.error('初始化拉取节点失败:', e);
            }

            // 加载核心列表
            this.loadCoreTypes();
            try {
              const sysRes = await window.pywebview.api.get_sys_info();
              if (sysRes && sysRes.success) {
                this.sysInfo.os = sysRes.os;
                this.sysInfo.mode = sysRes.mode;
                this.sysInfo.version = sysRes.version;
                this.addLog('info', `[System] ${sysRes.os} | ${sysRes.mode} | ${sysRes.version}`);
              }
            } catch (e) {
              console.error('拉取系统信息失败:', e);
            }
            try {
              const autoStartRes = await window.pywebview.api.check_auto_startup();
              this.settingsState.autoStart = !!autoStartRes;
            } catch (e) {
              console.error('拉取开机自启状态失败:', e);
            }

            // 检查核心是否已在运行
            try {
              const status = await window.pywebview.api.get_core_status();
              if (status && status.running) {
                this.coreState.isRunning = true;
                this.addLog('info', `[系统] 检测到核心进程正在运行 (PID: ${status.pid})`);
              } else {
                this.addLog('info', '[系统] 核心未运行，点击节点启动代理');
              }
            } catch (e) { console.warn('[系统] 检查核心状态失败:', e); }

            this.addLog('info', '[系统] 初始化完成，就绪');

            // 加载自定义测试网站
            this.loadCustomWebsites();
            // 应用持久化的主题
            this.applyTheme();
            // 同步关闭行为到后端
            window.pywebview.api.set_close_behavior(this.settingsState.closeBehavior);
            // 延迟 1.5 秒执行 IP 探测，让页面先完成渲染，体感更快
            setTimeout(() => this.refreshIpInfo(), 1500);
          });
}
};

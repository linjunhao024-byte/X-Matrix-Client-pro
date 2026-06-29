/**
 * X-Matrix — 配置管理模块
 * 进阶配置、DNS、TUN、Mux、配置预览/导出
 */
window.XMatrixConfig = {
        advancedState: {
          dnsStrategy: localStorage.getItem('xmatrix_adv_dns') || 'AsIs',
          enableFakeDns: _safeJsonParse('xmatrix_adv_fakedns', false),
          logLevel: localStorage.getItem('xmatrix_adv_log') || 'info',
          codeOutbounds: '{\n  "outbounds": [\n    // 在此处编写自定义的出口规则\n  ]\n}',
          jsonError: ''
        },

        // ── 端口管理状态 ──
        portConfig: {
          api_port: 20085,
          clash_api_port: 9090
        },
        portCheckResults: [],
        isCheckingPorts: false,

        // ── DNS 预设状态 ──
        dnsPresets: [],
        dnsPresetsLoaded: false,

        // ── 自定义配置状态 ──
        customConfigJson: '',
        customConfigError: '',
        customConfigExists: false,
        configPriorityMode: 'smart',

        // ── 副核心状态 ──
        preServicePort: 2076,

        /**
         * 构建核心配置参数数组（集中管理，避免 5 处重复的 30+ 参数列表）
         * @param {number} targetIdx - 目标节点索引
         * @param {Array} activeRules - 活跃路由规则
         * @returns {Array} 传递给 pywebview.api.activate_tunnel / preview_config 的参数数组
         */
        _buildCoreParams(targetIdx, activeRules) {
          return [
            targetIdx, activeRules, this.advancedState.logLevel, this.tunMode,
            this.configState.localPort, this.configState.allowLan, this.configState.enableUdp,
            this.configState.sniffing, [...this.configState.sniffTypes],
            this.advancedState.dnsStrategy, this.advancedState.enableFakeDns, this.proxyMode,
            this.configState.enableCustomDns, this.configState.remoteDns, this.configState.localDns,
            this.configState.enableFragment, this.configState.fragmentPackets,
            this.configState.fragmentLength, this.configState.fragmentInterval,
            this.configState.tunMtu, this.configState.tunStack, this.configState.tunAutoRoute,
            this.configState.tunStrictRoute, this.configState.tunExcludeAddress,
            this.configState.enableMux, this.configState.muxConcurrency,
            this.configState.inboundAuth, this.configState.inboundUser, this.configState.inboundPass,
            this.configState.lanPort, this.configState.httpPort,
            (this.configState.secondSocksEnabled ? this.configState.secondPort : 0),
            this.configState.directDns, this.configState.dnsRules,
            this.configState.tunExcludeApps, this.configState.tunIncludeApps,
            this.configState.useSystemHosts
          ];
        },

        async saveAdvancedConfig() {
          this.advancedState.jsonError = '';
          try {
            const rawJson = this.advancedState.codeOutbounds;
            if (!rawJson.trim() || rawJson.includes('在此处编写自定义的出口规则')) {
               return this.showToast('请先点击「预览配置」生成全量模板，或粘贴有效的 JSON', 'warning');
            }

            this.showToast('正在调用 Xray 核心校验自定义配置...', 'info');
            const valRes = await window.pywebview.api.validate_config(rawJson);

            if (valRes && valRes.valid) {
              // 将编辑器的内容强行写入底层 config.json
              const saveRes = await window.pywebview.api.save_raw_config(rawJson);
              if (!saveRes.success) throw new Error(saveRes.error);

              this.showToast('✅ 校验通过！配置已强行覆写，正在重启核心', 'success');

              // 强制执行启停逻辑 (绕过 activate_tunnel 避免数据被重新覆盖)
              if (this.coreState.isRunning) {
                 await window.pywebview.api.stop_core();
                 await new Promise(r => setTimeout(r, 600));
                 await window.pywebview.api.start_core();
              } else {
                 await window.pywebview.api.start_core();
                 this.coreState.isRunning = true;
              }
            } else {
              this.advancedState.jsonError = ('❌ 核心校验拒绝: ') + (valRes ? valRes.message : ('未知错误'));
            }
          } catch (e) {
            this.advancedState.jsonError = ('❌ 覆写异常: ') + e.message;
          }
        },
        async previewFullConfig() {
          try {
            const activeRules = this.getActiveRules();
            const res = await window.pywebview.api.preview_config(...this._buildCoreParams(-1, activeRules));
            if (res) {
              // 将生成的完整 JSON 填入编辑器
              this.advancedState.codeOutbounds = res;
              this.showToast('✅ 已生成最新的完整配置预览', 'success');
            }
          } catch (e) {
            this.showToast(('预览失败: ') + e.message, 'error');
          }
        },
        async exportFullConfig() {
          try {
            let jsonToExport = this.advancedState.codeOutbounds;
            if (!jsonToExport.trim() || jsonToExport.includes('在此处编写自定义的出口规则')) {
               const activeRules = this.getActiveRules();
               jsonToExport = await window.pywebview.api.preview_config(...this._buildCoreParams(-1, activeRules));
            }
            const res = await window.pywebview.api.export_config(jsonToExport);
            if (res && res.success) {
              this.showToast(('✅ 配置文件已成功导出至: ') + res.path, 'success');
            } else if (res && !res.success && res.error !== ('用户取消')) {
              this.showToast(('导出失败: ') + res.error, 'error');
            }
          } catch (e) {
            this.showToast(('通信异常: ') + e.message, 'error');
          }
        },
        importAdvancedJson(event) {
          const file = event.target.files[0];
          if (!file) return;
          const reader = new FileReader();
          reader.onload = (e) => {
            this.advancedState.codeOutbounds = e.target.result;
            this.advancedState.jsonError = '';
            event.target.value = '';
          };
          reader.readAsText(file);
        },

        // ── 端口管理方法 ──
        async loadPortConfig() {
          try {
            const res = await window.pywebview.api.get_port_config();
            if (res && res.success && res.ports) {
              this.portConfig = {
                api_port: res.ports.api_port || 20085,
                clash_api_port: res.ports.clash_api_port || 9090
              };
            }
          } catch (e) {
            console.warn('[配置] 加载端口配置失败:', e);
          }
        },
        async savePortConfig() {
          try {
            const res = await window.pywebview.api.save_port_config(this.portConfig);
            if (res && res.success) {
              this.showToast('✅ 端口配置已保存，重启核心后生效', 'success');
            } else {
              this.showToast(('保存失败: ') + (res ? res.error : ''), 'error');
            }
          } catch (e) {
            this.showToast(('通信异常: ') + e.message, 'error');
          }
        },
        async checkPorts() {
          this.isCheckingPorts = true;
          this.portCheckResults = [];
          try {
            const ports = [this.portConfig.api_port, this.portConfig.clash_api_port];
            const res = await window.pywebview.api.check_ports(ports);
            if (res && res.success) {
              this.portCheckResults = res.results || [];
              const allOk = this.portCheckResults.every(r => r.available);
              this.showToast(allOk ? '✅ 所有端口可用' : '⚠️ 部分端口被占用', allOk ? 'success' : 'warning');
            } else {
              this.showToast(('检测失败: ') + (res ? res.error : ''), 'error');
            }
          } catch (e) {
            this.showToast(('通信异常: ') + e.message, 'error');
          } finally {
            this.isCheckingPorts = false;
          }
        },

        // ── DNS 预设方法 ──
        async loadDnsPresets() {
          if (this.dnsPresetsLoaded) return;
          try {
            const res = await window.pywebview.api.get_dns_presets();
            if (res && res.success) {
              this.dnsPresets = res.presets || [];
              this.dnsPresetsLoaded = true;
            }
          } catch (e) {
            console.warn('[配置] 加载 DNS 预设失败:', e);
          }
        },
        applyDnsPreset(preset) {
          if (preset.remote_dns) this.configState.remoteDns = preset.remote_dns;
          if (preset.local_dns) this.configState.localDns = preset.local_dns;
          if (preset.direct_dns) this.configState.directDns = preset.direct_dns;
          this.showToast('✅ 已应用 DNS 预设: ' + (preset.name || ''), 'success');
        },

        // ── 自定义配置方法 ──
        async loadConfigPriority() {
          try {
            const res = await window.pywebview.api.get_config_priority();
            if (res && res.success) {
              this.configPriorityMode = res.mode || 'smart';
              this.customConfigExists = res.custom_exists || false;
            }
          } catch (e) {
            console.warn('[配置] 加载配置优先级失败:', e);
          }
        },
        async saveConfigPriority(mode) {
          try {
            const res = await window.pywebview.api.set_config_priority(mode);
            if (res && res.success) {
              this.configPriorityMode = mode;
              this.showToast('✅ 配置优先级已设置', 'success');
            }
          } catch (e) {
            this.showToast(('通信异常: ') + e.message, 'error');
          }
        },
        async saveCustomConfig() {
          this.customConfigError = '';
          try {
            const res = await window.pywebview.api.save_custom_config(this.customConfigJson);
            if (res && res.success) {
              this.customConfigExists = true;
              this.showToast('✅ 自定义配置已保存', 'success');
            } else {
              this.customConfigError = res ? res.error : '保存失败';
            }
          } catch (e) {
            this.customConfigError = e.message;
          }
        },
        async deleteCustomConfig() {
          if (!confirm('确定要删除自定义配置文件吗？')) return;
          try {
            const res = await window.pywebview.api.delete_custom_config();
            if (res && res.success) {
              this.customConfigExists = false;
              this.customConfigJson = '';
              this.showToast('✅ 自定义配置已删除', 'success');
            }
          } catch (e) {
            this.showToast(('通信异常: ') + e.message, 'error');
          }
        },

        // ── 副核心方法 ──
        async loadPreServiceConfig() {
          try {
            const res = await window.pywebview.api.get_pre_service_config();
            if (res && res.success) {
              this.settingsState.preServiceEnabled = res.pre_service_enabled || false;
              this.settingsState.preServiceCore = res.pre_service_core || 'xray';
              this.preServicePort = res.pre_service_port || 2076;
            }
          } catch (e) {
            console.warn('[配置] 加载副核心配置失败:', e);
          }
        },
        async savePreServiceConfig() {
          try {
            const res = await window.pywebview.api.save_pre_service_config(
              this.settingsState.preServiceEnabled,
              this.settingsState.preServiceCore,
              this.preServicePort
            );
            if (res && res.success) {
              this.showToast('✅ 副核心配置已保存', 'success');
            }
          } catch (e) {
            this.showToast(('通信异常: ') + e.message, 'error');
          }
        },

        // ── 出站统计方法 ──
        outboundStats: null,
        async loadOutboundStats(nodeId) {
          try {
            const res = await window.pywebview.api.get_outbound_stats(nodeId);
            if (res && res.success) {
              this.outboundStats = res;
            }
          } catch (e) {
            console.warn('[配置] 加载出站统计失败:', e);
          }
        }
};

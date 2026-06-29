/**
 * X-Matrix — 节点管理模块
 * 节点 CRUD、批量操作、排序、导入导出
 */
window.XMatrixNodes = {
  isNodeModalOpen: false,
  modalMode: 'add',
  editingNode: {
    protocol: 'vless', out_tag: '', server_addr: '', server_port: 443,
    uuid: '', alter_id: '0', vmess_security: 'auto', password: '', method: 'aes-256-gcm',
    socks_user: '', socks_pass: '', http_user: '', http_pass: '',
    anytls_password: '', naive_user: '', naive_proto: 'https',
    hy2_obfs: 'none', hy2_obfs_password: '', hy2_up_mbps: '', hy2_down_mbps: '',
    tuic_congestion: 'bbr', tuic_udp_relay: 'native',
    wg_secret_key: '', wg_public_key: '', wg_address: '10.0.0.2/32', wg_reserved: '', wg_mtu: '1420', wg_pre_shared_key: '', wg_allowed_ips: '0.0.0.0/0',
    network: 'tcp', security: 'none',
    sni: '', fingerprint: 'chrome', public_key: '', short_id: '', flow: '', alpn: '',
    ech_enable: false, ech_config: '', finalmask_enable: false,
    send_through: '', bind_interface: '', socks_mark: '', default_ua: '', chain_id: '',
    ws_path: '/', ws_host: '',
    grpc_service_name: '', kcp_header: 'none', kcp_seed: '', h2_host: '', h2_path: '', httpupgrade_path: '', httpupgrade_host: '', xhttp_mode: 'auto', xhttp_path: '', xhttp_host: '', quic_security: 'none', quic_key: '', quic_header: 'none', bgp_manual: '', bgp_alias: ''
  },
  editingIndex: -1,
  isBatchTesting: false,
  batchProgress: { done: 0, total: 0, currentNode: '', results: [] },
  batchMode: false,
  selectedNodes: [],
  dragIndex: null,
  tunnels: [],
        openNodeModal(mode, node = null, index = -1) {
          this.modalMode = mode;
          this.editingIndex = index;
          if (mode === 'edit' && node) {
            this.editingNode = JSON.parse(JSON.stringify(node));
            delete this.editingNode.delay;
            delete this.editingNode.active;
          } else {
            this.editingNode = {
              protocol: 'vless', out_tag: '', server_addr: '', server_port: 443,
              uuid: '', alter_id: '0', vmess_security: 'auto', password: '', method: 'aes-256-gcm',
              socks_user: '', socks_pass: '', http_user: '', http_pass: '',
          anytls_password: '', naive_user: '', naive_proto: 'https',
          hy2_obfs: 'none', hy2_obfs_password: '', hy2_up_mbps: '', hy2_down_mbps: '',
          tuic_congestion: 'bbr', tuic_udp_relay: 'native',
          wg_secret_key: '', wg_public_key: '', wg_address: '10.0.0.2/32', wg_reserved: '', wg_mtu: '1420', wg_pre_shared_key: '', wg_allowed_ips: '0.0.0.0/0',
              network: 'tcp', security: 'none',
              sni: '', fingerprint: 'chrome', public_key: '', short_id: '', flow: '', alpn: '',
              ech_enable: false, ech_config: '', finalmask_enable: false,
              send_through: '', bind_interface: '', socks_mark: '', default_ua: '', chain_id: '',
              ws_path: '/', ws_host: '',
              grpc_service_name: '', kcp_header: 'none', kcp_seed: '', h2_host: '', h2_path: '', httpupgrade_path: '', httpupgrade_host: '', xhttp_mode: 'auto', xhttp_path: '', xhttp_host: '', quic_security: 'none', quic_key: '', quic_header: 'none', bgp_manual: '', bgp_alias: ''
            };
          }
          this.isNodeModalOpen = true;
        },
        async activateNode(index) {
          if (this.isActivatingNode) return;

          // 越界保护拦截
          if (index !== -2 && (index < 0 || index >= this.tunnels.length)) return;

          this.isActivatingNode = true;
          try {
            const activeRules = this.getActiveRules();
            // 处理特殊的热刷新指令 (-2)：仅重载配置，不改变当前激活节点
            let targetIdx = index;
            let isDisconnecting = false;
            let isHotReload = false;
            if (index === -2) {
              isHotReload = true;
              targetIdx = this.tunnels.findIndex(t => t.active);
              if (targetIdx === -1) targetIdx = -1; // 无激活节点则走直连
            } else {
              isDisconnecting = this.tunnels[index].active;
              targetIdx = isDisconnecting ? -1 : index;
            }

            const res = await window.pywebview.api.activate_tunnel(...this._buildCoreParams(targetIdx, activeRules));
            if (res && res.success) {
              this.coreState.isRunning = true;
              if (!isHotReload) {
                // 正常切换：更新激活状态
                this.tunnels.forEach((t, i) => { t.active = (i === targetIdx); });
                if (targetIdx !== -1) {
                  localStorage.setItem('xmatrix_last_node', targetIdx);
                }
                this.addLog('info', isDisconnecting ? ('已断开代理节点，当前走全局直连') : ('已成功切换并接管代理节点'));
              } else {
                // 热刷新：仅重载配置，保持当前节点不变
                this.addLog('info', '路由模式已切换，配置已热重载');
              }
              this.refreshIpInfo();
            } else {
              this.showToast(('操作失败：') + ((res && res.error) || ('未知错误')), 'error');
            }
          } catch (e) {
            this.showToast(('通信异常：') + e.message, 'error');
          } finally {
            this.isActivatingNode = false;
          }
        },
        async saveNodeModal() {
          // ── 防呆校验逻辑 ──
          const addr = this.editingNode.server_addr;
          const port = parseInt(this.editingNode.server_port);
          if (!addr || addr.trim() === '') return this.showToast('服务器地址不能为空', 'warning');
          if (isNaN(port) || port < 1 || port > 65535) return this.showToast('端口必须在 1-65535 之间', 'warning');
          if (['vless', 'vmess'].includes(this.editingNode.protocol) && !this.editingNode.uuid) {
            return this.showToast(`${this.editingNode.protocol.toUpperCase()} 协议必须填写 UUID`, 'warning');
          }
          if (this.editingNode.protocol === 'shadowsocks' && (!this.editingNode.method || !this.editingNode.password)) {
            return this.showToast('Shadowsocks 必须填写加密方式和密码', 'warning');
          }
          if (this.editingNode.protocol === 'anytls' && !this.editingNode.anytls_password) {
            return this.showToast('Anytls 必须填写密码', 'warning');
          }

          try {
            let res;
            if (this.modalMode === 'add') {
              res = await window.pywebview.api.add_tunnel(this.editingNode);
            } else {
              res = await window.pywebview.api.update_tunnel(this.editingIndex, this.editingNode);
            }
            if (res && res.success) {
              this.syncTunnels(res.tunnels);
              // ProxyChain: 保存后设置代理链
              const chainId = this.editingNode.chain_id || '';
              const nodeId = this.editingNode.id || (res.tunnels && res.tunnels.length ? res.tunnels[res.tunnels.length - 1].id : '');
              if (nodeId) {
                try { await window.pywebview.api.set_proxy_chain(nodeId, chainId); } catch (e) { console.warn('[节点] 设置代理链失败:', e); }
              }
              this.isNodeModalOpen = false;
              this.addLog('info', ('[节点] ') + (this.modalMode === 'add' ? ('添加') : ('更新')) + ('成功: ') + (this.editingNode.out_tag || ('未命名节点')));
            } else {
              this.showToast(('保存失败: ') + ((res && res.error) || '未知错误'), 'error');
            }
          } catch (e) {
            this.showToast(('通信异常: ') + e.message, 'error');
          }
        },
        async testNodePort() {
          if (!this.editingNode.server_addr || !this.editingNode.server_port) {
            return this.showToast('请先填写服务器地址和端口', 'warning');
          }
          this.showToast('正在探测端口连通性...', 'info');
          try {
            const res = await window.pywebview.api.test_port(this.editingNode.server_addr, this.editingNode.server_port.toString());
            if (res && res.success) {
              if (res.status === 'open') this.showToast('✅ 端口开放，连接正常！', 'success');
              else this.showToast('❌ 端口关闭或被阻断', 'error');
            } else {
              this.showToast(('探测失败: ') + ((res && res.error) || '未知错误'), 'error');
            }
          } catch (e) {
            this.showToast(('探测异常: ') + e.message, 'error');
          }
        },
        async deleteNode(index) {
          if (!confirm('确定要删除该节点吗？')) return;
          const nodeName = (this.tunnels[index] ? this.tunnels[index].out_tag : '') || ('未命名节点');
          try {
            const res = await window.pywebview.api.delete_tunnel(index);
            if (res && res.success) {
              this.syncTunnels(res.tunnels);
              this.addLog('warning', `[节点] 已删除: ${nodeName}`);
            } else {
              this.showToast(('删除失败：') + ((res && res.error) || ('未知错误')), 'error');
            }
          } catch (e) {
            this.showToast(('前后端通信异常：') + e.message, 'error');
          }
        },
        async loadNodeStats(index) {
          const node = this.tunnels[index];
          if (!node || node._statsLoaded) return;
          try {
            const res = await window.pywebview.api.get_node_stats(node.id);
            if (res && res.success && res.stats) {
              this.tunnels[index]._stats = { today_up: res.stats.today_up || 0, today_down: res.stats.today_down || 0, total_up: res.stats.total_up || 0, total_down: res.stats.total_down || 0 };
              this.tunnels[index]._statsLoaded = true;
            }
          } catch (e) { console.warn('[节点] 加载节点统计失败:', e); }
        },
        async tcpPingNode(index) {
          const nodeName = this.tunnels[index].out_tag || (`节点 ${index}`);
          this.tunnels[index].delay = '...';
          this.addLog('info', `[TCP Ping] ${nodeName}: 正在探测...`);
          try {
            const res = await window.pywebview.api.test_node_tcp_ping(index);
            if (res && res.success && res.delay >= 0) {
              this.tunnels[index].delay = res.delay;
              this.addLog('info', `[TCP Ping] ${nodeName}: ${res.delay}ms`);
            } else {
              this.tunnels[index].delay = -1;
              this.addLog('warning', `[TCP Ping] ${nodeName}: Timeout`);
            }
          } catch (e) {
            this.tunnels[index].delay = -1;
            this.addLog('error', `[TCP Ping] ${nodeName}: 异常`);
          }
        },
        async udpPingNode(index) {
          this.showToast('UDP Ping: 功能开发中', 'info');
        },
        async mixedTestNode(index) {
          this.showToast('混合测速: 功能开发中', 'info');
        },
        async batchTcpPing() {
          if (!this.tunnels.length) return this.showToast('没有可用节点', 'warning');
          this.isBatchTesting = true;
          this.batchProgress = { done: 0, total: this.tunnels.length, currentNode: '', results: [] };
          this.tunnels.forEach(t => t.delay = '...');
          const concurrency = this.settingsState.testConcurrency || 5;
          const queue = this.tunnels.map((_, i) => i);
          const workers = Array.from({ length: Math.min(concurrency, this.tunnels.length) }, async () => {
            while (queue.length && this.isBatchTesting) {
              const idx = queue.shift();
              const nodeName = this.tunnels[idx].out_tag || `Node ${idx}`;
              this.batchProgress.currentNode = nodeName;
              try {
                const res = await window.pywebview.api.test_node_tcp_ping(idx);
                const delay = (res && res.success && res.delay >= 0) ? res.delay : -1;
                this.tunnels[idx].delay = delay;
                this.batchProgress.results.push(delay);
              } catch { this.tunnels[idx].delay = -1; this.batchProgress.results.push(-1); }
              this.batchProgress.done++;
            }
          });
          await Promise.all(workers);
          this.isBatchTesting = false;
          const ok = this.tunnels.filter(t => t.delay > 0).length;
          this.showToast(('TCP Ping 完成: ') + ok + '/' + this.tunnels.length + (' 成功'), 'success');
        },
        toggleBatchMode() {
          this.batchMode = !this.batchMode;
          if (!this.batchMode) this.selectedNodes = [];
        },
        toggleNodeSelect(index) {
          const i = this.selectedNodes.indexOf(index);
          if (i === -1) this.selectedNodes.push(index);
          else this.selectedNodes.splice(i, 1);
        },
        async batchDeleteNodes() {
          if (this.selectedNodes.length === 0) return;
          if (!confirm(`确定要删除选中的 ${this.selectedNodes.length} 个节点吗？`)) return;
          try {
            const res = await window.pywebview.api.delete_tunnels_batch(this.selectedNodes);
            if (res && res.success) {
              this.syncTunnels(res.tunnels);
              const count = this.selectedNodes.length;
              this.selectedNodes = [];
              this.batchMode = false;
              this.showToast(`✅ 已删除 ${count} 个节点`, 'success');
            } else {
              this.showToast(('批量删除失败：') + ((res && res.error) || ('未知错误')), 'error');
            }
          } catch (e) {
            this.showToast(('通信异常：') + e.message, 'error');
          }
        },
        async batchExportNodes() {
          if (this.selectedNodes.length === 0) return;
          try {
            const uris = [];
            for (const idx of this.selectedNodes) {
              const res = await window.pywebview.api.export_uri(idx);
              if (res && res.success) uris.push(res.uri);
            }
            if (uris.length > 0) {
              await navigator.clipboard.writeText(uris.join('\n'));
              this.showToast(`✅ 成功导出 ${uris.length} 个节点链接到剪贴板`, 'success');
              this.batchMode = false;
              this.selectedNodes = [];
            }
          } catch (e) {
            this.showToast(('批量导出异常：') + e.message, 'error');
          }
        },
        async reorderNode(toIndex) {
          const fromIndex = this.dragIndex;
          if (fromIndex === null || fromIndex === toIndex) return;
          try {
            const res = await window.pywebview.api.reorder_tunnels(fromIndex, toIndex);
            if (res && res.success) {
              this.syncTunnels(res.tunnels);
            }
          } catch (e) {
            console.error('排序失败:', e);
          }
          this.dragIndex = null;
        },
        async reorderNodeDirect(fromIndex, toIndex) {
          try {
            const res = await window.pywebview.api.reorder_tunnels(fromIndex, toIndex);
            if (res && res.success) this.syncTunnels(res.tunnels);
          } catch (e) { console.warn('[节点] 移动节点失败:', e); }
        },
        async moveNodeTop(index) { if (index > 0) await this.reorderNodeDirect(index, 0); },
        async moveNodeUp(index) { if (index > 0) await this.reorderNodeDirect(index, index - 1); },
        async moveNodeDown(index) { if (index < this.tunnels.length - 1) await this.reorderNodeDirect(index, index + 1); },
        async sortByDelay() {
          if (this.tunnels.length <= 1) return;
          const indexed = this.tunnels.map((t, i) => ({ index: i, delay: t.delay }));
          indexed.sort((a, b) => {
            const getVal = d => (typeof d === 'number' && d > 0) ? d : 999999;
            return getVal(a.delay) - getVal(b.delay);
          });
          const newIndices = indexed.map(item => item.index);
          try {
            const res = await window.pywebview.api.apply_tunnels_order(newIndices);
            if (res && res.success) {
              this.syncTunnels(res.tunnels);
              this.showToast('✅ 节点已按延迟升序排列', 'success');
            } else {
              this.showToast(('排序失败: ') + ((res && res.error) || '未知错误'), 'error');
            }
          } catch (e) {
            this.showToast(('排序异常: ') + e.message, 'error');
          }
        },
        async dedupNodes() {
          if (this.tunnels.length <= 1) return;
          try {
            const res = await window.pywebview.api.dedup_server_list(this.settingsState.keepOlderDedup);
            if (res && res.success) {
              this.syncTunnels(res.tunnels);
              const removed = res.before - res.after;
              if (removed > 0) {
                this.showToast(('✅ 去重完成，移除 ') + removed + (' 个重复节点'), 'success');
              } else {
                this.showToast('没有重复节点', 'info');
              }
            } else {
              this.showToast(('去重失败: ') + ((res && res.error) || '未知错误'), 'error');
            }
          } catch (e) {
            this.showToast(('去重异常: ') + e.message, 'error');
          }
        },
        async generateGroupAll() {
          // 收集所有非组节点的 ID
          const childIds = this.tunnels.filter(t => t.protocol !== 'policy_group').map(t => t.id);
          if (childIds.length < 2) {
            return this.showToast('至少需要 2 个节点才能创建负载均衡组', 'warning');
          }
          const name = '全部节点均衡组';
          try {
            const res = await window.pywebview.api.add_policy_group(name, 'leastPing', childIds, '');
            if (res && res.success) {
              this.syncTunnels(res.tunnels);
              this.showToast(('✅ 负载均衡组已创建，包含 ') + childIds.length + (' 个节点'), 'success');
            } else {
              this.showToast(('创建组失败: ') + ((res && res.error) || '未知错误'), 'error');
            }
          } catch (e) {
            this.showToast(('创建组异常: ') + e.message, 'error');
          }
        },
        async autoGroupByRegion() {
          try {
            const res = await window.pywebview.api.auto_group_by_region();
            if (res && res.success) {
              this.syncTunnels(res.tunnels);
              const count = (res.groups || []).length;
              this.showToast(('✅ 已创建 ') + count + (' 个地区分组'), 'success');
            } else {
              this.showToast(('自动分组失败: ') + (res ? res.error : ''), 'error');
            }
          } catch (e) { this.showToast(e.message, 'error'); }
        },
        async _applySort(sortedIndices, label) {
          if (this.tunnels.length <= 1) return;
          try {
            const res = await window.pywebview.api.apply_tunnels_order(sortedIndices);
            if (res && res.success) {
              this.syncTunnels(res.tunnels);
              this.showToast('✅ ' + label, 'success');
            }
          } catch (e) {
            this.showToast(('排序异常: ') + e.message, 'error');
          }
        },
        async sortNodesByName() {
          const indexed = this.tunnels.map((t, i) => ({ i, name: (t.out_tag || '').toLowerCase() }));
          indexed.sort((a, b) => a.name.localeCompare(b.name));
          this._applySort(indexed.map(x => x.i), '已按名称排序');
        },
        async sortNodesByAddr() {
          const indexed = this.tunnels.map((t, i) => ({ i, addr: (t.server_addr || '') + ':' + (t.server_port || 0) }));
          indexed.sort((a, b) => a.addr.localeCompare(b.addr));
          this._applySort(indexed.map(x => x.i), '已按地址排序');
        },
        async sortNodesByProtocol() {
          const order = { vless: 0, vmess: 1, trojan: 2, shadowsocks: 3, hysteria2: 4, tuic: 5, wireguard: 6, anytls: 7, naive: 8, socks: 9, http: 10 };
          const indexed = this.tunnels.map((t, i) => ({ i, p: order[t.protocol] ?? 9 }));
          indexed.sort((a, b) => a.p - b.p);
          this._applySort(indexed.map(x => x.i), '已按协议排序');
        },
        async sortNodesReverse() {
          const indices = this.tunnels.map((_, i) => i).reverse();
          this._applySort(indices, '已反转顺序');
        },
        async testAllNodes() {
          if (this.isBatchTesting) {
            this.isBatchTesting = false;
            this.addLog('warning', '用户手动取消批量测速');
            return;
          }
          if (this.tunnels.length === 0) return this.showToast('没有可测速的节点', 'warning');

          this.isBatchTesting = true;
          this.batchProgress = { done: 0, total: this.tunnels.length, currentNode: '', results: [] };
          const limit = this.settingsState.testConcurrency || 5;
          this.showToast(`开始并发测速，最大线程数：${limit}`, 'info');
          this.addLog('info', `[测速] 启动批量测速: ${this.tunnels.length} 个节点, 并发数: ${limit}`);

          // 重置所有延迟状态
          this.tunnels.forEach(t => t.delay = '...');
          const queue = this.tunnels.map((_, i) => i);

          const worker = async () => {
            while (queue.length > 0 && this.isBatchTesting) {
              const idx = queue.shift();
              const nodeName = this.tunnels[idx].out_tag || (`节点 ${idx}`);
              this.batchProgress.currentNode = nodeName;
              try {
                const res = await window.pywebview.api.test_node_real_delay(
                  idx, this.settingsState.delayTestUrl,
                  this.settingsState.testTimeout, this.configState.localPort
                );
                if (this.isBatchTesting) {
                  const delay = (res && res.success && res.delay > 0) ? res.delay : -1;
                  this.tunnels[idx].delay = delay;
                  this.batchProgress.results.push(delay > 0 ? delay : -1);
                  this.batchProgress.done++;
                  this.addLog(delay > 0 ? 'info' : 'warning', `[Speed] ${nodeName}: ${delay > 0 ? delay + 'ms' : 'Timeout'}`);
                }
              } catch (e) {
                if (this.isBatchTesting) {
                  this.tunnels[idx].delay = -1;
                  this.batchProgress.results.push(-1);
                  this.batchProgress.done++;
                  this.addLog('error', `[测速] ${nodeName}: 异常 - ${e.message}`);
                }
              }
            }
          };

          // 启动指定数量的并发 worker
          const workers = Array(limit).fill(0).map(() => worker());
          await Promise.all(workers);

          if (this.isBatchTesting) {
            this.isBatchTesting = false;
            this.showToast('✅ 全部节点测速完成', 'success');
          } else {
            this.showToast('已取消测速', 'warning');
          }
        },
        async testNode(index) {
          const nodeName = this.tunnels[index].out_tag || (`节点 ${index}`);
          this.tunnels[index].delay = '...';
          this.addLog('info', `[真连接] ${nodeName}: 正在测速...`);
          try {
            const res = await window.pywebview.api.test_node_real_delay(index, this.settingsState.delayTestUrl, this.settingsState.testTimeout, this.configState.localPort);
            if (res && res.success && res.delay > 0) {
              this.tunnels[index].delay = res.delay;
              this.addLog('info', `[Real Conn] ${nodeName}: ${res.delay}ms`);
            } else {
              this.tunnels[index].delay = -1;
              this.addLog('warning', `[Real Conn] ${nodeName}: Timeout`);
            }
          } catch (e) {
            this.tunnels[index].delay = -1;
            this.addLog('error', `[真连接] ${nodeName}: 异常`);
          }
        },
        async cloneNode(index) {
          const copiedNode = JSON.parse(JSON.stringify(this.tunnels[index]));
          const oldName = copiedNode.out_tag || copiedNode.tag || 'Node';
          delete copiedNode.in_tag;
          delete copiedNode.out_tag;
          delete copiedNode.delay;
          delete copiedNode.active;
          copiedNode.out_tag = oldName + ' - Copy';
          delete copiedNode.tag; // 防止污染后端
          try {
            const res = await window.pywebview.api.add_tunnel(copiedNode);
            if (res && res.success) {
              this.syncTunnels(res.tunnels);
            } else {
              this.showToast(('克隆失败：') + ((res && res.error) || ('未知错误')), 'error');
            }
          } catch (e) {
            this.showToast(('通信异常：') + e.message, 'error');
          }
        },
        async importConfig() {
          try {
            this.addLog('info', '[导入] 正在导入 config.json...');
            const res = await window.pywebview.api.import_config();
            if (res && res.success) {
              this.syncTunnels(res.tunnels);
              this.addLog('info', `[导入] 成功导入 ${res.tunnels.length} 个节点`);
            } else if (res && !res.success && res.error !== ('用户取消选择') && res.error !== ('用户取消')) {
              this.showToast(('导入失败：') + res.error, 'error');
            }
          } catch (e) {
            this.showToast(('通信异常：') + e.message, 'error');
          }
        },
        async importSubscriptionUrl() {
          const url = prompt('请输入订阅链接 (支持 Clash YAML / Base64 编码):', 'https://');
          if (!url || url === 'https://') return;
          this.showToast('正在拉取并解析订阅...', 'info');
          try {
            const res = await window.pywebview.api.import_subscription(url);
            if (res && res.success) {
              this.syncTunnels(res.tunnels);
              this.showToast(`✅ 成功导入 ${res.count} 个节点！`, 'success');
            } else {
              this.showToast(('导入失败：') + ((res && res.error) || ('未知错误')), 'error');
            }
          } catch (e) {
            this.showToast(('通信异常：') + e.message, 'error');
          }
        },
        async importFromClipboard() {
          let text = '';
          try {
            text = await navigator.clipboard.readText();
          } catch (err) {
            text = prompt('无法自动读取剪贴板，请在此处粘贴您的节点分享链接 (支持批量换行):');
          }
          if (!text) return;
          try {
            this.addLog('info', '[剪贴板] 正在解析分享链接...');
            const res = await window.pywebview.api.import_uri(text);
            if (res && res.success) {
              this.syncTunnels(res.tunnels);
              this.addLog('info', `[剪贴板] 成功导入 ${res.count} 个节点`);
              this.showToast(`成功导入 ${res.count} 个节点！✨`, 'success');
            } else {
              this.showToast((res && res.error) || ('解析失败，未识别到合法链接'), 'error');
            }
          } catch (e) {
            this.showToast(('通信异常: ') + e.message, 'error');
          }
        },
        async shareNode(index) {
          try {
            const res = await window.pywebview.api.export_uri(index);
            if (res && res.success) {
              this.qrModal.uri = res.uri;
              this.qrModal.show = true;
            } else {
              this.showToast(('导出失败：') + ((res && res.error) || '未知错误'), 'error');
            }
          } catch (e) {
            this.showToast('生成链接异常', 'error');
          }
        },
        async exportXmatrixUri(index) {
          try {
            const res = await window.pywebview.api.export_xmatrix_uri(index);
            if (res && res.success) {
              await navigator.clipboard.writeText(res.uri);
              this.showToast('✅ xmatrix:// 链接已复制到剪贴板', 'success');
            } else {
              this.showToast(('导出失败：') + (res ? res.error : ''), 'error');
            }
          } catch (e) {
            this.showToast('导出链接异常: ' + e.message, 'error');
          }
        },
        async removeTimeoutNodes() {
          if (!confirm('确定要清理所有超时节点吗？')) return;
          try {
            const res = await window.pywebview.api.remove_timeout_nodes();
            if (res && res.success) {
              this.syncTunnels(res.tunnels);
              const removed = (res.removed || 0);
              if (removed > 0) {
                this.showToast(('✅ 已清理 ') + removed + (' 个超时节点'), 'success');
              } else {
                this.showToast('没有超时节点', 'info');
              }
            } else {
              this.showToast(('清理失败：') + (res ? res.error : ''), 'error');
            }
          } catch (e) {
            this.showToast(('通信异常：') + e.message, 'error');
          }
        }
};
/**
 * X-Matrix — 路由管理模块
 * 路由规则、BGP 拓扑、Geo 预设
 */
window.XMatrixRouting = {
        // BGP 路由拓扑状态
        bgpNodes: [],
        bgpEdges: [],
        isBgpTesting: false,
        bgpDragNode: null,
        lastBgpResults: [],
        bgpAutoRefreshTimer: null,
        bgpIpVisible: false,
        bgpAppVisible: true,
        bgpAppFocused: true,
        bgpCanvasHeight: 500,
        bgpCanvasLocked: false,
        selectedBgpNode: null,

        routingRules: _safeJsonParse('xmatrix_routing_rules', null) || [
          { id: 1, enabled: true, type: 'domain', content: 'geosite:category-ads-all', outbound: 'block' },
          { id: 2, enabled: true, type: 'domain', content: 'geosite:cn', outbound: 'direct' },
          { id: 3, enabled: true, type: 'ip', content: 'geoip:private, geoip:cn', outbound: 'direct' },
          { id: 4, enabled: false, type: 'domain', content: 'domain:github.com', outbound: 'proxy' }
        ],
        saveRoutingRules() {
          localStorage.setItem('xmatrix_routing_rules', JSON.stringify(this.routingRules));
        },
        /** 返回启用的路由规则，并将 negate boolean 转为 content 的 ! 前缀（仅 domain/ip 类型，Xray 原生支持） */
        getActiveRules() {
          return this.routingRules.filter(r => r.enabled).map(r => {
            if (!r.negate) return r;
            // 仅 domain/ip 类型支持取反（Xray geosite:!cn / geoip:!cn 语法）
            if (r.type !== 'domain' && r.type !== 'ip') return r;
            const negated = { ...r };
            negated.content = r.content.split(',').map(c => c.trim().startsWith('!') ? c.trim() : '!' + c.trim()).join(',');
            return negated;
          });
        },
        addRoutingRule() {
          const type = prompt('请输入规则类型 (domain / ip / port / network / process / geosite / geoip / protocol):', 'domain');
          if (!type || !['domain', 'ip', 'port', 'network', 'process', 'geosite', 'geoip', 'protocol'].includes(type)) return this.showToast('类型无效，支持: domain, ip, port, network, process, geosite, geoip, protocol', 'warning');

          let content = '';
          if (type === 'protocol') {
            // Protocol 类型使用多选 checkbox 模式
            this._pendingProtocolRule = true;
            this._protocolModal = { show: true, selected: [], outbound: 'proxy' };
            return;
          }

          content = prompt(`请输入 [${type}] 的匹配内容 (如 port填 80,443 ; network填 tcp,udp):`, '');
          if (!content) return;
          const outbound = prompt('请输入目标出站 (proxy / direct / block):', 'proxy');
          if (!outbound || !['proxy', 'direct', 'block'].includes(outbound)) return this.showToast('出站无效，添加取消', 'warning');

          this.routingRules.unshift({ id: Date.now(), enabled: true, negate: false, type, content, outbound });
          this.saveRoutingRules();
          this.showToast('✅ 规则添加成功，重启核心后生效', 'success');
        },
        _protocolModal: { show: false, selected: [], outbound: 'proxy' },
        confirmProtocolRule() {
          const pm = this._protocolModal;
          if (!pm.selected.length) return this.showToast('请至少选择一个协议', 'warning');
          const content = pm.selected.join(',');
          if (pm.editIndex !== undefined && pm.editIndex >= 0) {
            // 编辑模式
            const rule = this.routingRules[pm.editIndex];
            rule.content = content;
            rule.outbound = pm.outbound;
            this.saveRoutingRules();
            this.showToast('✅ 修改成功，重启核心后生效', 'success');
          } else {
            // 新增模式
            this.routingRules.unshift({ id: Date.now(), enabled: true, negate: false, type: 'protocol', content, outbound: pm.outbound });
            this.saveRoutingRules();
            this.showToast('✅ 规则添加成功，重启核心后生效', 'success');
          }
          pm.show = false;
          pm.selected = [];
          pm.outbound = 'proxy';
          delete pm.editIndex;
        },
        editRoutingRule(index) {
          const rule = this.routingRules[index];
          if (rule.type === 'protocol') {
            // Protocol 类型使用多选 checkbox 编辑
            this._protocolModal = { show: true, selected: rule.content.split(','), outbound: rule.outbound, editIndex: index };
            return;
          }
          const content = prompt(`修改 [${rule.type}] 的匹配内容:`, rule.content);
          if (content && content !== rule.content) {
            rule.content = content;
            this.saveRoutingRules();
            this.showToast('✅ 修改成功，重启核心后生效', 'success');
          }
        },
        async importRoutingRules() {
          try {
            const res = await window.pywebview.api.import_routing_rules();
            if (res && res.success) {
              this.routingRules = [...res.rules, ...this.routingRules];
              this.saveRoutingRules();
              this.showToast(`✅ 成功导入 ${res.count} 条路由规则`, 'success');
            } else if (res && res.error !== ('用户取消选择')) {
              this.showToast(('导入失败：') + res.error, 'error');
            }
          } catch (e) {
            this.showToast(('通信异常：') + e.message, 'error');
          }
        },
        clearRoutingRules() {
          if(confirm('⚠️ 警告：确定要清空所有自定义规则吗？(内置的系统规则不受影响)')) {
            this.routingRules = [];
            this.saveRoutingRules();
            this.showToast('已清空自定义规则', 'info');
          }
        },
        applyGeoPreset(preset) {
          if (!preset || !preset.rules) return;
          const existing = new Set(this.routingRules.map(r => r.type + ':' + r.content));
          let added = 0;
          for (const rule of preset.rules) {
            const key = rule.type + ':' + rule.content;
            if (!existing.has(key)) {
              this.routingRules.push({ id: Date.now() + added, enabled: rule.enabled !== false, negate: rule.negate || false, type: rule.type, content: rule.content, outbound: rule.outbound });
              existing.add(key);
              added++;
            }
          }
          this.saveRoutingRules();
          this.showToast(('✅ 已应用 ') + preset.name + (`，新增 ${added} 条规则`), 'success');
        },
        async showGeoStatus() {
          try {
            const res = await window.pywebview.api.get_geo_status();
            if (res && res.success) {
              const files = res.files || [];
              const ok = files.filter(f => f.exists);
              const msg = ok.map(f => `${f.name} (${f.size_kb}KB)`).join(', ') || ('未找到 Geo 文件');
              this.showToast(msg, ok.length > 0 ? 'info' : 'warning');
            }
          } catch (e) { this.showToast(e.message, 'error'); }
        },
        async applyConfigPreset(preset) {
          if (!preset) return;
          try {
            const res = await window.pywebview.api.apply_config_preset(preset.name);
            if (res && res.success) {
              // 应用路由规则
              if (preset.rules) {
                const existing = new Set(this.routingRules.map(r => r.type + ':' + r.content));
                let added = 0;
                for (const rule of preset.rules) {
                  const key = rule.type + ':' + rule.content;
                  if (!existing.has(key)) {
                    this.routingRules.push({ id: Date.now() + added, enabled: rule.enabled !== false, negate: rule.negate || false, type: rule.type, content: rule.content, outbound: rule.outbound });
                    existing.add(key);
                    added++;
                  }
                }
                this.saveRoutingRules();
              }
              // 应用 DNS 和代理模式设置
              if (preset.proxy_mode) this.proxyMode = preset.proxy_mode;
              if (preset.enable_custom_dns !== undefined) this.configState.enableCustomDns = preset.enable_custom_dns;
              if (preset.remote_dns) this.configState.remoteDns = preset.remote_dns;
              if (preset.local_dns) this.configState.localDns = preset.local_dns;
              if (preset.enable_fake_dns !== undefined) this.advancedState.enableFakeDns = preset.enable_fake_dns;
              this.showToast(('✅ 已应用: ') + preset.name, 'success');
            } else {
              this.showToast((res ? res.error : ''), 'error');
            }
          } catch (e) { this.showToast(e.message, 'error'); }
        },
        async exportRoutingRules() {
          if (this.routingRules.length === 0) {
            return this.showToast('没有可导出的规则', 'warning');
          }
          try {
            const rules = this.routingRules.map(({ id, enabled, negate, type, content, outbound }) => ({ id, enabled, negate: negate || false, type, content, outbound }));
            const res = await window.pywebview.api.export_routing_rules(JSON.stringify(rules));
            if (res && res.success) {
              this.showToast(('✅ 已导出 ') + res.count + (' 条规则到 ') + res.path, 'success');
            } else if (res && res.error !== '用户取消') {
              this.showToast(('导出失败：') + res.error, 'error');
            }
          } catch (e) {
            this.showToast(('通信异常：') + e.message, 'error');
          }
        },
        async importRoutingTemplateFromUrl(url) {
          if (!url || url === 'https://') return;
          try {
            this.showToast('正在下载模板...', 'info');
            const res = await window.pywebview.api.import_routing_template_from_url(url);
            if (res && res.success) {
              this.routingRules = [...res.rules, ...this.routingRules];
              this.saveRoutingRules();
              this.showToast(('✅ 成功导入 ') + res.count + (' 条模板规则'), 'success');
            } else {
              this.showToast(('导入失败：') + (res ? res.error : 'Unknown error'), 'error');
            }
          } catch (e) {
            this.showToast(('通信异常：') + e.message, 'error');
          }
        },
        async updateBgpAlias(event) {
          if (!this.selectedBgpNode) return;
          const val = event.target.value.trim();
          const indices = this.selectedBgpNode.groupedIndices || [this.selectedBgpNode.index];
          await Promise.all(indices.map(idx => {
            this.tunnels[idx].bgp_alias = val;
            return window.pywebview.api.update_tunnel(idx, this.tunnels[idx]);
          }));
          this.buildBgpTopology(this.lastBgpResults);
          this.showToast('拓扑专属名称已更新', 'success');
        },
        async updateBgpRouteType(event) {
          if (!this.selectedBgpNode) return;
          const val = event.target.value;
          const indices = this.selectedBgpNode.groupedIndices || [this.selectedBgpNode.index];
          await Promise.all(indices.map(idx => {
            this.tunnels[idx].bgp_manual = val;
            return window.pywebview.api.update_tunnel(idx, this.tunnels[idx]);
          }));
          this.buildBgpTopology(this.lastBgpResults);
          this.showToast('路由类型已批量更新 (' + indices.length + ' 个节点)', 'success');
        },
        initBgpListeners() {
          this.$nextTick(() => {
            document.addEventListener('visibilitychange', () => { this.bgpAppVisible = !document.hidden; this.bgpScheduleNext(); });
            window.addEventListener('blur', () => { this.bgpAppFocused = false; this.bgpScheduleNext(); });
            window.addEventListener('focus', () => { this.bgpAppFocused = true; this.bgpScheduleNext(); });
            this.bgpScheduleNext();
          });
        },
        bgpScheduleNext() {
          if (this.bgpAutoRefreshTimer) { clearTimeout(this.bgpAutoRefreshTimer); this.bgpAutoRefreshTimer = null; }
          var onTab = this.currentTab === 'routing-table';
          var visible = this.bgpAppVisible && this.bgpAppFocused;
          var interval = onTab ? 30000 : (visible ? 60000 : 600000);
          this.bgpAutoRefreshTimer = setTimeout(() => {
            if (!this.isBgpTesting) this.testBgpTopology();
            this.bgpScheduleNext();
          }, interval);
        },
        autoLayoutBgp() {
          this.$nextTick(() => {
            if (this.lastBgpResults && this.lastBgpResults.length > 0) {
              this.buildBgpTopology(this.lastBgpResults);
              this.showToast('已恢复自动物理排列', 'info');
            } else {
              this.showToast('暂无拓扑数据', 'warning');
            }
          });
        },

        async testBgpTopology() {
          if (this.isBgpTesting) return;
          if (this.tunnels.length === 0) return this.showToast('没有节点数据', 'warning');

          this.isBgpTesting = true;
          this.bgpEdges = [];
          this.bgpNodes = [];

          this.addLog('info', '[BGP] 启动双重探针拓扑探测...');
          try {
            var res = await window.pywebview.api.test_bgp_topology();
            if (res && res.success) {
              this.tunnels = res.tunnels;
              this.lastBgpResults = res.results;
              await this.$nextTick();
              this.buildBgpTopology(res.results);
              this.addLog('info', '[BGP] 探测完成，' + res.results.length + ' 个节点已分析');
              this.showToast('BGP 拓扑探测完成', 'success');
            } else {
              this.showToast('探测失败：' + (res ? res.error : '未知错误'), 'error');
            }
          } catch (e) {
            this.showToast('探测异常：' + e.message, 'error');
          } finally {
            this.isBgpTesting = false;
          }
        },

        buildBgpTopology(results) {
          // 过滤掉所有超时或无数据的死节点，只渲染真实打通的 BGP 链路
          this.lastBgpResults = (results || []).filter(r => r && r.latency_a > 0 && r.latency_b > 0);
          var nodes = [];
          var edges = [];
          var canvas = this.$refs.bgpCanvas;
          var canvasW = Math.max((canvas ? canvas.clientWidth : 900), 600);
          var canvasH = Math.max((canvas ? canvas.clientHeight : 500), 400);

          // --- 以视觉中心为基准的动态展开布局 ---
          const drawerPadding = this.showRouteList ? 290 : 40;
          const availableW = canvasW - drawerPadding;
          const centerX = drawerPadding + availableW / 2;
          const gap = Math.min(180, availableW * 0.22);
          const outerGap = Math.min(220, availableW * 0.28);
          const col2X = centerX - gap;       // 物理枢纽 (Hub)
          const col3X = centerX + gap;       // 逻辑出口 (Box)
          const col1X = col2X - outerGap;    // 入站节点
          const col4X = col3X + outerGap;    // Internet 出口

          // 1. Hub 按 IP 聚合（物理入口唯一性）
          var hubsMap = {};
          results.forEach(function(r) {
            if (!r) return;
            var nodeConf = this.tunnels[r.index];
            if (!nodeConf) return;
            var ip = nodeConf.server_addr;

            if (!hubsMap[ip]) {
              hubsMap[ip] = {
                id: 'hub-' + ip.replace(/[^a-zA-Z0-9]/g, '-'),
                type: 'transit',
                label: '入口:\n' + ip,
                latency: r.latency_a > 0 ? r.latency_a : null,
                children: []
              };
            } else if (r.latency_a > 0 && (hubsMap[ip].latency === null || r.latency_a < hubsMap[ip].latency)) {
              hubsMap[ip].latency = r.latency_a;
            }
          }.bind(this));

          // 2. 节点按 featureKey 去重（防止克隆节点重复出现）
          var uniqueNodesMap = {};
          results.forEach(function(r) {
            if (!r) return;
            var nodeConf = this.tunnels[r.index];
            if (!nodeConf) return;

            var featureKey = [
              nodeConf.protocol, nodeConf.server_addr, nodeConf.server_port,
              nodeConf.uuid || '', nodeConf.password || '', nodeConf.sni || '',
              nodeConf.ws_path || '', nodeConf.grpc_service_name || ''
            ].join('|');

            if (!uniqueNodesMap[featureKey]) {
              uniqueNodesMap[featureKey] = {
                r: r, nodeConf: nodeConf, count: 1, indices: [r.index],
                rtype: nodeConf.bgp_manual || r.bgp_auto || 'unknown'
              };
            } else {
              uniqueNodesMap[featureKey].count++;
              uniqueNodesMap[featureKey].indices.push(r.index);
            }
          }.bind(this));

          // 3. 将去重后的节点挂载到对应 Hub
          Object.values(uniqueNodesMap).forEach(function(group) {
            var ip = group.nodeConf.server_addr;
            if (hubsMap[ip]) hubsMap[ip].children.push(group);
          });

          var hubs = Object.values(hubsMap);
          var totalHubs = hubs.length;

          // 4. 坐标渲染与抗碰撞连线引擎
          var totalLogicalNodes = hubs.reduce(function(sum, hub) { return sum + hub.children.length; }, 0);

          // 设定安全的最小垂直间距，防止任何情况下的堆叠碰撞
          var childSpacing = Math.max(55, (canvasH - 140) / Math.max(totalLogicalNodes, 1));

          // 动态计算整棵树的起始 Y 坐标 (节点少居中，节点多从顶部排列)
          var totalTreeHeight = totalLogicalNodes * childSpacing + (hubs.length - 1) * 40;
          var currentY = Math.max(80, (canvasH - totalTreeHeight) / 2);

          // 动态画布高度
          var neededH = Math.max(canvasH, totalTreeHeight + 160);
          this.bgpCanvasHeight = neededH;

          nodes.push({ id: 'inbound', type: 'inbound', label: '入站 (' + this.configState.localPort + ')', x: col1X, y: canvasH / 2, latency: null });

          hubs.forEach(function(hub) {
            hub.x = col2X;
            var childrenHeight = Math.max(0, hub.children.length - 1) * childSpacing;

            // 物理枢纽 (Hub) 永远在它的子节点群正中央
            hub.y = currentY + childrenHeight / 2;
            nodes.push(hub);

            // 连线: 入站 -> Hub
            edges.push({
              id: 'e-in-' + hub.id, from: 'inbound', to: hub.id,
              x1: col1X + 40, y1: canvasH / 2, x2: col2X - 40, y2: hub.y,
              color: '#c084fc', dash: false
            });

            var startY = currentY;

            hub.children.forEach(function(child, j) {
              var nodeY = startY + (j * childSpacing);
              var nodeId = 'node-group-' + child.indices[0];

              var label = child.nodeConf.bgp_alias || child.nodeConf.out_tag;
              if (child.count > 1) label += ' (+' + (child.count - 1) + ')';

              nodes.push({
                id: nodeId, index: child.indices[0], type: 'node', label: label,
                rtype: child.rtype, latency: child.r.latency_b, latencyA: child.r.latency_a,
                exitIp: child.r.exit_ip || '', exitCountry: child.r.exit_country || '',
                exitCity: child.r.exit_city || '', exitResidential: child.r.exit_residential,
                x: col3X, y: nodeY, groupedIndices: child.indices
              });

              var edgeColor = child.rtype === 'direct' ? '#3b82f6' : (child.rtype === 'transit' ? '#f59e0b' : '#9ca3af');

              // 连线: Hub -> Node
              edges.push({
                id: 'e-' + hub.id + '-' + nodeId, from: hub.id, to: nodeId,
                x1: col2X + 40, y1: hub.y, x2: col3X - 60, y2: nodeY,
                color: edgeColor, dash: true
              });

              // 连线: Node -> Exit
              edges.push({
                id: 'e-' + nodeId + '-exit', from: nodeId, to: 'exit',
                x1: col3X + 60, y1: nodeY, x2: col4X - 60, y2: canvasH / 2,
                color: edgeColor, dash: true
              });
            });

            // 累加高度并为下一个 Hub 预留安全间距 (40px)
            currentY += childrenHeight + childSpacing + 40;
          });

          nodes.push({ id: 'exit', type: 'exit', label: '出口', x: col4X, y: canvasH / 2, latency: null });

          this.bgpNodes = nodes;
          this.bgpEdges = edges;
        },

        /** 转义 SVG 属性值中的特殊字符，防止 XSS 注入 */
        _escapeSvgAttr(str) {
          return String(str).replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
        },

        generateSvgEdges() {
          if (!this.bgpEdges || !this.bgpNodes || this.bgpNodes.length === 0) return '';

          return this.bgpEdges.map(edge => {
            var x1 = edge.x1 || 0;
            var y1 = edge.y1 || 0;
            var x2 = edge.x2 || 0;
            var y2 = edge.y2 || 0;

            var cp1x = x1 + Math.max(40, (x2 - x1) * 0.4);
            var cp2x = x2 - Math.max(40, (x2 - x1) * 0.4);
            var d = 'M ' + x1 + ' ' + y1 + ' C ' + cp1x + ' ' + y1 + ', ' + cp2x + ' ' + y2 + ', ' + x2 + ' ' + y2;

            var color = this._escapeSvgAttr(edge.color || '#9ca3af');
            var dashStyle = edge.dash ? 'stroke-dasharray: 6,4; animation: dash 1s linear infinite;' : '';

            return '<path d="' + d + '" stroke="' + color + '" stroke-width="2.5" fill="none" opacity="0.7" style="' + dashStyle + '" />';
          }).join('');
        },

        startDragBgpNode(event, node) {
          if (this.bgpCanvasLocked) return;
          this.bgpDragNode = node;
          const canvas = this.$refs.bgpCanvas;
          const rect = canvas.getBoundingClientRect();
          const offsetX = event.clientX - rect.left - node.x;
          const offsetY = event.clientY - rect.top - node.y;

          const onMove = (e) => {
            if (!this.bgpDragNode) return;
            node.x = e.clientX - rect.left - offsetX;
            node.y = e.clientY - rect.top - offsetY;

            // 实时吸附边缘连线，保持高帧率重绘
            this.bgpEdges.forEach(edge => {
              if (edge.from === node.id) {
                const offset = node.type === 'inbound' ? 60 : (node.type === 'transit' ? 50 : 60);
                edge.x1 = node.x + offset;
                edge.y1 = node.y;
              }
              if (edge.to === node.id) {
                const offset = node.type === 'exit' ? 60 : (node.type === 'transit' ? 50 : 60);
                edge.x2 = node.x - offset;
                edge.y2 = node.y;
              }
            });
          };
          const onUp = () => {
            this.bgpDragNode = null;
            window.removeEventListener('mousemove', onMove);
            window.removeEventListener('mouseup', onUp);
          };
          window.addEventListener('mousemove', onMove);
          window.addEventListener('mouseup', onUp);
        }
};

/**
 * X-Matrix — 监控模块
 * 流量图表、日志系统、连接雷达
 */
window.XMatrixMonitor = {
        radarConnections: [],
        isRadarAutoRefresh: true,
        radarFilter: 'all',
        logControls: {
          autoRefresh: true,
          autoScroll: true,
          level: 'info',
          outboundFilter: 'all'
        },
        logs: [],
        filteredLogs() {
          const levelMap = {debug:1, info:2, warning:3, error:4};
          const minLevel = levelMap[this.logControls.level] || 0;
          const obFilter = this.logControls.outboundFilter;
          return this.logs.filter(l => {
            const levelOk = minLevel === 0 || (levelMap[l.level] || 0) >= minLevel;
            const outboundOk = obFilter === 'all' || (l.outbound || '') === obFilter;
            return levelOk && outboundOk;
          });
        },
        syncTunnels(newTunnels) {
          // 空值保护：如果传入无效数据，保持现有状态
          if (!Array.isArray(newTunnels)) {
            console.warn('[syncTunnels] 无效的节点数据:', newTunnels);
            return;
          }

          // 建立旧状态映射表 (同时保留 delay 和 active)
          const stateMap = {};
          if (Array.isArray(this.tunnels)) {
            this.tunnels.forEach(t => {
              stateMap[t.out_tag] = { delay: t.delay, active: t.active };
            });
          }

          // 强制通过 map 重构数组，为所有新节点显式注入前端专属的响应式字段
          // 彻底解决 Alpine 因为对象属性缺失导致 DOM 重用时出现 "Timeout 幽灵" 和状态丢失的 Bug
          this.tunnels = newTunnels.map(t => {
            const oldState = stateMap[t.out_tag] || {};
            return {
              ...t,
              delay: oldState.delay !== undefined ? oldState.delay : undefined,
              active: oldState.active !== undefined ? oldState.active : false
            };
          });
        },
        addLog(level, msg) {
          this.logs.push({ id: Date.now() + Math.random(), time: new Date().toLocaleTimeString('en-GB'), level, msg, outbound: '' });
          if (this.logs.length > 300) this.logs = this.logs.slice(-300);
        },
        async copyAllLogs() {
          const text = this.logs.map(l => `[${l.time}] [${l.level.toUpperCase()}] ${l.msg}`).join('\n');
          try {
            await navigator.clipboard.writeText(text);
            this.showToast('✅ 日志已复制到剪贴板', 'success');
          } catch (e) {
            this.showToast('复制失败', 'error');
          }
        },
        clearLogs() {
          this.logs = [];
          this.showToast('日志已清空', 'info');
        },
        // 流量图表资源引用（用于清理）
        _trafficChart: null,
        _trafficIntervalId: null,
        _trafficResizeHandler: null,

        initTrafficChart() {
          // 清理旧实例，防止重复初始化导致内存泄漏
          this._cleanupTrafficChart();

          const chartDom = document.getElementById('traffic-chart');
          if (!chartDom) return;
          const myChart = echarts.init(chartDom);
          this._trafficChart = myChart;

          const option = {
            animation: false,
            tooltip: {
              trigger: 'axis',
              axisPointer: { type: 'line' },
              appendToBody: true,
              backgroundColor: 'rgba(255, 255, 255, 0.95)',
              borderColor: '#f3f4f6',
              padding: [8, 12],
              textStyle: { color: '#374151', fontSize: 12, fontFamily: 'monospace' },
              valueFormatter: (value) => parseFloat(value).toFixed(2) + ' KB/s'
            },
            grid: { left: -10, right: -10, top: 10, bottom: -10, containLabel: false },
            xAxis: { type: 'category', boundaryGap: false, data: Array(60).fill(''), show: false },
            yAxis: { type: 'value', show: false, min: 0 },
            series: [
              {
                name: '上传', type: 'line', smooth: true, symbol: 'none',
                itemStyle: { color: '#f59e0b' },
                areaStyle: {
                  color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
                    { offset: 0, color: 'rgba(245, 158, 11, 0.3)' },
                    { offset: 1, color: 'rgba(245, 158, 11, 0.05)' }
                  ])
                },
                data: []
              },
              {
                name: '下载', type: 'line', smooth: true, symbol: 'none',
                itemStyle: { color: '#3b82f6' },
                areaStyle: {
                  color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
                    { offset: 0, color: 'rgba(59, 130, 246, 0.3)' },
                    { offset: 1, color: 'rgba(59, 130, 246, 0.05)' }
                  ])
                },
                data: []
              }
            ]
          };
          myChart.setOption(option);

          // 存储引用以便清理
          this._trafficResizeHandler = () => myChart.resize();
          window.addEventListener('resize', this._trafficResizeHandler);

          this._trafficIntervalId = setInterval(async () => {
            if (this.currentTab === 'home') {
              try {
                const status = await window.pywebview.api.get_core_status();
                this.coreState.isRunning = status.running;
              } catch (e) { /* 轮询核心状态静默 */ }
              try {
                const stats = await window.pywebview.api.fetch_traffic_stats();
                if (stats) {
                  const formatBytes = this.formatBytes.bind(this);
                  this.traffic.upSpeed = formatBytes(stats.up_speed) + '/s';
                  this.traffic.downSpeed = formatBytes(stats.down_speed) + '/s';
                  this.traffic.upTotal = formatBytes(stats.up);
                  this.traffic.downTotal = formatBytes(stats.down);
                  this.traffic.connections = stats.connections !== undefined ? stats.connections : 0;
                  this.traffic.memory = stats.memory !== undefined ? stats.memory + ' MB' : '0.00 MB';
                  const newUpKb = stats.up_speed / 1024;
                  const newDownKb = stats.down_speed / 1024;

                  this.traffic.historyUp.push(newUpKb);
                  this.traffic.historyDown.push(newDownKb);

                  // 维护最大 600 个点 (10分钟) 的历史记录池
                  if (this.traffic.historyUp.length > 600) {
                    this.traffic.historyUp.shift();
                    this.traffic.historyDown.shift();
                  }

                  // 根据用户选择的区间动态截取，并在启动初期向前补零保持平滑
                  const range = this.traffic.chartRange;
                  const padLength = Math.max(0, range - this.traffic.historyUp.length);
                  const padArray = Array(padLength).fill(0);

                  const displayUp = padArray.concat(this.traffic.historyUp).slice(-range);
                  const displayDown = padArray.concat(this.traffic.historyDown).slice(-range);

                  myChart.setOption({
                    xAxis: { data: Array(range).fill('') },
                    series: [{ data: displayUp }, { data: displayDown }]
                  });
                }
                const newLogs = await window.pywebview.api.fetch_logs();
                if (newLogs && newLogs.length > 0) {
                  newLogs.forEach(logStr => {
                    // V2RayN 风格日志解析引擎
                    const raw = logStr.trim();
                    if (!raw) return;

                    // 提取原始时间戳 (2026/06/26 23:56:28.221927)
                    let time = '';
                    const tsMatch = raw.match(/^(\d{4}\/\d{2}\/\d{2}\s+\d{2}:\d{2}:\d{2})/);
                    if (tsMatch) {
                      const parts = tsMatch[1].split(/[\s/:]/);
                      time = parts[3] + ':' + parts[4] + ':' + parts[5];
                    } else {
                      const now = new Date();
                      time = String(now.getHours()).padStart(2, '0') + ':' + String(now.getMinutes()).padStart(2, '0') + ':' + String(now.getSeconds()).padStart(2, '0');
                    }

                    // 提取日志级别
                    let level = 'info';
                    if (raw.includes('[Warning]') || raw.includes('[warning]')) level = 'warning';
                    else if (raw.includes('[Error]') || raw.includes('[error]')) level = 'error';

                    // 提取连接 ID
                    let connId = '';
                    const idMatch = raw.match(/\[(\d{6})\]/);
                    if (idMatch) connId = idMatch[1];

                    // 提取子系统标签 (proxy/http, app/dispatcher, transport/internet/tcp 等)
                    let subsystem = '';
                    const subMatch = raw.match(/\]\s+([\w/.]+):\s/);
                    if (subMatch) subsystem = subMatch[1];

                    // 提取出站类型 (从 accepted 行的 [in -> out] 格式)
                    let outbound = '';
                    const obMatch = raw.match(/\[.+? -> (.+?)\]/);
                    if (obMatch) {
                      const tag = obMatch[1].toLowerCase();
                      if (tag === 'direct') outbound = 'direct';
                      else if (tag === 'block') outbound = 'block';
                      else outbound = 'proxy';
                    }

                    // 日志分类 (用于样式)
                    let category = 'system';
                    if (subsystem.startsWith('proxy/') || subsystem.startsWith('app/')) category = 'routing';
                    else if (subsystem.startsWith('transport/')) category = 'transport';
                    else if (raw.includes('accepted')) category = 'connection';
                    else if (raw.includes('[系统]') || raw.includes('[探测]') || raw.includes('[检测]') || raw.includes('[测速]') || raw.includes('[TCP') || raw.includes('[真连接]') || raw.includes('[导入]') || raw.includes('[剪贴板]') || raw.includes('[代理]') || raw.includes('[节点]')) category = 'app';

                    this.logs.push({ id: Date.now() + Math.random(), time, level, msg: raw, outbound, connId, subsystem, category });
                  });
                  if (this.logs.length > 300) this.logs = this.logs.slice(-300);
                  if (this.logControls.autoScroll) {
                    setTimeout(() => {
                      const container = document.getElementById('log-container');
                      if (container) container.scrollTop = container.scrollHeight;
                    }, 50);
                  }
                }

                // 连接监控雷达数据拉取
                if (this.currentTab === 'radar' && this.isRadarAutoRefresh) {
                  try {
                    const connRes = await window.pywebview.api.fetch_connections();
                    if (connRes && connRes.success) this.radarConnections = connRes.connections;
                  } catch(e) { /* 雷达数据轮询静默 */ }
                }
              } catch (e) {
                // 静默忽略轮询错误
              }
            }
          }, 1000);
        },

        /** 清理流量图表的定时器、监听器和 ECharts 实例，防止内存泄漏 */
        _cleanupTrafficChart() {
          if (this._trafficIntervalId) {
            clearInterval(this._trafficIntervalId);
            this._trafficIntervalId = null;
          }
          if (this._trafficResizeHandler) {
            window.removeEventListener('resize', this._trafficResizeHandler);
            this._trafficResizeHandler = null;
          }
          if (this._trafficChart) {
            this._trafficChart.dispose();
            this._trafficChart = null;
          }
}
};

/**
 * X-Matrix — 检测模块
 * IP 检测、质量检测、WebRTC 泄漏、网站测试
 */
window.XMatrixInspection = {
        ipInfo: {
          country: '未知', countryCode: 'un', ip: '0.0.0.0', asn: '-',
          isp: '-', org: '-',
          location: '-', timezone: '-'
        },
        isRefreshingIp: false,
        showIp: false,
        ipQualityState: {
          isChecking: false,
          isDropdownOpen: false,
          source: 'ippure',
          selectedPort: 2077,
          availablePorts: [],
          availableSources: [
            { id: 'ippure', name: 'IPPure (专业欺诈库)' },
            { id: 'ipinfo', name: 'IPinfo (商业地理库)' },
            { id: 'ipapi', name: 'IP-API (免费地理库)' }
          ],
          webrtc: { status: 'idle', ip: '-' },
          data: {
            ip: '0.0.0.0',
            asn: '-',
            org: '-',
            country: '未知',
            countryCode: 'un',
            city: '',
            isResidential: false,
            fraudScore: 0,
            isProxy: null
          },
          websites: [
            { id: 1, name: 'Google', iconText: 'G', bgColor: 'bg-blue-100', textColor: 'text-blue-600', url: 'https://www.google.com/generate_204', state: 'idle', ms: 0 },
            { id: 2, name: 'YouTube', iconText: 'Y', bgColor: 'bg-rose-100', textColor: 'text-rose-600', url: 'https://www.youtube.com', state: 'idle', ms: 0 },
            { id: 3, name: 'ChatGPT', iconText: 'AI', bgColor: 'bg-emerald-100', textColor: 'text-emerald-600', url: 'https://chatgpt.com', state: 'idle', ms: 0 },
            { id: 4, name: 'Netflix', iconText: 'N', bgColor: 'bg-red-100', textColor: 'text-red-600', url: 'https://www.netflix.com', state: 'idle', ms: 0 },
            { id: 5, name: 'Disney+', iconText: 'D+', bgColor: 'bg-indigo-100', textColor: 'text-indigo-600', url: 'https://www.disneyplus.com', state: 'idle', ms: 0 },
            { id: 6, name: 'TikTok', iconText: 'TK', bgColor: 'bg-zinc-200', textColor: 'text-zinc-800', url: 'https://www.tiktok.com', state: 'idle', ms: 0 },
            { id: 7, name: 'Telegram', iconText: 'TG', bgColor: 'bg-sky-100', textColor: 'text-sky-600', url: 'https://web.telegram.org', state: 'idle', ms: 0 },
            { id: 8, name: 'Spotify', iconText: 'SP', bgColor: 'bg-green-100', textColor: 'text-green-600', url: 'https://www.spotify.com', state: 'idle', ms: 0 }
          ],
          globalPing: [
            { id: 'cn', name: '中国大陆 (Baidu)', country: 'cn', url: 'https://www.baidu.com', state: 'idle', ms: 0 },
            { id: 'sg', name: '新加坡 (AWS)', country: 'sg', url: 'https://dynamodb.ap-southeast-1.amazonaws.com/', state: 'idle', ms: 0 },
            { id: 'us', name: '美东 (AWS)', country: 'us', url: 'https://dynamodb.us-east-1.amazonaws.com/', state: 'idle', ms: 0 },
            { id: 'jp', name: '日本 (AWS)', country: 'jp', url: 'https://dynamodb.ap-northeast-1.amazonaws.com/', state: 'idle', ms: 0 },
            { id: 'gb', name: '英国 (AWS)', country: 'gb', url: 'https://dynamodb.eu-west-2.amazonaws.com/', state: 'idle', ms: 0 },
            { id: 'au', name: '澳洲 (AWS)', country: 'au', url: 'https://dynamodb.ap-southeast-2.amazonaws.com/', state: 'idle', ms: 0 }
          ]
        },
        async testWebsite(index) {
          const site = this.ipQualityState.websites[index];
          site.state = 'testing';
          try {
            const res = await window.pywebview.api.test_website_access(site.url, this.configState.localPort);
            if (res.success) {
              site.state = res.status === 200 || res.status === 204 ? 'success' : 'warning';
              site.ms = res.ms;
            } else {
              site.state = 'error';
            }
          } catch (e) {
            site.state = 'error';
          }
        },
        async testAllWebsites() {
          this.ipQualityState.websites.forEach((_, i) => this.testWebsite(i));
        },
        async testPing(index) {
          const ping = this.ipQualityState.globalPing[index];
          ping.state = 'testing';
          try {
            const res = await window.pywebview.api.test_website_access(ping.url, this.configState.localPort);
            if (res.success) { ping.state = 'success'; ping.ms = res.ms; }
            else { ping.state = 'error'; }
          } catch (e) { ping.state = 'error'; }
        },
        async testAllPings() {
          this.ipQualityState.globalPing.forEach((_, i) => this.testPing(i));
        },
        checkWebRTC() {
          this.ipQualityState.webrtc = { status: 'checking', ip: '正在检测...' };
          try {
            const rtc = new RTCPeerConnection({ iceServers: [{ urls: "stun:stun.l.google.com:19302" }] });
            rtc.createDataChannel("xmatrix");
            rtc.createOffer().then(offer => rtc.setLocalDescription(offer)).catch(() => {
              this.ipQualityState.webrtc = { status: 'safe', ip: '浏览器已拦截' };
            });
            rtc.onicecandidate = (event) => {
              if (event.candidate && event.candidate.candidate) {
                const match = /([0-9]{1,3}(\.[0-9]{1,3}){3})/.exec(event.candidate.candidate);
                if (match) {
                  const ip = match[1];
                  if (!ip.startsWith("192.168") && !ip.startsWith("10.") && !ip.startsWith("172.")) {
                    const isLeaking = (ip !== this.ipQualityState.data.ip && this.ipQualityState.data.ip !== '0.0.0.0');
                    this.ipQualityState.webrtc = { status: isLeaking ? 'leaked' : 'safe', ip: ip };
                    rtc.close();
                  }
                }
              }
            };
            setTimeout(() => {
              if (this.ipQualityState.webrtc.status === 'checking') {
                this.ipQualityState.webrtc = { status: 'safe', ip: '未检测到泄漏' };
              }
              try { rtc.close(); } catch(e) { /* RTC 关闭静默 */ }
            }, 3000);
          } catch (e) {
            this.ipQualityState.webrtc = { status: 'safe', ip: '环境不支持' };
          }
        },
        addCustomWebsite() {
          const url = prompt('请输入要测试的网站完整 URL (例如: https://www.netflix.com):', 'https://');
          if (!url || !url.startsWith('http')) return;
          const name = prompt('请输入网站简称 (例如: Netflix):', 'Custom');
          if (!name) return;
          const iconText = name.substring(0, 2).toUpperCase();
          const colors = ['bg-indigo-100 text-indigo-600', 'bg-cyan-100 text-cyan-600', 'bg-fuchsia-100 text-fuchsia-600', 'bg-violet-100 text-violet-600', 'bg-orange-100 text-orange-600'];
          const randomColor = colors[Math.floor(Math.random() * colors.length)];
          this.ipQualityState.websites.push({
            id: Date.now(), name: name, iconText: iconText,
            bgColor: randomColor.split(' ')[0], textColor: randomColor.split(' ')[1],
            url: url, state: 'idle', ms: 0
          });
          this.saveCustomWebsites();
        },
        removeCustomWebsite(id) {
          if (!confirm('确定移除该测试项吗？')) return;
          this.ipQualityState.websites = this.ipQualityState.websites.filter(w => w.id !== id);
          this.saveCustomWebsites();
        },
        saveCustomWebsites() {
          localStorage.setItem('xmatrix_custom_sites', JSON.stringify(this.ipQualityState.websites));
        },
        loadCustomWebsites() {
          const saved = localStorage.getItem('xmatrix_custom_sites');
          if (saved) {
            try { this.ipQualityState.websites = JSON.parse(saved); } catch (e) { console.warn('[检测] 加载自定义测试网站失败:', e); }
          }
        },
        async refreshIpInfo() {
          if (this.isRefreshingIp) return;
          this.isRefreshingIp = true;
          this.ipInfo.isp = '正在连接内核并探测出口...';
          this.addLog('info', '[探测] 正在查询出口 IP...');
          try {
            await new Promise(resolve => setTimeout(resolve, 1500));
            const res = await window.pywebview.api.check_outbound_ip(this.configState.localPort);
            if (res && res.success) {
              this.ipInfo = { ...this.ipInfo, ...res };
              this.addLog('info', `[探测] 出口 IP: ${res.ip} (${res.country}, ${res.org || res.isp})`);
            } else {
              this.ipInfo.isp = res ? res.error : ('探测失败');
            }
          } catch (e) {
            this.ipInfo.isp = '通信异常';
          } finally {
            this.isRefreshingIp = false;
          }
        },
        resetIpDetection() {
          this.ipQualityState.data = {
            ip: '0.0.0.0', asn: '-', org: '-',
            country: '未知', countryCode: 'un', city: '',
            isResidential: false, fraudScore: 0, isProxy: null
          };
          this.ipQualityState.webrtc = { status: 'idle', ip: '-' };
        },
        async checkIpQuality() {
          this.ipQualityState.isChecking = true;
          try {
            var res = await window.pywebview.api.check_ip_quality(this.ipQualityState.selectedPort, this.ipQualityState.source);
            if (res && res.success && res.data) {
              // 绝对安全的合并方式，不会破坏对象地址和层级
              this.ipQualityState.data = Object.assign({}, this.ipQualityState.data, res.data);
            } else {
              this.showToast('❌ ' + ('检测失败：') + (res ? res.error : ('未知错误')), 'error');
            }
          } catch (e) {
            this.showToast('❌ ' + ('前后端通信崩溃：') + e.message, 'error');
          } finally {
            this.ipQualityState.isChecking = false;
            this.checkWebRTC();
          }
        }
};

/**
 * X-Matrix — 通用工具函数
 * 提供安全解析、格式化等基础工具
 */

// 全局函数别名（供 state.js / config.js 等模块直接调用）
function _safeJsonParse(key, defaultValue) {
  try {
    var raw = localStorage.getItem(key);
    if (raw === null) return defaultValue;
    return JSON.parse(raw);
  } catch { return defaultValue; }
}

window.XMatrixUtils = {
  /** 安全解析 localStorage 中的 JSON 值，解析失败时返回默认值 */
  _safeJsonParse(key, defaultValue) {
    try {
      var raw = localStorage.getItem(key);
      if (raw === null) return defaultValue;
      return JSON.parse(raw);
    } catch { return defaultValue; }
  },

  /** 格式化字节数为人类可读字符串 (保留两位小数) */
  formatBytes(bytes) {
    if (!bytes || bytes === 0) return '0.00 B';
    const k = 1024, sizes = ['B', 'KB', 'MB', 'GB'], i = Math.floor(Math.log(bytes) / Math.log(k));
    return (bytes / Math.pow(k, i)).toFixed(2) + ' ' + sizes[i];
  },

  /** 格式化字节数为简短字符串 (一位小数或整数) */
  formatBytesShort(bytes) {
    if (!bytes || bytes === 0) return '0 B';
    const k = 1024, sizes = ['B', 'KB', 'MB', 'GB'], i = Math.floor(Math.log(bytes) / Math.log(k));
    return (bytes / Math.pow(k, i)).toFixed(i > 1 ? 1 : 0) + ' ' + sizes[i];
  },
};

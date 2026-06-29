/**
 * X-Matrix — 翻译函数
 * 将英文 Tab ID 映射为中文标签
 */
window.XMatrixI18n = {
  t(key) {
    var map = {
      'home': '首页',
      'config': '配置',
      'routing': '路由',
      'advanced': '进阶',
      'routing-table': '路由表',
      'radar': '连接监控',
      'inspection': '检测',
      'settings': '设置'
    };
    return map[key] || key;
  }
};

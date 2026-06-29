/**
 * X-Matrix — Alpine.js 主组装入口
 * 合并所有模块，注册 Alpine.data('app')
 */
document.addEventListener('alpine:init', () => {
  Alpine.data('app', () => {
    var app = Object.assign({},
      window.XMatrixUtils,
      window.XMatrixI18n,
      window.XMatrixState,
      window.XMatrixNodes,
      window.XMatrixConfig,
      window.XMatrixRouting,
      window.XMatrixMonitor,
      window.XMatrixInspection,
      window.XMatrixSettings
    );

    // 合并 init 方法
    var moduleInit = window.XMatrixInit && window.XMatrixInit.init ? window.XMatrixInit.init : null;
    app.init = function() {
      if (moduleInit) moduleInit.call(this);
    };

    return app;
  });
});

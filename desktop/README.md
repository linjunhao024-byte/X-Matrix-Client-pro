# XMatrix.Desktop

Avalonia 11 桌面客户端骨架（P0 阶段）。它只做一件事：读 core 进程写下的
`core.endpoint`，通过 loopback HTTP + `X-Core-Token` 显示节点与内核状态，并把
「启动/停止内核」转成 core 的 POST 请求。**界面自己不启动内核、不碰系统代理。**

## 构建

需要 .NET SDK 10（本机装在 `D:\CODE.config\dotnet`，不在 PATH 里）：

```powershell
$env:DOTNET_ROOT = 'D:\CODE.config\dotnet'
$env:PATH = 'D:\CODE.config\dotnet;' + $env:PATH
dotnet build desktop\XMatrix.Desktop\XMatrix.Desktop.csproj -c Debug
```

产物：`desktop\XMatrix.Desktop\bin\Debug\net10.0\XMatrix.Desktop.exe`。

## 和 core 一起跑

两个进程必须用**同一个 Windows 用户**跑，因为端点文件在用户 AppData 下。

1. 起 core（无窗口，写端点文件后返回提示）：

   ```powershell
   cd src-tauri
   .\target\release\x-matrix.exe --serve --port 47100
   ```

   端口写 `0` 表示让系统分配；实际端口、token、pid 写在
   `%APPDATA%\com.xmatrix.client\core.endpoint`（JSON，tmp+rename 落盘）。

2. 另开一个终端起桌面端：

   ```powershell
   $env:DOTNET_ROOT = 'D:\CODE.config\dotnet'
   $env:PATH = 'D:\CODE.config\dotnet;' + $env:PATH
   dotnet run --project desktop\XMatrix.Desktop\XMatrix.Desktop.csproj
   ```

   打开窗口时会自动读端点文件并拉 `/nodes`、`/status`；点「刷新」可重新读。
   状态栏会显示当前 core 的 pid 与端口，底部一行显示内核/系统代理/混合端口。

3. 收尾只关 core：

   ```powershell
   $token = (Get-Content "$env:APPDATA\com.xmatrix.client\core.endpoint" | ConvertFrom-Json).token
   curl.exe -s -X POST -H "X-Core-Token: $token" http://127.0.0.1:47100/shutdown
   ```

   窗口随之显示「未连接」，这是预期行为；`core.endpoint` 会被删除。

## 边界（P0 故意不做的）

- **不启动内核**：启动按钮只是把 `{kind:"xray", nodeId}` POST 给 core；真正的
  内核拉起、端口占用检查、系统代理开关全部在 core 内完成。本机 dev 上不要用它
  起内核（硬规则）。
- **不读注册表**：系统代理状态来自 core 的 `/status`，不是界面自己查。
- **订阅/设置页**只是占位，提示「P0 未接」；P1 接真数据。
- 端点文件名带 `.endpoint`，不叫 `.json`：它不是用户配置，是进程间握手的临时
  凭证，删掉只会让界面显示「未连接」。

## 已知缺口

- 没有自动重连：core 重启后需点「刷新」（P1 做 endpoint 文件 watcher）。
- 托盘、开机自启、主题跟随系统都未接；当前固定 light 主题，颜色取自
  `src/styles/tokens.css`。
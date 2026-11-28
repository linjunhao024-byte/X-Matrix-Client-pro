using System.Collections.ObjectModel;
using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Threading;
using Avalonia.Media;
using XMatrix.Desktop.Models;
using XMatrix.Desktop.Services;

namespace XMatrix.Desktop;

public partial class MainWindow : Window
{
    private readonly ObservableCollection<NodeItem> _nodes = [];
    private CoreClient? _client;
    private CoreStatus? _status;
    private bool _busy;

    // The status palette, same source as tokens.css.
    private static readonly IBrush Accent = new SolidColorBrush(Color.Parse("#5B5BD6"));
    private static readonly IBrush AccentSoft = new SolidColorBrush(Color.Parse("#E9E9FB"));
    private static readonly IBrush Danger = new SolidColorBrush(Color.Parse("#B91C1C"));
    private static readonly IBrush DangerSoft = new SolidColorBrush(Color.Parse("#FBEBEB"));
    private static readonly IBrush Success = new SolidColorBrush(Color.Parse("#15803D"));
    private static readonly IBrush Tertiary = new SolidColorBrush(Color.Parse("#6B7280"));

    public MainWindow()
    {
        InitializeComponent();
        NodeList.ItemsSource = _nodes;
        ShowPage("home");
        Opened += async (_, _) =>
        {
            // A throw in a size fix must not swallow the first refresh: on the
            // 1280×800 test box it did exactly that, leaving an empty node list
            // that only the 刷新 button could fill.
            try
            {
                FitsScreen();
            }
            catch (Exception error)
            {
                Note(error);
            }
            await RefreshAsync();
        };

        // WindowState has to be set after the window exists, and it is the
        // backstop for screens where even the shrink below cannot fit the
        // design size (Avalonia does not clamp the window for us).
        Opened += (_, _) =>
        {
            try
            {
                if (!FitsScreen())
                {
                    WindowState = WindowState.Maximized;
                }
            }
            catch (Exception error)
            {
                Note(error);
            }

            // The platform applies Width/Height asynchronously and lays the
            // content out once at the old size. Measured: the right-aligned
            // toolbar stayed outside the visible area after the shrink until a
            // second layout pass ran. Settle again after the dust does.
            var timer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(500) };
            timer.Tick += (_, _) =>
            {
                timer.Stop();
                try
                {
                    SettlePlacement();
                    UpdateLayout();
                }
                catch (Exception error)
                {
                    Note(error);
                }
            };
            timer.Start();
        };
    }

    /// <summary>
    /// The design size (1120×720) assumes a 1080p desktop at 100%. On a smaller
    /// or scaled screen that window would hang off the edge, taking the page
    /// buttons with it — measured at 1280×800 and 200%, where "启动内核" sat
    /// outside the display. Shrink to the working area instead, and lower the
    /// minimum the same way so the clamp can always win.
    /// </summary>
    private bool FitsScreen()
    {
        // Avalonia's Screen.WorkingArea lies on this box: it reports a
        // 1920×1140 desktop while the real display is 1280×800 at 1.5×, so a
        // 1120-wide window "fits" and gets pushed off the left edge — measured,
        // the nav column sat at x = -42. Ask Win32 for the physical work area
        // instead, shrink against that, and place the window inside it here;
        // this is the only place that knows both units.
        var host = Screens.ScreenFromWindow(this) ?? Screens.Primary;
        var scale = host is null || host.Scaling <= 0 ? 1 : host.Scaling;

        var work = WorkArea();
        if (work.Width <= 0 || work.Height <= 0)
        {
            return true;
        }

        var maxWidth = work.Width / scale;
        var maxHeight = work.Height / scale;
        MinWidth = Math.Min(MinWidth, maxWidth);
        MinHeight = Math.Min(MinHeight, maxHeight);
        if (Width > maxWidth)
        {
            Width = Math.Max(MinWidth, maxWidth);
        }
        if (Height > maxHeight)
        {
            Height = Math.Max(MinHeight, maxHeight);
        }

        var physicalWidth = (int)Math.Round(Width * scale);
        var physicalHeight = (int)Math.Round(Height * scale);
        Position = new PixelPoint(
            work.X + Math.Max(0, (work.Width - physicalWidth) / 2),
            work.Y + Math.Max(0, (work.Height - physicalHeight) / 2));
        SettlePlacement();
        return Width <= maxWidth && Height <= maxHeight;
    }

    private const int SmCxScreen = 0;
    private const int SmCyScreen = 1;

    [DllImport("user32.dll", SetLastError = true)]
    private static extern int GetSystemMetrics(int index);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool GetWindowRect(IntPtr handle, out NativeRect rect);

    /// <summary>
    /// Measured, not assumed. On the test box Avalonia's Scaling said 1.5 while
    /// the window rendered 1 pixel per DIP, so Width x scale overshot the display
    /// by 15 px: the right edge was cut at 844 and the nav sat at x = 0. Ask for
    /// the real window rect and the real screen, then shrink and nudge until it
    /// genuinely fits.
    /// </summary>
    private void SettlePlacement()
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }

        var handle = TryGetPlatformHandle()?.Handle ?? IntPtr.Zero;
        var screenWidth = GetSystemMetrics(SmCxScreen);
        var screenHeight = GetSystemMetrics(SmCyScreen);
        if (handle == IntPtr.Zero || screenWidth <= 0 || screenHeight <= 0)
        {
            return;
        }

        // A shrunk window still measured its content at the old width on the
        // test box (right-aligned toolbar off-screen). Maximizing hands both the
        // size and the layout pass to Windows, which is what the design size
        // assumes anyway on a display this small.
        if (screenWidth < 1140 || screenHeight < 760)
        {
            if (WindowState != WindowState.Maximized)
            {
                WindowState = WindowState.Maximized;
            }
            return;
        }

        for (var pass = 0; pass < 3; pass++)
        {
            if (!GetWindowRect(handle, out var rect))
            {
                return;
            }

            // Compare the real rect against the real screen: a window can be
            // narrower than the display and still hang off the right edge
            // (measured 841 px wide at x = 9 on an 844 px screen).
            var overRight = rect.Right - screenWidth;
            var overBottom = rect.Bottom - screenHeight;
            var underLeft = -rect.Left;
            var underTop = -rect.Top;
            if (overRight <= 0 && overBottom <= 0 && underLeft <= 0 && underTop <= 0)
            {
                return;
            }

            var width = rect.Right - rect.Left;
            var height = rect.Bottom - rect.Top;
            var dipWidth = Bounds.Width <= 1 ? width : Bounds.Width;
            var dipHeight = Bounds.Height <= 1 ? height : Bounds.Height;
            var factorX = width / dipWidth;
            var factorY = height / dipHeight;

            if (overRight > 0 && factorX > 0)
            {
                var target = Math.Max(320, Math.Floor(dipWidth - (overRight + 10) / factorX));
                MinWidth = Math.Min(MinWidth, target);
                Width = target;
            }

            if (overBottom > 0 && factorY > 0)
            {
                var target = Math.Max(240, Math.Floor(dipHeight - (overBottom + 10) / factorY));
                MinHeight = Math.Min(MinHeight, target);
                Height = target;
            }

            Position = new PixelPoint(
                Math.Max(0, Position.X - Math.Max(0, underLeft)),
                Math.Max(0, Position.Y - Math.Max(0, underTop)));
            UpdateLayout();
            Dispatcher.UIThread.RunJobs();
        }
    }

    private const uint SpiGetWorkArea = 0x0030;

    [StructLayout(LayoutKind.Sequential)]
    private struct NativeRect
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool SystemParametersInfo(uint action, uint param, out NativeRect data, uint winIni);

    /// <summary>
    /// The physical work area, excluding the taskbar. Falls back to whatever
    /// Avalonia thinks when the call is unavailable.
    /// </summary>
    private PixelRect WorkArea()
    {
        if (OperatingSystem.IsWindows()
            && SystemParametersInfo(SpiGetWorkArea, 0, out var native, 0)
            && native.Right > native.Left
            && native.Bottom > native.Top)
        {
            return new PixelRect(
                native.Left,
                native.Top,
                native.Right - native.Left,
                native.Bottom - native.Top);
        }

        var screen = Screens.ScreenFromWindow(this) ?? Screens.Primary;
        return screen?.WorkingArea ?? default;
    }

    /// <summary>
    /// A one-line breadcrumb next to the binary: this window has no console, and
    /// a swallowed exception there reads as "the app did nothing". Only failures
    /// and the last operation's error are written.
    /// </summary>
    private static void Log(string message)
    {
        try
        {
            File.AppendAllText(
                Path.Combine(AppContext.BaseDirectory, "shell.log"),
                DateTime.Now.ToString("HH:mm:ss") + " " + message + Environment.NewLine);
        }
        catch
        {
            // Logging must never be the reason a window fails to open.
        }
    }

    private static void Note(Exception error) => Log($"{error.GetType().Name}: {error.Message}");

    // ---- navigation -------------------------------------------------------

    private void OnNavClick(object? sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string page })
        {
            ShowPage(page);
        }
    }

    private void OnGoNodes(object? sender, RoutedEventArgs e) => ShowPage("nodes");

    private void ShowPage(string page)
    {
        HomePage.IsVisible = page == "home";
        NodesPage.IsVisible = page == "nodes";
        SubscriptionsPage.IsVisible = page == "subscriptions";
        SettingsPage.IsVisible = page == "settings";

        (PageTitle.Text, PageSubtitle.Text) = page switch
        {
            "nodes" => ("节点", "来自 core 的真实数据，只读"),
            "subscriptions" => ("订阅", "P1 接入；P0 不放假开关"),
            "settings" => ("设置", "core 连接信息"),
            _ => ("首页", "core 通道与节点页"),
        };

        foreach (var button in new[] { NavHome, NavNodes, NavSubscriptions, NavSettings })
        {
            button.Classes.Remove("active");
        }
        var active = page switch
        {
            "nodes" => NavNodes,
            "subscriptions" => NavSubscriptions,
            "settings" => NavSettings,
            _ => NavHome,
        };
        active.Classes.Add("active");
    }

    // ---- core calls -------------------------------------------------------

    private async Task RefreshAsync()
    {
        _client?.Dispose();
        _client = null;

        var endpoint = CoreEndpoint.TryRead();
        if (endpoint is null)
        {
            ShowCoreDown("core 未运行 — 先启动 x-matrix.exe --serve，再点刷新。");
            return;
        }

        _client = new CoreClient(endpoint);
        try
        {
            var nodes = await _client.GetNodesAsync();
            _nodes.Clear();
            foreach (var node in nodes)
            {
                _nodes.Add(node);
            }

            var status = await _client.GetStatusAsync();
            _status = status;
            RenderConnected(endpoint, status);
        }
        catch (Exception error)
        {
            // A core that is up but unresponsive is a different sentence from
            // one that never started; both are shown verbatim.
            ShowCoreDown($"连上了 {endpoint.Port} 但请求失败：{error.Message}" + (error.InnerException is null ? "" : $"（内层：{error.InnerException.Message}）"));
        }
    }

    private void RenderConnected(CoreEndpoint endpoint, CoreStatus status)
    {
        StatusPill.Background = AccentSoft;
        StatusPillText.Foreground = Accent;
        StatusPillText.Text = "core 已连接";

        StatusDot.Fill = status.Running ? Success : Tertiary;
        StatusText.Text = $"内核：{status.EngineText} · 系统代理：{status.ProxyText}"
                          + (status.Running && status.MixedPort > 0 ? $" · 混合端口 {status.MixedPort}" : "");
        EndpointText.Text = $"127.0.0.1:{endpoint.Port} · pid {endpoint.Pid}";

        HomeNodeCount.Text = _nodes.Count.ToString();
        HomeEngineText.Text = status.EngineText;
        SettingsEndpoint.Text = $"{CoreEndpoint.DefaultPath}（port {endpoint.Port}）";
        NodesHint.Text = status.Running
            ? $"正在运行：{status.KindLabel} · {status.NodeName}"
            : $"{_nodes.Count} 个节点";
        EmptyNodes.IsVisible = _nodes.Count == 0;

        StartButton.IsEnabled = !_busy && !status.Running;
        StopButton.IsEnabled = !_busy && status.Running;
        RefreshButton.IsEnabled = !_busy;
    }

    private void ShowCoreDown(string message)
    {
        _status = null;
        // Stale rows would look like live data; an empty list plus "未连接" is
        // the honest state.
        _nodes.Clear();
        StatusPill.Background = DangerSoft;
        StatusPillText.Foreground = Danger;
        StatusPillText.Text = "未连接";
        StatusDot.Fill = Danger;
        StatusText.Text = message;
        EndpointText.Text = CoreEndpoint.TryRead() is { } endpoint
            ? $"127.0.0.1:{endpoint.Port} · pid {endpoint.Pid}"
            : "core.endpoint 未找到";
        SettingsEndpoint.Text = CoreEndpoint.DefaultPath;
        HomeNodeCount.Text = "—";
        HomeEngineText.Text = "未连接";
        NodesHint.Text = string.Empty;
        EmptyNodes.IsVisible = _nodes.Count == 0;
        StartButton.IsEnabled = false;
        StopButton.IsEnabled = false;
        RefreshButton.IsEnabled = true;
    }

    private async void OnRefresh(object? sender, RoutedEventArgs e)
    {
        await RefreshAsync();
    }

    private async void OnStart(object? sender, RoutedEventArgs e)
    {
        if (_client is null)
        {
            return;
        }
        var node = NodeList.SelectedItem as NodeItem ?? _nodes.FirstOrDefault();
        if (node is null)
        {
            StatusText.Text = "没有可选节点：先在 store.json 里放节点。";
            return;
        }

        await RunBusyAsync($"正在启动 {node.Name}…", async () =>
            await _client.StartAsync(node.Id, "xray"));
    }

    private async void OnStop(object? sender, RoutedEventArgs e)
    {
        if (_client is null)
        {
            return;
        }
        await RunBusyAsync("正在停止内核…", async () => await _client.StopAsync());
    }

    /// <summary>
    /// One guarded call: buttons off, one status line, then re-read `/status`
    /// so the window shows what the core did, not what the click asked for.
    /// </summary>
    private async Task RunBusyAsync(string pending, Func<Task> action)
    {
        if (_busy)
        {
            return;
        }
        _busy = true;
        StartButton.IsEnabled = false;
        StopButton.IsEnabled = false;
        RefreshButton.IsEnabled = false;
        StatusText.Text = pending;
        try
        {
            await action();
        }
        catch (Exception error)
        {
            Log($"操作失败：{error.GetType().Name}: {error.Message}");
            _busy = false;
            // Refresh first, then speak: /status re-renders the status line, so an
            // error written before it was erased by the next tick (measured).
            await RefreshAsync();
            StatusText.Text = $"操作失败：{error.Message}";
            return;
        }
        finally
        {
            _busy = false;
        }
        await RefreshAsync();
    }
}
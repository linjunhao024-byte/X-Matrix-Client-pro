using Avalonia;

namespace XMatrix.Desktop;

internal static class Program
{
    // Avalonia requires an STA thread before any control is created.
    [STAThread]
    public static void Main(string[] args) =>
        BuildAvaloniaApp().StartWithClassicDesktopLifetime(args);

    public static AppBuilder BuildAvaloniaApp() =>
        AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .LogToTrace();
}
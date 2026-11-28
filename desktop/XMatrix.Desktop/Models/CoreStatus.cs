using System.Text.Json;

namespace XMatrix.Desktop.Models;

/// <summary>
/// The `/status` payload: the Rust `EngineStatus` (snake_case, flattened) plus
/// the proxy block. Parsed by hand instead of by attribute so a field added on
/// the core side cannot make the whole window fail to deserialize.
/// </summary>
public sealed record CoreStatus(
    bool Running,
    string? Kind,
    string? NodeName,
    int Pid,
    int MixedPort,
    ProxyStatus Proxy)
{
    public string EngineText => Running
        ? $"运行中 · {KindLabel}" + (string.IsNullOrEmpty(NodeName) ? "" : $" · {NodeName}")
        : "未运行";

    public string KindLabel => Kind switch
    {
        "xray" => "Xray",
        "sing-box" => "sing-box",
        null or "" => "—",
        _ => Kind,
    };

    public string ProxyText => Proxy.Active
        ? string.IsNullOrEmpty(Proxy.Server) ? "开" : $"开 · {Proxy.Server}"
        : "关";

    public static CoreStatus FromJson(JsonElement root)
    {
        bool running = root.TryGetProperty("running", out var runningValue)
                       && runningValue.ValueKind == JsonValueKind.True;
        string? kind = root.TryGetProperty("kind", out var kindValue) && kindValue.ValueKind == JsonValueKind.String
            ? kindValue.GetString()
            : null;
        string? nodeName = root.TryGetProperty("node_name", out var nodeValue) && nodeValue.ValueKind == JsonValueKind.String
            ? nodeValue.GetString()
            : null;
        int pid = root.TryGetProperty("pid", out var pidValue) && pidValue.TryGetInt32(out var parsedPid)
            ? parsedPid
            : 0;
        int mixedPort = root.TryGetProperty("mixed_port", out var portValue) && portValue.TryGetInt32(out var parsedPort)
            ? parsedPort
            : 0;

        var proxy = new ProxyStatus(false, string.Empty);
        if (root.TryGetProperty("proxy", out var proxyValue) && proxyValue.ValueKind == JsonValueKind.Object)
        {
            bool active = proxyValue.TryGetProperty("active", out var activeValue)
                          && activeValue.ValueKind == JsonValueKind.True;
            string server = proxyValue.TryGetProperty("server", out var serverValue)
                            && serverValue.ValueKind == JsonValueKind.String
                ? serverValue.GetString() ?? string.Empty
                : string.Empty;
            proxy = new ProxyStatus(active, server);
        }

        return new CoreStatus(running, kind, nodeName, pid, mixedPort, proxy);
    }
}

public sealed record ProxyStatus(bool Active, string Server);
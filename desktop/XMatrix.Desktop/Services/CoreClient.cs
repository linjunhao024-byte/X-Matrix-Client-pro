using System.Net.Http;
using System.Net.Http.Json;
using System.Text;
using System.Text.Json;
using XMatrix.Desktop.Models;

namespace XMatrix.Desktop.Services;

/// <summary>What a running core publishes in `%APPDATA%\com.xmatrix.client\core.endpoint`.</summary>
public sealed record CoreEndpoint(int Port, string Token, int Pid)
{
    public static string DefaultPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
        "com.xmatrix.client",
        "core.endpoint");

    /// <summary>
    /// Read the endpoint file, or return null when no core is running. A file
    /// that exists but cannot be parsed is also "no usable core": pretending we
    /// read it would send the token to the wrong port.
    /// </summary>
    public static CoreEndpoint? TryRead(string? path = null)
    {
        path ??= DefaultPath;
        if (!File.Exists(path))
        {
            return null;
        }
        try
        {
            var text = File.ReadAllText(path, Encoding.UTF8);
            var endpoint = JsonSerializer.Deserialize<CoreEndpoint>(
                text,
                new JsonSerializerOptions { PropertyNameCaseInsensitive = true });
            return endpoint is { Port: > 0 } ? endpoint : null;
        }
        catch (IOException)
        {
            return null;
        }
        catch (JsonException)
        {
            return null;
        }
    }
}

/// <summary>The core answered with its `{"error":{"code","message"}}` envelope.</summary>
public sealed class CoreException(string code, string message) : Exception(message)
{
    public string Code { get; } = code;
}

/// <summary>
/// The five routes from the P0 task, typed for the window. One instance per
/// core endpoint; disposing it releases the loopback connection pool.
/// </summary>
public sealed class CoreClient : IDisposable
{
    private readonly HttpClient _http;

    public CoreClient(CoreEndpoint endpoint)
    {
        Endpoint = endpoint;
        // A loopback API must never go through a proxy: on a freshly installed
        // machine the WinINET setting or a stray *_PROXY variable is enough to
        // turn every core call into "an error occurred while sending the
        // request".
        var handler = new HttpClientHandler { UseProxy = false };
        _http = new HttpClient(handler)
        {
            BaseAddress = new Uri($"http://127.0.0.1:{endpoint.Port}/"),
            Timeout = TimeSpan.FromSeconds(5),
        };
        _http.DefaultRequestHeaders.Add("X-Core-Token", endpoint.Token);
        // The core answers one request per connection and closes; telling
        // HttpClient that up front stops it from reusing the socket for the
        // very next call, which surfaced as "the connection was aborted".
        _http.DefaultRequestHeaders.ConnectionClose = true;
    }

    public CoreEndpoint Endpoint { get; }

    public async Task<List<NodeItem>> GetNodesAsync(CancellationToken cancellationToken = default)
    {
        using var response = await _http.GetAsync("nodes", cancellationToken);
        await EnsureSuccessAsync(response, cancellationToken);
        var nodes = await response.Content.ReadFromJsonAsync<List<NodeItem>>(
            new JsonSerializerOptions { PropertyNameCaseInsensitive = true },
            cancellationToken);
        return nodes ?? [];
    }

    public async Task<CoreStatus> GetStatusAsync(CancellationToken cancellationToken = default)
    {
        using var response = await _http.GetAsync("status", cancellationToken);
        await EnsureSuccessAsync(response, cancellationToken);
        await using var stream = await response.Content.ReadAsStreamAsync(cancellationToken);
        using var document = await JsonDocument.ParseAsync(stream, cancellationToken: cancellationToken);
        return CoreStatus.FromJson(document.RootElement);
    }

    public async Task StartAsync(string nodeId, string kind, CancellationToken cancellationToken = default)
    {
        // PostAsJsonAsync leaves the length unknown, and HttpClient then sends
        // the body chunked; the core reads `Content-Length` and saw an empty
        // body ("请求体不是合法 JSON"). Serialising here hands it a length.
        var payload = JsonSerializer.Serialize(new { kind, nodeId });
        using var body = new StringContent(payload, Encoding.UTF8, "application/json");
        using var response = await _http.PostAsync("engine/start", body, cancellationToken);
        await EnsureSuccessAsync(response, cancellationToken);
    }

    public async Task StopAsync(CancellationToken cancellationToken = default)
    {
        using var response = await _http.PostAsync(
            "engine/stop",
            new ByteArrayContent([]),
            cancellationToken);
        await EnsureSuccessAsync(response, cancellationToken);
    }

    public async Task ShutdownAsync(CancellationToken cancellationToken = default)
    {
        using var response = await _http.PostAsync(
            "shutdown",
            new ByteArrayContent([]),
            cancellationToken);
        await EnsureSuccessAsync(response, cancellationToken);
    }

    /// <summary>
    /// Turn a non-2xx response into the core's own message. The window shows
    /// that text verbatim: it is already the Chinese sentence the Rust side
    /// wrote, and a second wording here would be a second truth.
    /// </summary>
    private static async Task EnsureSuccessAsync(
        HttpResponseMessage response,
        CancellationToken cancellationToken)
    {
        if (response.IsSuccessStatusCode)
        {
            return;
        }
        string code = $"http_{(int)response.StatusCode}";
        string message = $"core 返回 HTTP {(int)response.StatusCode}";
        try
        {
            var text = await response.Content.ReadAsStringAsync(cancellationToken);
            using var document = JsonDocument.Parse(text);
            if (document.RootElement.TryGetProperty("error", out var error)
                && error.ValueKind == JsonValueKind.Object)
            {
                if (error.TryGetProperty("code", out var codeValue)
                    && codeValue.ValueKind == JsonValueKind.String)
                {
                    code = codeValue.GetString() ?? code;
                }
                if (error.TryGetProperty("message", out var messageValue)
                    && messageValue.ValueKind == JsonValueKind.String)
                {
                    message = messageValue.GetString() ?? message;
                }
            }
        }
        catch (JsonException)
        {
            // A non-JSON body is a broken core; the HTTP status stays useful.
        }
        throw new CoreException(code, message);
    }

    public void Dispose() => _http.Dispose();
}
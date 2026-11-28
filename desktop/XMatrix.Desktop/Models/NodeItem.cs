namespace XMatrix.Desktop.Models;

/// <summary>
/// One node exactly as `GET /nodes` returns it: no uuid, no password. The
/// display strings are computed here so the XAML stays free of formatting.
/// </summary>
public sealed record NodeItem(string Id, string Name, string Protocol, string Host, int Port)
{
    /// <summary>"vless" → "VLESS": the protocol labels the rest of the app uses.</summary>
    public string ProtocolLabel => Protocol.ToUpperInvariant();

    public string Address => $"{Host}:{Port}";
}
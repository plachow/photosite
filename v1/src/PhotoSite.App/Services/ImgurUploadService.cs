using System.Net;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text.Json;
using System.Windows.Media.Imaging;

namespace PhotoSite.Services;

public sealed class ImgurUploadService
{
    private static readonly Uri UploadEndpoint =
        new("https://api.imgur.com/3/image");

    private readonly HttpClient httpClient;

    public ImgurUploadService(HttpClient httpClient)
    {
        this.httpClient = httpClient;
    }

    public async Task<string> UploadAsync(
        BitmapSource image,
        string clientId,
        string? title = null,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(image);
        if (string.IsNullOrWhiteSpace(clientId))
        {
            throw new ArgumentException(
                "An Imgur Client ID is required.",
                nameof(clientId));
        }

        var encoded = await Task.Run(
            () => EncodePng(image),
            cancellationToken);
        using var request = new HttpRequestMessage(
            HttpMethod.Post,
            UploadEndpoint);
        request.Headers.Authorization = new AuthenticationHeaderValue(
            "Client-ID",
            clientId.Trim());
        using var form = new MultipartFormDataContent();
        var imageContent = new ByteArrayContent(encoded);
        imageContent.Headers.ContentType = new MediaTypeHeaderValue("image/png");
        form.Add(imageContent, "image", "PhotoSite.png");
        if (!string.IsNullOrWhiteSpace(title))
        {
            form.Add(new StringContent(title), "title");
        }

        request.Content = form;
        using var response = await httpClient.SendAsync(
            request,
            HttpCompletionOption.ResponseHeadersRead,
            cancellationToken);
        var responseJson = await response.Content.ReadAsStringAsync(
            cancellationToken);
        if (!response.IsSuccessStatusCode)
        {
            throw new ImgurUploadException(
                response.StatusCode,
                $"Imgur rejected the upload ({(int)response.StatusCode}): "
                + ReadError(responseJson));
        }

        return ReadDirectLink(responseJson);
    }

    internal static string ReadDirectLink(string responseJson)
    {
        using var document = JsonDocument.Parse(responseJson);
        if (!document.RootElement.TryGetProperty("data", out var data)
            || !data.TryGetProperty("link", out var linkProperty)
            || linkProperty.GetString() is not { } link
            || !Uri.TryCreate(link, UriKind.Absolute, out var uri))
        {
            throw new InvalidOperationException(
                "Imgur did not return a direct image URL.");
        }

        if (uri.Scheme == Uri.UriSchemeHttp)
        {
            var secure = new UriBuilder(uri)
            {
                Scheme = Uri.UriSchemeHttps,
                Port = -1
            };
            uri = secure.Uri;
        }

        if (uri.Scheme != Uri.UriSchemeHttps
            || !uri.Host.Equals(
                "i.imgur.com",
                StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException(
                "Imgur returned an unsupported direct image URL.");
        }

        return uri.AbsoluteUri;
    }

    private static byte[] EncodePng(BitmapSource image)
    {
        var encoder = new PngBitmapEncoder();
        encoder.Frames.Add(BitmapFrame.Create(image));
        using var stream = new MemoryStream();
        encoder.Save(stream);
        return stream.ToArray();
    }

    private static string ReadError(string responseJson)
    {
        try
        {
            using var document = JsonDocument.Parse(responseJson);
            if (!document.RootElement.TryGetProperty("data", out var data))
            {
                return "unknown API error";
            }

            if (data.ValueKind == JsonValueKind.String)
            {
                return data.GetString() ?? "unknown API error";
            }

            if (data.TryGetProperty("error", out var error))
            {
                return error.ValueKind == JsonValueKind.String
                    ? error.GetString() ?? "unknown API error"
                    : error.GetRawText();
            }
        }
        catch (JsonException)
        {
        }

        return "unknown API error";
    }
}

public sealed class ImgurUploadException : Exception
{
    public ImgurUploadException(
        HttpStatusCode statusCode,
        string message)
        : base(message)
    {
        StatusCode = statusCode;
    }

    public HttpStatusCode StatusCode { get; }
}

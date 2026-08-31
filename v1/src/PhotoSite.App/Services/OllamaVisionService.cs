using System.Globalization;
using System.Net;
using System.Net.Http;
using System.Text;
using System.Text.Json;
using PhotoSite.Domain;

namespace PhotoSite.Services;

/// <summary>The metadata one vision-model call extracted from a photograph.</summary>
public sealed record AiPhotoInsights(
    string? Title,
    string? Description,
    IReadOnlyList<string> Keywords,
    string? DescriptionEn = null);

/// <summary>How AI results treat metadata that is already on a photograph.</summary>
public enum AiApplyMode
{
    /// <summary>Only fill title and description that are currently empty.</summary>
    FillEmpty = 0,

    /// <summary>Replace title and description with the AI results.</summary>
    Overwrite = 1
}

/// <summary>
/// Talks to a locally running Ollama server and asks a vision model to
/// describe one photograph as structured JSON: a title, a thorough
/// description, and a keyword list.
/// </summary>
public sealed class OllamaVisionService
{
    public const string DefaultEndpoint = "http://localhost:11434";
    public const string DefaultModel = "qwen3.8:27b";

    private static readonly JsonSerializerOptions SerializerOptions = new();

    private readonly HttpClient httpClient;

    public OllamaVisionService(HttpClient httpClient)
    {
        this.httpClient = httpClient;
    }

    /// <summary>
    /// Names of the models the server offers, vision-capable ones first.
    /// Models whose capability list is unknown are kept, so an older server
    /// that does not report capabilities still shows its models.
    /// </summary>
    public async Task<IReadOnlyList<string>> ListModelsAsync(
        string endpoint,
        CancellationToken cancellationToken)
    {
        using var response = await httpClient.GetAsync(
            BuildUri(endpoint, "api/tags"),
            cancellationToken);
        var json = await response.Content.ReadAsStringAsync(cancellationToken);
        if (!response.IsSuccessStatusCode)
        {
            throw new OllamaVisionException(DescribeServerError(response.StatusCode, json));
        }

        return ParseModelNames(json);
    }

    public async Task<AiPhotoInsights> DescribeAsync(
        string endpoint,
        string model,
        byte[] jpegImage,
        string language,
        bool includeEnglishDescription,
        GeoPlace? place,
        bool approximateLocation,
        CancellationToken cancellationToken)
    {
        var placeContext = BuildPlaceContext(place, approximateLocation);
        var uri = BuildUri(endpoint, "api/chat");
        var response = await PostAsync(
            uri,
            BuildRequestJson(
                model,
                jpegImage,
                language,
                includeEnglishDescription,
                disableThinking: true,
                placeContext),
            cancellationToken);
        if (response.StatusCode == HttpStatusCode.BadRequest)
        {
            // An older server, or a model without a thinking mode, can reject
            // the "think" switch; the request works without it, just slower.
            response.Response.Dispose();
            response = await PostAsync(
                uri,
                BuildRequestJson(
                    model,
                    jpegImage,
                    language,
                    includeEnglishDescription,
                    disableThinking: false,
                    placeContext),
                cancellationToken);
        }

        using var _ = response.Response;
        if (!response.Response.IsSuccessStatusCode)
        {
            throw new OllamaVisionException(
                DescribeServerError(response.StatusCode, response.Body));
        }

        return AugmentKeywords(
            ParseInsights(response.Body),
            place,
            approximateLocation);
    }

    /// <summary>
    /// One sentence of verified place context for the prompt, or null when
    /// there is nothing trustworthy to say. A nearby place reads "in or
    /// near"; a distant one only as a reference point ("about 39 km east
    /// of ..."), because claiming the photo was taken in a town two valleys
    /// away would put a wrong name into the caption. An approximate GPS fix
    /// softens the wording instead of pretending precision.
    /// </summary>
    internal static string? BuildPlaceContext(
        GeoPlace? place,
        bool approximateLocation)
    {
        if (place?.City is not { Length: > 0 } city)
        {
            return null;
        }

        var parents = new List<string>(3);
        foreach (var name in new[] { place.Subregion, place.Region, place.Country })
        {
            if (name is { Length: > 0 }
                && !string.Equals(name, city, StringComparison.OrdinalIgnoreCase)
                && !parents.Contains(name, StringComparer.OrdinalIgnoreCase))
            {
                parents.Add(name);
            }
        }

        var suffix = parents.Count == 0
            ? string.Empty
            : $" ({string.Join(", ", parents)})";
        var location = place.DistanceKm <= GeoPlace.NearbyKm
            ? $"in or near {city}{suffix}"
            : string.Create(
                CultureInfo.InvariantCulture,
                $"about {place.DistanceKm:0} km "
                + $"{ExifToolGeolocator.CompassFromPlace(place.BearingDegrees)} "
                + $"of {city}{suffix}");
        return approximateLocation
            ? $"probably {location}; the GPS fix was imprecise, so treat "
              + "the place as approximate"
            : location;
    }

    /// <summary>
    /// Puts the resolved place names into the keyword list ahead of the
    /// model's own, so a search for the region or country always finds the
    /// photo no matter how the model chose to phrase things.
    /// </summary>
    internal static AiPhotoInsights AugmentKeywords(
        AiPhotoInsights insights,
        GeoPlace? place,
        bool approximateLocation) =>
        place is null
            ? insights
            : insights with
            {
                Keywords = CleanKeywords(
                    ExifToolGeolocator.PlaceKeywords(
                        place,
                        includeCity: !approximateLocation)
                        .Concat(insights.Keywords))
            };

    private async Task<(HttpResponseMessage Response, HttpStatusCode StatusCode, string Body)>
        PostAsync(
            Uri uri,
            string requestJson,
            CancellationToken cancellationToken)
    {
        var content = new StringContent(requestJson, Encoding.UTF8, "application/json");
        var response = await httpClient.PostAsync(uri, content, cancellationToken);
        var body = await response.Content.ReadAsStringAsync(cancellationToken);
        return (response, response.StatusCode, body);
    }

    internal static Uri BuildUri(string endpoint, string relativePath)
    {
        var trimmed = endpoint.Trim().TrimEnd('/');
        if (trimmed.Length == 0)
        {
            trimmed = DefaultEndpoint;
        }

        if (!trimmed.Contains("://", StringComparison.Ordinal))
        {
            trimmed = "http://" + trimmed;
        }

        if (!Uri.TryCreate($"{trimmed}/{relativePath}", UriKind.Absolute, out var uri))
        {
            throw new OllamaVisionException(
                $"“{endpoint}” is not a valid Ollama server address.");
        }

        return uri;
    }

    internal static string BuildRequestJson(
        string model,
        byte[] jpegImage,
        string language,
        bool includeEnglishDescription,
        bool disableThinking,
        string? placeContext = null)
    {
        var properties = new Dictionary<string, object?>
        {
            ["title"] = new Dictionary<string, object?> { ["type"] = "string" },
            ["description"] = new Dictionary<string, object?> { ["type"] = "string" },
            ["keywords"] = new Dictionary<string, object?>
            {
                ["type"] = "array",
                ["items"] = new Dictionary<string, object?> { ["type"] = "string" }
            }
        };
        var required = new List<string> { "title", "description", "keywords" };
        if (includeEnglishDescription)
        {
            properties["description_en"] =
                new Dictionary<string, object?> { ["type"] = "string" };
            required.Add("description_en");
        }

        var request = new Dictionary<string, object?>
        {
            ["model"] = model,
            ["stream"] = false,
            ["messages"] = new[]
            {
                new Dictionary<string, object?>
                {
                    ["role"] = "user",
                    ["content"] = BuildPrompt(
                        language,
                        includeEnglishDescription,
                        placeContext),
                    ["images"] = new[] { Convert.ToBase64String(jpegImage) }
                }
            },
            // Constraining the reply to this schema means the answer is always
            // machine-readable JSON, never prose that needs to be picked apart.
            ["format"] = new Dictionary<string, object?>
            {
                ["type"] = "object",
                ["properties"] = properties,
                ["required"] = required
            },
            ["options"] = new Dictionary<string, object?>
            {
                ["temperature"] = 0.2
            }
        };
        if (disableThinking)
        {
            request["think"] = false;
        }

        return JsonSerializer.Serialize(request, SerializerOptions);
    }

    internal static string BuildPrompt(
        string language,
        bool includeEnglishDescription,
        string? placeContext = null) =>
        "You are an expert photo librarian. Analyze this photograph and "
        + "extract as much information as you can.\n"
        + (placeContext is null
            ? string.Empty
            : $"Verified place: the photograph was taken {placeContext}. "
              + "Work this place into the description and the keywords, and "
              + "into the title when it fits naturally.\n")
        + "Return:\n"
        + "- \"title\": a short factual title, at most 8 words.\n"
        + "- \"description\": 2 to 5 sentences covering the main subject, the "
        + "setting and type of location, actions, the number of people, "
        + "notable objects, animals, plants, weather, light, season if "
        + "apparent, dominant colours, mood and composition. Quote any "
        + "readable text, signs or inscriptions exactly.\n"
        + "- \"keywords\": 10 to 25 keywords: subjects, objects, animals, "
        + "plants, type of location, activities, events, season, time of day, "
        + "weather, dominant colours, mood, photographic style. Use single "
        + "words or short phrases.\n"
        + (includeEnglishDescription
            ? "- \"description_en\": the same description written in English.\n"
            : string.Empty)
        + $"Write the title, the description and the keywords in {language}. "
        + (placeContext is null
            ? "Do not guess names of people or exact places unless visible "
              + "text makes them certain."
            : "Do not guess names of people. Beyond the verified place, name "
              + "a more specific spot or landmark only if you clearly "
              + "recognize it in the photograph or readable text makes it "
              + "certain.");

    /// <summary>Reads the schema-constrained JSON out of a chat response.</summary>
    internal static AiPhotoInsights ParseInsights(string chatResponseJson)
    {
        string? content;
        try
        {
            using var document = JsonDocument.Parse(chatResponseJson);
            content = document.RootElement.TryGetProperty("message", out var message)
                      && message.TryGetProperty("content", out var contentProperty)
                ? contentProperty.GetString()
                : null;
        }
        catch (JsonException)
        {
            throw new OllamaVisionException("The Ollama reply was not valid JSON.");
        }

        if (string.IsNullOrWhiteSpace(content))
        {
            throw new OllamaVisionException("The Ollama reply carried no message.");
        }

        try
        {
            using var document = JsonDocument.Parse(content);
            var root = document.RootElement;
            return new AiPhotoInsights(
                CleanLine(ReadString(root, "title"), maxLength: 200),
                CleanText(ReadString(root, "description"), maxLength: 4000),
                CleanKeywords(ReadStrings(root, "keywords")),
                CleanText(ReadString(root, "description_en"), maxLength: 4000));
        }
        catch (JsonException)
        {
            throw new OllamaVisionException(
                "The model answer was not the requested JSON.");
        }
    }

    internal static IReadOnlyList<string> ParseModelNames(string tagsResponseJson)
    {
        try
        {
            using var document = JsonDocument.Parse(tagsResponseJson);
            if (!document.RootElement.TryGetProperty("models", out var models)
                || models.ValueKind != JsonValueKind.Array)
            {
                return [];
            }

            var vision = new List<string>();
            var unknown = new List<string>();
            foreach (var model in models.EnumerateArray())
            {
                if (ReadString(model, "name") is not { Length: > 0 } name)
                {
                    continue;
                }

                if (!model.TryGetProperty("capabilities", out var capabilities)
                    || capabilities.ValueKind != JsonValueKind.Array)
                {
                    unknown.Add(name);
                    continue;
                }

                if (capabilities.EnumerateArray().Any(capability =>
                        capability.ValueKind == JsonValueKind.String
                        && string.Equals(
                            capability.GetString(),
                            "vision",
                            StringComparison.OrdinalIgnoreCase)))
                {
                    vision.Add(name);
                }
            }

            return [.. vision, .. unknown];
        }
        catch (JsonException)
        {
            return [];
        }
    }

    /// <summary>
    /// Keywords already on the photograph stay; the AI ones are appended,
    /// deduplicated without regard to case.
    /// </summary>
    internal static string? MergeKeywords(
        string? existing,
        IReadOnlyList<string> added)
    {
        var current = string.IsNullOrWhiteSpace(existing)
            ? []
            : existing.Split(
                ';',
                StringSplitOptions.RemoveEmptyEntries
                | StringSplitOptions.TrimEntries);
        var merged = PhotoRecord.JoinKeywords(current.Concat(added));
        return merged.Length == 0 ? null : merged;
    }

    /// <summary>
    /// In fill-empty mode a photograph that already carries both a title and
    /// a description is done; skipping it is what lets an interrupted bulk
    /// run be restarted over the same selection.
    /// </summary>
    internal static bool ShouldSkip(
        AiApplyMode mode,
        string? title,
        string? description) =>
        mode == AiApplyMode.FillEmpty
        && !string.IsNullOrWhiteSpace(title)
        && !string.IsNullOrWhiteSpace(description);

    private static string? ReadString(JsonElement root, string name) =>
        root.TryGetProperty(name, out var value)
        && value.ValueKind == JsonValueKind.String
            ? value.GetString()
            : null;

    private static IEnumerable<string> ReadStrings(JsonElement root, string name)
    {
        if (!root.TryGetProperty(name, out var value)
            || value.ValueKind != JsonValueKind.Array)
        {
            yield break;
        }

        foreach (var item in value.EnumerateArray())
        {
            if (item.ValueKind == JsonValueKind.String
                && item.GetString() is { Length: > 0 } text)
            {
                yield return text;
            }
        }
    }

    private static string? CleanLine(string? text, int maxLength)
    {
        var cleaned = CollapseWhitespace(text);
        return cleaned is null
            ? null
            : cleaned.Length <= maxLength
                ? cleaned
                : cleaned[..maxLength].TrimEnd();
    }

    private static string? CleanText(string? text, int maxLength)
    {
        if (string.IsNullOrWhiteSpace(text))
        {
            return null;
        }

        var cleaned = text.Replace("\r\n", "\n").Trim();
        return cleaned.Length <= maxLength
            ? cleaned
            : cleaned[..maxLength].TrimEnd();
    }

    internal static IReadOnlyList<string> CleanKeywords(
        IEnumerable<string> keywords) =>
        keywords
            .Select(CleanKeyword)
            .OfType<string>()
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .Take(30)
            .ToArray();

    /// <summary>
    /// Keywords travel as a semicolon-separated list, so the separators and
    /// hashtag prefixes a model may emit must not survive into a keyword.
    /// </summary>
    private static string? CleanKeyword(string keyword) =>
        CollapseWhitespace(
            keyword.Replace(';', ' ').Replace(',', ' ').Replace("#", string.Empty));

    private static string? CollapseWhitespace(string? text) =>
        string.IsNullOrWhiteSpace(text)
            ? null
            : string.Join(
                ' ',
                text.Split(
                    (char[]?)null,
                    StringSplitOptions.RemoveEmptyEntries));

    private static string DescribeServerError(HttpStatusCode statusCode, string body)
    {
        try
        {
            using var document = JsonDocument.Parse(body);
            if (document.RootElement.TryGetProperty("error", out var error)
                && error.ValueKind == JsonValueKind.String
                && error.GetString() is { Length: > 0 } message)
            {
                return $"Ollama rejected the request: {message}";
            }
        }
        catch (JsonException)
        {
        }

        return $"Ollama rejected the request ({(int)statusCode}).";
    }
}

public sealed class OllamaVisionException : Exception
{
    public OllamaVisionException(string message)
        : base(message)
    {
    }
}

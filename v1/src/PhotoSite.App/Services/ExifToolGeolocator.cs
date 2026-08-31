using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Text;
using System.Text.Json;

namespace PhotoSite.Services;

/// <summary>
/// The place the offline geolocation database resolved for a coordinate:
/// the nearest populated place (which can be as fine as a neighbourhood),
/// its administrative parents, and how far away it actually is.
/// </summary>
public sealed record GeoPlace(
    string? City,
    string? Subregion,
    string? Region,
    string? Country,
    double DistanceKm,
    int BearingDegrees)
{
    /// <summary>
    /// Within this distance the photo is genuinely "in or near" the place;
    /// beyond it the place is only a reference point on the horizon.
    /// </summary>
    public const double NearbyKm = 3;

    /// <summary>
    /// The nearest populated place stops being a trustworthy keyword when it
    /// is this far away - a wilderness photo is not "in" a town 40 km off.
    /// </summary>
    public const double CityKeywordMaxKm = 10;
}

/// <summary>
/// Reverse-geocodes coordinates entirely on this machine through the
/// geolocation database bundled with exiftool - the same local-only path the
/// metadata writer already uses; nothing leaves the computer.
/// </summary>
/// <remarks>
/// Each lookup is one exiftool run with <c>-api geolocation="lat,lon"</c>,
/// pointed at the exiftool executable itself as a target file that is
/// guaranteed to exist and be readable. Results are cached per ~100 m grid
/// cell, so a folder shot around one spot costs a handful of runs, not one
/// per photograph. Built for the AI describe loop, which is sequential; the
/// cache is not synchronized.
/// </remarks>
public sealed class ExifToolGeolocator
{
    private readonly string exePath;
    private readonly Dictionary<(double, double), GeoPlace?> cache = [];

    public ExifToolGeolocator()
        : this(Path.Combine(
            AppContext.BaseDirectory,
            "tools",
            "exiftool",
            "exiftool.exe"))
    {
    }

    internal ExifToolGeolocator(string exePath)
    {
        this.exePath = exePath;
    }

    public bool IsAvailable => File.Exists(exePath);

    /// <summary>
    /// The nearest catalogued place, or null when exiftool is missing, fails,
    /// or knows nothing for the coordinates. Failures are cached too: a
    /// broken exiftool must not be retried for every photo of a bulk run.
    /// </summary>
    public async Task<GeoPlace?> ResolveAsync(
        double latitude,
        double longitude,
        CancellationToken cancellationToken)
    {
        var key = (Math.Round(latitude, 3), Math.Round(longitude, 3));
        if (cache.TryGetValue(key, out var cached))
        {
            return cached;
        }

        var place = IsAvailable
            ? ParseGeoPlace(await RunAsync(latitude, longitude, cancellationToken))
            : null;
        cache[key] = place;
        return place;
    }

    private async Task<string> RunAsync(
        double latitude,
        double longitude,
        CancellationToken cancellationToken)
    {
        var coordinates = string.Create(
            CultureInfo.InvariantCulture,
            $"{latitude:0.######},{longitude:0.######}");
        try
        {
            var startInfo = new ProcessStartInfo
            {
                FileName = exePath,
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
                StandardOutputEncoding = Encoding.UTF8,
                StandardErrorEncoding = Encoding.UTF8
            };
            startInfo.ArgumentList.Add("-api");
            startInfo.ArgumentList.Add($"geolocation={coordinates}");
            startInfo.ArgumentList.Add("-n");
            startInfo.ArgumentList.Add("-j");
            startInfo.ArgumentList.Add("-Geolocation*");
            // The target file only carries the generated tags; the exe is the
            // one file guaranteed present, ASCII-safe and readable here.
            startInfo.ArgumentList.Add(exePath);

            using var process = Process.Start(startInfo);
            if (process is null)
            {
                return string.Empty;
            }

            var output = await process.StandardOutput
                .ReadToEndAsync(cancellationToken);
            await process.StandardError.ReadToEndAsync(cancellationToken);
            await process.WaitForExitAsync(cancellationToken);
            return process.ExitCode == 0 ? output : string.Empty;
        }
        catch (Exception exception) when (
            exception is IOException
            or UnauthorizedAccessException
            or InvalidOperationException
            or System.ComponentModel.Win32Exception)
        {
            return string.Empty;
        }
    }

    /// <summary>Reads one place out of exiftool's -j -n output.</summary>
    internal static GeoPlace? ParseGeoPlace(string json)
    {
        if (string.IsNullOrWhiteSpace(json))
        {
            return null;
        }

        try
        {
            using var document = JsonDocument.Parse(json);
            if (document.RootElement.ValueKind != JsonValueKind.Array
                || document.RootElement.GetArrayLength() == 0)
            {
                return null;
            }

            var root = document.RootElement[0];
            var city = ReadString(root, "GeolocationCity");
            if (city is null)
            {
                return null;
            }

            return new GeoPlace(
                city,
                ReadString(root, "GeolocationSubregion"),
                ReadString(root, "GeolocationRegion"),
                ReadString(root, "GeolocationCountry"),
                ReadNumber(root, "GeolocationDistance") ?? 0,
                (int)Math.Round(ReadNumber(root, "GeolocationBearing") ?? 0));
        }
        catch (JsonException)
        {
            return null;
        }
    }

    /// <summary>
    /// The searchable place names for a photograph, most specific first, the
    /// way a librarian would tag it: neighbourhood, county, region, country.
    /// The nearest place itself is left out when it is too far away - or the
    /// GPS fix too shaky - to honestly claim the photo was taken there.
    /// </summary>
    internal static IReadOnlyList<string> PlaceKeywords(
        GeoPlace place,
        bool includeCity)
    {
        var keywords = new List<string>(4);
        if (includeCity
            && place.DistanceKm <= GeoPlace.CityKeywordMaxKm
            && place.City is { Length: > 0 } city)
        {
            keywords.Add(city);
        }

        foreach (var name in new[] { place.Subregion, place.Region, place.Country })
        {
            if (name is { Length: > 0 }
                && !keywords.Contains(name, StringComparer.OrdinalIgnoreCase))
            {
                keywords.Add(name);
            }
        }

        return keywords;
    }

    /// <summary>
    /// Where the photo sits relative to the place: the database bearing
    /// points from the photo toward the place, so the photo lies on the
    /// opposite side of it.
    /// </summary>
    internal static string CompassFromPlace(int bearingToPlace)
    {
        string[] directions =
        [
            "north", "north-east", "east", "south-east",
            "south", "south-west", "west", "north-west"
        ];
        var fromPlace = ((bearingToPlace + 180) % 360 + 360) % 360;
        return directions[(int)Math.Round(fromPlace / 45.0) % 8];
    }

    private static string? ReadString(JsonElement root, string name) =>
        root.TryGetProperty(name, out var value)
        && value.ValueKind == JsonValueKind.String
        && value.GetString() is { Length: > 0 } text
            ? text
            : null;

    private static double? ReadNumber(JsonElement root, string name) =>
        root.TryGetProperty(name, out var value)
        && value.ValueKind == JsonValueKind.Number
            ? value.GetDouble()
            : null;
}

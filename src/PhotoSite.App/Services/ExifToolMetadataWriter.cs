using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Text;
using System.Text.Json;

namespace PhotoSite.Services;

internal sealed record MetadataWritePayload(
    int? Rating = null,
    bool TitleChanged = false,
    string? Title = null,
    bool DescriptionChanged = false,
    string? Description = null,
    bool LocationChanged = false,
    double? Latitude = null,
    double? Longitude = null,
    bool LabelChanged = false,
    string? Label = null,
    bool KeywordsChanged = false,
    string? Keywords = null,
    bool RegionsChanged = false,
    string? RegionsJson = null)
{
    public bool IsEmpty =>
        Rating is null
        && !TitleChanged
        && !DescriptionChanged
        && !LocationChanged
        && !LabelChanged
        && !KeywordsChanged
        && !RegionsChanged;
}

internal readonly record struct ExifToolResult(
    bool Success,
    string Error);

internal sealed class ExifToolMetadataWriter
{
    private static readonly HashSet<string> SidecarExtensions =
        new(StringComparer.OrdinalIgnoreCase)
        {
            ".dng", ".cr2", ".cr3", ".nef", ".arw", ".orf", ".rw2",
            ".raf", ".pef", ".heic", ".heif"
        };

    private readonly string exePath;

    public ExifToolMetadataWriter()
        : this(Path.Combine(
            AppContext.BaseDirectory,
            "tools",
            "exiftool",
            "exiftool.exe"))
    {
    }

    internal ExifToolMetadataWriter(string exePath)
    {
        this.exePath = exePath;
    }

    public bool IsAvailable => File.Exists(exePath);

    internal static bool UsesSidecar(string extension) =>
        SidecarExtensions.Contains(extension);

    internal static string GetSidecarPath(string photoPath) =>
        Path.ChangeExtension(photoPath, ".xmp");

    public async Task<ExifToolResult> WriteAsync(
        string photoPath,
        MetadataWritePayload payload,
        CancellationToken cancellationToken)
    {
        if (payload.IsEmpty)
        {
            return new ExifToolResult(true, string.Empty);
        }

        if (!IsAvailable)
        {
            return new ExifToolResult(
                false,
                $"exiftool was not found at {exePath}");
        }

        var sidecar = UsesSidecar(Path.GetExtension(photoPath));
        var targetPath = sidecar ? GetSidecarPath(photoPath) : photoPath;
        var createSidecar = sidecar && !File.Exists(targetPath);
        var arguments = BuildArguments(
            targetPath,
            payload,
            sidecar,
            createSidecar);
        return await RunAsync(arguments, cancellationToken);
    }

    private async Task<ExifToolResult> RunAsync(
        IReadOnlyList<string> arguments,
        CancellationToken cancellationToken)
    {
        var argumentFile = Path.Combine(
            Path.GetTempPath(),
            $"photosite-exiftool-{Guid.NewGuid():N}.args");
        await File.WriteAllLinesAsync(
            argumentFile,
            arguments,
            new UTF8Encoding(encoderShouldEmitUTF8Identifier: false),
            cancellationToken);

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
            startInfo.ArgumentList.Add("-@");
            startInfo.ArgumentList.Add(argumentFile);

            using var process = Process.Start(startInfo);
            if (process is null)
            {
                return new ExifToolResult(
                    false,
                    "The exiftool process could not be started.");
            }

            var standardError = await process.StandardError
                .ReadToEndAsync(cancellationToken);
            await process.StandardOutput.ReadToEndAsync(cancellationToken);
            await process.WaitForExitAsync(cancellationToken);

            return process.ExitCode == 0
                ? new ExifToolResult(true, string.Empty)
                : new ExifToolResult(
                    false,
                    string.IsNullOrWhiteSpace(standardError)
                        ? $"exiftool exited with code {process.ExitCode}"
                        : standardError.Trim());
        }
        finally
        {
            try
            {
                File.Delete(argumentFile);
            }
            catch (IOException)
            {
                // A leaked temp file must not fail the metadata write.
            }
        }
    }

    /// <summary>
    /// Copies metadata from each source onto its converted output in one
    /// exiftool run.
    /// </summary>
    /// <remarks>
    /// Starting a process per photograph would add minutes to a large batch,
    /// so the whole list goes into a single argument file separated by
    /// <c>-execute</c>, which exiftool processes as consecutive commands.
    /// </remarks>
    public async Task<ExifToolResult> CopyMetadataBatchAsync(
        IReadOnlyList<(string Source, string Destination)> pairs,
        bool removeLocation,
        CancellationToken cancellationToken)
    {
        if (pairs.Count == 0)
        {
            return new ExifToolResult(true, string.Empty);
        }

        if (!IsAvailable)
        {
            return new ExifToolResult(
                false,
                $"exiftool was not found at {exePath}");
        }

        var arguments = new List<string>(pairs.Count * 10);
        foreach (var (source, destination) in pairs)
        {
            arguments.Add("-charset");
            arguments.Add("filename=UTF8");
            arguments.Add("-overwrite_original");
            arguments.Add("-m");
            arguments.Add("-q");
            arguments.Add("-TagsFromFile");
            arguments.Add(source);
            arguments.Add("-all:all");
            // The pixels were already rotated on the way out; carrying the
            // source orientation over would rotate the output a second time.
            // The '#' suffix forces the numeric value: without it exiftool
            // matches "1" as an abbreviation of the printable conversions,
            // where it uniquely hits "Rotate 180" and writes 3 instead.
            arguments.Add("-Orientation#=1");
            arguments.Add("-XMP-tiff:Orientation=");
            if (removeLocation)
            {
                arguments.Add("-gps:all=");
                arguments.Add("-xmp:geotag=");
            }

            arguments.Add(destination);
            arguments.Add("-execute");
        }

        return await RunAsync(arguments, cancellationToken);
    }

    internal static IReadOnlyList<string> BuildArguments(
        string targetPath,
        MetadataWritePayload payload,
        bool sidecar,
        bool createSidecar)
    {
        var arguments = new List<string>
        {
            "-charset",
            "filename=UTF8",
            "-charset",
            "iptc=UTF8",
            "-codedcharacterset=utf8",
            "-overwrite_original",
            "-m",
            "-q"
        };

        if (payload.Rating is { } rating)
        {
            if (rating > 0)
            {
                arguments.Add($"-XMP-xmp:Rating={rating}");
                if (!sidecar)
                {
                    arguments.Add($"-IFD0:Rating={rating}");
                    arguments.Add(
                        $"-IFD0:RatingPercent={RatingToPercent(rating)}");
                }
            }
            else
            {
                arguments.Add("-XMP-xmp:Rating=");
                if (!sidecar)
                {
                    arguments.Add("-IFD0:Rating=");
                    arguments.Add("-IFD0:RatingPercent=");
                }
            }
        }

        if (payload.TitleChanged)
        {
            arguments.Add($"-XMP-dc:Title={payload.Title}");
            if (!sidecar)
            {
                arguments.Add($"-IFD0:XPTitle={payload.Title}");
            }
        }

        if (payload.DescriptionChanged)
        {
            arguments.Add($"-XMP-dc:Description={payload.Description}");
            if (!sidecar)
            {
                arguments.Add($"-IFD0:ImageDescription={payload.Description}");
                arguments.Add($"-IFD0:XPComment={payload.Description}");
            }
        }

        if (payload.LabelChanged)
        {
            arguments.Add($"-XMP-xmp:Label={payload.Label}");
        }

        if (payload.KeywordsChanged)
        {
            // Clearing first stops exiftool from appending to the bag that is
            // already in the file, which would make keywords accumulate.
            arguments.Add("-XMP-dc:Subject=");
            if (!sidecar)
            {
                arguments.Add("-IPTC:Keywords=");
            }

            foreach (var keyword in SplitKeywords(payload.Keywords))
            {
                arguments.Add($"-XMP-dc:Subject+={keyword}");
                if (!sidecar)
                {
                    arguments.Add($"-IPTC:Keywords+={keyword}");
                }
            }
        }

        if (payload.RegionsChanged)
        {
            AppendRegionArguments(arguments, payload.RegionsJson);
        }

        if (payload.LocationChanged)
        {
            if (payload is { Latitude: { } latitude, Longitude: { } longitude })
            {
                var lat = latitude.ToString(CultureInfo.InvariantCulture);
                var lon = longitude.ToString(CultureInfo.InvariantCulture);
                arguments.Add($"-XMP:GPSLatitude={lat}");
                arguments.Add($"-XMP:GPSLongitude={lon}");
                if (!sidecar)
                {
                    arguments.Add(
                        $"-GPS:GPSLatitude={Math.Abs(latitude).ToString(CultureInfo.InvariantCulture)}");
                    arguments.Add(
                        $"-GPS:GPSLatitudeRef={(latitude >= 0 ? "N" : "S")}");
                    arguments.Add(
                        $"-GPS:GPSLongitude={Math.Abs(longitude).ToString(CultureInfo.InvariantCulture)}");
                    arguments.Add(
                        $"-GPS:GPSLongitudeRef={(longitude >= 0 ? "E" : "W")}");
                }
            }
            else
            {
                arguments.Add("-XMP:GPSLatitude=");
                arguments.Add("-XMP:GPSLongitude=");
                if (!sidecar)
                {
                    arguments.Add("-GPS:GPSLatitude=");
                    arguments.Add("-GPS:GPSLatitudeRef=");
                    arguments.Add("-GPS:GPSLongitude=");
                    arguments.Add("-GPS:GPSLongitudeRef=");
                }
            }

            // Whichever way the coordinates went, the old fix's error
            // estimate, stamp, source and altitude described the position
            // being replaced; left in place they would keep re-flagging the
            // corrected photo as approximate on the next scan.
            arguments.Add("-XMP:GPSHPositioningError=");
            arguments.Add("-XMP:GPSDateTime=");
            arguments.Add("-XMP:GPSProcessingMethod=");
            arguments.Add("-XMP:GPSAltitude=");
            arguments.Add("-XMP:GPSAltitudeRef=");
            if (!sidecar)
            {
                arguments.Add("-GPS:GPSHPositioningError=");
                arguments.Add("-GPS:GPSDateStamp=");
                arguments.Add("-GPS:GPSTimeStamp=");
                arguments.Add("-GPS:GPSProcessingMethod=");
                arguments.Add("-GPS:GPSAltitude=");
                arguments.Add("-GPS:GPSAltitudeRef=");
            }
        }

        if (createSidecar)
        {
            arguments.Add("-o");
        }

        arguments.Add(targetPath);
        return arguments;
    }

    /// <summary>
    /// Unfolds the queued face rectangles into MWG region tags - the format
    /// Lightroom, digiKam and Windows read face frames from. The whole
    /// struct is cleared first so removed or renamed people never linger,
    /// then rebuilt from flattened list tags, one element per face; MWG
    /// areas are centre-based, the queue stores top-left rectangles.
    /// </summary>
    internal static void AppendRegionArguments(
        List<string> arguments,
        string? regionsJson)
    {
        arguments.Add("-XMP-mwg-rs:RegionInfo=");
        if (string.IsNullOrWhiteSpace(regionsJson))
        {
            return;
        }

        try
        {
            using var document = JsonDocument.Parse(regionsJson);
            var root = document.RootElement;
            if (!root.TryGetProperty("width", out var width)
                || !root.TryGetProperty("height", out var height)
                || width.GetInt32() <= 0
                || height.GetInt32() <= 0
                || !root.TryGetProperty("regions", out var regions)
                || regions.ValueKind != JsonValueKind.Array
                || regions.GetArrayLength() == 0)
            {
                return;
            }

            arguments.Add(
                $"-XMP-mwg-rs:RegionAppliedToDimensionsW={width.GetInt32()}");
            arguments.Add(
                $"-XMP-mwg-rs:RegionAppliedToDimensionsH={height.GetInt32()}");
            arguments.Add("-XMP-mwg-rs:RegionAppliedToDimensionsUnit=pixel");
            foreach (var region in regions.EnumerateArray())
            {
                var name = region.GetProperty("name").GetString();
                if (string.IsNullOrWhiteSpace(name))
                {
                    continue;
                }

                var x = region.GetProperty("x").GetDouble();
                var y = region.GetProperty("y").GetDouble();
                var w = region.GetProperty("w").GetDouble();
                var h = region.GetProperty("h").GetDouble();
                arguments.Add($"-XMP-mwg-rs:RegionName+={name}");
                arguments.Add("-XMP-mwg-rs:RegionType+=Face");
                arguments.Add(FormatRegionValue("RegionAreaX", x + (w / 2)));
                arguments.Add(FormatRegionValue("RegionAreaY", y + (h / 2)));
                arguments.Add(FormatRegionValue("RegionAreaW", w));
                arguments.Add(FormatRegionValue("RegionAreaH", h));
                arguments.Add("-XMP-mwg-rs:RegionAreaUnit+=normalized");
            }
        }
        catch (Exception exception)
            when (exception is JsonException
                or KeyNotFoundException
                or InvalidOperationException
                or FormatException)
        {
            // A malformed queue entry degenerates to clearing the regions;
            // the rest of the metadata write still goes through.
        }
    }

    private static string FormatRegionValue(string tag, double value) =>
        string.Create(
            CultureInfo.InvariantCulture,
            $"-XMP-mwg-rs:{tag}+={Math.Clamp(value, 0, 1):0.######}");

    internal static IReadOnlyList<string> SplitKeywords(string? keywords) =>
        string.IsNullOrWhiteSpace(keywords)
            ? []
            : keywords
                .Split(
                    [';', ','],
                    StringSplitOptions.RemoveEmptyEntries
                    | StringSplitOptions.TrimEntries)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .ToArray();

    internal static int RatingToPercent(int rating) => rating switch
    {
        <= 0 => 0,
        1 => 1,
        2 => 25,
        3 => 50,
        4 => 75,
        _ => 99
    };
}

using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Text;

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
    string? Keywords = null)
{
    public bool IsEmpty =>
        Rating is null
        && !TitleChanged
        && !DescriptionChanged
        && !LocationChanged
        && !LabelChanged
        && !KeywordsChanged;
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
        }

        if (createSidecar)
        {
            arguments.Add("-o");
        }

        arguments.Add(targetPath);
        return arguments;
    }

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

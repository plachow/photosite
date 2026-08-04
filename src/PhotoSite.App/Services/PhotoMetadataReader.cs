using System.Globalization;
using MetadataExtractor;
using MetadataExtractor.Formats.Exif;
using MetadataExtractor.Formats.Xmp;
using PhotoSite.Domain;

namespace PhotoSite.Services;

internal readonly record struct PhotoTakenAt(
    long? Ticks,
    PhotoDateSource Source);

internal readonly record struct PhotoFileMetadata(
    PhotoTakenAt TakenAt,
    int Rating,
    string? Title,
    string? Description,
    double? Latitude,
    double? Longitude);

internal static class PhotoMetadataReader
{
    /// <summary>
    /// Bump whenever the reader learns to extract new fields so that
    /// records indexed by an older reader get re-read once.
    /// </summary>
    internal const int CurrentVersion = 3;

    private static readonly string[] XmpCreateDateKeys =
    [
        "xmp:CreateDate",
        "photoshop:DateCreated",
        "exif:DateTimeOriginal"
    ];

    public static PhotoTakenAt ReadTakenAt(string path)
    {
        try
        {
            return ReadTakenAt(ImageMetadataReader.ReadMetadata(path));
        }
        catch (Exception exception) when (
            exception is ImageProcessingException
            or IOException
            or UnauthorizedAccessException
            or ArgumentException
            or NotSupportedException)
        {
            return new PhotoTakenAt(null, PhotoDateSource.None);
        }
    }

    public static PhotoFileMetadata ReadAll(string path)
    {
        IReadOnlyCollection<MetadataExtractor.Directory> metadata;
        try
        {
            metadata = ImageMetadataReader.ReadMetadata(path)
                as IReadOnlyCollection<MetadataExtractor.Directory>
                ?? [];
        }
        catch (Exception exception) when (
            exception is ImageProcessingException
            or IOException
            or UnauthorizedAccessException
            or ArgumentException
            or NotSupportedException)
        {
            return new PhotoFileMetadata(
                new PhotoTakenAt(null, PhotoDateSource.None),
                0,
                null,
                null,
                null,
                null);
        }

        var fromFile = ReadDescriptive(metadata);
        var sidecar = ReadSidecar(path);
        return new PhotoFileMetadata(
            ReadTakenAt(metadata),
            sidecar?.Rating > 0 ? sidecar.Value.Rating : fromFile.Rating,
            sidecar?.Title ?? fromFile.Title,
            sidecar?.Description ?? fromFile.Description,
            sidecar?.Latitude ?? fromFile.Latitude,
            sidecar?.Longitude ?? fromFile.Longitude);
    }

    private readonly record struct DescriptiveMetadata(
        int Rating,
        string? Title,
        string? Description,
        double? Latitude,
        double? Longitude);

    private static DescriptiveMetadata ReadDescriptive(
        IReadOnlyCollection<MetadataExtractor.Directory> metadata)
    {
        var rating = 0;
        string? title = null;
        string? description = null;
        double? latitude = null;
        double? longitude = null;

        foreach (var directory in metadata.OfType<XmpDirectory>())
        {
            var properties = directory.GetXmpProperties();
            if (rating == 0
                && properties.TryGetValue("xmp:Rating", out var xmpRating)
                && double.TryParse(
                    xmpRating,
                    NumberStyles.Any,
                    CultureInfo.InvariantCulture,
                    out var parsedRating))
            {
                rating = Math.Clamp((int)Math.Round(parsedRating), 0, 5);
            }

            title ??= ReadXmpText(properties, "dc:title");
            description ??= ReadXmpText(properties, "dc:description");
            if (latitude is null
                && properties.TryGetValue("exif:GPSLatitude", out var xmpLat)
                && properties.TryGetValue("exif:GPSLongitude", out var xmpLon)
                && ParseXmpGpsCoordinate(xmpLat) is { } parsedLat
                && ParseXmpGpsCoordinate(xmpLon) is { } parsedLon)
            {
                latitude = parsedLat;
                longitude = parsedLon;
            }
        }

        foreach (var directory in metadata.OfType<ExifIfd0Directory>())
        {
            if (rating == 0
                && directory.TryGetInt32(ExifDirectoryBase.TagRating, out var exifRating))
            {
                rating = Math.Clamp(exifRating, 0, 5);
            }

            title ??= Normalize(
                directory.GetDescription(ExifDirectoryBase.TagWinTitle));
            description ??= Normalize(
                directory.GetDescription(ExifDirectoryBase.TagImageDescription))
                ?? Normalize(
                    directory.GetDescription(ExifDirectoryBase.TagWinComment));
        }

        foreach (var directory in metadata.OfType<GpsDirectory>())
        {
            if (directory.GetGeoLocation() is { IsZero: false } location)
            {
                latitude = location.Latitude;
                longitude = location.Longitude;
                break;
            }
        }

        return new DescriptiveMetadata(
            rating,
            title,
            description,
            latitude,
            longitude);
    }

    private static DescriptiveMetadata? ReadSidecar(string path)
    {
        if (!ExifToolMetadataWriter.UsesSidecar(Path.GetExtension(path)))
        {
            return null;
        }

        var sidecarPath = ExifToolMetadataWriter.GetSidecarPath(path);
        if (!File.Exists(sidecarPath))
        {
            return null;
        }

        try
        {
            var xmpDirectories = new XmpReader()
                .Extract(File.ReadAllBytes(sidecarPath));
            return ReadDescriptive([xmpDirectories]);
        }
        catch (Exception exception) when (
            exception is XmpCore.XmpException
            or ImageProcessingException
            or IOException
            or UnauthorizedAccessException)
        {
            return null;
        }
    }

    private static string? ReadXmpText(
        IDictionary<string, string> properties,
        string key)
    {
        if (properties.TryGetValue($"{key}[1]", out var indexed))
        {
            return Normalize(indexed);
        }

        return properties.TryGetValue(key, out var direct)
            ? Normalize(direct)
            : null;
    }

    private static string? Normalize(string? value) =>
        string.IsNullOrWhiteSpace(value) ? null : value.Trim();

    /// <summary>
    /// Parses the XMP GPS coordinate format, e.g. "49,11.703667N"
    /// (degrees, decimal minutes, hemisphere) or a plain signed decimal.
    /// </summary>
    internal static double? ParseXmpGpsCoordinate(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return null;
        }

        var trimmed = value.Trim();
        var sign = char.ToUpperInvariant(trimmed[^1]) switch
        {
            'S' or 'W' => -1,
            'N' or 'E' => 1,
            _ => 0
        };
        var body = sign == 0 ? trimmed : trimmed[..^1];
        if (sign == 0)
        {
            sign = 1;
        }

        var parts = body.Split(',');
        if (parts.Length == 2
            && double.TryParse(
                parts[0],
                NumberStyles.Float,
                CultureInfo.InvariantCulture,
                out var degrees)
            && double.TryParse(
                parts[1],
                NumberStyles.Float,
                CultureInfo.InvariantCulture,
                out var minutes))
        {
            return sign * (Math.Abs(degrees) + (minutes / 60));
        }

        return double.TryParse(
            body,
            NumberStyles.Float,
            CultureInfo.InvariantCulture,
            out var plain)
            ? sign * plain
            : null;
    }

    internal static PhotoTakenAt ReadTakenAt(
        IEnumerable<MetadataExtractor.Directory> directories)
    {
        var metadata = directories as IReadOnlyCollection<MetadataExtractor.Directory>
                       ?? directories.ToArray();
        foreach (var directory in metadata.OfType<ExifSubIfdDirectory>())
        {
            if (directory.TryGetDateTime(
                    ExifDirectoryBase.TagDateTimeOriginal,
                    out var original))
            {
                return FromDateTime(
                    original,
                    PhotoDateSource.ExifDateTimeOriginal);
            }
        }

        foreach (var directory in metadata.OfType<ExifSubIfdDirectory>())
        {
            if (directory.TryGetDateTime(
                    ExifDirectoryBase.TagDateTimeDigitized,
                    out var digitized))
            {
                return FromDateTime(
                    digitized,
                    PhotoDateSource.ExifDateTimeDigitized);
            }
        }

        foreach (var directory in metadata.OfType<XmpDirectory>())
        {
            var properties = directory.GetXmpProperties();
            foreach (var key in XmpCreateDateKeys)
            {
                if (properties.TryGetValue(key, out var value)
                    && TryParseXmpDate(value, out var xmpDate))
                {
                    return FromDateTime(
                        xmpDate,
                        PhotoDateSource.XmpCreateDate);
                }
            }

            foreach (var property in properties)
            {
                if ((property.Key.EndsWith(
                         ":CreateDate",
                         StringComparison.OrdinalIgnoreCase)
                     || property.Key.EndsWith(
                         ":DateTimeOriginal",
                         StringComparison.OrdinalIgnoreCase))
                    && TryParseXmpDate(property.Value, out var xmpDate))
                {
                    return FromDateTime(
                        xmpDate,
                        PhotoDateSource.XmpCreateDate);
                }
            }
        }

        return new PhotoTakenAt(null, PhotoDateSource.None);
    }

    private static PhotoTakenAt FromDateTime(
        DateTime value,
        PhotoDateSource source) =>
        new(
            DateTime.SpecifyKind(value, DateTimeKind.Unspecified).Ticks,
            source);

    private static bool TryParseXmpDate(string value, out DateTime result)
    {
        if (DateTimeOffset.TryParse(
                value,
                CultureInfo.InvariantCulture,
                DateTimeStyles.AllowWhiteSpaces,
                out var offset))
        {
            result = offset.DateTime;
            return true;
        }

        return DateTime.TryParse(
            value,
            CultureInfo.InvariantCulture,
            DateTimeStyles.AllowWhiteSpaces,
            out result);
    }
}

using System.Globalization;
using MetadataExtractor;
using MetadataExtractor.Formats.Exif;
using MetadataExtractor.Formats.Xmp;
using PhotoSite.Domain;

namespace PhotoSite.Services;

internal readonly record struct PhotoTakenAt(
    long? Ticks,
    PhotoDateSource Source);

internal static class PhotoMetadataReader
{
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

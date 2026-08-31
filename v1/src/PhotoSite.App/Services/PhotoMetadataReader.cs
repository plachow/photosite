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
    double? Longitude,
    ColorLabel ColorLabel = ColorLabel.None,
    string? Keywords = null,
    int? PixelWidth = null,
    int? PixelHeight = null,
    string? Camera = null,
    string? Lens = null,
    double? FocalLength = null,
    double? Aperture = null,
    double? ExposureSeconds = null,
    int? Iso = null,
    double? GpsErrorMeters = null,
    double? GpsFixAgeSeconds = null,
    string? GpsProcessingMethod = null,
    double? GpsAltitude = null);

internal static class PhotoMetadataReader
{
    /// <summary>
    /// Bump whenever the reader learns to extract new fields so that
    /// records indexed by an older reader get re-read once.
    /// </summary>
    internal const int CurrentVersion = 6;

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
        var technical = ReadTechnical(metadata);
        // Coordinates taken over from a sidecar are somebody's deliberate
        // correction, so the file's own fix evidence no longer describes them.
        var gps = sidecar?.Latitude is null
            ? ReadGpsEvidence(metadata)
            : default;
        return new PhotoFileMetadata(
            ReadTakenAt(metadata),
            sidecar?.Rating > 0 ? sidecar.Value.Rating : fromFile.Rating,
            sidecar?.Title ?? fromFile.Title,
            sidecar?.Description ?? fromFile.Description,
            sidecar?.Latitude ?? fromFile.Latitude,
            sidecar?.Longitude ?? fromFile.Longitude,
            sidecar?.ColorLabel is { } sidecarLabel and not ColorLabel.None
                ? sidecarLabel
                : fromFile.ColorLabel,
            sidecar?.Keywords ?? fromFile.Keywords,
            technical.PixelWidth,
            technical.PixelHeight,
            technical.Camera,
            technical.Lens,
            technical.FocalLength,
            technical.Aperture,
            technical.ExposureSeconds,
            technical.Iso,
            gps.ErrorMeters,
            gps.FixAgeSeconds,
            gps.ProcessingMethod,
            gps.Altitude);
    }

    /// <summary>
    /// The file's own testimony about how good its GPS fix was: the
    /// receiver's horizontal error estimate, how stale the fix already was
    /// when the shutter fired - a phone pulled out mid-walk stamps the
    /// location it last knew, hundreds of metres back - and where the
    /// position came from at all (GPSProcessingMethod: satellites, Wi-Fi or
    /// a cell tower), together with the altitude, whose absence gives a pure
    /// tower guess away. All feed
    /// <see cref="PhotoRecord.LocationAccuracy"/>.
    /// </summary>
    internal static (double? ErrorMeters, double? FixAgeSeconds,
        string? ProcessingMethod, double? Altitude) ReadGpsEvidence(
        IReadOnlyCollection<MetadataExtractor.Directory> metadata)
    {
        var gps = metadata.OfType<GpsDirectory>()
            .FirstOrDefault(directory =>
                directory.GetGeoLocation() is { IsZero: false });
        if (gps is null)
        {
            return default;
        }

        var error = TryGetDouble(gps, GpsDirectory.TagHPositioningError);
        var method = Normalize(
            gps.GetDescription(GpsDirectory.TagProcessingMethod));
        var altitude = TryGetDouble(gps, GpsDirectory.TagAltitude);

        double? age = null;
        if (gps.TryGetGpsDate(out var fixUtc))
        {
            foreach (var directory in metadata.OfType<ExifSubIfdDirectory>())
            {
                if (directory.TryGetDateTime(
                        ExifDirectoryBase.TagDateTimeOriginal,
                        out var taken))
                {
                    age = ComputeGpsFixAgeSeconds(
                        taken,
                        fixUtc,
                        ParseUtcOffset(directory.GetString(
                            ExifDirectoryBase.TagTimeZoneOriginal)));
                    break;
                }
            }
        }

        return (error, age, method, altitude);
    }

    /// <summary>
    /// How old the GPS fix already was when the photo was taken. The photo
    /// time is local wall time while the GPS stamp is UTC; when the file
    /// carries no OffsetTimeOriginal, the unknown timezone is assumed to be
    /// whatever multiple of a quarter hour lands closest, which measures
    /// staleness up to ±7.5 minutes - plenty for a fix from minutes ago.
    /// </summary>
    internal static double? ComputeGpsFixAgeSeconds(
        DateTime takenLocal,
        DateTime fixUtc,
        TimeSpan? utcOffset)
    {
        var difference = takenLocal - fixUtc;
        if (utcOffset is { } offset)
        {
            // A stamp slightly ahead of the photo clock is just skew between
            // the phone clock and GPS time, not a fix from the future.
            return Math.Max(0, (difference - offset).TotalSeconds);
        }

        var quarterHours = Math.Round(difference.TotalMinutes / 15);
        var residualMinutes = difference.TotalMinutes - (quarterHours * 15);
        return Math.Max(0, residualMinutes * 60);
    }

    /// <summary>Parses an EXIF OffsetTime string such as "+02:00".</summary>
    internal static TimeSpan? ParseUtcOffset(string? value)
    {
        var trimmed = value?.Trim();
        if (string.IsNullOrEmpty(trimmed))
        {
            return null;
        }

        if (trimmed is "Z")
        {
            return TimeSpan.Zero;
        }

        var negative = trimmed[0] == '-';
        var body = trimmed[0] is '+' or '-' ? trimmed[1..] : trimmed;
        return TimeSpan.TryParseExact(
            body,
            @"hh\:mm",
            CultureInfo.InvariantCulture,
            out var parsed)
            ? negative ? -parsed : parsed
            : null;
    }

    private readonly record struct DescriptiveMetadata(
        int Rating,
        string? Title,
        string? Description,
        double? Latitude,
        double? Longitude,
        ColorLabel ColorLabel = ColorLabel.None,
        string? Keywords = null);

    private readonly record struct TechnicalMetadata(
        int? PixelWidth,
        int? PixelHeight,
        string? Camera,
        string? Lens,
        double? FocalLength,
        double? Aperture,
        double? ExposureSeconds,
        int? Iso);

    /// <summary>
    /// The shooting data shown in the info panel and used by the camera, lens
    /// and orientation filters.
    /// </summary>
    private static TechnicalMetadata ReadTechnical(
        IReadOnlyCollection<MetadataExtractor.Directory> metadata)
    {
        int? width = null;
        int? height = null;
        string? make = null;
        string? model = null;
        string? lens = null;
        double? focalLength = null;
        double? aperture = null;
        double? exposure = null;
        int? iso = null;

        foreach (var directory in metadata.OfType<ExifSubIfdDirectory>())
        {
            width ??= TryGetInt(directory, ExifDirectoryBase.TagExifImageWidth);
            height ??= TryGetInt(directory, ExifDirectoryBase.TagExifImageHeight);
            focalLength ??= TryGetDouble(directory, ExifDirectoryBase.TagFocalLength);
            aperture ??= TryGetDouble(directory, ExifDirectoryBase.TagFNumber);
            exposure ??= TryGetDouble(directory, ExifDirectoryBase.TagExposureTime);
            iso ??= TryGetInt(directory, ExifDirectoryBase.TagIsoEquivalent);
            lens ??= Normalize(directory.GetDescription(ExifDirectoryBase.TagLensModel));
        }

        foreach (var directory in metadata.OfType<ExifIfd0Directory>())
        {
            make ??= Normalize(directory.GetDescription(ExifDirectoryBase.TagMake));
            model ??= Normalize(directory.GetDescription(ExifDirectoryBase.TagModel));
        }

        if (width is null || height is null)
        {
            // JPEG, PNG and WebP each report their real frame size in their own
            // header directory, which is also the only source for a file that
            // carries no EXIF at all.
            foreach (var directory in metadata)
            {
                foreach (var tag in directory.Tags)
                {
                    if (width is null
                        && tag.Name.Equals("Image Width", StringComparison.Ordinal))
                    {
                        width = ParseLeadingInt(tag.Description);
                    }
                    else if (height is null
                             && tag.Name.Equals("Image Height", StringComparison.Ordinal))
                    {
                        height = ParseLeadingInt(tag.Description);
                    }
                }

                if (width is not null && height is not null)
                {
                    break;
                }
            }
        }

        var camera = BuildCameraName(make, model);
        return new TechnicalMetadata(
            width,
            height,
            camera,
            lens,
            focalLength,
            aperture,
            exposure,
            iso);
    }

    /// <summary>
    /// "NIKON CORPORATION" + "NIKON Z 6" should read as one camera, not as a
    /// stuttering "NIKON CORPORATION NIKON Z 6".
    /// </summary>
    internal static string? BuildCameraName(string? make, string? model)
    {
        if (string.IsNullOrWhiteSpace(model))
        {
            return Normalize(make);
        }

        if (string.IsNullOrWhiteSpace(make))
        {
            return Normalize(model);
        }

        var firstMakeWord = make.Split(' ', StringSplitOptions.RemoveEmptyEntries)
            .FirstOrDefault();
        if (firstMakeWord is not null
            && model.StartsWith(firstMakeWord, StringComparison.OrdinalIgnoreCase))
        {
            return Normalize(model);
        }

        return Normalize($"{firstMakeWord ?? make} {model}");
    }

    private static int? TryGetInt(
        MetadataExtractor.Directory directory,
        int tag) =>
        directory.TryGetInt32(tag, out var value) ? value : null;

    private static double? TryGetDouble(
        MetadataExtractor.Directory directory,
        int tag) =>
        directory.TryGetDouble(tag, out var value) && double.IsFinite(value)
            ? value
            : null;

    private static int? ParseLeadingInt(string? description)
    {
        if (string.IsNullOrWhiteSpace(description))
        {
            return null;
        }

        var digits = new string(
            description.TakeWhile(char.IsDigit).ToArray());
        return int.TryParse(digits, out var value) && value > 0 ? value : null;
    }

    private static DescriptiveMetadata ReadDescriptive(
        IReadOnlyCollection<MetadataExtractor.Directory> metadata)
    {
        var rating = 0;
        string? title = null;
        string? description = null;
        double? latitude = null;
        double? longitude = null;
        var colorLabel = ColorLabel.None;
        string? keywords = null;

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

            if (colorLabel == ColorLabel.None
                && properties.TryGetValue("xmp:Label", out var xmpLabel))
            {
                colorLabel = PhotoLabels.FromXmpName(xmpLabel);
            }

            keywords ??= ReadXmpBag(properties, "dc:subject");
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
            longitude,
            colorLabel,
            keywords);
    }

    /// <summary>
    /// XMP arrays surface as indexed properties (<c>dc:subject[1]</c>,
    /// <c>dc:subject[2]</c>, ...), which is how keyword lists arrive.
    /// </summary>
    private static string? ReadXmpBag(
        IDictionary<string, string> properties,
        string key)
    {
        var values = new List<string>();
        for (var index = 1; ; index++)
        {
            if (!properties.TryGetValue($"{key}[{index}]", out var value))
            {
                break;
            }

            if (Normalize(value) is { } normalized)
            {
                values.Add(normalized);
            }
        }

        if (values.Count == 0
            && properties.TryGetValue(key, out var single)
            && Normalize(single) is { } singleValue)
        {
            values.Add(singleValue);
        }

        return values.Count == 0 ? null : PhotoRecord.JoinKeywords(values);
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

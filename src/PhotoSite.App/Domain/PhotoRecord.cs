namespace PhotoSite.Domain;

public enum PhotoOrientation
{
    Unknown = 0,
    Landscape = 1,
    Portrait = 2,
    Square = 3
}

public sealed record PhotoRecord(
    string Path,
    string RootPath,
    string FileName,
    string Extension,
    long Length,
    long ModifiedUtcTicks,
    int Rating,
    long ScanId,
    long? TakenAtTicks = null,
    PhotoDateSource TakenAtSource = PhotoDateSource.None,
    int MetadataVersion = 0,
    string? Title = null,
    string? Description = null,
    double? Latitude = null,
    double? Longitude = null,
    ColorLabel ColorLabel = ColorLabel.None,
    PhotoFlag Flag = PhotoFlag.None,
    string? Keywords = null,
    int? PixelWidth = null,
    int? PixelHeight = null,
    string? Camera = null,
    string? Lens = null,
    double? FocalLength = null,
    double? Aperture = null,
    double? ExposureSeconds = null,
    int? Iso = null,
    string? DescriptionEn = null)
{
    public PhotoOrientation Orientation =>
        PixelWidth is not { } width || PixelHeight is not { } height
            ? PhotoOrientation.Unknown
            : width > height
                ? PhotoOrientation.Landscape
                : width < height
                    ? PhotoOrientation.Portrait
                    : PhotoOrientation.Square;

    public IReadOnlyList<string> KeywordList =>
        string.IsNullOrWhiteSpace(Keywords)
            ? []
            : Keywords
                .Split(';', StringSplitOptions.RemoveEmptyEntries
                            | StringSplitOptions.TrimEntries)
                .ToArray();

    public static string JoinKeywords(IEnumerable<string> keywords) =>
        string.Join(
            "; ",
            keywords
                .Select(keyword => keyword.Trim())
                .Where(keyword => keyword.Length > 0)
                .Distinct(StringComparer.OrdinalIgnoreCase));
}

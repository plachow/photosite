namespace PhotoSite.Domain;

public enum PhotoOrientation
{
    Unknown = 0,
    Landscape = 1,
    Portrait = 2,
    Square = 3
}

/// <summary>
/// The verdict the file's own GPS evidence renders over the coordinates,
/// worst tier last so comparisons read naturally.
/// </summary>
public enum LocationAccuracy
{
    /// <summary>No coordinates at all.</summary>
    None = 0,

    /// <summary>Nothing disputes the position.</summary>
    Precise = 1,

    /// <summary>Probably off by a street or two.</summary>
    Approximate = 2,

    /// <summary>Probably a different place altogether.</summary>
    Poor = 3
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
    string? DescriptionEn = null,
    double? GpsErrorMeters = null,
    double? GpsFixAgeSeconds = null,
    string? GpsProcessingMethod = null,
    double? GpsAltitude = null)
{
    /// <summary>
    /// A receiver owning up to a coarser error estimate than this is usually
    /// a Wi-Fi or cell-tower guess, not a satellite fix.
    /// </summary>
    public const double ApproximateGpsErrorMeters = 100;

    /// <summary>
    /// A fix already this stale when the shutter fired is the "quickly pull
    /// the phone out" photo: it carries the location from minutes - and
    /// hundreds of metres - back along the walk.
    /// </summary>
    public const double ApproximateGpsFixAgeSeconds = 120;

    /// <summary>
    /// Beyond half a kilometre of reported error - or a fix ten minutes
    /// stale - the coordinates likely point at a different place altogether,
    /// not just the wrong end of a street.
    /// </summary>
    public const double PoorGpsErrorMeters = 500;

    public const double PoorGpsFixAgeSeconds = 600;

    /// <summary>
    /// The position came from cell towers or Wi-Fi rather than satellites -
    /// phones write GPSProcessingMethod values like CELLID, WLAN or NETWORK
    /// when they stamp a network position, often one cached from long before
    /// the shutter fired.
    /// </summary>
    public bool IsNetworkPositioned =>
        GpsProcessingMethod is { } method
        && (method.Contains("CELL", StringComparison.OrdinalIgnoreCase)
            || method.Contains("WLAN", StringComparison.OrdinalIgnoreCase)
            || method.Contains("NETWORK", StringComparison.OrdinalIgnoreCase));

    /// <summary>
    /// A pure cell-tower guess: the method says CELLID and there is no real
    /// altitude - a receiver that knows its height above the sea had an
    /// actual fix, a tower estimate writes zero. Typically kilometres off.
    /// </summary>
    public bool IsCellTowerFix =>
        GpsProcessingMethod is { } method
        && method.Contains("CELL", StringComparison.OrdinalIgnoreCase)
        && GpsAltitude.GetValueOrDefault() == 0;

    /// <summary>
    /// Graded from the fix evidence the scan read out of the file; a typed
    /// correction clears the evidence and turns the verdict precise.
    /// </summary>
    public LocationAccuracy LocationAccuracy =>
        Latitude is null || Longitude is null
            ? LocationAccuracy.None
            : GpsErrorMeters >= PoorGpsErrorMeters
              || GpsFixAgeSeconds >= PoorGpsFixAgeSeconds
              || IsCellTowerFix
                ? LocationAccuracy.Poor
                : GpsErrorMeters >= ApproximateGpsErrorMeters
                  || GpsFixAgeSeconds >= ApproximateGpsFixAgeSeconds
                  || IsNetworkPositioned
                    ? LocationAccuracy.Approximate
                    : LocationAccuracy.Precise;

    /// <summary>
    /// True when the file's own GPS evidence says the coordinates are
    /// probably off - a little or a lot; the badge, the filter and the AI
    /// describer treat both graded tiers the same way.
    /// </summary>
    public bool HasApproximateLocation =>
        LocationAccuracy >= LocationAccuracy.Approximate;

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

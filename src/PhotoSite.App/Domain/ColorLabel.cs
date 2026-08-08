namespace PhotoSite.Domain;

/// <summary>
/// The Lightroom/Bridge colour labels, stored as the XMP <c>xmp:Label</c>
/// names so a label set here survives a round-trip through other software.
/// </summary>
public enum ColorLabel
{
    None = 0,
    Red = 1,
    Yellow = 2,
    Green = 3,
    Blue = 4,
    Purple = 5
}

/// <summary>
/// The culling verdict. Rejected photos stay on disk and in the catalogue -
/// deleting is always an explicit second step.
/// </summary>
public enum PhotoFlag
{
    Rejected = -1,
    None = 0,
    Picked = 1
}

public static class PhotoLabels
{
    public static string ToXmpName(this ColorLabel label) => label switch
    {
        ColorLabel.Red => "Red",
        ColorLabel.Yellow => "Yellow",
        ColorLabel.Green => "Green",
        ColorLabel.Blue => "Blue",
        ColorLabel.Purple => "Purple",
        _ => string.Empty
    };

    public static ColorLabel FromXmpName(string? name) =>
        name?.Trim().ToLowerInvariant() switch
        {
            "red" => ColorLabel.Red,
            "yellow" => ColorLabel.Yellow,
            "green" => ColorLabel.Green,
            "blue" => ColorLabel.Blue,
            "purple" => ColorLabel.Purple,
            _ => ColorLabel.None
        };

    /// <summary>The swatch colour shown on tiles and in the filter bar.</summary>
    public static string ToHexColor(this ColorLabel label) => label switch
    {
        ColorLabel.Red => "#E5533D",
        ColorLabel.Yellow => "#E8B84A",
        ColorLabel.Green => "#6FBF5B",
        ColorLabel.Blue => "#4B9BE8",
        ColorLabel.Purple => "#A67BD8",
        _ => "#00000000"
    };
}

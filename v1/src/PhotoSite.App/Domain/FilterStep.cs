namespace PhotoSite.Domain;

public enum PhotoFilterKind
{
    Sharpen,
    UnsharpMask,
    Blur,
    GaussianBlur,
    Pixelize,
    NoiseReduction,
    AddNoise,
    Grayscale,
    Sepia,
    Vignette
}

/// <summary>
/// One creative filter in the recipe. The three generic parameters keep the
/// persisted shape stable while each filter interprets them itself, which is
/// what lets a new filter ship without another schema migration.
/// </summary>
public sealed record FilterStep(
    PhotoFilterKind Kind,
    double Amount = 50,
    double Radius = 2,
    double Threshold = 0)
{
    public string DisplayName => Kind switch
    {
        PhotoFilterKind.Sharpen => "Sharpen",
        PhotoFilterKind.UnsharpMask => "Unsharp mask",
        PhotoFilterKind.Blur => "Blur",
        PhotoFilterKind.GaussianBlur => "Gaussian blur",
        PhotoFilterKind.Pixelize => "Pixelize",
        PhotoFilterKind.NoiseReduction => "Noise reduction",
        PhotoFilterKind.AddNoise => "Add noise",
        PhotoFilterKind.Grayscale => "Grayscale",
        PhotoFilterKind.Sepia => "Sepia",
        PhotoFilterKind.Vignette => "Vignette",
        _ => Kind.ToString()
    };

    /// <summary>
    /// The neutral starting point shown when a filter dialog opens, chosen so
    /// the very first preview already looks like a sensible edit.
    /// </summary>
    public static FilterStep CreateDefault(PhotoFilterKind kind) => kind switch
    {
        PhotoFilterKind.Sharpen => new FilterStep(kind, 60, 1),
        PhotoFilterKind.UnsharpMask => new FilterStep(kind, 80, 2, 4),
        PhotoFilterKind.Blur => new FilterStep(kind, 100, 2),
        PhotoFilterKind.GaussianBlur => new FilterStep(kind, 100, 4),
        PhotoFilterKind.Pixelize => new FilterStep(kind, 100, 8),
        PhotoFilterKind.NoiseReduction => new FilterStep(kind, 50, 2),
        PhotoFilterKind.AddNoise => new FilterStep(kind, 15, 0),
        PhotoFilterKind.Grayscale => new FilterStep(kind, 100, 0),
        PhotoFilterKind.Sepia => new FilterStep(kind, 100, 0),
        PhotoFilterKind.Vignette => new FilterStep(kind, 45, 55),
        _ => new FilterStep(kind)
    };

    public bool UsesRadius => Kind is not (
        PhotoFilterKind.Grayscale
        or PhotoFilterKind.Sepia
        or PhotoFilterKind.AddNoise);

    public bool UsesThreshold => Kind is PhotoFilterKind.UnsharpMask
        or PhotoFilterKind.Sharpen
        or PhotoFilterKind.NoiseReduction;

    public string RadiusLabel => Kind switch
    {
        PhotoFilterKind.Pixelize => "Cell size",
        PhotoFilterKind.Vignette => "Midpoint",
        _ => "Radius"
    };
}

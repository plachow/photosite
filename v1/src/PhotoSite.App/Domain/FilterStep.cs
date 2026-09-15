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
    Vignette,
    MotionBlur,
    Deinterlace,
    Invert,
    Posterize,
    Solarize
}

/// <summary>
/// One creative filter in the recipe. The generic parameters keep the
/// persisted shape stable while each filter interprets them itself, which is
/// what lets a new filter ship without another schema migration.
/// </summary>
/// <remarks>
/// <see cref="Mode"/> is the per-kind variant selector, always 0 for the
/// plain behaviour a recipe written before it existed expects:
/// <list type="bullet">
/// <item>Sharpen, UnsharpMask: 1 sharpens the luminance only, which keeps
/// colour fringes from growing along edges.</item>
/// <item>AddNoise: 1 draws a different sample per channel, so the grain is
/// coloured rather than monochrome.</item>
/// <item>NoiseReduction: 1 reads the colour strength from
/// <see cref="Radius"/> instead of deriving it from <see cref="Amount"/>.</item>
/// <item>Deinterlace: 0 keeps the even lines, 1 the odd, 2 blends.</item>
/// <item>MotionBlur: <see cref="Radius"/> is the length in pixels and
/// <see cref="Threshold"/> the angle in degrees.</item>
/// <item>Vignette: <see cref="Radius"/> is the midpoint; <see cref="Threshold"/>
/// the feather, 0 meaning the classic hard-edged roll-off.</item>
/// <item>Posterize: <see cref="Radius"/> is the number of levels per
/// channel.</item>
/// </list>
/// </remarks>
public sealed record FilterStep(
    PhotoFilterKind Kind,
    double Amount = 50,
    double Radius = 2,
    double Threshold = 0)
{
    public int Mode { get; init; }

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
        PhotoFilterKind.MotionBlur => "Motion blur",
        PhotoFilterKind.Deinterlace => "Deinterlace",
        PhotoFilterKind.Invert => "Invert",
        PhotoFilterKind.Posterize => "Posterize",
        PhotoFilterKind.Solarize => "Solarize",
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
        PhotoFilterKind.MotionBlur => new FilterStep(kind, 100, 12, 0),
        PhotoFilterKind.Deinterlace => new FilterStep(kind, 100, 0) { Mode = 2 },
        PhotoFilterKind.Invert => new FilterStep(kind, 100, 0),
        PhotoFilterKind.Posterize => new FilterStep(kind, 100, 6),
        PhotoFilterKind.Solarize => new FilterStep(kind, 100, 50),
        _ => new FilterStep(kind)
    };

    public bool UsesRadius => Kind is not (
        PhotoFilterKind.Grayscale
        or PhotoFilterKind.Sepia
        or PhotoFilterKind.AddNoise
        or PhotoFilterKind.Deinterlace
        or PhotoFilterKind.Invert);

    public bool UsesThreshold => Kind is PhotoFilterKind.UnsharpMask
        or PhotoFilterKind.Sharpen
        or PhotoFilterKind.NoiseReduction
        or PhotoFilterKind.MotionBlur
        or PhotoFilterKind.Vignette;

    public string RadiusLabel => Kind switch
    {
        PhotoFilterKind.Pixelize => "Cell size",
        PhotoFilterKind.Vignette => "Midpoint",
        PhotoFilterKind.MotionBlur => "Length",
        PhotoFilterKind.Posterize => "Levels",
        PhotoFilterKind.Solarize => "Threshold",
        _ => "Radius"
    };
}

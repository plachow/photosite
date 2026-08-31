namespace PhotoSite.Domain;

/// <summary>
/// Every photographic adjustment PhotoSite can apply to a decoded frame.
/// All numeric sliders are neutral at their default value, so
/// <see cref="Neutral"/> is simply <c>new()</c> and a recipe that has never
/// been touched serializes to an almost empty JSON object.
/// </summary>
public sealed record PhotoAdjustments
{
    public static PhotoAdjustments Neutral { get; } = new();

    // Tone, all -100..100 except Exposure which is in EV stops.
    public double Exposure { get; init; }

    public double Brightness { get; init; }

    public double Contrast { get; init; }

    public double Highlights { get; init; }

    public double Shadows { get; init; }

    public double Whites { get; init; }

    public double Blacks { get; init; }

    public double Clarity { get; init; }

    // Colour.
    public double Saturation { get; init; }

    public double Vibrance { get; init; }

    public double Temperature { get; init; }

    public double Tint { get; init; }

    public double Gamma { get; init; } = 1;

    // Levels, expressed in 0..255 input space with a midtone gamma.
    public double BlackPoint { get; init; }

    public double WhitePoint { get; init; } = 255;

    public double MidPoint { get; init; } = 1;

    // Curves.
    public ToneCurve Curve { get; init; } = ToneCurve.Linear;

    public ToneCurve RedCurve { get; init; } = ToneCurve.Linear;

    public ToneCurve GreenCurve { get; init; } = ToneCurve.Linear;

    public ToneCurve BlueCurve { get; init; } = ToneCurve.Linear;

    // Detail.
    public double SharpenAmount { get; init; }

    public double SharpenRadius { get; init; } = 1;

    public double SharpenThreshold { get; init; }

    public double LuminanceNoiseReduction { get; init; }

    public double ColorNoiseReduction { get; init; }

    // Lens and creative.
    public double Vignette { get; init; }

    public double LensDistortion { get; init; }

    public double LensVignetting { get; init; }

    public double ChromaticAberration { get; init; }

    public bool IsNeutral => Equals(Neutral);

    public bool HasToneOrColorChange =>
        Exposure != 0
        || Brightness != 0
        || Contrast != 0
        || Highlights != 0
        || Shadows != 0
        || Whites != 0
        || Blacks != 0
        || Saturation != 0
        || Vibrance != 0
        || Temperature != 0
        || Tint != 0
        || Gamma != 1
        || BlackPoint != 0
        || WhitePoint != 255
        || MidPoint != 1
        || !Curve.IsLinear
        || !RedCurve.IsLinear
        || !GreenCurve.IsLinear
        || !BlueCurve.IsLinear;

    public bool HasDetailChange =>
        SharpenAmount != 0
        || Clarity != 0
        || LuminanceNoiseReduction != 0
        || ColorNoiseReduction != 0
        || ChromaticAberration != 0;

    public bool HasLensOrVignette =>
        Vignette != 0 || LensVignetting != 0 || LensDistortion != 0;

    /// <summary>
    /// Resets only the tonal and colour sliders, keeping detail and lens work.
    /// </summary>
    public PhotoAdjustments WithNeutralTone() =>
        this with
        {
            Exposure = 0,
            Brightness = 0,
            Contrast = 0,
            Highlights = 0,
            Shadows = 0,
            Whites = 0,
            Blacks = 0,
            Saturation = 0,
            Vibrance = 0,
            Temperature = 0,
            Tint = 0,
            Gamma = 1,
            BlackPoint = 0,
            WhitePoint = 255,
            MidPoint = 1,
            Curve = ToneCurve.Linear,
            RedCurve = ToneCurve.Linear,
            GreenCurve = ToneCurve.Linear,
            BlueCurve = ToneCurve.Linear
        };
}

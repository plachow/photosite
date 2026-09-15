using PhotoSite.Domain;

namespace PhotoSite.EditorTools;

/// <summary>
/// A tool that appends one <see cref="FilterStep"/> to the recipe. The
/// settings are the step itself, so a preset is a filter step in JSON and
/// the last used one is simply the step applied last time.
/// </summary>
internal sealed class FilterTool : EditTool<FilterStep>
{
    private readonly SliderRange amount;
    private readonly SliderRange? radius;
    private readonly SliderRange? threshold;
    private readonly string? modeToggle;
    private readonly (string Label, string[] Options)? modeChoice;

    public FilterTool(
        PhotoFilterKind kind,
        string title,
        string? hint,
        SliderRange amount,
        SliderRange? radius = null,
        SliderRange? threshold = null,
        string? modeToggle = null,
        (string Label, string[] Options)? modeChoice = null)
        : base(FilterStep.CreateDefault(kind))
    {
        Kind = kind;
        Title = title;
        Hint = hint;
        this.amount = amount;
        this.radius = radius;
        this.threshold = threshold;
        this.modeToggle = modeToggle;
        this.modeChoice = modeChoice;
    }

    public PhotoFilterKind Kind { get; }

    public override string Id => "filter." + Kind.ToString().ToLowerInvariant();

    public override string Title { get; }

    public override string? Hint { get; }

    public override EditRecipe Apply(EditRecipe recipe) =>
        recipe with { Filters = [.. recipe.Filters, Settings with { Kind = Kind }] };

    protected override ToolPanelBuilder<FilterStep> Describe(
        ToolPanelBuilder<FilterStep> panel,
        EditToolContext context)
    {
        if (modeChoice is { } choice)
        {
            panel.Choice(
                choice.Label,
                choice.Options,
                step => step.Mode,
                (step, mode) => step with { Mode = mode });
        }

        panel.Slider(
            amount.Label,
            amount.Minimum,
            amount.Maximum,
            step => step.Amount,
            (step, value) => step with { Amount = value },
            amount.Format);
        if (radius is { } radiusRange)
        {
            panel.Slider(
                radiusRange.Label,
                radiusRange.Minimum,
                radiusRange.Maximum,
                step => step.Radius,
                (step, value) => step with { Radius = value },
                radiusRange.Format);
        }

        if (threshold is { } thresholdRange)
        {
            panel.Slider(
                thresholdRange.Label,
                thresholdRange.Minimum,
                thresholdRange.Maximum,
                step => step.Threshold,
                (step, value) => step with { Threshold = value },
                thresholdRange.Format);
        }

        if (modeToggle is not null)
        {
            panel.Toggle(
                modeToggle,
                step => step.Mode == 1,
                (step, on) => step with { Mode = on ? 1 : 0 });
        }

        return panel;
    }
}

internal readonly record struct SliderRange(
    string Label,
    double Minimum,
    double Maximum,
    string Format = "0");

/// <summary>
/// Sharpening with the three approaches a photo editor is expected to offer:
/// a plain one-slider sharpen, the unsharp mask with radius and threshold,
/// and a wide-radius pass that lifts local contrast. All three are the same
/// unsharp mask underneath, differing in radius and in what the sliders
/// expose, and any of them can be restricted to luminance so that no colour
/// halo forms along an edge.
/// </summary>
internal sealed class SharpenTool : EditTool<SharpenSettings>
{
    public const int SimpleType = 0;
    public const int UnsharpMaskType = 1;
    public const int GaussianType = 2;

    private static readonly string[] TypeNames =
    [
        "Simple sharpen",
        "Unsharp mask",
        "Gaussian sharpen"
    ];

    public SharpenTool()
        : base(new SharpenSettings())
    {
    }

    public override string Id => "sharpen";

    public override string Title => "Sharpen";

    public override string Hint =>
        "Unsharp mask is the everyday choice; Gaussian sharpening with a wide "
        + "radius lifts local contrast instead of edges. Luminance only keeps "
        + "colour fringes from growing.";

    public override EditRecipe Apply(EditRecipe recipe) =>
        recipe with { Filters = [.. recipe.Filters, ToStep(Settings)] };

    public static FilterStep ToStep(SharpenSettings settings)
    {
        var mode = settings.LuminanceOnly ? 1 : 0;
        return settings.Type switch
        {
            SimpleType => new FilterStep(PhotoFilterKind.Sharpen, settings.Amount, 0.7, 0)
            {
                Mode = mode
            },
            GaussianType => new FilterStep(
                PhotoFilterKind.UnsharpMask,
                settings.Amount,
                settings.GaussianRadius,
                settings.NoiseSuppression)
            {
                Mode = mode
            },
            _ => new FilterStep(
                PhotoFilterKind.UnsharpMask,
                settings.Amount,
                settings.Radius,
                settings.Threshold)
            {
                Mode = mode
            }
        };
    }

    protected override ToolPanelBuilder<SharpenSettings> Describe(
        ToolPanelBuilder<SharpenSettings> panel,
        EditToolContext context) =>
        panel
            .Choice(
                "Type",
                TypeNames,
                settings => settings.Type,
                (settings, type) => settings with { Type = type })
            .Slider(
                "Strength",
                0,
                500,
                settings => settings.Amount,
                (settings, value) => settings with { Amount = value })
            .Slider(
                "Radius",
                0.3,
                10,
                settings => settings.Radius,
                (settings, value) => settings with { Radius = value },
                "0.0")
            .VisibleWhen(settings => settings.Type == UnsharpMaskType)
            .Slider(
                "Threshold",
                0,
                60,
                settings => settings.Threshold,
                (settings, value) => settings with { Threshold = value })
            .VisibleWhen(settings => settings.Type == UnsharpMaskType)
            .Slider(
                "Radius",
                1,
                60,
                settings => settings.GaussianRadius,
                (settings, value) => settings with { GaussianRadius = value },
                "0.0")
            .VisibleWhen(settings => settings.Type == GaussianType)
            .Slider(
                "Noise suppression",
                0,
                60,
                settings => settings.NoiseSuppression,
                (settings, value) => settings with { NoiseSuppression = value })
            .VisibleWhen(settings => settings.Type == GaussianType)
            .Toggle(
                "Luminance only",
                settings => settings.LuminanceOnly,
                (settings, on) => settings with { LuminanceOnly = on });
}

internal sealed record SharpenSettings(
    int Type = SharpenTool.UnsharpMaskType,
    double Amount = 80,
    double Radius = 1.2,
    double Threshold = 2,
    double GaussianRadius = 12,
    double NoiseSuppression = 10,
    bool LuminanceOnly = false);

/// <summary>
/// Blur in its three shapes: a soft box blur, a Gaussian, and the directional
/// streak of motion.
/// </summary>
internal sealed class BlurTool : EditTool<BlurSettings>
{
    public const int BoxType = 0;
    public const int GaussianType = 1;
    public const int MotionType = 2;

    private static readonly string[] TypeNames =
    [
        "Soft blur",
        "Gaussian blur",
        "Motion blur"
    ];

    public BlurTool()
        : base(new BlurSettings())
    {
    }

    public override string Id => "blur";

    public override string Title => "Blur";

    public override string Hint =>
        "Strength blends the blurred copy with the sharp photograph, so a "
        + "wide radius at low strength gives a gentle glow rather than mush.";

    public override EditRecipe Apply(EditRecipe recipe) =>
        recipe with { Filters = [.. recipe.Filters, ToStep(Settings)] };

    public static FilterStep ToStep(BlurSettings settings) => settings.Type switch
    {
        MotionType => new FilterStep(
            PhotoFilterKind.MotionBlur,
            settings.Amount,
            settings.Length,
            settings.Angle),
        BoxType => new FilterStep(PhotoFilterKind.Blur, settings.Amount, settings.Radius),
        _ => new FilterStep(PhotoFilterKind.GaussianBlur, settings.Amount, settings.Radius)
    };

    protected override ToolPanelBuilder<BlurSettings> Describe(
        ToolPanelBuilder<BlurSettings> panel,
        EditToolContext context) =>
        panel
            .Choice(
                "Type",
                TypeNames,
                settings => settings.Type,
                (settings, type) => settings with { Type = type })
            .Slider(
                "Strength",
                0,
                100,
                settings => settings.Amount,
                (settings, value) => settings with { Amount = value })
            .Slider(
                "Radius",
                0.5,
                60,
                settings => settings.Radius,
                (settings, value) => settings with { Radius = value },
                "0.0")
            .VisibleWhen(settings => settings.Type != MotionType)
            .Slider(
                "Length",
                1,
                200,
                settings => settings.Length,
                (settings, value) => settings with { Length = value })
            .VisibleWhen(settings => settings.Type == MotionType)
            .Slider(
                "Angle",
                -180,
                180,
                settings => settings.Angle,
                (settings, value) => settings with { Angle = value })
            .VisibleWhen(settings => settings.Type == MotionType);
}

internal sealed record BlurSettings(
    int Type = BlurTool.GaussianType,
    double Amount = 100,
    double Radius = 4,
    double Length = 12,
    double Angle = 0);

/// <summary>
/// Every editor tool the menus offer, in one place, so the menu, the
/// shortcuts and the tests agree on the list.
/// </summary>
internal static class EditorToolCatalog
{
    public static FilterTool AddNoise() => new(
        PhotoFilterKind.AddNoise,
        "Add noise",
        "Grain hides banding in smooth gradients and gives a scan its film "
        + "back. Coloured grain varies per channel; plain grain is monochrome.",
        new SliderRange("Strength", 0, 100),
        modeToggle: "Coloured grain");

    public static FilterTool Vignette() => new(
        PhotoFilterKind.Vignette,
        "Vignetting",
        "Negative strength lightens the corners instead. The midpoint is "
        + "where the roll-off starts; feather sets how gently it arrives.",
        new SliderRange("Strength", -100, 100),
        new SliderRange("Midpoint", 1, 100),
        new SliderRange("Feather", 1, 100));

    public static FilterTool Deinterlace() => new(
        PhotoFilterKind.Deinterlace,
        "Deinterlace",
        "Takes the comb out of a still taken from interlaced video by "
        + "rebuilding one field from the other.",
        new SliderRange("Strength", 0, 100),
        modeChoice: ("Keep", ["Even lines", "Odd lines", "Blend both"]));

    public static FilterTool Grayscale() => new(
        PhotoFilterKind.Grayscale,
        "Grayscale",
        "Luminance-weighted, so a red and a green of the same brightness "
        + "land on the same grey.",
        new SliderRange("Strength", 0, 100));

    public static FilterTool Sepia() => new(
        PhotoFilterKind.Sepia,
        "Sepia",
        null,
        new SliderRange("Strength", 0, 100));

    public static FilterTool Invert() => new(
        PhotoFilterKind.Invert,
        "Invert",
        "A negative; at partial strength a faded, solarized look.",
        new SliderRange("Strength", 0, 100));

    public static FilterTool Posterize() => new(
        PhotoFilterKind.Posterize,
        "Posterize",
        "Reduces every channel to a handful of levels.",
        new SliderRange("Strength", 0, 100),
        new SliderRange("Levels", 2, 32));

    public static FilterTool Solarize() => new(
        PhotoFilterKind.Solarize,
        "Solarize",
        "Inverts everything brighter than the threshold, the darkroom "
        + "accident turned effect.",
        new SliderRange("Strength", 0, 100),
        new SliderRange("Threshold", 0, 100));

    public static FilterTool Pixelize() => new(
        PhotoFilterKind.Pixelize,
        "Pixelize",
        "Averages the photograph into square cells - the usual way to hide a "
        + "face or a number plate.",
        new SliderRange("Strength", 0, 100),
        new SliderRange("Cell size", 2, 120));
}

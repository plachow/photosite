using PhotoSite.Domain;

namespace PhotoSite.EditorTools;

internal sealed record CombinedSettings(
    double Exposure = 0,
    double Contrast = 0,
    double Highlights = 0,
    double Shadows = 0,
    double Temperature = 0,
    double Tint = 0,
    double Vibrance = 0,
    double Saturation = 0,
    double Clarity = 0);

/// <summary>
/// The everyday correction in one window: light, white balance and colour
/// together, for the photograph that needs a little of each.
/// </summary>
internal sealed class CombinedTool : AdjustmentTool<CombinedSettings>
{
    public CombinedTool()
        : base(new CombinedSettings())
    {
    }

    public override string Id => "combined";

    public override string Title => "Combined adjustment";

    public override string Hint =>
        "Auto fills every section from a measurement of the photograph; each "
        + "value it chose stays on its slider to be taken further or back.";

    protected override CombinedSettings Read(PhotoAdjustments adjustments) => new(
        adjustments.Exposure,
        adjustments.Contrast,
        adjustments.Highlights,
        adjustments.Shadows,
        adjustments.Temperature,
        adjustments.Tint,
        adjustments.Vibrance,
        adjustments.Saturation,
        adjustments.Clarity);

    protected override PhotoAdjustments Write(
        PhotoAdjustments adjustments,
        CombinedSettings settings) =>
        adjustments with
        {
            Exposure = settings.Exposure,
            Contrast = settings.Contrast,
            Highlights = settings.Highlights,
            Shadows = settings.Shadows,
            Temperature = settings.Temperature,
            Tint = settings.Tint,
            Vibrance = settings.Vibrance,
            Saturation = settings.Saturation,
            Clarity = settings.Clarity
        };

    protected override ToolPanelBuilder<CombinedSettings> Describe(
        ToolPanelBuilder<CombinedSettings> panel,
        EditToolContext context) =>
        panel
            .Button(
                "Auto",
                () =>
                {
                    var measured = Measure(context);
                    Settings = Settings with
                    {
                        Exposure = measured.Exposure,
                        Contrast = measured.Contrast,
                        Highlights = measured.Highlights,
                        Shadows = measured.Shadows,
                        Temperature = measured.Temperature,
                        Tint = measured.Tint,
                        Vibrance = measured.Vibrance,
                        Saturation = measured.Saturation
                    };
                },
                "Measure the photograph and fill every section conservatively")
            .Header("Light")
            .Slider("Exposure", -4, 4, s => s.Exposure, (s, v) => s with { Exposure = v }, "0.00")
            .Slider("Contrast", -100, 100, s => s.Contrast, (s, v) => s with { Contrast = v })
            .Slider("Highlights", -100, 100, s => s.Highlights, (s, v) => s with { Highlights = v })
            .Slider("Shadows", -100, 100, s => s.Shadows, (s, v) => s with { Shadows = v })
            .Slider("Clarity", -100, 100, s => s.Clarity, (s, v) => s with { Clarity = v })
            .Header("White balance")
            .Slider("Temperature", -100, 100, s => s.Temperature, (s, v) => s with { Temperature = v })
            .Slider("Tint", -100, 100, s => s.Tint, (s, v) => s with { Tint = v })
            .Header("Colour")
            .Slider("Vibrance", -100, 100, s => s.Vibrance, (s, v) => s with { Vibrance = v })
            .Slider("Saturation", -100, 100, s => s.Saturation, (s, v) => s with { Saturation = v });
}

internal sealed record OldPhotoSettings(
    double Sepia = 85,
    double Vignette = 45,
    double Grain = 12,
    double Fade = 15);

/// <summary>
/// A print from a shoebox: sepia, a vignette, some grain and a lifted black
/// point, appended as the filter steps they are so any one of them can be
/// undone or retuned on its own.
/// </summary>
internal sealed class OldPhotoTool : EditTool<OldPhotoSettings>
{
    public OldPhotoTool()
        : base(new OldPhotoSettings())
    {
    }

    public override string Id => "old-photo";

    public override string Title => "Old photo";

    public override string Hint =>
        "Fade lifts the blacks the way a print loses its depth; grain and the "
        + "vignette do the rest.";

    public override EditRecipe Apply(EditRecipe recipe)
    {
        var steps = new List<FilterStep>(recipe.Filters);
        if (Settings.Sepia > 0)
        {
            steps.Add(new FilterStep(PhotoFilterKind.Sepia, Settings.Sepia, 0));
        }

        if (Settings.Fade > 0)
        {
            // A few percent of the negative blended in lifts the blacks and
            // pulls the whites while leaving middle grey alone, which is
            // exactly what age does to a print.
            steps.Add(new FilterStep(PhotoFilterKind.Invert, Settings.Fade * 0.5, 0));
        }

        if (Settings.Vignette != 0)
        {
            steps.Add(new FilterStep(PhotoFilterKind.Vignette, Settings.Vignette, 50, 60));
        }

        if (Settings.Grain > 0)
        {
            steps.Add(new FilterStep(PhotoFilterKind.AddNoise, Settings.Grain, 0));
        }

        return recipe with { Filters = steps };
    }

    protected override ToolPanelBuilder<OldPhotoSettings> Describe(
        ToolPanelBuilder<OldPhotoSettings> panel,
        EditToolContext context) =>
        panel
            .Slider("Sepia", 0, 100, s => s.Sepia, (s, v) => s with { Sepia = v })
            .Slider("Fade", 0, 100, s => s.Fade, (s, v) => s with { Fade = v })
            .Slider("Vignette", -100, 100, s => s.Vignette, (s, v) => s with { Vignette = v })
            .Slider("Grain", 0, 100, s => s.Grain, (s, v) => s with { Grain = v });
}

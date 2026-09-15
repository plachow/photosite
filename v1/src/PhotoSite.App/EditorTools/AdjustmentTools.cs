using System.Windows;
using System.Windows.Media.Imaging;
using PhotoSite.Controls;
using PhotoSite.Domain;
using PhotoSite.Services.Imaging;
using PhotoSite.ViewModels;

namespace PhotoSite.EditorTools;

/// <summary>
/// A tool over a slice of <see cref="PhotoAdjustments"/>. It opens on the
/// values the recipe holds and writes them back, so what the tool window
/// shows is exactly what the adjustment panel's sliders show afterwards -
/// nothing is stored twice and there is nothing to stack.
/// </summary>
internal abstract class AdjustmentTool<TSettings> : EditTool<TSettings>
    where TSettings : class
{
    protected AdjustmentTool(TSettings defaults)
        : base(defaults)
    {
    }

    public sealed override bool StartsFromRecipe => true;

    public sealed override EditRecipe Apply(EditRecipe recipe) =>
        recipe with { Adjustments = Write(recipe.Adjustments, Settings) };

    public sealed override void LoadFrom(EditRecipe recipe) =>
        Settings = Read(recipe.Adjustments);

    protected abstract TSettings Read(PhotoAdjustments adjustments);

    protected abstract PhotoAdjustments Write(
        PhotoAdjustments adjustments,
        TSettings settings);

    /// <summary>
    /// Auto buttons measure the untouched photograph, the same rule Auto Fix
    /// follows, so pressing one twice gives the same answer.
    /// </summary>
    protected static PhotoAdjustments Measure(EditToolContext context) =>
        AutoFixAnalyzer.Analyze(
            PixelBuffer.FromBitmap(context.Original),
            context.BaseRecipe.Adjustments);
}

internal sealed record ExposureSettings(
    double Exposure = 0,
    double Contrast = 0,
    double Highlights = 0,
    double Shadows = 0,
    double Whites = 0,
    double Blacks = 0,
    double Brightness = 0,
    double Clarity = 0);

internal sealed class ExposureTool : AdjustmentTool<ExposureSettings>
{
    public ExposureTool()
        : base(new ExposureSettings())
    {
    }

    public override string Id => "exposure";

    public override string Title => "Enhance exposure";

    public override string Hint =>
        "Exposure is in stops. Highlights and shadows work on the ends of "
        + "the tonal range only; whites and blacks set where those ends are.";

    protected override ExposureSettings Read(PhotoAdjustments adjustments) => new(
        adjustments.Exposure,
        adjustments.Contrast,
        adjustments.Highlights,
        adjustments.Shadows,
        adjustments.Whites,
        adjustments.Blacks,
        adjustments.Brightness,
        adjustments.Clarity);

    protected override PhotoAdjustments Write(
        PhotoAdjustments adjustments,
        ExposureSettings settings) =>
        adjustments with
        {
            Exposure = settings.Exposure,
            Contrast = settings.Contrast,
            Highlights = settings.Highlights,
            Shadows = settings.Shadows,
            Whites = settings.Whites,
            Blacks = settings.Blacks,
            Brightness = settings.Brightness,
            Clarity = settings.Clarity
        };

    protected override ToolPanelBuilder<ExposureSettings> Describe(
        ToolPanelBuilder<ExposureSettings> panel,
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
                        Whites = measured.Whites,
                        Blacks = measured.Blacks,
                        Brightness = measured.Brightness
                    };
                },
                "Measure the photograph and apply a conservative correction")
            .Slider("Exposure", -4, 4, s => s.Exposure, (s, v) => s with { Exposure = v }, "0.00")
            .Slider("Contrast", -100, 100, s => s.Contrast, (s, v) => s with { Contrast = v })
            .Slider("Highlights", -100, 100, s => s.Highlights, (s, v) => s with { Highlights = v })
            .Slider("Shadows", -100, 100, s => s.Shadows, (s, v) => s with { Shadows = v })
            .Slider("Whites", -100, 100, s => s.Whites, (s, v) => s with { Whites = v })
            .Slider("Blacks", -100, 100, s => s.Blacks, (s, v) => s with { Blacks = v })
            .Slider("Brightness", -100, 100, s => s.Brightness, (s, v) => s with { Brightness = v })
            .Slider("Clarity", -100, 100, s => s.Clarity, (s, v) => s with { Clarity = v });
}

internal sealed record ShadowsSettings(double Shadows = 0, double Highlights = 0);

/// <summary>Brighten shadows: the two recovery sliders on their own.</summary>
internal sealed class ShadowsTool : AdjustmentTool<ShadowsSettings>
{
    public ShadowsTool()
        : base(new ShadowsSettings())
    {
    }

    public override string Id => "shadows";

    public override string Title => "Brighten shadows";

    public override string Hint =>
        "Lifts the dark end without touching the midtones; pull highlights "
        + "down to keep a bright sky from washing out at the same time.";

    protected override ShadowsSettings Read(PhotoAdjustments adjustments) =>
        new(adjustments.Shadows, adjustments.Highlights);

    protected override PhotoAdjustments Write(
        PhotoAdjustments adjustments,
        ShadowsSettings settings) =>
        adjustments with
        {
            Shadows = settings.Shadows,
            Highlights = settings.Highlights
        };

    protected override ToolPanelBuilder<ShadowsSettings> Describe(
        ToolPanelBuilder<ShadowsSettings> panel,
        EditToolContext context) =>
        panel
            .Slider("Shadows", -100, 100, s => s.Shadows, (s, v) => s with { Shadows = v })
            .Slider("Highlights", -100, 100, s => s.Highlights, (s, v) => s with { Highlights = v });
}

internal sealed record ColorSettings(
    double Hue = 0,
    double Saturation = 0,
    double Vibrance = 0);

internal sealed class ColorsTool : AdjustmentTool<ColorSettings>
{
    public ColorsTool()
        : base(new ColorSettings())
    {
    }

    public override string Id => "colors";

    public override string Title => "Adjust colours";

    public override string Hint =>
        "Hue turns every colour round the wheel by the same angle. Vibrance "
        + "pushes the muted colours and spares the ones already saturated, "
        + "which is what keeps skin natural.";

    protected override ColorSettings Read(PhotoAdjustments adjustments) =>
        new(adjustments.Hue, adjustments.Saturation, adjustments.Vibrance);

    protected override PhotoAdjustments Write(
        PhotoAdjustments adjustments,
        ColorSettings settings) =>
        adjustments with
        {
            Hue = settings.Hue,
            Saturation = settings.Saturation,
            Vibrance = settings.Vibrance
        };

    protected override ToolPanelBuilder<ColorSettings> Describe(
        ToolPanelBuilder<ColorSettings> panel,
        EditToolContext context) =>
        panel
            .Slider("Hue", -180, 180, s => s.Hue, (s, v) => s with { Hue = v }, "0°")
            .Slider("Saturation", -100, 100, s => s.Saturation, (s, v) => s with { Saturation = v })
            .Slider("Vibrance", -100, 100, s => s.Vibrance, (s, v) => s with { Vibrance = v });
}

internal sealed record WhiteBalanceSettings(double Temperature = 0, double Tint = 0);

/// <summary>
/// Colour temperature with the two ways of finding it: an Auto that measures
/// the cast, and a click on the preview that makes the clicked patch grey.
/// </summary>
internal sealed class WhiteBalanceTool : AdjustmentTool<WhiteBalanceSettings>
{
    public WhiteBalanceTool()
        : base(new WhiteBalanceSettings())
    {
    }

    public override string Id => "white-balance";

    public override string Title => "Colour temperature";

    public override string Hint =>
        "Click something in the preview that should be neutral grey and the "
        + "temperature and tint are solved from it.";

    public override bool AcceptsPreviewPick => true;

    protected override WhiteBalanceSettings Read(PhotoAdjustments adjustments) =>
        new(adjustments.Temperature, adjustments.Tint);

    protected override PhotoAdjustments Write(
        PhotoAdjustments adjustments,
        WhiteBalanceSettings settings) =>
        adjustments with
        {
            Temperature = settings.Temperature,
            Tint = settings.Tint
        };

    private BitmapSource? neutralRender;

    public override void OnPreviewPicked(
        EditToolContext context,
        double x,
        double y,
        BitmapSource displayed)
    {
        // Sampled from the photograph with no white balance at all, so the
        // solved values are absolute and a second click on the same patch
        // lands on the same answer instead of chasing its own correction.
        neutralRender ??= context.Render(
            context.BaseRecipe with
            {
                Adjustments = Write(context.BaseRecipe.Adjustments, new WhiteBalanceSettings())
            });
        var (red, green, blue) = SamplePatch(neutralRender, x, y);
        var (temperature, tint) = AdjustmentsViewModel.SolveWhiteBalance(red, green, blue);
        Settings = new WhiteBalanceSettings(temperature, tint);
    }

    /// <summary>
    /// The mean of a small patch around the click; a single pixel of a JPEG
    /// is noise, not a measurement.
    /// </summary>
    internal static (double Red, double Green, double Blue) SamplePatch(
        BitmapSource bitmap,
        double x,
        double y)
    {
        var buffer = PixelBuffer.FromBitmap(bitmap);
        var centerX = (int)Math.Clamp(x * buffer.Width, 0, buffer.Width - 1);
        var centerY = (int)Math.Clamp(y * buffer.Height, 0, buffer.Height - 1);
        double red = 0, green = 0, blue = 0;
        var count = 0;
        for (var row = Math.Max(0, centerY - 3); row <= Math.Min(buffer.Height - 1, centerY + 3); row++)
        {
            for (var column = Math.Max(0, centerX - 3); column <= Math.Min(buffer.Width - 1, centerX + 3); column++)
            {
                var index = ((row * buffer.Width) + column) * PixelBuffer.BytesPerPixel;
                blue += buffer.Pixels[index];
                green += buffer.Pixels[index + 1];
                red += buffer.Pixels[index + 2];
                count++;
            }
        }

        return count == 0 ? (0, 0, 0) : (red / count, green / count, blue / count);
    }

    protected override ToolPanelBuilder<WhiteBalanceSettings> Describe(
        ToolPanelBuilder<WhiteBalanceSettings> panel,
        EditToolContext context) =>
        panel
            .Button(
                "Auto",
                () =>
                {
                    var measured = Measure(context);
                    Settings = new WhiteBalanceSettings(measured.Temperature, measured.Tint);
                },
                "Neutralize the measured colour cast")
            .Slider("Temperature", -100, 100, s => s.Temperature, (s, v) => s with { Temperature = v })
            .Slider("Tint", -100, 100, s => s.Tint, (s, v) => s with { Tint = v });
}

internal sealed record LevelsSettings(
    double BlackPoint = 0,
    double MidPoint = 1,
    double WhitePoint = 255);

/// <summary>
/// Levels over a histogram of the untouched photograph, with an Auto that
/// stretches to the outermost half-percent.
/// </summary>
internal sealed class LevelsTool : AdjustmentTool<LevelsSettings>
{
    public LevelsTool()
        : base(new LevelsSettings())
    {
    }

    public override string Id => "levels";

    public override string Title => "Levels";

    public override string Hint =>
        "Black and white point set which input values become pure black and "
        + "white; the midpoint bends what lies between.";

    protected override LevelsSettings Read(PhotoAdjustments adjustments) =>
        new(adjustments.BlackPoint, adjustments.MidPoint, adjustments.WhitePoint);

    protected override PhotoAdjustments Write(
        PhotoAdjustments adjustments,
        LevelsSettings settings) =>
        adjustments with
        {
            BlackPoint = Math.Clamp(settings.BlackPoint, 0, 254),
            MidPoint = settings.MidPoint,
            WhitePoint = Math.Clamp(settings.WhitePoint, 1, 255)
        };

    /// <summary>
    /// The levels that put the outermost half-percent of the luminance
    /// histogram at pure black and white.
    /// </summary>
    internal static LevelsSettings AutoLevels(HistogramData histogram)
    {
        if (histogram.Total == 0)
        {
            return new LevelsSettings();
        }

        var black = histogram.GetLuminancePercentile(0.005);
        var white = histogram.GetLuminancePercentile(0.995);
        return new LevelsSettings(
            Math.Clamp(black, 0, 254),
            1,
            Math.Clamp(Math.Max(white, black + 1), 1, 255));
    }

    protected override ToolPanelBuilder<LevelsSettings> Describe(
        ToolPanelBuilder<LevelsSettings> panel,
        EditToolContext context)
    {
        var histogram = new HistogramView
        {
            Height = 96,
            Margin = new Thickness(0, 0, 0, 8),
            ShowClipping = false
        };
        _ = histogram.UpdateAsync(context.Original, CancellationToken.None);

        return panel
            .Element(histogram)
            .Button(
                "Auto",
                () => Settings = AutoLevels(HistogramData.FromBitmap(context.Original)),
                "Stretch the histogram to the full range")
            .Slider("Black point", 0, 254, s => s.BlackPoint, (s, v) => s with { BlackPoint = v })
            .Slider("Midpoint", 0.2, 3, s => s.MidPoint, (s, v) => s with { MidPoint = v }, "0.00")
            .Slider("White point", 1, 255, s => s.WhitePoint, (s, v) => s with { WhitePoint = v });
    }
}

internal sealed record NoiseReductionSettings(double Luminance = 0, double Color = 0);

internal sealed class NoiseReductionTool : AdjustmentTool<NoiseReductionSettings>
{
    public NoiseReductionTool()
        : base(new NoiseReductionSettings())
    {
    }

    public override string Id => "noise-reduction";

    public override string Title => "Noise reduction";

    public override string Hint =>
        "Colour noise tolerates a strong setting; luminance grain is smoothed "
        + "only where the neighbourhood is flat, so push it slowly or the "
        + "photograph turns to plastic.";

    protected override NoiseReductionSettings Read(PhotoAdjustments adjustments) =>
        new(adjustments.LuminanceNoiseReduction, adjustments.ColorNoiseReduction);

    protected override PhotoAdjustments Write(
        PhotoAdjustments adjustments,
        NoiseReductionSettings settings) =>
        adjustments with
        {
            LuminanceNoiseReduction = settings.Luminance,
            ColorNoiseReduction = settings.Color
        };

    protected override ToolPanelBuilder<NoiseReductionSettings> Describe(
        ToolPanelBuilder<NoiseReductionSettings> panel,
        EditToolContext context) =>
        panel
            .Slider("Luminance", 0, 100, s => s.Luminance, (s, v) => s with { Luminance = v })
            .Slider("Colour", 0, 100, s => s.Color, (s, v) => s with { Color = v });
}

internal sealed record ChromaticAberrationSettings(double Amount = 0);

internal sealed class ChromaticAberrationTool : AdjustmentTool<ChromaticAberrationSettings>
{
    public ChromaticAberrationTool()
        : base(new ChromaticAberrationSettings())
    {
    }

    public override string Id => "chromatic-aberration";

    public override string Title => "Chromatic aberration";

    public override string Hint =>
        "Rescales the red and blue planes against green to pull colour "
        + "fringes back onto the edge. Judge it on a high-contrast corner.";

    protected override ChromaticAberrationSettings Read(PhotoAdjustments adjustments) =>
        new(adjustments.ChromaticAberration);

    protected override PhotoAdjustments Write(
        PhotoAdjustments adjustments,
        ChromaticAberrationSettings settings) =>
        adjustments with { ChromaticAberration = settings.Amount };

    protected override ToolPanelBuilder<ChromaticAberrationSettings> Describe(
        ToolPanelBuilder<ChromaticAberrationSettings> panel,
        EditToolContext context) =>
        panel.Slider("Red / cyan", -100, 100, s => s.Amount, (s, v) => s with { Amount = v });
}

internal sealed record DistortionSettings(double Distortion = 0);

internal sealed class DistortionTool : AdjustmentTool<DistortionSettings>
{
    public DistortionTool()
        : base(new DistortionSettings())
    {
    }

    public override string Id => "distortion";

    public override string Title => "Lens distortion";

    public override string Hint =>
        "Positive removes barrel distortion (lines bowing outward), negative "
        + "removes pincushion. The frame is scaled up just enough that no "
        + "empty corner is left.";

    public override bool OffersGrid => true;

    protected override DistortionSettings Read(PhotoAdjustments adjustments) =>
        new(adjustments.LensDistortion);

    protected override PhotoAdjustments Write(
        PhotoAdjustments adjustments,
        DistortionSettings settings) =>
        adjustments with { LensDistortion = settings.Distortion };

    protected override ToolPanelBuilder<DistortionSettings> Describe(
        ToolPanelBuilder<DistortionSettings> panel,
        EditToolContext context) =>
        panel.Slider("Correction", -100, 100, s => s.Distortion, (s, v) => s with { Distortion = v });
}

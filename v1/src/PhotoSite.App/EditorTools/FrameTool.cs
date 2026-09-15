using PhotoSite.Domain;

namespace PhotoSite.EditorTools;

internal sealed record FrameSettings(
    double Thickness = 3,
    int ColorIndex = 0,
    double LineThickness = 0,
    int LineColorIndex = 1);

/// <summary>
/// Borders and frames: a band of colour around the finished image with an
/// optional thin line just inside it. Sizes are percent of the shorter side,
/// so a preset made on one photograph looks the same on the next.
/// </summary>
internal sealed class FrameTool : EditTool<FrameSettings>
{
    /// <summary>The palette both the band and the line choose from.</summary>
    internal static readonly (string Name, uint Argb)[] Palette =
    [
        ("White", 0xFFFFFFFF),
        ("Black", 0xFF000000),
        ("Charcoal", 0xFF2B2E36),
        ("Light grey", 0xFFD9DCE2),
        ("Cream", 0xFFF4EBD8),
        ("Warm grey", 0xFF8C8680),
        ("Navy", 0xFF1E2A44),
        ("Burgundy", 0xFF5B1F2B)
    ];

    private static readonly string[] PaletteNames =
        Palette.Select(entry => entry.Name).ToArray();

    public FrameTool()
        : base(new FrameSettings())
    {
    }

    public override string Id => "frame";

    public override string Title => "Borders and frames";

    public override string Hint =>
        "The frame is added outside the photograph on save and export, so "
        + "the output grows by twice the band. Layers stay on the picture, "
        + "not on the frame.";

    public override bool StartsFromRecipe => true;

    public override EditRecipe Apply(EditRecipe recipe) =>
        recipe with { Frame = ToFrame(Settings) };

    public override void LoadFrom(EditRecipe recipe) =>
        Settings = recipe.Frame is { } frame ? FromFrame(frame) : Defaults;

    internal static PhotoFrame? ToFrame(FrameSettings settings)
    {
        var thickness = Math.Clamp(settings.Thickness, 0, 50) / 100;
        var line = Math.Clamp(settings.LineThickness, 0, 10) / 100;
        if (thickness <= 0 && line <= 0)
        {
            return null;
        }

        return new PhotoFrame(
            thickness,
            Palette[Math.Clamp(settings.ColorIndex, 0, Palette.Length - 1)].Argb,
            line,
            Palette[Math.Clamp(settings.LineColorIndex, 0, Palette.Length - 1)].Argb);
    }

    internal static FrameSettings FromFrame(PhotoFrame frame) => new(
        Math.Round(frame.Thickness * 100, 1),
        IndexOf(frame.Color, 0),
        Math.Round(frame.LineThickness * 100, 2),
        IndexOf(frame.LineColor, 1));

    private static int IndexOf(uint argb, int fallback)
    {
        for (var index = 0; index < Palette.Length; index++)
        {
            if (Palette[index].Argb == argb)
            {
                return index;
            }
        }

        return fallback;
    }

    protected override ToolPanelBuilder<FrameSettings> Describe(
        ToolPanelBuilder<FrameSettings> panel,
        EditToolContext context) =>
        panel
            .Header("Band")
            .Slider("Width", 0, 25, s => s.Thickness, (s, v) => s with { Thickness = v }, "0.0 '%'")
            .Choice("Colour", PaletteNames, s => s.ColorIndex, (s, v) => s with { ColorIndex = v })
            .Header("Inner line")
            .Slider("Width", 0, 3, s => s.LineThickness, (s, v) => s with { LineThickness = v }, "0.00 '%'")
            .Choice("Colour", PaletteNames, s => s.LineColorIndex, (s, v) => s with { LineColorIndex = v });
}

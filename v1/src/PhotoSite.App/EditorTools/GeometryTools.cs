using System.Globalization;
using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using PhotoSite.Domain;
using PhotoSite.Services.Imaging;

namespace PhotoSite.EditorTools;

internal sealed record StraightenSettings(
    double Angle = 0,
    double Vertical = 0,
    double Horizontal = 0);

/// <summary>
/// Straightening and keystone correction with a grid over the preview to
/// line a horizon or a wall up against.
/// </summary>
internal sealed class StraightenTool : EditTool<StraightenSettings>
{
    public StraightenTool()
        : base(new StraightenSettings())
    {
    }

    public override string Id => "straighten";

    public override string Title => "Straighten and perspective";

    public override string Hint =>
        "The frame is scaled up just enough that no empty corner is left "
        + "behind, so a strong correction costs some of the edge.";

    public override bool StartsFromRecipe => true;

    public override bool OffersGrid => true;

    public override EditRecipe Apply(EditRecipe recipe) =>
        recipe with
        {
            StraightenAngle = Settings.Angle,
            PerspectiveVertical = Settings.Vertical,
            PerspectiveHorizontal = Settings.Horizontal
        };

    public override void LoadFrom(EditRecipe recipe) =>
        Settings = new StraightenSettings(
            recipe.StraightenAngle,
            recipe.PerspectiveVertical,
            recipe.PerspectiveHorizontal);

    protected override ToolPanelBuilder<StraightenSettings> Describe(
        ToolPanelBuilder<StraightenSettings> panel,
        EditToolContext context) =>
        panel
            .Slider("Angle", -45, 45, s => s.Angle, (s, v) => s with { Angle = v }, "0.0°")
            .Header("Perspective")
            .Slider("Vertical", -100, 100, s => s.Vertical, (s, v) => s with { Vertical = v })
            .Slider("Horizontal", -100, 100, s => s.Horizontal, (s, v) => s with { Horizontal = v });
}

internal sealed record ResizeSettings(
    int Width = 0,
    int Height = 0,
    bool KeepAspect = true);

/// <summary>
/// The output size of the recipe. Nothing is resampled until a save or an
/// export - the canvas keeps showing the full photograph - so the tool is
/// a statement of intent the info line and the exporter honour.
/// </summary>
internal sealed class ResizeTool : EditTool<ResizeSettings>
{
    private static readonly (string Label, int Size)[] LongestSidePresets =
    [
        ("HD 1920", 1920),
        ("4K 3840", 3840),
        ("Web 1600", 1600),
        ("Email 1024", 1024)
    ];

    public ResizeTool()
        : base(new ResizeSettings())
    {
    }

    public override string Id => "resize";

    public override string Title => "Resize";

    public override string Hint =>
        "Sizes are in pixels of the finished, cropped image; 0 keeps the "
        + "native size. Applied on save and export, never to the file you "
        + "opened.";

    public override bool StartsFromRecipe => true;

    public override EditRecipe Apply(EditRecipe recipe) =>
        recipe with
        {
            OutputWidth = Math.Max(0, Settings.Width),
            OutputHeight = Math.Max(0, Settings.Height)
        };

    public override void LoadFrom(EditRecipe recipe) =>
        Settings = Settings with
        {
            Width = recipe.OutputWidth,
            Height = recipe.OutputHeight
        };

    protected override ToolPanelBuilder<ResizeSettings> Describe(
        ToolPanelBuilder<ResizeSettings> panel,
        EditToolContext context)
    {
        var (frameWidth, frameHeight) = ImageRenderer.MeasureFrame(
            context.FullWidth,
            context.FullHeight,
            context.BaseRecipe);
        var aspect = frameHeight <= 0 ? 1 : frameWidth / (double)frameHeight;

        panel.Note($"Native size {frameWidth} × {frameHeight} px");
        AddNumberRow(panel, "Width", s => s.Width, (s, value) =>
            s.KeepAspect && value > 0
                ? s with { Width = value, Height = Math.Max(1, (int)Math.Round(value / aspect)) }
                : s with { Width = value });
        AddNumberRow(panel, "Height", s => s.Height, (s, value) =>
            s.KeepAspect && value > 0
                ? s with { Height = value, Width = Math.Max(1, (int)Math.Round(value * aspect)) }
                : s with { Height = value });
        panel.Toggle(
            "Keep aspect ratio",
            s => s.KeepAspect,
            (s, on) => s with { KeepAspect = on });

        panel.Header("Longest side");
        var presets = new WrapPanel();
        foreach (var (label, size) in LongestSidePresets)
        {
            var button = new Button
            {
                Content = label,
                Margin = new Thickness(0, 0, 4, 4),
                Padding = new Thickness(8, 3, 8, 3)
            };
            button.Click += (_, _) => Settings = aspect >= 1
                ? new ResizeSettings(size, Math.Max(1, (int)Math.Round(size / aspect)), Settings.KeepAspect)
                : new ResizeSettings(Math.Max(1, (int)Math.Round(size * aspect)), size, Settings.KeepAspect);
            presets.Children.Add(button);
        }

        var native = new Button
        {
            Content = "Native",
            Margin = new Thickness(0, 0, 4, 4),
            Padding = new Thickness(8, 3, 8, 3)
        };
        native.Click += (_, _) => Settings = Settings with { Width = 0, Height = 0 };
        presets.Children.Add(native);
        return panel.Element(presets);
    }

    private void AddNumberRow(
        ToolPanelBuilder<ResizeSettings> panel,
        string label,
        Func<ResizeSettings, int> get,
        Func<ResizeSettings, int, ResizeSettings> set)
    {
        var row = new Grid { Margin = new Thickness(0, 4, 0, 2) };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(96) });
        row.ColumnDefinitions.Add(new ColumnDefinition());
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.Children.Add(new TextBlock
        {
            Text = label,
            VerticalAlignment = VerticalAlignment.Center,
            Style = (Style)Application.Current.FindResource("FieldLabelStyle")
        });
        var box = new TextBox
        {
            Text = get(Settings).ToString(CultureInfo.InvariantCulture),
            Padding = new Thickness(6, 3, 6, 3),
            VerticalContentAlignment = VerticalAlignment.Center
        };
        AutomationProperties.SetName(box, label);
        Grid.SetColumn(box, 1);
        row.Children.Add(box);
        var unit = new TextBlock
        {
            Text = "px",
            Margin = new Thickness(6, 0, 0, 0),
            VerticalAlignment = VerticalAlignment.Center,
            Style = (Style)Application.Current.FindResource("FieldLabelStyle")
        };
        Grid.SetColumn(unit, 2);
        row.Children.Add(unit);

        void Commit()
        {
            if (panel.IsRefreshing)
            {
                return;
            }

            if (int.TryParse(box.Text.Trim(), NumberStyles.Integer, CultureInfo.InvariantCulture, out var value)
                && value >= 0
                && value != get(Settings))
            {
                Settings = set(Settings, Math.Min(value, 30000));
            }
        }

        box.LostKeyboardFocus += (_, _) => Commit();
        box.KeyDown += (_, eventArgs) =>
        {
            if (eventArgs.Key == System.Windows.Input.Key.Enter)
            {
                Commit();
                eventArgs.Handled = true;
            }
        };
        panel.Element(
            row,
            () => box.Text = get(Settings).ToString(CultureInfo.InvariantCulture));
    }
}

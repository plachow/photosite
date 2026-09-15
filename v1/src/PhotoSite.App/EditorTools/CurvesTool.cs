using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using System.Windows.Media;
using PhotoSite.Controls;
using PhotoSite.Domain;
using PhotoSite.Services.Imaging;

namespace PhotoSite.EditorTools;

internal sealed record CurvesSettings
{
    public ToneCurve Master { get; init; } = ToneCurve.Linear;

    public ToneCurve Red { get; init; } = ToneCurve.Linear;

    public ToneCurve Green { get; init; } = ToneCurve.Linear;

    public ToneCurve Blue { get; init; } = ToneCurve.Linear;
}

/// <summary>
/// Tone curves, master and per channel, on a curve editor with the
/// photograph's histogram behind it. The four curves already lived in the
/// recipe; this is the first window that lets them be drawn.
/// </summary>
internal sealed class CurvesTool : AdjustmentTool<CurvesSettings>
{
    private static readonly string[] ChannelNames = ["RGB", "Red", "Green", "Blue"];

    private static readonly Brush[] ChannelBrushes =
    [
        Brushes.White,
        new SolidColorBrush(Color.FromRgb(0xFF, 0x6B, 0x6B)),
        new SolidColorBrush(Color.FromRgb(0x7A, 0xE0, 0x7A)),
        new SolidColorBrush(Color.FromRgb(0x7A, 0xB4, 0xFF))
    ];

    private int channel;

    public CurvesTool()
        : base(new CurvesSettings())
    {
    }

    public override string Id => "curves";

    public override string Title => "Curves";

    public override string Hint =>
        "Click the curve to add a point, drag to shape it, drag a point far "
        + "outside the square or right-click it to remove it. The curve is "
        + "applied after levels and before the colour channels.";

    protected override CurvesSettings Read(PhotoAdjustments adjustments) => new()
    {
        Master = adjustments.Curve,
        Red = adjustments.RedCurve,
        Green = adjustments.GreenCurve,
        Blue = adjustments.BlueCurve
    };

    protected override PhotoAdjustments Write(
        PhotoAdjustments adjustments,
        CurvesSettings settings) =>
        adjustments with
        {
            Curve = settings.Master,
            RedCurve = settings.Red,
            GreenCurve = settings.Green,
            BlueCurve = settings.Blue
        };

    private ToneCurve CurrentCurve => channel switch
    {
        1 => Settings.Red,
        2 => Settings.Green,
        3 => Settings.Blue,
        _ => Settings.Master
    };

    private CurvesSettings WithCurrentCurve(ToneCurve curve) => channel switch
    {
        1 => Settings with { Red = curve },
        2 => Settings with { Green = curve },
        3 => Settings with { Blue = curve },
        _ => Settings with { Master = curve }
    };

    protected override ToolPanelBuilder<CurvesSettings> Describe(
        ToolPanelBuilder<CurvesSettings> panel,
        EditToolContext context)
    {
        var histogram = HistogramData.FromBitmap(context.Original);
        var editor = new CurveEditor
        {
            Width = 256,
            Height = 256,
            HorizontalAlignment = HorizontalAlignment.Left,
            Curve = CurrentCurve,
            Histogram = histogram.Luminance
        };
        var editing = false;
        editor.CurveChanged += (_, curve) =>
        {
            editing = true;
            try
            {
                Settings = WithCurrentCurve(curve);
            }
            finally
            {
                editing = false;
            }
        };

        var channelRow = new Grid { Margin = new Thickness(0, 0, 0, 8) };
        channelRow.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(96) });
        channelRow.ColumnDefinitions.Add(new ColumnDefinition());
        channelRow.Children.Add(new TextBlock
        {
            Text = "Channel",
            VerticalAlignment = VerticalAlignment.Center,
            Style = (Style)Application.Current.FindResource("FieldLabelStyle")
        });
        var channelBox = new ComboBox
        {
            ItemsSource = ChannelNames,
            SelectedIndex = channel,
            Height = 26,
            Padding = new Thickness(6, 0, 6, 0)
        };
        AutomationProperties.SetName(channelBox, "Curve channel");
        Grid.SetColumn(channelBox, 1);
        channelRow.Children.Add(channelBox);
        channelBox.SelectionChanged += (_, _) =>
        {
            channel = Math.Max(0, channelBox.SelectedIndex);
            editor.Curve = CurrentCurve;
            editor.CurveBrush = ChannelBrushes[channel];
            editor.Histogram = channel switch
            {
                1 => histogram.Red,
                2 => histogram.Green,
                3 => histogram.Blue,
                _ => histogram.Luminance
            };
        };

        return panel
            .Element(channelRow)
            .Element(
                editor,
                () =>
                {
                    if (!editing)
                    {
                        editor.Curve = CurrentCurve;
                    }
                })
            .Button(
                "Reset channel",
                () => Settings = WithCurrentCurve(ToneCurve.Linear),
                "Straighten the curve of the selected channel");
    }
}

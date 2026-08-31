using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using PhotoSite.Services.Imaging;

namespace PhotoSite.Controls;

public enum HistogramChannel
{
    Rgb,
    Red,
    Green,
    Blue,
    Luminance
}

/// <summary>
/// Draws the distribution of the frame currently on the editor canvas, with
/// optional clipping warnings at both ends.
/// </summary>
internal sealed class HistogramView : FrameworkElement
{
    private static readonly Brush BackgroundBrush =
        Freeze(new SolidColorBrush(Color.FromRgb(0x0C, 0x0E, 0x12)));
    private static readonly Brush GridBrush =
        Freeze(new SolidColorBrush(Color.FromArgb(0x38, 0xC8, 0xCD, 0xD8)));
    private static readonly Brush LuminanceBrush =
        Freeze(new SolidColorBrush(Color.FromArgb(0xCC, 0xD9, 0xE0, 0xEA)));
    private static readonly Brush ShadowWarningBrush =
        Freeze(new SolidColorBrush(Color.FromArgb(0xB0, 0x67, 0xB7, 0xFF)));
    private static readonly Brush HighlightWarningBrush =
        Freeze(new SolidColorBrush(Color.FromArgb(0xB0, 0xFF, 0x8A, 0x6A)));

    // Additive so overlapping channels read as the neutral they combine into,
    // which is how a histogram is expected to look.
    private static readonly Brush[] ChannelBrushes =
    [
        Freeze(new SolidColorBrush(Color.FromArgb(0x9A, 0xFF, 0x5A, 0x4E))),
        Freeze(new SolidColorBrush(Color.FromArgb(0x9A, 0x62, 0xD9, 0x62))),
        Freeze(new SolidColorBrush(Color.FromArgb(0x9A, 0x5A, 0x9A, 0xFF)))
    ];

    public static readonly DependencyProperty ChannelProperty =
        DependencyProperty.Register(
            nameof(Channel),
            typeof(HistogramChannel),
            typeof(HistogramView),
            new FrameworkPropertyMetadata(
                HistogramChannel.Rgb,
                FrameworkPropertyMetadataOptions.AffectsRender));

    public static readonly DependencyProperty ShowClippingProperty =
        DependencyProperty.Register(
            nameof(ShowClipping),
            typeof(bool),
            typeof(HistogramView),
            new FrameworkPropertyMetadata(
                true,
                FrameworkPropertyMetadataOptions.AffectsRender));

    private HistogramData histogram = HistogramData.Empty;

    public HistogramView()
    {
        MinHeight = 92;
    }

    public HistogramChannel Channel
    {
        get => (HistogramChannel)GetValue(ChannelProperty);
        set => SetValue(ChannelProperty, value);
    }

    public bool ShowClipping
    {
        get => (bool)GetValue(ShowClippingProperty);
        set => SetValue(ShowClippingProperty, value);
    }

    public HistogramData Data => histogram;

    /// <summary>
    /// Measures a frame off the dispatcher. Called on every preview render,
    /// so it must never block the canvas.
    /// </summary>
    public async Task UpdateAsync(
        BitmapSource? source,
        CancellationToken cancellationToken = default)
    {
        if (source is null)
        {
            histogram = HistogramData.Empty;
            InvalidateVisual();
            return;
        }

        try
        {
            // Marshalled explicitly rather than relying on the captured
            // context: this can be called before the dispatcher starts
            // pumping, when there is no synchronization context to capture
            // and the continuation would land on a thread-pool thread.
            var computed = await Task.Run(
                    () => HistogramData.FromBitmap(source),
                    cancellationToken)
                .ConfigureAwait(false);
            if (cancellationToken.IsCancellationRequested)
            {
                return;
            }

            await Dispatcher.InvokeAsync(() =>
            {
                histogram = computed;
                InvalidateVisual();
            });
        }
        catch (OperationCanceledException)
        {
        }
    }

    protected override void OnRender(DrawingContext drawingContext)
    {
        var bounds = new Rect(RenderSize);
        drawingContext.DrawRoundedRectangle(
            BackgroundBrush,
            null,
            bounds,
            3,
            3);

        if (bounds.Width <= 2 || bounds.Height <= 2)
        {
            return;
        }

        for (var division = 1; division < 4; division++)
        {
            var x = bounds.Width * division / 4;
            drawingContext.DrawRectangle(
                GridBrush,
                null,
                new Rect(x, 0, 1, bounds.Height));
        }

        if (histogram.Total == 0)
        {
            return;
        }

        switch (Channel)
        {
            case HistogramChannel.Rgb:
                DrawChannel(drawingContext, histogram.Red, ChannelBrushes[0], bounds);
                DrawChannel(drawingContext, histogram.Green, ChannelBrushes[1], bounds);
                DrawChannel(drawingContext, histogram.Blue, ChannelBrushes[2], bounds);
                break;
            case HistogramChannel.Red:
                DrawChannel(drawingContext, histogram.Red, ChannelBrushes[0], bounds);
                break;
            case HistogramChannel.Green:
                DrawChannel(drawingContext, histogram.Green, ChannelBrushes[1], bounds);
                break;
            case HistogramChannel.Blue:
                DrawChannel(drawingContext, histogram.Blue, ChannelBrushes[2], bounds);
                break;
            default:
                DrawChannel(
                    drawingContext,
                    histogram.Luminance,
                    LuminanceBrush,
                    bounds);
                break;
        }

        if (!ShowClipping)
        {
            return;
        }

        // A thin bar at whichever end is losing detail; the threshold is set
        // so ordinary specular highlights do not light it up.
        if (histogram.ShadowClippedFraction > 0.001)
        {
            drawingContext.DrawRectangle(
                ShadowWarningBrush,
                null,
                new Rect(0, 0, 3, bounds.Height));
        }

        if (histogram.HighlightClippedFraction > 0.001)
        {
            drawingContext.DrawRectangle(
                HighlightWarningBrush,
                null,
                new Rect(bounds.Width - 3, 0, 3, bounds.Height));
        }
    }

    private void DrawChannel(
        DrawingContext drawingContext,
        int[] bins,
        Brush brush,
        Rect bounds)
    {
        // The extremes routinely tower over everything else (a clipped sky is
        // one enormous bin), so the peak is taken from the interior and a
        // square-root scale keeps small but real detail visible.
        var peak = Math.Max(1, histogram.Peak);
        var geometry = new StreamGeometry();
        using (var context = geometry.Open())
        {
            context.BeginFigure(
                new Point(0, bounds.Height),
                isFilled: true,
                isClosed: true);
            for (var bin = 0; bin < bins.Length; bin++)
            {
                var x = bounds.Width * bin / (bins.Length - 1);
                var normalized = Math.Sqrt(Math.Min(1, bins[bin] / (double)peak));
                var y = bounds.Height - (normalized * (bounds.Height - 2));
                context.LineTo(new Point(x, y), isStroked: false, isSmoothJoin: false);
            }

            context.LineTo(
                new Point(bounds.Width, bounds.Height),
                isStroked: false,
                isSmoothJoin: false);
        }

        geometry.Freeze();
        drawingContext.DrawGeometry(brush, null, geometry);
    }

    private static Brush Freeze(SolidColorBrush brush)
    {
        brush.Freeze();
        return brush;
    }
}

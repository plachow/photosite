using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using PhotoSite.Domain;

namespace PhotoSite.Services.Imaging;

/// <summary>
/// Adds the recipe's frame around a finished image. The same numbers drive
/// the canvas, which paints the band under the photograph rather than
/// re-rendering, so the two agree to the pixel at every zoom.
/// </summary>
internal static class FrameRenderer
{
    public static BitmapSource Compose(BitmapSource image, PhotoFrame frame)
    {
        var band = frame.MeasureBand(image.PixelWidth, image.PixelHeight);
        var line = frame.MeasureLine(image.PixelWidth, image.PixelHeight);
        if (band <= 0 && line <= 0)
        {
            return image;
        }

        var width = image.PixelWidth + (2 * band);
        var height = image.PixelHeight + (2 * band);
        var visual = new DrawingVisual();
        RenderOptions.SetBitmapScalingMode(visual, BitmapScalingMode.HighQuality);
        using (var drawingContext = visual.RenderOpen())
        {
            drawingContext.DrawRectangle(
                CreateBrush(frame.Color),
                null,
                new Rect(0, 0, width, height));
            drawingContext.DrawImage(
                image,
                new Rect(band, band, image.PixelWidth, image.PixelHeight));
            DrawLine(drawingContext, frame, line, new Rect(band, band, image.PixelWidth, image.PixelHeight));
        }

        var rendered = new RenderTargetBitmap(width, height, 96, 96, PixelFormats.Pbgra32);
        rendered.Render(visual);
        rendered.Freeze();
        return rendered;
    }

    /// <summary>
    /// The thin line sits just inside the photograph's edge, over its
    /// outermost pixels, so a zero band still shows a line.
    /// </summary>
    public static void DrawLine(
        DrawingContext drawingContext,
        PhotoFrame frame,
        double lineThickness,
        Rect image)
    {
        if (lineThickness <= 0)
        {
            return;
        }

        var pen = new Pen(CreateBrush(frame.LineColor), lineThickness);
        var half = lineThickness / 2;
        drawingContext.DrawRectangle(
            null,
            pen,
            new Rect(
                image.Left + half,
                image.Top + half,
                Math.Max(0, image.Width - lineThickness),
                Math.Max(0, image.Height - lineThickness)));
    }

    public static SolidColorBrush CreateBrush(uint argb)
    {
        var brush = new SolidColorBrush(
            Color.FromArgb(
                (byte)(argb >> 24),
                (byte)(argb >> 16),
                (byte)(argb >> 8),
                (byte)argb));
        brush.Freeze();
        return brush;
    }
}

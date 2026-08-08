using System.Globalization;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using PhotoSite.Domain;

namespace PhotoSite.Services.Imaging;

/// <summary>
/// Draws annotation layers. The same code paints the live editor overlay and
/// the exported file, so what the user arranges on screen is exactly what
/// lands in the JPEG - just at a different pixel size.
/// </summary>
internal static class LayerRenderer
{
    private const double ArrowHeadLengthFactor = 4.5;
    private const double ArrowHeadWidthFactor = 3.0;

    public static void Draw(
        DrawingContext drawingContext,
        IEnumerable<AnnotationLayer> layers,
        Size imageSize)
    {
        foreach (var layer in layers)
        {
            if (!layer.IsVisible || layer.Opacity <= 0)
            {
                continue;
            }

            drawingContext.PushOpacity(Math.Clamp(layer.Opacity, 0, 1));
            try
            {
                switch (layer)
                {
                    case ShapeLayer shape:
                        DrawShape(drawingContext, shape, imageSize);
                        break;
                    case TextLayer text:
                        DrawText(drawingContext, text, imageSize);
                        break;
                    case FreehandLayer freehand:
                        DrawFreehand(drawingContext, freehand, imageSize);
                        break;
                }
            }
            finally
            {
                drawingContext.Pop();
            }
        }
    }

    public static BitmapSource Compose(
        BitmapSource image,
        IReadOnlyList<AnnotationLayer> layers)
    {
        if (layers.Count == 0)
        {
            return image;
        }

        var size = new Size(image.PixelWidth, image.PixelHeight);
        var visual = new DrawingVisual();
        System.Windows.Media.RenderOptions.SetBitmapScalingMode(
            visual,
            BitmapScalingMode.HighQuality);
        using (var drawingContext = visual.RenderOpen())
        {
            drawingContext.DrawImage(image, new Rect(size));
            Draw(drawingContext, layers, size);
        }

        var rendered = new RenderTargetBitmap(
            image.PixelWidth,
            image.PixelHeight,
            96,
            96,
            PixelFormats.Pbgra32);
        rendered.Render(visual);
        rendered.Freeze();
        return rendered;
    }

    public static Size MeasureText(TextLayer layer, Size imageSize)
    {
        var formatted = CreateFormattedText(layer, imageSize);
        return new Size(
            imageSize.Width <= 0 ? 0 : formatted.Width / imageSize.Width,
            imageSize.Height <= 0 ? 0 : formatted.Height / imageSize.Height);
    }

    private static void DrawShape(
        DrawingContext drawingContext,
        ShapeLayer layer,
        Size imageSize)
    {
        var start = ToPixels(layer.X1, layer.Y1, imageSize);
        var end = ToPixels(layer.X2, layer.Y2, imageSize);
        var pen = CreatePen(layer, imageSize);
        var fill = CreateBrush(layer.FillColor);

        switch (layer.Shape)
        {
            case ShapeKind.Rectangle:
                drawingContext.DrawRectangle(
                    fill,
                    pen,
                    new Rect(start, end));
                break;
            case ShapeKind.Ellipse:
                var bounds = new Rect(start, end);
                drawingContext.DrawEllipse(
                    fill,
                    pen,
                    new Point(
                        bounds.Left + (bounds.Width / 2),
                        bounds.Top + (bounds.Height / 2)),
                    bounds.Width / 2,
                    bounds.Height / 2);
                break;
            case ShapeKind.Line:
                drawingContext.DrawLine(pen, start, end);
                break;
            case ShapeKind.Arrow:
                DrawArrow(drawingContext, layer, start, end, pen, imageSize);
                break;
        }
    }

    private static void DrawArrow(
        DrawingContext drawingContext,
        ShapeLayer layer,
        Point start,
        Point end,
        Pen pen,
        Size imageSize)
    {
        var direction = end - start;
        var length = direction.Length;
        if (length < 0.5)
        {
            return;
        }

        direction /= length;
        var strokePixels = GetStrokePixels(layer, imageSize);
        var headLength = Math.Min(
            length,
            strokePixels * ArrowHeadLengthFactor);
        var headWidth = strokePixels * ArrowHeadWidthFactor;
        var normal = new Vector(-direction.Y, direction.X);
        var basePoint = end - (direction * headLength);

        // Stop the shaft where the head begins so a thick stroke does not
        // poke through the tip of the arrow.
        drawingContext.DrawLine(
            pen,
            start,
            basePoint + (direction * (headLength * 0.35)));

        var head = new StreamGeometry();
        using (var geometryContext = head.Open())
        {
            geometryContext.BeginFigure(end, isFilled: true, isClosed: true);
            geometryContext.LineTo(
                basePoint + (normal * (headWidth / 2)),
                isStroked: false,
                isSmoothJoin: false);
            geometryContext.LineTo(
                basePoint - (normal * (headWidth / 2)),
                isStroked: false,
                isSmoothJoin: false);
        }

        head.Freeze();
        drawingContext.DrawGeometry(CreateBrush(layer.StrokeColor), null, head);
    }

    private static void DrawText(
        DrawingContext drawingContext,
        TextLayer layer,
        Size imageSize)
    {
        var formatted = CreateFormattedText(layer, imageSize);
        var origin = ToPixels(layer.X, layer.Y, imageSize);
        if ((layer.BackgroundColor >> 24) > 0)
        {
            var padding = formatted.Height * 0.14;
            drawingContext.DrawRectangle(
                CreateBrush(layer.BackgroundColor),
                null,
                new Rect(
                    origin.X - padding,
                    origin.Y - (padding / 2),
                    formatted.Width + (padding * 2),
                    formatted.Height + padding));
        }

        drawingContext.DrawText(formatted, origin);
    }

    private static void DrawFreehand(
        DrawingContext drawingContext,
        FreehandLayer layer,
        Size imageSize)
    {
        if (layer.Points.Count < 2)
        {
            return;
        }

        var geometry = new StreamGeometry();
        using (var geometryContext = geometry.Open())
        {
            var first = ToPixels(layer.Points[0].X, layer.Points[0].Y, imageSize);
            geometryContext.BeginFigure(first, isFilled: false, isClosed: false);
            for (var index = 1; index < layer.Points.Count; index++)
            {
                geometryContext.LineTo(
                    ToPixels(
                        layer.Points[index].X,
                        layer.Points[index].Y,
                        imageSize),
                    isStroked: true,
                    isSmoothJoin: true);
            }
        }

        geometry.Freeze();
        drawingContext.DrawGeometry(null, CreatePen(layer, imageSize), geometry);
    }

    private static FormattedText CreateFormattedText(
        TextLayer layer,
        Size imageSize)
    {
        var typeface = new Typeface(
            new FontFamily(
                string.IsNullOrWhiteSpace(layer.FontFamily)
                    ? "Segoe UI"
                    : layer.FontFamily),
            layer.Italic ? FontStyles.Italic : FontStyles.Normal,
            layer.Bold ? FontWeights.Bold : FontWeights.Normal,
            FontStretches.Normal);
        var fontSize = Math.Max(
            1,
            layer.FontSize * Math.Max(1, imageSize.Height));
        return new FormattedText(
            string.IsNullOrEmpty(layer.Text) ? " " : layer.Text,
            CultureInfo.CurrentUICulture,
            FlowDirection.LeftToRight,
            typeface,
            fontSize,
            CreateBrush(layer.StrokeColor),
            96)
        {
            TextAlignment = TextAlignment.Left
        };
    }

    private static double GetStrokePixels(AnnotationLayer layer, Size imageSize)
    {
        var shorterSide = Math.Max(
            1,
            Math.Min(imageSize.Width, imageSize.Height));
        return Math.Max(1, layer.StrokeWidth * shorterSide);
    }

    private static Pen CreatePen(AnnotationLayer layer, Size imageSize)
    {
        var pen = new Pen(
            CreateBrush(layer.StrokeColor),
            GetStrokePixels(layer, imageSize))
        {
            StartLineCap = PenLineCap.Round,
            EndLineCap = PenLineCap.Round,
            LineJoin = PenLineJoin.Round
        };
        pen.Freeze();
        return pen;
    }

    private static Brush CreateBrush(uint argb)
    {
        var brush = new SolidColorBrush(
            Color.FromArgb(
                (byte)((argb >> 24) & 0xFF),
                (byte)((argb >> 16) & 0xFF),
                (byte)((argb >> 8) & 0xFF),
                (byte)(argb & 0xFF)));
        brush.Freeze();
        return brush;
    }

    private static Point ToPixels(double x, double y, Size imageSize) =>
        new(x * imageSize.Width, y * imageSize.Height);
}

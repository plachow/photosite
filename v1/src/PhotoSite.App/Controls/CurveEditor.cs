using System.Windows;
using System.Windows.Input;
using System.Windows.Media;
using PhotoSite.Domain;

namespace PhotoSite.Controls;

/// <summary>
/// The tone curve as a square you draw on: click to add a point, drag to
/// move it, drag it well outside the square to delete it. The curve stays a
/// <see cref="ToneCurve"/> the whole time, so what the editor shows is what
/// the pipeline bakes into its lookup table.
/// </summary>
public sealed class CurveEditor : FrameworkElement
{
    private const double HandleRadius = 5;
    private const double HitRadius = 9;
    private const double DeleteDistance = 40;
    private const double MinimumGap = 0.01;

    public static readonly DependencyProperty CurveProperty =
        DependencyProperty.Register(
            nameof(Curve),
            typeof(ToneCurve),
            typeof(CurveEditor),
            new FrameworkPropertyMetadata(
                ToneCurve.Linear,
                FrameworkPropertyMetadataOptions.AffectsRender));

    public static readonly DependencyProperty HistogramProperty =
        DependencyProperty.Register(
            nameof(Histogram),
            typeof(int[]),
            typeof(CurveEditor),
            new FrameworkPropertyMetadata(
                null,
                FrameworkPropertyMetadataOptions.AffectsRender));

    public static readonly DependencyProperty CurveBrushProperty =
        DependencyProperty.Register(
            nameof(CurveBrush),
            typeof(Brush),
            typeof(CurveEditor),
            new FrameworkPropertyMetadata(
                Brushes.White,
                FrameworkPropertyMetadataOptions.AffectsRender));

    private static readonly Brush BackgroundBrush =
        new SolidColorBrush(Color.FromRgb(0x11, 0x13, 0x18));

    private static readonly Pen GridPen =
        new(new SolidColorBrush(Color.FromRgb(0x2E, 0x33, 0x3E)), 1);

    private static readonly Pen DiagonalPen =
        new(new SolidColorBrush(Color.FromRgb(0x4A, 0x51, 0x60)), 1)
        {
            DashStyle = DashStyles.Dash
        };

    private static readonly Brush HistogramBrush =
        new SolidColorBrush(Color.FromArgb(0x50, 0xC8, 0xCD, 0xD8));

    private static readonly Brush HandleFill =
        new SolidColorBrush(Color.FromRgb(0x11, 0x13, 0x18));

    private List<CurvePoint> points = [];
    private int draggedIndex = -1;

    static CurveEditor()
    {
        BackgroundBrush.Freeze();
        GridPen.Freeze();
        DiagonalPen.Freeze();
        HistogramBrush.Freeze();
        HandleFill.Freeze();
    }

    public CurveEditor()
    {
        Width = 256;
        Height = 256;
        Cursor = Cursors.Cross;
        Focusable = true;
    }

    /// <summary>Raised after the user moved, added or removed a point.</summary>
    public event EventHandler<ToneCurve>? CurveChanged;

    public ToneCurve Curve
    {
        get => (ToneCurve)GetValue(CurveProperty);
        set => SetValue(CurveProperty, value);
    }

    /// <summary>An optional 256-bin histogram drawn behind the curve.</summary>
    public int[]? Histogram
    {
        get => (int[]?)GetValue(HistogramProperty);
        set => SetValue(HistogramProperty, value);
    }

    public Brush CurveBrush
    {
        get => (Brush)GetValue(CurveBrushProperty);
        set => SetValue(CurveBrushProperty, value);
    }

    /// <summary>
    /// The points as edited, which for a linear curve are its two ends so
    /// there is always something to grab.
    /// </summary>
    private IReadOnlyList<CurvePoint> EditablePoints =>
        Curve.Points.Count >= 2
            ? Curve.Points
            : [new CurvePoint(0, 0), new CurvePoint(1, 1)];

    protected override void OnRender(DrawingContext drawingContext)
    {
        var bounds = new Rect(RenderSize);
        drawingContext.DrawRoundedRectangle(BackgroundBrush, null, bounds, 3, 3);
        if (bounds.Width < 8 || bounds.Height < 8)
        {
            return;
        }

        drawingContext.PushClip(new RectangleGeometry(bounds));
        DrawHistogram(drawingContext, bounds);
        for (var step = 1; step < 4; step++)
        {
            var x = bounds.Width * step / 4;
            var y = bounds.Height * step / 4;
            drawingContext.DrawLine(GridPen, new Point(x, 0), new Point(x, bounds.Height));
            drawingContext.DrawLine(GridPen, new Point(0, y), new Point(bounds.Width, y));
        }

        drawingContext.DrawLine(
            DiagonalPen,
            new Point(0, bounds.Height),
            new Point(bounds.Width, 0));

        var samples = Curve.Sample(256);
        var geometry = new StreamGeometry();
        using (var sink = geometry.Open())
        {
            sink.BeginFigure(ToScreen(0, samples[0], bounds), false, false);
            for (var index = 1; index < samples.Length; index++)
            {
                sink.LineTo(
                    ToScreen(index / 255d, samples[index], bounds),
                    true,
                    false);
            }
        }

        geometry.Freeze();
        drawingContext.DrawGeometry(null, new Pen(CurveBrush, 2), geometry);

        var handlePen = new Pen(CurveBrush, 1.5);
        foreach (var point in draggedIndex >= 0 ? points : EditablePoints)
        {
            drawingContext.DrawEllipse(
                HandleFill,
                handlePen,
                ToScreen(point.X, point.Y, bounds),
                HandleRadius,
                HandleRadius);
        }

        drawingContext.Pop();
    }

    private void DrawHistogram(DrawingContext drawingContext, Rect bounds)
    {
        if (Histogram is not { Length: > 1 } histogram)
        {
            return;
        }

        var peak = histogram.Max();
        if (peak <= 0)
        {
            return;
        }

        var geometry = new StreamGeometry();
        using (var sink = geometry.Open())
        {
            sink.BeginFigure(new Point(0, bounds.Height), true, true);
            for (var index = 0; index < histogram.Length; index++)
            {
                var x = bounds.Width * index / (histogram.Length - 1);
                // A square root keeps a single huge bin from flattening
                // everything else, the usual way histograms are drawn.
                var height = Math.Sqrt(histogram[index] / (double)peak) * bounds.Height;
                sink.LineTo(new Point(x, bounds.Height - height), true, false);
            }

            sink.LineTo(new Point(bounds.Width, bounds.Height), true, false);
        }

        geometry.Freeze();
        drawingContext.DrawGeometry(HistogramBrush, null, geometry);
    }

    private static Point ToScreen(double x, double y, Rect bounds) =>
        new(x * bounds.Width, (1 - y) * bounds.Height);

    private CurvePoint ToCurve(Point position) =>
        new(
            Math.Clamp(position.X / Math.Max(1, ActualWidth), 0, 1),
            Math.Clamp(1 - (position.Y / Math.Max(1, ActualHeight)), 0, 1));

    protected override void OnMouseLeftButtonDown(MouseButtonEventArgs eventArgs)
    {
        base.OnMouseLeftButtonDown(eventArgs);
        Focus();
        var position = eventArgs.GetPosition(this);
        points = [.. EditablePoints];
        draggedIndex = FindPoint(position);
        if (draggedIndex < 0)
        {
            var added = ToCurve(position);
            draggedIndex = points.FindIndex(point => point.X > added.X);
            if (draggedIndex < 0)
            {
                draggedIndex = points.Count;
            }

            points.Insert(draggedIndex, added);
            Commit();
        }

        CaptureMouse();
        eventArgs.Handled = true;
    }

    protected override void OnMouseRightButtonDown(MouseButtonEventArgs eventArgs)
    {
        base.OnMouseRightButtonDown(eventArgs);
        var index = FindPoint(eventArgs.GetPosition(this));
        if (index < 0)
        {
            return;
        }

        points = [.. EditablePoints];
        RemoveIfAllowed(index);
        draggedIndex = -1;
        Commit();
        eventArgs.Handled = true;
    }

    protected override void OnMouseMove(MouseEventArgs eventArgs)
    {
        base.OnMouseMove(eventArgs);
        if (draggedIndex < 0 || !IsMouseCaptured)
        {
            return;
        }

        var position = eventArgs.GetPosition(this);
        var moved = ToCurve(position);
        // The ends stay on their edges; a middle point may not overtake its
        // neighbours, which is what keeps the curve a function.
        var minimumX = draggedIndex == 0 ? 0 : points[draggedIndex - 1].X + MinimumGap;
        var maximumX = draggedIndex == points.Count - 1
            ? 1
            : points[draggedIndex + 1].X - MinimumGap;
        if (draggedIndex == 0)
        {
            maximumX = 0;
        }

        if (draggedIndex == points.Count - 1)
        {
            minimumX = 1;
        }

        points[draggedIndex] = new CurvePoint(
            Math.Clamp(moved.X, Math.Min(minimumX, maximumX), Math.Max(minimumX, maximumX)),
            moved.Y);
        Commit();
    }

    protected override void OnMouseLeftButtonUp(MouseButtonEventArgs eventArgs)
    {
        base.OnMouseLeftButtonUp(eventArgs);
        if (draggedIndex < 0)
        {
            return;
        }

        var position = eventArgs.GetPosition(this);
        var outside = position.X < -DeleteDistance
                      || position.Y < -DeleteDistance
                      || position.X > ActualWidth + DeleteDistance
                      || position.Y > ActualHeight + DeleteDistance;
        if (outside)
        {
            RemoveIfAllowed(draggedIndex);
        }

        draggedIndex = -1;
        ReleaseMouseCapture();
        Commit();
        eventArgs.Handled = true;
    }

    private void RemoveIfAllowed(int index)
    {
        // The two ends stay; a curve needs them to be a curve.
        if (index > 0 && index < points.Count - 1)
        {
            points.RemoveAt(index);
        }
    }

    private int FindPoint(Point position)
    {
        var bounds = new Rect(RenderSize);
        var candidates = draggedIndex >= 0 ? points : (IReadOnlyList<CurvePoint>)[.. EditablePoints];
        var best = -1;
        var bestDistance = HitRadius;
        for (var index = 0; index < candidates.Count; index++)
        {
            var screen = ToScreen(candidates[index].X, candidates[index].Y, bounds);
            var distance = (screen - position).Length;
            if (distance <= bestDistance)
            {
                best = index;
                bestDistance = distance;
            }
        }

        return best;
    }

    private void Commit()
    {
        var curve = ToneCurve.FromPoints(points);
        Curve = curve;
        CurveChanged?.Invoke(this, curve);
        InvalidateVisual();
    }
}

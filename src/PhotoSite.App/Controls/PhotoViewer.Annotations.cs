using System.Windows;
using System.Windows.Input;
using System.Windows.Media;
using PhotoSite.Domain;
using PhotoSite.Services.Imaging;

namespace PhotoSite.Controls;

public enum AnnotationTool
{
    None,
    Select,
    Text,
    Arrow,
    Line,
    Rectangle,
    Ellipse,
    Freehand
}

/// <summary>
/// Editing of the vector layers that sit above the photograph.
/// </summary>
/// <remarks>
/// Layers are positioned in the coordinate space of the finished, oriented,
/// cropped image - exactly the space <see cref="LayerRenderer"/> composes into
/// on export. Drawing them through the image's own rotation transform instead
/// would put an arrow in a different place on screen than in the saved file,
/// and would mirror text whenever the photograph was flipped.
/// </remarks>
public sealed partial class PhotoViewer
{
    private const double HandleSize = 9;
    private const double LayerHitTolerance = 10;

    private static readonly Brush LayerSelectionBrush =
        CreateFrozenBrush(Color.FromArgb(0xF0, 0x67, 0xB7, 0xFF));
    private static readonly Pen LayerSelectionPen =
        CreateFrozenPen(Color.FromArgb(0xF0, 0x67, 0xB7, 0xFF), 1.2);

    public static readonly DependencyProperty ActiveToolProperty =
        DependencyProperty.Register(
            nameof(ActiveTool),
            typeof(AnnotationTool),
            typeof(PhotoViewer),
            new FrameworkPropertyMetadata(
                AnnotationTool.None,
                FrameworkPropertyMetadataOptions.AffectsRender,
                OnActiveToolChanged));

    public static readonly DependencyProperty LayersProperty =
        DependencyProperty.Register(
            nameof(Layers),
            typeof(IReadOnlyList<AnnotationLayer>),
            typeof(PhotoViewer),
            new FrameworkPropertyMetadata(
                Array.Empty<AnnotationLayer>(),
                FrameworkPropertyMetadataOptions.AffectsRender));

    public static readonly DependencyProperty SelectedLayerIdProperty =
        DependencyProperty.Register(
            nameof(SelectedLayerId),
            typeof(string),
            typeof(PhotoViewer),
            new FrameworkPropertyMetadata(
                null,
                FrameworkPropertyMetadataOptions.AffectsRender));

    public static readonly DependencyProperty NewLayerTemplateProperty =
        DependencyProperty.Register(
            nameof(NewLayerTemplate),
            typeof(AnnotationLayer),
            typeof(PhotoViewer));

    private AnnotationLayer? draftLayer;
    private AnnotationLayer? layerAtDragStart;
    private Point layerDragOrigin;
    private LayerDragMode layerDragMode;

    private enum LayerDragMode
    {
        None,
        Create,
        Move,
        ResizeStart,
        ResizeEnd
    }

    public AnnotationTool ActiveTool
    {
        get => (AnnotationTool)GetValue(ActiveToolProperty);
        set => SetValue(ActiveToolProperty, value);
    }

    public IReadOnlyList<AnnotationLayer> Layers
    {
        get => (IReadOnlyList<AnnotationLayer>)GetValue(LayersProperty);
        set => SetValue(LayersProperty, value);
    }

    public string? SelectedLayerId
    {
        get => (string?)GetValue(SelectedLayerIdProperty);
        set => SetValue(SelectedLayerIdProperty, value);
    }

    /// <summary>
    /// Carries the colour, stroke width, font and fill the tool panel is
    /// currently showing, so a new object is created with them already set.
    /// </summary>
    public AnnotationLayer? NewLayerTemplate
    {
        get => (AnnotationLayer?)GetValue(NewLayerTemplateProperty);
        set => SetValue(NewLayerTemplateProperty, value);
    }

    /// <summary>Raised with a layer that was created or edited on the canvas.</summary>
    public event EventHandler<AnnotationLayer>? LayerCommitted;

    /// <summary>Raised when the canvas selection changes, with null for none.</summary>
    public event EventHandler<string?>? LayerSelectionChanged;

    public bool IsAnnotationMode => ActiveTool != AnnotationTool.None;

    private static void OnActiveToolChanged(
        DependencyObject dependencyObject,
        DependencyPropertyChangedEventArgs eventArgs)
    {
        var viewer = (PhotoViewer)dependencyObject;
        viewer.draftLayer = null;
        viewer.layerDragMode = LayerDragMode.None;
        if ((AnnotationTool)eventArgs.NewValue == AnnotationTool.None)
        {
            viewer.SelectedLayerId = null;
            viewer.LayerSelectionChanged?.Invoke(viewer, null);
        }

        viewer.InvalidateVisual();
    }

    /// <summary>
    /// The on-screen rectangle the finished image occupies. Because rotation
    /// is always a quarter turn, this rectangle is the displayed image.
    /// </summary>
    private Rect GetDisplayBounds() =>
        bitmap is null
            ? Rect.Empty
            : GetScreenBounds(bitmap, EditRecipe, GetRecipeCrop(EditRecipe), pan);

    private bool TryGetLayerPoint(Point viewerPoint, out Point layerPoint)
    {
        layerPoint = default;
        var bounds = GetDisplayBounds();
        if (bounds.IsEmpty || bounds.Width <= 0 || bounds.Height <= 0)
        {
            return false;
        }

        layerPoint = new Point(
            (viewerPoint.X - bounds.X) / bounds.Width,
            (viewerPoint.Y - bounds.Y) / bounds.Height);
        return true;
    }

    private void DrawLayers(DrawingContext drawingContext)
    {
        var bounds = GetDisplayBounds();
        if (bounds.IsEmpty || bounds.Width <= 0 || bounds.Height <= 0)
        {
            return;
        }

        var layers = draftLayer is null
            ? Layers
            : [.. Layers.Where(layer => layer.Id != draftLayer.Id), draftLayer];

        drawingContext.PushClip(new RectangleGeometry(bounds));
        drawingContext.PushTransform(
            new TranslateTransform(bounds.X, bounds.Y));
        LayerRenderer.Draw(
            drawingContext,
            layers,
            new Size(bounds.Width, bounds.Height));
        drawingContext.Pop();
        drawingContext.Pop();

        if (!IsAnnotationMode)
        {
            return;
        }

        var selected = layers.FirstOrDefault(
            layer => layer.Id == SelectedLayerId);
        if (selected is not null)
        {
            DrawLayerAdorner(drawingContext, selected, bounds);
        }
    }

    private void DrawLayerAdorner(
        DrawingContext drawingContext,
        AnnotationLayer layer,
        Rect bounds)
    {
        if (layer is ShapeLayer { Shape: ShapeKind.Line or ShapeKind.Arrow } line)
        {
            var start = ToScreen(new Point(line.X1, line.Y1), bounds);
            var end = ToScreen(new Point(line.X2, line.Y2), bounds);
            DrawHandle(drawingContext, start);
            DrawHandle(drawingContext, end);
            return;
        }

        var region = layer.GetBounds();
        var rectangle = new Rect(
            ToScreen(new Point(region.X, region.Y), bounds),
            ToScreen(new Point(region.Right, region.Bottom), bounds));
        rectangle.Inflate(3, 3);
        drawingContext.DrawRectangle(null, LayerSelectionPen, rectangle);
        if (layer is ShapeLayer)
        {
            DrawHandle(drawingContext, rectangle.TopLeft);
            DrawHandle(drawingContext, rectangle.BottomRight);
        }
    }

    private static void DrawHandle(DrawingContext drawingContext, Point point)
    {
        drawingContext.DrawRectangle(
            LayerSelectionBrush,
            null,
            new Rect(
                point.X - (HandleSize / 2),
                point.Y - (HandleSize / 2),
                HandleSize,
                HandleSize));
    }

    private static Point ToScreen(Point layerPoint, Rect bounds) =>
        new(
            bounds.X + (layerPoint.X * bounds.Width),
            bounds.Y + (layerPoint.Y * bounds.Height));

    /// <summary>
    /// Handles a press while an annotation tool is active. Returns true when
    /// the viewer consumed it and must not also start a pan.
    /// </summary>
    private bool TryBeginLayerInteraction(Point position)
    {
        if (!IsAnnotationMode || !TryGetLayerPoint(position, out var point))
        {
            return false;
        }

        layerDragOrigin = point;
        var bounds = GetDisplayBounds();

        if (ActiveTool == AnnotationTool.Select)
        {
            var hit = HitTestLayer(position, bounds);
            SelectedLayerId = hit?.Id;
            LayerSelectionChanged?.Invoke(this, hit?.Id);
            layerAtDragStart = hit;
            layerDragMode = hit is null
                ? LayerDragMode.None
                : ResolveDragMode(hit, position, bounds);
            if (hit is not null)
            {
                CaptureMouse();
            }

            InvalidateVisual();
            return true;
        }

        if (ActiveTool == AnnotationTool.Text)
        {
            var text = CreateTextLayer(point);
            draftLayer = null;
            LayerCommitted?.Invoke(this, text);
            SelectedLayerId = text.Id;
            LayerSelectionChanged?.Invoke(this, text.Id);
            ActiveTool = AnnotationTool.Select;
            InvalidateVisual();
            return true;
        }

        draftLayer = CreateDraftLayer(point);
        layerDragMode = LayerDragMode.Create;
        CaptureMouse();
        InvalidateVisual();
        return true;
    }

    private LayerDragMode ResolveDragMode(
        AnnotationLayer layer,
        Point position,
        Rect bounds)
    {
        if (layer is not ShapeLayer shape)
        {
            return LayerDragMode.Move;
        }

        if (shape.Shape is ShapeKind.Line or ShapeKind.Arrow)
        {
            var start = ToScreen(new Point(shape.X1, shape.Y1), bounds);
            var end = ToScreen(new Point(shape.X2, shape.Y2), bounds);
            if ((position - start).Length <= LayerHitTolerance)
            {
                return LayerDragMode.ResizeStart;
            }

            return (position - end).Length <= LayerHitTolerance
                ? LayerDragMode.ResizeEnd
                : LayerDragMode.Move;
        }

        var region = shape.GetBounds();
        var topLeft = ToScreen(new Point(region.X, region.Y), bounds);
        var bottomRight = ToScreen(new Point(region.Right, region.Bottom), bounds);
        if ((position - topLeft).Length <= LayerHitTolerance)
        {
            return LayerDragMode.ResizeStart;
        }

        return (position - bottomRight).Length <= LayerHitTolerance
            ? LayerDragMode.ResizeEnd
            : LayerDragMode.Move;
    }

    private bool TryUpdateLayerInteraction(Point position)
    {
        if (layerDragMode == LayerDragMode.None
            || !TryGetLayerPoint(position, out var point))
        {
            return false;
        }

        if (layerDragMode == LayerDragMode.Create && draftLayer is not null)
        {
            draftLayer = ExtendDraft(draftLayer, point);
            InvalidateVisual();
            return true;
        }

        if (layerAtDragStart is null)
        {
            return false;
        }

        var delta = point - layerDragOrigin;
        draftLayer = layerDragMode switch
        {
            LayerDragMode.Move => layerAtDragStart.Translate(delta.X, delta.Y),
            LayerDragMode.ResizeStart when layerAtDragStart is ShapeLayer shape =>
                ResizeShape(shape, point, movesStart: true),
            LayerDragMode.ResizeEnd when layerAtDragStart is ShapeLayer shape =>
                ResizeShape(shape, point, movesStart: false),
            _ => layerAtDragStart
        };
        InvalidateVisual();
        return true;
    }

    private void FinishLayerInteraction()
    {
        if (layerDragMode == LayerDragMode.None)
        {
            return;
        }

        var committed = draftLayer;
        layerDragMode = LayerDragMode.None;
        layerAtDragStart = null;
        draftLayer = null;
        if (IsMouseCaptured)
        {
            ReleaseMouseCapture();
        }

        if (committed is null)
        {
            return;
        }

        // A stray click should not leave an invisible zero-size object behind.
        var region = committed.GetBounds();
        if (committed is ShapeLayer && region.Width < 0.004 && region.Height < 0.004)
        {
            InvalidateVisual();
            return;
        }

        if (committed is FreehandLayer { Points.Count: < 2 })
        {
            InvalidateVisual();
            return;
        }

        LayerCommitted?.Invoke(this, committed);
        SelectedLayerId = committed.Id;
        LayerSelectionChanged?.Invoke(this, committed.Id);
        if (ActiveTool != AnnotationTool.Freehand)
        {
            // One shape per press; freehand stays armed for the next stroke.
            ActiveTool = AnnotationTool.Select;
        }

        InvalidateVisual();
    }

    private AnnotationLayer? HitTestLayer(Point position, Rect bounds)
    {
        // Topmost first: the layer drawn last is the one the user sees.
        for (var index = Layers.Count - 1; index >= 0; index--)
        {
            var layer = Layers[index];
            if (!layer.IsVisible)
            {
                continue;
            }

            if (layer is ShapeLayer { Shape: ShapeKind.Line or ShapeKind.Arrow } line)
            {
                if (DistanceToSegment(
                        position,
                        ToScreen(new Point(line.X1, line.Y1), bounds),
                        ToScreen(new Point(line.X2, line.Y2), bounds))
                    <= LayerHitTolerance)
                {
                    return layer;
                }

                continue;
            }

            var region = layer.GetBounds();
            var rectangle = new Rect(
                ToScreen(new Point(region.X, region.Y), bounds),
                ToScreen(new Point(region.Right, region.Bottom), bounds));
            rectangle.Inflate(LayerHitTolerance / 2, LayerHitTolerance / 2);
            if (rectangle.Contains(position))
            {
                return layer;
            }
        }

        return null;
    }

    internal static double DistanceToSegment(Point point, Point start, Point end)
    {
        var segment = end - start;
        var lengthSquared = segment.LengthSquared;
        if (lengthSquared < 0.0001)
        {
            return (point - start).Length;
        }

        var t = Math.Clamp(
            ((point - start) * segment) / lengthSquared,
            0,
            1);
        return (point - (start + (segment * t))).Length;
    }

    private AnnotationLayer CreateDraftLayer(Point point)
    {
        var template = NewLayerTemplate;
        return ActiveTool switch
        {
            AnnotationTool.Freehand => new FreehandLayer
            {
                Name = "Drawing",
                Points = [new CurvePoint(point.X, point.Y)],
                StrokeColor = template?.StrokeColor ?? 0xFFFF3B30,
                StrokeWidth = template?.StrokeWidth ?? 0.004,
                Opacity = template?.Opacity ?? 1
            },
            _ => new ShapeLayer
            {
                Name = ActiveTool.ToString(),
                Shape = ActiveTool switch
                {
                    AnnotationTool.Arrow => ShapeKind.Arrow,
                    AnnotationTool.Line => ShapeKind.Line,
                    AnnotationTool.Ellipse => ShapeKind.Ellipse,
                    _ => ShapeKind.Rectangle
                },
                X1 = point.X,
                Y1 = point.Y,
                X2 = point.X,
                Y2 = point.Y,
                StrokeColor = template?.StrokeColor ?? 0xFFFF3B30,
                FillColor = template?.FillColor ?? 0,
                StrokeWidth = template?.StrokeWidth ?? 0.004,
                Opacity = template?.Opacity ?? 1
            }
        };
    }

    private TextLayer CreateTextLayer(Point point)
    {
        var template = NewLayerTemplate as TextLayer;
        var layer = new TextLayer
        {
            Name = "Text",
            Text = string.IsNullOrWhiteSpace(template?.Text)
                ? "Text"
                : template.Text,
            X = point.X,
            Y = point.Y,
            FontSize = template?.FontSize ?? 0.05,
            FontFamily = template?.FontFamily ?? "Segoe UI",
            Bold = template?.Bold ?? false,
            Italic = template?.Italic ?? false,
            StrokeColor = template?.StrokeColor ?? 0xFFFF3B30,
            BackgroundColor = template?.BackgroundColor ?? 0,
            Opacity = template?.Opacity ?? 1
        };
        return MeasureText(layer);
    }

    /// <summary>
    /// Records the drawn size on the layer so hit-testing and the selection
    /// box match the glyphs instead of guessing from the font size.
    /// </summary>
    internal TextLayer MeasureText(TextLayer layer)
    {
        var bounds = GetDisplayBounds();
        if (bounds.IsEmpty || bounds.Width <= 0)
        {
            return layer;
        }

        var measured = LayerRenderer.MeasureText(
            layer,
            new Size(bounds.Width, bounds.Height));
        return layer with
        {
            MeasuredWidth = measured.Width,
            MeasuredHeight = measured.Height
        };
    }

    private static AnnotationLayer ExtendDraft(
        AnnotationLayer draft,
        Point point) =>
        draft switch
        {
            FreehandLayer freehand => freehand with
            {
                Points = [.. freehand.Points, new CurvePoint(point.X, point.Y)]
            },
            ShapeLayer shape => shape with { X2 = point.X, Y2 = point.Y },
            _ => draft
        };

    private static ShapeLayer ResizeShape(
        ShapeLayer shape,
        Point point,
        bool movesStart) =>
        movesStart
            ? shape with { X1 = point.X, Y1 = point.Y }
            : shape with { X2 = point.X, Y2 = point.Y };

    /// <summary>Deletes the selected layer from the canvas keyboard shortcut.</summary>
    public bool TryDeleteSelectedLayer()
    {
        if (SelectedLayerId is not { } id)
        {
            return false;
        }

        LayerDeleteRequested?.Invoke(this, id);
        SelectedLayerId = null;
        LayerSelectionChanged?.Invoke(this, null);
        return true;
    }

    public event EventHandler<string>? LayerDeleteRequested;
}

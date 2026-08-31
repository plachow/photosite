using System.Text.Json.Serialization;

namespace PhotoSite.Domain;

public enum ShapeKind
{
    Rectangle,
    Ellipse,
    Line,
    Arrow
}

public enum LayerKind
{
    Shape,
    Text,
    Freehand
}

/// <summary>
/// A vector object living above the photograph. Coordinates are normalized to
/// the cropped image (0..1 on each axis) so a layer survives a later crop,
/// resize or export at a different resolution without being rasterized.
/// </summary>
[JsonPolymorphic(TypeDiscriminatorPropertyName = "$kind")]
[JsonDerivedType(typeof(ShapeLayer), "shape")]
[JsonDerivedType(typeof(TextLayer), "text")]
[JsonDerivedType(typeof(FreehandLayer), "freehand")]
public abstract record AnnotationLayer
{
    public string Id { get; init; } = Guid.NewGuid().ToString("N");

    public string Name { get; init; } = "Layer";

    public bool IsVisible { get; init; } = true;

    public double Opacity { get; init; } = 1;

    /// <summary>ARGB packed colour of the stroke or text.</summary>
    public uint StrokeColor { get; init; } = 0xFFFF3B30;

    /// <summary>ARGB packed fill colour; a zero alpha means "no fill".</summary>
    public uint FillColor { get; init; }

    /// <summary>
    /// Stroke width as a fraction of the image's shorter side, so the same
    /// layer looks identical in a 640 px preview and a 6000 px export.
    /// </summary>
    public double StrokeWidth { get; init; } = 0.004;

    [JsonIgnore]
    public abstract LayerKind Kind { get; }

    public abstract AnnotationLayer Translate(double deltaX, double deltaY);

    /// <summary>The normalized bounding box used for selection and handles.</summary>
    public abstract CropRegion GetBounds();
}

public sealed record ShapeLayer : AnnotationLayer
{
    public ShapeKind Shape { get; init; } = ShapeKind.Rectangle;

    public double X1 { get; init; }

    public double Y1 { get; init; }

    public double X2 { get; init; }

    public double Y2 { get; init; }

    public override LayerKind Kind => LayerKind.Shape;

    public override AnnotationLayer Translate(double deltaX, double deltaY) =>
        this with
        {
            X1 = X1 + deltaX,
            Y1 = Y1 + deltaY,
            X2 = X2 + deltaX,
            Y2 = Y2 + deltaY
        };

    public override CropRegion GetBounds() =>
        CropRegion.FromPoints(X1, Y1, X2, Y2);
}

public sealed record TextLayer : AnnotationLayer
{
    public string Text { get; init; } = "Text";

    public double X { get; init; }

    public double Y { get; init; }

    /// <summary>Font size as a fraction of the image height.</summary>
    public double FontSize { get; init; } = 0.05;

    public string FontFamily { get; init; } = "Segoe UI";

    public bool Bold { get; init; }

    public bool Italic { get; init; }

    /// <summary>ARGB background plate; zero alpha renders text without a plate.</summary>
    public uint BackgroundColor { get; init; }

    /// <summary>Cached during rendering so hit-testing knows the drawn box.</summary>
    public double MeasuredWidth { get; init; }

    public double MeasuredHeight { get; init; }

    public override LayerKind Kind => LayerKind.Text;

    public override AnnotationLayer Translate(double deltaX, double deltaY) =>
        this with { X = X + deltaX, Y = Y + deltaY };

    public override CropRegion GetBounds() =>
        new(
            X,
            Y,
            MeasuredWidth > 0 ? MeasuredWidth : FontSize * 4,
            MeasuredHeight > 0 ? MeasuredHeight : FontSize * 1.4);
}

public sealed record FreehandLayer : AnnotationLayer
{
    public IReadOnlyList<CurvePoint> Points { get; init; } = [];

    public override LayerKind Kind => LayerKind.Freehand;

    public override AnnotationLayer Translate(double deltaX, double deltaY) =>
        this with
        {
            Points = Points
                .Select(point => new CurvePoint(
                    point.X + deltaX,
                    point.Y + deltaY))
                .ToArray()
        };

    public override CropRegion GetBounds()
    {
        if (Points.Count == 0)
        {
            return default;
        }

        var left = Points.Min(point => point.X);
        var top = Points.Min(point => point.Y);
        var right = Points.Max(point => point.X);
        var bottom = Points.Max(point => point.Y);
        return new CropRegion(left, top, right - left, bottom - top);
    }

    public bool Equals(FreehandLayer? other) =>
        other is not null
        && base.Equals(other)
        && Points.SequenceEqual(other.Points);

    public override int GetHashCode()
    {
        var hash = new HashCode();
        hash.Add(base.GetHashCode());
        foreach (var point in Points)
        {
            hash.Add(point);
        }

        return hash.ToHashCode();
    }
}

using System.Text.Json.Serialization;

namespace PhotoSite.Domain;

public enum QuarterRotation
{
    None = 0,
    Clockwise90 = 1,
    Clockwise180 = 2,
    Clockwise270 = 3
}

public readonly record struct CropRegion(
    double X,
    double Y,
    double Width,
    double Height)
{
    private const double FullRegionTolerance = 0.000001;

    public static CropRegion Full { get; } = new(0, 0, 1, 1);

    public double Right => X + Width;

    public double Bottom => Y + Height;

    public bool IsEmpty => Width <= 0 || Height <= 0;

    public bool IsFull =>
        Math.Abs(X) <= FullRegionTolerance
        && Math.Abs(Y) <= FullRegionTolerance
        && Math.Abs(Width - 1) <= FullRegionTolerance
        && Math.Abs(Height - 1) <= FullRegionTolerance;

    public CropRegion ConstrainToUnit()
    {
        if (!double.IsFinite(X)
            || !double.IsFinite(Y)
            || !double.IsFinite(Width)
            || !double.IsFinite(Height))
        {
            return default;
        }

        var left = Math.Clamp(Math.Min(X, Right), 0, 1);
        var top = Math.Clamp(Math.Min(Y, Bottom), 0, 1);
        var right = Math.Clamp(Math.Max(X, Right), 0, 1);
        var bottom = Math.Clamp(Math.Max(Y, Bottom), 0, 1);
        return new CropRegion(
            left,
            top,
            Math.Max(0, right - left),
            Math.Max(0, bottom - top));
    }

    public static CropRegion FromPoints(
        double firstX,
        double firstY,
        double secondX,
        double secondY) =>
        new(
            Math.Min(firstX, secondX),
            Math.Min(firstY, secondY),
            Math.Abs(secondX - firstX),
            Math.Abs(secondY - firstY));
}

/// <summary>
/// The complete non-destructive description of an edit. Nothing here touches
/// the source file: the viewer, the exporter and the batch processor all
/// render from the original pixels plus this recipe.
/// </summary>
/// <remarks>
/// The first three members stay positional because recipes persisted by
/// earlier versions deserialize straight into them.
/// </remarks>
public sealed record EditRecipe(
    QuarterRotation Rotation = QuarterRotation.None,
    bool FlipHorizontal = false,
    CropRegion? Crop = null)
{
    public static EditRecipe Empty { get; } = new();

    public bool FlipVertical { get; init; }

    /// <summary>Free rotation in degrees applied before the crop, -45..45.</summary>
    public double StraightenAngle { get; init; }

    /// <summary>Keystone correction, -100..100.</summary>
    public double PerspectiveVertical { get; init; }

    public double PerspectiveHorizontal { get; init; }

    /// <summary>
    /// The pixel size the finished image is scaled to, 0 meaning the native
    /// size. One of the two at 0 keeps the aspect ratio; both set stretch.
    /// Applied after crop and orientation and before the layers, so a layer
    /// keeps its place on the photograph at every output size.
    /// </summary>
    public int OutputWidth { get; init; }

    public int OutputHeight { get; init; }

    public PhotoAdjustments Adjustments { get; init; } =
        PhotoAdjustments.Neutral;

    public IReadOnlyList<FilterStep> Filters { get; init; } = [];

    public IReadOnlyList<AnnotationLayer> Layers { get; init; } = [];

    [JsonIgnore]
    public bool HasGeometry =>
        Rotation != QuarterRotation.None
        || FlipHorizontal
        || FlipVertical
        || Crop is { IsFull: false, IsEmpty: false }
        || StraightenAngle != 0
        || PerspectiveVertical != 0
        || PerspectiveHorizontal != 0;

    [JsonIgnore]
    public bool HasPixelWork =>
        !Adjustments.IsNeutral || Filters.Count > 0;

    [JsonIgnore]
    public bool HasResize => OutputWidth > 0 || OutputHeight > 0;

    /// <summary>
    /// The size a finished image of <paramref name="width"/> by
    /// <paramref name="height"/> ends up at after the recipe's resize.
    /// </summary>
    public (int Width, int Height) MeasureResize(int width, int height)
    {
        if (!HasResize || width <= 0 || height <= 0)
        {
            return (width, height);
        }

        if (OutputWidth > 0 && OutputHeight > 0)
        {
            return (OutputWidth, OutputHeight);
        }

        return OutputWidth > 0
            ? (OutputWidth, Math.Max(1, (int)Math.Round(height * (OutputWidth / (double)width))))
            : (Math.Max(1, (int)Math.Round(width * (OutputHeight / (double)height))), OutputHeight);
    }

    [JsonIgnore]
    public bool HasLayers => Layers.Count > 0;

    [JsonIgnore]
    public bool IsEmpty => Equals(Empty);

    public EditRecipe RotateClockwise() =>
        this with { Rotation = (QuarterRotation)(((int)Rotation + 1) % 4) };

    public EditRecipe RotateCounterClockwise() =>
        this with { Rotation = (QuarterRotation)(((int)Rotation + 3) % 4) };

    /// <summary>
    /// Keeps geometry and layers but discards every tonal and creative edit.
    /// </summary>
    public EditRecipe WithoutPixelWork() =>
        this with
        {
            Adjustments = PhotoAdjustments.Neutral,
            Filters = []
        };

    public EditRecipe WithLayer(AnnotationLayer layer)
    {
        var replaced = false;
        var layers = new List<AnnotationLayer>(Layers.Count + 1);
        foreach (var existing in Layers)
        {
            if (existing.Id == layer.Id)
            {
                layers.Add(layer);
                replaced = true;
            }
            else
            {
                layers.Add(existing);
            }
        }

        if (!replaced)
        {
            layers.Add(layer);
        }

        return this with { Layers = layers };
    }

    public EditRecipe WithoutLayer(string layerId) =>
        this with
        {
            Layers = Layers.Where(layer => layer.Id != layerId).ToArray()
        };

    public EditRecipe WithLayerMoved(string layerId, int offset)
    {
        var layers = Layers.ToList();
        var index = layers.FindIndex(layer => layer.Id == layerId);
        var target = index + offset;
        if (index < 0 || target < 0 || target >= layers.Count)
        {
            return this;
        }

        var moved = layers[index];
        layers.RemoveAt(index);
        layers.Insert(target, moved);
        return this with { Layers = layers };
    }

    // Records give reference equality to the list members, which would make
    // every undo step and every "did anything change?" check wrong.
    public bool Equals(EditRecipe? other)
    {
        if (other is null)
        {
            return false;
        }

        if (ReferenceEquals(this, other))
        {
            return true;
        }

        return Rotation == other.Rotation
               && FlipHorizontal == other.FlipHorizontal
               && FlipVertical == other.FlipVertical
               && Nullable.Equals(Crop, other.Crop)
               && StraightenAngle.Equals(other.StraightenAngle)
               && PerspectiveVertical.Equals(other.PerspectiveVertical)
               && PerspectiveHorizontal.Equals(other.PerspectiveHorizontal)
               && OutputWidth == other.OutputWidth
               && OutputHeight == other.OutputHeight
               && Adjustments == other.Adjustments
               && Filters.SequenceEqual(other.Filters)
               && Layers.SequenceEqual(other.Layers);
    }

    public override int GetHashCode()
    {
        var hash = new HashCode();
        hash.Add(Rotation);
        hash.Add(FlipHorizontal);
        hash.Add(FlipVertical);
        hash.Add(Crop);
        hash.Add(StraightenAngle);
        hash.Add(PerspectiveVertical);
        hash.Add(PerspectiveHorizontal);
        hash.Add(OutputWidth);
        hash.Add(OutputHeight);
        hash.Add(Adjustments);
        hash.Add(Filters.Count);
        foreach (var filter in Filters)
        {
            hash.Add(filter);
        }

        hash.Add(Layers.Count);
        foreach (var layer in Layers)
        {
            hash.Add(layer);
        }

        return hash.ToHashCode();
    }
}

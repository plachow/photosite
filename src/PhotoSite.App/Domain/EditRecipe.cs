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

public sealed record EditRecipe(
    QuarterRotation Rotation = QuarterRotation.None,
    bool FlipHorizontal = false,
    CropRegion? Crop = null)
{
    public static EditRecipe Empty { get; } = new();

    public EditRecipe RotateClockwise() =>
        this with { Rotation = (QuarterRotation)(((int)Rotation + 1) % 4) };

    public EditRecipe RotateCounterClockwise() =>
        this with { Rotation = (QuarterRotation)(((int)Rotation + 3) % 4) };
}

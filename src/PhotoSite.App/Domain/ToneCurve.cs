namespace PhotoSite.Domain;

public readonly record struct CurvePoint(double X, double Y)
{
    public CurvePoint Clamped() =>
        new(Math.Clamp(X, 0, 1), Math.Clamp(Y, 0, 1));
}

/// <summary>
/// A monotone-in-X control-point curve evaluated with a monotone cubic
/// (Fritsch-Carlson) interpolation, so dragging one point never makes the
/// curve overshoot and invert tones somewhere else.
/// </summary>
public sealed record ToneCurve
{
    public static ToneCurve Linear { get; } = new();

    public IReadOnlyList<CurvePoint> Points { get; init; } = [];

    public bool IsLinear => Points.Count == 0;

    public static ToneCurve FromPoints(IEnumerable<CurvePoint> points)
    {
        var ordered = points
            .Select(point => point.Clamped())
            .OrderBy(point => point.X)
            .ToArray();
        if (ordered.Length == 0)
        {
            return Linear;
        }

        // Two identical X values would divide by zero during interpolation.
        var distinct = new List<CurvePoint>(ordered.Length) { ordered[0] };
        foreach (var point in ordered.Skip(1))
        {
            if (point.X - distinct[^1].X > 0.0005)
            {
                distinct.Add(point);
            }
            else
            {
                distinct[^1] = point;
            }
        }

        if (distinct.Count == 2
            && IsIdentity(distinct[0])
            && IsIdentity(distinct[1]))
        {
            return Linear;
        }

        return new ToneCurve { Points = distinct };
    }

    private static bool IsIdentity(CurvePoint point) =>
        Math.Abs(point.X - point.Y) < 0.0005;

    /// <summary>
    /// Samples the curve into a 0..1 lookup table of <paramref name="size"/>
    /// entries. Callers bake this into the per-channel byte LUT.
    /// </summary>
    public double[] Sample(int size = 256)
    {
        var table = new double[size];
        if (IsLinear || Points.Count < 2)
        {
            for (var index = 0; index < size; index++)
            {
                table[index] = index / (double)(size - 1);
            }

            return table;
        }

        var points = Points;
        var count = points.Count;
        var slopes = new double[count - 1];
        var tangents = new double[count];

        for (var index = 0; index < count - 1; index++)
        {
            slopes[index] =
                (points[index + 1].Y - points[index].Y)
                / (points[index + 1].X - points[index].X);
        }

        tangents[0] = slopes[0];
        tangents[count - 1] = slopes[count - 2];
        for (var index = 1; index < count - 1; index++)
        {
            tangents[index] = slopes[index - 1] * slopes[index] <= 0
                ? 0
                : (slopes[index - 1] + slopes[index]) / 2;
        }

        for (var index = 0; index < count - 1; index++)
        {
            if (Math.Abs(slopes[index]) < 1e-9)
            {
                tangents[index] = 0;
                tangents[index + 1] = 0;
                continue;
            }

            var alpha = tangents[index] / slopes[index];
            var beta = tangents[index + 1] / slopes[index];
            var magnitude = (alpha * alpha) + (beta * beta);
            if (magnitude > 9)
            {
                var scale = 3 / Math.Sqrt(magnitude);
                tangents[index] = scale * alpha * slopes[index];
                tangents[index + 1] = scale * beta * slopes[index];
            }
        }

        var segment = 0;
        for (var index = 0; index < size; index++)
        {
            var x = index / (double)(size - 1);
            if (x <= points[0].X)
            {
                table[index] = Math.Clamp(points[0].Y, 0, 1);
                continue;
            }

            if (x >= points[count - 1].X)
            {
                table[index] = Math.Clamp(points[count - 1].Y, 0, 1);
                continue;
            }

            while (segment < count - 2 && x > points[segment + 1].X)
            {
                segment++;
            }

            var start = points[segment];
            var end = points[segment + 1];
            var width = end.X - start.X;
            var t = (x - start.X) / width;
            var t2 = t * t;
            var t3 = t2 * t;
            var value =
                ((2 * t3) - (3 * t2) + 1) * start.Y
                + ((t3 - (2 * t2) + t) * width * tangents[segment])
                + (((-2 * t3) + (3 * t2)) * end.Y)
                + ((t3 - t2) * width * tangents[segment + 1]);
            table[index] = Math.Clamp(value, 0, 1);
        }

        return table;
    }

    public bool Equals(ToneCurve? other)
    {
        if (other is null)
        {
            return false;
        }

        if (ReferenceEquals(this, other))
        {
            return true;
        }

        return Points.SequenceEqual(other.Points);
    }

    public override int GetHashCode()
    {
        var hash = new HashCode();
        foreach (var point in Points)
        {
            hash.Add(point);
        }

        return hash.ToHashCode();
    }
}

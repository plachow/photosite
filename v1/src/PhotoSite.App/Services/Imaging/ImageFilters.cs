using PhotoSite.Domain;

namespace PhotoSite.Services.Imaging;

/// <summary>
/// The creative filters. Blur is the workhorse - sharpening, clarity and
/// vignette midpoints all build on it - so it uses three box passes, which
/// approximate a true Gaussian closely enough to be indistinguishable while
/// staying O(pixels) instead of O(pixels x radius).
/// </summary>
internal static class ImageFilters
{
    public static void Apply(
        PixelBuffer buffer,
        FilterStep filter,
        CancellationToken cancellationToken = default)
    {
        switch (filter.Kind)
        {
            case PhotoFilterKind.Sharpen:
            case PhotoFilterKind.UnsharpMask:
                ApplyUnsharpMask(
                    buffer,
                    filter.Amount,
                    Math.Max(0.3, filter.Radius),
                    filter.Threshold,
                    invert: false,
                    cancellationToken,
                    luminanceOnly: filter.Mode == 1);
                break;
            case PhotoFilterKind.Blur:
            case PhotoFilterKind.GaussianBlur:
                ApplyBlend(
                    buffer,
                    BlurCopy(buffer, Math.Max(0.5, filter.Radius), cancellationToken),
                    filter.Amount / 100,
                    cancellationToken);
                break;
            case PhotoFilterKind.MotionBlur:
                ApplyBlend(
                    buffer,
                    MotionBlurCopy(
                        buffer,
                        filter.Radius,
                        filter.Threshold,
                        cancellationToken),
                    filter.Amount / 100,
                    cancellationToken);
                break;
            case PhotoFilterKind.Pixelize:
                ApplyPixelize(
                    buffer,
                    (int)Math.Max(2, filter.Radius),
                    filter.Amount / 100,
                    cancellationToken);
                break;
            case PhotoFilterKind.NoiseReduction:
                NoiseReducer.Apply(
                    buffer,
                    filter.Amount,
                    filter.Mode == 1 ? filter.Radius : filter.Amount * 0.7,
                    cancellationToken);
                break;
            case PhotoFilterKind.AddNoise:
                ApplyNoise(
                    buffer,
                    filter.Amount,
                    colored: filter.Mode == 1,
                    cancellationToken);
                break;
            case PhotoFilterKind.Grayscale:
                ApplyMonochrome(buffer, filter.Amount / 100, sepia: false, cancellationToken);
                break;
            case PhotoFilterKind.Sepia:
                ApplyMonochrome(buffer, filter.Amount / 100, sepia: true, cancellationToken);
                break;
            case PhotoFilterKind.Vignette:
                ApplyVignette(
                    buffer,
                    filter.Amount,
                    filter.Radius,
                    cancellationToken,
                    feather: filter.Threshold);
                break;
            case PhotoFilterKind.Deinterlace:
                ApplyDeinterlace(
                    buffer,
                    filter.Mode,
                    filter.Amount / 100,
                    cancellationToken);
                break;
            case PhotoFilterKind.Invert:
                ApplyPointOperation(
                    buffer,
                    filter.Amount / 100,
                    static value => 255 - value,
                    cancellationToken);
                break;
            case PhotoFilterKind.Posterize:
                var levels = Math.Clamp((int)Math.Round(filter.Radius), 2, 64);
                var step = 255d / (levels - 1);
                ApplyPointOperation(
                    buffer,
                    filter.Amount / 100,
                    value => Math.Round(value / step) * step,
                    cancellationToken);
                break;
            case PhotoFilterKind.Solarize:
                var flip = Math.Clamp(filter.Radius, 0, 100) * 2.55;
                ApplyPointOperation(
                    buffer,
                    filter.Amount / 100,
                    value => value > flip ? 255 - value : value,
                    cancellationToken);
                break;
        }
    }

    public static void ApplyUnsharpMask(
        PixelBuffer buffer,
        double amount,
        double radius,
        double threshold,
        bool invert,
        CancellationToken cancellationToken = default,
        bool luminanceOnly = false)
    {
        if (amount == 0)
        {
            return;
        }

        var blurred = BlurCopy(buffer, radius, cancellationToken);
        var strength = (invert ? -amount : amount) / 100;
        var thresholdBytes = Math.Clamp(threshold, 0, 255);
        var pixels = buffer.Pixels;
        var blurredPixels = blurred.Pixels;
        var width = buffer.Width;

        Parallel.For(
            0,
            buffer.Height,
            new ParallelOptions { CancellationToken = cancellationToken },
            row =>
            {
                var offset = row * width * PixelBuffer.BytesPerPixel;
                for (var column = 0; column < width; column++)
                {
                    var index = offset + (column * PixelBuffer.BytesPerPixel);
                    if (luminanceOnly)
                    {
                        // One offset shared by the three channels keeps the
                        // chroma where it was; only the brightness of the
                        // edge changes, so no colour halo can appear.
                        var difference =
                            Luminance(pixels, index) - Luminance(blurredPixels, index);
                        if (Math.Abs(difference) < thresholdBytes)
                        {
                            continue;
                        }

                        var delta = difference * strength;
                        pixels[index] = ToByte(pixels[index] + delta);
                        pixels[index + 1] = ToByte(pixels[index + 1] + delta);
                        pixels[index + 2] = ToByte(pixels[index + 2] + delta);
                        continue;
                    }

                    for (var channel = 0; channel < 3; channel++)
                    {
                        var original = pixels[index + channel];
                        var difference = original - blurredPixels[index + channel];
                        if (Math.Abs(difference) < thresholdBytes)
                        {
                            continue;
                        }

                        pixels[index + channel] = ToByte(
                            original + (difference * strength));
                    }
                }
            });
    }

    private static double Luminance(byte[] pixels, int index) =>
        (0.2126 * pixels[index + 2])
        + (0.7152 * pixels[index + 1])
        + (0.0722 * pixels[index]);

    public static void ApplyVignette(
        PixelBuffer buffer,
        double amount,
        double midpoint,
        CancellationToken cancellationToken = default,
        double feather = 0)
    {
        if (amount == 0)
        {
            return;
        }

        var strength = Math.Clamp(amount, -100, 100) / 100;
        var center = Math.Clamp(midpoint, 1, 100) / 100;
        // Feather is the exponent of the roll-off: 50 is the classic square
        // law, lower bites harder at the midpoint, higher fades in gently.
        // A recipe from before the feather existed carries 0 and keeps the
        // square.
        var exponent = feather <= 0 ? 2 : Math.Clamp(feather, 1, 100) / 25;
        var pixels = buffer.Pixels;
        var width = buffer.Width;
        var height = buffer.Height;
        var centerX = (width - 1) / 2d;
        var centerY = (height - 1) / 2d;
        var maximumDistance = Math.Sqrt((centerX * centerX) + (centerY * centerY));

        Parallel.For(
            0,
            height,
            new ParallelOptions { CancellationToken = cancellationToken },
            row =>
            {
                var offset = row * width * PixelBuffer.BytesPerPixel;
                var deltaY = row - centerY;
                for (var column = 0; column < width; column++)
                {
                    var deltaX = column - centerX;
                    var distance = Math.Sqrt((deltaX * deltaX) + (deltaY * deltaY))
                                   / maximumDistance;
                    var falloff = Math.Clamp(
                        (distance - center) / Math.Max(0.0001, 1 - center),
                        0,
                        1);
                    // The power keeps the centre untouched and rolls the
                    // darkening on gradually towards the corners.
                    var factor = 1 - (strength * Math.Pow(falloff, exponent));
                    if (Math.Abs(factor - 1) < 0.0005)
                    {
                        continue;
                    }

                    var index = offset + (column * PixelBuffer.BytesPerPixel);
                    pixels[index] = ToByte(pixels[index] * factor);
                    pixels[index + 1] = ToByte(pixels[index + 1] * factor);
                    pixels[index + 2] = ToByte(pixels[index + 2] * factor);
                }
            });
    }

    public static PixelBuffer BlurCopy(
        PixelBuffer buffer,
        double radius,
        CancellationToken cancellationToken = default)
    {
        var blurred = buffer.Clone();
        if (radius < 0.4)
        {
            return blurred;
        }

        foreach (var boxRadius in GetBoxRadii(radius))
        {
            if (boxRadius <= 0)
            {
                continue;
            }

            BoxBlurHorizontal(blurred, boxRadius, cancellationToken);
            BoxBlurVertical(blurred, boxRadius, cancellationToken);
        }

        return blurred;
    }

    /// <summary>
    /// Three box passes whose radii sum to the requested Gaussian sigma; the
    /// classic Kovesi approximation.
    /// </summary>
    private static int[] GetBoxRadii(double radius)
    {
        var sigma = Math.Max(0.4, radius);
        var idealWidth = Math.Sqrt(((12 * sigma * sigma) / 3) + 1);
        var lower = (int)Math.Floor(idealWidth);
        if (lower % 2 == 0)
        {
            lower--;
        }

        var upper = lower + 2;
        var ideal = ((12 * sigma * sigma)
                     - (3 * lower * lower)
                     - (12 * lower)
                     - 9)
                    / ((-4 * lower) - 4);
        var median = (int)Math.Round(ideal);

        var radii = new int[3];
        for (var pass = 0; pass < 3; pass++)
        {
            var size = pass < median ? lower : upper;
            radii[pass] = Math.Max(0, (size - 1) / 2);
        }

        return radii;
    }

    private static void BoxBlurHorizontal(
        PixelBuffer buffer,
        int radius,
        CancellationToken cancellationToken)
    {
        var width = buffer.Width;
        var pixels = buffer.Pixels;
        var source = new byte[pixels.Length];
        Buffer.BlockCopy(pixels, 0, source, 0, pixels.Length);

        Parallel.For(
            0,
            buffer.Height,
            new ParallelOptions { CancellationToken = cancellationToken },
            row =>
            {
                var rowOffset = row * width * PixelBuffer.BytesPerPixel;
                Span<int> sums = stackalloc int[4];
                var window = 0;
                for (var column = -radius; column <= radius; column++)
                {
                    var clamped = Math.Clamp(column, 0, width - 1);
                    var index = rowOffset + (clamped * PixelBuffer.BytesPerPixel);
                    for (var channel = 0; channel < 4; channel++)
                    {
                        sums[channel] += source[index + channel];
                    }

                    window++;
                }

                for (var column = 0; column < width; column++)
                {
                    var index = rowOffset + (column * PixelBuffer.BytesPerPixel);
                    for (var channel = 0; channel < 4; channel++)
                    {
                        pixels[index + channel] = (byte)(sums[channel] / window);
                    }

                    var leaving = Math.Clamp(column - radius, 0, width - 1);
                    var entering = Math.Clamp(column + radius + 1, 0, width - 1);
                    var leavingIndex = rowOffset + (leaving * PixelBuffer.BytesPerPixel);
                    var enteringIndex = rowOffset + (entering * PixelBuffer.BytesPerPixel);
                    for (var channel = 0; channel < 4; channel++)
                    {
                        sums[channel] += source[enteringIndex + channel]
                                         - source[leavingIndex + channel];
                    }
                }
            });
    }

    private static void BoxBlurVertical(
        PixelBuffer buffer,
        int radius,
        CancellationToken cancellationToken)
    {
        var width = buffer.Width;
        var height = buffer.Height;
        var stride = width * PixelBuffer.BytesPerPixel;
        var pixels = buffer.Pixels;
        var source = new byte[pixels.Length];
        Buffer.BlockCopy(pixels, 0, source, 0, pixels.Length);

        Parallel.For(
            0,
            width,
            new ParallelOptions { CancellationToken = cancellationToken },
            column =>
            {
                var columnOffset = column * PixelBuffer.BytesPerPixel;
                Span<int> sums = stackalloc int[4];
                var window = 0;
                for (var row = -radius; row <= radius; row++)
                {
                    var clamped = Math.Clamp(row, 0, height - 1);
                    var index = (clamped * stride) + columnOffset;
                    for (var channel = 0; channel < 4; channel++)
                    {
                        sums[channel] += source[index + channel];
                    }

                    window++;
                }

                for (var row = 0; row < height; row++)
                {
                    var index = (row * stride) + columnOffset;
                    for (var channel = 0; channel < 4; channel++)
                    {
                        pixels[index + channel] = (byte)(sums[channel] / window);
                    }

                    var leaving = Math.Clamp(row - radius, 0, height - 1);
                    var entering = Math.Clamp(row + radius + 1, 0, height - 1);
                    var leavingIndex = (leaving * stride) + columnOffset;
                    var enteringIndex = (entering * stride) + columnOffset;
                    for (var channel = 0; channel < 4; channel++)
                    {
                        sums[channel] += source[enteringIndex + channel]
                                         - source[leavingIndex + channel];
                    }
                }
            });
    }

    private static void ApplyBlend(
        PixelBuffer destination,
        PixelBuffer overlay,
        double weight,
        CancellationToken cancellationToken)
    {
        var strength = Math.Clamp(weight, 0, 1);
        if (strength <= 0)
        {
            return;
        }

        var pixels = destination.Pixels;
        var overlayPixels = overlay.Pixels;
        var width = destination.Width;

        Parallel.For(
            0,
            destination.Height,
            new ParallelOptions { CancellationToken = cancellationToken },
            row =>
            {
                var offset = row * width * PixelBuffer.BytesPerPixel;
                for (var column = 0; column < width; column++)
                {
                    var index = offset + (column * PixelBuffer.BytesPerPixel);
                    for (var channel = 0; channel < 3; channel++)
                    {
                        pixels[index + channel] = ToByte(
                            pixels[index + channel]
                            + ((overlayPixels[index + channel]
                                - pixels[index + channel]) * strength));
                    }
                }
            });
    }

    private static void ApplyPixelize(
        PixelBuffer buffer,
        int cellSize,
        double weight,
        CancellationToken cancellationToken)
    {
        var strength = Math.Clamp(weight, 0, 1);
        if (strength <= 0)
        {
            return;
        }

        var width = buffer.Width;
        var height = buffer.Height;
        var pixels = buffer.Pixels;
        var cellRows = (height + cellSize - 1) / cellSize;

        Parallel.For(
            0,
            cellRows,
            new ParallelOptions { CancellationToken = cancellationToken },
            cellRow =>
            {
                var startRow = cellRow * cellSize;
                var endRow = Math.Min(height, startRow + cellSize);
                for (var startColumn = 0; startColumn < width; startColumn += cellSize)
                {
                    var endColumn = Math.Min(width, startColumn + cellSize);
                    long blue = 0, green = 0, red = 0;
                    var count = 0;
                    for (var row = startRow; row < endRow; row++)
                    {
                        var offset = row * width * PixelBuffer.BytesPerPixel;
                        for (var column = startColumn; column < endColumn; column++)
                        {
                            var index = offset + (column * PixelBuffer.BytesPerPixel);
                            blue += pixels[index];
                            green += pixels[index + 1];
                            red += pixels[index + 2];
                            count++;
                        }
                    }

                    if (count == 0)
                    {
                        continue;
                    }

                    var averageBlue = (double)blue / count;
                    var averageGreen = (double)green / count;
                    var averageRed = (double)red / count;
                    for (var row = startRow; row < endRow; row++)
                    {
                        var offset = row * width * PixelBuffer.BytesPerPixel;
                        for (var column = startColumn; column < endColumn; column++)
                        {
                            var index = offset + (column * PixelBuffer.BytesPerPixel);
                            pixels[index] = ToByte(
                                pixels[index]
                                + ((averageBlue - pixels[index]) * strength));
                            pixels[index + 1] = ToByte(
                                pixels[index + 1]
                                + ((averageGreen - pixels[index + 1]) * strength));
                            pixels[index + 2] = ToByte(
                                pixels[index + 2]
                                + ((averageRed - pixels[index + 2]) * strength));
                        }
                    }
                }
            });
    }

    private static void ApplyNoise(
        PixelBuffer buffer,
        double amount,
        bool colored,
        CancellationToken cancellationToken)
    {
        var strength = Math.Clamp(amount, 0, 100) / 100 * 96;
        if (strength <= 0)
        {
            return;
        }

        var pixels = buffer.Pixels;
        var width = buffer.Width;

        Parallel.For(
            0,
            buffer.Height,
            new ParallelOptions { CancellationToken = cancellationToken },
            () => new Random(Environment.CurrentManagedThreadId),
            (row, _, random) =>
            {
                var offset = row * width * PixelBuffer.BytesPerPixel;
                for (var column = 0; column < width; column++)
                {
                    var index = offset + (column * PixelBuffer.BytesPerPixel);
                    var delta = (random.NextDouble() - 0.5) * 2 * strength;
                    for (var channel = 0; channel < 3; channel++)
                    {
                        if (colored && channel > 0)
                        {
                            delta = (random.NextDouble() - 0.5) * 2 * strength;
                        }

                        pixels[index + channel] = ToByte(
                            pixels[index + channel] + delta);
                    }
                }

                return random;
            },
            _ => { });
    }

    private static void ApplyMonochrome(
        PixelBuffer buffer,
        double weight,
        bool sepia,
        CancellationToken cancellationToken)
    {
        var strength = Math.Clamp(weight, 0, 1);
        if (strength <= 0)
        {
            return;
        }

        var pixels = buffer.Pixels;
        var width = buffer.Width;

        Parallel.For(
            0,
            buffer.Height,
            new ParallelOptions { CancellationToken = cancellationToken },
            row =>
            {
                var offset = row * width * PixelBuffer.BytesPerPixel;
                for (var column = 0; column < width; column++)
                {
                    var index = offset + (column * PixelBuffer.BytesPerPixel);
                    double blue = pixels[index];
                    double green = pixels[index + 1];
                    double red = pixels[index + 2];
                    var luminance = (0.2126 * red)
                                    + (0.7152 * green)
                                    + (0.0722 * blue);

                    var targetRed = luminance;
                    var targetGreen = luminance;
                    var targetBlue = luminance;
                    if (sepia)
                    {
                        targetRed = luminance * 1.07;
                        targetGreen = luminance * 0.94;
                        targetBlue = luminance * 0.74;
                    }

                    pixels[index] = ToByte(blue + ((targetBlue - blue) * strength));
                    pixels[index + 1] = ToByte(green + ((targetGreen - green) * strength));
                    pixels[index + 2] = ToByte(red + ((targetRed - red) * strength));
                }
            });
    }

    /// <summary>
    /// Averages every pixel along one direction, the streak a moving subject
    /// or a panning camera leaves. Samples step one pixel at a time so the
    /// cost is pixels times length; the length is capped so a full-size
    /// render stays within seconds.
    /// </summary>
    public static PixelBuffer MotionBlurCopy(
        PixelBuffer buffer,
        double length,
        double angleDegrees,
        CancellationToken cancellationToken = default)
    {
        var result = buffer.Clone();
        var span = (int)Math.Round(Math.Clamp(length, 0, 200));
        if (span < 1)
        {
            return result;
        }

        var angle = angleDegrees * Math.PI / 180;
        var stepX = Math.Cos(angle);
        var stepY = -Math.Sin(angle);
        var width = buffer.Width;
        var height = buffer.Height;
        var source = buffer.Pixels;
        var pixels = result.Pixels;
        var half = span / 2;

        Parallel.For(
            0,
            height,
            new ParallelOptions { CancellationToken = cancellationToken },
            row =>
            {
                var offset = row * width * PixelBuffer.BytesPerPixel;
                for (var column = 0; column < width; column++)
                {
                    var sumBlue = 0;
                    var sumGreen = 0;
                    var sumRed = 0;
                    var count = 0;
                    for (var sample = -half; sample <= span - half; sample++)
                    {
                        var x = (int)Math.Round(column + (sample * stepX));
                        var y = (int)Math.Round(row + (sample * stepY));
                        if (x < 0 || x >= width || y < 0 || y >= height)
                        {
                            continue;
                        }

                        var index = ((y * width) + x) * PixelBuffer.BytesPerPixel;
                        sumBlue += source[index];
                        sumGreen += source[index + 1];
                        sumRed += source[index + 2];
                        count++;
                    }

                    if (count == 0)
                    {
                        continue;
                    }

                    var target = offset + (column * PixelBuffer.BytesPerPixel);
                    pixels[target] = (byte)(sumBlue / count);
                    pixels[target + 1] = (byte)(sumGreen / count);
                    pixels[target + 2] = (byte)(sumRed / count);
                }
            });

        return result;
    }

    /// <summary>
    /// Rebuilds one field of an interlaced frame from the other, or blends
    /// neighbouring lines, which is what takes the comb out of a video still.
    /// </summary>
    private static void ApplyDeinterlace(
        PixelBuffer buffer,
        int mode,
        double weight,
        CancellationToken cancellationToken)
    {
        var strength = Math.Clamp(weight, 0, 1);
        var height = buffer.Height;
        if (strength <= 0 || height < 3)
        {
            return;
        }

        var stride = buffer.Stride;
        var pixels = buffer.Pixels;
        var source = new byte[pixels.Length];
        Buffer.BlockCopy(pixels, 0, source, 0, pixels.Length);

        Parallel.For(
            0,
            height,
            new ParallelOptions { CancellationToken = cancellationToken },
            row =>
            {
                // Mode 0 keeps the even lines and rebuilds the odd ones, mode
                // 1 the reverse; mode 2 rebuilds every line from its
                // neighbours.
                var rebuild = mode switch
                {
                    0 => row % 2 == 1,
                    1 => row % 2 == 0,
                    _ => true
                };
                if (!rebuild)
                {
                    return;
                }

                var above = Math.Max(0, row - 1) * stride;
                var below = Math.Min(height - 1, row + 1) * stride;
                var offset = row * stride;
                for (var index = 0; index < stride; index++)
                {
                    var rebuilt = (source[above + index] + source[below + index]) / 2d;
                    pixels[offset + index] = ToByte(
                        source[offset + index]
                        + ((rebuilt - source[offset + index]) * strength));
                }
            });
    }

    /// <summary>
    /// A per-channel mapping blended in by weight - invert, posterize and
    /// solarize are all this with a different function.
    /// </summary>
    private static void ApplyPointOperation(
        PixelBuffer buffer,
        double weight,
        Func<double, double> map,
        CancellationToken cancellationToken)
    {
        var strength = Math.Clamp(weight, 0, 1);
        if (strength <= 0)
        {
            return;
        }

        var table = new byte[256];
        for (var value = 0; value < 256; value++)
        {
            table[value] = ToByte(value + ((map(value) - value) * strength));
        }

        var pixels = buffer.Pixels;
        var width = buffer.Width;
        Parallel.For(
            0,
            buffer.Height,
            new ParallelOptions { CancellationToken = cancellationToken },
            row =>
            {
                var offset = row * width * PixelBuffer.BytesPerPixel;
                for (var column = 0; column < width; column++)
                {
                    var index = offset + (column * PixelBuffer.BytesPerPixel);
                    pixels[index] = table[pixels[index]];
                    pixels[index + 1] = table[pixels[index + 1]];
                    pixels[index + 2] = table[pixels[index + 2]];
                }
            });
    }

    private static byte ToByte(double value) =>
        value <= 0 ? (byte)0
        : value >= 255 ? (byte)255
        : (byte)(value + 0.5);
}

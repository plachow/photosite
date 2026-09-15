using PhotoSite.Domain;

namespace PhotoSite.Services.Imaging;

/// <summary>
/// Straighten, keystone correction, crop, quarter rotation and flips.
/// </summary>
internal static class GeometryProcessor
{
    /// <summary>
    /// Straightening, keystone and lens distortion correction in one
    /// resampling pass.
    /// </summary>
    /// <remarks>
    /// Doing them separately would resample the photograph twice and lose
    /// detail for nothing. The frame is also scaled up just enough that the
    /// rotated source still covers every output pixel, which is why
    /// straightening never leaves the empty triangles in the corners that a
    /// plain rotation produces.
    /// </remarks>
    public static PixelBuffer Warp(
        PixelBuffer source,
        double straightenDegrees,
        double keystoneVertical,
        double keystoneHorizontal,
        CancellationToken cancellationToken = default,
        double lensDistortion = 0)
    {
        var angle = Math.Clamp(straightenDegrees, -45, 45) * Math.PI / 180;
        var vertical = Math.Clamp(keystoneVertical, -100, 100) / 100 * 0.5;
        var horizontal = Math.Clamp(keystoneHorizontal, -100, 100) / 100 * 0.5;
        // A positive slider removes barrel distortion by pulling the corners
        // outward, a negative one removes pincushion; a full slider moves
        // the corner of a 3:2 frame by roughly a fifth of the half-height,
        // which is more than any real lens needs.
        var distortion = -Math.Clamp(lensDistortion, -100, 100) / 100 * 0.25;
        if (angle == 0 && vertical == 0 && horizontal == 0 && distortion == 0)
        {
            return source;
        }

        var cos = Math.Cos(angle);
        var sin = Math.Sin(angle);
        var width = source.Width;
        var height = source.Height;
        var aspect = width / (double)height;
        var scale = FindCoveringScale(
            cos,
            sin,
            vertical,
            horizontal,
            distortion,
            aspect);

        var destination = source.CloneEmpty();
        var sourcePixels = source.Pixels;
        var destinationPixels = destination.Pixels;

        Parallel.For(
            0,
            height,
            new ParallelOptions { CancellationToken = cancellationToken },
            row =>
            {
                var offset = row * width * PixelBuffer.BytesPerPixel;
                var normalizedY = ((row + 0.5) / height) - 0.5;
                for (var column = 0; column < width; column++)
                {
                    var normalizedX = (((column + 0.5) / width) - 0.5) * aspect;
                    if (!TryProjectToSource(
                            normalizedX,
                            normalizedY,
                            cos,
                            sin,
                            vertical,
                            horizontal,
                            scale,
                            distortion,
                            out var sourceX,
                            out var sourceY))
                    {
                        continue;
                    }

                    var pixelX = ((sourceX / aspect) + 0.5) * width - 0.5;
                    var pixelY = (sourceY + 0.5) * height - 0.5;
                    SampleBilinear(
                        sourcePixels,
                        width,
                        height,
                        pixelX,
                        pixelY,
                        destinationPixels,
                        offset + (column * PixelBuffer.BytesPerPixel));
                }
            });

        return destination;
    }

    /// <summary>
    /// Maps a destination point back onto the source frame by undoing scale,
    /// rotation, both keystone shears and finally the lens correction, in
    /// reverse order. The lens is the first thing that ever touched the
    /// frame, so its correction is the last thing undone.
    /// </summary>
    private static bool TryProjectToSource(
        double x,
        double y,
        double cos,
        double sin,
        double vertical,
        double horizontal,
        double scale,
        double distortion,
        out double sourceX,
        out double sourceY)
    {
        sourceX = 0;
        sourceY = 0;

        var scaledX = x / scale;
        var scaledY = y / scale;

        var rotatedX = (scaledX * cos) + (scaledY * sin);
        var rotatedY = (-scaledX * sin) + (scaledY * cos);

        var horizontalFactor = 1 + (horizontal * rotatedX);
        if (Math.Abs(horizontalFactor) < 0.02)
        {
            return false;
        }

        var shearedY = rotatedY / horizontalFactor;
        var verticalFactor = 1 + (vertical * shearedY);
        if (Math.Abs(verticalFactor) < 0.02)
        {
            return false;
        }

        sourceX = rotatedX / verticalFactor;
        sourceY = shearedY;
        if (distortion != 0)
        {
            var radiusSquared = (sourceX * sourceX) + (sourceY * sourceY);
            var factor = 1 + (distortion * radiusSquared);
            sourceX *= factor;
            sourceY *= factor;
        }

        return true;
    }

    /// <summary>
    /// Binary-searches the smallest uniform scale at which all four output
    /// corners still land inside the source frame, so no transparent wedge
    /// survives the warp.
    /// </summary>
    private static double FindCoveringScale(
        double cos,
        double sin,
        double vertical,
        double horizontal,
        double distortion,
        double aspect)
    {
        var low = 1.0;
        var high = 4.0;
        for (var iteration = 0; iteration < 28; iteration++)
        {
            var middle = (low + high) / 2;
            if (CoversFrame(
                    cos,
                    sin,
                    vertical,
                    horizontal,
                    distortion,
                    aspect,
                    middle))
            {
                high = middle;
            }
            else
            {
                low = middle;
            }
        }

        return high;
    }

    private static bool CoversFrame(
        double cos,
        double sin,
        double vertical,
        double horizontal,
        double distortion,
        double aspect,
        double scale)
    {
        // Sampling the edges as well as the corners matters: a keystone shear
        // bows an edge inwards, so four corner checks alone would still leave
        // a sliver of empty pixels along the middle of a side. The frame is
        // measured in half-height units, the same space Warp projects in,
        // so a wide frame's corner sits where it really is.
        var halfWidth = aspect / 2;
        for (var stepX = 0; stepX <= 8; stepX++)
        {
            for (var stepY = 0; stepY <= 8; stepY++)
            {
                if (stepX is > 0 and < 8 && stepY is > 0 and < 8)
                {
                    continue;
                }

                var x = ((stepX / 8d) - 0.5) * aspect;
                var y = (stepY / 8d) - 0.5;
                if (!TryProjectToSource(
                        x,
                        y,
                        cos,
                        sin,
                        vertical,
                        horizontal,
                        scale,
                        distortion,
                        out var sourceX,
                        out var sourceY)
                    || Math.Abs(sourceX) > halfWidth
                    || Math.Abs(sourceY) > 0.5)
                {
                    return false;
                }
            }
        }

        return true;
    }

    private static void SampleBilinear(
        byte[] source,
        int width,
        int height,
        double x,
        double y,
        byte[] destination,
        int destinationIndex)
    {
        var clampedX = Math.Clamp(x, 0, width - 1);
        var clampedY = Math.Clamp(y, 0, height - 1);
        var left = (int)clampedX;
        var top = (int)clampedY;
        var right = Math.Min(width - 1, left + 1);
        var bottom = Math.Min(height - 1, top + 1);
        var fractionX = clampedX - left;
        var fractionY = clampedY - top;

        var topLeft = ((top * width) + left) * PixelBuffer.BytesPerPixel;
        var topRight = ((top * width) + right) * PixelBuffer.BytesPerPixel;
        var bottomLeft = ((bottom * width) + left) * PixelBuffer.BytesPerPixel;
        var bottomRight = ((bottom * width) + right) * PixelBuffer.BytesPerPixel;

        for (var channel = 0; channel < PixelBuffer.BytesPerPixel; channel++)
        {
            var upper = source[topLeft + channel]
                        + ((source[topRight + channel]
                            - source[topLeft + channel]) * fractionX);
            var lower = source[bottomLeft + channel]
                        + ((source[bottomRight + channel]
                            - source[bottomLeft + channel]) * fractionX);
            destination[destinationIndex + channel] =
                (byte)Math.Clamp(upper + ((lower - upper) * fractionY) + 0.5, 0, 255);
        }
    }

    public static PixelBuffer Crop(PixelBuffer source, CropRegion region)
    {
        var rectangle = GetPixelRect(source.Width, source.Height, region);
        if (rectangle.Width == source.Width && rectangle.Height == source.Height
            && rectangle.X == 0 && rectangle.Y == 0)
        {
            return source;
        }

        var destination = PixelBuffer.Create(
            rectangle.Width,
            rectangle.Height,
            source.DpiX,
            source.DpiY);
        var sourceStride = source.Stride;
        var destinationStride = destination.Stride;
        for (var row = 0; row < rectangle.Height; row++)
        {
            Buffer.BlockCopy(
                source.Pixels,
                ((rectangle.Y + row) * sourceStride)
                + (rectangle.X * PixelBuffer.BytesPerPixel),
                destination.Pixels,
                row * destinationStride,
                destinationStride);
        }

        return destination;
    }

    public static (int X, int Y, int Width, int Height) GetPixelRect(
        int width,
        int height,
        CropRegion region)
    {
        var constrained = region.ConstrainToUnit();
        if (constrained.IsEmpty)
        {
            return (0, 0, width, height);
        }

        var left = Math.Clamp(
            (int)Math.Floor(constrained.X * width),
            0,
            Math.Max(0, width - 1));
        var top = Math.Clamp(
            (int)Math.Floor(constrained.Y * height),
            0,
            Math.Max(0, height - 1));
        var right = Math.Clamp(
            (int)Math.Ceiling(constrained.Right * width),
            left + 1,
            width);
        var bottom = Math.Clamp(
            (int)Math.Ceiling(constrained.Bottom * height),
            top + 1,
            height);
        return (left, top, right - left, bottom - top);
    }

    public static PixelBuffer Orient(
        PixelBuffer source,
        QuarterRotation rotation,
        bool flipHorizontal,
        bool flipVertical)
    {
        if (rotation == QuarterRotation.None && !flipHorizontal && !flipVertical)
        {
            return source;
        }

        var swaps = rotation is QuarterRotation.Clockwise90
            or QuarterRotation.Clockwise270;
        var width = source.Width;
        var height = source.Height;
        var destinationWidth = swaps ? height : width;
        var destinationHeight = swaps ? width : height;
        var destination = PixelBuffer.Create(
            destinationWidth,
            destinationHeight,
            source.DpiX,
            source.DpiY);

        var sourcePixels = source.Pixels;
        var destinationPixels = destination.Pixels;

        for (var row = 0; row < height; row++)
        {
            var sourceOffset = row * width * PixelBuffer.BytesPerPixel;
            for (var column = 0; column < width; column++)
            {
                var x = column;
                var y = row;
                if (flipHorizontal)
                {
                    x = width - 1 - x;
                }

                if (flipVertical)
                {
                    y = height - 1 - y;
                }

                var (destinationX, destinationY) = rotation switch
                {
                    QuarterRotation.Clockwise90 => (height - 1 - y, x),
                    QuarterRotation.Clockwise180 => (width - 1 - x, height - 1 - y),
                    QuarterRotation.Clockwise270 => (y, width - 1 - x),
                    _ => (x, y)
                };

                var sourceIndex = sourceOffset + (column * PixelBuffer.BytesPerPixel);
                var destinationIndex =
                    ((destinationY * destinationWidth) + destinationX)
                    * PixelBuffer.BytesPerPixel;
                destinationPixels[destinationIndex] = sourcePixels[sourceIndex];
                destinationPixels[destinationIndex + 1] = sourcePixels[sourceIndex + 1];
                destinationPixels[destinationIndex + 2] = sourcePixels[sourceIndex + 2];
                destinationPixels[destinationIndex + 3] = sourcePixels[sourceIndex + 3];
            }
        }

        return destination;
    }
}

namespace PhotoSite.Services.Imaging;

/// <summary>
/// Lateral chromatic aberration shows up as red and blue fringes that grow
/// with the distance from the optical centre. Rescaling the red and blue
/// planes very slightly around that centre - green stays the reference - pulls
/// the fringes back onto the edge.
/// </summary>
internal static class ChromaticAberrationCorrector
{
    public static void Apply(
        PixelBuffer buffer,
        double amount,
        CancellationToken cancellationToken = default)
    {
        var strength = Math.Clamp(amount, -100, 100) / 100;
        if (strength == 0)
        {
            return;
        }

        // A full slider is 0.4 % of the frame, which is already more than most
        // lenses need; anything larger reads as a deliberate colour shift.
        var redScale = 1 - (strength * 0.004);
        var blueScale = 1 + (strength * 0.004);

        var width = buffer.Width;
        var height = buffer.Height;
        var source = buffer.Clone().Pixels;
        var pixels = buffer.Pixels;
        var centerX = (width - 1) / 2d;
        var centerY = (height - 1) / 2d;

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
                    var index = offset + (column * PixelBuffer.BytesPerPixel);
                    pixels[index + 2] = SampleChannel(
                        source,
                        width,
                        height,
                        centerX + (deltaX * redScale),
                        centerY + (deltaY * redScale),
                        channel: 2);
                    pixels[index] = SampleChannel(
                        source,
                        width,
                        height,
                        centerX + (deltaX * blueScale),
                        centerY + (deltaY * blueScale),
                        channel: 0);
                }
            });
    }

    private static byte SampleChannel(
        byte[] source,
        int width,
        int height,
        double x,
        double y,
        int channel)
    {
        var clampedX = Math.Clamp(x, 0, width - 1);
        var clampedY = Math.Clamp(y, 0, height - 1);
        var left = (int)clampedX;
        var top = (int)clampedY;
        var right = Math.Min(width - 1, left + 1);
        var bottom = Math.Min(height - 1, top + 1);
        var fractionX = clampedX - left;
        var fractionY = clampedY - top;

        var topLeft = source[
            (((top * width) + left) * PixelBuffer.BytesPerPixel) + channel];
        var topRight = source[
            (((top * width) + right) * PixelBuffer.BytesPerPixel) + channel];
        var bottomLeft = source[
            (((bottom * width) + left) * PixelBuffer.BytesPerPixel) + channel];
        var bottomRight = source[
            (((bottom * width) + right) * PixelBuffer.BytesPerPixel) + channel];

        var upper = topLeft + ((topRight - topLeft) * fractionX);
        var lower = bottomLeft + ((bottomRight - bottomLeft) * fractionX);
        var value = upper + ((lower - upper) * fractionY);
        return (byte)Math.Clamp(value + 0.5, 0, 255);
    }
}

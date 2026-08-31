namespace PhotoSite.Services.Imaging;

/// <summary>
/// Splits the frame into luminance and chroma and treats them separately,
/// because the two kinds of sensor noise need opposite handling: colour blobs
/// tolerate a wide blur without visible loss, while luminance grain has to be
/// smoothed only where the neighbourhood is genuinely flat or the photograph
/// turns to plastic.
/// </summary>
internal static class NoiseReducer
{
    public static void Apply(
        PixelBuffer buffer,
        double luminanceAmount,
        double colorAmount,
        CancellationToken cancellationToken = default)
    {
        var luminanceStrength = Math.Clamp(luminanceAmount, 0, 100) / 100;
        var colorStrength = Math.Clamp(colorAmount, 0, 100) / 100;
        if (luminanceStrength <= 0 && colorStrength <= 0)
        {
            return;
        }

        var width = buffer.Width;
        var height = buffer.Height;
        var pixels = buffer.Pixels;

        var chromaRadius = (int)Math.Round(1 + (colorStrength * 5));
        var blurred = ImageFilters.BlurCopy(
            buffer,
            Math.Max(1, chromaRadius),
            cancellationToken);
        var blurredPixels = blurred.Pixels;

        // Above this local contrast a pixel is treated as detail, not grain.
        var edgeThreshold = 6 + (luminanceStrength * 26);

        Parallel.For(
            0,
            height,
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
                    var luminance = (0.2126 * red) + (0.7152 * green) + (0.0722 * blue);

                    double blurredBlue = blurredPixels[index];
                    double blurredGreen = blurredPixels[index + 1];
                    double blurredRed = blurredPixels[index + 2];
                    var blurredLuminance = (0.2126 * blurredRed)
                                           + (0.7152 * blurredGreen)
                                           + (0.0722 * blurredBlue);

                    var targetLuminance = luminance;
                    if (luminanceStrength > 0)
                    {
                        var difference = Math.Abs(luminance - blurredLuminance);
                        // Full smoothing on flat areas, none across an edge.
                        var flatness = Math.Clamp(
                            1 - (difference / edgeThreshold),
                            0,
                            1);
                        targetLuminance = luminance
                                          + ((blurredLuminance - luminance)
                                             * luminanceStrength
                                             * flatness);
                    }

                    var targetBlue = blue;
                    var targetGreen = green;
                    var targetRed = red;
                    if (colorStrength > 0)
                    {
                        // Keep this pixel's own luminance, take the neighbourhood's colour.
                        targetBlue = blue
                                     + ((blurredBlue - blurredLuminance
                                         + luminance - blue) * colorStrength);
                        targetGreen = green
                                      + ((blurredGreen - blurredLuminance
                                          + luminance - green) * colorStrength);
                        targetRed = red
                                    + ((blurredRed - blurredLuminance
                                        + luminance - red) * colorStrength);
                    }

                    var luminanceShift = targetLuminance - luminance;
                    pixels[index] = ToByte(targetBlue + luminanceShift);
                    pixels[index + 1] = ToByte(targetGreen + luminanceShift);
                    pixels[index + 2] = ToByte(targetRed + luminanceShift);
                }
            });
    }

    private static byte ToByte(double value) =>
        value <= 0 ? (byte)0
        : value >= 255 ? (byte)255
        : (byte)(value + 0.5);
}

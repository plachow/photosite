using PhotoSite.Domain;

namespace PhotoSite.Services.Imaging;

/// <summary>
/// Applies the photographic adjustments of a recipe to a pixel buffer.
/// </summary>
/// <remarks>
/// Everything that acts on one channel independently - white balance,
/// exposure, blacks/whites, levels, gamma, contrast, brightness and the four
/// curves - is folded into a single 3x256 lookup table before the first pixel
/// is touched. Only the operations that genuinely need the other channels
/// (highlight/shadow recovery, vibrance, saturation) run per pixel, which is
/// what keeps a full-resolution render in the hundreds of milliseconds rather
/// than the tens of seconds a naive per-pixel implementation costs.
/// </remarks>
internal static class AdjustmentPipeline
{
    private const int LutSize = 256;

    public static void Apply(
        PixelBuffer buffer,
        PhotoAdjustments adjustments,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(buffer);
        ArgumentNullException.ThrowIfNull(adjustments);

        if (adjustments.ColorNoiseReduction > 0
            || adjustments.LuminanceNoiseReduction > 0)
        {
            NoiseReducer.Apply(
                buffer,
                adjustments.LuminanceNoiseReduction,
                adjustments.ColorNoiseReduction,
                cancellationToken);
        }

        if (adjustments.ChromaticAberration != 0)
        {
            ChromaticAberrationCorrector.Apply(
                buffer,
                adjustments.ChromaticAberration,
                cancellationToken);
        }

        if (adjustments.HasToneOrColorChange)
        {
            ApplyToneAndColor(buffer, adjustments, cancellationToken);
        }

        if (adjustments.Clarity != 0)
        {
            ImageFilters.ApplyUnsharpMask(
                buffer,
                amount: Math.Abs(adjustments.Clarity) * 0.9,
                radius: 18,
                threshold: 0,
                invert: adjustments.Clarity < 0,
                cancellationToken);
        }

        if (adjustments.SharpenAmount != 0)
        {
            ImageFilters.ApplyUnsharpMask(
                buffer,
                adjustments.SharpenAmount,
                Math.Max(0.3, adjustments.SharpenRadius),
                adjustments.SharpenThreshold,
                invert: false,
                cancellationToken);
        }

        var vignette = adjustments.Vignette - adjustments.LensVignetting;
        if (vignette != 0)
        {
            ImageFilters.ApplyVignette(buffer, vignette, 55, cancellationToken);
        }
    }

    private static void ApplyToneAndColor(
        PixelBuffer buffer,
        PhotoAdjustments adjustments,
        CancellationToken cancellationToken)
    {
        var channelLut = BuildChannelLut(adjustments);
        var toneGain = BuildHighlightShadowGain(adjustments);
        var hueMatrix = BuildHueMatrix(adjustments.Hue);
        var saturation = 1 + (adjustments.Saturation / 100);
        var vibrance = adjustments.Vibrance / 100;
        var needsSaturation = adjustments.Saturation != 0 || vibrance != 0;

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
                    double blue = channelLut[2][pixels[index]];
                    double green = channelLut[1][pixels[index + 1]];
                    double red = channelLut[0][pixels[index + 2]];

                    if (toneGain is not null)
                    {
                        var luminance = (0.2126 * red)
                                        + (0.7152 * green)
                                        + (0.0722 * blue);
                        var gain = toneGain[
                            (int)Math.Clamp(luminance * 255, 0, 255)];
                        red *= gain;
                        green *= gain;
                        blue *= gain;
                    }

                    if (hueMatrix is not null)
                    {
                        var rotatedRed = (hueMatrix[0] * red)
                                         + (hueMatrix[1] * green)
                                         + (hueMatrix[2] * blue);
                        var rotatedGreen = (hueMatrix[3] * red)
                                           + (hueMatrix[4] * green)
                                           + (hueMatrix[5] * blue);
                        var rotatedBlue = (hueMatrix[6] * red)
                                          + (hueMatrix[7] * green)
                                          + (hueMatrix[8] * blue);
                        red = rotatedRed;
                        green = rotatedGreen;
                        blue = rotatedBlue;
                    }

                    if (needsSaturation)
                    {
                        ApplySaturation(
                            ref red,
                            ref green,
                            ref blue,
                            saturation,
                            vibrance);
                    }

                    pixels[index] = ToByte(blue);
                    pixels[index + 1] = ToByte(green);
                    pixels[index + 2] = ToByte(red);
                }
            });
    }

    private static void ApplySaturation(
        ref double red,
        ref double green,
        ref double blue,
        double saturation,
        double vibrance)
    {
        var luminance = (0.2126 * red) + (0.7152 * green) + (0.0722 * blue);
        var factor = saturation;

        if (vibrance != 0)
        {
            // Vibrance protects colours that are already saturated (and, with
            // that, skin tones) by scaling its push down as the pixel's own
            // saturation rises.
            var maximum = Math.Max(red, Math.Max(green, blue));
            var minimum = Math.Min(red, Math.Min(green, blue));
            var current = maximum <= 0 ? 0 : (maximum - minimum) / maximum;
            factor += vibrance * (1 - Math.Clamp(current, 0, 1));
        }

        red = luminance + ((red - luminance) * factor);
        green = luminance + ((green - luminance) * factor);
        blue = luminance + ((blue - luminance) * factor);
    }

    /// <summary>
    /// Hue rotation as one 3x3 matrix: into YIQ, turn the chroma plane by the
    /// angle, back to RGB. Luminance is the axis of the rotation, so a
    /// pixel keeps its brightness and only its colour moves round the wheel.
    /// </summary>
    internal static double[]? BuildHueMatrix(double hueDegrees)
    {
        if (hueDegrees == 0)
        {
            return null;
        }

        // Negated so that a positive turn goes red, green, blue - the way
        // every hue wheel is read - rather than the direction the I/Q axes
        // happen to spin.
        var angle = -Math.Clamp(hueDegrees, -180, 180) * Math.PI / 180;
        var cos = Math.Cos(angle);
        var sin = Math.Sin(angle);

        double[] forward =
        [
            0.299, 0.587, 0.114,
            0.596, -0.274, -0.322,
            0.211, -0.523, 0.312
        ];
        double[] rotation =
        [
            1, 0, 0,
            0, cos, -sin,
            0, sin, cos
        ];
        double[] inverse =
        [
            1, 0.956, 0.621,
            1, -0.272, -0.647,
            1, -1.106, 1.703
        ];

        return Multiply(inverse, Multiply(rotation, forward));
    }

    private static double[] Multiply(double[] left, double[] right)
    {
        var result = new double[9];
        for (var row = 0; row < 3; row++)
        {
            for (var column = 0; column < 3; column++)
            {
                result[(row * 3) + column] =
                    (left[row * 3] * right[column])
                    + (left[(row * 3) + 1] * right[3 + column])
                    + (left[(row * 3) + 2] * right[6 + column]);
            }
        }

        return result;
    }

    /// <summary>
    /// A luminance-driven multiplier, so recovering highlights or lifting
    /// shadows keeps the hue of a pixel instead of shifting it the way three
    /// independent per-channel curves would.
    /// </summary>
    private static double[]? BuildHighlightShadowGain(PhotoAdjustments adjustments)
    {
        if (adjustments.Highlights == 0 && adjustments.Shadows == 0)
        {
            return null;
        }

        var highlights = adjustments.Highlights / 100;
        var shadows = adjustments.Shadows / 100;
        var gain = new double[LutSize];
        for (var index = 0; index < LutSize; index++)
        {
            var luminance = index / 255d;
            var value = luminance;

            if (shadows != 0)
            {
                // Weight peaks in the deep shadows and fades out by midtones.
                var mask = Math.Pow(1 - luminance, 3);
                value += shadows * 0.55 * mask * (1 - luminance);
            }

            if (highlights != 0)
            {
                var mask = Math.Pow(luminance, 3);
                value += highlights * 0.55 * mask * luminance;
            }

            value = Math.Clamp(value, 0, 1);
            gain[index] = luminance <= 0.0001 ? 1 : value / luminance;
        }

        return gain;
    }

    /// <summary>
    /// Folds every per-channel operation into one table per channel, applied
    /// in the order a photographer expects: white balance, exposure, black and
    /// white points, levels, gamma, contrast, brightness, then curves.
    /// </summary>
    private static double[][] BuildChannelLut(PhotoAdjustments adjustments)
    {
        var gains = GetWhiteBalanceGains(
            adjustments.Temperature,
            adjustments.Tint);
        var exposure = Math.Pow(2, adjustments.Exposure);
        var blacks = adjustments.Blacks / 100;
        var whites = adjustments.Whites / 100;
        var contrast = adjustments.Contrast / 100;
        var brightness = adjustments.Brightness / 100;
        var gamma = adjustments.Gamma <= 0 ? 1 : adjustments.Gamma;
        var levelBlack = Math.Clamp(adjustments.BlackPoint, 0, 254) / 255;
        var levelWhite = Math.Clamp(adjustments.WhitePoint, 1, 255) / 255;
        var levelRange = Math.Max(0.0001, levelWhite - levelBlack);
        var levelMid = adjustments.MidPoint <= 0 ? 1 : adjustments.MidPoint;

        var master = adjustments.Curve.Sample(LutSize);
        var perChannel = new[]
        {
            adjustments.RedCurve.Sample(LutSize),
            adjustments.GreenCurve.Sample(LutSize),
            adjustments.BlueCurve.Sample(LutSize)
        };

        var lut = new double[3][];
        for (var channel = 0; channel < 3; channel++)
        {
            var table = new double[LutSize];
            for (var index = 0; index < LutSize; index++)
            {
                var value = index / 255d;

                value *= gains[channel];
                value *= exposure;

                // Blacks crush or lift the toe; whites stretch or pull the
                // shoulder. Both are anchored so the opposite end stays put.
                if (blacks != 0)
                {
                    value += blacks * 0.25 * Math.Pow(1 - value, 2);
                }

                if (whites != 0)
                {
                    value += whites * 0.25 * Math.Pow(value, 2);
                }

                value = (value - levelBlack) / levelRange;
                value = Math.Clamp(value, 0, 1);
                if (levelMid != 1)
                {
                    value = Math.Pow(value, 1 / levelMid);
                }

                if (gamma != 1)
                {
                    value = Math.Pow(Math.Clamp(value, 0, 1), 1 / gamma);
                }

                if (contrast != 0)
                {
                    value = ApplyContrast(value, contrast);
                }

                if (brightness != 0)
                {
                    value = brightness > 0
                        ? value + (brightness * (1 - value))
                        : value * (1 + brightness);
                }

                value = Math.Clamp(value, 0, 1);
                value = Interpolate(master, value);
                value = Interpolate(perChannel[channel], value);
                table[index] = Math.Clamp(value, 0, 1);
            }

            lut[channel] = table;
        }

        return lut;
    }

    /// <summary>
    /// A smooth S-curve pivoting on middle grey. A straight linear stretch
    /// clips both ends long before the slider reaches its limit.
    /// </summary>
    private static double ApplyContrast(double value, double contrast)
    {
        var clamped = Math.Clamp(value, 0, 1);
        if (contrast > 0)
        {
            var smooth = clamped * clamped * (3 - (2 * clamped));
            return clamped + ((smooth - clamped) * contrast);
        }

        var flattened = 0.5 + ((clamped - 0.5) * (1 + contrast));
        return flattened;
    }

    private static double Interpolate(double[] table, double value)
    {
        var position = Math.Clamp(value, 0, 1) * (table.Length - 1);
        var lower = (int)position;
        if (lower >= table.Length - 1)
        {
            return table[^1];
        }

        var fraction = position - lower;
        return table[lower] + ((table[lower + 1] - table[lower]) * fraction);
    }

    /// <summary>
    /// Temperature and tint as simple channel gains, normalized so that a pure
    /// temperature move keeps overall luminance roughly constant.
    /// </summary>
    internal static double[] GetWhiteBalanceGains(
        double temperature,
        double tint)
    {
        if (temperature == 0 && tint == 0)
        {
            return [1, 1, 1];
        }

        var warm = Math.Clamp(temperature, -100, 100) / 100;
        var magenta = Math.Clamp(tint, -100, 100) / 100;

        var red = 1 + (warm * 0.35);
        var blue = 1 - (warm * 0.35);
        var green = 1 - (magenta * 0.28);

        var mean = ((0.2126 * red) + (0.7152 * green) + (0.0722 * blue));
        if (mean > 0.0001)
        {
            red /= mean;
            green /= mean;
            blue /= mean;
        }

        return [red, green, blue];
    }

    private static byte ToByte(double value) =>
        value <= 0 ? (byte)0
        : value >= 1 ? (byte)255
        : (byte)((value * 255) + 0.5);
}

using PhotoSite.Domain;

namespace PhotoSite.Services.Imaging;

/// <summary>
/// Works out a conservative set of adjustments for a photograph by measuring
/// it rather than by applying a fixed contrast boost.
/// </summary>
/// <remarks>
/// Every limit here is deliberately tight. The goal is the correction a
/// photographer would have dialled in anyway - a slightly flat frame opened
/// up, a colour cast removed - and explicitly not the over-cooked local-
/// contrast look that "auto enhance" buttons are known for. Because the
/// result is a plain <see cref="PhotoAdjustments"/>, every value it chose
/// lands on a visible slider the user can then take further or undo.
/// </remarks>
internal static class AutoFixAnalyzer
{
    public static PhotoAdjustments Analyze(
        PixelBuffer buffer,
        PhotoAdjustments current)
    {
        var histogram = HistogramData.FromBuffer(buffer);
        if (histogram.Total == 0)
        {
            return current;
        }

        var result = current.WithNeutralTone();

        var shadowEdge = histogram.GetLuminancePercentile(0.002);
        var highlightEdge = histogram.GetLuminancePercentile(0.998);

        // Only reclaim range the photograph genuinely is not using, and leave
        // a little headroom so nothing that was visible becomes pure black.
        if (shadowEdge > 6)
        {
            result = result with
            {
                BlackPoint = Math.Clamp(Math.Round((shadowEdge - 3) * 0.75), 0, 42)
            };
        }

        if (highlightEdge < 249)
        {
            result = result with
            {
                WhitePoint = Math.Clamp(
                    Math.Round(255 - ((255 - highlightEdge - 3) * 0.75)),
                    213,
                    255)
            };
        }

        var mean = histogram.GetMeanLuminance();
        if (mean > 0.01)
        {
            // Aim a touch below middle grey; most photographs read as correctly
            // exposed slightly darker than a mathematical 0.5.
            var exposure = Math.Log2(0.46 / mean);
            result = result with
            {
                Exposure = Math.Round(Math.Clamp(exposure, -0.75, 0.75), 2)
            };
        }

        var lowMid = histogram.GetLuminancePercentile(0.05);
        var highMid = histogram.GetLuminancePercentile(0.95);
        var spread = (highMid - lowMid) / 255d;
        if (spread < 0.62)
        {
            // A flat frame gets contrast in proportion to how flat it is.
            var contrast = (0.62 - spread) * 90;
            result = result with
            {
                Contrast = Math.Round(Math.Clamp(contrast, 0, 26))
            };
        }

        if (histogram.ShadowClippedFraction > 0.004 || lowMid < 22)
        {
            var lift = 12 + (histogram.ShadowClippedFraction * 900);
            result = result with
            {
                Shadows = Math.Round(Math.Clamp(lift, 0, 32))
            };
        }

        if (histogram.HighlightClippedFraction > 0.004 || highMid > 244)
        {
            var recovery = 12 + (histogram.HighlightClippedFraction * 900);
            result = result with
            {
                Highlights = -Math.Round(Math.Clamp(recovery, 0, 32))
            };
        }

        var whiteBalance = EstimateWhiteBalance(histogram);
        result = result with
        {
            Temperature = whiteBalance.Temperature,
            Tint = whiteBalance.Tint
        };

        var saturation = EstimateSaturation(buffer);
        if (saturation < 0.26)
        {
            result = result with
            {
                Vibrance = Math.Round(Math.Clamp((0.26 - saturation) * 90, 0, 18))
            };
        }

        return result;
    }

    /// <summary>
    /// Grey-world estimation: over a whole photograph the average of all
    /// colours tends towards neutral, so the deviation between the channel
    /// means is a usable measure of the cast. The result is deliberately
    /// under-applied - a sunset is supposed to stay warm.
    /// </summary>
    internal static (double Temperature, double Tint) EstimateWhiteBalance(
        HistogramData histogram)
    {
        var red = histogram.GetChannelMean(histogram.Red);
        var green = histogram.GetChannelMean(histogram.Green);
        var blue = histogram.GetChannelMean(histogram.Blue);
        if (red < 4 || green < 4 || blue < 4)
        {
            // A near-black or single-channel frame carries no usable estimate.
            return (0, 0);
        }

        var average = (red + green + blue) / 3;
        var redGain = average / red;
        var greenGain = average / green;
        var blueGain = average / blue;

        // Invert the gain model used by the pipeline, then keep 60 % of it.
        var temperature = (redGain - blueGain) / 0.7 * 100 * 0.6;
        var tint = (1 - greenGain) / 0.28 * 100 * 0.6;

        return (
            Math.Round(Math.Clamp(temperature, -22, 22)),
            Math.Round(Math.Clamp(tint, -18, 18)));
    }

    private static double EstimateSaturation(PixelBuffer buffer)
    {
        var pixels = buffer.Pixels;
        var width = buffer.Width;
        var height = buffer.Height;
        var step = Math.Max(1, (int)Math.Sqrt(width * height / 40_000d));
        double total = 0;
        var count = 0;

        for (var row = 0; row < height; row += step)
        {
            var offset = row * width * PixelBuffer.BytesPerPixel;
            for (var column = 0; column < width; column += step)
            {
                var index = offset + (column * PixelBuffer.BytesPerPixel);
                int blue = pixels[index];
                int green = pixels[index + 1];
                int red = pixels[index + 2];
                var maximum = Math.Max(red, Math.Max(green, blue));
                if (maximum == 0)
                {
                    count++;
                    continue;
                }

                var minimum = Math.Min(red, Math.Min(green, blue));
                total += (maximum - minimum) / (double)maximum;
                count++;
            }
        }

        return count == 0 ? 0 : total / count;
    }
}

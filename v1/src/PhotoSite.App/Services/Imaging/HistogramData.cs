using System.Windows.Media.Imaging;

namespace PhotoSite.Services.Imaging;

/// <summary>
/// The 256-bin distribution of a rendered frame, plus the clipping counts the
/// editor uses to warn about blown highlights and crushed shadows.
/// </summary>
internal sealed class HistogramData
{
    public const int BinCount = 256;

    private HistogramData(
        int[] red,
        int[] green,
        int[] blue,
        int[] luminance,
        long total,
        long shadowClipped,
        long highlightClipped)
    {
        Red = red;
        Green = green;
        Blue = blue;
        Luminance = luminance;
        Total = total;
        ShadowClippedPixels = shadowClipped;
        HighlightClippedPixels = highlightClipped;
        Peak = Math.Max(
            1,
            Math.Max(
                red.Skip(1).Take(BinCount - 2).DefaultIfEmpty(0).Max(),
                Math.Max(
                    green.Skip(1).Take(BinCount - 2).DefaultIfEmpty(0).Max(),
                    blue.Skip(1).Take(BinCount - 2).DefaultIfEmpty(0).Max())));
    }

    public static HistogramData Empty { get; } = new(
        new int[BinCount],
        new int[BinCount],
        new int[BinCount],
        new int[BinCount],
        0,
        0,
        0);

    public int[] Red { get; }

    public int[] Green { get; }

    public int[] Blue { get; }

    public int[] Luminance { get; }

    public long Total { get; }

    public int Peak { get; }

    public long ShadowClippedPixels { get; }

    public long HighlightClippedPixels { get; }

    public double ShadowClippedFraction =>
        Total == 0 ? 0 : (double)ShadowClippedPixels / Total;

    public double HighlightClippedFraction =>
        Total == 0 ? 0 : (double)HighlightClippedPixels / Total;

    public static HistogramData FromBitmap(BitmapSource source) =>
        FromBuffer(PixelBuffer.FromBitmap(source));

    public static HistogramData FromBuffer(PixelBuffer buffer)
    {
        var red = new int[BinCount];
        var green = new int[BinCount];
        var blue = new int[BinCount];
        var luminance = new int[BinCount];
        long shadowClipped = 0;
        long highlightClipped = 0;

        var pixels = buffer.Pixels;
        var width = buffer.Width;
        var height = buffer.Height;

        // Large frames are sampled: a histogram is a shape, and every fourth
        // pixel of a 24 megapixel photo already describes it exactly.
        var step = width * height > 4_000_000 ? 2 : 1;
        long total = 0;

        for (var row = 0; row < height; row += step)
        {
            var offset = row * width * PixelBuffer.BytesPerPixel;
            for (var column = 0; column < width; column += step)
            {
                var index = offset + (column * PixelBuffer.BytesPerPixel);
                var b = pixels[index];
                var g = pixels[index + 1];
                var r = pixels[index + 2];
                red[r]++;
                green[g]++;
                blue[b]++;
                var lum = (int)((0.2126 * r) + (0.7152 * g) + (0.0722 * b) + 0.5);
                luminance[Math.Clamp(lum, 0, 255)]++;
                total++;

                if (r <= 1 && g <= 1 && b <= 1)
                {
                    shadowClipped++;
                }
                else if (r >= 254 || g >= 254 || b >= 254)
                {
                    highlightClipped++;
                }
            }
        }

        return new HistogramData(
            red,
            green,
            blue,
            luminance,
            total,
            shadowClipped,
            highlightClipped);
    }

    /// <summary>
    /// The luminance level below which <paramref name="fraction"/> of the
    /// photo's pixels sit. Auto Fix uses this to find a real black and white
    /// point without being fooled by a handful of stray specular pixels.
    /// </summary>
    public int GetLuminancePercentile(double fraction)
    {
        if (Total == 0)
        {
            return 0;
        }

        var target = Total * Math.Clamp(fraction, 0, 1);
        long running = 0;
        for (var bin = 0; bin < BinCount; bin++)
        {
            running += Luminance[bin];
            if (running >= target)
            {
                return bin;
            }
        }

        return BinCount - 1;
    }

    public double GetMeanLuminance()
    {
        if (Total == 0)
        {
            return 0;
        }

        long sum = 0;
        for (var bin = 0; bin < BinCount; bin++)
        {
            sum += (long)bin * Luminance[bin];
        }

        return (double)sum / Total / 255;
    }

    public double GetChannelMean(int[] channel)
    {
        if (Total == 0)
        {
            return 0;
        }

        long sum = 0;
        for (var bin = 0; bin < BinCount; bin++)
        {
            sum += (long)bin * channel[bin];
        }

        return (double)sum / Total;
    }
}

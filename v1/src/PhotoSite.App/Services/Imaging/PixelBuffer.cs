using System.Windows.Media;
using System.Windows.Media.Imaging;

namespace PhotoSite.Services.Imaging;

/// <summary>
/// A plain BGRA byte surface. Every pixel operation in PhotoSite works on this
/// instead of on a <see cref="BitmapSource"/> so a whole chain of adjustments
/// and filters costs exactly one decode and one encode.
/// </summary>
internal sealed class PixelBuffer
{
    public const int BytesPerPixel = 4;

    private PixelBuffer(int width, int height, byte[] pixels, double dpiX, double dpiY)
    {
        Width = width;
        Height = height;
        Pixels = pixels;
        DpiX = dpiX;
        DpiY = dpiY;
    }

    public int Width { get; }

    public int Height { get; }

    public byte[] Pixels { get; }

    public double DpiX { get; }

    public double DpiY { get; }

    public int Stride => Width * BytesPerPixel;

    public static PixelBuffer FromBitmap(BitmapSource source)
    {
        ArgumentNullException.ThrowIfNull(source);
        var converted = source.Format == PixelFormats.Bgra32
            ? source
            : ConvertFormat(source);
        var width = converted.PixelWidth;
        var height = converted.PixelHeight;
        var pixels = new byte[width * height * BytesPerPixel];
        converted.CopyPixels(pixels, width * BytesPerPixel, 0);
        return new PixelBuffer(
            width,
            height,
            pixels,
            source.DpiX > 0 ? source.DpiX : 96,
            source.DpiY > 0 ? source.DpiY : 96);
    }

    public static PixelBuffer Create(int width, int height, double dpiX = 96, double dpiY = 96) =>
        new(
            width,
            height,
            new byte[width * height * BytesPerPixel],
            dpiX,
            dpiY);

    public PixelBuffer CloneEmpty() =>
        Create(Width, Height, DpiX, DpiY);

    public PixelBuffer Clone()
    {
        var clone = CloneEmpty();
        Buffer.BlockCopy(Pixels, 0, clone.Pixels, 0, Pixels.Length);
        return clone;
    }

    public BitmapSource ToBitmap()
    {
        var bitmap = BitmapSource.Create(
            Width,
            Height,
            DpiX,
            DpiY,
            PixelFormats.Bgra32,
            null,
            Pixels,
            Stride);
        bitmap.Freeze();
        return bitmap;
    }

    /// <summary>
    /// Converting through WIC also divides out the alpha of a premultiplied
    /// surface such as the <see cref="RenderTargetBitmap"/> the geometry stage
    /// produces. Adjusting premultiplied channels directly would darken every
    /// semi-transparent pixel of a rotated frame.
    /// </summary>
    private static BitmapSource ConvertFormat(BitmapSource source)
    {
        var converted = new FormatConvertedBitmap(
            source,
            PixelFormats.Bgra32,
            null,
            0);
        converted.Freeze();
        return converted;
    }
}

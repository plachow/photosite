using System.Windows.Media.Imaging;

namespace PhotoSite.Services;

/// <summary>
/// Opens camera RAW files.
/// </summary>
/// <remarks>
/// Windows can decode many RAW formats through WIC, but only when the vendor
/// codec or Microsoft's Raw Image Extension happens to be installed, which
/// cannot be relied on. Every RAW container also carries a full-size JPEG
/// preview - it is what the camera shows on its own screen - so when WIC
/// declines, that preview is extracted instead. The result is that RAW files
/// always browse, preview and export, with the caveat that the fallback path
/// is the camera's rendering rather than a re-development of the sensor data.
/// </remarks>
internal static class RawImageDecoder
{
    private static readonly HashSet<string> RawExtensions =
        new(StringComparer.OrdinalIgnoreCase)
        {
            ".dng", ".cr2", ".cr3", ".nef", ".arw", ".orf",
            ".rw2", ".raf", ".pef", ".srw", ".raw", ".rwl"
        };

    /// <summary>
    /// Below this the embedded image is a thumbnail rather than the preview,
    /// and using it would give a blurry "full size" photo.
    /// </summary>
    private const int MinimumPreviewBytes = 24 * 1024;

    public static bool IsRaw(string path) =>
        RawExtensions.Contains(Path.GetExtension(path));

    /// <summary>
    /// Returns the embedded preview as a decodable JPEG, or null when the file
    /// carries none.
    /// </summary>
    public static byte[]? ExtractEmbeddedJpeg(string path)
    {
        try
        {
            var bytes = File.ReadAllBytes(path);
            return ExtractLargestJpeg(bytes);
        }
        catch (Exception exception) when (
            exception is IOException
                or UnauthorizedAccessException
                or OutOfMemoryException)
        {
            return null;
        }
    }

    /// <summary>
    /// Scans for embedded JPEG streams and returns the largest complete one.
    /// </summary>
    /// <remarks>
    /// Deliberately format-agnostic rather than parsing each vendor's
    /// container: CR2, NEF, ARW, ORF, RW2 and DNG are all TIFF variants with
    /// different preview tags, while CR3 and RAF are not TIFF at all. Looking
    /// for the markers themselves handles every one of them, and the largest
    /// stream is always the display preview rather than the small thumbnail.
    /// </remarks>
    internal static byte[]? ExtractLargestJpeg(ReadOnlySpan<byte> bytes)
    {
        byte[]? best = null;
        var index = 0;

        while (index < bytes.Length - 3)
        {
            if (bytes[index] != 0xFF
                || bytes[index + 1] != 0xD8
                || bytes[index + 2] != 0xFF)
            {
                index++;
                continue;
            }

            var end = FindEndOfImage(bytes, index + 2);
            if (end < 0)
            {
                break;
            }

            var length = end - index;
            if (length >= MinimumPreviewBytes
                && (best is null || length > best.Length))
            {
                best = bytes.Slice(index, length).ToArray();
            }

            // Continue past this stream: a RAW routinely holds a thumbnail,
            // a medium preview and a full-size one.
            index = end;
        }

        return best;
    }

    /// <summary>
    /// Walks the JPEG segment table to the end-of-image marker. Skipping
    /// segment payloads matters: entropy-coded data and thumbnails contain
    /// byte pairs that look exactly like markers.
    /// </summary>
    private static int FindEndOfImage(ReadOnlySpan<byte> bytes, int start)
    {
        var index = start;
        while (index < bytes.Length - 1)
        {
            if (bytes[index] != 0xFF)
            {
                index++;
                continue;
            }

            var marker = bytes[index + 1];
            switch (marker)
            {
                case 0xD8:
                case 0x01:
                case 0xFF:
                case >= 0xD0 and <= 0xD7:
                    index += 2;
                    continue;
                case 0xD9:
                    return index + 2;
                case 0xDA:
                    // Start of scan: the compressed data follows, in which
                    // only a real marker is not preceded by a stuffed zero.
                    index = SkipEntropyCodedData(bytes, index + 2);
                    continue;
                default:
                    if (index + 3 >= bytes.Length)
                    {
                        return -1;
                    }

                    var segmentLength = (bytes[index + 2] << 8) | bytes[index + 3];
                    if (segmentLength < 2)
                    {
                        return -1;
                    }

                    index += 2 + segmentLength;
                    continue;
            }
        }

        return -1;
    }

    private static int SkipEntropyCodedData(ReadOnlySpan<byte> bytes, int start)
    {
        var index = start;
        while (index < bytes.Length - 1)
        {
            if (bytes[index] != 0xFF)
            {
                index++;
                continue;
            }

            var next = bytes[index + 1];
            if (next == 0x00 || (next >= 0xD0 && next <= 0xD7))
            {
                index += 2;
                continue;
            }

            return index;
        }

        return bytes.Length;
    }

    /// <summary>
    /// Decodes a RAW file to a bitmap, preferring a real WIC decode and
    /// falling back to the embedded preview.
    /// </summary>
    public static BitmapSource? TryDecode(
        string path,
        int decodePixelWidth,
        Rotation rotation)
    {
        if (TryDecodeWithWic(path, decodePixelWidth, rotation) is { } decoded)
        {
            return decoded;
        }

        var preview = ExtractEmbeddedJpeg(path);
        return preview is null
            ? null
            : DecodeBytes(preview, decodePixelWidth, rotation);
    }

    private static BitmapSource? TryDecodeWithWic(
        string path,
        int decodePixelWidth,
        Rotation rotation)
    {
        try
        {
            using var stream = new FileStream(
                path,
                FileMode.Open,
                FileAccess.Read,
                FileShare.ReadWrite | FileShare.Delete,
                128 * 1024,
                FileOptions.SequentialScan);
            var image = new BitmapImage();
            image.BeginInit();
            image.CacheOption = BitmapCacheOption.OnLoad;
            image.CreateOptions = BitmapCreateOptions.PreservePixelFormat;
            if (decodePixelWidth > 0)
            {
                image.DecodePixelWidth = Math.Max(256, decodePixelWidth);
            }

            image.Rotation = rotation;
            image.StreamSource = stream;
            image.EndInit();
            image.Freeze();
            return image;
        }
        catch (Exception exception) when (
            exception is NotSupportedException
                or FileFormatException
                or ArgumentException
                or InvalidOperationException
                or OverflowException)
        {
            // No codec on this machine for this camera; use the preview.
            return null;
        }
    }

    private static BitmapSource? DecodeBytes(
        byte[] jpeg,
        int decodePixelWidth,
        Rotation rotation)
    {
        try
        {
            using var stream = new MemoryStream(jpeg, writable: false);
            var image = new BitmapImage();
            image.BeginInit();
            image.CacheOption = BitmapCacheOption.OnLoad;
            image.CreateOptions = BitmapCreateOptions.PreservePixelFormat;
            if (decodePixelWidth > 0)
            {
                image.DecodePixelWidth = Math.Max(256, decodePixelWidth);
            }

            image.Rotation = rotation;
            image.StreamSource = stream;
            image.EndInit();
            image.Freeze();
            return image;
        }
        catch (Exception exception) when (
            exception is NotSupportedException
                or FileFormatException
                or ArgumentException
                or InvalidOperationException)
        {
            return null;
        }
    }
}

using System.Collections.Concurrent;
using System.IO;
using System.Windows.Media.Imaging;

namespace PhotoSite.Services.Imaging;

/// <summary>
/// The decoded pixels of image layers, keyed by path. A layer is drawn on
/// every canvas frame and again on export, and decoding a PNG each time
/// would make dragging one stutter; a handful of frozen bitmaps is cheap
/// to keep. A file that changes on disk is re-read.
/// </summary>
internal static class ImageLayerCache
{
    private const int Capacity = 12;

    private static readonly ConcurrentDictionary<string, (DateTime WriteTime, BitmapSource Bitmap)> Entries =
        new(StringComparer.OrdinalIgnoreCase);

    /// <summary>The bitmap for a path, or null when the file cannot be read.</summary>
    public static BitmapSource? TryLoad(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            return null;
        }

        DateTime writeTime;
        try
        {
            if (!File.Exists(path))
            {
                Entries.TryRemove(path, out _);
                return null;
            }

            writeTime = File.GetLastWriteTimeUtc(path);
        }
        catch (IOException)
        {
            return null;
        }
        catch (UnauthorizedAccessException)
        {
            return null;
        }

        if (Entries.TryGetValue(path, out var cached) && cached.WriteTime == writeTime)
        {
            return cached.Bitmap;
        }

        try
        {
            // The same decode the canvas uses, so a phone photo comes in
            // the right way up and a RAW arrives as its embedded preview.
            var bitmap = PreviewService.Load(path, 0, CancellationToken.None);
            if (!bitmap.IsFrozen && bitmap.CanFreeze)
            {
                bitmap.Freeze();
            }

            if (Entries.Count >= Capacity)
            {
                // Not an LRU, deliberately: a dozen entries is more than one
                // photograph ever carries, so evicting anything is rare.
                Entries.Clear();
            }

            Entries[path] = (writeTime, bitmap);
            return bitmap;
        }
        catch (Exception exception) when (exception is IOException
                                              or NotSupportedException
                                              or UnauthorizedAccessException
                                              or System.Runtime.InteropServices.COMException
                                              or MetadataExtractor.ImageProcessingException)
        {
            return null;
        }
    }

    /// <summary>The pixel size of an image file, or null when it cannot be read.</summary>
    public static (int Width, int Height)? Measure(string path) =>
        TryLoad(path) is { } bitmap ? (bitmap.PixelWidth, bitmap.PixelHeight) : null;
}

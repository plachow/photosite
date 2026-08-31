using System.Drawing;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using PhotoSauce.MagicScaler;
using PhotoSauce.NativeCodecs.Libwebp;
using PhotoSite.Domain;

namespace PhotoSite.Services;

/// <summary>
/// The single place PhotoSite turns pixels into a file.
/// </summary>
/// <remarks>
/// WPF's own encoders cover JPEG, PNG, TIFF and BMP; WebP has no WIC encoder
/// that can be relied on, so those go through MagicScaler's bundled libwebp.
/// Both paths write to a temporary file first and move it into place, so an
/// interrupted export can never leave a half-written photograph behind.
/// </remarks>
internal static class ImageEncoder
{
    private static int codecsRegistered;

    public static void EnsureCodecsRegistered()
    {
        if (Interlocked.Exchange(ref codecsRegistered, 1) == 1)
        {
            return;
        }

        CodecManager.Configure(codecs =>
        {
            codecs.UseWicCodecs(WicCodecPolicy.All);
            codecs.UseLibwebp();
        });
    }

    public static ImageOutputFormat ResolveFormat(
        ImageOutputFormat requested,
        string sourceExtension) =>
        requested == ImageOutputFormat.KeepOriginal
            ? FromExtension(sourceExtension)
            : requested;

    public static ImageOutputFormat FromExtension(string extension) =>
        extension.ToLowerInvariant() switch
        {
            ".png" => ImageOutputFormat.Png,
            ".webp" => ImageOutputFormat.WebP,
            ".tif" or ".tiff" => ImageOutputFormat.Tiff,
            ".bmp" => ImageOutputFormat.Bmp,
            _ => ImageOutputFormat.Jpeg
        };

    public static bool CanEncodeExtension(string extension) =>
        extension.ToLowerInvariant() is ".jpg" or ".jpeg" or ".png"
            or ".webp" or ".tif" or ".tiff" or ".bmp";

    public static void Write(
        BitmapSource image,
        string destinationPath,
        ImageOutputFormat format,
        int quality,
        string? metadataSourcePath = null,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(image);
        var directory = Path.GetDirectoryName(Path.GetFullPath(destinationPath))
            ?? throw new InvalidOperationException(
                "The destination folder is unavailable.");
        Directory.CreateDirectory(directory);

        var temporaryPath = Path.Combine(
            directory,
            $".{Path.GetFileName(destinationPath)}.{Guid.NewGuid():N}.tmp");
        try
        {
            if (format == ImageOutputFormat.WebP)
            {
                WriteWebP(image, temporaryPath, quality);
            }
            else
            {
                WriteWithWpf(
                    image,
                    temporaryPath,
                    format,
                    quality,
                    metadataSourcePath,
                    destinationPath);
            }

            cancellationToken.ThrowIfCancellationRequested();
            if (File.Exists(destinationPath))
            {
                File.Replace(temporaryPath, destinationPath, null);
            }
            else
            {
                File.Move(temporaryPath, destinationPath);
            }
        }
        finally
        {
            if (File.Exists(temporaryPath))
            {
                File.Delete(temporaryPath);
            }
        }
    }

    private static void WriteWebP(
        BitmapSource image,
        string path,
        int quality)
    {
        EnsureCodecsRegistered();
        var settings = new ProcessImageSettings();
        settings.TrySetEncoderFormat(ImageMimeTypes.Webp);
        settings.EncoderOptions =
            new WebpLossyEncoderOptions(Math.Clamp(quality, 1, 100));
        using var stream = new FileStream(
            path,
            FileMode.CreateNew,
            FileAccess.Write,
            FileShare.None,
            128 * 1024,
            FileOptions.WriteThrough);
        MagicImageProcessor.ProcessImage(
            new BitmapPixelSource(image),
            stream,
            settings);
        stream.Flush(flushToDisk: true);
    }

    private static void WriteWithWpf(
        BitmapSource image,
        string path,
        ImageOutputFormat format,
        int quality,
        string? metadataSourcePath,
        string destinationPath)
    {
        var frame = CreateFrame(image, metadataSourcePath, destinationPath);
        try
        {
            WriteEncodedFile(path, CreateEncoder(format, quality), frame);
        }
        catch (NotSupportedException)
        {
            // Some source metadata blocks cannot be re-encoded into the target
            // container; the pixels still have to reach the disk.
            if (File.Exists(path))
            {
                File.Delete(path);
            }

            WriteEncodedFile(
                path,
                CreateEncoder(format, quality),
                BitmapFrame.Create(image));
        }
    }

    private static void WriteEncodedFile(
        string path,
        BitmapEncoder encoder,
        BitmapFrame frame)
    {
        encoder.Frames.Add(frame);
        using var stream = new FileStream(
            path,
            FileMode.CreateNew,
            FileAccess.Write,
            FileShare.None,
            128 * 1024,
            FileOptions.WriteThrough);
        encoder.Save(stream);
        stream.Flush(flushToDisk: true);
    }

    private static BitmapEncoder CreateEncoder(
        ImageOutputFormat format,
        int quality) =>
        format switch
        {
            ImageOutputFormat.Jpeg => new JpegBitmapEncoder
            {
                QualityLevel = Math.Clamp(quality, 1, 100)
            },
            ImageOutputFormat.Png => new PngBitmapEncoder(),
            ImageOutputFormat.Tiff => new TiffBitmapEncoder
            {
                Compression = TiffCompressOption.Zip
            },
            ImageOutputFormat.Bmp => new BmpBitmapEncoder(),
            _ => throw new NotSupportedException(
                $"Saving {format} files is not supported.")
        };

    /// <summary>
    /// Carries the source metadata across when the container type is the same,
    /// resetting the orientation tag because the pixels have already been
    /// rotated - leaving it would rotate the photograph a second time.
    /// </summary>
    private static BitmapFrame CreateFrame(
        BitmapSource image,
        string? sourcePath,
        string destinationPath)
    {
        if (string.IsNullOrWhiteSpace(sourcePath)
            || !File.Exists(sourcePath)
            || !string.Equals(
                Path.GetExtension(sourcePath),
                Path.GetExtension(destinationPath),
                StringComparison.OrdinalIgnoreCase))
        {
            return BitmapFrame.Create(image);
        }

        try
        {
            using var stream = new FileStream(
                sourcePath,
                FileMode.Open,
                FileAccess.Read,
                FileShare.ReadWrite | FileShare.Delete);
            var decoder = BitmapDecoder.Create(
                stream,
                BitmapCreateOptions.PreservePixelFormat,
                BitmapCacheOption.OnLoad);
            var sourceFrame = decoder.Frames[0];
            var metadata = sourceFrame.Metadata is BitmapMetadata sourceMetadata
                ? sourceMetadata.Clone() as BitmapMetadata
                : null;
            if (metadata is not null && !metadata.IsReadOnly)
            {
                TrySetOrientation(metadata, "/app1/ifd/{ushort=274}");
                TrySetOrientation(metadata, "/ifd/{ushort=274}");
            }

            return BitmapFrame.Create(
                image,
                null,
                metadata,
                sourceFrame.ColorContexts);
        }
        catch
        {
            return BitmapFrame.Create(image);
        }
    }

    private static void TrySetOrientation(
        BitmapMetadata metadata,
        string query)
    {
        try
        {
            metadata.SetQuery(query, (ushort)1);
        }
        catch
        {
            // Not every container exposes this block for writing.
        }
    }

    /// <summary>
    /// Bridges a WPF bitmap into MagicScaler's pipeline so the WebP encoder
    /// can consume edited pixels that never existed as a file.
    /// </summary>
    private sealed class BitmapPixelSource : IPixelSource
    {
        private readonly BitmapSource source;

        public BitmapPixelSource(BitmapSource source)
        {
            this.source = source.Format == System.Windows.Media.PixelFormats.Bgra32
                ? source
                : Convert(source);
        }

        public Guid Format => PhotoSauce.MagicScaler.PixelFormats.Bgra32bpp;

        public int Width => source.PixelWidth;

        public int Height => source.PixelHeight;

        public void CopyPixels(
            Rectangle sourceArea,
            int cbStride,
            Span<byte> buffer)
        {
            var rectangle = new System.Windows.Int32Rect(
                sourceArea.X,
                sourceArea.Y,
                sourceArea.Width,
                sourceArea.Height);
            var bytes = new byte[cbStride * sourceArea.Height];
            source.CopyPixels(rectangle, bytes, cbStride, 0);
            bytes.AsSpan(0, Math.Min(bytes.Length, buffer.Length)).CopyTo(buffer);
        }

        private static BitmapSource Convert(BitmapSource source)
        {
            var converted = new FormatConvertedBitmap(
                source,
                System.Windows.Media.PixelFormats.Bgra32,
                null,
                0);
            converted.Freeze();
            return converted;
        }
    }
}

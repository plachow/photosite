using System.Windows.Media.Imaging;
using MetadataExtractor;
using MetadataExtractor.Formats.Exif;

namespace PhotoSite.Services;

public sealed class PreviewService
{
    public Task<BitmapSource> LoadAsync(
        string path,
        int decodePixelWidth,
        CancellationToken cancellationToken)
    {
        return Task.Run(
            () => LoadCore(path, decodePixelWidth, cancellationToken),
            cancellationToken);
    }

    private static BitmapSource LoadCore(
        string path,
        int decodePixelWidth,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var rotation = ReadExifRotation(path);

        if (RawImageDecoder.IsRaw(path))
        {
            return RawImageDecoder.TryDecode(path, decodePixelWidth, rotation)
                   ?? throw new NotSupportedException(
                       "This RAW file carries no readable preview, and no "
                       + "codec for it is installed on this computer.");
        }

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
        cancellationToken.ThrowIfCancellationRequested();
        return image;
    }

    private static Rotation ReadExifRotation(string path)
    {
        try
        {
            var directories = ImageMetadataReader.ReadMetadata(path);
            var directory = directories.OfType<ExifIfd0Directory>().FirstOrDefault();
            if (directory?.TryGetInt32(ExifDirectoryBase.TagOrientation, out var value) != true)
            {
                return Rotation.Rotate0;
            }

            return value switch
            {
                3 => Rotation.Rotate180,
                6 => Rotation.Rotate90,
                8 => Rotation.Rotate270,
                _ => Rotation.Rotate0
            };
        }
        catch (ImageProcessingException)
        {
            return Rotation.Rotate0;
        }
        catch (IOException)
        {
            return Rotation.Rotate0;
        }
    }
}

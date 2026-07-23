using System.Security.Cryptography;
using System.Text;
using PhotoSauce.MagicScaler;

namespace PhotoSite.Services;

public sealed class ThumbnailService
{
    private readonly string cacheDirectory;

    public ThumbnailService(string cacheDirectory)
    {
        this.cacheDirectory = cacheDirectory;
    }

    public Task<string> GetOrCreateAsync(
        string sourcePath,
        int width,
        int height,
        CancellationToken cancellationToken)
    {
        return Task.Run(
            () => GetOrCreateCore(sourcePath, width, height, cancellationToken),
            cancellationToken);
    }

    private string GetOrCreateCore(
        string sourcePath,
        int width,
        int height,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var file = new FileInfo(sourcePath);
        var key =
            $"{file.FullName}|{file.Length}|{file.LastWriteTimeUtc.Ticks}|{width}|{height}|v1";
        var hash = Convert.ToHexString(
            SHA256.HashData(Encoding.UTF8.GetBytes(key)));
        var directory = Path.Combine(cacheDirectory, hash[..2]);
        var destination = Path.Combine(directory, $"{hash}.jpg");

        if (File.Exists(destination))
        {
            return destination;
        }

        Directory.CreateDirectory(directory);
        var temporary = Path.Combine(directory, $"{hash}.{Guid.NewGuid():N}.tmp");

        try
        {
            var settings = new ProcessImageSettings
            {
                Width = width,
                Height = height,
                ResizeMode = CropScaleMode.Crop,
                HybridMode = HybridScaleMode.FavorSpeed,
                Sharpen = true
            };
            settings.TrySetEncoderFormat(ImageMimeTypes.Jpeg);
            MagicImageProcessor.ProcessImage(sourcePath, temporary, settings);
            cancellationToken.ThrowIfCancellationRequested();
            File.Move(temporary, destination, true);
            return destination;
        }
        finally
        {
            if (File.Exists(temporary))
            {
                File.Delete(temporary);
            }
        }
    }
}

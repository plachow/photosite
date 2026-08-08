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

    /// <summary>
    /// Resolves an already generated thumbnail without touching the thread
    /// pool, so a tile whose bitmap is still in memory can be painted in the
    /// same layout pass instead of blanking for a dispatcher turn.
    /// </summary>
    public bool TryGetCached(
        string sourcePath,
        int width,
        int height,
        out string thumbnailPath)
    {
        thumbnailPath = string.Empty;
        try
        {
            var file = new FileInfo(sourcePath);
            if (!file.Exists)
            {
                return false;
            }

            var destination = ResolveDestination(file, width, height);
            if (!File.Exists(destination))
            {
                return false;
            }

            thumbnailPath = destination;
            return true;
        }
        catch (Exception exception) when (
            exception is IOException
            or UnauthorizedAccessException
            or ArgumentException)
        {
            return false;
        }
    }

    private string ResolveDestination(FileInfo file, int width, int height)
    {
        var key =
            $"{file.FullName}|{file.Length}|{file.LastWriteTimeUtc.Ticks}|{width}|{height}|v2";
        var hash = Convert.ToHexString(
            SHA256.HashData(Encoding.UTF8.GetBytes(key)));
        return Path.Combine(cacheDirectory, hash[..2], $"{hash}.jpg");
    }

    private string GetOrCreateCore(
        string sourcePath,
        int width,
        int height,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var file = new FileInfo(sourcePath);
        var destination = ResolveDestination(file, width, height);
        var directory = Path.GetDirectoryName(destination)!;

        if (File.Exists(destination))
        {
            return destination;
        }

        Directory.CreateDirectory(directory);
        var hash = Path.GetFileNameWithoutExtension(destination);
        var temporary = Path.Combine(directory, $"{hash}.{Guid.NewGuid():N}.tmp");

        try
        {
            var settings = new ProcessImageSettings
            {
                Width = width,
                Height = height,
                // Max keeps the whole frame (no crop) so portrait photos
                // survive with their full height; the tile decides the fit.
                ResizeMode = CropScaleMode.Max,
                HybridMode = HybridScaleMode.FavorSpeed,
                Sharpen = true
            };
            settings.TrySetEncoderFormat(ImageMimeTypes.Jpeg);
            ProcessToFile(sourcePath, temporary, settings);
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

    /// <summary>
    /// A RAW file without an installed codec cannot be scaled directly, so the
    /// embedded camera preview is scaled instead. Without this, a folder from
    /// a camera would show nothing but grey tiles.
    /// </summary>
    private static void ProcessToFile(
        string sourcePath,
        string destinationPath,
        ProcessImageSettings settings)
    {
        try
        {
            MagicImageProcessor.ProcessImage(sourcePath, destinationPath, settings);
            return;
        }
        catch (Exception exception) when (
            RawImageDecoder.IsRaw(sourcePath)
            && exception is not (IOException or UnauthorizedAccessException))
        {
            if (File.Exists(destinationPath))
            {
                File.Delete(destinationPath);
            }
        }

        var preview = RawImageDecoder.ExtractEmbeddedJpeg(sourcePath)
            ?? throw new NotSupportedException(
                $"No readable preview was found in {Path.GetFileName(sourcePath)}.");
        MagicImageProcessor.ProcessImage(preview, destinationPath, settings);
    }
}

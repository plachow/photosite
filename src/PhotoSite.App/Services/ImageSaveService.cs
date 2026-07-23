using System.Windows.Media.Imaging;
using PhotoSite.Controls;
using PhotoSite.Domain;

namespace PhotoSite.Services;

public sealed class ImageSaveService
{
    public const int DefaultPastedJpegQuality = 90;

    private static readonly HashSet<string> WritableExtensions =
        new(StringComparer.OrdinalIgnoreCase)
        {
            ".jpg", ".jpeg", ".png", ".tif", ".tiff", ".bmp"
        };

    private readonly PreviewService previews;

    public ImageSaveService(PreviewService previews)
    {
        this.previews = previews;
    }

    public bool CanOverwrite(string path) =>
        WritableExtensions.Contains(Path.GetExtension(path));

    public async Task SaveAsync(
        string sourcePath,
        string destinationPath,
        EditRecipe recipe,
        bool overwrite,
        CancellationToken cancellationToken = default)
    {
        var source = await previews.LoadAsync(
            sourcePath,
            0,
            cancellationToken);
        var crop = recipe.Crop?.ConstrainToUnit()
            ?? new CropRegion(0, 0, 1, 1);
        var rendered = PhotoViewer.RenderSelection(
            source,
            recipe,
            crop);
        await Task.Run(
            () => WriteAtomically(
                rendered,
                sourcePath,
                destinationPath,
                overwrite,
                jpegQuality: 100,
                cancellationToken),
            cancellationToken);
    }

    public async Task<BitmapSource> SaveAsync(
        BitmapSource source,
        string destinationPath,
        EditRecipe recipe,
        bool overwrite,
        int jpegQuality = DefaultPastedJpegQuality,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(source);
        var crop = recipe.Crop?.ConstrainToUnit()
            ?? new CropRegion(0, 0, 1, 1);
        var rendered = PhotoViewer.RenderSelection(
            source,
            recipe,
            crop);
        await Task.Run(
            () => WriteAtomically(
                rendered,
                sourcePath: null,
                destinationPath,
                overwrite,
                jpegQuality,
                cancellationToken),
            cancellationToken);
        return rendered;
    }

    public static string BuildVersionCopyPath(string sourcePath)
    {
        var directory = Path.GetDirectoryName(sourcePath)
            ?? Environment.CurrentDirectory;
        var name = Path.GetFileNameWithoutExtension(sourcePath);
        var extension = WritableExtensions.Contains(Path.GetExtension(sourcePath))
            ? Path.GetExtension(sourcePath)
            : ".png";
        for (var version = 1; ; version++)
        {
            var candidate = Path.Combine(
                directory,
                $"{name} ver {version}{extension}");
            if (!File.Exists(candidate))
            {
                return candidate;
            }
        }
    }

    private static void WriteAtomically(
        BitmapSource image,
        string? sourcePath,
        string destinationPath,
        bool overwrite,
        int jpegQuality,
        CancellationToken cancellationToken)
    {
        var extension = Path.GetExtension(destinationPath);
        var directory = Path.GetDirectoryName(destinationPath)
            ?? throw new InvalidOperationException(
                "The destination folder is unavailable.");
        Directory.CreateDirectory(directory);
        var temporaryPath = Path.Combine(
            directory,
            $".{Path.GetFileName(destinationPath)}.{Guid.NewGuid():N}.tmp");
        try
        {
            try
            {
                WriteEncodedFile(
                    temporaryPath,
                    CreateEncoder(extension, jpegQuality),
                    CreateFrameWithMetadata(
                        image,
                        sourcePath,
                        destinationPath));
            }
            catch (NotSupportedException)
            {
                if (File.Exists(temporaryPath))
                {
                    File.Delete(temporaryPath);
                }

                WriteEncodedFile(
                    temporaryPath,
                    CreateEncoder(extension, jpegQuality),
                    BitmapFrame.Create(image));
            }

            cancellationToken.ThrowIfCancellationRequested();
            if (overwrite && File.Exists(destinationPath))
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
        string extension,
        int jpegQuality) =>
        extension.ToLowerInvariant() switch
        {
            ".jpg" or ".jpeg" => new JpegBitmapEncoder
            {
                QualityLevel = Math.Clamp(jpegQuality, 1, 100)
            },
            ".png" => new PngBitmapEncoder(),
            ".tif" or ".tiff" => new TiffBitmapEncoder
            {
                Compression = TiffCompressOption.Zip
            },
            ".bmp" => new BmpBitmapEncoder(),
            _ => throw new NotSupportedException(
                $"Saving {extension} files is not supported yet.")
        };

    private static BitmapFrame CreateFrameWithMetadata(
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
        }
    }
}

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
            ".jpg", ".jpeg", ".png", ".tif", ".tiff", ".bmp", ".webp"
        };

    private readonly PreviewService previews;

    public ImageSaveService(PreviewService previews)
    {
        this.previews = previews;
    }

    /// <summary>
    /// A RAW file is never written back over: PhotoSite renders it, and the
    /// render is not a RAW.
    /// </summary>
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

    /// <summary>
    /// Delegates to the shared encoder so Save As, Export and a batch run all
    /// write files exactly the same way, including the atomic replace and the
    /// reset orientation tag.
    /// </summary>
    private static void WriteAtomically(
        BitmapSource image,
        string? sourcePath,
        string destinationPath,
        bool overwrite,
        int jpegQuality,
        CancellationToken cancellationToken)
    {
        if (!overwrite && File.Exists(destinationPath))
        {
            throw new IOException(
                $"The destination file already exists: {destinationPath}");
        }

        ImageEncoder.Write(
            image,
            destinationPath,
            ImageEncoder.FromExtension(Path.GetExtension(destinationPath)),
            jpegQuality,
            sourcePath,
            cancellationToken);
    }
}

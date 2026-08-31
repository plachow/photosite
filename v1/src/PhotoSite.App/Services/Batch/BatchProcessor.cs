using System.Windows.Media.Imaging;
using PhotoSite.Domain;
using PhotoSite.Services.Imaging;

namespace PhotoSite.Services.Batch;

internal sealed record BatchProgress(
    int Completed,
    int Total,
    int Failed,
    string CurrentFile)
{
    public double Fraction => Total == 0 ? 0 : (double)Completed / Total;
}

internal sealed record BatchOutcome(
    int Written,
    int Skipped,
    int Failed,
    IReadOnlyList<string> Errors,
    TimeSpan Elapsed,
    bool Cancelled)
{
    public string Describe()
    {
        var parts = new List<string> { $"{Written:N0} written" };
        if (Skipped > 0)
        {
            parts.Add($"{Skipped:N0} skipped");
        }

        if (Failed > 0)
        {
            parts.Add($"{Failed:N0} failed");
        }

        parts.Add($"{Elapsed.TotalSeconds:0.#} s");
        return (Cancelled ? "Cancelled · " : string.Empty)
               + string.Join(" · ", parts);
    }
}

/// <summary>
/// Runs a planned batch. Work happens entirely off the dispatcher and reports
/// through <see cref="IProgress{T}"/>, so converting hundreds of RAW files
/// never freezes the gallery behind it.
/// </summary>
internal sealed class BatchProcessor
{
    private readonly PreviewService previews;
    private readonly ExifToolMetadataWriter metadataWriter;

    public BatchProcessor(
        PreviewService previews,
        ExifToolMetadataWriter metadataWriter)
    {
        this.previews = previews;
        this.metadataWriter = metadataWriter;
    }

    public async Task<BatchOutcome> RunAsync(
        BatchPlan plan,
        BatchPreset preset,
        IProgress<BatchProgress>? progress = null,
        CancellationToken cancellationToken = default)
    {
        var startedAt = DateTime.UtcNow;
        var work = plan.Items.Where(item => !item.IsSkipped).ToArray();
        var errors = new List<string>();
        var written = 0;
        var failed = 0;
        var cancelled = false;
        var metadataPairs = new List<(string Source, string Destination)>();

        for (var index = 0; index < work.Length; index++)
        {
            if (cancellationToken.IsCancellationRequested)
            {
                cancelled = true;
                break;
            }

            var item = work[index];
            progress?.Report(new BatchProgress(
                index,
                work.Length,
                failed,
                Path.GetFileName(item.Source.Path)));

            try
            {
                await Task.Run(
                    () => ConvertOne(item, preset, cancellationToken),
                    cancellationToken);
                written++;
                if (preset.MetadataPolicy != BatchMetadataPolicy.RemoveAll)
                {
                    metadataPairs.Add((item.Source.Path, item.DestinationPath));
                }
            }
            catch (OperationCanceledException)
            {
                cancelled = true;
                break;
            }
            catch (Exception exception)
            {
                failed++;
                errors.Add(
                    $"{Path.GetFileName(item.Source.Path)}: {exception.Message}");
            }
        }

        // One exiftool run for the whole batch instead of one per photograph;
        // a per-file process would add minutes to a large conversion.
        if (metadataPairs.Count > 0 && metadataWriter.IsAvailable)
        {
            var result = await metadataWriter.CopyMetadataBatchAsync(
                metadataPairs,
                preset.MetadataPolicy == BatchMetadataPolicy.RemoveLocation,
                CancellationToken.None);
            if (!result.Success)
            {
                errors.Add($"Metadata copy: {result.Error}");
            }
        }

        progress?.Report(new BatchProgress(
            work.Length,
            work.Length,
            failed,
            string.Empty));

        return new BatchOutcome(
            written,
            plan.SkipCount,
            failed,
            errors,
            DateTime.UtcNow - startedAt,
            cancelled);
    }

    private void ConvertOne(
        BatchPlanItem item,
        BatchPreset preset,
        CancellationToken cancellationToken)
    {
        var source = previews.LoadAsync(
                item.Source.Path,
                0,
                cancellationToken)
            .GetAwaiter()
            .GetResult();

        var recipe = preset.ApplyEdits ? item.Source.Recipe : EditRecipe.Empty;
        var rendered = ImageRenderer.Render(
            source,
            recipe,
            RenderRequest.Full,
            cancellationToken);

        rendered = ApplyResize(rendered, preset);

        if (preset.SharpenAmount > 0)
        {
            // Output sharpening belongs after the downscale, which is where
            // the softness it compensates for is introduced.
            var buffer = PixelBuffer.FromBitmap(rendered);
            ImageFilters.ApplyUnsharpMask(
                buffer,
                preset.SharpenAmount,
                radius: 1,
                threshold: 2,
                invert: false,
                cancellationToken);
            rendered = buffer.ToBitmap();
        }

        var format = ImageEncoder.ResolveFormat(
            preset.Format,
            Path.GetExtension(item.Source.Path));
        var carryMetadata = preset.MetadataPolicy == BatchMetadataPolicy.Preserve
            ? item.Source.Path
            : null;
        ImageEncoder.Write(
            rendered,
            item.DestinationPath,
            format,
            preset.Quality,
            carryMetadata,
            cancellationToken);
    }

    internal static BitmapSource ApplyResize(
        BitmapSource image,
        BatchPreset preset)
    {
        var target = MeasureResize(
            image.PixelWidth,
            image.PixelHeight,
            preset);
        return target is { } size
            ? ImageRenderer.ResizeTo(image, size.Width, size.Height)
            : image;
    }

    /// <summary>
    /// The output size for a resize mode, always preserving the aspect ratio.
    /// Returns null when the photograph should be left at its own size, which
    /// includes the case of an already-small image when enlarging is off.
    /// </summary>
    internal static (int Width, int Height)? MeasureResize(
        int width,
        int height,
        BatchPreset preset)
    {
        if (preset.ResizeMode == BatchResizeMode.None || width <= 0 || height <= 0)
        {
            return null;
        }

        var value = preset.ResizeValue;
        if (value <= 0)
        {
            return null;
        }

        var scale = preset.ResizeMode switch
        {
            BatchResizeMode.Width => value / (double)width,
            BatchResizeMode.Height => value / (double)height,
            BatchResizeMode.LongestSide =>
                value / (double)Math.Max(width, height),
            BatchResizeMode.ShortestSide =>
                value / (double)Math.Min(width, height),
            BatchResizeMode.Percentage => value / 100d,
            _ => 1
        };

        if (!double.IsFinite(scale) || scale <= 0)
        {
            return null;
        }

        if (scale > 1 && !preset.AllowEnlarge)
        {
            return null;
        }

        var targetWidth = Math.Max(1, (int)Math.Round(width * scale));
        var targetHeight = Math.Max(1, (int)Math.Round(height * scale));
        return targetWidth == width && targetHeight == height
            ? null
            : (targetWidth, targetHeight);
    }
}

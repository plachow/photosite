using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using PhotoSite.Domain;

namespace PhotoSite.Services.Imaging;

/// <summary>
/// Which stages of the recipe a caller wants. Deliberately a record class:
/// as a struct, <c>default</c> would zero the flags instead of running the
/// constructor defaults and every optional parameter would silently render
/// geometry only.
/// </summary>
internal sealed record RenderRequest(
    CropRegion? RegionOverride = null,
    bool IncludeAdjustments = true,
    bool IncludeFilters = true,
    bool IncludeLayers = true,
    int MaxDimension = 0,
    bool IncludeFrame = true)
{
    public static RenderRequest Full { get; } = new();

    /// <summary>Geometry only - what a plain "copy this crop" needs.</summary>
    public static RenderRequest GeometryOnly { get; } = new(
        IncludeAdjustments: false,
        IncludeFilters: false,
        IncludeLayers: false,
        IncludeFrame: false);
}

/// <summary>
/// The single place a recipe becomes pixels. The viewer, Save As, the export
/// dialog and the batch processor all call this, which is what guarantees the
/// exported file matches the preview.
/// </summary>
internal static class ImageRenderer
{
    public static BitmapSource Render(
        BitmapSource source,
        EditRecipe recipe,
        RenderRequest? request = null,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(source);
        ArgumentNullException.ThrowIfNull(recipe);
        request ??= RenderRequest.Full;

        var buffer = RenderToBuffer(source, recipe, request, cancellationToken);
        var bitmap = buffer.ToBitmap();

        if (recipe.HasResize)
        {
            // The recipe's size is stated for the whole finished frame; a
            // region render (copying a selection) gets the same scale so
            // the copied pixels match what an export of the frame would
            // hold.
            var (nativeWidth, nativeHeight) = MeasureFrame(
                source.PixelWidth,
                source.PixelHeight,
                recipe);
            var (outputWidth, outputHeight) = recipe.MeasureResize(
                nativeWidth,
                nativeHeight);
            var scaleX = outputWidth / (double)Math.Max(1, nativeWidth);
            var scaleY = outputHeight / (double)Math.Max(1, nativeHeight);
            bitmap = ResizeTo(
                bitmap,
                Math.Max(1, (int)Math.Round(bitmap.PixelWidth * scaleX)),
                Math.Max(1, (int)Math.Round(bitmap.PixelHeight * scaleY)));
        }

        if (request.MaxDimension > 0)
        {
            bitmap = Resize(bitmap, request.MaxDimension);
        }

        if (request.IncludeLayers && recipe.Layers.Count > 0)
        {
            bitmap = LayerRenderer.Compose(bitmap, recipe.Layers);
        }

        // A region render is a piece of the photograph, and a piece has no
        // frame; the whole frame does.
        if (request.IncludeFrame
            && request.RegionOverride is null
            && recipe.Frame is { IsEmpty: false } frame)
        {
            bitmap = FrameRenderer.Compose(bitmap, frame);
        }

        return bitmap;
    }

    /// <summary>
    /// Produces the surface the editor canvas paints: straightening,
    /// adjustments and filters applied, but the crop, rotation, flips and
    /// annotation layers deliberately left out.
    /// </summary>
    /// <remarks>
    /// The viewer already expresses crop and orientation as a cheap transform
    /// it can change every frame, so baking them in here would force a full
    /// re-render on every rotate. Keeping the surface at the full frame also
    /// lets the crop rectangle be dragged outside the current crop.
    /// </remarks>
    public static BitmapSource RenderPreviewSurface(
        BitmapSource source,
        EditRecipe recipe,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(source);
        ArgumentNullException.ThrowIfNull(recipe);

        if (recipe.StraightenAngle == 0
            && recipe.PerspectiveVertical == 0
            && recipe.PerspectiveHorizontal == 0
            && recipe.Adjustments.IsNeutral
            && recipe.Filters.Count == 0)
        {
            return source;
        }

        var buffer = PixelBuffer.FromBitmap(source);
        if (NeedsWarp(recipe))
        {
            buffer = GeometryProcessor.Warp(
                buffer,
                recipe.StraightenAngle,
                recipe.PerspectiveVertical,
                recipe.PerspectiveHorizontal,
                cancellationToken,
                recipe.Adjustments.LensDistortion);
        }

        if (!recipe.Adjustments.IsNeutral)
        {
            AdjustmentPipeline.Apply(
                buffer,
                recipe.Adjustments,
                cancellationToken);
        }

        foreach (var filter in recipe.Filters)
        {
            cancellationToken.ThrowIfCancellationRequested();
            ImageFilters.Apply(buffer, filter, cancellationToken);
        }

        return buffer.ToBitmap();
    }

    /// <summary>
    /// Renders everything except the annotation layers, which the editor draws
    /// itself so they stay live and selectable on the canvas.
    /// </summary>
    public static PixelBuffer RenderToBuffer(
        BitmapSource source,
        EditRecipe recipe,
        RenderRequest? request = null,
        CancellationToken cancellationToken = default)
    {
        request ??= RenderRequest.Full;
        var buffer = PixelBuffer.FromBitmap(source);
        cancellationToken.ThrowIfCancellationRequested();

        if (NeedsWarp(recipe))
        {
            buffer = GeometryProcessor.Warp(
                buffer,
                recipe.StraightenAngle,
                recipe.PerspectiveVertical,
                recipe.PerspectiveHorizontal,
                cancellationToken,
                recipe.Adjustments.LensDistortion);
        }

        var region = request.RegionOverride
                     ?? recipe.Crop
                     ?? CropRegion.Full;
        buffer = GeometryProcessor.Crop(buffer, region);
        cancellationToken.ThrowIfCancellationRequested();

        if (request.IncludeAdjustments && !recipe.Adjustments.IsNeutral)
        {
            AdjustmentPipeline.Apply(
                buffer,
                recipe.Adjustments,
                cancellationToken);
        }

        if (request.IncludeFilters)
        {
            foreach (var filter in recipe.Filters)
            {
                cancellationToken.ThrowIfCancellationRequested();
                ImageFilters.Apply(buffer, filter, cancellationToken);
            }
        }

        return GeometryProcessor.Orient(
            buffer,
            recipe.Rotation,
            recipe.FlipHorizontal,
            recipe.FlipVertical);
    }

    /// <summary>
    /// Scales so the longest side matches <paramref name="maxDimension"/>,
    /// never enlarging - upsampling an export silently costs quality.
    /// </summary>
    public static BitmapSource Resize(BitmapSource source, int maxDimension)
    {
        if (maxDimension <= 0)
        {
            return source;
        }

        var longest = Math.Max(source.PixelWidth, source.PixelHeight);
        if (longest <= maxDimension)
        {
            return source;
        }

        var scale = maxDimension / (double)longest;
        return ResizeTo(
            source,
            Math.Max(1, (int)Math.Round(source.PixelWidth * scale)),
            Math.Max(1, (int)Math.Round(source.PixelHeight * scale)));
    }

    public static BitmapSource ResizeTo(
        BitmapSource source,
        int width,
        int height)
    {
        if (width == source.PixelWidth && height == source.PixelHeight)
        {
            return source;
        }

        var visual = new DrawingVisual();
        System.Windows.Media.RenderOptions.SetBitmapScalingMode(
            visual,
            BitmapScalingMode.HighQuality);
        using (var drawingContext = visual.RenderOpen())
        {
            drawingContext.DrawImage(source, new Rect(0, 0, width, height));
        }

        var rendered = new RenderTargetBitmap(
            width,
            height,
            96,
            96,
            PixelFormats.Pbgra32);
        rendered.Render(visual);
        rendered.Freeze();
        return rendered;
    }

    private static bool NeedsWarp(EditRecipe recipe) =>
        recipe.StraightenAngle != 0
        || recipe.PerspectiveVertical != 0
        || recipe.PerspectiveHorizontal != 0
        || recipe.Adjustments.HasDistortion;

    /// <summary>
    /// The pixel size a recipe produces for a given source, without doing any
    /// of the work. Dialogs use it to show the resulting dimensions live.
    /// </summary>
    public static (int Width, int Height) MeasureOutput(
        int sourceWidth,
        int sourceHeight,
        EditRecipe recipe)
    {
        var (width, height) = MeasureFrame(sourceWidth, sourceHeight, recipe);
        var (resizedWidth, resizedHeight) = recipe.MeasureResize(width, height);
        return recipe.MeasureFrame(resizedWidth, resizedHeight);
    }

    /// <summary>
    /// The size of the cropped and oriented frame before the recipe's own
    /// resize - the space the resize is stated against.
    /// </summary>
    public static (int Width, int Height) MeasureFrame(
        int sourceWidth,
        int sourceHeight,
        EditRecipe recipe)
    {
        var region = recipe.Crop ?? CropRegion.Full;
        var (_, _, width, height) = GeometryProcessor.GetPixelRect(
            sourceWidth,
            sourceHeight,
            region);
        return recipe.Rotation is QuarterRotation.Clockwise90
            or QuarterRotation.Clockwise270
            ? (height, width)
            : (width, height);
    }
}

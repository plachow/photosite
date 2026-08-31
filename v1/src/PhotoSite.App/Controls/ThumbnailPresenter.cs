using System.Windows;
using System.Windows.Controls;
using System.Windows.Media.Imaging;

namespace PhotoSite.Controls;

public sealed class ThumbnailPresenter : Image
{
    // A few viewports' worth of decoded frames is what lets a regenerated
    // tile paint without an async round-trip through the pool.
    private const int MaxCachedBitmaps = 144;

    public static readonly DependencyProperty SourcePathProperty =
        DependencyProperty.Register(
            nameof(SourcePath),
            typeof(string),
            typeof(ThumbnailPresenter),
            new PropertyMetadata(null, OnSourcePathChanged));

    public static readonly DependencyProperty TileWidthProperty =
        DependencyProperty.Register(
            nameof(TileWidth),
            typeof(double),
            typeof(ThumbnailPresenter),
            new PropertyMetadata(204d, OnTileWidthChanged));

    // The cache key is the resolved thumbnail path, whose hash already covers
    // source path, length, write time and thumbnail size - an edited or
    // re-stamped file simply misses instead of serving a stale frame.
    private static readonly object cacheGate = new();
    private static readonly Dictionary<string, LinkedListNode<CacheEntry>> cache =
        new(StringComparer.OrdinalIgnoreCase);
    private static readonly LinkedList<CacheEntry> cacheOrder = new();

    private CancellationTokenSource? cancellation;

    public ThumbnailPresenter()
    {
        Loaded += OnLoaded;
        Unloaded += OnUnloaded;
    }

    public string? SourcePath
    {
        get => (string?)GetValue(SourcePathProperty);
        set => SetValue(SourcePathProperty, value);
    }

    /// <summary>
    /// The width of the tile this thumbnail is painted into, which decides how
    /// much of the photograph is worth decoding.
    /// </summary>
    public double TileWidth
    {
        get => (double)GetValue(TileWidthProperty);
        set => SetValue(TileWidthProperty, value);
    }

    /// <summary>
    /// Snaps a tile width to one of a few cache sizes.
    /// </summary>
    /// <remarks>
    /// Caching per exact pixel width would regenerate every thumbnail on the
    /// disk each time the size slider moved. Buckets mean the gallery keeps a
    /// crisp thumbnail at any size while only ever writing four variants, and
    /// small tiles still decode small.
    /// </remarks>
    internal static (int Width, int Height) ResolveThumbnailSize(double tileWidth) =>
        tileWidth switch
        {
            <= 160 => (240, 160),
            <= 240 => (360, 240),
            <= 340 => (480, 320),
            _ => (640, 428)
        };

    private static void OnSourcePathChanged(
        DependencyObject dependencyObject,
        DependencyPropertyChangedEventArgs eventArgs)
    {
        if (dependencyObject is ThumbnailPresenter presenter && presenter.IsLoaded)
        {
            presenter.BeginLoad();
        }
    }

    private static void OnTileWidthChanged(
        DependencyObject dependencyObject,
        DependencyPropertyChangedEventArgs eventArgs)
    {
        if (dependencyObject is not ThumbnailPresenter presenter
            || !presenter.IsLoaded)
        {
            return;
        }

        // Only a change that crosses a bucket boundary is worth a reload.
        if (ResolveThumbnailSize((double)eventArgs.OldValue)
            != ResolveThumbnailSize((double)eventArgs.NewValue))
        {
            presenter.BeginLoad();
        }
    }

    private void OnLoaded(object sender, RoutedEventArgs e) => BeginLoad();

    private void OnUnloaded(object sender, RoutedEventArgs e)
    {
        cancellation?.Cancel();
        cancellation?.Dispose();
        cancellation = null;
        Source = null;
    }

    private async void BeginLoad()
    {
        cancellation?.Cancel();
        cancellation?.Dispose();
        cancellation = new CancellationTokenSource();
        var token = cancellation.Token;
        var path = SourcePath;

        if (string.IsNullOrWhiteSpace(path))
        {
            Source = null;
            return;
        }

        var (thumbnailWidth, thumbnailHeight) = ResolveThumbnailSize(TileWidth);

        try
        {
            if (App.Services.Thumbnails.TryGetCached(
                    path,
                    thumbnailWidth,
                    thumbnailHeight,
                    out var cachedPath)
                && TryGetCachedBitmap(cachedPath, out var cachedBitmap))
            {
                // Assigning before the first await means the tile composes
                // with its bitmap in the same layout pass - no blank frame.
                ApplyBitmap(cachedBitmap);
                return;
            }

            var thumbnailPath = await App.Services.Thumbnails.GetOrCreateAsync(
                path,
                thumbnailWidth,
                thumbnailHeight,
                token);
            token.ThrowIfCancellationRequested();

            var bitmap = new BitmapImage();
            bitmap.BeginInit();
            bitmap.CacheOption = BitmapCacheOption.OnLoad;
            bitmap.UriSource = new Uri(thumbnailPath, UriKind.Absolute);
            bitmap.EndInit();
            bitmap.Freeze();
            StoreCachedBitmap(thumbnailPath, bitmap);

            if (!token.IsCancellationRequested)
            {
                ApplyBitmap(bitmap);
            }
        }
        catch (OperationCanceledException)
        {
        }
        catch
        {
            Source = null;
        }
    }

    private void ApplyBitmap(BitmapSource bitmap)
    {
        // Portrait photos fit by height (letterboxed) instead of being blown
        // up to full width and cropped; landscape photos keep filling the tile.
        Stretch = bitmap.PixelHeight > bitmap.PixelWidth
            ? System.Windows.Media.Stretch.Uniform
            : System.Windows.Media.Stretch.UniformToFill;
        Source = bitmap;
    }

    private static bool TryGetCachedBitmap(
        string thumbnailPath,
        out BitmapSource bitmap)
    {
        lock (cacheGate)
        {
            if (cache.TryGetValue(thumbnailPath, out var node))
            {
                cacheOrder.Remove(node);
                cacheOrder.AddFirst(node);
                bitmap = node.Value.Bitmap;
                return true;
            }
        }

        bitmap = null!;
        return false;
    }

    private static void StoreCachedBitmap(string thumbnailPath, BitmapSource bitmap)
    {
        lock (cacheGate)
        {
            if (cache.TryGetValue(thumbnailPath, out var existing))
            {
                cacheOrder.Remove(existing);
                cacheOrder.AddFirst(existing);
                return;
            }

            var node = cacheOrder.AddFirst(new CacheEntry(thumbnailPath, bitmap));
            cache[thumbnailPath] = node;
            while (cacheOrder.Count > MaxCachedBitmaps)
            {
                var oldest = cacheOrder.Last!;
                cacheOrder.RemoveLast();
                cache.Remove(oldest.Value.Key);
            }
        }
    }

    private sealed record CacheEntry(string Key, BitmapSource Bitmap);
}

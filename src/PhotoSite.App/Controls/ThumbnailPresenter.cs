using System.Windows;
using System.Windows.Controls;
using System.Windows.Media.Imaging;

namespace PhotoSite.Controls;

public sealed class ThumbnailPresenter : Image
{
    private const int ThumbnailWidth = 360;
    private const int ThumbnailHeight = 240;

    // Roughly 345 KB per decoded frame; a few viewports' worth is what lets a
    // regenerated tile paint without an async round-trip through the pool.
    private const int MaxCachedBitmaps = 192;

    public static readonly DependencyProperty SourcePathProperty =
        DependencyProperty.Register(
            nameof(SourcePath),
            typeof(string),
            typeof(ThumbnailPresenter),
            new PropertyMetadata(null, OnSourcePathChanged));

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

    private static void OnSourcePathChanged(
        DependencyObject dependencyObject,
        DependencyPropertyChangedEventArgs eventArgs)
    {
        if (dependencyObject is ThumbnailPresenter presenter && presenter.IsLoaded)
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

        try
        {
            if (App.Services.Thumbnails.TryGetCached(
                    path,
                    ThumbnailWidth,
                    ThumbnailHeight,
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
                ThumbnailWidth,
                ThumbnailHeight,
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

using System.Windows;
using System.Windows.Controls;
using System.Windows.Media.Imaging;

namespace PhotoSite.Controls;

public sealed class ThumbnailPresenter : Image
{
    public static readonly DependencyProperty SourcePathProperty =
        DependencyProperty.Register(
            nameof(SourcePath),
            typeof(string),
            typeof(ThumbnailPresenter),
            new PropertyMetadata(null, OnSourcePathChanged));

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
            var thumbnailPath = await App.Services.Thumbnails.GetOrCreateAsync(
                path,
                360,
                240,
                token);
            token.ThrowIfCancellationRequested();

            var bitmap = new BitmapImage();
            bitmap.BeginInit();
            bitmap.CacheOption = BitmapCacheOption.OnLoad;
            bitmap.UriSource = new Uri(thumbnailPath, UriKind.Absolute);
            bitmap.EndInit();
            bitmap.Freeze();

            if (!token.IsCancellationRequested)
            {
                Source = bitmap;
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
}

using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using PhotoSite.Infrastructure;
using PhotoSite.ViewModels;

namespace PhotoSite;

public partial class App : Application
{
    public static AppServices Services { get; } = new();

    internal bool SuppressStartup { get; init; }

    protected override async void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
        if (SuppressStartup)
        {
            return;
        }

        var isSmokeTest = e.Args.Contains(
            "--smoke-test",
            StringComparer.OrdinalIgnoreCase);

        try
        {
            await Services.InitializeAsync();
            var viewModel = new MainViewModel(
                Services.Catalog,
                Services.Indexer);
            var window = new MainWindow(viewModel, Services.Catalog);
            await window.RestoreLayoutAsync();
            if (isSmokeTest)
            {
                window.ApplyTemplate();
                ValidateScrollBarDirections();
                window.ValidatePaneScrollBarsForSmokeTest();
                window.Close();
                Shutdown();
                return;
            }

            window.Show();
            await viewModel.InitializeAsync();
        }
        catch (Exception exception)
        {
            if (isSmokeTest)
            {
                Console.Error.WriteLine(exception);
                Shutdown(-1);
                return;
            }

            MessageBox.Show(
                exception.ToString(),
                "PhotoSite could not start",
                MessageBoxButton.OK,
                MessageBoxImage.Error);
            Shutdown(-1);
        }
    }

    internal static void ValidateScrollBarDirections()
    {
        var style = (Style)Current.FindResource(typeof(ScrollBar));
        var photoListStyle =
            (Style)Current.FindResource("PhotoListScrollBarStyle");
        var vertical = CreateScrollBarTrack(style, Orientation.Vertical);
        var horizontal = CreateScrollBarTrack(style, Orientation.Horizontal);
        var largeCatalog = CreateScrollBarTrack(
            photoListStyle,
            Orientation.Vertical,
            maximum: 100_000,
            viewportSize: 1);
        if (!vertical.IsDirectionReversed || horizontal.IsDirectionReversed)
        {
            throw new InvalidOperationException(
                "Scrollbar tracks do not map their values to the expected direction.");
        }

        if (vertical.Thumb.MinHeight < 40 || horizontal.Thumb.MinWidth < 40)
        {
            throw new InvalidOperationException(
                "Scrollbar thumbs do not have the required minimum length.");
        }

        if (vertical.Thumb.ActualHeight < 40
            || vertical.Thumb.ActualWidth > 7
            || horizontal.Thumb.ActualWidth < 40
            || horizontal.Thumb.ActualHeight > 7)
        {
            throw new InvalidOperationException(
                "Scrollbar thumbs are arranged in the wrong orientation.");
        }

        if (largeCatalog.Thumb.ActualHeight < 48)
        {
            throw new InvalidOperationException(
                "The PhotoList scrollbar thumb becomes too small for a large catalogue.");
        }
    }

    private static Track CreateScrollBarTrack(
        Style style,
        Orientation orientation,
        double maximum = 1_000,
        double viewportSize = 0)
    {
        var scrollBar = new ScrollBar
        {
            Orientation = orientation,
            Style = style,
            Minimum = 0,
            Maximum = maximum,
            ViewportSize = viewportSize,
            Value = 0
        };
        scrollBar.ApplyTemplate();
        var size = orientation == Orientation.Vertical
            ? new Size(7, 120)
            : new Size(120, 7);
        scrollBar.Measure(size);
        scrollBar.Arrange(new Rect(size));
        scrollBar.UpdateLayout();
        return scrollBar.Template.FindName("PART_Track", scrollBar) as Track
               ?? throw new InvalidOperationException(
                   "The scrollbar template does not expose PART_Track.");
    }
}

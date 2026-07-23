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

    private static void ValidateScrollBarDirections()
    {
        var style = (Style)Current.FindResource(typeof(ScrollBar));
        var vertical = CreateScrollBarTrack(style, Orientation.Vertical);
        var horizontal = CreateScrollBarTrack(style, Orientation.Horizontal);
        if (!vertical.IsDirectionReversed || horizontal.IsDirectionReversed)
        {
            throw new InvalidOperationException(
                "Scrollbar tracks do not map their values to the expected direction.");
        }
    }

    private static Track CreateScrollBarTrack(
        Style style,
        Orientation orientation)
    {
        var scrollBar = new ScrollBar
        {
            Orientation = orientation,
            Style = style
        };
        scrollBar.ApplyTemplate();
        return scrollBar.Template.FindName("PART_Track", scrollBar) as Track
               ?? throw new InvalidOperationException(
                   "The scrollbar template does not expose PART_Track.");
    }
}

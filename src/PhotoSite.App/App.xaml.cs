using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using PhotoSite.Infrastructure;
using PhotoSite.Services;
using PhotoSite.ViewModels;
using Velopack;

namespace PhotoSite;

public partial class App : Application
{
    private readonly CancellationTokenSource updateCancellation = new();
    private static readonly Lazy<AppServices> services = new();

    public static AppServices Services => services.Value;

    internal bool SuppressStartup { get; init; }

    [STAThread]
    private static void Main(string[] args)
    {
        VelopackApp.Build().Run();

        var app = new App();
        app.InitializeComponent();
        app.Run();
    }

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
        var startupPath = e.Args.FirstOrDefault(
            argument => !argument.Equals(
                "--smoke-test",
                StringComparison.OrdinalIgnoreCase));

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
            await viewModel.InitializeAsync(
                startupPath,
                updateCancellation.Token);
            _ = CheckForUpdatesAsync(window, viewModel);
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

    protected override void OnExit(ExitEventArgs e)
    {
        updateCancellation.Cancel();
        updateCancellation.Dispose();
        base.OnExit(e);
    }

    private async Task CheckForUpdatesAsync(
        MainWindow window,
        MainViewModel viewModel)
    {
        if (!Services.Updates.CanCheckForUpdates)
        {
            return;
        }

        try
        {
            viewModel.ReportStatus("Checking for PhotoSite updates…");
            IProgress<int> progress = new Progress<int>(
                percentage => viewModel.ReportStatus(
                    $"Downloading PhotoSite update… {percentage}%"));
            var update = await Services.Updates.CheckAndDownloadAsync(
                progress.Report,
                updateCancellation.Token);
            if (update is null)
            {
                viewModel.ReportStatus("PhotoSite is up to date");
                return;
            }

            viewModel.ReportStatus(
                $"PhotoSite {update.Version} is ready to install");
            var choice = MessageBox.Show(
                window,
                $"PhotoSite {update.Version} has been downloaded.\n\n"
                + "Restart now to finish the update?",
                "PhotoSite update ready",
                MessageBoxButton.YesNo,
                MessageBoxImage.Information,
                MessageBoxResult.Yes);
            if (choice != MessageBoxResult.Yes
                || !await window.PrepareForUpdateRestartAsync())
            {
                return;
            }

            Services.Updates.ApplyAndRestart(update);
        }
        catch (OperationCanceledException)
            when (updateCancellation.IsCancellationRequested)
        {
            // Normal application shutdown.
        }
        catch (Exception exception)
        {
            viewModel.ReportStatus(
                $"Update check failed: {exception.Message}");
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

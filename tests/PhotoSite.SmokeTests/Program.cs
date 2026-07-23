using System.IO;
using System.Windows;
using System.Windows.Input;
using System.Windows.Threading;
using Microsoft.Data.Sqlite;
using PhotoSite;
using PhotoSite.Domain;
using PhotoSite.Infrastructure;
using PhotoSite.Services;
using PhotoSite.ViewModels;

var testRoot = Path.Combine(
    Path.GetTempPath(),
    "PhotoSiteSmokeTests",
    Guid.NewGuid().ToString("N"));

try
{
    Directory.CreateDirectory(testRoot);
    var photoRoot = Directory.CreateDirectory(Path.Combine(testRoot, "photos")).FullName;
    var nested = Directory.CreateDirectory(Path.Combine(photoRoot, "nested")).FullName;
    var firstPhoto = Path.Combine(photoRoot, "first.png");
    var secondPhoto = Path.Combine(nested, "second.png");
    var ignoredFile = Path.Combine(photoRoot, "notes.txt");

    // A valid one-pixel PNG is enough to exercise WIC/MagicScaler without
    // committing binary fixtures to the repository.
    var png = Convert.FromBase64String(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9ZlEsAAAAASUVORK5CYII=");
    await File.WriteAllBytesAsync(firstPhoto, png);
    await File.WriteAllBytesAsync(secondPhoto, png);
    await File.WriteAllTextAsync(ignoredFile, "not a photograph");

    var databasePath = Path.Combine(testRoot, "catalogue.db");
    var repository = new PhotoCatalogRepository(databasePath);
    await repository.InitializeAsync();

    await repository.SetSettingAsync("last_directory", photoRoot);
    var savedDirectory = await repository.GetSettingAsync("last_directory");
    Assert(
        savedDirectory == photoRoot,
        "The last directory setting should survive a database round-trip.");
    Assert(
        MainViewModel.ResolveInitialDirectory(savedDirectory) == photoRoot,
        "An existing saved directory should be restored.");
    var missingDirectory = Path.Combine(testRoot, "does-not-exist");
    var fallbackDirectory = MainViewModel.ResolveInitialDirectory(missingDirectory);
    Assert(
        fallbackDirectory is not null
        && Directory.Exists(fallbackDirectory)
        && !string.Equals(
            fallbackDirectory,
            missingDirectory,
            StringComparison.OrdinalIgnoreCase),
        "A missing saved directory should fall back to an existing known folder.");

    var indexer = new PhotoIndexer();
    var directRecords = new List<PhotoRecord>();
    await foreach (var record in indexer.ScanAsync(
                       photoRoot,
                       50,
                       includeSubfolders: false,
                       CancellationToken.None))
    {
        directRecords.Add(record);
    }

    Assert(
        directRecords.Count == 1 && directRecords[0].Path == firstPhoto,
        "A non-recursive scan should only discover photos in the selected folder.");

    const long firstScan = 100;
    var records = new List<PhotoRecord>();
    await foreach (var record in indexer.ScanAsync(
                       photoRoot,
                       firstScan,
                       includeSubfolders: true,
                       CancellationToken.None))
    {
        records.Add(record);
    }

    Assert(records.Count == 2, "Indexer should discover the two PNG files only.");
    await repository.UpsertBatchAsync(records, CancellationToken.None);
    await repository.CompleteScanAsync(
        photoRoot,
        firstScan,
        includeSubfolders: true,
        CancellationToken.None);

    var loaded = await repository.GetByRootAsync(photoRoot, CancellationToken.None);
    Assert(loaded.Count == 2, "Catalogue should contain both indexed photos.");

    const long directScan = 150;
    var refreshedDirectRecords = directRecords
        .Select(record => record with { ScanId = directScan })
        .ToArray();
    await repository.UpsertBatchAsync(refreshedDirectRecords, CancellationToken.None);
    await repository.CompleteScanAsync(
        photoRoot,
        directScan,
        includeSubfolders: false,
        CancellationToken.None);
    var afterDirectRefresh = await repository.GetByRootAsync(
        photoRoot,
        CancellationToken.None);
    Assert(
        afterDirectRefresh.Count == 2
        && afterDirectRefresh.Any(item => item.Path == secondPhoto),
        "A non-recursive refresh must preserve catalogue rows from subfolders.");

    var mainViewModel = new MainViewModel(repository, indexer);
    var selectedViewModel = new PhotoItemViewModel(
        afterDirectRefresh.Single(item => item.Path == firstPhoto),
        EditRecipe.Empty,
        repository);
    var nextViewModel = new PhotoItemViewModel(
        afterDirectRefresh.Single(item => item.Path == secondPhoto),
        EditRecipe.Empty,
        repository);
    mainViewModel.Photos.Add(selectedViewModel);
    mainViewModel.Photos.Add(nextViewModel);
    mainViewModel.SelectedPhoto = selectedViewModel;
    mainViewModel.ToggleFullscreenCommand.Execute(null);
    Assert(
        mainViewModel.IsFullscreenMode,
        "The middle-click command should enter fullscreen mode.");
    Assert(
        !mainViewModel.ShowEditorCommand.CanExecute(null),
        "Editor mode must not be entered while fullscreen is active.");
    mainViewModel.NextCommand.Execute(null);
    mainViewModel.ToggleFullscreenCommand.Execute(null);
    Assert(
        !mainViewModel.IsFullscreenMode,
        "The middle-click command should leave fullscreen mode.");
    Assert(
        ReferenceEquals(mainViewModel.SelectedPhoto, nextViewModel),
        "The photo reached in fullscreen should remain selected after returning.");
    mainViewModel.PreviousCommand.Execute(null);
    mainViewModel.ShowEditorCommand.Execute(null);
    Assert(
        mainViewModel.IsEditorMode,
        "The thumbnail command should switch from Manager to Editor.");
    Assert(
        !mainViewModel.ShowEditorCommand.CanExecute(null),
        "The thumbnail command must not toggle back from Editor.");
    mainViewModel.ToggleFullscreenCommand.Execute(null);
    Assert(
        mainViewModel.IsFullscreenMode && mainViewModel.IsEditorMode,
        "Fullscreen entered from Editor should preserve the underlying mode.");
    mainViewModel.ToggleFullscreenCommand.Execute(null);
    Assert(
        !mainViewModel.IsFullscreenMode && mainViewModel.IsEditorMode,
        "Leaving fullscreen should return to Editor.");
    mainViewModel.NextCommand.Execute(null);
    Assert(
        ReferenceEquals(mainViewModel.SelectedPhoto, nextViewModel),
        "Editor navigation should follow the current Manager collection order.");
    mainViewModel.PreviousCommand.Execute(null);
    Assert(
        ReferenceEquals(mainViewModel.SelectedPhoto, selectedViewModel),
        "Reverse Editor navigation should follow the Manager collection order.");
    mainViewModel.ShowManagerCommand.Execute(null);
    Assert(
        !mainViewModel.IsEditorMode,
        "The large-photo command should return to Manager.");

    Assert(
        selectedViewModel.RatingText is null,
        "An unrated photo should not render a meaningless zero-star label.");
    var ratedViewModel = new PhotoItemViewModel(
        selectedViewModel.Record with { Rating = 4 },
        EditRecipe.Empty,
        repository);
    Assert(
        ratedViewModel.RatingText == "★ 4",
        "A rated photo should render its star rating.");
    AssertRatingShortcut(Key.Oem3, 0);
    AssertRatingShortcut(Key.D1, 1);
    AssertRatingShortcut(Key.D2, 2);
    AssertRatingShortcut(Key.D3, 3);
    AssertRatingShortcut(Key.D4, 4);
    AssertRatingShortcut(Key.D5, 5);
    Assert(
        !MainWindow.TryGetRatingShortcut(Key.NumPad1, out _),
        "Numpad digits are reserved for viewer controls, not ratings.");

    mainViewModel.SelectedPhoto = nextViewModel;
    await mainViewModel.SaveSessionAsync();
    var restoredViewModel = new MainViewModel(repository, indexer);
    await restoredViewModel.InitializeAsync();
    Assert(
        restoredViewModel.SelectedPhoto?.Path == secondPhoto,
        "Startup should restore the last active photo in the saved directory.");

    await repository.UpdateRatingAsync(firstPhoto, 4);
    var rated = await repository.GetByRootAsync(photoRoot, CancellationToken.None);
    Assert(
        rated.Single(item => item.Path == firstPhoto).Rating == 4,
        "Rating should survive a database round-trip.");

    var recipe = new EditRecipe(
        QuarterRotation.Clockwise90,
        FlipHorizontal: true);
    await repository.SaveEditRecipeAsync(firstPhoto, recipe);
    var loadedRecipe = await repository.GetEditRecipeAsync(firstPhoto);
    Assert(loadedRecipe == recipe, "Edit recipe should survive a database round-trip.");

    var thumbnailDirectory = Path.Combine(testRoot, "thumbnails");
    var thumbnails = new ThumbnailService(thumbnailDirectory);
    var thumbnail = await thumbnails.GetOrCreateAsync(
        firstPhoto,
        120,
        80,
        CancellationToken.None);
    Assert(File.Exists(thumbnail), "Thumbnail should be written to the disk cache.");
    Assert(new FileInfo(thumbnail).Length > 0, "Thumbnail should not be empty.");

    var previews = new PreviewService();
    var preview = await previews.LoadAsync(
        firstPhoto,
        512,
        CancellationToken.None);
    Assert(preview.PixelWidth > 0, "Preview should decode to a WPF bitmap.");

    File.Delete(secondPhoto);
    const long secondScan = 200;
    var secondRecords = new List<PhotoRecord>();
    await foreach (var record in indexer.ScanAsync(
                       photoRoot,
                       secondScan,
                       includeSubfolders: true,
                       CancellationToken.None))
    {
        secondRecords.Add(record);
    }

    await repository.UpsertBatchAsync(secondRecords, CancellationToken.None);
    await repository.CompleteScanAsync(
        photoRoot,
        secondScan,
        includeSubfolders: true,
        CancellationToken.None);
    var afterCleanup = await repository.GetByRootAsync(photoRoot, CancellationToken.None);
    Assert(afterCleanup.Count == 1, "A completed rescan should remove stale catalogue rows.");

    await AssertWindowClosesCleanlyAsync(
        repository,
        indexer,
        selectedViewModel,
        nextViewModel);

    Console.WriteLine("PhotoSite integration smoke tests passed.");
    return 0;
}
finally
{
    SqliteConnection.ClearAllPools();
    if (Directory.Exists(testRoot))
    {
        Directory.Delete(testRoot, recursive: true);
    }
}

static void Assert(bool condition, string message)
{
    if (!condition)
    {
        throw new InvalidOperationException(message);
    }
}

static void AssertRatingShortcut(Key key, int expectedRating)
{
    Assert(
        MainWindow.TryGetRatingShortcut(key, out var rating)
        && rating == expectedRating,
        $"{key} should set rating {expectedRating}.");
}

static async Task AssertWindowClosesCleanlyAsync(
    PhotoCatalogRepository repository,
    PhotoIndexer indexer,
    PhotoItemViewModel cataloguePhoto,
    PhotoItemViewModel nextCataloguePhoto)
{
    var completion = new TaskCompletionSource(
        TaskCreationOptions.RunContinuationsAsynchronously);
    var thread = new Thread(
        () =>
        {
            Exception? dispatcherException = null;
            try
            {
                var application = new App
                {
                    ShutdownMode = ShutdownMode.OnLastWindowClose,
                    SuppressStartup = true
                };
                application.InitializeComponent();
                App.ValidateScrollBarDirections();
                application.DispatcherUnhandledException += (_, eventArgs) =>
                {
                    dispatcherException = eventArgs.Exception;
                    eventArgs.Handled = true;
                    application.Shutdown(-1);
                };

                var viewModel = new MainViewModel(repository, indexer);
                viewModel.Photos.Add(cataloguePhoto);
                viewModel.Photos.Add(nextCataloguePhoto);
                viewModel.SelectedPhoto = cataloguePhoto;
                var window = new MainWindow(viewModel, repository);
                window.RestoreLayoutAsync().GetAwaiter().GetResult();
                window.ValidatePaneScrollBarsForSmokeTest();
                window.ValidateCatalogTileForSmokeTest(
                    cataloguePhoto.FileName);
                window.ValidatePreviewWheelNavigationForSmokeTest();
                window.Loaded += (_, _) =>
                    window.Dispatcher.BeginInvoke(
                        DispatcherPriority.ApplicationIdle,
                        new Action(window.Close));

                application.Run(window);
                if (dispatcherException is not null)
                {
                    throw new InvalidOperationException(
                        "Closing a visible window must not raise a dispatcher exception.",
                        dispatcherException);
                }

                completion.SetResult();
            }
            catch (Exception exception)
            {
                completion.SetException(exception);
            }
        })
    {
        IsBackground = true,
        Name = "PhotoSite shutdown smoke test"
    };
    thread.SetApartmentState(ApartmentState.STA);
    thread.Start();

    await completion.Task.WaitAsync(TimeSpan.FromSeconds(10));
}

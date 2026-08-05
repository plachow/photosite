using System.Collections.Specialized;
using System.IO;
using System.Net;
using System.Net.Http;
using System.Text.Json;
using System.Windows;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using MetadataExtractor.Formats.Exif;
using Microsoft.Data.Sqlite;
using PhotoSite;
using PhotoSite.Controls;
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
    var directLaunchRoot = Directory.CreateDirectory(
        Path.Combine(testRoot, "direct-launch")).FullName;
    var firstPhoto = Path.Combine(photoRoot, "first.png");
    var secondPhoto = Path.Combine(nested, "second.png");
    var directLaunchPhoto = Path.Combine(directLaunchRoot, "opened.png");
    var directLaunchNeighbor = Path.Combine(directLaunchRoot, "neighbor.png");
    var ignoredFile = Path.Combine(photoRoot, "notes.txt");

    // A valid one-pixel PNG is enough to exercise WIC/MagicScaler without
    // committing binary fixtures to the repository.
    var png = Convert.FromBase64String(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9ZlEsAAAAASUVORK5CYII=");
    await File.WriteAllBytesAsync(firstPhoto, png);
    await File.WriteAllBytesAsync(secondPhoto, png);
    await File.WriteAllBytesAsync(directLaunchPhoto, png);
    await File.WriteAllBytesAsync(directLaunchNeighbor, png);
    await File.WriteAllTextAsync(ignoredFile, "not a photograph");

    var databasePath = Path.Combine(testRoot, "catalogue.db");
    await using (var legacyConnection = new SqliteConnection(
                     new SqliteConnectionStringBuilder
                     {
                         DataSource = databasePath
                     }.ToString()))
    {
        await legacyConnection.OpenAsync();
        await using var legacySchema = legacyConnection.CreateCommand();
        legacySchema.CommandText =
            """
            CREATE TABLE photos (
                path                TEXT PRIMARY KEY COLLATE NOCASE,
                root_path           TEXT NOT NULL COLLATE NOCASE,
                file_name           TEXT NOT NULL COLLATE NOCASE,
                extension           TEXT NOT NULL COLLATE NOCASE,
                length              INTEGER NOT NULL,
                modified_utc_ticks  INTEGER NOT NULL,
                rating              INTEGER NOT NULL DEFAULT 0,
                scan_id             INTEGER NOT NULL
            );
            """;
        await legacySchema.ExecuteNonQueryAsync();
    }

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
    await foreach (var result in indexer.ScanAsync(
                       photoRoot,
                       50,
                       includeSubfolders: false,
                       CancellationToken.None))
    {
        directRecords.Add(result.Record);
    }

    Assert(
        directRecords.Count == 1 && directRecords[0].Path == firstPhoto,
        "A non-recursive scan should only discover photos in the selected folder.");

    var exifDirectory = new ExifSubIfdDirectory();
    exifDirectory.Set(
        ExifDirectoryBase.TagDateTimeDigitized,
        "2025:03:04 05:06:07");
    exifDirectory.Set(
        ExifDirectoryBase.TagDateTimeOriginal,
        "2024:01:02 03:04:05");
    var takenAt = PhotoMetadataReader.ReadTakenAt([exifDirectory]);
    Assert(
        takenAt.Ticks == new DateTime(2024, 1, 2, 3, 4, 5).Ticks
        && takenAt.Source == PhotoDateSource.ExifDateTimeOriginal,
        "EXIF DateTimeOriginal should be preferred over DateTimeDigitized.");

    const long firstScan = 100;
    var records = new List<PhotoRecord>();
    await foreach (var result in indexer.ScanAsync(
                       photoRoot,
                       firstScan,
                       includeSubfolders: true,
                       CancellationToken.None))
    {
        records.Add(result.Record);
    }

    Assert(records.Count == 2, "Indexer should discover the two PNG files only.");
    var datedRecordIndex = records.FindIndex(record => record.Path == firstPhoto);
    records[datedRecordIndex] = records[datedRecordIndex] with
    {
        TakenAtTicks = takenAt.Ticks,
        TakenAtSource = takenAt.Source
    };
    await repository.UpsertBatchAsync(records, CancellationToken.None);
    await repository.CompleteScanAsync(
        photoRoot,
        firstScan,
        includeSubfolders: true,
        CancellationToken.None);

    var loaded = await repository.GetByRootAsync(photoRoot, CancellationToken.None);
    Assert(loaded.Count == 2, "Catalogue should contain both indexed photos.");
    Assert(
        loaded.Single(record => record.Path == firstPhoto).TakenAtTicks
        == takenAt.Ticks,
        "The catalogue should persist the photo capture date.");
    Assert(
        loaded.All(record =>
            record.MetadataVersion >= PhotoMetadataReader.CurrentVersion),
        "A completed first scan should mark photo metadata as indexed.");

    var cachedByPath = loaded.ToDictionary(
        record => record.Path,
        StringComparer.OrdinalIgnoreCase);
    var repeatedScan = new List<PhotoScanResult>();
    await foreach (var result in indexer.ScanAsync(
                       photoRoot,
                       scanId: 125,
                       includeSubfolders: true,
                       CancellationToken.None,
                       cachedByPath))
    {
        repeatedScan.Add(result);
    }

    Assert(
        repeatedScan.Count == loaded.Count
        && repeatedScan.All(result => !result.RequiresUpsert),
        "An unchanged repeat scan should reuse cached metadata and require no database writes.");

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
        afterDirectRefresh.Single(item => item.Path == firstPhoto) with
        {
            TakenAtTicks = new DateTime(2024, 1, 2, 3, 4, 5).Ticks,
            TakenAtSource = PhotoDateSource.ExifDateTimeOriginal
        },
        EditRecipe.Empty,
        repository);
    var nextViewModel = new PhotoItemViewModel(
        afterDirectRefresh.Single(item => item.Path == secondPhoto) with
        {
            FileName = "holiday-favourite.png",
            TakenAtTicks = new DateTime(2025, 6, 7, 8, 9, 10).Ticks,
            TakenAtSource = PhotoDateSource.ExifDateTimeOriginal,
            Rating = 4
        },
        EditRecipe.Empty,
        repository);

    var unknownDateViewModel = new PhotoItemViewModel(
        selectedViewModel.Record with
        {
            Path = Path.Combine(photoRoot, "undated.png"),
            FileName = "undated.png",
            TakenAtTicks = null,
            TakenAtSource = PhotoDateSource.None,
            Rating = 5
        },
        EditRecipe.Empty,
        repository);
    var presented = MainViewModel.BuildPhotoPresentation(
        [selectedViewModel, nextViewModel, unknownDateViewModel],
        PhotoSortField.TakenAt,
        descending: true,
        minimumRating: 3,
        searchText: "d");
    Assert(
        presented.Count == 2
        && ReferenceEquals(presented[0], nextViewModel)
        && ReferenceEquals(presented[1], unknownDateViewModel),
        "Presentation should combine filename search and minimum-rating filtering, "
        + "sort dated photos newest-first, and keep unknown dates last.");
    var oldestFirst = MainViewModel.BuildPhotoPresentation(
        [unknownDateViewModel, nextViewModel, selectedViewModel],
        PhotoSortField.TakenAt,
        descending: false,
        minimumRating: 0,
        searchText: null);
    Assert(
        ReferenceEquals(oldestFirst[0], selectedViewModel)
        && ReferenceEquals(oldestFirst[1], nextViewModel)
        && ReferenceEquals(oldestFirst[2], unknownDateViewModel),
        "Ascending capture-date sorting should keep unknown dates last.");

    AssertIncrementalPresentation(
        selectedViewModel,
        nextViewModel,
        unknownDateViewModel);

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
    mainViewModel.ToggleEditorCommand.Execute(null);
    Assert(
        mainViewModel.IsEditorMode,
        "Double-clicking the Manager preview should open Editor.");
    mainViewModel.ToggleEditorCommand.Execute(null);
    Assert(
        !mainViewModel.IsEditorMode,
        "Double-clicking the Editor photo should return to Manager.");

    Assert(
        selectedViewModel.RatingText is null,
        "An unrated photo should not render a meaningless zero-star label.");
    selectedViewModel.BeginEditorSession();
    selectedViewModel.RotateRightCommand.Execute(null);
    Assert(
        selectedViewModel.IsEditorDirty
        && selectedViewModel.UndoEditCommand.CanExecute(null),
        "Editor operations should create a dirty undoable draft.");
    Assert(
        await repository.GetEditRecipeAsync(firstPhoto) == EditRecipe.Empty,
        "Draft editor operations must not persist before save.");
    selectedViewModel.UndoEditCommand.Execute(null);
    Assert(
        !selectedViewModel.IsEditorDirty
        && selectedViewModel.RedoEditCommand.CanExecute(null),
        "Undo should restore the clean editor baseline.");
    selectedViewModel.RedoEditCommand.Execute(null);
    selectedViewModel.DiscardEditorSession();
    Assert(
        selectedViewModel.EditRecipe == EditRecipe.Empty,
        "Discard should restore the recipe from editor entry.");
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
        MainWindow.IsSelectShortcut(Key.C, ModifierKeys.None)
        && !MainWindow.IsSelectShortcut(Key.C, ModifierKeys.Control),
        "C should activate Select without intercepting Ctrl+C.");
    Assert(
        MainWindow.IsCopyFilesShortcut(Key.C, ModifierKeys.Control)
        && !MainWindow.IsCopyFilesShortcut(
            Key.C,
            ModifierKeys.Control | ModifierKeys.Shift)
        && MainWindow.IsQuickFileCopyShortcut(
            Key.C,
            ModifierKeys.Control | ModifierKeys.Shift)
        && !MainWindow.IsQuickFileCopyShortcut(
            Key.C,
            ModifierKeys.Control)
        && MainWindow.IsSelectAllShortcut(Key.A, ModifierKeys.Control)
        && MainWindow.IsCopyToShortcut(Key.C, ModifierKeys.Alt)
        && MainWindow.IsMoveToShortcut(Key.X, ModifierKeys.Alt)
        && !MainWindow.IsCopyToShortcut(Key.C, ModifierKeys.Control),
        "Manager shortcuts must distinguish file clipboard copy, select all, "
        + "quick copy, Copy to, and Move to.");
    Assert(
        MainWindow.IsPasteImageShortcut(Key.V, ModifierKeys.Control)
        && !MainWindow.IsPasteImageShortcut(
            Key.V,
            ModifierKeys.Control | ModifierKeys.Shift)
        && MainWindow.IsSaveAsShortcut(
            Key.S,
            ModifierKeys.Control | ModifierKeys.Shift)
        && !MainWindow.IsSaveAsShortcut(Key.S, ModifierKeys.Control)
        && MainWindow.IsImgurUploadShortcut(Key.U, ModifierKeys.Control)
        && !MainWindow.IsImgurUploadShortcut(Key.U, ModifierKeys.None),
        "Paste, Save As, and Imgur upload shortcuts must use exact modifiers.");
    Assert(
        !MainWindow.TryGetRatingShortcut(Key.NumPad1, out _),
        "Numpad digits are reserved for viewer controls, not ratings.");
    var explorerStartInfo = MainWindow.CreateExplorerSelectStartInfo(firstPhoto);
    Assert(
        explorerStartInfo.FileName == "explorer.exe"
        && explorerStartInfo.ArgumentList.SequenceEqual(["/select,", firstPhoto]),
        "The Explorer action should select the exact photo path.");
    var fileCopyDestination = MainWindow.BuildFileCopyDestination(
        firstPhoto,
        photoRoot);
    Assert(
        fileCopyDestination == Path.Combine(photoRoot, "first.png"),
        "File copy should preserve the original filename in the chosen folder.");
    var fileCopyData = MainWindow.CreateFileCopyDataObject(
        [firstPhoto, secondPhoto]);
    Assert(
        fileCopyData.GetFileDropList()
            .Cast<string>()
            .SequenceEqual([firstPhoto, secondPhoto])
        && fileCopyData.GetData("Preferred DropEffect")
            is MemoryStream copyEffect
        && copyEffect.ToArray().Take(sizeof(int)).SequenceEqual(
            new byte[] { 1, 0, 0, 0 }),
        "Ctrl+C should publish physical file-drop paths with copy semantics.");
    var sameFolderCopyPlan = PhotoFileOperations.Plan(
        firstPhoto,
        photoRoot,
        PhotoFileTransferMode.Copy);
    Assert(
        sameFolderCopyPlan.DestinationPath
            == Path.Combine(photoRoot, "first - Copy.png"),
        "Copying into the source folder should use an Explorer-style copy name.");
    await PhotoFileOperations.ExecuteAsync(
        sameFolderCopyPlan,
        PhotoFileTransferMode.Copy,
        overwrite: false);
    Assert(
        File.Exists(sameFolderCopyPlan.DestinationPath),
        "The physical copy operation should create the planned file.");
    File.Delete(sameFolderCopyPlan.DestinationPath);

    var transferSource = Directory.CreateDirectory(
        Path.Combine(testRoot, "transfer-source")).FullName;
    var transferCopy = Directory.CreateDirectory(
        Path.Combine(testRoot, "transfer-copy")).FullName;
    var transferMove = Directory.CreateDirectory(
        Path.Combine(testRoot, "transfer-move")).FullName;
    var rawSource = Path.Combine(transferSource, "raw.cr2");
    var rawSidecar = Path.Combine(transferSource, "raw.xmp");
    await File.WriteAllBytesAsync(rawSource, [1, 2, 3]);
    await File.WriteAllTextAsync(rawSidecar, "sidecar");
    var rawCopyPlan = PhotoFileOperations.Plan(
        rawSource,
        transferCopy,
        PhotoFileTransferMode.Copy);
    await PhotoFileOperations.ExecuteAsync(
        rawCopyPlan,
        PhotoFileTransferMode.Copy,
        overwrite: false);
    Assert(
        File.Exists(Path.Combine(transferCopy, "raw.cr2"))
        && File.Exists(Path.Combine(transferCopy, "raw.xmp")),
        "Copying a RAW file should preserve its physical XMP sidecar.");
    var rawMovePlan = PhotoFileOperations.Plan(
        rawSource,
        transferMove,
        PhotoFileTransferMode.Move);
    await PhotoFileOperations.ExecuteAsync(
        rawMovePlan,
        PhotoFileTransferMode.Move,
        overwrite: false);
    Assert(
        !File.Exists(rawSource)
        && !File.Exists(rawSidecar)
        && File.Exists(Path.Combine(transferMove, "raw.cr2"))
        && File.Exists(Path.Combine(transferMove, "raw.xmp")),
        "Moving a RAW file should move its physical XMP sidecar too.");
    Assert(
        FileDestinationDialog.FormatFileCount(1) == "1 file"
        && FileDestinationDialog.FormatFileCount(3) == "3 files",
        "The destination dialog should describe single and bulk operations.");
    Assert(
        !PhotoSite.Controls.PhotoViewer.ShouldCrossfade(
            firstPhoto,
            firstPhoto),
        "Reloading another resolution of the same photo must not crossfade.");
    Assert(
        PhotoSite.Controls.PhotoViewer.ShouldCrossfade(
            firstPhoto,
            secondPhoto),
        "Navigating to a different photo should crossfade.");

    mainViewModel.SelectedPhoto = nextViewModel;
    await mainViewModel.SaveSessionAsync();
    var restoredViewModel = new MainViewModel(repository, indexer);
    await restoredViewModel.InitializeAsync();
    Assert(
        restoredViewModel.SelectedPhoto?.Path == secondPhoto,
        "Startup should restore the last active photo in the saved directory.");

    var directoryStartupViewModel = new MainViewModel(repository, indexer);
    await directoryStartupViewModel.InitializeAsync(
        nested,
        CancellationToken.None);
    Assert(
        directoryStartupViewModel.CurrentFolder == nested
        && directoryStartupViewModel.SelectedPhoto?.Path == secondPhoto
        && !directoryStartupViewModel.IsEditorMode,
        "A command-line directory should open that folder in Manager.");

    var uncachedPhotoStartupViewModel = new MainViewModel(repository, indexer);
    await uncachedPhotoStartupViewModel.InitializeAsync(
        directLaunchPhoto,
        CancellationToken.None);
    var directLaunchRecords = await repository.GetByRootAsync(
        directLaunchRoot,
        CancellationToken.None);
    Assert(
        uncachedPhotoStartupViewModel.CurrentFolder == directLaunchRoot
        && uncachedPhotoStartupViewModel.SelectedPhoto?.Path == directLaunchPhoto
        && uncachedPhotoStartupViewModel.Photos.Count == 1
        && directLaunchRecords.Count == 1
        && directLaunchRecords[0].Path == directLaunchPhoto,
        "A directly opened uncached photo should become editable without "
        + "waiting for its whole folder to be indexed.");

    var directStartupOrder = new List<string>();
    await App.StartWindowAsync(
        directLaunchPhoto,
        () =>
        {
            directStartupOrder.Add("editor-ready");
            return Task.CompletedTask;
        },
        () => directStartupOrder.Add("visible"));
    Assert(
        directStartupOrder.SequenceEqual(["editor-ready", "visible"]),
        "A direct photo window must not become visible before Editor is ready.");

    var managerStartupOrder = new List<string>();
    await App.StartWindowAsync(
        directLaunchRoot,
        () =>
        {
            managerStartupOrder.Add("manager-ready");
            return Task.CompletedTask;
        },
        () => managerStartupOrder.Add("visible"));
    Assert(
        managerStartupOrder.SequenceEqual(["visible", "manager-ready"]),
        "A normal Manager launch should remain visible while it initializes.");

    var photoStartupViewModel = new MainViewModel(repository, indexer);
    await photoStartupViewModel.InitializeAsync(
        firstPhoto,
        CancellationToken.None);
    Assert(
        photoStartupViewModel.CurrentFolder == photoRoot
        && photoStartupViewModel.SelectedPhoto?.Path == firstPhoto
        && photoStartupViewModel.IsEditorMode
        && photoStartupViewModel.IsDirectPhotoLaunch,
        "A command-line photo should open its folder and select it in Editor.");
    Assert(
        MainWindow.GetDirectPhotoLaunchKeyAction(
            Key.Escape,
            isDirectPhotoLaunch: true,
            isEditorMode: true)
        == DirectPhotoLaunchKeyAction.CloseWindow,
        "Escape should close a directly opened photo window.");
    Assert(
        MainWindow.GetDirectPhotoLaunchKeyAction(
            Key.Enter,
            isDirectPhotoLaunch: true,
            isEditorMode: true)
        == DirectPhotoLaunchKeyAction.OpenManager,
        "Enter should open Manager from a directly opened photo.");
    Assert(
        MainWindow.GetDirectPhotoLaunchKeyAction(
            Key.Escape,
            isDirectPhotoLaunch: false,
            isEditorMode: true)
        == DirectPhotoLaunchKeyAction.None,
        "Normal Editor launches must keep their existing Escape behavior.");

    await photoStartupViewModel.OpenSelectedPhotoFolderInManagerAsync();
    Assert(
        photoStartupViewModel.CurrentFolder == photoRoot
        && photoStartupViewModel.SelectedPhoto?.Path == firstPhoto
        && !photoStartupViewModel.IsEditorMode
        && !photoStartupViewModel.IsDirectPhotoLaunch,
        "Enter from direct viewing should open Manager in the current photo's directory.");

    var unsupportedStartupViewModel = new MainViewModel(repository, indexer);
    await unsupportedStartupViewModel.InitializeAsync(
        ignoredFile,
        CancellationToken.None);
    Assert(
        !unsupportedStartupViewModel.IsEditorMode
        && unsupportedStartupViewModel.StatusText.StartsWith(
            "Unsupported photo format:",
            StringComparison.Ordinal),
        "An unsupported command-line file should stay in Manager and report the problem.");

    await repository.UpdateRatingAsync(firstPhoto, 4);
    var rated = await repository.GetByRootAsync(photoRoot, CancellationToken.None);
    Assert(
        rated.Single(item => item.Path == firstPhoto).Rating == 4,
        "Rating should survive a database round-trip.");

    var recipe = new EditRecipe(
        QuarterRotation.Clockwise90,
        FlipHorizontal: true,
        Crop: new CropRegion(0.1, 0.2, 0.6, 0.5));
    await repository.SaveEditRecipeAsync(firstPhoto, recipe);
    var loadedRecipe = await repository.GetEditRecipeAsync(firstPhoto);
    Assert(loadedRecipe == recipe, "Edit recipe should survive a database round-trip.");
    var legacyRecipe = JsonSerializer.Deserialize<EditRecipe>(
        """{"Rotation":2,"FlipHorizontal":true}""");
    Assert(
        legacyRecipe is
        {
            Rotation: QuarterRotation.Clockwise180,
            FlipHorizontal: true,
            Crop: null
        },
        "Recipes saved before crop support should remain compatible.");

    var constrainedCrop = new CropRegion(
        0.8,
        0.9,
        -0.6,
        0.4).ConstrainToUnit();
    Assert(
        Math.Abs(constrainedCrop.X - 0.2) < 0.000001
        && Math.Abs(constrainedCrop.Y - 0.9) < 0.000001
        && Math.Abs(constrainedCrop.Width - 0.6) < 0.000001
        && Math.Abs(constrainedCrop.Height - 0.1) < 0.000001,
        "Crop regions should normalize direction and remain inside the image.");

    var constrainedPan = PhotoViewer.ConstrainPanToVisible(
        new Rect(0, 0, 1000, 600),
        new Size(1000, 600),
        new Vector(5000, -5000));
    Assert(
        constrainedPan == new Vector(904, -504),
        "Free pan should keep a 96-DIP grab area visible in both axes.");

    var selectionFromVoid = PhotoViewer.CreateSelectionFromDrag(
        new CropRegion(0.1, 0.2, 0.6, 0.5),
        new Point(-0.5, -0.5),
        new Point(0.4, 0.5));
    Assert(
        Math.Abs(selectionFromVoid.X - 0.1) < 0.000001
        && Math.Abs(selectionFromVoid.Y - 0.2) < 0.000001
        && Math.Abs(selectionFromVoid.Width - 0.3) < 0.000001
        && Math.Abs(selectionFromVoid.Height - 0.3) < 0.000001,
        "Selection drags may start in the void but must only select image pixels.");

    Assert(
        ReferenceEquals(
            PhotoViewer.ResolveResizeCursor(
                new Point(0, 0),
                new Point(5, 5),
                isCorner: true),
            Cursors.SizeNWSE)
        && ReferenceEquals(
            PhotoViewer.ResolveResizeCursor(
                new Point(10, 0),
                new Point(5, 5),
                isCorner: true),
            Cursors.SizeNESW)
        && ReferenceEquals(
            PhotoViewer.ResolveResizeCursor(
                new Point(0, 5),
                new Point(5, 5),
                isCorner: false),
            Cursors.SizeWE)
        && ReferenceEquals(
            PhotoViewer.ResolveResizeCursor(
                new Point(5, 0),
                new Point(5, 5),
                isCorner: false),
            Cursors.SizeNS),
        "Crop handles should expose standard side and corner resize cursors.");

    var syntheticPixels = new byte[6 * 4 * 4];
    Array.Fill<byte>(syntheticPixels, 255);
    var syntheticBitmap = BitmapSource.Create(
        6,
        4,
        96,
        96,
        PixelFormats.Bgra32,
        null,
        syntheticPixels,
        6 * 4);
    syntheticBitmap.Freeze();
    var renderedSelection = PhotoViewer.RenderSelection(
        syntheticBitmap,
        new EditRecipe(
            QuarterRotation.Clockwise90,
            FlipHorizontal: true),
        new CropRegion(0, 0, 0.5, 0.5));
    Assert(
        renderedSelection.PixelWidth == 2
        && renderedSelection.PixelHeight == 3,
        "Copied selections should preserve crop pixels and current orientation.");
    var renderedPixels = new byte[
        renderedSelection.PixelWidth
        * renderedSelection.PixelHeight
        * 4];
    renderedSelection.CopyPixels(
        renderedPixels,
        renderedSelection.PixelWidth * 4,
        0);
    Assert(
        renderedPixels
            .Where((_, index) => index % 4 == 3)
            .All(alpha => alpha > 0),
        "Copied selections should render visible pixels after flip and rotation.");

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

    var imageSaver = new ImageSaveService(previews);
    var suggestedCopy = ImageSaveService.BuildVersionCopyPath(firstPhoto);
    Assert(
        Path.GetFileName(suggestedCopy) == "first ver 1.png",
        "Save as copy should prefill the first available versioned filename.");
    await imageSaver.SaveAsync(
        firstPhoto,
        suggestedCopy,
        new EditRecipe(QuarterRotation.Clockwise90),
        overwrite: false);
    Assert(
        File.Exists(suggestedCopy)
        && (await previews.LoadAsync(
            suggestedCopy,
            0,
            CancellationToken.None)).PixelWidth > 0,
        "Save as copy should create a decodable edited image.");
    File.Delete(suggestedCopy);

    mainViewModel.SelectedPhoto = selectedViewModel;
    var pastedDocument = mainViewModel.OpenPastedImage(syntheticBitmap);
    Assert(
        mainViewModel.IsEditorMode
        && pastedDocument.IsTransient
        && pastedDocument.IsUnsaved
        && pastedDocument.IsEditorDirty
        && ReferenceEquals(pastedDocument.SourceBitmap, syntheticBitmap),
        "A clipboard bitmap should open as an in-memory unsaved editor image.");
    pastedDocument.RotateRightCommand.Execute(null);
    var pastedJpeg = Path.Combine(photoRoot, "pasted.jpg");
    var savedPastedBitmap = await imageSaver.SaveAsync(
        pastedDocument.SourceBitmap!,
        pastedJpeg,
        pastedDocument.EditRecipe,
        overwrite: false,
        jpegQuality: ImageSaveService.DefaultPastedJpegQuality);
    pastedDocument.CompleteTransientSave(
        pastedJpeg,
        savedPastedBitmap,
        continueEditing: true);
    var pastedBytes = await File.ReadAllBytesAsync(pastedJpeg);
    Assert(
        pastedBytes.Length > 2
        && pastedBytes[0] == 0xFF
        && pastedBytes[1] == 0xD8
        && savedPastedBitmap.PixelWidth == 4
        && savedPastedBitmap.PixelHeight == 6
        && !pastedDocument.IsUnsaved
        && !pastedDocument.IsEditorDirty
        && pastedDocument.Path == pastedJpeg,
        "Save As should write a rotated JPEG and adopt it as the clean "
        + "in-memory document.");
    mainViewModel.ShowManagerCommand.Execute(null);
    Assert(
        ReferenceEquals(mainViewModel.SelectedPhoto, selectedViewModel),
        "Closing the pasted document should restore the previous catalogue photo.");
    File.Delete(pastedJpeg);

    var directImgurUrl = ImgurUploadService.ReadDirectLink(
        """{"data":{"link":"http://i.imgur.com/vv462pA.png"}}""");
    Assert(
        directImgurUrl == "https://i.imgur.com/vv462pA.png",
        "Imgur responses should yield a secure direct image URL.");
    var rejectedNonDirectUrl = false;
    try
    {
        ImgurUploadService.ReadDirectLink(
            """{"data":{"link":"https://imgur.com/vv462pA"}}""");
    }
    catch (InvalidOperationException)
    {
        rejectedNonDirectUrl = true;
    }

    Assert(
        rejectedNonDirectUrl,
        "Imgur page URLs must not be copied in place of direct image URLs.");

    using var recordingHandler = new RecordingHttpMessageHandler();
    using var imgurHttpClient = new HttpClient(recordingHandler);
    var imgurUploader = new ImgurUploadService(imgurHttpClient);
    var uploadedUrl = await imgurUploader.UploadAsync(
        syntheticBitmap,
        "  smoke-client-id  ",
        "Smoke upload");
    var multipartText = System.Text.Encoding.Latin1.GetString(
        recordingHandler.RequestBody
        ?? throw new InvalidOperationException(
            "The Imgur request body was not recorded."));
    Assert(
        uploadedUrl == "https://i.imgur.com/vv462pA.png"
        && recordingHandler.Method == HttpMethod.Post
        && recordingHandler.RequestUri
            == new Uri("https://api.imgur.com/3/image")
        && recordingHandler.AuthorizationScheme == "Client-ID"
        && recordingHandler.AuthorizationParameter == "smoke-client-id"
        && recordingHandler.ContentType?.StartsWith(
            "multipart/form-data",
            StringComparison.OrdinalIgnoreCase) == true
        && multipartText.Contains(
            "PhotoSite.png",
            StringComparison.Ordinal)
        && multipartText.Contains(
            "image/png",
            StringComparison.OrdinalIgnoreCase)
        && multipartText.Contains(
            "Smoke upload",
            StringComparison.Ordinal),
        "Imgur upload should POST a PNG multipart body with Client-ID "
        + "authorization and return the direct URL.");

    File.Delete(secondPhoto);
    const long secondScan = 200;
    var beforeCleanup = await repository.GetByRootAsync(
        photoRoot,
        CancellationToken.None);
    var beforeCleanupByPath = beforeCleanup.ToDictionary(
        record => record.Path,
        StringComparer.OrdinalIgnoreCase);
    var secondResults = new List<PhotoScanResult>();
    await foreach (var result in indexer.ScanAsync(
                       photoRoot,
                       secondScan,
                       includeSubfolders: true,
                       CancellationToken.None,
                       beforeCleanupByPath))
    {
        secondResults.Add(result);
    }

    await repository.UpsertBatchAsync(
        secondResults
            .Where(result => result.RequiresUpsert)
            .Select(result => result.Record)
            .ToArray(),
        CancellationToken.None);
    var seenPaths = secondResults
        .Select(result => result.Record.Path)
        .ToHashSet(StringComparer.OrdinalIgnoreCase);
    await repository.DeleteByPathsAsync(
        beforeCleanup
            .Where(record => !seenPaths.Contains(record.Path))
            .Select(record => record.Path)
            .ToArray(),
        CancellationToken.None);
    var afterCleanup = await repository.GetByRootAsync(photoRoot, CancellationToken.None);
    Assert(
        afterCleanup.Count == 1,
        "An incremental rescan should remove stale catalogue rows.");

    var jpegArguments = ExifToolMetadataWriter.BuildArguments(
        @"C:\photos\a.jpg",
        new MetadataWritePayload(
            Rating: 4,
            TitleChanged: true,
            Title: "T",
            DescriptionChanged: true,
            Description: "D",
            LocationChanged: true,
            Latitude: 49.5,
            Longitude: -16.25),
        sidecar: false,
        createSidecar: false);
    Assert(
        jpegArguments.Contains("-XMP-xmp:Rating=4")
        && jpegArguments.Contains("-IFD0:RatingPercent=75")
        && jpegArguments.Contains("-IFD0:XPTitle=T")
        && jpegArguments.Contains("-GPS:GPSLatitudeRef=N")
        && jpegArguments.Contains("-GPS:GPSLongitudeRef=W")
        && jpegArguments[^1] == @"C:\photos\a.jpg",
        "JPEG metadata arguments should cover XMP and Windows EXIF tags.");

    var sidecarArguments = ExifToolMetadataWriter.BuildArguments(
        @"C:\photos\a.xmp",
        new MetadataWritePayload(Rating: 2),
        sidecar: true,
        createSidecar: true);
    Assert(
        sidecarArguments.Contains("-XMP-xmp:Rating=2")
        && !sidecarArguments.Any(
            argument => argument.StartsWith("-IFD0", StringComparison.Ordinal))
        && sidecarArguments[^2] == "-o",
        "Sidecar arguments should stay XMP-only and create the file with -o.");

    Assert(
        ExifToolMetadataWriter.UsesSidecar(".CR2")
        && !ExifToolMetadataWriter.UsesSidecar(".jpg"),
        "RAW files should use XMP sidecars while JPEGs are written in place.");

    var mergedPayload = MetadataOutboxProcessor.MergePayload(
    [
        new MetadataOutboxEntry(1, "p", "rating", """{"rating":2}""", 0),
        new MetadataOutboxEntry(2, "p", "rating", """{"rating":5}""", 0),
        new MetadataOutboxEntry(3, "p", "title", """{"title":"Hello"}""", 0),
        new MetadataOutboxEntry(
            4,
            "p",
            "location",
            """{"latitude":null,"longitude":null}""",
            0)
    ]);
    Assert(
        mergedPayload is
        {
            Rating: 5,
            TitleChanged: true,
            Title: "Hello",
            LocationChanged: true,
            Latitude: null,
            Longitude: null,
            DescriptionChanged: false
        },
        "Merging outbox entries should keep the latest value per kind.");

    Assert(
        PhotoItemViewModel.TryParseLocation("49.5, 16.25", out var parsedLocation)
        && parsedLocation == (49.5, 16.25)
        && PhotoItemViewModel.TryParseLocation("  ", out var clearedLocation)
        && clearedLocation is null
        && !PhotoItemViewModel.TryParseLocation("91, 0", out _)
        && !PhotoItemViewModel.TryParseLocation("foo", out _),
        "GPS text parsing should accept decimal pairs and reject invalid input.");

    Assert(
        PhotoMetadataReader.ParseXmpGpsCoordinate("49,11.703667N") is { } dmLat
        && Math.Abs(dmLat - 49.19506111) < 0.0001
        && PhotoMetadataReader.ParseXmpGpsCoordinate("16,36.410167W") is { } dmLon
        && dmLon < 0
        && PhotoMetadataReader.ParseXmpGpsCoordinate("-50.087") == -50.087,
        "XMP GPS coordinates should parse both DM and signed decimal formats.");

    var metadataTarget = afterCleanup[0];
    await repository.UpdateTitleAsync(metadataTarget.Path, "Smoke title");
    await repository.UpdateDescriptionAsync(
        metadataTarget.Path,
        "Smoke description");
    await repository.UpdateLocationAsync(
        metadataTarget.Path,
        49.195061,
        16.606836);
    await repository.UpdateRatingAsync(metadataTarget.Path, 4);
    var metadataReloaded = await repository.GetByPathAsync(metadataTarget.Path);
    Assert(
        metadataReloaded is
        {
            Title: "Smoke title",
            Description: "Smoke description",
            Latitude: 49.195061,
            Longitude: 16.606836,
            Rating: 4
        },
        "Metadata updates should round-trip through the catalogue.");

    var pendingMetadata = await repository.GetPendingMetadataAsync(8);
    var targetEntries = pendingMetadata
        .Where(entry => string.Equals(
            entry.Path,
            metadataTarget.Path,
            StringComparison.OrdinalIgnoreCase))
        .ToArray();
    Assert(
        targetEntries
            .Select(entry => entry.Kind)
            .Distinct(StringComparer.Ordinal)
            .OrderBy(kind => kind, StringComparer.Ordinal)
            .SequenceEqual(["description", "location", "rating", "title"]),
        "Each metadata update should enqueue an outbox entry of its kind.");

    await repository.IncrementMetadataOutboxAttemptsAsync(
        targetEntries.Select(entry => entry.Id).ToArray());
    Assert(
        (await repository.GetPendingMetadataAsync(8))
        .Where(entry => string.Equals(
            entry.Path,
            metadataTarget.Path,
            StringComparison.OrdinalIgnoreCase))
        .All(entry => entry.Attempts == 1),
        "Attempts should increment after a failed write.");

    await repository.DeleteMetadataOutboxEntriesAsync(
        targetEntries.Select(entry => entry.Id).ToArray());
    Assert(
        (await repository.GetPendingMetadataAsync(8)).All(
            entry => !string.Equals(
                entry.Path,
                metadataTarget.Path,
                StringComparison.OrdinalIgnoreCase)),
        "Processed outbox entries should be deleted.");

    var exifToolWriter = new ExifToolMetadataWriter();
    Assert(
        exifToolWriter.IsAvailable,
        "The bundled exiftool executable should be copied next to the binaries.");
    var writeResult = await exifToolWriter.WriteAsync(
        firstPhoto,
        new MetadataWritePayload(
            Rating: 5,
            TitleChanged: true,
            Title: "Zapsaný titulek",
            DescriptionChanged: true,
            Description: "Zapsaný popis",
            LocationChanged: true,
            Latitude: 49.195061,
            Longitude: 16.606836),
        CancellationToken.None);
    Assert(
        writeResult.Success,
        $"The exiftool write should succeed: {writeResult.Error}");
    var readBack = PhotoMetadataReader.ReadAll(firstPhoto);
    Assert(
        readBack is { Rating: 5, Title: "Zapsaný titulek", Description: "Zapsaný popis" }
        && readBack.Latitude is { } readLat
        && Math.Abs(readLat - 49.195061) < 0.0001
        && readBack.Longitude is { } readLon
        && Math.Abs(readLon - 16.606836) < 0.0001,
        "Metadata written by exiftool should be read back during indexing.");

    await AssertWindowClosesCleanlyAsync(
        repository,
        indexer,
        selectedViewModel,
        nextViewModel,
        syntheticBitmap);

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

static void AssertIncrementalPresentation(
    PhotoItemViewModel first,
    PhotoItemViewModel second,
    PhotoItemViewModel third)
{
    var photos = new BulkObservableCollection<PhotoItemViewModel>();
    var resets = 0;
    var granular = 0;
    photos.CollectionChanged += (_, args) =>
    {
        if (args.Action == NotifyCollectionChangedAction.Reset)
        {
            resets++;
        }
        else
        {
            granular++;
        }
    };

    photos.SynchronizeTo([first, second]);
    Assert(
        resets == 0 && granular == 2 && photos.Count == 2,
        "Filling an empty gallery should insert rather than reset.");

    granular = 0;
    photos.SynchronizeTo([first, second]);
    Assert(
        resets == 0 && granular == 0,
        "Re-presenting an unchanged sequence must raise nothing at all.");

    photos.SynchronizeTo([first, third, second]);
    Assert(
        resets == 0
        && ReferenceEquals(photos[0], first)
        && ReferenceEquals(photos[1], third)
        && ReferenceEquals(photos[2], second),
        "A newly indexed photo should slot in without resetting the gallery.");

    photos.SynchronizeTo([second, third]);
    Assert(
        resets == 0
        && photos.Count == 2
        && ReferenceEquals(photos[0], second)
        && ReferenceEquals(photos[1], third),
        "A removal combined with a reorder should stay incremental.");

    photos.SynchronizeTo([]);
    Assert(
        resets == 0 && photos.Count == 0,
        "Clearing a small gallery should remove rather than reset.");
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
    PhotoItemViewModel nextCataloguePhoto,
    BitmapSource pastedBitmap)
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
                window.ValidatePhotoContextMenuForSmokeTest();
                window.ValidateDarkThemeIconsForSmokeTest();
                window.ValidateStatusBarLayoutForSmokeTest();
                window.ValidateSelectionToolsForSmokeTest();
                window.ValidatePastedImageBindingForSmokeTest(pastedBitmap);
                window.ValidatePastedImageCropForSmokeTest(pastedBitmap);
                window.ValidateCatalogTileForSmokeTest(
                    cataloguePhoto.FileName);
                window.ValidateCatalogScrollResetForSmokeTest();
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

sealed class RecordingHttpMessageHandler : HttpMessageHandler
{
    public HttpMethod? Method { get; private set; }

    public Uri? RequestUri { get; private set; }

    public string? AuthorizationScheme { get; private set; }

    public string? AuthorizationParameter { get; private set; }

    public string? ContentType { get; private set; }

    public byte[]? RequestBody { get; private set; }

    protected override async Task<HttpResponseMessage> SendAsync(
        HttpRequestMessage request,
        CancellationToken cancellationToken)
    {
        Method = request.Method;
        RequestUri = request.RequestUri;
        AuthorizationScheme = request.Headers.Authorization?.Scheme;
        AuthorizationParameter = request.Headers.Authorization?.Parameter;
        ContentType = request.Content?.Headers.ContentType?.ToString();
        RequestBody = request.Content is null
            ? null
            : await request.Content.ReadAsByteArrayAsync(cancellationToken);

        return new HttpResponseMessage(HttpStatusCode.OK)
        {
            Content = new StringContent(
                """{"data":{"link":"https://i.imgur.com/vv462pA.png"}}""")
        };
    }
}

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
using CommunityToolkit.Mvvm.Input;
using MetadataExtractor.Formats.Exif;
using Microsoft.Data.Sqlite;
using PhotoSite;
using PhotoSite.Controls;
using PhotoSite.Domain;
using PhotoSite.Infrastructure;
using PhotoSite.Services;
using PhotoSite.Services.Batch;
using PhotoSite.Services.Faces;
using PhotoSite.Services.Imaging;
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
    Assert(
        mainViewModel.GalleryTotalsText
        == MainViewModel.FormatFileTally(
            2,
            selectedViewModel.Record.Length + nextViewModel.Record.Length),
        "The status bar should tally the presented files and their disk size.");
    Assert(
        mainViewModel.SelectionTotalsText == "none",
        "The status bar should report an empty gallery selection as none.");
    mainViewModel.UpdateSelectionTotals([nextViewModel]);
    Assert(
        mainViewModel.SelectionTotalsText
        == MainViewModel.FormatFileTally(1, nextViewModel.Record.Length),
        "The status bar should tally the selected files and their disk size.");
    mainViewModel.Photos.Remove(nextViewModel);
    Assert(
        mainViewModel.GalleryTotalsText
        == MainViewModel.FormatFileTally(1, selectedViewModel.Record.Length),
        "Removing a photo should take its size out of the status bar tally.");
    mainViewModel.Photos.Add(nextViewModel);
    mainViewModel.UpdateSelectionTotals([]);
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

    var visionRequest = OllamaVisionService.BuildRequestJson(
        "qwen3.8:27b",
        [1, 2, 3],
        "Czech",
        includeEnglishDescription: true,
        disableThinking: true);
    using (var visionRequestDocument = JsonDocument.Parse(visionRequest))
    {
        var visionRoot = visionRequestDocument.RootElement;
        var message = visionRoot.GetProperty("messages")[0];
        Assert(
            visionRoot.GetProperty("model").GetString() == "qwen3.8:27b"
            && !visionRoot.GetProperty("stream").GetBoolean()
            && !visionRoot.GetProperty("think").GetBoolean()
            && visionRoot.GetProperty("format")
                .GetProperty("required").GetArrayLength() == 4
            && visionRoot.GetProperty("format").GetProperty("properties")
                .TryGetProperty("description_en", out _)
            && message.GetProperty("images")[0].GetString()
                == Convert.ToBase64String(new byte[] { 1, 2, 3 })
            && message.GetProperty("content").GetString()!.Contains(
                "in Czech",
                StringComparison.Ordinal),
            "The Ollama request should carry the model, the encoded image, "
            + "the answer schema and the language.");
    }

    using (var visionRequestDocument = JsonDocument.Parse(
               OllamaVisionService.BuildRequestJson(
                   "qwen3.8:27b",
                   [1],
                   "English",
                   includeEnglishDescription: false,
                   disableThinking: false)))
    {
        var visionRoot = visionRequestDocument.RootElement;
        Assert(
            !visionRoot.TryGetProperty("think", out _)
            && visionRoot.GetProperty("format")
                .GetProperty("required").GetArrayLength() == 3,
            "An English-language retry request must leave out both the "
            + "thinking switch and the second description.");
    }

    Assert(
        OllamaVisionService.BuildUri("localhost:11434/", "api/tags")
        == new Uri("http://localhost:11434/api/tags"),
        "A bare host:port server address should become a full HTTP URL.");

    var visionInsights = OllamaVisionService.ParseInsights(
        """
        {"message":{"role":"assistant","content":"{\"title\":\"  Západ slunce\\nnad Lipnem \",\"description\":\" Dva lidé sedí na molu.  \",\"description_en\":\" Two people sit on a pier. \",\"keywords\":[\"#západ slunce\",\"molo; jezero\",\"Molo\",\"  \",\"voda\"]}"}}
        """);
    Assert(
        visionInsights.Title == "Západ slunce nad Lipnem"
        && visionInsights.Description == "Dva lidé sedí na molu."
        && visionInsights.DescriptionEn == "Two people sit on a pier."
        && visionInsights.Keywords.SequenceEqual(
            ["západ slunce", "molo jezero", "Molo", "voda"]),
        "AI insights should come back trimmed, without hashtag prefixes, and "
        + "with the keyword separators removed from each keyword.");

    var visionModels = OllamaVisionService.ParseModelNames(
        """
        {"models":[{"name":"text-only:1b","capabilities":["completion"]},{"name":"older:7b"},{"name":"vision:27b","capabilities":["completion","vision"]}]}
        """);
    Assert(
        visionModels.SequenceEqual(["vision:27b", "older:7b"]),
        "Vision models should lead the list, models without a capability "
        + "report should stay, and text-only models should be dropped.");

    Assert(
        OllamaVisionService.MergeKeywords("les; Šumava", ["šumava", "mlha"])
            == "les; Šumava; mlha"
        && OllamaVisionService.MergeKeywords(null, []) is null,
        "AI keywords should merge case-insensitively into the existing list.");

    Assert(
        OllamaVisionService.ShouldSkip(AiApplyMode.FillEmpty, "t", "d")
        && !OllamaVisionService.ShouldSkip(AiApplyMode.FillEmpty, "t", null)
        && !OllamaVisionService.ShouldSkip(AiApplyMode.FillEmpty, null, "d")
        && !OllamaVisionService.ShouldSkip(AiApplyMode.Overwrite, "t", "d"),
        "Fill-empty runs should skip only photos that already carry both a "
        + "title and a description.");

    var waikikiPlace = ExifToolGeolocator.ParseGeoPlace(
        """
        [{"SourceFile":"x","GeolocationCity":"Waikīkī","GeolocationRegion":"Hawaii","GeolocationSubregion":"Honolulu County","GeolocationCountryCode":"US","GeolocationCountry":"United States","GeolocationDistance":0.96,"GeolocationBearing":313}]
        """);
    Assert(
        waikikiPlace == new GeoPlace(
            "Waikīkī",
            "Honolulu County",
            "Hawaii",
            "United States",
            0.96,
            313),
        "The geolocator should read the city, its parents, the distance and "
        + "the bearing out of exiftool's JSON.");
    Assert(
        ExifToolGeolocator.ParseGeoPlace("[{}]") is null
        && ExifToolGeolocator.ParseGeoPlace(string.Empty) is null
        && ExifToolGeolocator.ParseGeoPlace("not json") is null,
        "Geolocation output without a city should resolve to no place.");

    Assert(
        ExifToolGeolocator.PlaceKeywords(waikikiPlace!, includeCity: true)
            .SequenceEqual(
                ["Waikīkī", "Honolulu County", "Hawaii", "United States"])
        && ExifToolGeolocator.PlaceKeywords(waikikiPlace!, includeCity: false)
            .SequenceEqual(["Honolulu County", "Hawaii", "United States"])
        && ExifToolGeolocator.PlaceKeywords(
                waikikiPlace! with { DistanceKm = 39 },
                includeCity: true)
            .SequenceEqual(["Honolulu County", "Hawaii", "United States"]),
        "Place keywords should run most specific first and leave out the "
        + "town when it is distant or the fix is shaky.");

    var vestmannaeyjarPlace = new GeoPlace(
        "Vestmannaeyjar",
        "Vestmannaeyjabær",
        "South",
        "Iceland",
        39.05,
        264);
    Assert(
        ExifToolGeolocator.CompassFromPlace(264) == "east"
        && ExifToolGeolocator.CompassFromPlace(0) == "south"
        && ExifToolGeolocator.CompassFromPlace(313) == "south-east",
        "The compass names where the photo sits as seen from the place, the "
        + "opposite of the database bearing.");

    Assert(
        OllamaVisionService.BuildPlaceContext(waikikiPlace, false)
            == "in or near Waikīkī (Honolulu County, Hawaii, United States)"
        && OllamaVisionService.BuildPlaceContext(vestmannaeyjarPlace, false)
            == "about 39 km east of Vestmannaeyjar "
            + "(Vestmannaeyjabær, South, Iceland)"
        && OllamaVisionService.BuildPlaceContext(waikikiPlace, true)!
            .StartsWith(
                "probably in or near Waikīkī",
                StringComparison.Ordinal)
        && OllamaVisionService.BuildPlaceContext(null, false) is null,
        "The place context reads \"in or near\" close by, becomes a distance "
        + "and direction far away, and softens on an approximate fix.");

    var augmentedInsights = OllamaVisionService.AugmentKeywords(
        new AiPhotoInsights("t", "d", ["surf", "hawaii", "beach"]),
        waikikiPlace,
        approximateLocation: false);
    Assert(
        augmentedInsights.Keywords.SequenceEqual(
            [
                "Waikīkī", "Honolulu County", "Hawaii", "United States",
                "surf", "beach"
            ])
        && OllamaVisionService.AugmentKeywords(
                augmentedInsights,
                null,
                approximateLocation: false) == augmentedInsights,
        "Place names should lead the keyword list, absorbing the model's "
        + "duplicates, and no place should change nothing.");

    var placedPrompt = OllamaVisionService.BuildPrompt(
        "Czech",
        includeEnglishDescription: false,
        "in or near Waikīkī (Hawaii, United States)");
    Assert(
        placedPrompt.Contains(
            "taken in or near Waikīkī",
            StringComparison.Ordinal)
        && placedPrompt.Contains(
            "Beyond the verified place",
            StringComparison.Ordinal)
        && OllamaVisionService.BuildPrompt("Czech", false).Contains(
            "Do not guess names of people or exact places",
            StringComparison.Ordinal),
        "The prompt should carry the verified place and relax the "
        + "no-place-guessing rule only around it.");

    var geolocator = new ExifToolGeolocator();
    Assert(
        geolocator.IsAvailable,
        "The bundled exiftool should ship next to the test binary.");
    var brnoPlace = await geolocator.ResolveAsync(
        49.1951,
        16.6068,
        CancellationToken.None);
    Assert(
        brnoPlace is { City: "Brno", Country: "Czechia" }
        && brnoPlace.DistanceKm < 5,
        "Reverse geocoding Brno should resolve offline through the bundled "
        + "database.");

    Assert(
        Math.Abs(FaceMath.Cosine([1f, 0f], [1f, 0f]) - 1) < 1e-9
        && Math.Abs(FaceMath.Cosine([1f, 0f], [0f, 1f])) < 1e-9
        && Math.Abs(
            FaceMath.Normalize([3f, 4f])
                .Zip([0.6f, 0.8f])
                .Max(pair => Math.Abs(pair.First - pair.Second))) < 1e-6,
        "Cosine similarity and normalization must behave like the textbook.");

    var faceClusters = FaceClusterer.Cluster(
    [
        new FaceRecord(1, "a.jpg", 0, 0, 0.2, 0.2, 0.9, [1f, 0f, 0f]),
        new FaceRecord(2, "b.jpg", 0, 0, 0.2, 0.2, 0.8, [0.9f, 0.1f, 0f]),
        new FaceRecord(3, "c.jpg", 0, 0, 0.2, 0.2, 0.95, [0f, 1f, 0f]),
        new FaceRecord(4, "a.jpg", 0, 0, 0.2, 0.2, 0.7, [0.1f, 0.95f, 0f]),
        new FaceRecord(5, "d.jpg", 0, 0, 0.2, 0.2, 0.6, [0f, 0f, 1f])
    ]);
    Assert(
        faceClusters.Count == 3
        && faceClusters[0].Faces.Count == 2
        && faceClusters[1].Faces.Count == 2
        && faceClusters[2].Faces.Count == 1
        && faceClusters.Single(cluster => cluster.Faces.Count == 1)
            .Faces[0].Id == 5,
        "Similar embeddings should group; an unrelated face should stand "
        + "alone.");

    var centerCrop = FaceCropper.ComputeCropRect(
        1000,
        1000,
        new FaceRecord(0, "x", 0.4, 0.4, 0.2, 0.2, 1, []));
    var edgeCrop = FaceCropper.ComputeCropRect(
        1000,
        800,
        new FaceRecord(0, "x", 0.9, 0.9, 0.2, 0.2, 1, []));
    Assert(
        centerCrop is { X: 330, Y: 330, Width: 340, Height: 340 }
        && edgeCrop.X + edgeCrop.Width == 1000
        && edgeCrop.Y + edgeCrop.Height == 800,
        "Face crops should grow by their margin and clamp to the frame.");

    var faceEmbedding = new float[] { 1f, 0f, 0.25f, -0.5f };
    Assert(
        PhotoCatalogRepository.BlobToEmbedding(
                PhotoCatalogRepository.EmbeddingToBlob(faceEmbedding))
            .SequenceEqual(faceEmbedding),
        "Embeddings must survive the BLOB round-trip bit-exactly.");

    await repository.ReplaceFacesAsync(
        firstPhoto,
        111,
        [
            new FaceObservation(
                0.1, 0.2, 0.3, 0.4, 0.9, faceEmbedding,
                Smile: 0.92, EyesOpen: 0.15),
            new FaceObservation(
                0.5, 0.5, 0.2, 0.2, 0.8, new float[] { 0f, 1f, 0f, 0f })
        ]);
    Assert(
        (await repository.GetFaceScanStatesAsync())[firstPhoto] == 111,
        "A face scan should stamp the file it covered.");
    var unassignedFaces = await repository.GetUnassignedFacesAsync();
    Assert(
        unassignedFaces.Count == 2
        && unassignedFaces[0].Confidence == 0.9
        && unassignedFaces[0].Embedding.SequenceEqual(faceEmbedding)
        && unassignedFaces[0] is { X: 0.1, Y: 0.2, Width: 0.3, Height: 0.4 }
        && unassignedFaces[0] is { Smile: 0.92, EyesOpen: 0.15 }
        && unassignedFaces[1] is { Smile: null, EyesOpen: null },
        "Detected faces should round-trip ordered by confidence, expression "
        + "scores included.");

    var expressionsByPhoto = await repository.GetExpressionsByPhotoAsync();
    Assert(
        expressionsByPhoto.TryGetValue(firstPhoto, out var firstExpressions)
        && firstExpressions == new ExpressionSummary(2, 1, 1, 0)
        && firstExpressions is
            {
                AllSmiling: false,
                AllEyesOpen: false,
                AnyNotSmiling: false,
                AnyEyesClosed: true
            },
        "The per-photo expression tally should count only scored faces "
        + "against the thresholds.");
    Assert(
        (await repository.GetPathsMissingExpressionsAsync())
            .Contains(firstPhoto, StringComparer.OrdinalIgnoreCase),
        "A face without expression scores should keep its photo on the "
        + "backfill list.");
    await repository.UpdateFaceExpressionsAsync(
        [(unassignedFaces[1].Id, 0.8, 0.9)]);
    expressionsByPhoto = await repository.GetExpressionsByPhotoAsync();
    Assert(
        (await repository.GetPathsMissingExpressionsAsync()).Count == 0
        && expressionsByPhoto[firstPhoto] == new ExpressionSummary(2, 2, 2, 1)
        && expressionsByPhoto[firstPhoto] is
            {
                AllSmiling: true,
                AllEyesOpen: false,
                AnyEyesClosed: true
            },
        "Backfilled expression scores should join the tally and clear the "
        + "backfill list.");

    var personId = await repository.GetOrCreatePersonAsync("Test Person");
    Assert(
        await repository.GetOrCreatePersonAsync("test person") == personId,
        "Person names must match without regard to case.");
    await repository.AssignFacesAsync([unassignedFaces[0].Id], personId);
    var peopleRows = await repository.GetPeopleAsync();
    Assert(
        peopleRows.Count == 1
        && peopleRows[0].Name == "Test Person"
        && peopleRows[0].FaceCount == 1
        && (await repository.GetUnassignedFacesAsync()).Count == 1
        && (await repository.GetFacesForPersonAsync(personId)).Count == 1,
        "Assigning a face should move it from the unnamed pool to its "
        + "person.");

    await repository.RenamePersonAsync(personId, "Renamed Person");
    await repository.ReplaceFacesAsync(
        firstPhoto,
        222,
        [new FaceObservation(0.1, 0.1, 0.1, 0.1, 0.5, faceEmbedding, personId)]);
    Assert(
        (await repository.GetPeopleAsync())[0].Name == "Renamed Person"
        && (await repository.GetFaceScanStatesAsync())[firstPhoto] == 222
        && (await repository.GetAssignedFacesAsync()).Count == 1
        && (await repository.GetUnassignedFacesAsync()).Count == 0,
        "A re-scan should replace a file's faces and keep assignments the "
        + "detector re-established.");

    await repository.DeletePersonAsync(personId);
    Assert(
        (await repository.GetPeopleAsync()).Count == 0
        && (await repository.GetUnassignedFacesAsync()).Count == 1,
        "Removing a person should return their faces to the unnamed pool.");

    var suggestPersonId = await repository.GetOrCreatePersonAsync(
        "Suggested Person");
    await repository.ReplaceFacesAsync(
        firstPhoto,
        444,
        [
            new FaceObservation(
                0.1, 0.1, 0.2, 0.2, 0.9, faceEmbedding,
                SuggestedPersonId: suggestPersonId),
            new FaceObservation(
                0.6, 0.6, 0.2, 0.2, 0.8, new float[] { 0f, 1f, 0f, 0f },
                suggestPersonId)
        ]);
    var suggestedFaces = await repository.GetSuggestedFacesAsync();
    Assert(
        suggestedFaces.Count == 1
        && suggestedFaces[0].SuggestedPersonId == suggestPersonId
        && (await repository.GetUnassignedFacesAsync()).Count == 0
        && (await repository.GetPersonPhotoPathsAsync(suggestPersonId))
            .SequenceEqual([firstPhoto]),
        "A suggested face waits apart from both the unnamed pool and the "
        + "person.");

    await repository.AssignFacesAsync([suggestedFaces[0].Id], suggestPersonId);
    Assert(
        (await repository.GetSuggestedFacesAsync()).Count == 0
        && (await repository.GetFacesForPathAsync(firstPhoto)).Count == 2
        && (await repository.GetFacesForPersonAsync(suggestPersonId)).Count == 2,
        "Confirming a suggestion assigns the face and clears the suggestion.");

    var peopleByPhoto = await repository.GetPeopleByPhotoAsync();
    Assert(
        peopleByPhoto.TryGetValue(firstPhoto, out var photoPeople)
        && photoPeople.Select(tag => tag.Name)
            .SequenceEqual(["Suggested Person"]),
        "The per-photo people map should list each named person once per "
        + "photo.");

    await repository.ReplaceFacesAsync(
        firstPhoto,
        555,
        [new FaceObservation(
            0.1, 0.1, 0.2, 0.2, 0.9, faceEmbedding,
            SuggestedPersonId: suggestPersonId)]);
    await repository.ClearSuggestionsAsync(
        [(await repository.GetSuggestedFacesAsync())[0].Id]);
    Assert(
        (await repository.GetSuggestedFacesAsync()).Count == 0
        && (await repository.GetUnassignedFacesAsync()).Count == 1,
        "Rejecting a suggestion returns the face to the unnamed pool.");

    var strangerFace = (await repository.GetUnassignedFacesAsync())[0];
    await repository.IgnoreFacesAsync([strangerFace.Id]);
    Assert(
        (await repository.GetUnassignedFacesAsync()).Count == 0
        && (await repository.GetFacesForPathAsync(firstPhoto)).Count == 1,
        "An ignored stranger leaves the unnamed pool but keeps its row.");
    await repository.AssignFacesAsync([strangerFace.Id], suggestPersonId);
    Assert(
        (await repository.GetFacesForPersonAsync(suggestPersonId)).Count == 1,
        "Assigning a person to an ignored face brings it back.");
    await repository.DeletePersonAsync(suggestPersonId);
    Assert(
        (await repository.GetUnassignedFacesAsync()).Count == 1,
        "Removing the person returns the once-ignored face to the pool.");

    var regionArguments = new List<string>();
    ExifToolMetadataWriter.AppendRegionArguments(
        regionArguments,
        """{"width":4000,"height":3000,"regions":[{"name":"Jana Nováková","x":0.25,"y":0.2,"w":0.1,"h":0.15}]}""");
    Assert(
        regionArguments[0] == "-XMP-mwg-rs:RegionInfo="
        && regionArguments.Contains("-XMP-mwg-rs:RegionAppliedToDimensionsW=4000")
        && regionArguments.Contains("-XMP-mwg-rs:RegionAppliedToDimensionsH=3000")
        && regionArguments.Contains("-XMP-mwg-rs:RegionAppliedToDimensionsUnit=pixel")
        && regionArguments.Contains("-XMP-mwg-rs:RegionName+=Jana Nováková")
        && regionArguments.Contains("-XMP-mwg-rs:RegionType+=Face")
        && regionArguments.Contains("-XMP-mwg-rs:RegionAreaX+=0.3")
        && regionArguments.Contains("-XMP-mwg-rs:RegionAreaY+=0.275")
        && regionArguments.Contains("-XMP-mwg-rs:RegionAreaW+=0.1")
        && regionArguments.Contains("-XMP-mwg-rs:RegionAreaH+=0.15")
        && regionArguments.Contains("-XMP-mwg-rs:RegionAreaUnit+=normalized"),
        "MWG regions should be cleared first, then rebuilt centre-based with "
        + "the photo's pixel dimensions.");

    var clearedRegionArguments = new List<string>();
    ExifToolMetadataWriter.AppendRegionArguments(clearedRegionArguments, null);
    Assert(
        clearedRegionArguments.SequenceEqual(["-XMP-mwg-rs:RegionInfo="]),
        "A missing region payload should only clear the region struct.");

    var regionsPayload = MetadataOutboxProcessor.MergePayload(
    [
        new MetadataOutboxEntry(
            11,
            "p",
            "regions",
            """{"width":100,"height":50,"regions":[]}""",
            0)
    ]);
    Assert(
        regionsPayload.RegionsChanged
        && regionsPayload.RegionsJson
            == """{"width":100,"height":50,"regions":[]}""",
        "A regions outbox entry should hand its payload to the writer whole.");

    var personCriteria = PhotoFilterCriteria.None with
    {
        PersonIds = new HashSet<long> { 7, 9 },
        PersonNames = ["Jana", "Petr"]
    };
    var personFilterViewModel = new PhotoItemViewModel(
        new PhotoRecord(firstPhoto, photoRoot, "first.png", ".png", 1, 1, 0, 1),
        EditRecipe.Empty,
        repository);
    var onlyJana = new Dictionary<string, HashSet<long>>(
        StringComparer.OrdinalIgnoreCase)
    {
        [firstPhoto] = [7]
    };
    var bothPeople = new Dictionary<string, HashSet<long>>(
        StringComparer.OrdinalIgnoreCase)
    {
        [firstPhoto] = [7, 9, 11]
    };
    Assert(
        personCriteria.IsActive
        && personCriteria.Describe().Contains(
            "Jana + Petr",
            StringComparison.Ordinal)
        && !personCriteria.Equals(PhotoFilterCriteria.None)
        && !MainViewModel.Matches(personFilterViewModel, personCriteria)
        && !MainViewModel.Matches(
            personFilterViewModel,
            personCriteria,
            onlyJana)
        && MainViewModel.Matches(
            personFilterViewModel,
            personCriteria,
            bothPeople)
        && MainViewModel.Matches(personFilterViewModel, PhotoFilterCriteria.None),
        "The person facet is a conjunction: every chosen person must be on "
        + "the photo.");

    var smilingCriteria = PhotoFilterCriteria.None with { Smiling = true };
    var blinkedCriteria = PhotoFilterCriteria.None with { EyesOpen = false };
    var allGood = new Dictionary<string, ExpressionSummary>(
        StringComparer.OrdinalIgnoreCase)
    {
        [firstPhoto] = new ExpressionSummary(2, 2, 2, 2)
    };
    var oneBlinked = new Dictionary<string, ExpressionSummary>(
        StringComparer.OrdinalIgnoreCase)
    {
        [firstPhoto] = new ExpressionSummary(2, 2, 1, 1)
    };
    Assert(
        smilingCriteria.IsActive
        && smilingCriteria.Describe().Contains("smiling", StringComparison.Ordinal)
        && blinkedCriteria.Describe().Contains(
            "closed eyes",
            StringComparison.Ordinal)
        && !MainViewModel.Matches(personFilterViewModel, smilingCriteria)
        && MainViewModel.Matches(
            personFilterViewModel,
            smilingCriteria,
            photoExpressions: allGood)
        && !MainViewModel.Matches(
            personFilterViewModel,
            smilingCriteria,
            photoExpressions: oneBlinked)
        && !MainViewModel.Matches(
            personFilterViewModel,
            blinkedCriteria,
            photoExpressions: allGood)
        && MainViewModel.Matches(
            personFilterViewModel,
            blinkedCriteria,
            photoExpressions: oneBlinked),
        "The expression facets pick everyone-passes on one side and "
        + "someone-fails on the other.");

    var approximateCriteria = PhotoFilterCriteria.None with
    {
        ApproximateLocation = true
    };
    var preciseCriteria = PhotoFilterCriteria.None with
    {
        ApproximateLocation = false
    };
    var approximateViewModel = new PhotoItemViewModel(
        new PhotoRecord(
            firstPhoto,
            photoRoot,
            "first.png",
            ".png",
            1,
            1,
            0,
            1,
            Latitude: 49.2,
            Longitude: 16.6,
            GpsErrorMeters: 250.0),
        EditRecipe.Empty,
        repository);
    var preciseViewModel = new PhotoItemViewModel(
        new PhotoRecord(
            firstPhoto,
            photoRoot,
            "first.png",
            ".png",
            1,
            1,
            0,
            1,
            Latitude: 49.2,
            Longitude: 16.6),
        EditRecipe.Empty,
        repository);
    Assert(
        approximateCriteria.IsActive
        && approximateCriteria.Describe().Contains(
            "approximate GPS",
            StringComparison.Ordinal)
        && MainViewModel.Matches(approximateViewModel, approximateCriteria)
        && !MainViewModel.Matches(approximateViewModel, preciseCriteria)
        && MainViewModel.Matches(preciseViewModel, preciseCriteria)
        && !MainViewModel.Matches(preciseViewModel, approximateCriteria)
        && !MainViewModel.Matches(personFilterViewModel, approximateCriteria)
        && !MainViewModel.Matches(personFilterViewModel, preciseCriteria),
        "The location facet keeps approximate fixes on one side, precise "
        + "ones on the other, and unlocated photos on neither.");

    const string vanishedPhoto = @"Z:\nowhere\gone.jpg";
    await repository.ReplaceFacesAsync(
        vanishedPhoto,
        333,
        [new FaceObservation(0.1, 0.1, 0.1, 0.1, 0.9, faceEmbedding)]);
    await repository.DeleteByPathsAsync([vanishedPhoto], CancellationToken.None);
    Assert(
        !(await repository.GetFaceScanStatesAsync()).ContainsKey(vanishedPhoto)
        && (await repository.GetUnassignedFacesAsync())
            .All(face => face.Path != vanishedPhoto),
        "Deleting a photo must take its faces and scan stamp with it.");

    // Leave no face rows behind for the scenarios that follow.
    await repository.ReplaceFacesAsync(firstPhoto, 222, []);

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
    Assert(
        jpegArguments.Contains("-GPS:GPSHPositioningError=")
        && jpegArguments.Contains("-GPS:GPSDateStamp=")
        && jpegArguments.Contains("-GPS:GPSTimeStamp=")
        && jpegArguments.Contains("-GPS:GPSProcessingMethod=")
        && jpegArguments.Contains("-GPS:GPSAltitude=")
        && jpegArguments.Contains("-XMP:GPSDateTime="),
        "Writing coordinates should retire the replaced fix's error "
        + "estimate, stamp, source and altitude.");

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

    Assert(
        PhotoMetadataReader.ParseUtcOffset("+02:00") == TimeSpan.FromHours(2)
        && PhotoMetadataReader.ParseUtcOffset("-05:30")
            == TimeSpan.FromMinutes(-330)
        && PhotoMetadataReader.ParseUtcOffset("Z") == TimeSpan.Zero
        && PhotoMetadataReader.ParseUtcOffset("  ") is null
        && PhotoMetadataReader.ParseUtcOffset("garbage") is null,
        "EXIF OffsetTime strings should parse with their sign.");

    Assert(
        PhotoMetadataReader.ComputeGpsFixAgeSeconds(
            new DateTime(2026, 8, 22, 12, 10, 0),
            new DateTime(2026, 8, 22, 10, 6, 0),
            TimeSpan.FromHours(2)) == 240
        && PhotoMetadataReader.ComputeGpsFixAgeSeconds(
            new DateTime(2026, 8, 22, 12, 3, 0),
            new DateTime(2026, 8, 22, 10, 0, 0),
            utcOffset: null) == 180
        && PhotoMetadataReader.ComputeGpsFixAgeSeconds(
            new DateTime(2026, 8, 22, 12, 0, 0),
            new DateTime(2026, 8, 22, 12, 0, 30),
            TimeSpan.Zero) == 0,
        "The GPS fix age should use the written offset when there is one, "
        + "fall back to the nearest quarter-hour timezone, and read clock "
        + "skew as fresh.");

    var gpsEvidenceDirectory = new GpsDirectory();
    gpsEvidenceDirectory.Set(
        GpsDirectory.TagLatitudeRef,
        "N");
    gpsEvidenceDirectory.Set(
        GpsDirectory.TagLatitude,
        new[]
        {
            new MetadataExtractor.Rational(49, 1),
            new MetadataExtractor.Rational(12, 1),
            new MetadataExtractor.Rational(0, 1)
        });
    gpsEvidenceDirectory.Set(
        GpsDirectory.TagLongitudeRef,
        "E");
    gpsEvidenceDirectory.Set(
        GpsDirectory.TagLongitude,
        new[]
        {
            new MetadataExtractor.Rational(16, 1),
            new MetadataExtractor.Rational(36, 1),
            new MetadataExtractor.Rational(0, 1)
        });
    gpsEvidenceDirectory.Set(
        GpsDirectory.TagHPositioningError,
        new MetadataExtractor.Rational(250, 1));
    gpsEvidenceDirectory.Set(
        GpsDirectory.TagProcessingMethod,
        new MetadataExtractor.StringValue(
            System.Text.Encoding.ASCII.GetBytes("ASCII\0\0\0CELLID")));
    gpsEvidenceDirectory.Set(
        GpsDirectory.TagAltitude,
        new MetadataExtractor.Rational(0, 1));
    gpsEvidenceDirectory.Set(GpsDirectory.TagDateStamp, "2026:08:22");
    gpsEvidenceDirectory.Set(
        GpsDirectory.TagTimeStamp,
        new[]
        {
            new MetadataExtractor.Rational(10, 1),
            new MetadataExtractor.Rational(0, 1),
            new MetadataExtractor.Rational(0, 1)
        });
    var gpsEvidenceSubIfd = new ExifSubIfdDirectory();
    gpsEvidenceSubIfd.Set(
        ExifDirectoryBase.TagDateTimeOriginal,
        "2026:08:22 12:04:00");
    gpsEvidenceSubIfd.Set(ExifDirectoryBase.TagTimeZoneOriginal, "+02:00");
    var gpsEvidence = PhotoMetadataReader.ReadGpsEvidence(
        [gpsEvidenceDirectory, gpsEvidenceSubIfd]);
    Assert(
        gpsEvidence.ErrorMeters == 250
        && gpsEvidence.FixAgeSeconds == 240
        && gpsEvidence.ProcessingMethod == "CELLID"
        && gpsEvidence.Altitude == 0,
        "The GPS evidence should surface the error estimate, fix age, "
        + "positioning method and altitude of a positioned photo.");
    Assert(
        PhotoMetadataReader.ReadGpsEvidence([gpsEvidenceSubIfd]) == default,
        "A photo without coordinates should carry no GPS evidence.");

    var locatedRecord = new PhotoRecord(
        firstPhoto,
        photoRoot,
        "first.png",
        ".png",
        1,
        1,
        0,
        1,
        Latitude: 49.2,
        Longitude: 16.6);
    Assert(
        (locatedRecord with { GpsErrorMeters = 250.0 }).HasApproximateLocation
        && (locatedRecord with { GpsFixAgeSeconds = 300.0 }).HasApproximateLocation
        && !(locatedRecord with
        {
            GpsErrorMeters = 30.0,
            GpsFixAgeSeconds = 30.0
        }).HasApproximateLocation
        && !locatedRecord.HasApproximateLocation
        && !(locatedRecord with
        {
            Latitude = null,
            GpsErrorMeters = 250.0
        }).HasApproximateLocation,
        "A location is approximate on a coarse error estimate or a stale "
        + "fix, and only when there are coordinates at all.");

    Assert(
        locatedRecord.LocationAccuracy == LocationAccuracy.Precise
        && (locatedRecord with { GpsErrorMeters = 250.0 }).LocationAccuracy
            == LocationAccuracy.Approximate
        && (locatedRecord with { GpsErrorMeters = 800.0 }).LocationAccuracy
            == LocationAccuracy.Poor
        && (locatedRecord with { GpsFixAgeSeconds = 700.0 }).LocationAccuracy
            == LocationAccuracy.Poor
        && (locatedRecord with { Latitude = null }).LocationAccuracy
            == LocationAccuracy.None
        && (locatedRecord with { GpsErrorMeters = 800.0 }).HasApproximateLocation,
        "The GPS verdict grades none, precise, approximate and far-off, and "
        + "the far-off tier still counts as approximate for badge and filter.");

    Assert(
        (locatedRecord with { GpsProcessingMethod = "CELLID" }).LocationAccuracy
            == LocationAccuracy.Poor
        && (locatedRecord with
        {
            GpsProcessingMethod = "CELLID",
            GpsAltitude = 0.0
        }).LocationAccuracy == LocationAccuracy.Poor
        && (locatedRecord with
        {
            GpsProcessingMethod = "CELLID",
            GpsAltitude = 332.5
        }).LocationAccuracy == LocationAccuracy.Approximate
        && (locatedRecord with
        {
            GpsProcessingMethod = "WLAN",
            GpsAltitude = 50.0
        }).LocationAccuracy == LocationAccuracy.Approximate
        && (locatedRecord with { GpsProcessingMethod = "GPS" }).LocationAccuracy
            == LocationAccuracy.Precise,
        "A pure cell-tower fix without altitude grades far off, any network "
        + "position grades approximate, a satellite fix stays precise.");

    var metadataTarget = afterCleanup[0];
    await repository.UpsertBatchAsync(
        [metadataTarget with
        {
            GpsErrorMeters = 250.0,
            GpsFixAgeSeconds = 300.0,
            GpsProcessingMethod = "CELLID",
            GpsAltitude = 0.0
        }],
        CancellationToken.None);
    var evidenceReloaded = await repository.GetByPathAsync(metadataTarget.Path);
    Assert(
        evidenceReloaded is
        {
            GpsErrorMeters: 250.0,
            GpsFixAgeSeconds: 300.0,
            GpsProcessingMethod: "CELLID",
            GpsAltitude: 0.0
        },
        "GPS evidence should round-trip through the catalogue.");

    await repository.UpdateTitleAsync(metadataTarget.Path, "Smoke title");
    await repository.UpdateDescriptionAsync(
        metadataTarget.Path,
        "Smoke description");
    await repository.UpdateDescriptionEnAsync(
        metadataTarget.Path,
        "Smoke English description");
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
            DescriptionEn: "Smoke English description",
            Latitude: 49.195061,
            Longitude: 16.606836,
            GpsErrorMeters: null,
            GpsFixAgeSeconds: null,
            GpsProcessingMethod: null,
            GpsAltitude: null,
            Rating: 4
        },
        "Metadata updates should round-trip through the catalogue, and a "
        + "typed location should retire the fix evidence.");

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
        "Each metadata update should enqueue an outbox entry of its kind; "
        + "the English description stays out of the outbox entirely.");

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

    await repository.UpdateColorLabelAsync(metadataTarget.Path, ColorLabel.Green);
    await repository.UpdateKeywordsAsync(
        metadataTarget.Path,
        "Iceland; Waterfall");
    await repository.UpdateFlagAsync(metadataTarget.Path, PhotoFlag.Rejected);
    var organized = await repository.GetByPathAsync(metadataTarget.Path);
    Assert(
        organized is
        {
            ColorLabel: ColorLabel.Green,
            Flag: PhotoFlag.Rejected,
            Keywords: "Iceland; Waterfall"
        }
        && organized.KeywordList.SequenceEqual(["Iceland", "Waterfall"]),
        "Colour label, flag and keywords should round-trip through the catalogue.");

    var organizationOutbox = (await repository.GetPendingMetadataAsync(8))
        .Where(entry => string.Equals(
            entry.Path,
            metadataTarget.Path,
            StringComparison.OrdinalIgnoreCase))
        .ToArray();
    Assert(
        organizationOutbox.Any(entry => entry.Kind == "label")
        && organizationOutbox.Any(entry => entry.Kind == "keywords")
        && organizationOutbox.All(entry => entry.Kind != "flag"),
        "Labels and keywords belong in the photo file; the culling flag does not.");
    await repository.DeleteMetadataOutboxEntriesAsync(
        organizationOutbox.Select(entry => entry.Id).ToArray());
    await repository.UpdateFlagAsync(metadataTarget.Path, PhotoFlag.None);

    var organizationPayload = MetadataOutboxProcessor.MergePayload(
    [
        new MetadataOutboxEntry(9, "p", "label", """{"label":"Green"}""", 0),
        new MetadataOutboxEntry(
            10,
            "p",
            "keywords",
            """{"keywords":"Iceland; Waterfall"}""",
            0)
    ]);
    Assert(
        organizationPayload is
        {
            LabelChanged: true,
            Label: "Green",
            KeywordsChanged: true,
            Keywords: "Iceland; Waterfall"
        },
        "Label and keyword outbox entries should merge into the write payload.");

    var organizationArguments = ExifToolMetadataWriter.BuildArguments(
        @"C:\photos\a.jpg",
        organizationPayload,
        sidecar: false,
        createSidecar: false);
    Assert(
        organizationArguments.Contains("-XMP-xmp:Label=Green")
        && organizationArguments.Contains("-XMP-dc:Subject=")
        && organizationArguments.Contains("-XMP-dc:Subject+=Iceland")
        && organizationArguments.Contains("-XMP-dc:Subject+=Waterfall")
        && organizationArguments.ToList().IndexOf("-XMP-dc:Subject=")
            < organizationArguments.ToList().IndexOf("-XMP-dc:Subject+=Iceland"),
        "Keyword writes must clear the existing bag before appending, "
        + "otherwise keywords accumulate on every save.");

    Assert(
        PhotoMetadataReader.BuildCameraName("NIKON CORPORATION", "NIKON Z 6")
            == "NIKON Z 6"
        && PhotoMetadataReader.BuildCameraName("Canon", "EOS R6")
            == "Canon EOS R6"
        && PhotoMetadataReader.BuildCameraName(null, "X-T5") == "X-T5",
        "Camera names should not stutter the manufacturer twice.");

    AssertGalleryFiltering(repository);

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

    AssertImagingPipeline();
    AssertFolderNavigation(testRoot, photoRoot, nested);
    AssertRawPreviewExtraction();
    await AssertImportWorkflowAsync(testRoot);
    await AssertBatchProcessingAsync(testRoot, repository, previews);

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

static async Task AssertBatchProcessingAsync(
    string testRoot,
    PhotoCatalogRepository repository,
    PreviewService previews)
{
    var batchRoot = Directory.CreateDirectory(
        Path.Combine(testRoot, "batch-source")).FullName;
    var batchOutput = Path.Combine(testRoot, "batch-output");

    var wide = Path.Combine(batchRoot, "wide.png");
    var tall = Path.Combine(batchRoot, "tall.png");
    SaveTestPng(wide, 400, 200);
    SaveTestPng(tall, 200, 400);

    var sources = new[]
    {
        new BatchSource(
            wide,
            EditRecipe.Empty,
            new DateTime(2025, 7, 4, 9, 30, 0).Ticks,
            400,
            200),
        new BatchSource(
            tall,
            EditRecipe.Empty with { Rotation = QuarterRotation.Clockwise90 },
            new DateTime(2025, 7, 4, 9, 31, 0).Ticks,
            200,
            400)
    };

    Assert(
        BatchProcessor.MeasureResize(400, 200, new BatchPreset
        {
            ResizeMode = BatchResizeMode.LongestSide,
            ResizeValue = 200
        }) == (200, 100),
        "Longest-side resizing should preserve the aspect ratio.");
    Assert(
        BatchProcessor.MeasureResize(400, 200, new BatchPreset
        {
            ResizeMode = BatchResizeMode.Percentage,
            ResizeValue = 50
        }) == (200, 100),
        "Percentage resizing should scale both axes.");
    Assert(
        BatchProcessor.MeasureResize(400, 200, new BatchPreset
        {
            ResizeMode = BatchResizeMode.Width,
            ResizeValue = 800
        }) is null,
        "Resizing must not enlarge unless the preset allows it.");
    Assert(
        BatchProcessor.MeasureResize(400, 200, new BatchPreset
        {
            ResizeMode = BatchResizeMode.Width,
            ResizeValue = 800,
            AllowEnlarge = true
        }) == (800, 400),
        "Enlarging should happen when the preset opts into it.");
    Assert(
        BatchProcessor.MeasureResize(400, 200, new BatchPreset
        {
            ResizeMode = BatchResizeMode.ShortestSide,
            ResizeValue = 100
        }) == (200, 100),
        "Shortest-side resizing should target the smaller axis.");

    var namingPreset = new BatchPreset
    {
        OutputDirectory = batchOutput,
        Format = ImageOutputFormat.Jpeg,
        Prefix = "IS_",
        Suffix = "_web",
        NameSource = BatchNameSource.CustomText,
        CustomName = "iceland",
        UseSequentialNumbering = true,
        NumberStart = 7,
        NumberDigits = 3
    };
    Assert(
        BatchPlanner.BuildFileName(sources[0], namingPreset, 7)
            == "IS_iceland_web_007.jpg",
        "File names should combine prefix, base name, suffix and numbering.");
    Assert(
        BatchPlanner.BuildFileName(
            sources[0],
            namingPreset with
            {
                NameSource = BatchNameSource.DateTaken,
                DateFormat = "yyyy-MM-dd",
                UseSequentialNumbering = false
            },
            1) == "IS_2025-07-04_web.jpg",
        "Date-based names should use the capture date.");
    Assert(
        BatchPlanner.BuildFileName(
            sources[0],
            namingPreset with
            {
                NameSource = BatchNameSource.CustomText,
                CustomName = "bad:name*",
                UseSequentialNumbering = false,
                Prefix = string.Empty,
                Suffix = string.Empty
            },
            1) == "badname.jpg",
        "Names must be stripped of characters Windows cannot write.");

    var collidingPlan = BatchPlanner.Plan(
        sources,
        namingPreset with { UseSequentialNumbering = false });
    Assert(
        collidingPlan.Items.Select(item => item.DestinationPath).Distinct().Count()
            == 2,
        "Two sources that would produce the same name must not collide.");

    var skipPlan = BatchPlanner.Plan(
        sources,
        namingPreset with
        {
            UseSequentialNumbering = false,
            OverwritePolicy = BatchOverwritePolicy.Skip
        });
    Assert(
        skipPlan.SkipCount == 1 && skipPlan.WriteCount == 1,
        "A Skip policy should report the colliding photo as skipped.");

    var preset = new BatchPreset
    {
        Name = "Smoke web export",
        OutputDirectory = batchOutput,
        Format = ImageOutputFormat.Jpeg,
        Quality = 80,
        ResizeMode = BatchResizeMode.LongestSide,
        ResizeValue = 100,
        SharpenAmount = 25,
        MetadataPolicy = BatchMetadataPolicy.RemoveAll,
        Prefix = "web_"
    };

    var processor = new BatchProcessor(previews, new ExifToolMetadataWriter());
    var reports = new List<BatchProgress>();
    var outcome = await processor.RunAsync(
        BatchPlanner.Plan(sources, preset),
        preset,
        new Progress<BatchProgress>(reports.Add));

    Assert(
        outcome is { Written: 2, Failed: 0, Cancelled: false },
        "A two-photo batch should write both files: "
        + string.Join(" | ", outcome.Errors));

    var wideOutput = Path.Combine(batchOutput, "web_wide.jpg");
    var tallOutput = Path.Combine(batchOutput, "web_tall.jpg");
    Assert(
        File.Exists(wideOutput) && File.Exists(tallOutput),
        "Batch output should land in the chosen folder under the new names.");

    var wideResult = await previews.LoadAsync(wideOutput, 0, CancellationToken.None);
    var tallResult = await previews.LoadAsync(tallOutput, 0, CancellationToken.None);
    Assert(
        wideResult.PixelWidth == 100 && wideResult.PixelHeight == 50,
        $"The landscape photo should be resized to its longest side, "
        + $"got {wideResult.PixelWidth}x{wideResult.PixelHeight}.");
    Assert(
        tallResult.PixelWidth == 100 && tallResult.PixelHeight == 50,
        "A rotation in the recipe should be applied before the resize, "
        + $"got {tallResult.PixelWidth}x{tallResult.PixelHeight}.");

    // WebP has no dependable WIC encoder, so this proves the bundled
    // libwebp codec is actually registered and reachable.
    var webpPreset = preset with
    {
        Format = ImageOutputFormat.WebP,
        Prefix = "webp_",
        ResizeMode = BatchResizeMode.None,
        SharpenAmount = 0
    };
    var webpOutcome = await processor.RunAsync(
        BatchPlanner.Plan([sources[0]], webpPreset),
        webpPreset);
    var webpOutput = Path.Combine(batchOutput, "webp_wide.webp");
    Assert(
        webpOutcome.Written == 1 && File.Exists(webpOutput),
        "WebP conversion should produce a file: "
        + string.Join(" | ", webpOutcome.Errors));
    var webpBytes = await File.ReadAllBytesAsync(webpOutput);
    Assert(
        webpBytes.Length > 12
        && System.Text.Encoding.ASCII.GetString(webpBytes, 0, 4) == "RIFF"
        && System.Text.Encoding.ASCII.GetString(webpBytes, 8, 4) == "WEBP",
        "The WebP encoder should write a real RIFF/WEBP container.");

    // A JPEG stored upside down: Orientation 3 tells the viewer to turn it
    // 180°. The pipeline bakes that turn into the pixels, so the metadata
    // copied onto the output must say "normal" - any surviving rotation tag
    // flips every export on display.
    var rotated = Path.Combine(batchRoot, "rotated.jpg");
    SaveOrientedJpeg(rotated, 64, 48, orientation: 3);
    var orientedPreset = new BatchPreset
    {
        Name = "Smoke oriented export",
        OutputDirectory = batchOutput,
        Format = ImageOutputFormat.Jpeg,
        Quality = 95,
        MetadataPolicy = BatchMetadataPolicy.Preserve,
        Prefix = "oriented_"
    };
    var orientedOutcome = await processor.RunAsync(
        BatchPlanner.Plan(
            [new BatchSource(rotated, EditRecipe.Empty, null, 64, 48)],
            orientedPreset),
        orientedPreset);
    var orientedOutput = Path.Combine(batchOutput, "oriented_rotated.jpg");
    Assert(
        orientedOutcome is { Written: 1, Failed: 0 } && File.Exists(orientedOutput),
        "The oriented JPEG should convert: "
        + string.Join(" | ", orientedOutcome.Errors));

    var orientationTag = MetadataExtractor.ImageMetadataReader
        .ReadMetadata(orientedOutput)
        .OfType<ExifIfd0Directory>()
        .Select(directory =>
            MetadataExtractor.DirectoryExtensions.TryGetInt32(
                directory,
                ExifDirectoryBase.TagOrientation,
                out var value)
                ? value
                : 1)
        .FirstOrDefault(1);
    Assert(
        orientationTag == 1,
        "An export bakes the rotation into the pixels, so its EXIF "
        + $"orientation must be normal; the tag says {orientationTag}.");

    var orientedResult = await previews.LoadAsync(
        orientedOutput,
        0,
        CancellationToken.None);
    var orientedBgra = new FormatConvertedBitmap(
        orientedResult,
        PixelFormats.Bgra32,
        null,
        0);
    var corner = new byte[4];
    orientedBgra.CopyPixels(new Int32Rect(2, 2, 1, 1), corner, 4, 0);
    Assert(
        corner[0] > 200 && corner[1] > 200,
        "A 180° source orientation should leave the export upright on "
        + $"display; the top-left corner reads ({corner[2]},{corner[1]},{corner[0]}) "
        + "instead of the source's bottom-right gradient corner.");

    Assert(
        reports.Count > 0 && reports[^1].Completed == reports[^1].Total,
        "Batch progress should be reported and end at 100 %.");

    var store = new BatchPresetStore(repository);
    var seeded = await store.LoadAsync();
    Assert(
        seeded.Any(item => item.Name == "Facebook export")
        && seeded.Any(item => item.Name == "Small email photos"),
        "The starter presets should be seeded on first use.");

    await store.SaveAsync(preset);
    var reloaded = await store.LoadAsync();
    var roundTripped = reloaded.Single(item => item.Name == preset.Name);
    Assert(
        roundTripped.Quality == 80
        && roundTripped.ResizeMode == BatchResizeMode.LongestSide
        && roundTripped.ResizeValue == 100
        && roundTripped.Prefix == "web_",
        "A saved preset should round-trip through the catalogue.");

    await store.DeleteAsync(preset.Name);
    Assert(
        (await store.LoadAsync()).All(item => item.Name != preset.Name),
        "Deleting a preset should remove it.");

    var deletedBuiltIn = seeded.First(item => item.Name == "Web gallery");
    await store.DeleteAsync(deletedBuiltIn.Name);
    Assert(
        (await store.LoadAsync()).All(item => item.Name != deletedBuiltIn.Name),
        "A deleted starter preset must not come back on the next load.");
}

static void SaveTestPng(string path, int width, int height)
{
    var pixels = new byte[width * height * 4];
    for (var row = 0; row < height; row++)
    {
        for (var column = 0; column < width; column++)
        {
            var index = ((row * width) + column) * 4;
            pixels[index] = (byte)(column * 255 / Math.Max(1, width - 1));
            pixels[index + 1] = (byte)(row * 255 / Math.Max(1, height - 1));
            pixels[index + 2] = 140;
            pixels[index + 3] = 255;
        }
    }

    var bitmap = BitmapSource.Create(
        width,
        height,
        96,
        96,
        PixelFormats.Bgra32,
        null,
        pixels,
        width * 4);
    bitmap.Freeze();
    var encoder = new PngBitmapEncoder();
    encoder.Frames.Add(BitmapFrame.Create(bitmap));
    using var stream = File.Create(path);
    encoder.Save(stream);
}

static void AssertRawPreviewExtraction()
{
    Assert(
        RawImageDecoder.IsRaw(@"C:\photos\a.CR2")
        && RawImageDecoder.IsRaw(@"C:\photos\a.nef")
        && !RawImageDecoder.IsRaw(@"C:\photos\a.jpg"),
        "RAW detection should be extension-based and case-insensitive.");

    // A synthetic RAW container: a small thumbnail JPEG, then a larger
    // preview, wrapped in the sort of binary noise a real file carries.
    var thumbnail = CreateJpegBytes(64, 48);
    var preview = CreateJpegBytes(900, 600);
    Assert(
        preview.Length > 24 * 1024,
        "The synthetic preview must be large enough to be treated as one.");

    var container = new List<byte>();
    container.AddRange(Enumerable.Repeat((byte)0x2A, 512));
    container.AddRange(thumbnail);
    container.AddRange(Enumerable.Repeat((byte)0xFF, 64));
    container.AddRange(preview);
    container.AddRange(Enumerable.Repeat((byte)0x00, 256));

    var extracted = RawImageDecoder.ExtractLargestJpeg(container.ToArray());
    Assert(
        extracted is not null && extracted.SequenceEqual(preview),
        "The largest embedded JPEG should be recovered, not the thumbnail.");

    using var stream = new MemoryStream(extracted!);
    var decoded = BitmapFrame.Create(
        stream,
        BitmapCreateOptions.None,
        BitmapCacheOption.OnLoad);
    Assert(
        decoded.PixelWidth == 900 && decoded.PixelHeight == 600,
        "The recovered preview should decode at its own size.");

    Assert(
        RawImageDecoder.ExtractLargestJpeg(
            Enumerable.Repeat((byte)0x11, 4096).ToArray()) is null,
        "A file with no embedded JPEG should report none rather than throw.");
}

static void SaveOrientedJpeg(
    string path,
    int width,
    int height,
    ushort orientation)
{
    // The same gradient as SaveTestPng, so every corner is identifiable:
    // blue follows the column, green follows the row.
    var pixels = new byte[width * height * 4];
    for (var row = 0; row < height; row++)
    {
        for (var column = 0; column < width; column++)
        {
            var index = ((row * width) + column) * 4;
            pixels[index] = (byte)(column * 255 / Math.Max(1, width - 1));
            pixels[index + 1] = (byte)(row * 255 / Math.Max(1, height - 1));
            pixels[index + 2] = 140;
            pixels[index + 3] = 255;
        }
    }

    var bitmap = BitmapSource.Create(
        width,
        height,
        96,
        96,
        PixelFormats.Bgra32,
        null,
        pixels,
        width * 4);
    bitmap.Freeze();
    var metadata = new BitmapMetadata("jpg");
    metadata.SetQuery("/app1/ifd/{ushort=274}", orientation);
    var encoder = new JpegBitmapEncoder { QualityLevel = 95 };
    encoder.Frames.Add(BitmapFrame.Create(bitmap, null, metadata, null));
    using var stream = File.Create(path);
    encoder.Save(stream);
}

static byte[] CreateJpegBytes(int width, int height)
{
    // Noise rather than a flat colour, so the encoder cannot compress the
    // preview below the size that marks it as more than a thumbnail.
    var random = new Random(width * 7919);
    var pixels = new byte[width * height * 4];
    random.NextBytes(pixels);
    for (var index = 3; index < pixels.Length; index += 4)
    {
        pixels[index] = 255;
    }

    var bitmap = BitmapSource.Create(
        width,
        height,
        96,
        96,
        PixelFormats.Bgra32,
        null,
        pixels,
        width * 4);
    bitmap.Freeze();
    var encoder = new JpegBitmapEncoder { QualityLevel = 95 };
    encoder.Frames.Add(BitmapFrame.Create(bitmap));
    using var stream = new MemoryStream();
    encoder.Save(stream);
    return stream.ToArray();
}

static async Task AssertImportWorkflowAsync(string testRoot)
{
    var card = Directory.CreateDirectory(
        Path.Combine(testRoot, "card", "DCIM", "100CANON")).FullName;
    var library = Path.Combine(testRoot, "library");
    var backup = Path.Combine(testRoot, "library-backup");

    var first = Path.Combine(card, "IMG_0001.png");
    var second = Path.Combine(card, "IMG_0002.png");
    SaveTestPng(first, 60, 40);
    SaveTestPng(second, 40, 60);
    File.SetLastWriteTime(first, new DateTime(2025, 8, 3, 10, 15, 0));
    File.SetLastWriteTime(second, new DateTime(2025, 8, 4, 11, 30, 0));
    await File.WriteAllTextAsync(
        Path.Combine(card, "notes.txt"),
        "not a photograph");

    var importer = new ImportService();
    var options = new ImportOptions(
        Path.Combine(testRoot, "card"),
        library,
        IncludeSubfolders: true,
        OrganizeByDate: true,
        DateFolderFormat: "yyyy-MM-dd",
        RenameOnImport: true,
        RenamePattern: "yyyyMMdd_HHmmss",
        SkipAlreadyImported: true,
        BackupDirectory: backup);

    var plan = importer.Plan(options);
    Assert(
        plan.Candidates.Count == 2 && plan.NewCount == 2,
        "The import plan should find the photos and ignore other files.");
    Assert(
        plan.Candidates.All(item =>
            item.DestinationPath.Contains("2025-08-0", StringComparison.Ordinal)),
        "Organizing by date should place each photo in its own dated folder.");
    Assert(
        plan.Candidates.All(item =>
            Path.GetFileNameWithoutExtension(item.DestinationPath).Length == 15),
        "Renaming during import should use the capture timestamp.");

    var outcome = await importer.RunAsync(plan, options);
    Assert(
        outcome is { Imported: 2, Failed: 0 },
        "Both photos should import: " + string.Join(" | ", outcome.Errors));
    Assert(
        Directory.EnumerateFiles(library, "*", SearchOption.AllDirectories)
            .Count() == 2,
        "The library should contain exactly the imported photos.");
    Assert(
        Directory.EnumerateFiles(backup, "*", SearchOption.AllDirectories)
            .Count() == 2,
        "The second copy should mirror the library layout.");
    Assert(
        File.Exists(first) && File.Exists(second),
        "A plain import must leave the card untouched.");

    var repeatPlan = importer.Plan(options);
    Assert(
        repeatPlan.NewCount == 0 && repeatPlan.DuplicateCount == 2,
        "Re-importing the same card should recognise every file as already "
        + "imported rather than making a second copy.");

    var withoutSkip = importer.Plan(options with { SkipAlreadyImported = false });
    Assert(
        withoutSkip.NewCount == 2
        && withoutSkip.Candidates.All(item =>
            !File.Exists(item.DestinationPath)),
        "Turning the duplicate check off should plan fresh, non-colliding names.");

    var flatOptions = options with
    {
        OrganizeByDate = false,
        RenameOnImport = false,
        DestinationDirectory = Path.Combine(testRoot, "library-flat"),
        BackupDirectory = null
    };
    var flatPlan = importer.Plan(flatOptions);
    Assert(
        flatPlan.Candidates.All(item =>
            Path.GetDirectoryName(item.DestinationPath)
            == Path.GetFullPath(flatOptions.DestinationDirectory))
        && flatPlan.Candidates.Any(item =>
            Path.GetFileName(item.DestinationPath) == "IMG_0001.png"),
        "Without date folders or renaming, files keep their names in one folder.");
}

static void AssertFolderNavigation(
    string testRoot,
    string photoRoot,
    string nested)
{
    var history = new FolderHistory();
    Assert(
        !history.CanGoBack && !history.CanGoForward,
        "An empty history should offer neither direction.");

    history.Record(testRoot);
    Assert(
        !history.CanGoBack,
        "The folder a session opens with must not enable Back.");

    history.Record(photoRoot);
    history.Record(nested);
    Assert(
        history.CanGoBack && !history.CanGoForward,
        "Navigating forward should enable Back and nothing else.");
    Assert(
        history.GoBack() == photoRoot && history.CanGoForward,
        "Back should return the previous folder and enable Forward.");
    Assert(
        history.GoForward() == nested,
        "Forward should return the folder Back left behind.");

    history.GoBack();
    history.Record(testRoot);
    Assert(
        !history.CanGoForward,
        "Navigating somewhere new should abandon the forward entries.");

    history.Record(testRoot);
    Assert(
        history.GoBack() == photoRoot,
        "Re-opening the current folder must not create a duplicate entry.");

    var crumbs = BreadcrumbSegment.Build(
        nested,
        path => new RelayCommand(() => { }));
    Assert(
        crumbs.Count >= 3
        && crumbs[0].FullPath == Path.GetPathRoot(nested)
        && crumbs[^1].FullPath == Path.GetFullPath(nested)
        && crumbs[^1].IsLast
        && crumbs.Take(crumbs.Count - 1).All(crumb => !crumb.IsLast),
        "The breadcrumb should start at the drive root and end at the folder.");
    Assert(
        BreadcrumbSegment.Build(null, path => new RelayCommand(() => { })).Count == 0,
        "An unset folder should produce no breadcrumb.");

    Assert(
        MainWindow.ParseFilterDate("2025", endOfPeriod: false)
            == new DateTime(2025, 1, 1)
        && MainWindow.ParseFilterDate("2025", endOfPeriod: true)
            == new DateTime(2025, 12, 31, 23, 59, 59, 999).AddTicks(9999),
        "A year typed as an end date should mean the end of that year.");
    Assert(
        MainWindow.ParseFilterDate("2025-07", endOfPeriod: false)
            == new DateTime(2025, 7, 1)
        && MainWindow.ParseFilterDate("2025-07", endOfPeriod: true)
            == new DateTime(2025, 8, 1).AddTicks(-1),
        "A year and month should widen to the whole month.");
    Assert(
        MainWindow.ParseFilterDate("2025-07-04", endOfPeriod: false)
            == new DateTime(2025, 7, 4)
        && MainWindow.ParseFilterDate("  ", endOfPeriod: false) is null
        && MainWindow.ParseFilterDate("not a date", endOfPeriod: false) is null,
        "Full dates should parse and unusable text should simply not filter.");
}

static void AssertGalleryFiltering(PhotoCatalogRepository repository)
{
    PhotoItemViewModel Create(
        string fileName,
        Action<PhotoRecord> _,
        PhotoRecord record) =>
        new(record with { FileName = fileName }, EditRecipe.Empty, repository);

    var landscapeJpeg = Create(
        "beach.jpg",
        _ => { },
        new PhotoRecord(
            @"C:\p\beach.jpg",
            @"C:\p",
            "beach.jpg",
            ".jpg",
            2048,
            0,
            4,
            1,
            new DateTime(2025, 6, 1).Ticks,
            PhotoDateSource.ExifDateTimeOriginal,
            4,
            "Beach",
            null,
            null,
            null,
            ColorLabel.Green,
            PhotoFlag.None,
            "Iceland; Coast",
            6000,
            4000,
            "Canon EOS R6",
            "RF 24-70mm",
            35,
            2.8,
            0.004,
            200));
    var portraitRaw = Create(
        "glacier.cr2",
        _ => { },
        landscapeJpeg.Record with
        {
            Path = @"C:\p\glacier.cr2",
            FileName = "glacier.cr2",
            Extension = ".cr2",
            Rating = 2,
            ColorLabel = ColorLabel.Red,
            Flag = PhotoFlag.Rejected,
            Keywords = "Iceland; Ice",
            Title = null,
            PixelWidth = 4000,
            PixelHeight = 6000,
            Camera = "NIKON Z 6",
            Lens = "Z 24-70mm",
            TakenAtTicks = new DateTime(2024, 2, 3).Ticks
        });

    var photos = new[] { landscapeJpeg, portraitRaw };

    Assert(
        landscapeJpeg.Record.Orientation == PhotoOrientation.Landscape
        && portraitRaw.Record.Orientation == PhotoOrientation.Portrait,
        "Orientation should be derived from the stored pixel dimensions.");

    Assert(
        MainViewModel.BuildPhotoPresentation(
                photos,
                PhotoSortField.FileName,
                descending: false,
                PhotoFilterCriteria.None with { HideRejected = true })
            .Single() == landscapeJpeg,
        "Hiding rejects should remove them from the gallery.");

    Assert(
        MainViewModel.BuildPhotoPresentation(
                photos,
                PhotoSortField.FileName,
                descending: false,
                PhotoFilterCriteria.None with
                {
                    Formats = new HashSet<string>(StringComparer.OrdinalIgnoreCase)
                    {
                        ".cr2"
                    }
                })
            .Single() == portraitRaw,
        "Format filtering should match on the file extension.");

    Assert(
        MainViewModel.BuildPhotoPresentation(
                photos,
                PhotoSortField.FileName,
                descending: false,
                PhotoFilterCriteria.None with
                {
                    Orientation = PhotoOrientation.Portrait
                })
            .Single() == portraitRaw,
        "Orientation filtering should use the derived orientation.");

    Assert(
        MainViewModel.BuildPhotoPresentation(
                photos,
                PhotoSortField.FileName,
                descending: false,
                PhotoFilterCriteria.None with
                {
                    Cameras = new HashSet<string>(StringComparer.OrdinalIgnoreCase)
                    {
                        "NIKON Z 6"
                    }
                })
            .Single() == portraitRaw,
        "Camera filtering should match the indexed camera name.");

    Assert(
        MainViewModel.BuildPhotoPresentation(
                photos,
                PhotoSortField.FileName,
                descending: false,
                PhotoFilterCriteria.None with
                {
                    ColorLabels = new HashSet<ColorLabel> { ColorLabel.Green }
                })
            .Single() == landscapeJpeg,
        "Colour-label filtering should match the stored label.");

    Assert(
        MainViewModel.BuildPhotoPresentation(
                photos,
                PhotoSortField.FileName,
                descending: false,
                PhotoFilterCriteria.None with
                {
                    TakenFrom = new DateTime(2025, 1, 1)
                })
            .Single() == landscapeJpeg,
        "Date filtering should compare against the capture date.");

    Assert(
        MainViewModel.BuildPhotoPresentation(
            photos,
            PhotoSortField.FileName,
            descending: false,
            PhotoFilterCriteria.None with { SearchText = "iceland" }).Count == 2,
        "Search should also look inside keywords, not only file names.");

    Assert(
        MainViewModel.BuildPhotoPresentation(
                photos,
                PhotoSortField.FileName,
                descending: false,
                PhotoFilterCriteria.None with { SearchText = "ice" })
            .Count == 2
        && MainViewModel.BuildPhotoPresentation(
                photos,
                PhotoSortField.FileName,
                descending: false,
                PhotoFilterCriteria.None with { SearchText = "beach" })
            .Single() == landscapeJpeg,
        "Search should match captions and file names too.");

    var bySize = MainViewModel.BuildPhotoPresentation(
        photos,
        PhotoSortField.Dimensions,
        descending: true,
        PhotoFilterCriteria.None);
    Assert(
        bySize.Count == 2,
        "Sorting by dimensions should keep every photo in the gallery.");

    Assert(
        !PhotoFilterCriteria.None.IsActive
        && PhotoFilterCriteria.None.Describe() == "Filter"
        && (PhotoFilterCriteria.None with { MinimumRating = 3 }).IsActive,
        "An untouched filter must report itself as inactive.");
}

static void AssertImagingPipeline()
{
    var flatGray = CreateTestBitmap(48, 32, 90, 90, 90);

    var brightened = ImageRenderer.Render(
        flatGray,
        EditRecipe.Empty with
        {
            Adjustments = PhotoAdjustments.Neutral with { Exposure = 1 }
        });
    Assert(
        ReadPixel(brightened, 10, 10).Red > 150,
        "A one-stop exposure lift should visibly brighten a flat frame.");

    var darkened = ImageRenderer.Render(
        flatGray,
        EditRecipe.Empty with
        {
            Adjustments = PhotoAdjustments.Neutral with { Exposure = -1 }
        });
    Assert(
        ReadPixel(darkened, 10, 10).Red < 60,
        "A one-stop exposure cut should visibly darken a flat frame.");

    var saturatedRed = CreateTestBitmap(16, 16, 220, 40, 40);
    var grayscaled = ImageRenderer.Render(
        saturatedRed,
        EditRecipe.Empty with
        {
            Filters = [new FilterStep(PhotoFilterKind.Grayscale, 100)]
        });
    var grayPixel = ReadPixel(grayscaled, 8, 8);
    Assert(
        Math.Abs(grayPixel.Red - grayPixel.Green) <= 1
        && Math.Abs(grayPixel.Green - grayPixel.Blue) <= 1,
        "The grayscale filter should equalize the three channels.");

    var desaturated = ImageRenderer.Render(
        saturatedRed,
        EditRecipe.Empty with
        {
            Adjustments = PhotoAdjustments.Neutral with { Saturation = -100 }
        });
    var desaturatedPixel = ReadPixel(desaturated, 8, 8);
    Assert(
        Math.Abs(desaturatedPixel.Red - desaturatedPixel.Blue) <= 2,
        "Pulling saturation to its minimum should neutralize a red frame.");

    var lifted = ImageRenderer.Render(
        flatGray,
        EditRecipe.Empty with
        {
            Adjustments = PhotoAdjustments.Neutral with
            {
                Curve = ToneCurve.FromPoints(
                [
                    new CurvePoint(0, 0),
                    new CurvePoint(0.35, 0.6),
                    new CurvePoint(1, 1)
                ])
            }
        });
    Assert(
        ReadPixel(lifted, 10, 10).Red > 110,
        "A curve that lifts the midtones should brighten a midtone frame.");

    var linearTable = ToneCurve.Linear.Sample();
    Assert(
        linearTable[0] == 0 && Math.Abs(linearTable[255] - 1) < 0.0001,
        "A linear curve should sample to the identity ramp.");

    var oriented = ImageRenderer.Render(
        CreateTestBitmap(6, 4, 10, 10, 10),
        EditRecipe.Empty with { Rotation = QuarterRotation.Clockwise90 });
    Assert(
        oriented.PixelWidth == 4 && oriented.PixelHeight == 6,
        "A quarter rotation should swap the rendered dimensions.");
    Assert(
        ImageRenderer.MeasureOutput(
            6,
            4,
            EditRecipe.Empty with
            {
                Rotation = QuarterRotation.Clockwise90,
                Crop = new CropRegion(0, 0, 0.5, 1)
            }) == (4, 3),
        "Measuring an output should combine crop and rotation without rendering.");

    var straightened = ImageRenderer.Render(
        CreateTestBitmap(64, 48, 200, 180, 160),
        EditRecipe.Empty with { StraightenAngle = 7 });
    Assert(
        straightened.PixelWidth == 64 && straightened.PixelHeight == 48,
        "Straightening should keep the frame size.");
    Assert(
        ReadPixel(straightened, 1, 1).Alpha == 255
        && ReadPixel(straightened, 62, 46).Alpha == 255,
        "Straightening should scale up so no transparent corner survives.");

    var dark = CreateTestBitmap(64, 64, 34, 33, 30);
    var autoFixed = AutoFixAnalyzer.Analyze(
        PixelBuffer.FromBitmap(dark),
        PhotoAdjustments.Neutral);
    Assert(
        autoFixed.Exposure > 0.2 && autoFixed.Exposure <= 0.75,
        "Auto Fix should lift a dark frame without exceeding its safety limit.");
    Assert(
        Math.Abs(autoFixed.Temperature) <= 22
        && Math.Abs(autoFixed.Tint) <= 18,
        "Auto Fix white balance must stay inside its conservative range.");

    var alreadyGood = AutoFixAnalyzer.Analyze(
        PixelBuffer.FromBitmap(CreateGradientBitmap(128, 64)),
        PhotoAdjustments.Neutral);
    Assert(
        Math.Abs(alreadyGood.Exposure) < 0.35
        && alreadyGood.Contrast < 12,
        "Auto Fix should barely touch a frame that already uses the full range.");

    var histogram = HistogramData.FromBitmap(CreateGradientBitmap(256, 8));
    Assert(
        histogram.Total > 0
        && histogram.GetLuminancePercentile(0.02) < 40
        && histogram.GetLuminancePercentile(0.98) > 200,
        "A full-range gradient should report a full-range histogram.");

    var layer = new ShapeLayer
    {
        Id = "arrow-1",
        Shape = ShapeKind.Arrow,
        X1 = 0.1,
        Y1 = 0.1,
        X2 = 0.8,
        Y2 = 0.6
    };
    var withLayer = EditRecipe.Empty.WithLayer(layer);
    Assert(
        withLayer != EditRecipe.Empty
        && withLayer == EditRecipe.Empty.WithLayer(layer)
        && withLayer.WithoutLayer("arrow-1") == EditRecipe.Empty,
        "Recipes must compare layers and filters by value, not by reference.");
    Assert(
        withLayer.WithLayer(layer with { X1 = 0.2 }).Layers.Count == 1,
        "Storing a layer twice should replace it instead of duplicating it.");

    var annotated = ImageRenderer.Render(
        CreateTestBitmap(200, 200, 250, 250, 250),
        withLayer);
    Assert(
        annotated.PixelWidth == 200 && CountMarkedPixels(annotated) > 40,
        "An arrow layer should be composited into the rendered image.");

    var withoutLayers = ImageRenderer.Render(
        CreateTestBitmap(200, 200, 250, 250, 250),
        withLayer,
        new RenderRequest(IncludeLayers: false));
    Assert(
        CountMarkedPixels(withoutLayers) == 0,
        "The editor render path must be able to leave layers to the canvas.");

    var resized = ImageRenderer.Resize(CreateTestBitmap(200, 100, 5, 5, 5), 50);
    Assert(
        resized.PixelWidth == 50 && resized.PixelHeight == 25,
        "Resizing should fit the longest side and preserve the aspect ratio.");
    Assert(
        ImageRenderer.Resize(CreateTestBitmap(20, 10, 5, 5, 5), 500).PixelWidth == 20,
        "Resizing must never enlarge an image.");

    var legacyRecipeJson = JsonSerializer.Deserialize<EditRecipe>(
        """{"Rotation":1,"FlipHorizontal":true}""");
    Assert(
        legacyRecipeJson is
        {
            Rotation: QuarterRotation.Clockwise90,
            FlipHorizontal: true
        }
        && legacyRecipeJson.Adjustments.IsNeutral
        && legacyRecipeJson.Layers.Count == 0,
        "Recipes stored before adjustments existed must still load.");

    var roundTripped = JsonSerializer.Deserialize<EditRecipe>(
        JsonSerializer.Serialize(
            withLayer with
            {
                Adjustments = PhotoAdjustments.Neutral with
                {
                    Exposure = 0.4,
                    Curve = ToneCurve.FromPoints(
                        [new CurvePoint(0, 0.1), new CurvePoint(1, 0.9)])
                },
                Filters = [FilterStep.CreateDefault(PhotoFilterKind.Sepia)]
            }));
    Assert(
        roundTripped is not null
        && roundTripped.Adjustments.Exposure == 0.4
        && roundTripped.Filters.Count == 1
        && roundTripped.Layers.Single() is ShapeLayer { Shape: ShapeKind.Arrow }
        && !roundTripped.Adjustments.Curve.IsLinear,
        "A full recipe should survive a JSON round-trip including layer types.");
}

static BitmapSource CreateTestBitmap(
    int width,
    int height,
    byte red,
    byte green,
    byte blue)
{
    var pixels = new byte[width * height * 4];
    for (var index = 0; index < pixels.Length; index += 4)
    {
        pixels[index] = blue;
        pixels[index + 1] = green;
        pixels[index + 2] = red;
        pixels[index + 3] = 255;
    }

    var bitmap = BitmapSource.Create(
        width,
        height,
        96,
        96,
        PixelFormats.Bgra32,
        null,
        pixels,
        width * 4);
    bitmap.Freeze();
    return bitmap;
}

static BitmapSource CreateGradientBitmap(int width, int height)
{
    var pixels = new byte[width * height * 4];
    for (var row = 0; row < height; row++)
    {
        for (var column = 0; column < width; column++)
        {
            var value = (byte)(column * 255 / Math.Max(1, width - 1));
            var index = ((row * width) + column) * 4;
            pixels[index] = value;
            pixels[index + 1] = value;
            pixels[index + 2] = value;
            pixels[index + 3] = 255;
        }
    }

    var bitmap = BitmapSource.Create(
        width,
        height,
        96,
        96,
        PixelFormats.Bgra32,
        null,
        pixels,
        width * 4);
    bitmap.Freeze();
    return bitmap;
}

static int CountMarkedPixels(BitmapSource bitmap)
{
    var converted = bitmap.Format == PixelFormats.Bgra32
        ? bitmap
        : new FormatConvertedBitmap(bitmap, PixelFormats.Bgra32, null, 0);
    var pixels = new byte[converted.PixelWidth * converted.PixelHeight * 4];
    converted.CopyPixels(pixels, converted.PixelWidth * 4, 0);
    var marked = 0;
    for (var index = 0; index < pixels.Length; index += 4)
    {
        if (pixels[index] < 200 || pixels[index + 1] < 200)
        {
            marked++;
        }
    }

    return marked;
}

static (byte Blue, byte Green, byte Red, byte Alpha) ReadPixel(
    BitmapSource bitmap,
    int x,
    int y)
{
    var converted = bitmap.Format == PixelFormats.Bgra32
        ? bitmap
        : new FormatConvertedBitmap(bitmap, PixelFormats.Bgra32, null, 0);
    var pixel = new byte[4];
    converted.CopyPixels(new Int32Rect(x, y, 1, 1), pixel, 4, 0);
    return (pixel[0], pixel[1], pixel[2], pixel[3]);
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

                // An editable combo is how people get named and models get
                // typed; the dark template must carry the text-entry part.
                var editableCombo = new System.Windows.Controls.ComboBox
                {
                    IsEditable = true,
                    Style = (Style)application.FindResource(
                        typeof(System.Windows.Controls.ComboBox))
                };
                editableCombo.ApplyTemplate();
                if (editableCombo.Template.FindName(
                        "PART_EditableTextBox",
                        editableCombo)
                    is not System.Windows.Controls.TextBox
                    {
                        Visibility: Visibility.Visible
                    })
                {
                    throw new InvalidOperationException(
                        "The ComboBox template must expose a visible "
                        + "PART_EditableTextBox when IsEditable is set; "
                        + "without it typed text goes nowhere.");
                }

                // The filter dialog opens as its own window, so every
                // StaticResource it names must resolve from application
                // scope alone; a key that only exists in MainWindow's
                // dictionary crashes the app on the Filters… click.
                var filterDialog = new PhotoSite.Dialogs.FilterDialog(
                    pastedBitmap);
                if (filterDialog.FilterList.Items.Count
                    != Enum.GetValues<PhotoFilterKind>().Length)
                {
                    throw new InvalidOperationException(
                        "The filter dialog must list every filter kind.");
                }

                filterDialog.Close();
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
                window.ValidateManagerChromeForSmokeTest();
                window.ValidateDateFilterCalendarForSmokeTest();
                window.ValidateEditorPanelForSmokeTest();
                window.ValidateEditorTabsForSmokeTest();
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

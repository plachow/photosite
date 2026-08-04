using System.ComponentModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Microsoft.Win32;
using System.Windows.Media.Imaging;
using PhotoSite.Domain;
using PhotoSite.Infrastructure;
using PhotoSite.Services;

namespace PhotoSite.ViewModels;

public sealed class MainViewModel : ObservableObject
{
    private const int DatabaseBatchSize = 200;
    private const string LastDirectorySetting = "last_directory";
    private const string LastPhotoSetting = "last_photo";
    private const string IncludeSubfoldersSetting = "include_subfolders";
    // Only a backstop against spinning on a permanently locked file - a large
    // RAW landing over a slow link legitimately stays open for a while.
    private const int MaxWatcherRetries = 40;
    private static readonly TimeSpan WatcherDebounceDelay =
        TimeSpan.FromMilliseconds(600);
    // A sustained copy fires events faster than the debounce window, so cap
    // how long the oldest pending change may be held back - without this the
    // gallery stays empty until the whole import goes quiet.
    private static readonly TimeSpan WatcherMaxWait = TimeSpan.FromSeconds(2);
    private readonly PhotoCatalogRepository catalog;
    private readonly PhotoIndexer indexer;
    private readonly List<PhotoItemViewModel> allPhotos = [];
    private readonly Dictionary<string, int> photoIndexByPath =
        new(StringComparer.OrdinalIgnoreCase);
    private readonly SynchronizationContext? uiContext;
    private readonly object watcherGate = new();
    private readonly Dictionary<string, WatcherChangeTypes> pendingWatcherPaths =
        new(StringComparer.OrdinalIgnoreCase);
    private readonly Dictionary<string, int> watcherRetryCounts =
        new(StringComparer.OrdinalIgnoreCase);
    private readonly System.Threading.Timer watcherTimer;
    private DateTime firstPendingUtc = DateTime.MaxValue;
    private int folderGeneration;
    private bool watcherFlushRunning;
    private bool suppressPresentationRefresh;
    private FileSystemWatcher? folderWatcher;
    private CancellationTokenSource? scanCancellation;
    private PhotoItemViewModel? selectedPhoto;
    private PhotoItemViewModel? selectionBeforeTransientDocument;
    private string? currentFolder;
    private string photoCountText = "0 photos";
    private string statusText = "Choose a folder to begin";
    private bool isBusy;
    private bool isEditorMode;
    private bool isFullscreenMode;
    private bool includeSubfolders = true;
    private PhotoSortField sortField = PhotoSortField.TakenAt;
    private bool sortDescending = true;
    private int minimumRating;
    private string searchText = string.Empty;

    public MainViewModel(
        PhotoCatalogRepository catalog,
        PhotoIndexer indexer)
    {
        this.catalog = catalog;
        this.indexer = indexer;
        uiContext = SynchronizationContext.Current;
        watcherTimer = new System.Threading.Timer(
            _ => PostWatcherFlush(),
            null,
            Timeout.InfiniteTimeSpan,
            Timeout.InfiniteTimeSpan);
        DirectoryTree = new DirectoryTreeViewModel(
            path => _ = LoadFolderAsync(path, CancellationToken.None));
        OpenFolderCommand = new AsyncRelayCommand(OpenFolderAsync);
        PreviousCommand = new RelayCommand(SelectPrevious, () => SelectedPhoto is not null);
        NextCommand = new RelayCommand(SelectNext, () => SelectedPhoto is not null);
        ShowEditorCommand = new RelayCommand(
            ShowEditor,
            () => SelectedPhoto is not null
                  && !IsEditorMode
                  && !IsFullscreenMode);
        ShowManagerCommand = new RelayCommand(
            ShowManager,
            () => IsEditorMode && !IsFullscreenMode);
        ToggleEditorCommand = new RelayCommand(
            ToggleEditor,
            () => SelectedPhoto is not null && !IsFullscreenMode);
        ToggleFullscreenCommand = new RelayCommand(
            ToggleFullscreen,
            () => SelectedPhoto is not null);
        SortByTakenAtCommand = new RelayCommand(
            () => ChangeSort(PhotoSortField.TakenAt));
        SortByFileNameCommand = new RelayCommand(
            () => ChangeSort(PhotoSortField.FileName));
        SortByRatingCommand = new RelayCommand(
            () => ChangeSort(PhotoSortField.Rating));
    }

    public BulkObservableCollection<PhotoItemViewModel> Photos { get; } = new();

    public DirectoryTreeViewModel DirectoryTree { get; }

    public IAsyncRelayCommand OpenFolderCommand { get; }

    public IRelayCommand PreviousCommand { get; }

    public IRelayCommand NextCommand { get; }

    public IRelayCommand ShowEditorCommand { get; }

    public IRelayCommand ShowManagerCommand { get; }

    public IRelayCommand ToggleEditorCommand { get; }

    public IRelayCommand ToggleFullscreenCommand { get; }

    public IRelayCommand SortByTakenAtCommand { get; }

    public IRelayCommand SortByFileNameCommand { get; }

    public IRelayCommand SortByRatingCommand { get; }

    public string TakenAtSortLabel => BuildSortLabel(
        PhotoSortField.TakenAt,
        "Date taken");

    public string FileNameSortLabel => BuildSortLabel(
        PhotoSortField.FileName,
        "File name");

    public string RatingSortLabel => BuildSortLabel(
        PhotoSortField.Rating,
        "Rating");

    public string FolderScopeToolTip => IncludeSubfolders
        ? "Including subfolders"
        : "Current folder only";

    public int MinimumRating
    {
        get => minimumRating;
        set
        {
            var valid = Math.Clamp(value, 0, 5);
            if (SetProperty(ref minimumRating, valid))
            {
                ApplyPhotoPresentation();
            }
        }
    }

    public string SearchText
    {
        get => searchText;
        set
        {
            if (SetProperty(ref searchText, value ?? string.Empty))
            {
                ApplyPhotoPresentation();
            }
        }
    }

    public PhotoItemViewModel? SelectedPhoto
    {
        get => selectedPhoto;
        set
        {
            if (!SetProperty(ref selectedPhoto, value))
            {
                return;
            }

            PreviousCommand.NotifyCanExecuteChanged();
            NextCommand.NotifyCanExecuteChanged();
            ShowEditorCommand.NotifyCanExecuteChanged();
            ToggleEditorCommand.NotifyCanExecuteChanged();
            ToggleFullscreenCommand.NotifyCanExecuteChanged();
            if (value is null)
            {
                IsFullscreenMode = false;
                IsEditorMode = false;
            }
        }
    }

    public string? CurrentFolder
    {
        get => currentFolder;
        private set => SetProperty(ref currentFolder, value);
    }

    public string StatusText
    {
        get => statusText;
        private set => SetProperty(ref statusText, value);
    }

    public string PhotoCountText
    {
        get => photoCountText;
        private set => SetProperty(ref photoCountText, value);
    }

    public bool IsBusy
    {
        get => isBusy;
        private set => SetProperty(ref isBusy, value);
    }

    public bool IsDirectPhotoLaunch { get; private set; }

    public bool IsEditorMode
    {
        get => isEditorMode;
        private set
        {
            if (!SetProperty(ref isEditorMode, value))
            {
                return;
            }

            ShowEditorCommand.NotifyCanExecuteChanged();
            ShowManagerCommand.NotifyCanExecuteChanged();
            ToggleEditorCommand.NotifyCanExecuteChanged();
        }
    }

    public bool IsFullscreenMode
    {
        get => isFullscreenMode;
        private set
        {
            if (!SetProperty(ref isFullscreenMode, value))
            {
                return;
            }

            ShowEditorCommand.NotifyCanExecuteChanged();
            ShowManagerCommand.NotifyCanExecuteChanged();
            ToggleEditorCommand.NotifyCanExecuteChanged();
        }
    }

    public bool IncludeSubfolders
    {
        get => includeSubfolders;
        set
        {
            if (SetProperty(ref includeSubfolders, value))
            {
                NotifyFolderScopeChanged();
                _ = ApplyFolderScopeChangeAsync();
            }
        }
    }

    public void ReportStatus(string message)
    {
        StatusText = message;
    }

    public Task InitializeAsync(CancellationToken cancellationToken = default) =>
        InitializeAsync(startupPath: null, cancellationToken);

    public async Task InitializeAsync(
        string? startupPath,
        CancellationToken cancellationToken)
    {
        var startupTarget = ResolveStartupTarget(startupPath);
        var savedScope = await catalog.GetSettingAsync(
            IncludeSubfoldersSetting,
            cancellationToken);
        if (bool.TryParse(savedScope, out var savedIncludeSubfolders)
            && includeSubfolders != savedIncludeSubfolders)
        {
            includeSubfolders = savedIncludeSubfolders;
            OnPropertyChanged(nameof(IncludeSubfolders));
            NotifyFolderScopeChanged();
        }

        if (startupTarget.PhotoPath is not null
            && startupTarget.DirectoryPath is not null)
        {
            await InitializeDirectPhotoAsync(
                startupTarget.DirectoryPath,
                startupTarget.PhotoPath,
                cancellationToken);
            return;
        }

        var savedDirectory = await catalog.GetSettingAsync(
            LastDirectorySetting,
            cancellationToken);
        var savedPhoto = await catalog.GetSettingAsync(
            LastPhotoSetting,
            cancellationToken);
        var initialDirectory = startupTarget.DirectoryPath
                               ?? ResolveInitialDirectory(savedDirectory);
        if (initialDirectory is null)
        {
            StatusText = "No accessible directory was found";
            return;
        }

        await DirectoryTree.SelectPathAsync(
            initialDirectory,
            notifySelection: false);
        await LoadFolderAsync(
            initialDirectory,
            cancellationToken,
            startupTarget.PhotoPath
            ?? (startupTarget.DirectoryPath is null ? savedPhoto : null));

        IsEditorMode = false;

        if (startupTarget.ErrorMessage is not null)
        {
            StatusText = startupTarget.ErrorMessage;
        }
    }

    internal static bool IsDirectPhotoStartup(string? startupPath) =>
        ResolveStartupTarget(startupPath).PhotoPath is not null;

    private async Task InitializeDirectPhotoAsync(
        string directoryPath,
        string photoPath,
        CancellationToken cancellationToken)
    {
        var rootFolder = Path.GetFullPath(directoryPath);
        var fullPhotoPath = Path.GetFullPath(photoPath);
        CurrentFolder = rootFolder;
        StartFolderWatcher(rootFolder, IncludeSubfolders);
        IsBusy = true;

        try
        {
            await catalog.SetSettingAsync(
                LastDirectorySetting,
                rootFolder,
                cancellationToken);

            var cached = FilterRecordsForScope(
                    await catalog.GetByRootAsync(rootFolder, cancellationToken),
                    rootFolder,
                    IncludeSubfolders)
                .ToList();
            var targetIndex = cached.FindIndex(
                record => string.Equals(
                    record.Path,
                    fullPhotoPath,
                    StringComparison.OrdinalIgnoreCase));
            var cachedTarget = targetIndex >= 0
                ? cached[targetIndex]
                : await catalog.GetByPathAsync(
                    fullPhotoPath,
                    cancellationToken);
            var target = CreateDirectPhotoRecord(
                fullPhotoPath,
                rootFolder,
                cachedTarget);

            await catalog.UpsertBatchAsync([target], cancellationToken);
            if (targetIndex >= 0)
            {
                cached[targetIndex] = target;
            }
            else
            {
                cached.Add(target);
            }

            await ReplacePhotosAsync(
                cached,
                rootFolder,
                fullPhotoPath,
                cancellationToken);
            var startupPhoto = Photos.FirstOrDefault(
                photo => string.Equals(
                    photo.Path,
                    fullPhotoPath,
                    StringComparison.OrdinalIgnoreCase));
            if (startupPhoto is null)
            {
                StatusText = $"Photo could not be opened: {fullPhotoPath}";
                return;
            }

            SelectedPhoto = startupPhoto;
            IsDirectPhotoLaunch = true;
            ShowEditor();
            SetPhotoStatus(cached.Count);
        }
        finally
        {
            IsBusy = false;
        }
    }

    private static PhotoRecord CreateDirectPhotoRecord(
        string photoPath,
        string rootFolder,
        PhotoRecord? cached)
    {
        var file = new FileInfo(photoPath);
        file.Refresh();
        if (!file.Exists)
        {
            throw new FileNotFoundException(
                "The directly opened photo no longer exists.",
                photoPath);
        }

        var scanId = DateTime.UtcNow.Ticks;
        if (cached is not null && PhotoIndexer.CanReuseMetadata(file, cached))
        {
            return cached with
            {
                RootPath = rootFolder,
                ScanId = scanId
            };
        }

        var metadata = PhotoMetadataReader.ReadAll(file.FullName);
        return new PhotoRecord(
            file.FullName,
            rootFolder,
            file.Name,
            file.Extension,
            file.Length,
            file.LastWriteTimeUtc.Ticks,
            metadata.Rating > 0 ? metadata.Rating : cached?.Rating ?? 0,
            scanId,
            metadata.TakenAt.Ticks,
            metadata.TakenAt.Source,
            MetadataVersion: PhotoMetadataReader.CurrentVersion,
            metadata.Title,
            metadata.Description,
            metadata.Latitude,
            metadata.Longitude);
    }

    public async Task LoadFolderAsync(string folder, CancellationToken cancellationToken)
    {
        await LoadFolderAsync(
            folder,
            cancellationToken,
            preferredPhotoPath: null);
    }

    public async Task OpenSelectedPhotoFolderInManagerAsync(
        CancellationToken cancellationToken = default)
    {
        if (SelectedPhoto is not { IsTransient: false } photo)
        {
            return;
        }

        var folder = Path.GetDirectoryName(photo.Path);
        if (string.IsNullOrWhiteSpace(folder) || !Directory.Exists(folder))
        {
            StatusText = $"Photo directory does not exist: {folder}";
            return;
        }

        var photoPath = photo.Path;
        await DirectoryTree.SelectPathAsync(
            folder,
            notifySelection: false);
        await LoadFolderAsync(
            folder,
            cancellationToken,
            photoPath);
        IsFullscreenMode = false;
        ShowManager();
    }

    public async Task SaveSessionAsync(
        CancellationToken cancellationToken = default)
    {
        if (SelectedPhoto is not { IsTransient: false } photo)
        {
            return;
        }

        await catalog.SetSettingAsync(
            LastPhotoSetting,
            photo.Path,
            cancellationToken);
    }

    private async Task LoadFolderAsync(
        string folder,
        CancellationToken cancellationToken,
        string? preferredPhotoPath)
    {
        if (!Directory.Exists(folder))
        {
            StatusText = $"Directory no longer exists: {folder}";
            return;
        }

        scanCancellation?.Cancel();
        scanCancellation?.Dispose();
        scanCancellation = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        var activeScan = scanCancellation;
        var token = activeScan.Token;
        var rootFolder = Path.GetFullPath(folder);
        var includeSubfolders = IncludeSubfolders;

        CurrentFolder = rootFolder;
        StartFolderWatcher(rootFolder, includeSubfolders);
        IsBusy = true;

        try
        {
            await catalog.SetSettingAsync(
                LastDirectorySetting,
                rootFolder,
                token);
            var cached = FilterRecordsForScope(
                await catalog.GetByRootAsync(rootFolder, token),
                rootFolder,
                includeSubfolders);
            var cachedByPath = cached.ToDictionary(
                record => record.Path,
                StringComparer.OrdinalIgnoreCase);
            if (cached.Count > 0)
            {
                await ReplacePhotosAsync(
                    cached,
                    rootFolder,
                    preferredPhotoPath,
                    token);
                SetPhotoStatus(
                    cached.Count,
                    "cached · checking for changes…");
            }
            else
            {
                ReplaceAllPhotos([], preferredPhotoPath: null);
                SetPhotoStatus(0, "Indexing…");
            }

            var scanId = DateTime.UtcNow.Ticks;
            var batch = new List<PhotoRecord>(DatabaseBatchSize);
            var seenPaths = new HashSet<string>(
                StringComparer.OrdinalIgnoreCase);
            var discovered = 0;

            await foreach (var result in indexer.ScanAsync(
                               rootFolder,
                               scanId,
                               includeSubfolders,
                               token,
                               cachedByPath))
            {
                var record = result.Record;
                seenPaths.Add(record.Path);
                discovered++;

                if (result.RequiresUpsert)
                {
                    batch.Add(record);
                    if (batch.Count >= DatabaseBatchSize)
                    {
                        await catalog.UpsertBatchAsync(batch, token);
                        batch.Clear();
                    }
                }

                if (discovered % 1000 == 0)
                {
                    SetPhotoStatus(
                        cached.Count > 0 ? cached.Count : discovered,
                        $"{discovered:N0} checked…");
                }
            }

            await catalog.UpsertBatchAsync(batch, token);
            var missingPaths = cached
                .Where(record => !seenPaths.Contains(record.Path))
                .Select(record => record.Path)
                .ToArray();
            await catalog.DeleteByPathsAsync(missingPaths, token);
            var current = FilterRecordsForScope(
                await catalog.GetByRootAsync(rootFolder, token),
                rootFolder,
                includeSubfolders);
            await ReplacePhotosAsync(
                current,
                rootFolder,
                preferredPhotoPath,
                token);
            SetPhotoStatus(current.Count);
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested)
        {
            if (ReferenceEquals(scanCancellation, activeScan))
            {
                StatusText = "Indexing cancelled";
            }
        }
        catch (Exception exception)
        {
            if (ReferenceEquals(scanCancellation, activeScan))
            {
                StatusText = $"Indexing failed: {exception.Message}";
            }
        }
        finally
        {
            if (ReferenceEquals(scanCancellation, activeScan))
            {
                IsBusy = false;
            }
        }
    }

    private async Task OpenFolderAsync()
    {
        var dialog = new OpenFolderDialog
        {
            Title = "Choose a photo folder",
            Multiselect = false
        };

        if (dialog.ShowDialog() == true)
        {
            await DirectoryTree.SelectPathAsync(
                dialog.FolderName,
                notifySelection: false);
            await LoadFolderAsync(dialog.FolderName, CancellationToken.None);
        }
    }

    public static string? ResolveInitialDirectory(string? savedDirectory)
    {
        if (!string.IsNullOrWhiteSpace(savedDirectory)
            && Directory.Exists(savedDirectory))
        {
            return Path.GetFullPath(savedDirectory);
        }

        string[] fallbacks =
        [
            Environment.GetFolderPath(Environment.SpecialFolder.MyPictures),
            Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
            Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory),
            Path.GetPathRoot(Environment.SystemDirectory) ?? string.Empty
        ];

        return fallbacks.FirstOrDefault(
            path => !string.IsNullOrWhiteSpace(path) && Directory.Exists(path));
    }

    private static StartupTarget ResolveStartupTarget(string? startupPath)
    {
        if (string.IsNullOrWhiteSpace(startupPath))
        {
            return default;
        }

        string fullPath;
        try
        {
            fullPath = Path.GetFullPath(startupPath);
        }
        catch (Exception exception)
            when (exception is ArgumentException
                  or NotSupportedException
                  or PathTooLongException)
        {
            return new StartupTarget(
                ErrorMessage: $"Cannot open path: {startupPath}");
        }

        if (Directory.Exists(fullPath))
        {
            return new StartupTarget(DirectoryPath: fullPath);
        }

        if (!File.Exists(fullPath))
        {
            return new StartupTarget(
                ErrorMessage: $"Path does not exist: {fullPath}");
        }

        if (!PhotoIndexer.IsSupportedFile(fullPath))
        {
            return new StartupTarget(
                ErrorMessage: $"Unsupported photo format: {fullPath}");
        }

        var directory = Path.GetDirectoryName(fullPath);
        if (string.IsNullOrWhiteSpace(directory))
        {
            return new StartupTarget(
                ErrorMessage: $"Cannot determine photo directory: {fullPath}");
        }

        return new StartupTarget(
            DirectoryPath: directory,
            PhotoPath: fullPath);
    }

    private async Task ReplacePhotosAsync(
        IReadOnlyList<PhotoRecord> records,
        string rootFolder,
        string? preferredPhotoPath,
        CancellationToken cancellationToken)
    {
        var recipes = await catalog.GetEditRecipesByRootAsync(
            rootFolder,
            cancellationToken);
        var viewModels = new List<PhotoItemViewModel>(records.Count);
        foreach (var record in records)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var recipe = recipes.GetValueOrDefault(record.Path, EditRecipe.Empty);
            viewModels.Add(new PhotoItemViewModel(record, recipe, catalog));
        }

        ReplaceAllPhotos(
            viewModels,
            preferredPhotoPath ?? SelectedPhoto?.Path);
    }

    private void ReplaceAllPhotos(
        IReadOnlyList<PhotoItemViewModel> viewModels,
        string? preferredPhotoPath)
    {
        foreach (var photo in allPhotos)
        {
            photo.PropertyChanged -= OnPhotoPropertyChanged;
        }

        allPhotos.Clear();
        photoIndexByPath.Clear();
        allPhotos.AddRange(viewModels);
        for (var index = 0; index < allPhotos.Count; index++)
        {
            var photo = allPhotos[index];
            photo.PropertyChanged += OnPhotoPropertyChanged;
            photoIndexByPath[photo.Path] = index;
        }

        ApplyPhotoPresentation(preferredPhotoPath);
    }

    private void ApplyPhotoPresentation(string? preferredPhotoPath = null)
    {
        var transientSelection = SelectedPhoto is { IsTransient: true };
        var selectedPath = preferredPhotoPath ?? SelectedPhoto?.Path;
        var presented = BuildPhotoPresentation(
            allPhotos,
            sortField,
            sortDescending,
            minimumRating,
            searchText);
        // Granular updates keep the realized tiles - and the thumbnails they
        // already decoded - alive; a Reset would blank the whole viewport.
        Photos.SynchronizeTo(presented);
        if (transientSelection)
        {
            return;
        }

        SelectedPhoto = selectedPath is null
            ? Photos.FirstOrDefault()
            : Photos.FirstOrDefault(photo =>
                string.Equals(photo.Path, selectedPath, StringComparison.OrdinalIgnoreCase))
              ?? Photos.FirstOrDefault();
    }

    internal static IReadOnlyList<PhotoItemViewModel> BuildPhotoPresentation(
        IEnumerable<PhotoItemViewModel> source,
        PhotoSortField sortField,
        bool descending,
        int minimumRating,
        string? searchText)
    {
        var normalizedSearch = searchText?.Trim();
        var filtered = source.Where(
            photo => photo.Rating >= minimumRating
                     && (string.IsNullOrEmpty(normalizedSearch)
                         || photo.FileName.Contains(
                             normalizedSearch,
                             StringComparison.OrdinalIgnoreCase)));

        IOrderedEnumerable<PhotoItemViewModel> ordered = sortField switch
        {
            PhotoSortField.TakenAt when descending => filtered
                .OrderBy(photo => photo.TakenAtTicks is null)
                .ThenByDescending(photo => photo.TakenAtTicks),
            PhotoSortField.TakenAt => filtered
                .OrderBy(photo => photo.TakenAtTicks is null)
                .ThenBy(photo => photo.TakenAtTicks),
            PhotoSortField.FileName when descending => filtered
                .OrderByDescending(
                    photo => photo.FileName,
                    StringComparer.OrdinalIgnoreCase),
            PhotoSortField.FileName => filtered
                .OrderBy(
                    photo => photo.FileName,
                    StringComparer.OrdinalIgnoreCase),
            PhotoSortField.Rating when descending => filtered
                .OrderByDescending(photo => photo.Rating),
            PhotoSortField.Rating => filtered
                .OrderBy(photo => photo.Rating),
            _ => throw new ArgumentOutOfRangeException(
                nameof(sortField),
                sortField,
                null)
        };

        return ordered
            .ThenBy(photo => photo.FileName, StringComparer.OrdinalIgnoreCase)
            .ThenBy(photo => photo.Path, StringComparer.OrdinalIgnoreCase)
            .ToArray();
    }

    public event Action? SelectionRevealRequested;

    private void ChangeSort(PhotoSortField field)
    {
        if (sortField == field)
        {
            sortDescending = !sortDescending;
        }
        else
        {
            sortField = field;
            sortDescending = field != PhotoSortField.FileName;
        }

        OnPropertyChanged(nameof(TakenAtSortLabel));
        OnPropertyChanged(nameof(FileNameSortLabel));
        OnPropertyChanged(nameof(RatingSortLabel));
        ApplyPhotoPresentation();
        SelectionRevealRequested?.Invoke();
    }

    private string BuildSortLabel(PhotoSortField field, string label) =>
        sortField == field
            ? $"{label} {(sortDescending ? "↓" : "↑")}"
            : label;

    private void OnPhotoPropertyChanged(
        object? sender,
        PropertyChangedEventArgs eventArgs)
    {
        if (suppressPresentationRefresh
            || eventArgs.PropertyName != nameof(PhotoItemViewModel.Rating))
        {
            return;
        }

        // A rating can only move a photo when the gallery orders or filters
        // by it; otherwise re-presenting on every keypress is pure churn.
        if (sortField == PhotoSortField.Rating || minimumRating > 0)
        {
            ApplyPhotoPresentation();
        }
    }

    private void SelectPrevious()
    {
        if (SelectedPhoto is null)
        {
            return;
        }

        var index = Photos.IndexOf(SelectedPhoto);
        if (index > 0)
        {
            SelectedPhoto = Photos[index - 1];
        }
    }

    private void SelectNext()
    {
        if (SelectedPhoto is null)
        {
            return;
        }

        var index = Photos.IndexOf(SelectedPhoto);
        if (index >= 0 && index < Photos.Count - 1)
        {
            SelectedPhoto = Photos[index + 1];
        }
    }

    private void ShowEditor()
    {
        if (SelectedPhoto is not null)
        {
            IsEditorMode = true;
        }
    }

    private void ShowManager()
    {
        IsDirectPhotoLaunch = false;
        IsEditorMode = false;
        RestoreSelectionAfterTransientDocument();
    }

    private void ToggleEditor()
    {
        IsEditorMode = !IsEditorMode;
        if (!IsEditorMode)
        {
            IsDirectPhotoLaunch = false;
            RestoreSelectionAfterTransientDocument();
        }
    }

    public PhotoItemViewModel OpenPastedImage(BitmapSource bitmap)
    {
        if (SelectedPhoto is not { IsTransient: true })
        {
            selectionBeforeTransientDocument = SelectedPhoto;
        }

        IsFullscreenMode = false;
        var document = PhotoItemViewModel.CreateUnsaved(bitmap, catalog);
        SelectedPhoto = document;
        document.BeginEditorSession();
        IsEditorMode = true;
        return document;
    }

    private void RestoreSelectionAfterTransientDocument()
    {
        if (SelectedPhoto is not { IsTransient: true })
        {
            return;
        }

        var previous = selectionBeforeTransientDocument;
        selectionBeforeTransientDocument = null;
        SelectedPhoto = previous is not null && Photos.Contains(previous)
            ? previous
            : Photos.FirstOrDefault();
    }

    private void ToggleFullscreen()
    {
        if (SelectedPhoto is not null)
        {
            IsFullscreenMode = !IsFullscreenMode;
        }
    }

    private async Task ApplyFolderScopeChangeAsync()
    {
        try
        {
            await catalog.SetSettingAsync(
                IncludeSubfoldersSetting,
                IncludeSubfolders.ToString());
            if (CurrentFolder is not null)
            {
                await LoadFolderAsync(CurrentFolder, CancellationToken.None);
            }
        }
        catch (Exception exception)
        {
            StatusText = $"Cannot change folder scope: {exception.Message}";
        }
    }

    public async Task DeleteSelectedPhotoAsync(
        CancellationToken cancellationToken = default)
    {
        if (SelectedPhoto is not { IsTransient: false } photo)
        {
            return;
        }

        var path = photo.Path;
        var fileName = photo.FileName;
        try
        {
            await Task.Run(() => RecycleBin.MoveToRecycleBin(path), cancellationToken);
            var sidecarPath = ExifToolMetadataWriter.GetSidecarPath(path);
            if (ExifToolMetadataWriter.UsesSidecar(Path.GetExtension(path))
                && File.Exists(sidecarPath))
            {
                await Task.Run(
                    () => RecycleBin.MoveToRecycleBin(sidecarPath),
                    cancellationToken);
            }
        }
        catch (Exception exception) when (
            exception is IOException or UnauthorizedAccessException)
        {
            StatusText = $"Cannot delete {fileName}: {exception.Message}";
            return;
        }

        await catalog.DeleteByPathsAsync([path], cancellationToken);
        if (RemovePhotoByPath(path))
        {
            ApplyPhotoPresentation(SelectedPhoto?.Path);
        }

        SetPhotoStatus(Photos.Count, $"Moved to Recycle Bin: {fileName}");
    }

    private bool RemovePhotoByPath(string path)
    {
        if (!photoIndexByPath.TryGetValue(path, out var index))
        {
            return false;
        }

        var photo = allPhotos[index];
        photo.PropertyChanged -= OnPhotoPropertyChanged;
        allPhotos.RemoveAt(index);
        photoIndexByPath.Remove(photo.Path);
        for (var shifted = index; shifted < allPhotos.Count; shifted++)
        {
            photoIndexByPath[allPhotos[shifted].Path] = shifted;
        }

        if (ReferenceEquals(SelectedPhoto, photo))
        {
            var presentedIndex = Photos.IndexOf(photo);
            SelectedPhoto = presentedIndex switch
            {
                >= 0 when presentedIndex < Photos.Count - 1 =>
                    Photos[presentedIndex + 1],
                > 0 => Photos[presentedIndex - 1],
                _ => null
            };
        }

        return true;
    }

    private async Task<bool> AddOrUpdatePhotoAsync(
        PhotoRecord record,
        CancellationToken cancellationToken)
    {
        if (photoIndexByPath.TryGetValue(record.Path, out var existingIndex))
        {
            var existing = allPhotos[existingIndex];
            if (existing.IsEditorSessionActive)
            {
                // Never replace the document that is being edited right now.
                return false;
            }

            // Keeping the instance keeps the realized tile, its thumbnail and
            // the gallery selection alive across a background refresh.
            return existing.ApplyRecord(record);
        }

        var recipe = await catalog.GetEditRecipeAsync(
            record.Path,
            cancellationToken);
        var viewModel = new PhotoItemViewModel(record, recipe, catalog);
        allPhotos.Add(viewModel);
        photoIndexByPath[viewModel.Path] = allPhotos.Count - 1;
        viewModel.PropertyChanged += OnPhotoPropertyChanged;
        return true;
    }

    private void StartFolderWatcher(string rootFolder, bool watchSubfolders)
    {
        StopFolderWatcher();
        // Any flush still in flight belongs to the previous folder and must
        // not apply its records to the gallery we are about to build.
        folderGeneration++;
        try
        {
            var watcher = new FileSystemWatcher(rootFolder)
            {
                IncludeSubdirectories = watchSubfolders,
                InternalBufferSize = 64 * 1024,
                // Size on top of LastWrite only doubles the event rate for a
                // file being copied; both describe the same write.
                NotifyFilter = NotifyFilters.FileName
                               | NotifyFilters.DirectoryName
                               | NotifyFilters.LastWrite
            };
            watcher.Created += OnWatcherFileEvent;
            watcher.Changed += OnWatcherFileEvent;
            watcher.Deleted += OnWatcherFileEvent;
            watcher.Renamed += OnWatcherRenamed;
            watcher.Error += OnWatcherError;
            watcher.EnableRaisingEvents = true;
            folderWatcher = watcher;
        }
        catch (Exception exception) when (
            exception is IOException
            or ArgumentException
            or UnauthorizedAccessException)
        {
            // Live folder updates stay unavailable (e.g. an unplugged
            // network share); manual refresh still works.
            folderWatcher = null;
        }
    }

    private void StopFolderWatcher()
    {
        folderWatcher?.Dispose();
        folderWatcher = null;
        watcherTimer.Change(Timeout.InfiniteTimeSpan, Timeout.InfiniteTimeSpan);
        lock (watcherGate)
        {
            pendingWatcherPaths.Clear();
            watcherRetryCounts.Clear();
            firstPendingUtc = DateTime.MaxValue;
        }
    }

    private void OnWatcherFileEvent(object sender, FileSystemEventArgs eventArgs) =>
        QueueWatcherPath(eventArgs.FullPath, eventArgs.ChangeType);

    private void OnWatcherRenamed(object sender, RenamedEventArgs eventArgs)
    {
        QueueWatcherPath(eventArgs.OldFullPath, WatcherChangeTypes.Renamed);
        QueueWatcherPath(eventArgs.FullPath, WatcherChangeTypes.Renamed);
    }

    private void OnWatcherError(object sender, ErrorEventArgs eventArgs) =>
        uiContext?.Post(
            _ =>
            {
                // The change buffer overflowed; fall back to a full rescan.
                if (CurrentFolder is { } folder)
                {
                    _ = LoadFolderAsync(folder, CancellationToken.None);
                }
            },
            null);

    private void QueueWatcherPath(string path, WatcherChangeTypes changeType)
    {
        lock (watcherGate)
        {
            pendingWatcherPaths[path] =
                pendingWatcherPaths.TryGetValue(path, out var seen)
                    ? seen | changeType
                    : changeType;
        }

        ScheduleWatcherFlush();
    }

    /// <summary>
    /// Re-arms the debounce, but never past <see cref="WatcherMaxWait"/> after
    /// the oldest pending change: a card import fires events continuously, so
    /// a plain sliding window would hold every new photo back until the whole
    /// copy finished.
    /// </summary>
    private void ScheduleWatcherFlush()
    {
        lock (watcherGate)
        {
            if (firstPendingUtc == DateTime.MaxValue)
            {
                firstPendingUtc = DateTime.UtcNow;
            }

            var remaining = WatcherMaxWait - (DateTime.UtcNow - firstPendingUtc);
            var delay = remaining < WatcherDebounceDelay
                ? remaining
                : WatcherDebounceDelay;
            watcherTimer.Change(
                delay > TimeSpan.Zero ? delay : TimeSpan.Zero,
                Timeout.InfiniteTimeSpan);
        }
    }

    private void PostWatcherFlush() =>
        uiContext?.Post(
            async _ =>
            {
                try
                {
                    await FlushWatcherChangesAsync();
                }
                catch (Exception exception)
                {
                    StatusText =
                        $"Live folder update failed: {exception.Message}";
                }
            },
            null);

    private async Task FlushWatcherChangesAsync()
    {
        if (watcherFlushRunning)
        {
            // Paths stay queued and the running flush re-arms once it is done.
            // Re-arming here instead would spin: after the max-wait ceiling has
            // elapsed the computed delay is zero, so the timer would post back
            // to the dispatcher in a tight loop for the rest of the flush.
            return;
        }

        KeyValuePair<string, WatcherChangeTypes>[] batch;
        lock (watcherGate)
        {
            batch = [.. pendingWatcherPaths];
            pendingWatcherPaths.Clear();
            firstPendingUtc = DateTime.MaxValue;
        }

        if (batch.Length == 0 || CurrentFolder is not { } rootFolder)
        {
            return;
        }

        watcherFlushRunning = true;
        var generation = folderGeneration;
        try
        {
            var knownPaths = new HashSet<string>(
                photoIndexByPath.Keys,
                StringComparer.OrdinalIgnoreCase);
            var includeSubfolders = IncludeSubfolders;

            // Everything below the UI apply phase is file and database work;
            // running it on the dispatcher froze the gallery for as long as
            // the batch took to read metadata.
            var plan = await Task.Run(() => BuildWatcherPlanAsync(
                batch,
                rootFolder,
                includeSubfolders,
                knownPaths));

            if (generation != folderGeneration)
            {
                // The user moved to another folder while we were reading; that
                // folder runs its own scan, so this batch is simply stale.
                return;
            }

            if (plan.Removals.Count > 0)
            {
                await catalog.DeleteByPathsAsync(
                    plan.Removals,
                    CancellationToken.None);
            }

            if (plan.Upserts.Count > 0)
            {
                // One transaction for the whole batch instead of one per file.
                await catalog.UpsertBatchAsync(
                    plan.Upserts,
                    CancellationToken.None);
            }

            var presentationChanged = false;
            suppressPresentationRefresh = true;
            try
            {
                foreach (var path in plan.Removals)
                {
                    presentationChanged |= RemovePhotoByPath(path);
                }

                foreach (var record in plan.Upserts.Concat(plan.Reusable))
                {
                    // Adding an unseen photo awaits its edit recipe, so the
                    // folder can still change part-way through the batch.
                    if (generation != folderGeneration)
                    {
                        return;
                    }

                    presentationChanged |= await AddOrUpdatePhotoAsync(
                        record,
                        CancellationToken.None);
                }
            }
            finally
            {
                suppressPresentationRefresh = false;
            }

            if (presentationChanged)
            {
                ApplyPhotoPresentation(SelectedPhoto?.Path);
                SetPhotoStatus(Photos.Count);
            }

            // Files found inside a folder that appeared wholesale are fresh
            // work, not failed work, so they must not spend retry attempts.
            foreach (var discovered in plan.Discovered)
            {
                QueueWatcherPath(discovered, WatcherChangeTypes.Created);
            }

            RequeueWatcherPaths(plan.Retry);
        }
        finally
        {
            watcherFlushRunning = false;
            bool morePending;
            lock (watcherGate)
            {
                morePending = pendingWatcherPaths.Count > 0;
            }

            if (morePending)
            {
                ScheduleWatcherFlush();
            }
        }
    }

    private async Task<WatcherPlan> BuildWatcherPlanAsync(
        IReadOnlyList<KeyValuePair<string, WatcherChangeTypes>> batch,
        string rootFolder,
        bool includeSubfolders,
        HashSet<string> knownPaths)
    {
        var plan = new WatcherPlan([], [], [], [], []);
        foreach (var (path, changeType) in batch)
        {
            try
            {
                if (Directory.Exists(path))
                {
                    // A directory LastWrite only says "some child changed",
                    // and that child raises its own event. Only a folder that
                    // just appeared carries contents nobody reported.
                    if (includeSubfolders
                        && (changeType
                            & (WatcherChangeTypes.Created
                               | WatcherChangeTypes.Renamed)) != 0)
                    {
                        CollectDirectoryContents(path, plan.Discovered);
                    }

                    continue;
                }

                if (!PhotoIndexer.IsSupportedFile(path))
                {
                    // A path that is gone and is not a photo was most likely a
                    // folder that got deleted or renamed away. Windows reports
                    // the folder and none of its children, so the photos under
                    // it have to be reconciled from what the gallery knows.
                    if (!File.Exists(path))
                    {
                        var prefix = NormalizeRootPrefix(path);
                        foreach (var known in knownPaths)
                        {
                            if (known.StartsWith(
                                    prefix,
                                    StringComparison.OrdinalIgnoreCase))
                            {
                                plan.Removals.Add(known);
                            }
                        }
                    }

                    continue;
                }

                if (!IsWithinScope(path, rootFolder, includeSubfolders))
                {
                    continue;
                }

                var file = new FileInfo(path);
                if (!file.Exists)
                {
                    plan.Removals.Add(path);
                    continue;
                }

                if (SelfWriteGuard.ShouldIgnore(path))
                {
                    // Our own exiftool write; the catalogue is already right.
                    continue;
                }

                if (IsWriteInProgress(file))
                {
                    // Half-copied file: retry rather than index a torn read.
                    plan.Retry.Add(path);
                    continue;
                }

                var cached = await catalog.GetByPathAsync(path);
                if (cached is not null
                    && PhotoIndexer.CanReuseMetadata(file, cached))
                {
                    // The file matches the catalogue; only surface it when the
                    // gallery does not show it yet.
                    if (!knownPaths.Contains(path))
                    {
                        plan.Reusable.Add(cached with { RootPath = rootFolder });
                    }

                    continue;
                }

                plan.Upserts.Add(CreateRecordFromFile(file, rootFolder));
            }
            catch (Exception exception) when (
                exception is IOException or UnauthorizedAccessException)
            {
                // Locked or vanishing mid-copy; a retry reconciles it.
                plan.Retry.Add(path);
            }
        }

        return plan;
    }

    private static void CollectDirectoryContents(
        string directory,
        List<string> destination)
    {
        try
        {
            foreach (var file in Directory.EnumerateFiles(
                         directory,
                         "*",
                         SearchOption.AllDirectories))
            {
                if (PhotoIndexer.IsSupportedFile(file))
                {
                    destination.Add(file);
                }
            }
        }
        catch (Exception exception) when (
            exception is IOException or UnauthorizedAccessException)
        {
            // The directory disappeared while enumerating; ignore.
        }
    }

    private static bool IsWriteInProgress(FileInfo file)
    {
        try
        {
            using var stream = file.Open(
                FileMode.Open,
                FileAccess.Read,
                FileShare.Read);
            return false;
        }
        catch (IOException)
        {
            return true;
        }
        catch (UnauthorizedAccessException)
        {
            return false;
        }
    }

    private void RequeueWatcherPaths(IReadOnlyList<string> paths)
    {
        var requeued = false;
        var stillPending = new HashSet<string>(
            paths,
            StringComparer.OrdinalIgnoreCase);
        lock (watcherGate)
        {
            // A path that finally went through must not carry its old strikes.
            foreach (var settled in watcherRetryCounts.Keys
                         .Where(key => !stillPending.Contains(key))
                         .ToArray())
            {
                watcherRetryCounts.Remove(settled);
            }

            foreach (var path in paths)
            {
                var attempts = watcherRetryCounts.GetValueOrDefault(path);
                if (attempts >= MaxWatcherRetries)
                {
                    // A permanently locked file must not spin the flush loop.
                    continue;
                }

                watcherRetryCounts[path] = attempts + 1;
                pendingWatcherPaths[path] =
                    pendingWatcherPaths.TryGetValue(path, out var seen)
                        ? seen | WatcherChangeTypes.Changed
                        : WatcherChangeTypes.Changed;
                requeued = true;
            }
        }

        if (requeued)
        {
            ScheduleWatcherFlush();
        }
    }

    private sealed record WatcherPlan(
        List<string> Removals,
        List<PhotoRecord> Upserts,
        List<PhotoRecord> Reusable,
        List<string> Retry,
        List<string> Discovered);

    private static bool IsWithinScope(
        string path,
        string rootFolder,
        bool includeSubfolders)
    {
        if (!includeSubfolders)
        {
            return string.Equals(
                Path.GetDirectoryName(path),
                rootFolder,
                StringComparison.OrdinalIgnoreCase);
        }

        // A batch queued for a folder we have since left must not leak in, so
        // containment is checked rather than assumed.
        return path.StartsWith(
            NormalizeRootPrefix(rootFolder),
            StringComparison.OrdinalIgnoreCase);
    }

    /// <summary>
    /// Produces the root with exactly one trailing separator, which also keeps
    /// a drive root ("D:\") from turning into an unmatchable "D:\\".
    /// </summary>
    private static string NormalizeRootPrefix(string rootFolder) =>
        Path.TrimEndingDirectorySeparator(Path.GetFullPath(rootFolder))
        + Path.DirectorySeparatorChar;

    private static PhotoRecord CreateRecordFromFile(
        FileInfo file,
        string rootFolder)
    {
        var metadata = PhotoMetadataReader.ReadAll(file.FullName);
        return new PhotoRecord(
            file.FullName,
            rootFolder,
            file.Name,
            file.Extension,
            file.Length,
            file.LastWriteTimeUtc.Ticks,
            metadata.Rating,
            DateTime.UtcNow.Ticks,
            metadata.TakenAt.Ticks,
            metadata.TakenAt.Source,
            MetadataVersion: PhotoMetadataReader.CurrentVersion,
            metadata.Title,
            metadata.Description,
            metadata.Latitude,
            metadata.Longitude);
    }

    private static IReadOnlyList<PhotoRecord> FilterRecordsForScope(
        IReadOnlyList<PhotoRecord> records,
        string rootFolder,
        bool includeSubfolders)
    {
        if (includeSubfolders)
        {
            return records;
        }

        return records
            .Where(record => string.Equals(
                Path.GetDirectoryName(record.Path),
                rootFolder,
                StringComparison.OrdinalIgnoreCase))
            .ToArray();
    }

    private void SetPhotoStatus(
        int count,
        string? activity = null)
    {
        PhotoCountText = $"{count:N0} photos";
        StatusText = activity ?? string.Empty;
    }

    private void NotifyFolderScopeChanged()
    {
        OnPropertyChanged(nameof(FolderScopeToolTip));
    }

    private readonly record struct StartupTarget(
        string? DirectoryPath = null,
        string? PhotoPath = null,
        string? ErrorMessage = null);
}

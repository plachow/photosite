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
    private readonly PhotoCatalogRepository catalog;
    private readonly PhotoIndexer indexer;
    private readonly List<PhotoItemViewModel> allPhotos = [];
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

        var savedDirectory = await catalog.GetSettingAsync(
            LastDirectorySetting,
            cancellationToken);
        var savedPhoto = await catalog.GetSettingAsync(
            LastPhotoSetting,
            cancellationToken);
        var startupTarget = ResolveStartupTarget(startupPath);
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

        if (startupTarget.PhotoPath is not null)
        {
            var startupPhoto = Photos.FirstOrDefault(
                photo => string.Equals(
                    photo.Path,
                    startupTarget.PhotoPath,
                    StringComparison.OrdinalIgnoreCase));
            if (startupPhoto is not null)
            {
                SelectedPhoto = startupPhoto;
                IsDirectPhotoLaunch = true;
                ShowEditor();
            }
            else
            {
                StatusText = $"Photo could not be opened: {startupTarget.PhotoPath}";
            }
        }
        else
        {
            IsEditorMode = false;
        }

        if (startupTarget.ErrorMessage is not null)
        {
            StatusText = startupTarget.ErrorMessage;
        }
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
        allPhotos.AddRange(viewModels);
        foreach (var photo in allPhotos)
        {
            photo.PropertyChanged += OnPhotoPropertyChanged;
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
        Photos.ReplaceRange(presented);
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
    }

    private string BuildSortLabel(PhotoSortField field, string label) =>
        sortField == field
            ? $"{label} {(sortDescending ? "↓" : "↑")}"
            : label;

    private void OnPhotoPropertyChanged(
        object? sender,
        PropertyChangedEventArgs eventArgs)
    {
        if (eventArgs.PropertyName == nameof(PhotoItemViewModel.Rating))
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

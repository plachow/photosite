using System.ComponentModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Microsoft.Win32;
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
        ToggleFullscreenCommand = new RelayCommand(
            ToggleFullscreen,
            () => SelectedPhoto is not null);
        SortByTakenAtCommand = new RelayCommand(
            () => ChangeSort(PhotoSortField.TakenAt));
        SortByFileNameCommand = new RelayCommand(
            () => ChangeSort(PhotoSortField.FileName));
        SortByRatingCommand = new RelayCommand(
            () => ChangeSort(PhotoSortField.Rating));
        SetMinimumRatingCommand = new RelayCommand<RatingFilterOption>(
            SetMinimumRating);
        RatingFilters =
        [
            new RatingFilterOption(0, "★̸", "Zrušit filtr hodnocení"),
            new RatingFilterOption(1, "★", "Alespoň 1 hvězdička"),
            new RatingFilterOption(2, "★", "Alespoň 2 hvězdičky"),
            new RatingFilterOption(3, "★", "Alespoň 3 hvězdičky"),
            new RatingFilterOption(4, "★", "Alespoň 4 hvězdičky"),
            new RatingFilterOption(5, "★", "5 hvězdiček")
        ];
        UpdateRatingFilterState();
    }

    public BulkObservableCollection<PhotoItemViewModel> Photos { get; } = new();

    public DirectoryTreeViewModel DirectoryTree { get; }

    public IReadOnlyList<int> RatingValues { get; } = [0, 1, 2, 3, 4, 5];

    public IReadOnlyList<RatingFilterOption> RatingFilters { get; }

    public IAsyncRelayCommand OpenFolderCommand { get; }

    public IRelayCommand PreviousCommand { get; }

    public IRelayCommand NextCommand { get; }

    public IRelayCommand ShowEditorCommand { get; }

    public IRelayCommand ShowManagerCommand { get; }

    public IRelayCommand ToggleFullscreenCommand { get; }

    public IRelayCommand SortByTakenAtCommand { get; }

    public IRelayCommand SortByFileNameCommand { get; }

    public IRelayCommand SortByRatingCommand { get; }

    public IRelayCommand<RatingFilterOption> SetMinimumRatingCommand { get; }

    public string TakenAtSortLabel => BuildSortLabel(
        PhotoSortField.TakenAt,
        "Pořízeno");

    public string FileNameSortLabel => BuildSortLabel(
        PhotoSortField.FileName,
        "Název");

    public string RatingSortLabel => BuildSortLabel(
        PhotoSortField.Rating,
        "Hodnocení");

    public string FolderScopeToolTip => IncludeSubfolders
        ? "Včetně podsložek"
        : "Pouze aktuální složka";

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

    public async Task InitializeAsync(CancellationToken cancellationToken = default)
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
        var initialDirectory = ResolveInitialDirectory(savedDirectory);
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
            savedPhoto);
    }

    public async Task LoadFolderAsync(string folder, CancellationToken cancellationToken)
    {
        await LoadFolderAsync(
            folder,
            cancellationToken,
            preferredPhotoPath: null);
    }

    public async Task SaveSessionAsync(
        CancellationToken cancellationToken = default)
    {
        if (SelectedPhoto is not { } photo)
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
        var selectedPath = preferredPhotoPath ?? SelectedPhoto?.Path;
        var presented = BuildPhotoPresentation(
            allPhotos,
            sortField,
            sortDescending,
            minimumRating,
            searchText);
        Photos.ReplaceRange(presented);
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

    private void SetMinimumRating(RatingFilterOption? option)
    {
        if (option is null || minimumRating == option.MinimumRating)
        {
            return;
        }

        minimumRating = option.MinimumRating;
        UpdateRatingFilterState();
        ApplyPhotoPresentation();
    }

    private void UpdateRatingFilterState()
    {
        foreach (var option in RatingFilters)
        {
            option.IsActive = option.MinimumRating == 0
                ? minimumRating == 0
                : minimumRating > 0
                  && option.MinimumRating <= minimumRating;
        }
    }

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
        IsEditorMode = false;
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
}

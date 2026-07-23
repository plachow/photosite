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
    private CancellationTokenSource? scanCancellation;
    private PhotoItemViewModel? selectedPhoto;
    private string? currentFolder;
    private string statusText = "Choose a folder to begin";
    private bool isBusy;
    private bool isEditorMode;
    private bool isFullscreenMode;
    private bool includeSubfolders = true;

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
    }

    public BulkObservableCollection<PhotoItemViewModel> Photos { get; } = new();

    public DirectoryTreeViewModel DirectoryTree { get; }

    public IReadOnlyList<int> RatingValues { get; } = [0, 1, 2, 3, 4, 5];

    public IAsyncRelayCommand OpenFolderCommand { get; }

    public IRelayCommand PreviousCommand { get; }

    public IRelayCommand NextCommand { get; }

    public IRelayCommand ShowEditorCommand { get; }

    public IRelayCommand ShowManagerCommand { get; }

    public IRelayCommand ToggleFullscreenCommand { get; }

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
            if (cached.Count > 0)
            {
                await ReplacePhotosAsync(
                    cached,
                    rootFolder,
                    preferredPhotoPath,
                    token);
                StatusText = BuildStatus(cached.Count, includeSubfolders, "cached · refreshing…");
            }
            else
            {
                Photos.ReplaceRange([]);
                SelectedPhoto = null;
                StatusText = "Indexing…";
            }

            var scanId = DateTime.UtcNow.Ticks;
            var batch = new List<PhotoRecord>(DatabaseBatchSize);
            var discovered = 0;

            await foreach (var record in indexer.ScanAsync(
                               rootFolder,
                               scanId,
                               includeSubfolders,
                               token))
            {
                batch.Add(record);
                discovered++;
                if (batch.Count < DatabaseBatchSize)
                {
                    continue;
                }

                await catalog.UpsertBatchAsync(batch, token);
                batch.Clear();
                StatusText = BuildStatus(discovered, includeSubfolders, "indexing");
            }

            await catalog.UpsertBatchAsync(batch, token);
            await catalog.CompleteScanAsync(
                rootFolder,
                scanId,
                includeSubfolders,
                token);
            var current = FilterRecordsForScope(
                await catalog.GetByRootAsync(rootFolder, token),
                rootFolder,
                includeSubfolders);
            await ReplacePhotosAsync(
                current,
                rootFolder,
                preferredPhotoPath,
                token);
            StatusText = BuildStatus(current.Count, includeSubfolders);
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

        var selectedPath = preferredPhotoPath ?? SelectedPhoto?.Path;
        Photos.ReplaceRange(viewModels);
        SelectedPhoto = selectedPath is null
            ? Photos.FirstOrDefault()
            : Photos.FirstOrDefault(photo =>
                string.Equals(photo.Path, selectedPath, StringComparison.OrdinalIgnoreCase))
              ?? Photos.FirstOrDefault();
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

    private static string BuildStatus(
        int count,
        bool includeSubfolders,
        string? activity = null)
    {
        var scope = includeSubfolders ? "including subfolders" : "current folder only";
        return activity is null
            ? $"{count:N0} photos · {scope}"
            : $"{count:N0} photos · {scope} · {activity}";
    }
}

using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using System.Windows.Media.Imaging;
using PhotoSite.Domain;
using PhotoSite.Infrastructure;

namespace PhotoSite.ViewModels;

public sealed class PhotoItemViewModel : ObservableObject
{
    private readonly PhotoCatalogRepository catalog;
    private readonly SemaphoreSlim persistenceGate = new(1, 1);
    private readonly Stack<EditRecipe> undoStack = [];
    private readonly Stack<EditRecipe> redoStack = [];
    private int rating;
    private string? title;
    private string? description;
    private double? latitude;
    private double? longitude;
    private EditRecipe editRecipe;
    private EditRecipe editorBaseline = EditRecipe.Empty;
    private bool isEditorSessionActive;
    private bool isUnsaved;
    private string? savedPath;
    private BitmapSource? sourceBitmap;

    public PhotoItemViewModel(
        PhotoRecord record,
        EditRecipe editRecipe,
        PhotoCatalogRepository catalog,
        BitmapSource? sourceBitmap = null)
    {
        Record = record;
        rating = record.Rating;
        title = record.Title;
        description = record.Description;
        latitude = record.Latitude;
        longitude = record.Longitude;
        this.editRecipe = editRecipe;
        this.catalog = catalog;
        this.sourceBitmap = sourceBitmap;
        IsTransient = sourceBitmap is not null;
        isUnsaved = IsTransient;

        RotateLeftCommand = new RelayCommand(
            () => ApplyEdit(EditRecipe.RotateCounterClockwise()));
        RotateRightCommand = new RelayCommand(
            () => ApplyEdit(EditRecipe.RotateClockwise()));
        FlipHorizontalCommand = new RelayCommand(
            () => ApplyEdit(EditRecipe with
            {
                FlipHorizontal = !EditRecipe.FlipHorizontal
            }));
        ResetEditsCommand = new RelayCommand(
            () => ApplyEdit(EditRecipe.Empty),
            () => EditRecipe != EditRecipe.Empty);
        UndoEditCommand = new RelayCommand(UndoEdit, () => undoStack.Count > 0);
        RedoEditCommand = new RelayCommand(RedoEdit, () => redoStack.Count > 0);
    }

    public PhotoRecord Record { get; private set; }

    /// <summary>
    /// Refreshes this view model from a re-read record without disturbing the
    /// editor state, so the gallery keeps the realized tile, its decoded
    /// thumbnail and the current selection. Returns true when a value that
    /// participates in sorting or filtering actually moved.
    /// </summary>
    internal bool ApplyRecord(PhotoRecord updated)
    {
        var affectsPresentation = Record.TakenAtTicks != updated.TakenAtTicks
                                  || Record.Rating != updated.Rating
                                  || !string.Equals(
                                      Record.FileName,
                                      updated.FileName,
                                      StringComparison.Ordinal);
        Record = updated;

        // Assign the backing fields directly: routing through the public
        // setters would queue another metadata outbox write and bounce the
        // value we just read back out to exiftool.
        rating = updated.Rating;
        title = updated.Title;
        description = updated.Description;
        latitude = updated.Latitude;
        longitude = updated.Longitude;

        OnPropertyChanged(nameof(FileName));
        OnPropertyChanged(nameof(TakenAtTicks));
        OnPropertyChanged(nameof(Rating));
        OnPropertyChanged(nameof(RatingText));
        OnPropertyChanged(nameof(Title));
        OnPropertyChanged(nameof(Description));
        OnPropertyChanged(nameof(LocationText));
        return affectsPresentation;
    }

    public string Path => savedPath ?? Record.Path;

    public string FileName => savedPath is not null
        ? System.IO.Path.GetFileName(savedPath)
        : Record.FileName;

    public long? TakenAtTicks => Record.TakenAtTicks;

    public string? RatingText => Rating == 0 ? null : $"★ {Rating}";

    public bool IsTransient { get; }

    public bool IsUnsaved
    {
        get => isUnsaved;
        private set
        {
            if (SetProperty(ref isUnsaved, value))
            {
                OnPropertyChanged(nameof(IsEditorDirty));
            }
        }
    }

    public BitmapSource? SourceBitmap
    {
        get => sourceBitmap;
        private set => SetProperty(ref sourceBitmap, value);
    }

    public bool IsEditorSessionActive => isEditorSessionActive;

    public int Rating
    {
        get => rating;
        set
        {
            var valid = Math.Clamp(value, 0, 5);
            if (!SetProperty(ref rating, valid))
            {
                return;
            }

            OnPropertyChanged(nameof(RatingText));
            _ = PersistRatingAsync(valid);
        }
    }

    public string? Title
    {
        get => title;
        set
        {
            var normalized = string.IsNullOrWhiteSpace(value)
                ? null
                : value.Trim();
            if (!SetProperty(ref title, normalized))
            {
                return;
            }

            _ = PersistMetadataAsync(
                () => catalog.UpdateTitleAsync(Path, normalized));
        }
    }

    public string? Description
    {
        get => description;
        set
        {
            var normalized = string.IsNullOrWhiteSpace(value)
                ? null
                : value.Trim();
            if (!SetProperty(ref description, normalized))
            {
                return;
            }

            _ = PersistMetadataAsync(
                () => catalog.UpdateDescriptionAsync(Path, normalized));
        }
    }

    public string LocationText
    {
        get => latitude is { } lat && longitude is { } lon
            ? string.Create(
                System.Globalization.CultureInfo.InvariantCulture,
                $"{lat:0.######}, {lon:0.######}")
            : string.Empty;
        set
        {
            if (!TryParseLocation(value, out var parsed))
            {
                OnPropertyChanged();
                return;
            }

            if (latitude == parsed?.Latitude && longitude == parsed?.Longitude)
            {
                return;
            }

            latitude = parsed?.Latitude;
            longitude = parsed?.Longitude;
            OnPropertyChanged();
            _ = PersistMetadataAsync(
                () => catalog.UpdateLocationAsync(
                    Path,
                    latitude,
                    longitude));
        }
    }

    internal static bool TryParseLocation(
        string? text,
        out (double Latitude, double Longitude)? location)
    {
        location = null;
        if (string.IsNullOrWhiteSpace(text))
        {
            return true;
        }

        var parts = text.Split(
            [',', ';', ' '],
            StringSplitOptions.RemoveEmptyEntries
            | StringSplitOptions.TrimEntries);
        if (parts.Length != 2
            || !double.TryParse(
                parts[0],
                System.Globalization.NumberStyles.Float,
                System.Globalization.CultureInfo.InvariantCulture,
                out var latitude)
            || !double.TryParse(
                parts[1],
                System.Globalization.NumberStyles.Float,
                System.Globalization.CultureInfo.InvariantCulture,
                out var longitude)
            || latitude is < -90 or > 90
            || longitude is < -180 or > 180)
        {
            return false;
        }

        location = (latitude, longitude);
        return true;
    }

    public EditRecipe EditRecipe
    {
        get => editRecipe;
        private set => SetEditRecipe(value);
    }

    public IRelayCommand RotateLeftCommand { get; }

    public IRelayCommand RotateRightCommand { get; }

    public IRelayCommand FlipHorizontalCommand { get; }

    public IRelayCommand ResetEditsCommand { get; }

    public IRelayCommand UndoEditCommand { get; }

    public IRelayCommand RedoEditCommand { get; }

    public bool IsEditorDirty =>
        isEditorSessionActive
        && (IsUnsaved || EditRecipe != editorBaseline);

    public static PhotoItemViewModel CreateUnsaved(
        BitmapSource bitmap,
        PhotoCatalogRepository catalog)
    {
        ArgumentNullException.ThrowIfNull(bitmap);
        if (!bitmap.IsFrozen && bitmap.CanFreeze)
        {
            bitmap.Freeze();
        }

        return new PhotoItemViewModel(
            new PhotoRecord(
                string.Empty,
                string.Empty,
                "Pasted image",
                string.Empty,
                0,
                DateTime.UtcNow.Ticks,
                0,
                0),
            EditRecipe.Empty,
            catalog,
            bitmap);
    }

    public void BeginEditorSession()
    {
        if (isEditorSessionActive)
        {
            return;
        }

        editorBaseline = EditRecipe;
        isEditorSessionActive = true;
        undoStack.Clear();
        redoStack.Clear();
        NotifyEditStateChanged();
    }

    public void DiscardEditorSession()
    {
        if (!isEditorSessionActive)
        {
            return;
        }

        SetEditRecipe(editorBaseline, persist: false);
        EndEditorSession();
    }

    public async Task CompleteOverwriteAsync()
    {
        SetEditRecipe(EditRecipe.Empty, persist: false);
        EndEditorSession();
        if (!IsTransient)
        {
            await PersistEditRecipeAsync(EditRecipe.Empty);
        }
    }

    public void CompleteCopySession()
    {
        if (!isEditorSessionActive)
        {
            return;
        }

        SetEditRecipe(editorBaseline, persist: false);
        EndEditorSession();
    }

    public void CompleteTransientSave(
        string destinationPath,
        BitmapSource rendered,
        bool continueEditing)
    {
        if (!IsTransient)
        {
            throw new InvalidOperationException(
                "Only an in-memory document can adopt a rendered save.");
        }

        savedPath = System.IO.Path.GetFullPath(destinationPath);
        SourceBitmap = rendered;
        IsUnsaved = false;
        SetEditRecipe(EditRecipe.Empty, persist: false);
        editorBaseline = EditRecipe.Empty;
        undoStack.Clear();
        redoStack.Clear();
        OnPropertyChanged(nameof(Path));
        OnPropertyChanged(nameof(FileName));
        if (continueEditing)
        {
            isEditorSessionActive = true;
            NotifyEditStateChanged();
        }
        else
        {
            EndEditorSession();
        }
    }

    public void ApplyCrop(CropRegion region)
    {
        var constrained = region.ConstrainToUnit();
        if (constrained.IsEmpty)
        {
            return;
        }

        ApplyEdit(EditRecipe with
        {
            Crop = constrained.IsFull ? null : constrained
        });
    }

    private void ApplyEdit(EditRecipe recipe)
    {
        if (recipe == EditRecipe)
        {
            return;
        }

        if (isEditorSessionActive)
        {
            undoStack.Push(EditRecipe);
            redoStack.Clear();
        }

        SetEditRecipe(recipe, persist: !isEditorSessionActive);
    }

    private void UndoEdit()
    {
        if (undoStack.TryPop(out var previous))
        {
            redoStack.Push(EditRecipe);
            SetEditRecipe(previous, persist: false);
        }
    }

    private void RedoEdit()
    {
        if (redoStack.TryPop(out var next))
        {
            undoStack.Push(EditRecipe);
            SetEditRecipe(next, persist: false);
        }
    }

    private void SetEditRecipe(
        EditRecipe value,
        bool persist = true)
    {
        if (!SetProperty(
                ref editRecipe,
                value,
                nameof(EditRecipe)))
        {
            return;
        }

        ResetEditsCommand.NotifyCanExecuteChanged();
        NotifyEditStateChanged();
        if (persist)
        {
            _ = PersistEditRecipeAsync(value);
        }
    }

    private void EndEditorSession()
    {
        isEditorSessionActive = false;
        undoStack.Clear();
        redoStack.Clear();
        NotifyEditStateChanged();
    }

    private void NotifyEditStateChanged()
    {
        OnPropertyChanged(nameof(IsEditorDirty));
        UndoEditCommand.NotifyCanExecuteChanged();
        RedoEditCommand.NotifyCanExecuteChanged();
    }

    private Task PersistRatingAsync(int value) =>
        PersistMetadataAsync(() => catalog.UpdateRatingAsync(Path, value));

    private async Task PersistMetadataAsync(Func<Task> update)
    {
        if (IsTransient)
        {
            return;
        }

        await persistenceGate.WaitAsync();
        try
        {
            await update();
        }
        catch
        {
            // The catalogue remains usable; a later status surface will expose
            // durable metadata write failures and retries to the user.
        }
        finally
        {
            persistenceGate.Release();
        }
    }

    private async Task PersistEditRecipeAsync(EditRecipe recipe)
    {
        if (IsTransient)
        {
            return;
        }

        await persistenceGate.WaitAsync();
        try
        {
            await catalog.SaveEditRecipeAsync(Path, recipe);
        }
        catch
        {
            // Keep the viewer responsive. A later persistence status surface
            // will expose failures and retry them from a durable queue.
        }
        finally
        {
            persistenceGate.Release();
        }
    }
}

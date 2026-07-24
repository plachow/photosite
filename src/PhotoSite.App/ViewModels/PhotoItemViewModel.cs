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

    public PhotoRecord Record { get; }

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

    private async Task PersistRatingAsync(int value)
    {
        if (IsTransient)
        {
            return;
        }

        await persistenceGate.WaitAsync();
        try
        {
            await catalog.UpdateRatingAsync(Path, value);
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

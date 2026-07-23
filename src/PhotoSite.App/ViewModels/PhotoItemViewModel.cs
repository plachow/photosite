using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using PhotoSite.Domain;
using PhotoSite.Infrastructure;

namespace PhotoSite.ViewModels;

public sealed class PhotoItemViewModel : ObservableObject
{
    private readonly PhotoCatalogRepository catalog;
    private readonly SemaphoreSlim persistenceGate = new(1, 1);
    private int rating;
    private EditRecipe editRecipe;

    public PhotoItemViewModel(
        PhotoRecord record,
        EditRecipe editRecipe,
        PhotoCatalogRepository catalog)
    {
        Record = record;
        rating = record.Rating;
        this.editRecipe = editRecipe;
        this.catalog = catalog;

        RotateLeftCommand = new RelayCommand(
            () => EditRecipe = EditRecipe.RotateCounterClockwise());
        RotateRightCommand = new RelayCommand(
            () => EditRecipe = EditRecipe.RotateClockwise());
        FlipHorizontalCommand = new RelayCommand(
            () => EditRecipe = EditRecipe with
            {
                FlipHorizontal = !EditRecipe.FlipHorizontal
            });
        ResetEditsCommand = new RelayCommand(
            () => EditRecipe = EditRecipe.Empty,
            () => EditRecipe != EditRecipe.Empty);
    }

    public PhotoRecord Record { get; }

    public string Path => Record.Path;

    public string FileName => Record.FileName;

    public long? TakenAtTicks => Record.TakenAtTicks;

    public string? RatingText => Rating == 0 ? null : $"★ {Rating}";

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
        private set
        {
            if (!SetProperty(ref editRecipe, value))
            {
                return;
            }

            ResetEditsCommand.NotifyCanExecuteChanged();
            _ = PersistEditRecipeAsync(value);
        }
    }

    public IRelayCommand RotateLeftCommand { get; }

    public IRelayCommand RotateRightCommand { get; }

    public IRelayCommand FlipHorizontalCommand { get; }

    public IRelayCommand ResetEditsCommand { get; }

    private async Task PersistRatingAsync(int value)
    {
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

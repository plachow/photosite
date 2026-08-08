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
    private ColorLabel colorLabel;
    private PhotoFlag flag;
    private string? keywords;
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
        colorLabel = record.ColorLabel;
        flag = record.Flag;
        keywords = record.Keywords;
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
        colorLabel = updated.ColorLabel;
        flag = updated.Flag;
        keywords = updated.Keywords;

        OnPropertyChanged(nameof(FileName));
        OnPropertyChanged(nameof(TakenAtTicks));
        OnPropertyChanged(nameof(Rating));
        OnPropertyChanged(nameof(RatingText));
        OnPropertyChanged(nameof(Title));
        OnPropertyChanged(nameof(Description));
        OnPropertyChanged(nameof(LocationText));
        OnPropertyChanged(nameof(ColorLabel));
        OnPropertyChanged(nameof(ColorLabelBrush));
        OnPropertyChanged(nameof(HasColorLabel));
        OnPropertyChanged(nameof(Flag));
        OnPropertyChanged(nameof(IsRejected));
        OnPropertyChanged(nameof(FlagGlyph));
        OnPropertyChanged(nameof(Keywords));
        NotifyDetailsChanged();
        return affectsPresentation;
    }

    private void NotifyDetailsChanged()
    {
        OnPropertyChanged(nameof(CameraText));
        OnPropertyChanged(nameof(LensText));
        OnPropertyChanged(nameof(ExposureText));
        OnPropertyChanged(nameof(DimensionsText));
        OnPropertyChanged(nameof(FileSizeText));
        OnPropertyChanged(nameof(TakenAtText));
        OnPropertyChanged(nameof(HasLocation));
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

    public ColorLabel ColorLabel
    {
        get => colorLabel;
        set
        {
            if (!SetProperty(ref colorLabel, value))
            {
                return;
            }

            OnPropertyChanged(nameof(ColorLabelBrush));
            OnPropertyChanged(nameof(HasColorLabel));
            _ = PersistMetadataAsync(
                () => catalog.UpdateColorLabelAsync(Path, value));
        }
    }

    public System.Windows.Media.Brush ColorLabelBrush =>
        PhotoLabelBrushes.Get(colorLabel);

    public bool HasColorLabel => colorLabel != ColorLabel.None;

    public PhotoFlag Flag
    {
        get => flag;
        set
        {
            if (!SetProperty(ref flag, value))
            {
                return;
            }

            OnPropertyChanged(nameof(IsRejected));
            OnPropertyChanged(nameof(FlagGlyph));
            _ = PersistMetadataAsync(
                () => catalog.UpdateFlagAsync(Path, value));
        }
    }

    public bool IsRejected => flag == PhotoFlag.Rejected;

    public string? FlagGlyph => flag switch
    {
        PhotoFlag.Picked => "⚑",
        PhotoFlag.Rejected => "⛌",
        _ => null
    };

    public string? Keywords
    {
        get => keywords;
        set
        {
            var normalized = string.IsNullOrWhiteSpace(value)
                ? null
                : PhotoRecord.JoinKeywords(
                    value.Split(
                        [';', ','],
                        StringSplitOptions.RemoveEmptyEntries
                        | StringSplitOptions.TrimEntries));
            if (!SetProperty(ref keywords, normalized))
            {
                return;
            }

            _ = PersistMetadataAsync(
                () => catalog.UpdateKeywordsAsync(Path, normalized));
        }
    }

    public string? CameraText => Record.Camera;

    public string? LensText => Record.Lens;

    /// <summary>
    /// The exposure triangle on one line, the way it reads on a camera back.
    /// </summary>
    public string? ExposureText
    {
        get
        {
            var parts = new List<string>(4);
            if (Record.FocalLength is { } focal and > 0)
            {
                parts.Add($"{focal:0.#} mm");
            }

            if (Record.Aperture is { } aperture and > 0)
            {
                parts.Add($"f/{aperture:0.#}");
            }

            if (Record.ExposureSeconds is { } exposure and > 0)
            {
                parts.Add(
                    exposure >= 1
                        ? $"{exposure:0.#} s"
                        : $"1/{Math.Round(1 / exposure):0} s");
            }

            if (Record.Iso is { } iso and > 0)
            {
                parts.Add($"ISO {iso}");
            }

            return parts.Count == 0 ? null : string.Join(" · ", parts);
        }
    }

    public string? DimensionsText =>
        Record.PixelWidth is { } width && Record.PixelHeight is { } height
            ? $"{width:N0} × {height:N0}"
            : null;

    public string FileSizeText => FormatFileSize(Record.Length);

    public string? TakenAtText =>
        Record.TakenAtTicks is { } ticks
            ? new DateTime(ticks).ToString("d MMMM yyyy, HH:mm:ss")
            : null;

    public bool HasLocation => latitude is not null && longitude is not null;

    /// <summary>An OpenStreetMap pin for the "Open in map" action.</summary>
    public string? MapUrl =>
        latitude is { } lat && longitude is { } lon
            ? string.Create(
                System.Globalization.CultureInfo.InvariantCulture,
                $"https://www.openstreetmap.org/?mlat={lat:0.######}&mlon={lon:0.######}#map=15/{lat:0.######}/{lon:0.######}")
            : null;

    internal static string FormatFileSize(long bytes) => bytes switch
    {
        < 1024 => $"{bytes} B",
        < 1024 * 1024 => $"{bytes / 1024d:0.#} kB",
        < 1024L * 1024 * 1024 => $"{bytes / (1024d * 1024):0.#} MB",
        _ => $"{bytes / (1024d * 1024 * 1024):0.##} GB"
    };

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

    /// <summary>
    /// The editor's slider panel for this photo, created on first use so the
    /// gallery does not build one per thumbnail.
    /// </summary>
    public AdjustmentsViewModel Adjustments =>
        adjustments ??= new AdjustmentsViewModel(this);

    private AdjustmentsViewModel? adjustments;

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

    internal void ApplyEdit(EditRecipe recipe, string? coalesceKey = null)
    {
        if (recipe == EditRecipe)
        {
            return;
        }

        if (isEditorSessionActive)
        {
            // Dragging one slider is a single edit, not one per pixel of
            // travel, so consecutive changes to the same control fold into
            // the undo entry that opened them.
            var coalesces = coalesceKey is not null
                && coalesceKey == lastCoalesceKey
                && DateTime.UtcNow - lastCoalesceAt < CoalesceWindow;
            if (!coalesces)
            {
                undoStack.Push(EditRecipe);
                redoStack.Clear();
            }

            lastCoalesceKey = coalesceKey;
            lastCoalesceAt = DateTime.UtcNow;
        }

        SetEditRecipe(recipe, persist: !isEditorSessionActive);
    }

    private static readonly TimeSpan CoalesceWindow = TimeSpan.FromSeconds(1.5);
    private string? lastCoalesceKey;
    private DateTime lastCoalesceAt;

    /// <summary>
    /// Replaces the photographic adjustments, keeping every other part of the
    /// recipe. <paramref name="coalesceKey"/> names the control being dragged.
    /// </summary>
    public void SetAdjustments(
        PhotoAdjustments adjustments,
        string? coalesceKey = null) =>
        ApplyEdit(EditRecipe with { Adjustments = adjustments }, coalesceKey);

    public void SetFilters(IReadOnlyList<FilterStep> filters) =>
        ApplyEdit(EditRecipe with { Filters = filters });

    public void SetLayers(IReadOnlyList<AnnotationLayer> layers) =>
        ApplyEdit(EditRecipe with { Layers = layers });

    public void SetGeometry(
        double straightenAngle,
        double perspectiveVertical,
        double perspectiveHorizontal,
        string? coalesceKey = null) =>
        ApplyEdit(
            EditRecipe with
            {
                StraightenAngle = straightenAngle,
                PerspectiveVertical = perspectiveVertical,
                PerspectiveHorizontal = perspectiveHorizontal
            },
            coalesceKey);

    public void FlipVertical() =>
        ApplyEdit(EditRecipe with { FlipVertical = !EditRecipe.FlipVertical });

    /// <summary>Clears the crop without disturbing anything else.</summary>
    public void ResetCrop() => ApplyEdit(EditRecipe with { Crop = null });

    private void UndoEdit()
    {
        if (undoStack.TryPop(out var previous))
        {
            redoStack.Push(EditRecipe);
            // The next slider move must open a new undo entry rather than
            // folding into the one that was just undone.
            lastCoalesceKey = null;
            SetEditRecipe(previous, persist: false);
            adjustments?.NotifyAll();
        }
    }

    private void RedoEdit()
    {
        if (redoStack.TryPop(out var next))
        {
            undoStack.Push(EditRecipe);
            lastCoalesceKey = null;
            SetEditRecipe(next, persist: false);
            adjustments?.NotifyAll();
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

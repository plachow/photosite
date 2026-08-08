using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using PhotoSite.Domain;
using PhotoSite.Services.Imaging;

namespace PhotoSite.Controls;

public enum PreviewComparisonMode
{
    /// <summary>The photograph as edited - the normal editor view.</summary>
    Edited,

    /// <summary>The decoded original, for a straight before/after toggle.</summary>
    Original,

    /// <summary>Original on the left, edited on the right.</summary>
    Split
}

public sealed partial class PhotoViewer : FrameworkElement
{
    private const double TransitionDurationMilliseconds = 60;
    private const double ZoomStep = 1.18;
    private const double MinimumZoom = 0.1;
    private const double MaximumActualScale = 16;
    private const double MinimumGrabArea = 48;
    private const double MaximumGrabArea = 96;
    private const double GrabAreaFraction = 0.2;
    private const double SelectionHitTolerance = 8;
    private const double MinimumSelectionSize = 4;

    private static readonly Brush SelectionShadeBrush =
        CreateFrozenBrush(Color.FromArgb(142, 0, 0, 0));
    private static readonly Brush SelectionHandleBrush =
        CreateFrozenBrush(Color.FromRgb(242, 244, 248));
    private static readonly Pen SelectionBorderPen =
        CreateFrozenPen(Color.FromRgb(103, 183, 255), 1.5);

    public static readonly DependencyProperty SourcePathProperty =
        DependencyProperty.Register(
            nameof(SourcePath),
            typeof(string),
            typeof(PhotoViewer),
            new FrameworkPropertyMetadata(null, OnViewerPropertyChanged));

    public static readonly DependencyProperty SourceBitmapProperty =
        DependencyProperty.Register(
            nameof(SourceBitmap),
            typeof(BitmapSource),
            typeof(PhotoViewer),
            new FrameworkPropertyMetadata(null, OnViewerPropertyChanged));

    public static readonly DependencyProperty EditRecipeProperty =
        DependencyProperty.Register(
            nameof(EditRecipe),
            typeof(EditRecipe),
            typeof(PhotoViewer),
            new FrameworkPropertyMetadata(
                EditRecipe.Empty,
                FrameworkPropertyMetadataOptions.AffectsRender,
                OnEditRecipeChanged));

    public static readonly DependencyProperty DoubleClickCommandProperty =
        DependencyProperty.Register(
            nameof(DoubleClickCommand),
            typeof(ICommand),
            typeof(PhotoViewer));

    public static readonly DependencyProperty IsEditorModeProperty =
        DependencyProperty.Register(
            nameof(IsEditorMode),
            typeof(bool),
            typeof(PhotoViewer));

    public static readonly DependencyProperty IsFullscreenModeProperty =
        DependencyProperty.Register(
            nameof(IsFullscreenMode),
            typeof(bool),
            typeof(PhotoViewer));

    public static readonly DependencyProperty MiddleClickCommandProperty =
        DependencyProperty.Register(
            nameof(MiddleClickCommand),
            typeof(ICommand),
            typeof(PhotoViewer));

    public static readonly DependencyProperty PreviousPhotoCommandProperty =
        DependencyProperty.Register(
            nameof(PreviousPhotoCommand),
            typeof(ICommand),
            typeof(PhotoViewer));

    public static readonly DependencyProperty NextPhotoCommandProperty =
        DependencyProperty.Register(
            nameof(NextPhotoCommand),
            typeof(ICommand),
            typeof(PhotoViewer));

    public static readonly DependencyProperty IsSelectionModeProperty =
        DependencyProperty.Register(
            nameof(IsSelectionMode),
            typeof(bool),
            typeof(PhotoViewer),
            new FrameworkPropertyMetadata(
                false,
                FrameworkPropertyMetadataOptions.AffectsRender,
                OnSelectionModeChanged));

    public static readonly DependencyProperty IsColorPickerModeProperty =
        DependencyProperty.Register(
            nameof(IsColorPickerMode),
            typeof(bool),
            typeof(PhotoViewer),
            new FrameworkPropertyMetadata(false, OnColorPickerModeChanged));

    public static readonly DependencyProperty ComparisonModeProperty =
        DependencyProperty.Register(
            nameof(ComparisonMode),
            typeof(PreviewComparisonMode),
            typeof(PhotoViewer),
            new FrameworkPropertyMetadata(
                PreviewComparisonMode.Edited,
                FrameworkPropertyMetadataOptions.AffectsRender));

    public static readonly DependencyProperty SelectionAspectRatioProperty =
        DependencyProperty.Register(
            nameof(SelectionAspectRatio),
            typeof(double),
            typeof(PhotoViewer),
            new FrameworkPropertyMetadata(0d, OnSelectionAspectRatioChanged));

    private static readonly DependencyPropertyKey HasSelectionPropertyKey =
        DependencyProperty.RegisterReadOnly(
            nameof(HasSelection),
            typeof(bool),
            typeof(PhotoViewer),
            new FrameworkPropertyMetadata(false));

    public static readonly DependencyProperty HasSelectionProperty =
        HasSelectionPropertyKey.DependencyProperty;

    private BitmapSource? bitmap;
    private BitmapSource? rawBitmap;
    private BitmapSource? previousBitmap;
    private CancellationTokenSource? loadCancellation;
    private CancellationTokenSource? adjustmentCancellation;
    private EditRecipe? renderedPixelRecipe;
    private readonly System.Windows.Threading.DispatcherTimer adjustmentTimer;
    private EditRecipe bitmapRecipe = EditRecipe.Empty;
    private EditRecipe previousBitmapRecipe = EditRecipe.Empty;
    private string? bitmapPath;
    private double zoom = 1;
    private double transitionProgress = 1;
    private Vector pan;
    private Point dragOrigin;
    private Vector panOrigin;
    private bool isDragging;
    private CropRegion? selection;
    private CropRegion selectionAtDragStart;
    private Point selectionDragOrigin;
    private SelectionOperation selectionOperation;
    private bool isFullResolutionBitmap;
    private bool isTransitioning;
    private long transitionStartedAt;
    private string? error;

    [Flags]
    private enum SelectionOperation
    {
        None = 0,
        Create = 1,
        Move = 2,
        Left = 4,
        Top = 8,
        Right = 16,
        Bottom = 32
    }

    public PhotoViewer()
    {
        Focusable = true;
        ClipToBounds = true;
        // Dragging a slider changes the recipe far faster than a full-frame
        // render can keep up with, so renders are coalesced rather than
        // queued: the canvas stays responsive and only the last value counts.
        adjustmentTimer = new System.Windows.Threading.DispatcherTimer
        {
            Interval = TimeSpan.FromMilliseconds(70)
        };
        adjustmentTimer.Tick += (_, _) =>
        {
            adjustmentTimer.Stop();
            BeginAdjustmentRender();
        };
        Loaded += (_, _) => BeginLoad();
        Unloaded += (_, _) =>
        {
            CancelLoad();
            CancelAdjustmentRender();
            StopTransition();
        };
        SizeChanged += (_, _) =>
        {
            if (bitmap is null)
            {
                BeginLoad();
                return;
            }

            ConstrainPan();
            InvalidateVisual();
        };
    }

    public string? SourcePath
    {
        get => (string?)GetValue(SourcePathProperty);
        set => SetValue(SourcePathProperty, value);
    }

    public BitmapSource? SourceBitmap
    {
        get => (BitmapSource?)GetValue(SourceBitmapProperty);
        set => SetValue(SourceBitmapProperty, value);
    }

    public EditRecipe EditRecipe
    {
        get => (EditRecipe)GetValue(EditRecipeProperty);
        set => SetValue(EditRecipeProperty, value);
    }

    public ICommand? DoubleClickCommand
    {
        get => (ICommand?)GetValue(DoubleClickCommandProperty);
        set => SetValue(DoubleClickCommandProperty, value);
    }

    public bool IsEditorMode
    {
        get => (bool)GetValue(IsEditorModeProperty);
        set => SetValue(IsEditorModeProperty, value);
    }

    public bool IsFullscreenMode
    {
        get => (bool)GetValue(IsFullscreenModeProperty);
        set => SetValue(IsFullscreenModeProperty, value);
    }

    public ICommand? MiddleClickCommand
    {
        get => (ICommand?)GetValue(MiddleClickCommandProperty);
        set => SetValue(MiddleClickCommandProperty, value);
    }

    public ICommand? PreviousPhotoCommand
    {
        get => (ICommand?)GetValue(PreviousPhotoCommandProperty);
        set => SetValue(PreviousPhotoCommandProperty, value);
    }

    public ICommand? NextPhotoCommand
    {
        get => (ICommand?)GetValue(NextPhotoCommandProperty);
        set => SetValue(NextPhotoCommandProperty, value);
    }

    public bool IsSelectionMode
    {
        get => (bool)GetValue(IsSelectionModeProperty);
        set => SetValue(IsSelectionModeProperty, value);
    }

    /// <summary>
    /// While active the next click samples a colour instead of panning, which
    /// is how the white balance eyedropper is armed.
    /// </summary>
    public bool IsColorPickerMode
    {
        get => (bool)GetValue(IsColorPickerModeProperty);
        set => SetValue(IsColorPickerModeProperty, value);
    }

    public PreviewComparisonMode ComparisonMode
    {
        get => (PreviewComparisonMode)GetValue(ComparisonModeProperty);
        set => SetValue(ComparisonModeProperty, value);
    }

    /// <summary>Raised with the averaged colour the eyedropper sampled.</summary>
    public event EventHandler<(double Red, double Green, double Blue)>?
        PreviewColorPicked;

    private static void OnColorPickerModeChanged(
        DependencyObject dependencyObject,
        DependencyPropertyChangedEventArgs eventArgs)
    {
        var viewer = (PhotoViewer)dependencyObject;
        viewer.Cursor = (bool)eventArgs.NewValue
            ? Cursors.Cross
            : Cursors.Arrow;
    }

    /// <summary>
    /// Width divided by height of the crop, measured as the user sees it.
    /// Zero means a free selection. Use -1 for "the photo's own ratio".
    /// </summary>
    public double SelectionAspectRatio
    {
        get => (double)GetValue(SelectionAspectRatioProperty);
        set => SetValue(SelectionAspectRatioProperty, value);
    }

    public bool HasSelection => (bool)GetValue(HasSelectionProperty);

    public CropRegion? SelectionRegion => selection;

    internal EditRecipe DisplayedRecipeForSmokeTest => bitmapRecipe;

    internal void SetSelectionForSmokeTest(CropRegion region) =>
        SetSelection(region);

    public void ToggleSelectionMode()
    {
        IsSelectionMode = !IsSelectionMode;
    }

    public void EndSelectionMode()
    {
        IsSelectionMode = false;
    }

    public void ClearSelection()
    {
        SetSelection(null);
    }

    public async Task<(int Width, int Height)> CopySelectionToClipboardAsync(
        CancellationToken cancellationToken = default)
    {
        if (selection is not { } selected)
        {
            return default;
        }

        var recipe = EditRecipe;
        var source = SourceBitmap;
        if (source is null)
        {
            if (string.IsNullOrWhiteSpace(SourcePath))
            {
                return default;
            }

            source = await App.Services.Previews.LoadAsync(
                SourcePath,
                0,
                cancellationToken);
        }

        cancellationToken.ThrowIfCancellationRequested();
        var rendered = RenderSelection(source, recipe, selected);
        await SetClipboardImageAsync(rendered, cancellationToken);
        return (rendered.PixelWidth, rendered.PixelHeight);
    }

    public void FitToViewport()
    {
        ResetView();
        if (isFullResolutionBitmap)
        {
            BeginLoad();
        }
    }

    public void ShowActualSize()
    {
        if (bitmap is null)
        {
            return;
        }

        if (SourceBitmap is not null)
        {
            SetActualSize();
            return;
        }

        if (string.IsNullOrWhiteSpace(SourcePath))
        {
            return;
        }

        if (isFullResolutionBitmap
            && string.Equals(
                bitmapPath,
                SourcePath,
                StringComparison.OrdinalIgnoreCase))
        {
            SetActualSize();
            return;
        }

        BeginLoad(fullResolution: true);
    }

    public void ZoomIn() => ChangeZoom(ZoomStep);

    public void ZoomOut() => ChangeZoom(1 / ZoomStep);

    public void Reload()
    {
        ResetView();
        BeginLoad();
    }

    protected override void OnRender(DrawingContext drawingContext)
    {
        base.OnRender(drawingContext);
        drawingContext.DrawRectangle(
            new SolidColorBrush(Color.FromRgb(12, 14, 18)),
            null,
            new Rect(RenderSize));

        if (bitmap is null)
        {
            var text = error ?? (SourcePath is null ? "Choose a photograph" : null);
            if (text is not null)
            {
                DrawCenteredText(drawingContext, text);
            }

            return;
        }

        if (previousBitmap is not null && transitionProgress < 1)
        {
            var easedProgress = SmoothStep(transitionProgress);
            DrawBitmap(
                drawingContext,
                previousBitmap,
                previousBitmapRecipe,
                1 - easedProgress);
            DrawBitmap(
                drawingContext,
                bitmap,
                bitmapRecipe,
                easedProgress);
            DrawLayers(drawingContext);
            DrawSelection(drawingContext);
            return;
        }

        DrawComparison(drawingContext);
        DrawLayers(drawingContext);
        DrawSelection(drawingContext);
    }

    /// <summary>
    /// Paints the edited surface, the untouched original, or both split down
    /// the middle so the two can be judged against each other in place.
    /// </summary>
    private void DrawComparison(DrawingContext drawingContext)
    {
        if (bitmap is null)
        {
            return;
        }

        var original = rawBitmap ?? bitmap;
        switch (ComparisonMode)
        {
            case PreviewComparisonMode.Original:
                DrawBitmap(drawingContext, original, bitmapRecipe, 1);
                DrawComparisonBadge(drawingContext, "BEFORE", alignRight: false);
                break;
            case PreviewComparisonMode.Split:
                var middle = ActualWidth / 2;
                drawingContext.PushClip(
                    new RectangleGeometry(new Rect(0, 0, middle, ActualHeight)));
                DrawBitmap(drawingContext, original, bitmapRecipe, 1);
                drawingContext.Pop();

                drawingContext.PushClip(
                    new RectangleGeometry(
                        new Rect(middle, 0, ActualWidth - middle, ActualHeight)));
                DrawBitmap(drawingContext, bitmap, bitmapRecipe, 1);
                drawingContext.Pop();

                drawingContext.DrawRectangle(
                    SelectionHandleBrush,
                    null,
                    new Rect(middle - 0.5, 0, 1, ActualHeight));
                DrawComparisonBadge(drawingContext, "BEFORE", alignRight: false);
                DrawComparisonBadge(drawingContext, "AFTER", alignRight: true);
                break;
            default:
                DrawBitmap(drawingContext, bitmap, bitmapRecipe, 1);
                break;
        }
    }

    private void DrawComparisonBadge(
        DrawingContext drawingContext,
        string text,
        bool alignRight)
    {
        var formatted = new FormattedText(
            text,
            System.Globalization.CultureInfo.CurrentUICulture,
            FlowDirection.LeftToRight,
            new Typeface("Segoe UI"),
            11,
            SelectionHandleBrush,
            VisualTreeHelper.GetDpi(this).PixelsPerDip);
        var padding = 6d;
        var x = alignRight
            ? ActualWidth - formatted.Width - padding - 10
            : 10 + padding;
        var background = new Rect(
            x - padding,
            10,
            formatted.Width + (padding * 2),
            formatted.Height + 4);
        drawingContext.DrawRoundedRectangle(
            SelectionShadeBrush,
            null,
            background,
            3,
            3);
        drawingContext.DrawText(formatted, new Point(x, 12));
    }

    private void DrawBitmap(
        DrawingContext drawingContext,
        BitmapSource source,
        EditRecipe recipe,
        double opacity)
    {
        var fit = GetFitScale(source, recipe);
        var scale = Math.Max(0.0001, fit * zoom);
        var crop = GetCropPixelRect(source, recipe);

        var group = CreateViewerTransform(source, recipe, pan);

        drawingContext.PushOpacity(opacity);
        drawingContext.PushTransform(group);
        drawingContext.PushClip(
            new RectangleGeometry(
                new Rect(
                    -crop.Width / 2,
                    -crop.Height / 2,
                    crop.Width,
                    crop.Height)));
        drawingContext.DrawImage(
            source,
            new Rect(
                -crop.X - (crop.Width / 2),
                -crop.Y - (crop.Height / 2),
                source.PixelWidth,
                source.PixelHeight));
        drawingContext.Pop();
        drawingContext.Pop();
        drawingContext.Pop();
    }

    private void DrawSelection(DrawingContext drawingContext)
    {
        if (!IsSelectionMode
            || selection is not { } selected
            || bitmap is null)
        {
            return;
        }

        var imageBounds = GetScreenBounds(
            bitmap,
            EditRecipe,
            GetRecipeCrop(EditRecipe),
            pan);
        var selectionBounds = GetScreenBounds(
            bitmap,
            EditRecipe,
            selected,
            pan);
        var visibleImage = Rect.Intersect(
            imageBounds,
            new Rect(RenderSize));
        var visibleSelection = Rect.Intersect(
            selectionBounds,
            visibleImage);

        if (!visibleImage.IsEmpty && !visibleSelection.IsEmpty)
        {
            DrawSelectionShade(
                drawingContext,
                visibleImage,
                visibleSelection);
        }

        drawingContext.DrawRectangle(
            Brushes.Transparent,
            SelectionBorderPen,
            selectionBounds);
        DrawSelectionHandles(drawingContext, selectionBounds);
    }

    private static void DrawSelectionShade(
        DrawingContext drawingContext,
        Rect image,
        Rect selectionBounds)
    {
        DrawShadeRect(
            drawingContext,
            new Rect(
                image.Left,
                image.Top,
                image.Width,
                Math.Max(0, selectionBounds.Top - image.Top)));
        DrawShadeRect(
            drawingContext,
            new Rect(
                image.Left,
                selectionBounds.Bottom,
                image.Width,
                Math.Max(0, image.Bottom - selectionBounds.Bottom)));
        DrawShadeRect(
            drawingContext,
            new Rect(
                image.Left,
                selectionBounds.Top,
                Math.Max(0, selectionBounds.Left - image.Left),
                selectionBounds.Height));
        DrawShadeRect(
            drawingContext,
            new Rect(
                selectionBounds.Right,
                selectionBounds.Top,
                Math.Max(0, image.Right - selectionBounds.Right),
                selectionBounds.Height));
    }

    private static void DrawShadeRect(
        DrawingContext drawingContext,
        Rect rectangle)
    {
        if (rectangle.Width > 0 && rectangle.Height > 0)
        {
            drawingContext.DrawRectangle(
                SelectionShadeBrush,
                null,
                rectangle);
        }
    }

    private static void DrawSelectionHandles(
        DrawingContext drawingContext,
        Rect bounds)
    {
        const double handleSize = 7;
        var radius = handleSize / 2;
        var points = new[]
        {
            bounds.TopLeft,
            new Point(bounds.Left + bounds.Width / 2, bounds.Top),
            bounds.TopRight,
            new Point(bounds.Right, bounds.Top + bounds.Height / 2),
            bounds.BottomRight,
            new Point(bounds.Left + bounds.Width / 2, bounds.Bottom),
            bounds.BottomLeft,
            new Point(bounds.Left, bounds.Top + bounds.Height / 2)
        };

        foreach (var point in points)
        {
            drawingContext.DrawRoundedRectangle(
                SelectionHandleBrush,
                SelectionBorderPen,
                new Rect(
                    point.X - radius,
                    point.Y - radius,
                    handleSize,
                    handleSize),
                1.5,
                1.5);
        }
    }

    protected override void OnMouseWheel(MouseWheelEventArgs e)
    {
        base.OnMouseWheel(e);
        if ((Keyboard.Modifiers & ModifierKeys.Control) == 0)
        {
            var command = e.Delta > 0
                ? PreviousPhotoCommand
                : NextPhotoCommand;
            if (command?.CanExecute(null) == true)
            {
                command.Execute(null);
            }

            e.Handled = true;
            return;
        }

        ChangeZoom(e.Delta > 0 ? ZoomStep : 1 / ZoomStep);
        e.Handled = true;
    }

    protected override void OnMouseLeftButtonDown(MouseButtonEventArgs e)
    {
        base.OnMouseLeftButtonDown(e);
        Focus();

        if (IsColorPickerMode)
        {
            if (SampleOriginalColor(e.GetPosition(this)) is { } sample)
            {
                PreviewColorPicked?.Invoke(this, sample);
            }

            IsColorPickerMode = false;
            e.Handled = true;
            return;
        }

        if (TryBeginLayerInteraction(e.GetPosition(this)))
        {
            e.Handled = true;
            return;
        }

        if (IsSelectionMode
            && (Keyboard.Modifiers & ModifierKeys.Control) == 0
            && !Keyboard.IsKeyDown(Key.Space))
        {
            if (e.ClickCount == 1)
            {
                BeginSelectionDrag(e.GetPosition(this));
            }

            e.Handled = true;
            return;
        }

        if (e.ClickCount == 2)
        {
            if (DoubleClickCommand?.CanExecute(null) == true)
            {
                DoubleClickCommand.Execute(null);
            }

            e.Handled = true;
            return;
        }

        isDragging = true;
        dragOrigin = e.GetPosition(this);
        panOrigin = pan;
        CaptureMouse();
        Cursor = Cursors.SizeAll;
        e.Handled = true;
    }

    protected override void OnMouseDown(MouseButtonEventArgs e)
    {
        base.OnMouseDown(e);
        if (e.ChangedButton != MouseButton.Middle)
        {
            return;
        }

        Focus();
        if (MiddleClickCommand?.CanExecute(null) == true)
        {
            MiddleClickCommand.Execute(null);
        }

        e.Handled = true;
    }

    protected override void OnMouseMove(MouseEventArgs e)
    {
        base.OnMouseMove(e);
        var current = e.GetPosition(this);
        if (TryUpdateLayerInteraction(current))
        {
            e.Handled = true;
            return;
        }

        if (selectionOperation != SelectionOperation.None)
        {
            UpdateSelectionDrag(current);
            e.Handled = true;
            return;
        }

        if (!isDragging)
        {
            UpdateCursor(current);
            return;
        }

        pan = panOrigin + (current - dragOrigin);
        ConstrainPan();
        InvalidateVisual();
    }

    protected override void OnMouseLeftButtonUp(MouseButtonEventArgs e)
    {
        base.OnMouseLeftButtonUp(e);
        FinishLayerInteraction();
        if (selectionOperation != SelectionOperation.None)
        {
            FinishSelectionDrag();
        }

        isDragging = false;
        if (IsMouseCaptured)
        {
            ReleaseMouseCapture();
        }

        UpdateCursor(e.GetPosition(this));
    }

    private void ChangeZoom(double factor)
    {
        var maximumZoom = bitmap is null
            ? MaximumActualScale
            : Math.Max(
                1,
                MaximumActualScale / GetFitScale(bitmap, EditRecipe));
        zoom = Math.Clamp(
            zoom * factor,
            MinimumZoom,
            maximumZoom);
        ConstrainPan();
        InvalidateVisual();
    }

    private void SetActualSize()
    {
        if (bitmap is null)
        {
            return;
        }

        zoom = 1 / GetFitScale(bitmap, EditRecipe);
        pan = default;
        ConstrainPan();
        InvalidateVisual();
    }

    private double GetFitScale(BitmapSource source, EditRecipe recipe)
    {
        var rotation = (int)recipe.Rotation * 90;
        var swapsDimensions = rotation is 90 or 270;
        var crop = GetCropPixelRect(source, recipe);
        var displayedWidth = swapsDimensions ? crop.Height : crop.Width;
        var displayedHeight = swapsDimensions ? crop.Width : crop.Height;
        return Math.Max(
            0.0001,
            Math.Min(
                ActualWidth / Math.Max(1, displayedWidth),
                ActualHeight / Math.Max(1, displayedHeight)));
    }

    private static void OnViewerPropertyChanged(
        DependencyObject dependencyObject,
        DependencyPropertyChangedEventArgs eventArgs)
    {
        var viewer = (PhotoViewer)dependencyObject;
        viewer.ClearSelection();
        viewer.ResetView();
        viewer.BeginLoad();
    }

    private static void OnEditRecipeChanged(
        DependencyObject dependencyObject,
        DependencyPropertyChangedEventArgs eventArgs)
    {
        var viewer = (PhotoViewer)dependencyObject;
        var displaysInMemorySource =
            viewer.SourceBitmap is { } sourceBitmap
            && ReferenceEquals(viewer.rawBitmap, sourceBitmap);
        var displaysPathSource =
            viewer.SourceBitmap is null
            && !string.IsNullOrWhiteSpace(viewer.SourcePath)
            && string.Equals(
                viewer.bitmapPath,
                viewer.SourcePath,
                StringComparison.OrdinalIgnoreCase);
        if (displaysInMemorySource || displaysPathSource)
        {
            viewer.bitmapRecipe = (EditRecipe)eventArgs.NewValue;
            viewer.ScheduleAdjustmentRender();
        }

        viewer.ConstrainPan();
        viewer.InvalidateVisual();
    }

    /// <summary>
    /// True when the pixel work in the recipe differs from what the currently
    /// displayed surface was rendered with. Geometry is excluded: crop,
    /// rotation and flips are transforms the canvas applies for free.
    /// </summary>
    private bool NeedsAdjustmentRender()
    {
        if (rawBitmap is null)
        {
            return false;
        }

        var recipe = EditRecipe;
        return renderedPixelRecipe is null
               || renderedPixelRecipe.Adjustments != recipe.Adjustments
               || !renderedPixelRecipe.Filters.SequenceEqual(recipe.Filters)
               || renderedPixelRecipe.StraightenAngle != recipe.StraightenAngle
               || renderedPixelRecipe.PerspectiveVertical
                   != recipe.PerspectiveVertical
               || renderedPixelRecipe.PerspectiveHorizontal
                   != recipe.PerspectiveHorizontal;
    }

    private void ScheduleAdjustmentRender()
    {
        if (!NeedsAdjustmentRender())
        {
            return;
        }

        adjustmentTimer.Stop();
        adjustmentTimer.Start();
    }

    private void CancelAdjustmentRender()
    {
        adjustmentTimer.Stop();
        adjustmentCancellation?.Cancel();
        adjustmentCancellation?.Dispose();
        adjustmentCancellation = null;
    }

    private async void BeginAdjustmentRender()
    {
        if (rawBitmap is not { } source)
        {
            return;
        }

        adjustmentCancellation?.Cancel();
        adjustmentCancellation?.Dispose();
        adjustmentCancellation = new CancellationTokenSource();
        var token = adjustmentCancellation.Token;
        var recipe = EditRecipe;

        try
        {
            // Explicitly marshalled: the render can complete before the
            // dispatcher starts pumping, and a continuation on a thread-pool
            // thread would touch the visual tree from the wrong thread.
            var rendered = await Task.Run(
                    () => ImageRenderer.RenderPreviewSurface(source, recipe, token),
                    token)
                .ConfigureAwait(false);
            await Dispatcher.InvokeAsync(() =>
            {
                if (token.IsCancellationRequested
                    || !ReferenceEquals(rawBitmap, source))
                {
                    return;
                }

                bitmap = rendered;
                renderedPixelRecipe = recipe;
                PreviewRendered?.Invoke(this, EventArgs.Empty);
                InvalidateVisual();
            });
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception exception)
        {
            await Dispatcher.InvokeAsync(() =>
            {
                error = $"The preview could not be rendered\n{exception.Message}";
                InvalidateVisual();
            });
        }
    }

    /// <summary>Raised whenever a freshly adjusted surface is on screen.</summary>
    public event EventHandler? PreviewRendered;

    /// <summary>
    /// The surface currently on the canvas, used by the histogram so it
    /// measures exactly what the user is looking at.
    /// </summary>
    public BitmapSource? DisplayedBitmap => bitmap;

    /// <summary>The decoded pixels before any adjustment, for before/after.</summary>
    public BitmapSource? OriginalBitmap => rawBitmap;

    /// <summary>
    /// Samples the unadjusted image at a viewer point, which is what the
    /// white-balance eyedropper needs: it must measure the original cast,
    /// not the cast left after the correction already applied.
    /// </summary>
    public (double Red, double Green, double Blue)? SampleOriginalColor(
        Point viewerPoint)
    {
        if (rawBitmap is not { } source
            || !TryGetNormalizedSourcePoint(
                viewerPoint,
                clampToImage: true,
                out var normalized,
                out var isInsideImage)
            || !isInsideImage)
        {
            return null;
        }

        // A small patch rather than one pixel, so sensor noise cannot decide
        // the white balance of the whole photograph.
        const int patch = 5;
        var x = Math.Clamp(
            (int)(normalized.X * source.PixelWidth) - (patch / 2),
            0,
            Math.Max(0, source.PixelWidth - patch));
        var y = Math.Clamp(
            (int)(normalized.Y * source.PixelHeight) - (patch / 2),
            0,
            Math.Max(0, source.PixelHeight - patch));
        var width = Math.Min(patch, source.PixelWidth);
        var height = Math.Min(patch, source.PixelHeight);

        var converted = source.Format == PixelFormats.Bgra32
            ? source
            : new FormatConvertedBitmap(source, PixelFormats.Bgra32, null, 0);
        var stride = width * 4;
        var pixels = new byte[stride * height];
        converted.CopyPixels(
            new Int32Rect(x, y, width, height),
            pixels,
            stride,
            0);

        double red = 0, green = 0, blue = 0;
        var count = width * height;
        for (var index = 0; index < pixels.Length; index += 4)
        {
            blue += pixels[index];
            green += pixels[index + 1];
            red += pixels[index + 2];
        }

        return (red / count, green / count, blue / count);
    }

    private static void OnSelectionModeChanged(
        DependencyObject dependencyObject,
        DependencyPropertyChangedEventArgs eventArgs)
    {
        var viewer = (PhotoViewer)dependencyObject;
        if (!(bool)eventArgs.NewValue)
        {
            viewer.ClearSelection();
            viewer.selectionOperation = SelectionOperation.None;
            viewer.isDragging = false;
            if (viewer.IsMouseCaptured)
            {
                viewer.ReleaseMouseCapture();
            }
        }

        viewer.ConstrainPan();
        viewer.UpdateCursor(Mouse.GetPosition(viewer));
        viewer.InvalidateVisual();
    }

    private void BeginSelectionDrag(Point position)
    {
        if (!TryGetNormalizedSourcePoint(
                position,
                clampToImage: false,
                out var sourcePoint,
                out _))
        {
            return;
        }

        selectionOperation = HitTestSelection(sourcePoint);
        selectionDragOrigin = sourcePoint;
        selectionAtDragStart = selection ?? default;
        if (selectionOperation == SelectionOperation.None)
        {
            selectionOperation = SelectionOperation.Create;
            SetSelection(null);
        }

        isDragging = true;
        CaptureMouse();
        UpdateCursor(position);
    }

    private void UpdateSelectionDrag(Point position)
    {
        if (!TryGetNormalizedSourcePoint(
                position,
                clampToImage:
                    selectionOperation != SelectionOperation.Create,
                out var current,
                out _))
        {
            return;
        }

        var imageBounds = GetRecipeCrop(EditRecipe);
        CropRegion updated;
        if (selectionOperation == SelectionOperation.Create)
        {
            updated = CreateSelectionFromDrag(
                imageBounds,
                selectionDragOrigin,
                current);
            updated = ApplyAspectRatio(
                updated,
                imageBounds,
                anchorLeft: current.X >= selectionDragOrigin.X,
                anchorTop: current.Y >= selectionDragOrigin.Y);
        }
        else if (selectionOperation == SelectionOperation.Move)
        {
            var delta = current - selectionDragOrigin;
            updated = MoveRegionWithin(
                selectionAtDragStart,
                delta,
                imageBounds);
        }
        else
        {
            updated = ResizeSelection(current, imageBounds);
            updated = ApplyAspectRatio(
                updated,
                imageBounds,
                anchorLeft: !selectionOperation.HasFlag(SelectionOperation.Left),
                anchorTop: !selectionOperation.HasFlag(SelectionOperation.Top));
        }

        SetSelection(updated);
        UpdateCursor(position);
    }

    private static void OnSelectionAspectRatioChanged(
        DependencyObject dependencyObject,
        DependencyPropertyChangedEventArgs eventArgs)
    {
        var viewer = (PhotoViewer)dependencyObject;
        if (viewer.selection is not { } selected)
        {
            return;
        }

        viewer.SetSelection(
            viewer.ApplyAspectRatio(
                selected,
                GetRecipeCrop(viewer.EditRecipe),
                anchorLeft: true,
                anchorTop: true));
    }

    /// <summary>
    /// Forces a region to the requested display aspect ratio, keeping the
    /// corner the user is dragging away from fixed and shrinking rather than
    /// growing so the result always stays inside the photo.
    /// </summary>
    private CropRegion ApplyAspectRatio(
        CropRegion region,
        CropRegion bounds,
        bool anchorLeft,
        bool anchorTop)
    {
        var ratio = GetEffectiveAspectRatio();
        if (ratio <= 0 || bitmap is null || region.IsEmpty)
        {
            return region;
        }

        // The region is normalized to the frame, so a display ratio has to be
        // converted into the frame's own coordinate space first.
        var pixelRatio = ratio
                         * bitmap.PixelHeight
                         / Math.Max(1, bitmap.PixelWidth);

        var width = region.Width;
        var height = region.Height;
        if (width / Math.Max(0.0001, height) > pixelRatio)
        {
            width = height * pixelRatio;
        }
        else
        {
            height = width / Math.Max(0.0001, pixelRatio);
        }

        var left = anchorLeft ? region.X : region.Right - width;
        var top = anchorTop ? region.Y : region.Bottom - height;

        // Clamp back inside the frame, preserving the ratio if the drag ran
        // past an edge.
        if (left < bounds.X)
        {
            left = bounds.X;
        }

        if (top < bounds.Y)
        {
            top = bounds.Y;
        }

        if (left + width > bounds.Right)
        {
            width = bounds.Right - left;
            height = width / Math.Max(0.0001, pixelRatio);
        }

        if (top + height > bounds.Bottom)
        {
            height = bounds.Bottom - top;
            width = height * pixelRatio;
        }

        return new CropRegion(left, top, Math.Max(0, width), Math.Max(0, height));
    }

    private double GetEffectiveAspectRatio()
    {
        var ratio = SelectionAspectRatio;
        if (ratio == 0 || bitmap is null)
        {
            return 0;
        }

        if (ratio < 0)
        {
            // "Original" means the ratio of the whole frame as displayed.
            ratio = bitmap.PixelWidth / (double)Math.Max(1, bitmap.PixelHeight);
            return ratio;
        }

        // A quarter turn swaps what "wider than tall" means on screen.
        return EditRecipe.Rotation is QuarterRotation.Clockwise90
            or QuarterRotation.Clockwise270
            ? 1 / ratio
            : ratio;
    }

    private CropRegion ResizeSelection(
        Point current,
        CropRegion imageBounds)
    {
        var left = selectionAtDragStart.X;
        var top = selectionAtDragStart.Y;
        var right = selectionAtDragStart.Right;
        var bottom = selectionAtDragStart.Bottom;
        var minimum = GetMinimumSelectionSize();

        if (selectionOperation.HasFlag(SelectionOperation.Left))
        {
            left = Math.Clamp(
                current.X,
                imageBounds.X,
                Math.Max(imageBounds.X, right - minimum.Width));
        }
        else if (selectionOperation.HasFlag(SelectionOperation.Right))
        {
            right = Math.Clamp(
                current.X,
                Math.Min(imageBounds.Right, left + minimum.Width),
                imageBounds.Right);
        }

        if (selectionOperation.HasFlag(SelectionOperation.Top))
        {
            top = Math.Clamp(
                current.Y,
                imageBounds.Y,
                Math.Max(imageBounds.Y, bottom - minimum.Height));
        }
        else if (selectionOperation.HasFlag(SelectionOperation.Bottom))
        {
            bottom = Math.Clamp(
                current.Y,
                Math.Min(imageBounds.Bottom, top + minimum.Height),
                imageBounds.Bottom);
        }

        return new CropRegion(
            left,
            top,
            Math.Max(0, right - left),
            Math.Max(0, bottom - top));
    }

    private void FinishSelectionDrag()
    {
        selectionOperation = SelectionOperation.None;
        if (selection is { } selected && bitmap is not null)
        {
            var bounds = GetScreenBounds(
                bitmap,
                EditRecipe,
                selected,
                pan);
            if (bounds.Width < MinimumSelectionSize
                || bounds.Height < MinimumSelectionSize)
            {
                SetSelection(null);
            }
        }

        ConstrainPan();
        InvalidateVisual();
    }

    private SelectionOperation HitTestSelection(Point sourcePoint)
    {
        if (selection is not { } selected || bitmap is null)
        {
            return SelectionOperation.None;
        }

        var tolerance = GetNormalizedSelectionTolerance();
        var withinHorizontal = sourcePoint.X >= selected.X - tolerance.Width
            && sourcePoint.X <= selected.Right + tolerance.Width;
        var withinVertical = sourcePoint.Y >= selected.Y - tolerance.Height
            && sourcePoint.Y <= selected.Bottom + tolerance.Height;
        if (!withinHorizontal || !withinVertical)
        {
            return SelectionOperation.None;
        }

        var operation = SelectionOperation.None;
        if (Math.Abs(sourcePoint.X - selected.X) <= tolerance.Width)
        {
            operation |= SelectionOperation.Left;
        }
        else if (Math.Abs(sourcePoint.X - selected.Right) <= tolerance.Width)
        {
            operation |= SelectionOperation.Right;
        }

        if (Math.Abs(sourcePoint.Y - selected.Y) <= tolerance.Height)
        {
            operation |= SelectionOperation.Top;
        }
        else if (Math.Abs(sourcePoint.Y - selected.Bottom) <= tolerance.Height)
        {
            operation |= SelectionOperation.Bottom;
        }

        if (operation != SelectionOperation.None)
        {
            return operation;
        }

        return sourcePoint.X >= selected.X
               && sourcePoint.X <= selected.Right
               && sourcePoint.Y >= selected.Y
               && sourcePoint.Y <= selected.Bottom
            ? SelectionOperation.Move
            : SelectionOperation.None;
    }

    private void UpdateCursor(Point position)
    {
        if (!IsSelectionMode)
        {
            Cursor = isDragging ? Cursors.SizeAll : Cursors.Arrow;
            return;
        }

        if (Keyboard.IsKeyDown(Key.Space))
        {
            Cursor = Cursors.SizeAll;
            return;
        }

        if (selectionOperation != SelectionOperation.None)
        {
            Cursor = GetSelectionOperationCursor(selectionOperation);
            return;
        }

        if (TryGetNormalizedSourcePoint(
                position,
                clampToImage: false,
                out var sourcePoint,
                out var isInsideImage)
            && isInsideImage)
        {
            Cursor = GetSelectionOperationCursor(
                HitTestSelection(sourcePoint));
            return;
        }

        Cursor = Cursors.Cross;
    }

    private Cursor GetSelectionOperationCursor(
        SelectionOperation operation)
    {
        if (operation == SelectionOperation.Move)
        {
            return Cursors.SizeAll;
        }

        if (operation is SelectionOperation.None
            or SelectionOperation.Create
            || bitmap is null
            || selection is not { } selected)
        {
            return Cursors.Cross;
        }

        var hasHorizontalEdge =
            (operation
             & (SelectionOperation.Left | SelectionOperation.Right)) != 0;
        var hasVerticalEdge =
            (operation
             & (SelectionOperation.Top | SelectionOperation.Bottom)) != 0;
        var handle = new Point(
            operation.HasFlag(SelectionOperation.Left)
                ? selected.X
                : operation.HasFlag(SelectionOperation.Right)
                    ? selected.Right
                    : selected.X + selected.Width / 2,
            operation.HasFlag(SelectionOperation.Top)
                ? selected.Y
                : operation.HasFlag(SelectionOperation.Bottom)
                    ? selected.Bottom
                    : selected.Y + selected.Height / 2);
        var center = new Point(
            selected.X + selected.Width / 2,
            selected.Y + selected.Height / 2);
        return ResolveResizeCursor(
            GetScreenPoint(bitmap, EditRecipe, handle, pan),
            GetScreenPoint(bitmap, EditRecipe, center, pan),
            hasHorizontalEdge && hasVerticalEdge);
    }

    internal static Cursor ResolveResizeCursor(
        Point handle,
        Point center,
        bool isCorner)
    {
        var offset = handle - center;
        if (!isCorner)
        {
            return Math.Abs(offset.X) >= Math.Abs(offset.Y)
                ? Cursors.SizeWE
                : Cursors.SizeNS;
        }

        return offset.X * offset.Y >= 0
            ? Cursors.SizeNWSE
            : Cursors.SizeNESW;
    }

    private bool TryGetNormalizedSourcePoint(
        Point viewerPoint,
        bool clampToImage,
        out Point sourcePoint,
        out bool isInsideImage)
    {
        sourcePoint = default;
        isInsideImage = false;
        if (bitmap is null)
        {
            return false;
        }

        var transform = CreateViewerTransform(bitmap, EditRecipe, pan);
        var inverse = transform.Inverse;
        if (inverse is null)
        {
            return false;
        }

        var crop = GetCropPixelRect(bitmap, EditRecipe);
        var local = inverse.Transform(viewerPoint);
        var sourceX = local.X + crop.X + crop.Width / 2;
        var sourceY = local.Y + crop.Y + crop.Height / 2;
        var normalized = new Point(
            sourceX / Math.Max(1, bitmap.PixelWidth),
            sourceY / Math.Max(1, bitmap.PixelHeight));
        var imageRegion = GetRecipeCrop(EditRecipe);
        isInsideImage =
            normalized.X >= imageRegion.X
            && normalized.X <= imageRegion.Right
            && normalized.Y >= imageRegion.Y
            && normalized.Y <= imageRegion.Bottom;

        sourcePoint = clampToImage
            ? new Point(
                Math.Clamp(normalized.X, imageRegion.X, imageRegion.Right),
                Math.Clamp(normalized.Y, imageRegion.Y, imageRegion.Bottom))
            : normalized;
        return true;
    }

    private void SetSelection(CropRegion? value)
    {
        CropRegion? normalized = null;
        if (value is { } candidate)
        {
            var constrained = IntersectRegions(
                candidate.ConstrainToUnit(),
                GetRecipeCrop(EditRecipe));
            if (!constrained.IsEmpty)
            {
                normalized = constrained;
            }
        }

        selection = normalized;
        SetValue(HasSelectionPropertyKey, normalized is not null);
        InvalidateVisual();
    }

    private Size GetNormalizedSelectionTolerance()
    {
        if (bitmap is null)
        {
            return default;
        }

        var scale = Math.Max(
            0.0001,
            GetFitScale(bitmap, EditRecipe) * zoom);
        return new Size(
            SelectionHitTolerance
            / Math.Max(1, scale * bitmap.PixelWidth),
            SelectionHitTolerance
            / Math.Max(1, scale * bitmap.PixelHeight));
    }

    private Size GetMinimumSelectionSize()
    {
        if (bitmap is null)
        {
            return default;
        }

        var scale = Math.Max(
            0.0001,
            GetFitScale(bitmap, EditRecipe) * zoom);
        return new Size(
            MinimumSelectionSize
            / Math.Max(1, scale * bitmap.PixelWidth),
            MinimumSelectionSize
            / Math.Max(1, scale * bitmap.PixelHeight));
    }

    private void ConstrainPan()
    {
        if (bitmap is null || ActualWidth <= 0 || ActualHeight <= 0)
        {
            return;
        }

        var anchor = IsSelectionMode && selection is { } selected
            ? selected
            : GetRecipeCrop(EditRecipe);
        var baseBounds = GetScreenBounds(
            bitmap,
            EditRecipe,
            anchor,
            default);
        if (baseBounds.IsEmpty)
        {
            return;
        }

        pan = ConstrainPanToVisible(
            baseBounds,
            new Size(ActualWidth, ActualHeight),
            pan);
    }

    internal static Vector ConstrainPanToVisible(
        Rect baseBounds,
        Size viewport,
        Vector requestedPan)
    {
        var visibleWidth = CalculateGrabSize(baseBounds.Width);
        var visibleHeight = CalculateGrabSize(baseBounds.Height);
        return ConstrainPanToVisible(
            baseBounds,
            viewport,
            requestedPan,
            visibleWidth,
            visibleHeight);
    }

    private static Vector ConstrainPanToVisible(
        Rect baseBounds,
        Size viewport,
        Vector requestedPan,
        double visibleWidth,
        double visibleHeight)
    {
        visibleWidth = Math.Min(visibleWidth, viewport.Width);
        visibleHeight = Math.Min(visibleHeight, viewport.Height);
        var minimumX = visibleWidth - baseBounds.Right;
        var maximumX = viewport.Width - visibleWidth - baseBounds.Left;
        var minimumY = visibleHeight - baseBounds.Bottom;
        var maximumY = viewport.Height - visibleHeight - baseBounds.Top;
        return new Vector(
            Math.Clamp(requestedPan.X, minimumX, maximumX),
            Math.Clamp(requestedPan.Y, minimumY, maximumY));
    }

    private TransformGroup CreateViewerTransform(
        BitmapSource source,
        EditRecipe recipe,
        Vector translation)
    {
        var scale = Math.Max(
            0.0001,
            GetFitScale(source, recipe) * zoom);
        var group = new TransformGroup();
        group.Children.Add(
            new ScaleTransform(
                recipe.FlipHorizontal ? -scale : scale,
                scale));
        group.Children.Add(
            new RotateTransform((int)recipe.Rotation * 90));
        group.Children.Add(
            new TranslateTransform(
                (ActualWidth / 2) + translation.X,
                (ActualHeight / 2) + translation.Y));
        return group;
    }

    private Rect GetScreenBounds(
        BitmapSource source,
        EditRecipe recipe,
        CropRegion region,
        Vector translation)
    {
        var crop = GetCropPixelRect(source, recipe);
        var regionPixels = GetPixelRect(source, region);
        var localLeft = regionPixels.X - crop.X - crop.Width / 2;
        var localTop = regionPixels.Y - crop.Y - crop.Height / 2;
        var localRight = localLeft + regionPixels.Width;
        var localBottom = localTop + regionPixels.Height;
        var transform = CreateViewerTransform(
            source,
            recipe,
            translation);
        var points = new[]
        {
            transform.Transform(new Point(localLeft, localTop)),
            transform.Transform(new Point(localRight, localTop)),
            transform.Transform(new Point(localRight, localBottom)),
            transform.Transform(new Point(localLeft, localBottom))
        };
        var left = points.Min(point => point.X);
        var top = points.Min(point => point.Y);
        var right = points.Max(point => point.X);
        var bottom = points.Max(point => point.Y);
        return new Rect(
            left,
            top,
            Math.Max(0, right - left),
            Math.Max(0, bottom - top));
    }

    private Point GetScreenPoint(
        BitmapSource source,
        EditRecipe recipe,
        Point normalizedSourcePoint,
        Vector translation)
    {
        var crop = GetCropPixelRect(source, recipe);
        var local = new Point(
            normalizedSourcePoint.X * source.PixelWidth
            - crop.X
            - crop.Width / 2,
            normalizedSourcePoint.Y * source.PixelHeight
            - crop.Y
            - crop.Height / 2);
        return CreateViewerTransform(
            source,
            recipe,
            translation).Transform(local);
    }

    private static double CalculateGrabSize(double displayedSize) =>
        Math.Min(
            displayedSize,
            Math.Clamp(
                displayedSize * GrabAreaFraction,
                MinimumGrabArea,
                MaximumGrabArea));

    private static CropRegion GetRecipeCrop(EditRecipe recipe)
    {
        var crop = recipe.Crop?.ConstrainToUnit();
        return crop is { IsEmpty: false }
            ? crop.Value
            : new CropRegion(0, 0, 1, 1);
    }

    private static Rect GetCropPixelRect(
        BitmapSource source,
        EditRecipe recipe) =>
        GetPixelRect(source, GetRecipeCrop(recipe));

    private static Rect GetPixelRect(
        BitmapSource source,
        CropRegion region) =>
        new(
            region.X * source.PixelWidth,
            region.Y * source.PixelHeight,
            region.Width * source.PixelWidth,
            region.Height * source.PixelHeight);

    internal static Int32Rect GetPixelSelectionRect(
        BitmapSource source,
        CropRegion region)
    {
        var constrained = region.ConstrainToUnit();
        var left = Math.Clamp(
            (int)Math.Floor(constrained.X * source.PixelWidth),
            0,
            Math.Max(0, source.PixelWidth - 1));
        var top = Math.Clamp(
            (int)Math.Floor(constrained.Y * source.PixelHeight),
            0,
            Math.Max(0, source.PixelHeight - 1));
        var right = Math.Clamp(
            (int)Math.Ceiling(constrained.Right * source.PixelWidth),
            left + 1,
            source.PixelWidth);
        var bottom = Math.Clamp(
            (int)Math.Ceiling(constrained.Bottom * source.PixelHeight),
            top + 1,
            source.PixelHeight);
        return new Int32Rect(
            left,
            top,
            right - left,
            bottom - top);
    }

    /// <summary>
    /// Renders <paramref name="region"/> of the photograph with the whole
    /// recipe applied - geometry, adjustments, filters and annotation layers.
    /// </summary>
    internal static BitmapSource RenderSelection(
        BitmapSource source,
        EditRecipe recipe,
        CropRegion region) =>
        ImageRenderer.Render(
            source,
            recipe,
            new RenderRequest(RegionOverride: region));

    private static async Task SetClipboardImageAsync(
        BitmapSource image,
        CancellationToken cancellationToken)
    {
        for (var attempt = 0; ; attempt++)
        {
            cancellationToken.ThrowIfCancellationRequested();
            try
            {
                Clipboard.SetImage(image);
                return;
            }
            catch (ExternalException) when (attempt < 2)
            {
                await Task.Delay(35, cancellationToken);
            }
        }
    }

    private static CropRegion IntersectRegions(
        CropRegion first,
        CropRegion second)
    {
        var left = Math.Max(first.X, second.X);
        var top = Math.Max(first.Y, second.Y);
        var right = Math.Min(first.Right, second.Right);
        var bottom = Math.Min(first.Bottom, second.Bottom);
        return new CropRegion(
            left,
            top,
            Math.Max(0, right - left),
            Math.Max(0, bottom - top));
    }

    internal static CropRegion CreateSelectionFromDrag(
        CropRegion imageBounds,
        Point dragStart,
        Point dragEnd) =>
        IntersectRegions(
            CropRegion.FromPoints(
                dragStart.X,
                dragStart.Y,
                dragEnd.X,
                dragEnd.Y),
            imageBounds);

    private static CropRegion MoveRegionWithin(
        CropRegion region,
        Vector delta,
        CropRegion bounds)
    {
        var x = Math.Clamp(
            region.X + delta.X,
            bounds.X,
            Math.Max(bounds.X, bounds.Right - region.Width));
        var y = Math.Clamp(
            region.Y + delta.Y,
            bounds.Y,
            Math.Max(bounds.Y, bounds.Bottom - region.Height));
        return region with { X = x, Y = y };
    }

    private static SolidColorBrush CreateFrozenBrush(Color color)
    {
        var brush = new SolidColorBrush(color);
        brush.Freeze();
        return brush;
    }

    private static Pen CreateFrozenPen(
        Color color,
        double thickness)
    {
        var pen = new Pen(
            CreateFrozenBrush(color),
            thickness);
        pen.Freeze();
        return pen;
    }

    private void BeginLoad(bool fullResolution = false)
    {
        CancelLoad();
        CancelAdjustmentRender();
        StopTransition();
        error = null;
        renderedPixelRecipe = null;
        InvalidateVisual();

        if (SourceBitmap is { } inMemory)
        {
            rawBitmap = inMemory;
            bitmap = inMemory;
            bitmapPath = null;
            bitmapRecipe = EditRecipe;
            isFullResolutionBitmap = true;
            ScheduleAdjustmentRender();
            InvalidateVisual();
            return;
        }

        if (string.IsNullOrWhiteSpace(SourcePath))
        {
            rawBitmap = null;
            bitmap = null;
            bitmapPath = null;
            bitmapRecipe = EditRecipe.Empty;
            isFullResolutionBitmap = false;
            return;
        }

        if (!IsLoaded)
        {
            return;
        }

        loadCancellation = new CancellationTokenSource();
        LoadAsync(SourcePath, fullResolution, loadCancellation.Token);
    }

    private async void LoadAsync(
        string path,
        bool fullResolution,
        CancellationToken cancellationToken)
    {
        try
        {
            var targetWidth = fullResolution
                ? 0
                : Math.Clamp(
                    (int)Math.Ceiling(Math.Max(ActualWidth, 1280) * 1.5),
                    1280,
                    4096);
            var loaded = await App.Services.Previews.LoadAsync(
                path,
                targetWidth,
                cancellationToken);
            cancellationToken.ThrowIfCancellationRequested();
            if (string.Equals(path, SourcePath, StringComparison.OrdinalIgnoreCase))
            {
                ShowLoadedBitmap(path, loaded, fullResolution);
            }
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception exception)
        {
            error = $"Cannot display this file\n{exception.Message}";
            InvalidateVisual();
        }
    }

    private void CancelLoad()
    {
        loadCancellation?.Cancel();
        loadCancellation?.Dispose();
        loadCancellation = null;
    }

    private void ShowLoadedBitmap(
        string path,
        BitmapSource loaded,
        bool fullResolution)
    {
        var crossfades = bitmap is not null && ShouldCrossfade(bitmapPath, path);
        if (crossfades)
        {
            previousBitmap = bitmap;
            previousBitmapRecipe = bitmapRecipe;
        }

        rawBitmap = loaded;
        bitmap = loaded;
        bitmapPath = path;
        bitmapRecipe = EditRecipe;
        renderedPixelRecipe = null;
        isFullResolutionBitmap = fullResolution;
        if (fullResolution)
        {
            SetActualSize();
        }

        // The freshly decoded frame goes up immediately and the adjusted one
        // replaces it a moment later; waiting for the render would make every
        // photograph feel slow to open.
        ScheduleAdjustmentRender();

        if (!crossfades)
        {
            InvalidateVisual();
            return;
        }

        transitionProgress = 0;
        transitionStartedAt = Stopwatch.GetTimestamp();
        if (!isTransitioning)
        {
            CompositionTarget.Rendering += OnTransitionFrame;
            isTransitioning = true;
        }

        InvalidateVisual();
    }

    internal static bool ShouldCrossfade(
        string? currentPath,
        string loadedPath) =>
        currentPath is not null
        && !string.Equals(
            currentPath,
            loadedPath,
            StringComparison.OrdinalIgnoreCase);

    private void OnTransitionFrame(object? sender, EventArgs eventArgs)
    {
        transitionProgress = Math.Clamp(
            Stopwatch.GetElapsedTime(transitionStartedAt).TotalMilliseconds
            / TransitionDurationMilliseconds,
            0,
            1);
        if (transitionProgress >= 1)
        {
            StopTransition();
        }

        InvalidateVisual();
    }

    private void StopTransition()
    {
        if (isTransitioning)
        {
            CompositionTarget.Rendering -= OnTransitionFrame;
            isTransitioning = false;
        }

        previousBitmap = null;
        previousBitmapRecipe = EditRecipe.Empty;
        transitionProgress = 1;
    }

    private void ResetView()
    {
        zoom = 1;
        pan = default;
        InvalidateVisual();
    }

    private static double SmoothStep(double value) =>
        value * value * (3 - (2 * value));

    private void DrawCenteredText(DrawingContext drawingContext, string text)
    {
        var formatted = new FormattedText(
            text,
            System.Globalization.CultureInfo.CurrentUICulture,
            FlowDirection.LeftToRight,
            new Typeface("Segoe UI"),
            16,
            Brushes.LightGray,
            VisualTreeHelper.GetDpi(this).PixelsPerDip)
        {
            TextAlignment = TextAlignment.Center,
            MaxTextWidth = Math.Max(1, ActualWidth - 48)
        };
        drawingContext.DrawText(
            formatted,
            new Point(
                ActualWidth / 2,
                (ActualHeight - formatted.Height) / 2));
    }
}

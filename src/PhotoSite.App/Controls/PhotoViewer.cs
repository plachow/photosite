using System.Diagnostics;
using System.Windows;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using PhotoSite.Domain;

namespace PhotoSite.Controls;

public sealed class PhotoViewer : FrameworkElement
{
    private const double TransitionDurationMilliseconds = 180;
    private const double ZoomStep = 1.18;
    private const double MinimumZoom = 0.1;
    private const double MaximumActualScale = 16;

    public static readonly DependencyProperty SourcePathProperty =
        DependencyProperty.Register(
            nameof(SourcePath),
            typeof(string),
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

    private BitmapSource? bitmap;
    private BitmapSource? previousBitmap;
    private CancellationTokenSource? loadCancellation;
    private EditRecipe bitmapRecipe = EditRecipe.Empty;
    private EditRecipe previousBitmapRecipe = EditRecipe.Empty;
    private string? bitmapPath;
    private double zoom = 1;
    private double transitionProgress = 1;
    private Vector pan;
    private Point dragOrigin;
    private Vector panOrigin;
    private bool isDragging;
    private bool isFullResolutionBitmap;
    private bool isTransitioning;
    private long transitionStartedAt;
    private string? error;

    public PhotoViewer()
    {
        Focusable = true;
        ClipToBounds = true;
        Loaded += (_, _) => BeginLoad();
        Unloaded += (_, _) =>
        {
            CancelLoad();
            StopTransition();
        };
        SizeChanged += (_, _) =>
        {
            if (bitmap is null)
            {
                BeginLoad();
            }
        };
    }

    public string? SourcePath
    {
        get => (string?)GetValue(SourcePathProperty);
        set => SetValue(SourcePathProperty, value);
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
        if (bitmap is null || string.IsNullOrWhiteSpace(SourcePath))
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
            return;
        }

        DrawBitmap(drawingContext, bitmap, bitmapRecipe, 1);
    }

    private void DrawBitmap(
        DrawingContext drawingContext,
        BitmapSource source,
        EditRecipe recipe,
        double opacity)
    {
        var rotation = (int)recipe.Rotation * 90;
        var fit = GetFitScale(source, recipe);
        var scale = Math.Max(0.0001, fit * zoom);

        var group = new TransformGroup();
        group.Children.Add(
            new ScaleTransform(
                recipe.FlipHorizontal ? -scale : scale,
                scale));
        group.Children.Add(new RotateTransform(rotation));
        group.Children.Add(
            new TranslateTransform(
                (ActualWidth / 2) + pan.X,
                (ActualHeight / 2) + pan.Y));

        drawingContext.PushOpacity(opacity);
        drawingContext.PushTransform(group);
        drawingContext.DrawImage(
            source,
            new Rect(
                -source.PixelWidth / 2d,
                -source.PixelHeight / 2d,
                source.PixelWidth,
                source.PixelHeight));
        drawingContext.Pop();
        drawingContext.Pop();
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
        if (!isDragging)
        {
            return;
        }

        var current = e.GetPosition(this);
        pan = panOrigin + (current - dragOrigin);
        InvalidateVisual();
    }

    protected override void OnMouseLeftButtonUp(MouseButtonEventArgs e)
    {
        base.OnMouseLeftButtonUp(e);
        isDragging = false;
        ReleaseMouseCapture();
        Cursor = Cursors.Arrow;
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
        InvalidateVisual();
    }

    private double GetFitScale(BitmapSource source, EditRecipe recipe)
    {
        var rotation = (int)recipe.Rotation * 90;
        var swapsDimensions = rotation is 90 or 270;
        var displayedWidth = swapsDimensions ? source.PixelHeight : source.PixelWidth;
        var displayedHeight = swapsDimensions ? source.PixelWidth : source.PixelHeight;
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
        viewer.ResetView();
        viewer.BeginLoad();
    }

    private static void OnEditRecipeChanged(
        DependencyObject dependencyObject,
        DependencyPropertyChangedEventArgs eventArgs)
    {
        var viewer = (PhotoViewer)dependencyObject;
        if (string.Equals(
                viewer.bitmapPath,
                viewer.SourcePath,
                StringComparison.OrdinalIgnoreCase))
        {
            viewer.bitmapRecipe = (EditRecipe)eventArgs.NewValue;
        }
    }

    private void BeginLoad(bool fullResolution = false)
    {
        CancelLoad();
        StopTransition();
        error = null;
        InvalidateVisual();

        if (string.IsNullOrWhiteSpace(SourcePath))
        {
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
        if (bitmap is null)
        {
            bitmap = loaded;
            bitmapPath = path;
            bitmapRecipe = EditRecipe;
            isFullResolutionBitmap = fullResolution;
            if (fullResolution)
            {
                SetActualSize();
            }

            InvalidateVisual();
            return;
        }

        previousBitmap = bitmap;
        previousBitmapRecipe = bitmapRecipe;
        bitmap = loaded;
        bitmapPath = path;
        bitmapRecipe = EditRecipe;
        isFullResolutionBitmap = fullResolution;
        if (fullResolution)
        {
            SetActualSize();
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

using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using PhotoSite.Domain;
using PhotoSite.EditorTools;
using PhotoSite.Infrastructure;
using PhotoSite.Services.Imaging;

namespace PhotoSite.Dialogs;

/// <summary>
/// The one window every editor tool opens in. It owns what all tools share
/// - the preset strip, the live preview, the before toggle, zoom, OK and
/// Cancel - and hosts the tool's own settings panel beside the preview.
/// </summary>
/// <remarks>
/// The preview renders the whole recipe with the tool applied through the
/// same renderer the canvas and the exporter use. Fitted, it works on a
/// reduced copy of the photograph; zoomed, it renders only the visible
/// region, at the resolution the zoom needs, so 100 % shows the pixels an
/// export will hold and a slider still answers while it is dragged.
/// </remarks>
public partial class EditToolDialog : Window
{
    /// <summary>
    /// The fitted preview works on a reduced copy: a tool has to react while
    /// a slider is being dragged, and blurring 24 megapixels cannot.
    /// </summary>
    private const int PreviewLongestSide = 1100;

    /// <summary>
    /// Frame pixels rendered beyond the visible region so a blur or a
    /// sharpen sees its neighbours at the edge of the view.
    /// </summary>
    private const int RegionPadding = 32;

    private static readonly double[] ZoomSteps = [0.25, 0.33, 0.5, 0.66, 1, 1.5, 2, 3, 4];

    private static readonly RenderRequest PreviewRequest = new(IncludeLayers: false);

    private readonly EditTool tool;
    private readonly ToolPresetStore? presets;
    private readonly int fullWidth;
    private readonly int fullHeight;
    private readonly EditRecipe baseRecipe;
    private readonly EditToolContext context;
    private readonly DispatcherTimer renderTimer;
    private readonly Func<CancellationToken, Task<BitmapSource>> loadFullSource;
    private readonly Dictionary<int, BitmapSource> scaledSources = [];
    private CancellationTokenSource? renderCancellation;
    private Task<BitmapSource>? fullSourceTask;
    private BitmapSource? displayed;
    private bool isLoadingPreset;
    private bool suppressPresetReset;

    // Zoom state: fitted shows the whole photograph; otherwise `zoom` is
    // screen pixels per finished pixel and `viewCenter` the point of the
    // finished frame, normalized, that sits in the middle of the view.
    private bool isFitted = true;
    private double zoom = 1;
    private Point viewCenter = new(0.5, 0.5);
    private Point? panOrigin;
    private Point panCenterOrigin;
    private bool panMoved;

    /// <param name="source">
    /// The photograph as the canvas holds it, which for a large file is a
    /// reduced decode; <paramref name="fullWidth"/> and
    /// <paramref name="fullHeight"/> state the real pixel size so the output
    /// dimensions the window quotes are the ones an export will produce.
    /// </param>
    /// <param name="loadFullSource">
    /// Decodes the photograph at full size for the zoomed preview; null
    /// means <paramref name="source"/> already is the full photograph.
    /// </param>
    internal EditToolDialog(
        EditTool tool,
        BitmapSource source,
        EditRecipe baseRecipe,
        ToolPresetStore? presets,
        int fullWidth = 0,
        int fullHeight = 0,
        Func<CancellationToken, Task<BitmapSource>>? loadFullSource = null)
    {
        InitializeComponent();
        DarkWindowChrome.Apply(this);

        this.tool = tool;
        this.presets = presets;
        this.baseRecipe = baseRecipe;
        this.fullWidth = fullWidth > 0 ? fullWidth : source.PixelWidth;
        this.fullHeight = fullHeight > 0 ? fullHeight : source.PixelHeight;
        this.loadFullSource = loadFullSource ?? (_ => Task.FromResult(source));
        Title = tool.Title;
        SettingsHeader.Text = tool.Title.ToUpperInvariant();
        HintText.Text = tool.Hint;
        HintText.Visibility = string.IsNullOrEmpty(tool.Hint)
            ? Visibility.Collapsed
            : Visibility.Visible;
        GridButton.Visibility = tool.OffersGrid
            ? Visibility.Visible
            : Visibility.Collapsed;
        if (tool.AcceptsPreviewPick)
        {
            PreviewHost.Cursor = Cursors.Cross;
            PreviewImage.ToolTip = "Click a neutral grey area of the photograph";
        }

        var previewSource = ImageRenderer.Resize(source, PreviewLongestSide);
        context = new EditToolContext(
            previewSource,
            baseRecipe,
            recipe => ImageRenderer.Render(previewSource, recipe, PreviewRequest),
            this.fullWidth,
            this.fullHeight);

        renderTimer = new DispatcherTimer
        {
            Interval = TimeSpan.FromMilliseconds(90)
        };
        renderTimer.Tick += (_, _) =>
        {
            renderTimer.Stop();
            _ = RenderPreviewAsync();
        };

        if (tool.StartsFromRecipe)
        {
            tool.LoadFrom(baseRecipe);
        }

        EditorHost.Content = tool.CreateEditor(context);
        tool.SettingsChanged += OnToolSettingsChanged;

        Loaded += async (_, _) =>
        {
            await LoadPresetsAsync();
            UpdateZoomLabel();
            ScheduleRender();
        };
        Closed += (_, _) =>
        {
            renderTimer.Stop();
            renderCancellation?.Cancel();
            renderCancellation?.Dispose();
            tool.SettingsChanged -= OnToolSettingsChanged;
        };
    }

    /// <summary>The recipe the user confirmed, or null when cancelled.</summary>
    public EditRecipe? Result { get; private set; }

    /// <summary>The tool this window is showing, for tests and status text.</summary>
    internal EditTool Tool => tool;

    internal FrameworkElement? EditorContent => EditorHost.Content as FrameworkElement;

    internal int PresetCount => PresetBox.Items.Count;

    /// <summary>Screen pixels per finished pixel as shown right now.</summary>
    internal double CurrentZoom => isFitted ? FitZoom() : zoom;

    internal bool IsFitted => isFitted;

    private enum PresetKind
    {
        Default,
        LastUsed,
        Saved
    }

    private sealed record PresetEntry(string Name, PresetKind Kind, string? Payload)
    {
        // The themed combo shows its selected item through ToString rather
        // than DisplayMemberPath.
        public override string ToString() => Name;
    }

    private void OnToolSettingsChanged(object? sender, EventArgs eventArgs)
    {
        if (!isLoadingPreset && !suppressPresetReset)
        {
            // A hand-moved slider is no longer the preset it started from.
            suppressPresetReset = true;
            PresetBox.SelectedIndex = -1;
            suppressPresetReset = false;
            DeletePresetButton.IsEnabled = false;
        }

        ScheduleRender();
    }

    private async Task LoadPresetsAsync(string? select = null)
    {
        var entries = new List<PresetEntry>
        {
            new("<Default>", PresetKind.Default, null)
        };

        if (presets is not null)
        {
            try
            {
                if (await presets.LoadLastUsedAsync(tool.Id) is { Length: > 0 } lastUsed)
                {
                    entries.Add(new PresetEntry("<Last used>", PresetKind.LastUsed, lastUsed));
                }

                foreach (var (name, payload) in await presets.LoadAsync(tool.Id))
                {
                    entries.Add(new PresetEntry(name, PresetKind.Saved, payload));
                }
            }
            catch (Exception exception)
            {
                FooterText.Text = $"Presets could not be loaded: {exception.Message}";
            }
        }

        suppressPresetReset = true;
        try
        {
            PresetBox.ItemsSource = entries;
            if (select is not null)
            {
                PresetBox.SelectedItem = entries.FirstOrDefault(entry =>
                    entry.Kind == PresetKind.Saved
                    && string.Equals(entry.Name, select, StringComparison.OrdinalIgnoreCase));
            }
            else if (!tool.StartsFromRecipe)
            {
                // A filter starts from what was used last time, which is
                // what makes "sharpen it like the previous one" one click.
                var lastUsed = entries.FirstOrDefault(entry => entry.Kind == PresetKind.LastUsed);
                if (lastUsed is not null)
                {
                    ApplyPreset(lastUsed);
                    PresetBox.SelectedItem = lastUsed;
                }
            }

            if (PresetBox.SelectedItem is null && tool.IsAtDefaults)
            {
                PresetBox.SelectedIndex = 0;
            }
        }
        finally
        {
            suppressPresetReset = false;
        }
    }

    private void OnPresetSelectionChanged(
        object sender,
        SelectionChangedEventArgs eventArgs)
    {
        DeletePresetButton.IsEnabled =
            PresetBox.SelectedItem is PresetEntry { Kind: PresetKind.Saved };
        if (suppressPresetReset || PresetBox.SelectedItem is not PresetEntry entry)
        {
            return;
        }

        ApplyPreset(entry);
    }

    private void ApplyPreset(PresetEntry entry)
    {
        isLoadingPreset = true;
        try
        {
            if (entry.Payload is null)
            {
                tool.Reset();
            }
            else if (!tool.TryDeserialize(entry.Payload))
            {
                FooterText.Text = $"The preset “{entry.Name}” could not be read.";
            }
        }
        finally
        {
            isLoadingPreset = false;
        }
    }

    private async void OnSavePresetClick(object sender, RoutedEventArgs eventArgs)
    {
        if (presets is null)
        {
            return;
        }

        var suggested = PresetBox.SelectedItem is PresetEntry { Kind: PresetKind.Saved } selected
            ? selected.Name
            : "My preset";
        var dialog = new TextPromptDialog("Save preset", "Preset name", suggested)
        {
            Owner = this
        };
        if (dialog.ShowDialog() != true || string.IsNullOrWhiteSpace(dialog.Value))
        {
            return;
        }

        var name = dialog.Value.Trim();
        try
        {
            await presets.SaveAsync(tool.Id, name, tool.Serialize());
            await LoadPresetsAsync(select: name);
            FooterText.Text = $"Preset “{name}” saved";
        }
        catch (Exception exception)
        {
            FooterText.Text = $"The preset could not be saved: {exception.Message}";
        }
    }

    private async void OnDeletePresetClick(object sender, RoutedEventArgs eventArgs)
    {
        if (presets is null
            || PresetBox.SelectedItem is not PresetEntry { Kind: PresetKind.Saved } entry)
        {
            return;
        }

        if (MessageBox.Show(
                this,
                $"Delete the preset “{entry.Name}”?",
                "Delete preset",
                MessageBoxButton.YesNo,
                MessageBoxImage.Question) != MessageBoxResult.Yes)
        {
            return;
        }

        try
        {
            await presets.DeleteAsync(tool.Id, entry.Name);
            await LoadPresetsAsync();
            FooterText.Text = $"Preset “{entry.Name}” deleted";
        }
        catch (Exception exception)
        {
            FooterText.Text = $"The preset could not be deleted: {exception.Message}";
        }
    }

    private void OnResetClick(object sender, RoutedEventArgs eventArgs)
    {
        suppressPresetReset = true;
        try
        {
            PresetBox.SelectedIndex = 0;
        }
        finally
        {
            suppressPresetReset = false;
        }

        ApplyPreset(new PresetEntry("<Default>", PresetKind.Default, null));
    }

    private void OnLivePreviewChanged(object sender, RoutedEventArgs eventArgs) =>
        ScheduleRender();

    private void OnBeforeToggled(object sender, RoutedEventArgs eventArgs) =>
        ScheduleRender();

    private void OnGridToggled(object sender, RoutedEventArgs eventArgs) =>
        GridOverlay.Visibility = GridButton.IsChecked == true
            ? Visibility.Visible
            : Visibility.Collapsed;

    // ----- Zoom -----------------------------------------------------------

    /// <summary>The size of the finished image before its frame, in pixels.</summary>
    private (int Width, int Height) OutputSize(EditRecipe recipe)
    {
        var (frameWidth, frameHeight) = ImageRenderer.MeasureFrame(fullWidth, fullHeight, recipe);
        return recipe.MeasureResize(frameWidth, frameHeight);
    }

    private double DpiScale => VisualTreeHelper.GetDpi(this).DpiScaleX;

    private (double Width, double Height) ViewportDevicePixels()
    {
        var dpi = DpiScale;
        return (
            Math.Max(1, (PreviewHost.ActualWidth - 2) * dpi),
            Math.Max(1, (PreviewHost.ActualHeight - 2) * dpi));
    }

    /// <summary>The zoom at which the whole finished image fits the view.</summary>
    private double FitZoom()
    {
        var (outputWidth, outputHeight) = OutputSize(baseRecipe);
        var (viewWidth, viewHeight) = ViewportDevicePixels();
        return Math.Max(
            0.001,
            Math.Min(viewWidth / Math.Max(1, outputWidth), viewHeight / Math.Max(1, outputHeight)));
    }

    private void OnZoomInClick(object sender, RoutedEventArgs eventArgs) =>
        StepZoom(+1, null);

    private void OnZoomOutClick(object sender, RoutedEventArgs eventArgs) =>
        StepZoom(-1, null);

    private void OnZoomActualClick(object sender, RoutedEventArgs eventArgs) =>
        SetZoom(1, null);

    private void OnZoomFitClick(object sender, RoutedEventArgs eventArgs) =>
        SetFitted();

    private void OnPreviewHostSizeChanged(object sender, SizeChangedEventArgs eventArgs)
    {
        UpdateZoomLabel();
        if (!isFitted)
        {
            ClampViewCenter();
            ScheduleRender();
        }
    }

    private void OnPreviewMouseWheel(object sender, MouseWheelEventArgs eventArgs)
    {
        StepZoom(eventArgs.Delta > 0 ? +1 : -1, eventArgs.GetPosition(PreviewHost));
        eventArgs.Handled = true;
    }

    /// <summary>
    /// Moves one step up or down the zoom ladder from what is shown now;
    /// stepping below the fit lands on fit.
    /// </summary>
    internal void StepZoom(int direction, Point? anchor)
    {
        var current = CurrentZoom;
        var fit = FitZoom();
        if (direction > 0)
        {
            var next = ZoomSteps.FirstOrDefault(step => step > current * 1.001);
            if (next <= 0)
            {
                return;
            }

            SetZoom(Math.Max(next, fit), anchor);
            return;
        }

        var previous = ZoomSteps.LastOrDefault(step => step < current * 0.999);
        if (previous <= 0 || previous <= fit)
        {
            SetFitted();
            return;
        }

        SetZoom(previous, anchor);
    }

    internal void SetFitted()
    {
        if (isFitted)
        {
            return;
        }

        isFitted = true;
        viewCenter = new Point(0.5, 0.5);
        PreviewImage.Stretch = Stretch.Uniform;
        PreviewImage.Width = double.NaN;
        PreviewImage.Height = double.NaN;
        RenderOptions.SetBitmapScalingMode(PreviewImage, BitmapScalingMode.HighQuality);
        UpdateZoomLabel();
        ScheduleRender();
    }

    /// <summary>
    /// Sets an explicit zoom, keeping the finished point under
    /// <paramref name="anchor"/> (a position in the view) where it is, or
    /// the centre when there is no anchor.
    /// </summary>
    internal void SetZoom(double target, Point? anchor)
    {
        var fit = FitZoom();
        if (target <= fit)
        {
            SetFitted();
            return;
        }

        var (outputWidth, outputHeight) = OutputSize(baseRecipe);
        var previous = CurrentZoom;
        var (viewWidth, viewHeight) = ViewportDevicePixels();
        var dpi = DpiScale;
        if (anchor is { } point)
        {
            // Offset of the anchor from the view centre, in device pixels.
            var offsetX = ((point.X - 1) * dpi) - (viewWidth / 2);
            var offsetY = ((point.Y - 1) * dpi) - (viewHeight / 2);
            var anchoredX = (viewCenter.X * outputWidth) + (offsetX / previous);
            var anchoredY = (viewCenter.Y * outputHeight) + (offsetY / previous);
            viewCenter = new Point(
                (anchoredX - (offsetX / target)) / Math.Max(1, outputWidth),
                (anchoredY - (offsetY / target)) / Math.Max(1, outputHeight));
        }

        isFitted = false;
        zoom = target;
        ClampViewCenter();
        UpdateZoomLabel();
        ScheduleRender();
    }

    /// <summary>
    /// Keeps the view inside the finished image: a dimension smaller than
    /// the view is centred, a larger one never scrolls past its edge.
    /// </summary>
    private void ClampViewCenter()
    {
        var (outputWidth, outputHeight) = OutputSize(baseRecipe);
        var (viewWidth, viewHeight) = ViewportDevicePixels();
        var halfWidth = viewWidth / zoom / 2 / Math.Max(1, outputWidth);
        var halfHeight = viewHeight / zoom / 2 / Math.Max(1, outputHeight);
        viewCenter = new Point(
            halfWidth >= 0.5 ? 0.5 : Math.Clamp(viewCenter.X, halfWidth, 1 - halfWidth),
            halfHeight >= 0.5 ? 0.5 : Math.Clamp(viewCenter.Y, halfHeight, 1 - halfHeight));
    }

    private void UpdateZoomLabel() =>
        ZoomLabel.Text = isFitted
            ? $"Fit · {FitZoom() * 100:0} %"
            : $"{zoom * 100:0} %";

    private void OnPreviewMouseDown(object sender, MouseButtonEventArgs eventArgs)
    {
        panOrigin = eventArgs.GetPosition(PreviewHost);
        panCenterOrigin = viewCenter;
        panMoved = false;
        PreviewHost.CaptureMouse();
        eventArgs.Handled = true;
    }

    private void OnPreviewMouseMove(object sender, MouseEventArgs eventArgs)
    {
        if (panOrigin is not { } origin || !PreviewHost.IsMouseCaptured)
        {
            return;
        }

        var position = eventArgs.GetPosition(PreviewHost);
        var delta = position - origin;
        if (!panMoved && delta.Length < 3)
        {
            return;
        }

        panMoved = true;
        if (isFitted)
        {
            return;
        }

        var (outputWidth, outputHeight) = OutputSize(baseRecipe);
        var dpi = DpiScale;
        viewCenter = new Point(
            panCenterOrigin.X - (delta.X * dpi / zoom / Math.Max(1, outputWidth)),
            panCenterOrigin.Y - (delta.Y * dpi / zoom / Math.Max(1, outputHeight)));
        ClampViewCenter();
        ScheduleRender();
    }

    private void OnPreviewMouseUp(object sender, MouseButtonEventArgs eventArgs)
    {
        if (panOrigin is null)
        {
            return;
        }

        panOrigin = null;
        if (PreviewHost.IsMouseCaptured)
        {
            PreviewHost.ReleaseMouseCapture();
        }

        if (panMoved || !tool.AcceptsPreviewPick || displayed is null)
        {
            return;
        }

        // A click without a drag is a pick: where on the shown bitmap.
        var position = eventArgs.GetPosition(PreviewImage);
        if (PreviewImage.ActualWidth <= 0 || PreviewImage.ActualHeight <= 0)
        {
            return;
        }

        tool.OnPreviewPicked(
            context,
            Math.Clamp(position.X / PreviewImage.ActualWidth, 0, 1),
            Math.Clamp(position.Y / PreviewImage.ActualHeight, 0, 1),
            displayed);
        eventArgs.Handled = true;
    }

    // ----- Rendering ------------------------------------------------------

    private void ScheduleRender()
    {
        renderTimer.Stop();
        renderTimer.Start();
    }

    private async Task RenderPreviewAsync()
    {
        renderCancellation?.Cancel();
        renderCancellation?.Dispose();
        renderCancellation = new CancellationTokenSource();
        var token = renderCancellation.Token;

        var showBase = BeforeButton.IsChecked == true || LivePreviewBox.IsChecked != true;
        PreviewBadge.Text = showBase ? "BEFORE" : "AFTER";
        var recipe = showBase ? baseRecipe : tool.Apply(baseRecipe);
        var (outputWidth, outputHeight) = ImageRenderer.MeasureOutput(
            fullWidth,
            fullHeight,
            recipe);

        PreviewStatus.Text = "Rendering…";
        try
        {
            BitmapSource rendered;
            if (isFitted)
            {
                rendered = await Task.Run(
                    () => showBase
                        ? context.BaseRender
                        : ImageRenderer.Render(
                            context.Original,
                            recipe,
                            PreviewRequest,
                            token),
                    token);
            }
            else
            {
                rendered = await RenderZoomedAsync(recipe, token);
            }

            if (token.IsCancellationRequested)
            {
                return;
            }

            displayed = rendered;
            PreviewImage.Source = rendered;
            PreviewStatus.Text = $"{outputWidth} × {outputHeight} px";
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception exception)
        {
            PreviewStatus.Text = exception.Message;
        }
    }

    /// <summary>
    /// Renders just the part of the finished image the view shows, from the
    /// photograph decoded at the resolution the zoom calls for: the full
    /// file at 100 % and above, a reduced copy below. The recipe's frame
    /// and resize are left out - the frame belongs to the fitted view, the
    /// resize is folded into the zoom.
    /// </summary>
    private async Task<BitmapSource> RenderZoomedAsync(
        EditRecipe recipe,
        CancellationToken token)
    {
        var full = await GetFullSourceAsync(token);
        token.ThrowIfCancellationRequested();

        // Everything below is measured on the file's own pixels, which for a
        // photograph opened reduced may differ from the catalogued size.
        var (frameWidth, frameHeight) = ImageRenderer.MeasureFrame(
            full.PixelWidth,
            full.PixelHeight,
            recipe);
        var (outputWidth, _) = recipe.MeasureResize(frameWidth, frameHeight);
        var resizeScale = outputWidth / (double)Math.Max(1, frameWidth);
        // Screen pixels per frame pixel.
        var frameZoom = zoom * resizeScale;
        var (viewWidth, viewHeight) = ViewportDevicePixels();

        // The visible rectangle of the frame, in frame pixels.
        var visibleWidth = Math.Min(frameWidth, viewWidth / frameZoom);
        var visibleHeight = Math.Min(frameHeight, viewHeight / frameZoom);
        var left = Math.Clamp((viewCenter.X * frameWidth) - (visibleWidth / 2), 0, frameWidth - visibleWidth);
        var top = Math.Clamp((viewCenter.Y * frameHeight) - (visibleHeight / 2), 0, frameHeight - visibleHeight);
        var visible = new Rect(left, top, visibleWidth, visibleHeight);
        var padded = visible;
        padded.Inflate(RegionPadding, RegionPadding);
        padded.Intersect(new Rect(0, 0, frameWidth, frameHeight));

        // Below 100 % the render can come from a reduced copy; above it the
        // pixels have to be the real ones.
        var sourceScale = Math.Min(1, frameZoom);
        var longest = (int)Math.Round(Math.Max(full.PixelWidth, full.PixelHeight) * sourceScale);
        var region = MapFrameRectToSource(padded, frameWidth, frameHeight, recipe);
        var withoutOutputWork = recipe with { OutputWidth = 0, OutputHeight = 0, Frame = null };

        var rendered = await Task.Run(
            () =>
            {
                var source = GetScaledSource(full, longest);
                return ImageRenderer.Render(
                    source,
                    withoutOutputWork,
                    new RenderRequest(
                        RegionOverride: region,
                        IncludeLayers: false,
                        IncludeFrame: false),
                    token);
            },
            token);

        // Cut the padding back off; the rendered size follows the region's
        // pixel rounding, so the cut is measured against it.
        var scaleX = rendered.PixelWidth / padded.Width;
        var scaleY = rendered.PixelHeight / padded.Height;
        var cut = new Int32Rect(
            (int)Math.Round((visible.X - padded.X) * scaleX),
            (int)Math.Round((visible.Y - padded.Y) * scaleY),
            Math.Max(1, (int)Math.Round(visible.Width * scaleX)),
            Math.Max(1, (int)Math.Round(visible.Height * scaleY)));
        cut.Width = Math.Max(1, Math.Min(cut.Width, rendered.PixelWidth - cut.X));
        cut.Height = Math.Max(1, Math.Min(cut.Height, rendered.PixelHeight - cut.Y));
        var shown = cut.X == 0 && cut.Y == 0
                    && cut.Width == rendered.PixelWidth
                    && cut.Height == rendered.PixelHeight
            ? rendered
            : Freeze(new CroppedBitmap(rendered, cut));

        // Sized so that one finished pixel is `zoom` device pixels, which
        // at 100 % is exactly one.
        var dpi = DpiScale;
        PreviewImage.Stretch = Stretch.Fill;
        PreviewImage.Width = visible.Width * frameZoom / dpi;
        PreviewImage.Height = visible.Height * frameZoom / dpi;
        RenderOptions.SetBitmapScalingMode(
            PreviewImage,
            frameZoom >= 2 ? BitmapScalingMode.NearestNeighbor : BitmapScalingMode.HighQuality);
        return shown;
    }

    private static BitmapSource Freeze(BitmapSource bitmap)
    {
        if (!bitmap.IsFrozen && bitmap.CanFreeze)
        {
            bitmap.Freeze();
        }

        return bitmap;
    }

    private Task<BitmapSource> GetFullSourceAsync(CancellationToken token)
    {
        if (fullSourceTask is null || fullSourceTask.IsFaulted)
        {
            PreviewStatus.Text = "Loading full resolution…";
            fullSourceTask = LoadFullAsync();
        }

        return fullSourceTask.WaitAsync(token);
    }

    private async Task<BitmapSource> LoadFullAsync()
    {
        var loaded = await loadFullSource(CancellationToken.None);
        // Materialized as straight BGRA once, so every region render copies
        // pixels instead of converting the whole photograph again.
        return await Task.Run(() => PixelBuffer.FromBitmap(loaded).ToBitmap());
    }

    /// <summary>
    /// The photograph reduced to <paramref name="longest"/> pixels on its
    /// long side, kept per size so a zoom level renders from the same copy
    /// on every slider move.
    /// </summary>
    private BitmapSource GetScaledSource(BitmapSource full, int longest)
    {
        if (longest >= Math.Max(full.PixelWidth, full.PixelHeight))
        {
            return full;
        }

        lock (scaledSources)
        {
            if (scaledSources.TryGetValue(longest, out var cached))
            {
                return cached;
            }

            if (scaledSources.Count >= 3)
            {
                scaledSources.Clear();
            }

            var scaled = PixelBuffer.FromBitmap(ImageRenderer.Resize(full, longest)).ToBitmap();
            scaledSources[longest] = scaled;
            return scaled;
        }
    }

    /// <summary>
    /// Maps a rectangle of the finished frame (cropped and oriented) back to
    /// the normalized source region that renders it: the orientation is
    /// undone corner by corner, then the crop offset put back.
    /// </summary>
    internal static CropRegion MapFrameRectToSource(
        Rect frameRect,
        double frameWidth,
        double frameHeight,
        EditRecipe recipe)
    {
        var crop = recipe.Crop?.ConstrainToUnit() is { IsEmpty: false } region
            ? region
            : CropRegion.Full;
        var (firstU, firstV) = UnOrient(
            frameRect.Left / frameWidth,
            frameRect.Top / frameHeight,
            recipe);
        var (secondU, secondV) = UnOrient(
            frameRect.Right / frameWidth,
            frameRect.Bottom / frameHeight,
            recipe);
        return CropRegion.FromPoints(
            crop.X + (Math.Min(firstU, secondU) * crop.Width),
            crop.Y + (Math.Min(firstV, secondV) * crop.Height),
            crop.X + (Math.Max(firstU, secondU) * crop.Width),
            crop.Y + (Math.Max(firstV, secondV) * crop.Height))
            .ConstrainToUnit();
    }

    /// <summary>
    /// The inverse of <see cref="GeometryProcessor.Orient"/> for one point:
    /// the rotation comes off first, then the flips, both in normalized
    /// coordinates of the crop.
    /// </summary>
    private static (double U, double V) UnOrient(double x, double y, EditRecipe recipe)
    {
        var (u, v) = recipe.Rotation switch
        {
            QuarterRotation.Clockwise90 => (y, 1 - x),
            QuarterRotation.Clockwise180 => (1 - x, 1 - y),
            QuarterRotation.Clockwise270 => (1 - y, x),
            _ => (x, y)
        };
        if (recipe.FlipHorizontal)
        {
            u = 1 - u;
        }

        if (recipe.FlipVertical)
        {
            v = 1 - v;
        }

        return (u, v);
    }

    private void OnOkClick(object sender, RoutedEventArgs eventArgs)
    {
        Result = tool.Apply(baseRecipe);
        if (presets is { } store)
        {
            // Remembering the settings is a convenience that must neither
            // delay the edit nor fail it, so it is not awaited.
            _ = RememberLastUsedAsync(store, tool.Id, tool.Serialize());
        }

        DialogResult = true;
    }

    private static async Task RememberLastUsedAsync(
        ToolPresetStore store,
        string toolId,
        string payload)
    {
        try
        {
            await store.SaveLastUsedAsync(toolId, payload);
        }
        catch
        {
            // A failed write here loses nothing but the next opening's
            // starting point.
        }
    }
}

using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using PhotoSite.Controls;
using PhotoSite.Dialogs;
using PhotoSite.EditorTools;

namespace PhotoSite;

/// <summary>
/// The editor menu: what each entry does, which shortcut opens it, and the
/// one routine that opens any tool in the shared tool window and applies
/// its result as a single undo step.
/// </summary>
public partial class MainWindow
{
    /// <summary>
    /// Every tool the menu can open, by the id the menu items carry in their
    /// Tag. Tools are created fresh per opening so no settings leak from
    /// one photo to the next except through the last-used preset.
    /// </summary>
    private static readonly Dictionary<string, Func<EditTool>> ToolFactories =
        new(StringComparer.Ordinal)
        {
            ["levels"] = () => new LevelsTool(),
            ["curves"] = () => new CurvesTool(),
            ["exposure"] = () => new ExposureTool(),
            ["colors"] = () => new ColorsTool(),
            ["white-balance"] = () => new WhiteBalanceTool(),
            ["shadows"] = () => new ShadowsTool(),
            ["combined"] = () => new CombinedTool(),
            ["old-photo"] = () => new OldPhotoTool(),
            ["sharpen"] = () => new SharpenTool(),
            ["blur"] = () => new BlurTool(),
            ["noise-reduction"] = () => new NoiseReductionTool(),
            ["chromatic-aberration"] = () => new ChromaticAberrationTool(),
            ["distortion"] = () => new DistortionTool(),
            ["straighten"] = () => new StraightenTool(),
            ["resize"] = () => new ResizeTool(),
            ["filter.vignette"] = EditorToolCatalog.Vignette,
            ["filter.deinterlace"] = EditorToolCatalog.Deinterlace,
            ["filter.addnoise"] = EditorToolCatalog.AddNoise,
            ["filter.grayscale"] = EditorToolCatalog.Grayscale,
            ["filter.sepia"] = EditorToolCatalog.Sepia,
            ["filter.invert"] = EditorToolCatalog.Invert,
            ["filter.posterize"] = EditorToolCatalog.Posterize,
            ["filter.solarize"] = EditorToolCatalog.Solarize,
            ["filter.pixelize"] = EditorToolCatalog.Pixelize
        };

    /// <summary>
    /// The keyboard shortcuts of the tool entries, matching the gesture text
    /// the menu shows. Plain digits are ratings, so the tools take Ctrl and
    /// Shift combinations.
    /// </summary>
    private static readonly (Key Key, ModifierKeys Modifiers, string ToolId)[] ToolShortcuts =
    [
        (Key.L, ModifierKeys.Shift, "levels"),
        (Key.C, ModifierKeys.Shift, "curves"),
        (Key.E, ModifierKeys.Shift, "resize"),
        (Key.D1, ModifierKeys.Control, "exposure"),
        (Key.D2, ModifierKeys.Control, "colors"),
        (Key.D3, ModifierKeys.Control, "white-balance"),
        (Key.D4, ModifierKeys.Control, "combined"),
        (Key.D5, ModifierKeys.Control, "sharpen"),
        (Key.D6, ModifierKeys.Control, "blur"),
        (Key.D7, ModifierKeys.Control, "shadows"),
        (Key.R, ModifierKeys.Control | ModifierKeys.Shift, "noise-reduction"),
        (Key.A, ModifierKeys.Control | ModifierKeys.Shift, "chromatic-aberration"),
        (Key.D, ModifierKeys.Control | ModifierKeys.Shift, "distortion"),
        (Key.V, ModifierKeys.Control | ModifierKeys.Shift, "filter.vignette"),
        (Key.L, ModifierKeys.Control | ModifierKeys.Shift, "filter.deinterlace"),
        (Key.N, ModifierKeys.Control | ModifierKeys.Shift, "filter.addnoise")
    ];

    internal static IReadOnlyCollection<string> ToolIds => ToolFactories.Keys;

    internal static EditTool CreateTool(string toolId) =>
        ToolFactories.TryGetValue(toolId, out var factory)
            ? factory()
            : throw new ArgumentException($"Unknown editor tool '{toolId}'.", nameof(toolId));

    internal static string? TryGetToolShortcut(Key key, ModifierKeys modifiers)
    {
        foreach (var (shortcutKey, shortcutModifiers, toolId) in ToolShortcuts)
        {
            if (shortcutKey == key && shortcutModifiers == modifiers)
            {
                return toolId;
            }
        }

        return null;
    }

    internal static bool IsAutoEnhanceShortcut(Key key, ModifierKeys modifiers) =>
        key == Key.D0 && modifiers == ModifierKeys.Control;

    internal static bool IsRotateLeftShortcut(Key key, ModifierKeys modifiers) =>
        key == Key.L && modifiers == ModifierKeys.Control;

    internal static bool IsRotateRightShortcut(Key key, ModifierKeys modifiers) =>
        key == Key.R && modifiers == ModifierKeys.Control;

    private void OnToolMenuClick(object sender, RoutedEventArgs eventArgs)
    {
        if (sender is MenuItem { Tag: string toolId })
        {
            OpenTool(toolId);
        }
    }

    /// <summary>
    /// Opens a tool over the current photo and, on OK, applies the recipe
    /// it produced as one edit - so Ctrl+Z takes the whole tool back.
    /// </summary>
    private void OpenTool(string toolId)
    {
        if (!viewModel.IsEditorMode
            || viewModel.SelectedPhoto is not { } photo
            || PreviewViewer.OriginalBitmap is not { } source)
        {
            viewModel.ReportStatus("Open a photograph in the editor first");
            return;
        }

        var tool = CreateTool(toolId);
        var dialog = new EditToolDialog(
            tool,
            source,
            photo.EditRecipe,
            App.Services.ToolPresets)
        {
            Owner = this
        };
        if (dialog.ShowDialog() != true || dialog.Result is not { } recipe)
        {
            return;
        }

        if (recipe == photo.EditRecipe)
        {
            viewModel.ReportStatus($"{tool.Title}: nothing changed");
            return;
        }

        photo.ApplyEdit(recipe);
        // The adjustment panel reads the recipe, so a tool that wrote to it
        // has to tell the sliders to look again.
        photo.Adjustments.NotifyAll();
        viewModel.ReportStatus($"Applied {tool.Title} · Ctrl+Z takes it back");
    }

    private void OnMenuCloseEditorClick(object sender, RoutedEventArgs eventArgs) =>
        _ = LeaveEditorAsync();

    private async void OnMenuCopyClick(object sender, RoutedEventArgs eventArgs) =>
        await CopySelectionOrImageAsync();

    /// <summary>
    /// Ctrl+C in the editor: the selection when there is one, otherwise the
    /// whole finished image.
    /// </summary>
    private async Task CopySelectionOrImageAsync()
    {
        if (PreviewViewer.HasSelection)
        {
            await CopySelectionAsync();
            return;
        }

        try
        {
            viewModel.ReportStatus("Copying image…");
            var size = await PreviewViewer.CopyImageToClipboardAsync();
            if (size.Width > 0)
            {
                viewModel.ReportStatus($"Copied {size.Width:N0} × {size.Height:N0} pixels");
            }
        }
        catch (Exception exception)
        {
            viewModel.ReportStatus($"Cannot copy image: {exception.Message}");
        }
    }

    private async void OnMenuPasteClick(object sender, RoutedEventArgs eventArgs) =>
        await PasteImageFromClipboardAsync();

    private void OnMenuInsertImageClick(object sender, RoutedEventArgs eventArgs)
    {
        if (viewModel.SelectedPhoto is not { } photo
            || PreviewViewer.OriginalBitmap is not { } source)
        {
            return;
        }

        var dialog = new Microsoft.Win32.OpenFileDialog
        {
            Title = "Image from file",
            Filter = "Images|*.png;*.jpg;*.jpeg;*.webp;*.bmp;*.tif;*.tiff;*.gif|All files|*.*",
            CheckFileExists = true
        };
        if (dialog.ShowDialog(this) != true)
        {
            return;
        }

        var layer = CreateImageLayer(
            dialog.FileName,
            source.PixelWidth,
            source.PixelHeight,
            photo.EditRecipe);
        if (layer is null)
        {
            viewModel.ReportStatus(
                $"{System.IO.Path.GetFileName(dialog.FileName)} could not be read as an image");
            return;
        }

        photo.SetLayers(photo.EditRecipe.WithLayer(layer).Layers);
        PreviewViewer.SelectedLayerId = layer.Id;
        SetAnnotationTool(AnnotationTool.Select);
        RefreshLayerList();
        viewModel.ReportStatus(
            $"{layer.Name} placed as a layer · drag it, resize by its corners");
    }

    /// <summary>
    /// A layer for an image file, a third of the frame wide and centred,
    /// in the normalized space of the finished image so it keeps its own
    /// proportions whatever the photograph's are.
    /// </summary>
    internal static Domain.ImageLayer? CreateImageLayer(
        string path,
        int sourceWidth,
        int sourceHeight,
        Domain.EditRecipe recipe)
    {
        if (Services.Imaging.ImageLayerCache.Measure(path) is not { } size
            || size.Width <= 0
            || size.Height <= 0)
        {
            return null;
        }

        var (frameWidth, frameHeight) = Services.Imaging.ImageRenderer.MeasureFrame(
            sourceWidth,
            sourceHeight,
            recipe);
        var width = 0.35;
        var pixelWidth = width * Math.Max(1, frameWidth);
        var pixelHeight = pixelWidth * size.Height / size.Width;
        var height = pixelHeight / Math.Max(1, frameHeight);
        if (height > 0.8)
        {
            width *= 0.8 / height;
            height = 0.8;
        }

        return new Domain.ImageLayer
        {
            Name = System.IO.Path.GetFileName(path),
            Path = path,
            X = 0.5 - (width / 2),
            Y = 0.5 - (height / 2),
            Width = width,
            Height = height
        };
    }

    private void OnMenuRotate180Click(object sender, RoutedEventArgs eventArgs)
    {
        if (viewModel.SelectedPhoto is not { } photo)
        {
            return;
        }

        photo.ApplyEdit(photo.EditRecipe.RotateClockwise().RotateClockwise());
    }

    private void OnMenuAutoEnhanceClick(object sender, RoutedEventArgs eventArgs) =>
        OnAutoFixRequested(sender, EventArgs.Empty);

    private void OnMenuAutoWhiteBalanceClick(object sender, RoutedEventArgs eventArgs) =>
        OnAutoWhiteBalanceRequested(sender, EventArgs.Empty);

    private void OnMenuAnnotationToolClick(object sender, RoutedEventArgs eventArgs)
    {
        if (sender is MenuItem { Tag: string name }
            && Enum.TryParse<AnnotationTool>(name, out var tool))
        {
            SetAnnotationTool(tool);
        }
    }

    private void OnMenuBeforeClick(object sender, RoutedEventArgs eventArgs)
    {
        BeforeAfterButton.IsChecked = BeforeAfterButton.IsChecked != true;
        OnBeforeAfterChanged(BeforeAfterButton, new RoutedEventArgs());
    }

    private void OnMenuSplitClick(object sender, RoutedEventArgs eventArgs)
    {
        SplitCompareButton.IsChecked = SplitCompareButton.IsChecked != true;
        if (SplitCompareButton.IsChecked == true)
        {
            BeforeAfterButton.IsChecked = true;
        }

        OnBeforeAfterChanged(SplitCompareButton, new RoutedEventArgs());
    }

    private void OnMenuFacesClick(object sender, RoutedEventArgs eventArgs) =>
        FacesOverlayButton.IsChecked = FacesOverlayButton.IsChecked != true;

    private void OnMenuInfoClick(object sender, RoutedEventArgs eventArgs) =>
        MetadataPanelButton.IsChecked = MetadataPanelButton.IsChecked != true;

    private void OnMenuFitClick(object sender, RoutedEventArgs eventArgs) =>
        PreviewViewer.FitToViewport();

    private void OnMenuZoomInClick(object sender, RoutedEventArgs eventArgs) =>
        PreviewViewer.ZoomIn();

    private void OnMenuZoomOutClick(object sender, RoutedEventArgs eventArgs) =>
        PreviewViewer.ZoomOut();

    private void OnMenuActualSizeClick(object sender, RoutedEventArgs eventArgs) =>
        PreviewViewer.ShowActualSize();

    /// <summary>
    /// The editor-only shortcuts the menu advertises, tried before the older
    /// single-key ones so that Shift+L opens Levels rather than arming the
    /// line tool.
    /// </summary>
    private bool TryHandleMenuShortcut(Key key, ModifierKeys modifiers)
    {
        if (!viewModel.IsEditorMode || viewModel.IsFullscreenMode)
        {
            return false;
        }

        if (TryGetToolShortcut(key, modifiers) is { } toolId)
        {
            OpenTool(toolId);
            return true;
        }

        if (IsAutoEnhanceShortcut(key, modifiers))
        {
            OnAutoFixRequested(this, EventArgs.Empty);
            return true;
        }

        if (key == Key.C && modifiers == ModifierKeys.Control && !PreviewViewer.HasSelection)
        {
            _ = CopySelectionOrImageAsync();
            return true;
        }

        if (viewModel.SelectedPhoto is { } photo)
        {
            if (IsRotateLeftShortcut(key, modifiers))
            {
                photo.RotateLeftCommand.Execute(null);
                return true;
            }

            if (IsRotateRightShortcut(key, modifiers))
            {
                photo.RotateRightCommand.Execute(null);
                return true;
            }
        }

        return false;
    }
}

public partial class MainWindow
{
    internal void ValidateEditorMenuForSmokeTest()
    {
        if (Content is not UIElement content)
        {
            throw new InvalidOperationException("The main window has no content.");
        }

        viewModel.ShowEditorCommand.Execute(null);
        content.Measure(new Size(1500, 900));
        content.Arrange(new Rect(0, 0, 1500, 900));
        content.UpdateLayout();
        if (EditorMenuBar.Visibility != Visibility.Visible
            || EditorMenuBar.ActualHeight <= 0)
        {
            throw new InvalidOperationException(
                "The editor menu must appear with the editor.");
        }

        var reachable = new HashSet<string>(StringComparer.Ordinal);
        foreach (var item in EnumerateMenuItems(EditorMenu.Items))
        {
            if (item.Tag is not string tag || !item.Header.ToString()!.EndsWith('…'))
            {
                continue;
            }

            if (!ToolFactories.ContainsKey(tag))
            {
                throw new InvalidOperationException(
                    $"Menu entry '{item.Header}' names an unknown tool '{tag}'.");
            }

            reachable.Add(tag);
        }

        var unreachable = ToolIds.Where(id => !reachable.Contains(id)).ToArray();
        if (unreachable.Length > 0)
        {
            throw new InvalidOperationException(
                "Every tool must be reachable from the menu; missing: "
                + string.Join(", ", unreachable));
        }

        foreach (var (key, modifiers, toolId) in ToolShortcuts)
        {
            var gesture = DescribeShortcut(key, modifiers);
            var advertised = EnumerateMenuItems(EditorMenu.Items)
                .Any(item => Equals(item.Tag, toolId)
                             && string.Equals(
                                 item.InputGestureText,
                                 gesture,
                                 StringComparison.OrdinalIgnoreCase));
            if (!advertised)
            {
                throw new InvalidOperationException(
                    $"The shortcut {gesture} of '{toolId}' must be shown on its menu entry.");
            }
        }

        viewModel.ShowManagerCommand.Execute(null);
        content.UpdateLayout();
        if (EditorMenuBar.Visibility != Visibility.Collapsed)
        {
            throw new InvalidOperationException(
                "Manager keeps its toolbar; the editor menu must leave with the editor.");
        }
    }

    /// <summary>
    /// The gesture text the menu shows for a shortcut. WPF's KeyGesture
    /// refuses Shift+letter, so the text is spelled out here.
    /// </summary>
    internal static string DescribeShortcut(Key key, ModifierKeys modifiers)
    {
        var parts = new List<string>(4);
        if (modifiers.HasFlag(ModifierKeys.Control))
        {
            parts.Add("Ctrl");
        }

        if (modifiers.HasFlag(ModifierKeys.Shift))
        {
            parts.Add("Shift");
        }

        if (modifiers.HasFlag(ModifierKeys.Alt))
        {
            parts.Add("Alt");
        }

        parts.Add(key is >= Key.D0 and <= Key.D9
            ? ((int)key - (int)Key.D0).ToString(System.Globalization.CultureInfo.InvariantCulture)
            : key.ToString());
        return string.Join("+", parts);
    }

    private static IEnumerable<MenuItem> EnumerateMenuItems(ItemCollection items)
    {
        foreach (var item in items.OfType<MenuItem>())
        {
            yield return item;
            foreach (var nested in EnumerateMenuItems(item.Items))
            {
                yield return nested;
            }
        }
    }
}

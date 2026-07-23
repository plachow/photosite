using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using CommunityToolkit.Mvvm.Input;
using Microsoft.Win32;
using PhotoSite.Controls;
using PhotoSite.Domain;
using PhotoSite.Infrastructure;
using PhotoSite.Services;
using PhotoSite.ViewModels;
using ShapePath = System.Windows.Shapes.Path;

namespace PhotoSite;

public partial class MainWindow : Window
{
    private const int DwmUseImmersiveDarkMode = 20;
    private const int DwmUseImmersiveDarkModeLegacy = 19;
    private const int DwmBorderColor = 34;
    private const int DwmCaptionColor = 35;
    private const int DwmTextColor = 36;
    private const string WindowLayoutSetting = "window_layout_v1";
    private const string LastCopyDestinationSetting = "last_copy_destination";
    private const string ImgurClientIdSetting = "imgur_client_id";
    private const double DefaultNavigatorWidth = 260;
    private const double DefaultCatalogWidth = 420;
    private const double NavigatorMinWidth = 180;
    private const double CatalogMinWidth = 280;
    private const double ViewerMinWidth = 320;
    private const double SplitterWidth = 5;
    private const double MinimumVisibleWindowWidth = 96;
    private const double VisibleTitleBarHeight = 48;
    private readonly MainViewModel viewModel;
    private readonly PhotoCatalogRepository catalog;
    private readonly DispatcherTimer layoutSaveTimer;
    private readonly SemaphoreSlim layoutSaveGate = new(1, 1);
    private CancellationTokenSource? copySelectionCancellation;
    private CancellationTokenSource? imgurUploadCancellation;
    private double navigatorPaneWidth = DefaultNavigatorWidth;
    private double catalogPaneWidth = DefaultCatalogWidth;
    private WindowLayoutState? layoutBeforeFullscreen;
    private WindowStyle windowStyleBeforeFullscreen;
    private ResizeMode resizeModeBeforeFullscreen;
    private bool isLayoutRestored;
    private bool isLayoutClosePending;
    private bool isClosingAfterLayoutSave;
    private bool isEditorExitPromptActive;
    private bool isImgurUploadActive;
    private bool refreshCatalogAfterEditorExit;
    private string? lastCopyDestination;

    public MainWindow(
        MainViewModel viewModel,
        PhotoCatalogRepository catalog)
    {
        this.viewModel = viewModel;
        this.catalog = catalog;
        GuardedPreviousCommand = new AsyncRelayCommand(
            () => NavigateByAsync(-1));
        GuardedNextCommand = new AsyncRelayCommand(
            () => NavigateByAsync(1));
        GuardedToggleEditorCommand = new AsyncRelayCommand(
            ToggleEditorWithGuardAsync);
        InitializeComponent();
        layoutSaveTimer = new DispatcherTimer
        {
            Interval = TimeSpan.FromMilliseconds(350)
        };
        layoutSaveTimer.Tick += OnLayoutSaveTimerTick;
        DataContext = viewModel;
        SourceInitialized += OnSourceInitialized;
        LocationChanged += (_, _) => ScheduleLayoutSave();
        SizeChanged += (_, _) => ScheduleLayoutSave();
        StateChanged += (_, _) => ScheduleLayoutSave();
        Closing += OnWindowClosing;
        viewModel.PropertyChanged += OnViewModelPropertyChanged;
        Closed += OnWindowClosed;
    }

    public IAsyncRelayCommand GuardedPreviousCommand { get; }

    public IAsyncRelayCommand GuardedNextCommand { get; }

    public IAsyncRelayCommand GuardedToggleEditorCommand { get; }

    public async Task RestoreLayoutAsync()
    {
        try
        {
            var savedCopyDestination = await catalog.GetSettingAsync(
                LastCopyDestinationSetting);
            if (!string.IsNullOrWhiteSpace(savedCopyDestination)
                && Directory.Exists(savedCopyDestination))
            {
                lastCopyDestination = Path.GetFullPath(savedCopyDestination);
            }
        }
        catch
        {
            // A missing last-copy destination must not prevent startup.
        }

        try
        {
            var json = await catalog.GetSettingAsync(
                WindowLayoutSetting);
            if (!string.IsNullOrWhiteSpace(json))
            {
                var state = JsonSerializer.Deserialize<WindowLayoutState>(json);
                if (state is not null)
                {
                    ApplyWindowLayout(state);
                }
            }
        }
        catch (JsonException)
        {
            // Ignore malformed UI state and keep the safe XAML defaults.
        }
        catch
        {
            // A layout restore failure must not prevent the catalogue from opening.
        }
        finally
        {
            isLayoutRestored = true;
        }
    }

    internal void ValidatePaneScrollBarsForSmokeTest()
    {
        if (Content is not UIElement content)
        {
            throw new InvalidOperationException("The main window has no UI content.");
        }

        var size = new Size(1500, 900);
        content.Measure(size);
        content.Arrange(new Rect(size));
        content.UpdateLayout();

        var directoryScrollBar = FindVerticalScrollBar(DirectoryTreeView);
        var thumbnailScrollBar = FindVerticalScrollBar(PhotoList);
        if (!ReferenceEquals(
                directoryScrollBar.Template,
                thumbnailScrollBar.Template))
        {
            throw new InvalidOperationException(
                "Directory and thumbnail panes do not share the same scrollbar template.");
        }
    }

    internal void ValidatePhotoContextMenuForSmokeTest()
    {
        if (Resources["PhotoContextMenuItemStyle"] is not Style itemStyle
            || Resources["PhotoContextMenuStyle"] is not Style contextMenuStyle
            || Resources["PhotoContextMenuSeparatorStyle"]
                is not Style separatorStyle
            || Resources["PhotoFileContextMenu"] is not ContextMenu contextMenu)
        {
            throw new InvalidOperationException(
                "The photo context menu styles were not created.");
        }

        contextMenu.ApplyTemplate();
        contextMenu.Measure(new Size(340, 280));
        contextMenu.Arrange(new Rect(0, 0, 340, 280));
        if (!ReferenceEquals(contextMenu.Style, contextMenuStyle)
            || VisualTreeHelper.GetChildrenCount(contextMenu) != 1
            || VisualTreeHelper.GetChild(contextMenu, 0) is not Border
            {
                Background: SolidColorBrush
                {
                    Color: var menuBackground
                }
            }
            || menuBackground != Color.FromRgb(0x24, 0x28, 0x32))
        {
            throw new InvalidOperationException(
                "The context menu must replace the system light gutter "
                + "with a fully dark root template.");
        }

        var items = contextMenu.Items.OfType<MenuItem>().ToArray();
        var separators = contextMenu.Items.OfType<Separator>().ToArray();
        if (items.Length != 7
            || items.Any(item => item.Icon is null
                                 || !ReferenceEquals(item.Style, itemStyle))
            || separators.Length != 2
            || separators.Any(separator =>
                !ReferenceEquals(separator.Style, separatorStyle)))
        {
            throw new InvalidOperationException(
                "Photo context-menu sections must use the dark item "
                + "and separator templates.");
        }

        foreach (var item in items)
        {
            item.ApplyTemplate();
            item.Measure(new Size(260, 34));
            item.Arrange(new Rect(0, 0, 260, 34));
            if (VisualTreeHelper.GetChildrenCount(item) != 1
                || VisualTreeHelper.GetChild(item, 0) is not Border
                {
                    Background: SolidColorBrush
                    {
                        Color: var color
                    }
                }
                || color != Color.FromRgb(0x24, 0x28, 0x32))
            {
                throw new InvalidOperationException(
                    "Context-menu rows must cover the system icon gutter "
                    + "with an opaque dark background.");
            }
        }

        OnPhotoFileContextMenuOpened(
            contextMenu,
            new RoutedEventArgs());
        if (items.Single(item =>
                    Equals(item.Tag, "LastCopyDestination")).Visibility
                != Visibility.Collapsed
            || items.Where(item =>
                    !Equals(item.Tag, "LastCopyDestination"))
                .Any(item => item.Visibility != Visibility.Visible))
        {
            throw new InvalidOperationException(
                "The quick-copy action must stay hidden until a destination "
                + "has been used.");
        }

        if (Resources["EditorContextMenu"] is not ContextMenu editorContextMenu
            || ReferenceEquals(editorContextMenu, contextMenu)
            || !ReferenceEquals(PreviewViewer.ContextMenu, editorContextMenu))
        {
            throw new InvalidOperationException(
                "The viewer must use a dedicated editor context menu.");
        }

        var thumbnailMenuSetter = PhotoList.ItemContainerStyle.Setters
            .OfType<Setter>()
            .SingleOrDefault(setter => setter.Property == ContextMenuProperty);
        if (!ReferenceEquals(thumbnailMenuSetter?.Value, contextMenu))
        {
            throw new InvalidOperationException(
                "File actions must remain attached only to thumbnail items.");
        }

        var editorItems = editorContextMenu.Items
            .OfType<MenuItem>()
            .ToArray();
        if (editorItems.Length != 4
            || editorItems.Any(item => item.Icon is null
                                       || !ReferenceEquals(item.Style, itemStyle)))
        {
            throw new InvalidOperationException(
                "Editor context-menu items must use the dark icon template.");
        }

        PreviewViewer.EndSelectionMode();
        OnEditorContextMenuOpened(
            editorContextMenu,
            new RoutedEventArgs());
        if (editorItems.Single(item => Equals(item.Tag, "Select")).Visibility
                != Visibility.Visible
            || editorItems
                .Where(item => !Equals(item.Tag, "Select"))
                .Any(item => item.Visibility != Visibility.Collapsed)
            || editorContextMenu.Items
                .OfType<Separator>()
                .Any(separator => separator.Visibility != Visibility.Collapsed))
        {
            throw new InvalidOperationException(
                "The editor context menu must follow selection state.");
        }
    }

    internal void ValidateDarkThemeIconsForSmokeTest()
    {
        var icons = FolderScopeIcon.Child is Grid icon
            ? icon.Children.OfType<ShapePath>().ToArray()
            : [];
        if (icons.Length != 2
            || icons.Any(path => path.Stroke is not SolidColorBrush brush
                                 || brush.Color
                                 != Color.FromRgb(0xC8, 0xCD, 0xD8)))
        {
            throw new InvalidOperationException(
                "Status icons must use explicit colors that remain visible "
                + "on the dark application background.");
        }
    }

    internal void ValidateStatusBarLayoutForSmokeTest()
    {
        if (FindName("PhotoScopeSection") is not FrameworkElement
            {
                Width: 130
            }
            || FindName("FlatFolderIcon") is not ShapePath
            {
                Stroke: SolidColorBrush
                {
                    Color: var flatColor
                }
            }
            || FindName("RecursiveTreeIcon") is not ShapePath
            {
                Stroke: SolidColorBrush
                {
                    Color: var recursiveColor
                }
            }
            || flatColor != Color.FromRgb(0xC8, 0xCD, 0xD8)
            || recursiveColor != Color.FromRgb(0xC8, 0xCD, 0xD8)
            || FindName("StatusSeparator1") is not Border
            || FindName("StatusSeparator2") is not Border
            || FindName("StatusSeparator3") is not Border
            || FindName("RatingFilterSection") is not FrameworkElement ratingSection)
        {
            throw new InvalidOperationException(
                "Status sections must use stable widths, visible vector icons, "
                + "and explicit separators.");
        }

        var originalFlatVisibility = FlatFolderIcon.Visibility;
        var originalRecursiveVisibility = RecursiveTreeIcon.Visibility;
        var initialRatingPosition = ratingSection.TranslatePoint(
            new Point(),
            this).X;
        try
        {
            FlatFolderIcon.Visibility = originalFlatVisibility == Visibility.Visible
                ? Visibility.Collapsed
                : Visibility.Visible;
            RecursiveTreeIcon.Visibility =
                originalRecursiveVisibility == Visibility.Visible
                    ? Visibility.Collapsed
                    : Visibility.Visible;
            UpdateLayout();
            var toggledRatingPosition = ratingSection.TranslatePoint(
                new Point(),
                this).X;
            if (Math.Abs(toggledRatingPosition - initialRatingPosition) > 0.1)
            {
                throw new InvalidOperationException(
                    "Switching between flat and recursive icons must not move "
                    + "the rating or search filters.");
            }
        }
        finally
        {
            FlatFolderIcon.ClearValue(VisibilityProperty);
            RecursiveTreeIcon.ClearValue(VisibilityProperty);
            UpdateLayout();
        }
    }

    internal void ValidateSelectionToolsForSmokeTest()
    {
        if (PreviewViewer.IsSelectionMode
            || SelectionActionsPanel.Visibility != Visibility.Collapsed)
        {
            throw new InvalidOperationException(
                "Selection tools must start inactive.");
        }

        SelectRegionButton.RaiseEvent(
            new RoutedEventArgs(Button.ClickEvent));
        UpdateLayout();
        if (!PreviewViewer.IsSelectionMode
            || SelectRegionButton.IsChecked != true)
        {
            throw new InvalidOperationException(
                "The Select button must activate and display image selection mode.");
        }

        SelectRegionButton.RaiseEvent(
            new RoutedEventArgs(Button.ClickEvent));
        UpdateLayout();
        if (PreviewViewer.IsSelectionMode
            || SelectRegionButton.IsChecked != false)
        {
            throw new InvalidOperationException(
                "The Select button must also leave and clear image selection mode.");
        }
    }

    internal void ValidatePastedImageBindingForSmokeTest(BitmapSource bitmap)
    {
        var originalSelection = viewModel.SelectedPhoto;
        var pasted = viewModel.OpenPastedImage(bitmap);
        UpdateLayout();
        if (!ReferenceEquals(PreviewViewer.SourceBitmap, bitmap)
            || !string.IsNullOrEmpty(PreviewViewer.SourcePath)
            || !pasted.IsUnsaved
            || !pasted.IsEditorDirty)
        {
            throw new InvalidOperationException(
                "A pasted bitmap must open as the active unsaved editor image.");
        }

        pasted.DiscardEditorSession();
        viewModel.ShowManagerCommand.Execute(null);
        if (!ReferenceEquals(viewModel.SelectedPhoto, originalSelection))
        {
            throw new InvalidOperationException(
                "Leaving a pasted image must restore the catalogue selection.");
        }
    }

    internal void ValidateCatalogTileForSmokeTest(string expectedFileName)
    {
        if (Content is not UIElement content)
        {
            throw new InvalidOperationException("The main window has no UI content.");
        }

        var size = new Size(1500, 900);
        content.Measure(size);
        content.Arrange(new Rect(size));
        content.UpdateLayout();

        if (PhotoList.ItemContainerGenerator.ContainerFromIndex(0)
            is not ListBoxItem item)
        {
            throw new InvalidOperationException(
                "The first catalogue tile was not realized.");
        }

        var fileName = FindTextBlock(item, expectedFileName);
        if (fileName.Foreground is not SolidColorBrush brush
            || brush.Color != Color.FromRgb(0xF2, 0xF4, 0xF8))
        {
            throw new InvalidOperationException(
                "Catalogue file names must use the light foreground color.");
        }
    }

    internal void ValidateCatalogScrollResetForSmokeTest()
    {
        if (Content is not UIElement content)
        {
            throw new InvalidOperationException("The main window has no UI content.");
        }

        var originalPhotos = viewModel.Photos.ToArray();
        var originalSelection = viewModel.SelectedPhoto;
        if (originalPhotos.Length == 0)
        {
            throw new InvalidOperationException(
                "The scroll reset test requires at least one catalogue photo.");
        }

        try
        {
            viewModel.Photos.ReplaceRange(
                Enumerable.Range(0, 100)
                    .Select(index => originalPhotos[index % originalPhotos.Length]));
            var size = new Size(1500, 900);
            content.Measure(size);
            content.Arrange(new Rect(size));
            content.UpdateLayout();

            var scrollViewer = FindScrollViewer(PhotoList);
            scrollViewer.ScrollToBottom();
            content.UpdateLayout();
            if (scrollViewer.VerticalOffset <= 0)
            {
                throw new InvalidOperationException(
                    "The catalogue test could not establish a non-zero scroll offset.");
            }

            viewModel.Photos.ReplaceRange(originalPhotos);
            content.UpdateLayout();
            if (scrollViewer.VerticalOffset > 0)
            {
                throw new InvalidOperationException(
                    "Shrinking the catalogue must clamp an obsolete scroll offset.");
            }

            viewModel.Photos.ReplaceRange(
                Enumerable.Range(0, 100)
                    .Select(index => originalPhotos[index % originalPhotos.Length]));
            content.UpdateLayout();
            scrollViewer.ScrollToBottom();
            content.UpdateLayout();

            OnViewModelPropertyChanged(
                viewModel,
                new PropertyChangedEventArgs(nameof(MainViewModel.CurrentFolder)));
            content.UpdateLayout();
            if (scrollViewer.VerticalOffset > 0)
            {
                throw new InvalidOperationException(
                    "Changing folders must reset the catalogue scroll position.");
            }
        }
        finally
        {
            viewModel.Photos.ReplaceRange(originalPhotos);
            viewModel.SelectedPhoto = originalSelection;
            ResetCatalogScrollPosition();
            content.UpdateLayout();
        }
    }

    internal void ValidatePreviewWheelNavigationForSmokeTest()
    {
        if (viewModel.Photos.Count < 2)
        {
            throw new InvalidOperationException(
                "Wheel navigation requires at least two catalogue photos.");
        }

        viewModel.SelectedPhoto = viewModel.Photos[0];
        PreviewViewer.RaiseEvent(
            new MouseWheelEventArgs(
                Mouse.PrimaryDevice,
                Environment.TickCount,
                -120)
            {
                RoutedEvent = Mouse.MouseWheelEvent,
                Source = PreviewViewer
            });

        if (!ReferenceEquals(
                viewModel.SelectedPhoto,
                viewModel.Photos[1]))
        {
            throw new InvalidOperationException(
                "The preview mouse wheel must navigate in combined view.");
        }
    }

    private void OnPhotoListMouseDoubleClick(
        object sender,
        MouseButtonEventArgs eventArgs)
    {
        if (eventArgs.OriginalSource is not DependencyObject source
            || ItemsControl.ContainerFromElement(PhotoList, source)
                is not ListBoxItem)
        {
            return;
        }

        if (!viewModel.IsEditorMode
            && viewModel.ShowEditorCommand.CanExecute(null))
        {
            viewModel.ShowEditorCommand.Execute(null);
            eventArgs.Handled = true;
        }
    }

    private void OnPhotoListPreviewMouseDown(
        object sender,
        MouseButtonEventArgs eventArgs)
    {
        if (eventArgs.ChangedButton != MouseButton.Middle
            || eventArgs.OriginalSource is not DependencyObject source
            || ItemsControl.ContainerFromElement(PhotoList, source)
                is not ListBoxItem item
            || item.DataContext is not PhotoItemViewModel photo)
        {
            return;
        }

        viewModel.SelectedPhoto = photo;
        if (viewModel.ToggleFullscreenCommand.CanExecute(null))
        {
            viewModel.ToggleFullscreenCommand.Execute(null);
            eventArgs.Handled = true;
        }
    }

    private void OnShowInExplorerClick(object sender, RoutedEventArgs eventArgs)
    {
        if (sender is not MenuItem
            {
                DataContext: PhotoItemViewModel photo
            }
            || !File.Exists(photo.Path))
        {
            return;
        }

        Process.Start(CreateExplorerSelectStartInfo(photo.Path));
    }

    private void OnOpenInEditorClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (sender is not MenuItem
            {
                DataContext: PhotoItemViewModel photo
            })
        {
            return;
        }

        viewModel.SelectedPhoto = photo;
        if (viewModel.ShowEditorCommand.CanExecute(null))
        {
            viewModel.ShowEditorCommand.Execute(null);
        }
    }

    private void OnPhotoFileContextMenuOpened(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (sender is not ContextMenu contextMenu)
        {
            return;
        }

        var hasLastDestination = HasLastCopyDestination();
        var lastDestinationItem = contextMenu.Items
            .OfType<MenuItem>()
            .Single(item => Equals(item.Tag, "LastCopyDestination"));
        lastDestinationItem.Visibility = hasLastDestination
            ? Visibility.Visible
            : Visibility.Collapsed;
        if (!hasLastDestination)
        {
            lastDestinationItem.ToolTip = null;
            return;
        }

        var displayName = GetDestinationDisplayName(lastCopyDestination!);
        lastDestinationItem.Header = $"Copy to “{displayName}”";
        lastDestinationItem.ToolTip = lastCopyDestination;
    }

    private void OnCopyFileNameClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (sender is MenuItem
            {
                DataContext: PhotoItemViewModel photo
            })
        {
            SetClipboardText(
                photo.FileName,
                $"Copied filename: {photo.FileName}");
        }
    }

    private void OnCopyFullPathClick(object sender, RoutedEventArgs eventArgs)
    {
        if (sender is MenuItem
            {
                DataContext: PhotoItemViewModel photo
            })
        {
            SetClipboardText(
                photo.Path,
                $"Copied full path: {photo.FileName}");
        }
    }

    private async void OnCopyImageClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (sender is MenuItem
            {
                DataContext: PhotoItemViewModel photo
            })
        {
            await CopyImageAsync(photo);
        }
    }

    private async void OnCopyFileToClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (sender is MenuItem
            {
                DataContext: PhotoItemViewModel photo
            })
        {
            await ChooseAndCopyFileAsync(photo);
        }
    }

    private async void OnCopyFileToLastDestinationClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (sender is MenuItem
            {
                DataContext: PhotoItemViewModel photo
            })
        {
            await CopyFileToLastDestinationAsync(photo);
        }
    }

    private void SetClipboardText(string text, string successStatus)
    {
        try
        {
            Clipboard.SetText(text);
            viewModel.ReportStatus(successStatus);
        }
        catch (ExternalException exception)
        {
            viewModel.ReportStatus(
                $"Cannot access the clipboard: {exception.Message}");
        }
    }

    private async Task CopyImageAsync(PhotoItemViewModel photo)
    {
        if (!File.Exists(photo.Path))
        {
            viewModel.ReportStatus($"File not found: {photo.Path}");
            return;
        }

        copySelectionCancellation?.Cancel();
        copySelectionCancellation?.Dispose();
        copySelectionCancellation = new CancellationTokenSource();
        var cancellationToken = copySelectionCancellation.Token;
        viewModel.ReportStatus($"Copying image: {photo.FileName}…");

        try
        {
            var source = await App.Services.Previews.LoadAsync(
                photo.Path,
                0,
                cancellationToken);
            cancellationToken.ThrowIfCancellationRequested();
            var crop = photo.EditRecipe.Crop?.ConstrainToUnit()
                ?? new CropRegion(0, 0, 1, 1);
            var rendered = Controls.PhotoViewer.RenderSelection(
                source,
                photo.EditRecipe,
                crop);
            await SetClipboardImageAsync(rendered, cancellationToken);
            viewModel.ReportStatus(
                $"Copied {photo.FileName} "
                + $"({rendered.PixelWidth:N0} × {rendered.PixelHeight:N0} pixels)");
        }
        catch (OperationCanceledException)
        {
        }
        catch (ImgurUploadException exception)
            when (exception.StatusCode is
                  System.Net.HttpStatusCode.Unauthorized
                  or System.Net.HttpStatusCode.Forbidden)
        {
            try
            {
                await catalog.SetSettingAsync(
                    ImgurClientIdSetting,
                    string.Empty,
                    CancellationToken.None);
            }
            catch
            {
                // The upload error remains actionable even if settings cleanup fails.
            }

            viewModel.ReportStatus(
                "Imgur rejected the Client ID; the saved value was cleared.");
            MessageBox.Show(
                this,
                exception.Message
                + "\n\nThe saved Client ID was cleared. "
                + "Press Ctrl+U to enter it again.",
                "Imgur authorization failed",
                MessageBoxButton.OK,
                MessageBoxImage.Error);
        }
        catch (Exception exception)
        {
            viewModel.ReportStatus(
                $"Cannot copy image: {exception.Message}");
        }
    }

    private async Task ChooseAndCopyFileAsync(PhotoItemViewModel photo)
    {
        var sourceDirectory = Path.GetDirectoryName(photo.Path);
        var dialog = new OpenFolderDialog
        {
            Title = $"Copy {photo.FileName} to folder",
            Multiselect = false,
            InitialDirectory = HasLastCopyDestination()
                ? lastCopyDestination
                : sourceDirectory
        };
        if (dialog.ShowDialog(this) != true)
        {
            return;
        }

        var destinationDirectory = Path.GetFullPath(dialog.FolderName);
        if (!await CopyPhotoFileAsync(photo.Path, destinationDirectory))
        {
            return;
        }

        await RememberLastCopyDestinationAsync(destinationDirectory);
    }

    private async Task CopyFileToLastDestinationAsync(
        PhotoItemViewModel photo)
    {
        if (!HasLastCopyDestination())
        {
            return;
        }

        await CopyPhotoFileAsync(photo.Path, lastCopyDestination!);
    }

    private async Task<bool> CopyPhotoFileAsync(
        string sourcePath,
        string destinationDirectory)
    {
        if (!File.Exists(sourcePath))
        {
            viewModel.ReportStatus($"File not found: {sourcePath}");
            return false;
        }

        if (!Directory.Exists(destinationDirectory))
        {
            viewModel.ReportStatus(
                $"Destination folder not found: {destinationDirectory}");
            return false;
        }

        var destinationPath = BuildFileCopyDestination(
            sourcePath,
            destinationDirectory);
        if (string.Equals(
                Path.GetFullPath(sourcePath),
                destinationPath,
                StringComparison.OrdinalIgnoreCase))
        {
            viewModel.ReportStatus(
                $"{Path.GetFileName(sourcePath)} is already in that folder");
            return false;
        }

        var overwrite = false;
        if (File.Exists(destinationPath))
        {
            var choice = MessageBox.Show(
                this,
                $"{destinationPath} already exists.\n\nReplace it?",
                "Replace existing file?",
                MessageBoxButton.YesNo,
                MessageBoxImage.Warning,
                MessageBoxResult.No);
            if (choice != MessageBoxResult.Yes)
            {
                return false;
            }

            overwrite = true;
        }

        var fileName = Path.GetFileName(sourcePath);
        viewModel.ReportStatus($"Copying {fileName}…");
        try
        {
            await Task.Run(
                () => File.Copy(
                    sourcePath,
                    destinationPath,
                    overwrite));
            viewModel.ReportStatus(
                $"Copied {fileName} to {destinationDirectory}");
            return true;
        }
        catch (Exception exception)
        {
            MessageBox.Show(
                this,
                $"The file could not be copied.\n\n{exception.Message}",
                "Copy failed",
                MessageBoxButton.OK,
                MessageBoxImage.Error);
            viewModel.ReportStatus(
                $"Cannot copy {fileName}: {exception.Message}");
            return false;
        }
    }

    private async Task RememberLastCopyDestinationAsync(
        string destinationDirectory)
    {
        lastCopyDestination = Path.GetFullPath(destinationDirectory);
        try
        {
            await catalog.SetSettingAsync(
                LastCopyDestinationSetting,
                lastCopyDestination);
        }
        catch (Exception exception)
        {
            viewModel.ReportStatus(
                "File copied, but the destination could not be remembered: "
                + exception.Message);
        }
    }

    private bool HasLastCopyDestination() =>
        !string.IsNullOrWhiteSpace(lastCopyDestination)
        && Directory.Exists(lastCopyDestination);

    private static string GetDestinationDisplayName(string path)
    {
        var fullPath = Path.GetFullPath(path);
        var name = Path.GetFileName(
            Path.TrimEndingDirectorySeparator(fullPath));
        return string.IsNullOrWhiteSpace(name)
            ? fullPath
            : name;
    }

    internal static string BuildFileCopyDestination(
        string sourcePath,
        string destinationDirectory) =>
        Path.Combine(
            Path.GetFullPath(destinationDirectory),
            Path.GetFileName(sourcePath));

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

    internal static ProcessStartInfo CreateExplorerSelectStartInfo(string path)
    {
        var startInfo = new ProcessStartInfo
        {
            FileName = "explorer.exe",
            UseShellExecute = true
        };
        startInfo.ArgumentList.Add("/select,");
        startInfo.ArgumentList.Add(path);
        return startInfo;
    }

    private void OnSelectRegionClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        PreviewViewer.ToggleSelectionMode();
        PreviewViewer.Focus();
    }

    private void OnStartSelectionClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        PreviewViewer.IsSelectionMode = true;
        PreviewViewer.Focus();
    }

    private void OnClearSelectionClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        PreviewViewer.ClearSelection();
        PreviewViewer.Focus();
    }

    private void OnPreviewContextMenuOpening(
        object sender,
        ContextMenuEventArgs eventArgs)
    {
        if (!viewModel.IsEditorMode)
        {
            eventArgs.Handled = true;
        }
    }

    private void OnEditorContextMenuOpened(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (sender is not ContextMenu contextMenu)
        {
            return;
        }

        var hasSelection = PreviewViewer.HasSelection;
        foreach (var item in contextMenu.Items.OfType<MenuItem>())
        {
            var isSelectionAction = !Equals(item.Tag, "Select");
            item.Visibility = isSelectionAction == hasSelection
                ? Visibility.Visible
                : Visibility.Collapsed;
        }

        foreach (var separator in contextMenu.Items.OfType<Separator>())
        {
            separator.Visibility = hasSelection
                ? Visibility.Visible
                : Visibility.Collapsed;
        }
    }

    private void OnCropSelectionClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        ApplyCropSelection();
    }

    private async void OnCopySelectionClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        await CopySelectionAsync();
    }

    private void ApplyCropSelection()
    {
        if (viewModel.SelectedPhoto is null
            || PreviewViewer.SelectionRegion is not { } selection)
        {
            return;
        }

        viewModel.SelectedPhoto.ApplyCrop(selection);
        PreviewViewer.ClearSelection();
        PreviewViewer.FitToViewport();
        viewModel.ReportStatus("Crop applied non-destructively");
    }

    private async Task CopySelectionAsync()
    {
        if (!PreviewViewer.HasSelection)
        {
            return;
        }

        copySelectionCancellation?.Cancel();
        copySelectionCancellation?.Dispose();
        copySelectionCancellation = new CancellationTokenSource();
        var cancellationToken = copySelectionCancellation.Token;
        viewModel.ReportStatus("Copying selection…");

        try
        {
            var size = await PreviewViewer.CopySelectionToClipboardAsync(
                cancellationToken);
            if (!cancellationToken.IsCancellationRequested
                && size.Width > 0
                && size.Height > 0)
            {
                viewModel.ReportStatus(
                    $"Copied {size.Width:N0} × {size.Height:N0} pixels");
            }
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception exception)
        {
            viewModel.ReportStatus(
                $"Cannot copy selection: {exception.Message}");
        }
    }

    private async void OnSaveAsClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        await SaveCurrentImageAsAsync();
    }

    private async void OnUploadToImgurClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        await UploadCurrentImageToImgurAsync();
    }

    private async Task PasteImageFromClipboardAsync()
    {
        BitmapSource? bitmap;
        try
        {
            bitmap = await ReadClipboardBitmapAsync();
        }
        catch (Exception exception)
        {
            viewModel.ReportStatus(
                $"Cannot read an image from the clipboard: {exception.Message}");
            return;
        }

        if (bitmap is null)
        {
            viewModel.ReportStatus(
                "The clipboard does not contain a bitmap.");
            return;
        }

        if (viewModel.IsEditorMode
            && !await ConfirmEditorExitAsync())
        {
            return;
        }

        viewModel.OpenPastedImage(bitmap);
        PreviewViewer.Focus();
        viewModel.ReportStatus(
            $"Pasted {bitmap.PixelWidth:N0} × {bitmap.PixelHeight:N0} pixels "
            + "as an unsaved image");
    }

    private static async Task<BitmapSource?> ReadClipboardBitmapAsync(
        CancellationToken cancellationToken = default)
    {
        for (var attempt = 0; ; attempt++)
        {
            cancellationToken.ThrowIfCancellationRequested();
            try
            {
                if (!Clipboard.ContainsImage()
                    || Clipboard.GetImage() is not { } clipboardBitmap)
                {
                    return null;
                }

                var detached = new WriteableBitmap(clipboardBitmap);
                detached.Freeze();
                return detached;
            }
            catch (ExternalException) when (attempt < 2)
            {
                await Task.Delay(35, cancellationToken);
            }
        }
    }

    private async Task SaveCurrentImageAsAsync()
    {
        if (!viewModel.IsEditorMode
            || viewModel.SelectedPhoto is not { } photo)
        {
            return;
        }

        try
        {
            if (photo.IsTransient)
            {
                var destination = ChoosePastedImageDestination(
                    photo.IsUnsaved ? null : photo.Path);
                if (destination is null)
                {
                    return;
                }

                viewModel.ReportStatus("Saving pasted image…");
                await SaveTransientImageAsync(
                    photo,
                    destination,
                    continueEditing: true);
                viewModel.ReportStatus(
                    $"Saved JPEG: {Path.GetFileName(destination)}");
                return;
            }

            var copyDestination = ChooseCopyDestination(photo.Path);
            if (copyDestination is null)
            {
                return;
            }

            viewModel.ReportStatus("Saving edited copy…");
            await App.Services.ImageSaver.SaveAsync(
                photo.Path,
                copyDestination,
                photo.EditRecipe,
                overwrite: File.Exists(copyDestination));
            refreshCatalogAfterEditorExit = true;
            viewModel.ReportStatus(
                $"Saved copy: {Path.GetFileName(copyDestination)}");
        }
        catch (Exception exception)
        {
            MessageBox.Show(
                this,
                $"The image could not be saved.\n\n{exception.Message}",
                "Save failed",
                MessageBoxButton.OK,
                MessageBoxImage.Error);
        }
    }

    private async Task<BitmapSource> SaveTransientImageAsync(
        PhotoItemViewModel photo,
        string destination,
        bool continueEditing)
    {
        var source = photo.SourceBitmap
            ?? throw new InvalidOperationException(
                "The pasted image pixels are no longer available.");
        var rendered = await App.Services.ImageSaver.SaveAsync(
            source,
            destination,
            photo.EditRecipe,
            overwrite: File.Exists(destination),
            jpegQuality: Services.ImageSaveService.DefaultPastedJpegQuality);
        photo.CompleteTransientSave(
            destination,
            rendered,
            continueEditing);
        refreshCatalogAfterEditorExit = true;
        return rendered;
    }

    private async Task UploadCurrentImageToImgurAsync()
    {
        if (!viewModel.IsEditorMode
            || viewModel.SelectedPhoto is not { } photo)
        {
            return;
        }

        if (isImgurUploadActive)
        {
            viewModel.ReportStatus("An Imgur upload is already running.");
            return;
        }

        isImgurUploadActive = true;
        imgurUploadCancellation = new CancellationTokenSource();
        var cancellationToken = imgurUploadCancellation.Token;
        try
        {
            var clientId = await catalog.GetSettingAsync(
                ImgurClientIdSetting,
                cancellationToken);
            if (string.IsNullOrWhiteSpace(clientId))
            {
                var dialog = new ImgurClientIdDialog
                {
                    Owner = this
                };
                if (dialog.ShowDialog() != true
                    || string.IsNullOrWhiteSpace(dialog.ClientId))
                {
                    return;
                }

                clientId = dialog.ClientId;
                await catalog.SetSettingAsync(
                    ImgurClientIdSetting,
                    clientId,
                    cancellationToken);
            }

            viewModel.ReportStatus("Rendering image for Imgur…");
            var rendered = await RenderCurrentImageAsync(
                photo,
                cancellationToken);
            viewModel.ReportStatus("Uploading image to Imgur…");
            var directUrl = await App.Services.ImgurUploader.UploadAsync(
                rendered,
                clientId,
                photo.IsUnsaved ? "PhotoSite pasted image" : photo.FileName,
                cancellationToken);
            await CopyUploadedUrlToClipboardAsync(
                directUrl,
                cancellationToken);
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception exception)
        {
            viewModel.ReportStatus($"Imgur upload failed: {exception.Message}");
            MessageBox.Show(
                this,
                $"The image could not be uploaded to Imgur.\n\n"
                + exception.Message,
                "Imgur upload failed",
                MessageBoxButton.OK,
                MessageBoxImage.Error);
        }
        finally
        {
            imgurUploadCancellation?.Dispose();
            imgurUploadCancellation = null;
            isImgurUploadActive = false;
        }
    }

    private static async Task<BitmapSource> RenderCurrentImageAsync(
        PhotoItemViewModel photo,
        CancellationToken cancellationToken)
    {
        var source = photo.SourceBitmap;
        if (source is null)
        {
            if (string.IsNullOrWhiteSpace(photo.Path))
            {
                throw new InvalidOperationException(
                    "The current image pixels are unavailable.");
            }

            source = await App.Services.Previews.LoadAsync(
                photo.Path,
                0,
                cancellationToken);
        }

        cancellationToken.ThrowIfCancellationRequested();
        var crop = photo.EditRecipe.Crop?.ConstrainToUnit()
            ?? new CropRegion(0, 0, 1, 1);
        return PhotoViewer.RenderSelection(
            source,
            photo.EditRecipe,
            crop);
    }

    private async Task CopyUploadedUrlToClipboardAsync(
        string directUrl,
        CancellationToken cancellationToken)
    {
        ExternalException? clipboardError = null;
        for (var attempt = 0; attempt < 3; attempt++)
        {
            cancellationToken.ThrowIfCancellationRequested();
            try
            {
                Clipboard.SetText(directUrl);
                viewModel.ReportStatus(
                    $"Uploaded to Imgur and copied URL: {directUrl}");
                return;
            }
            catch (ExternalException exception)
            {
                clipboardError = exception;
                if (attempt < 2)
                {
                    await Task.Delay(35, cancellationToken);
                }
            }
        }

        viewModel.ReportStatus(
            "Uploaded to Imgur, but the clipboard is unavailable.");
        MessageBox.Show(
            this,
            $"The image was uploaded, but its URL could not be copied "
            + $"to the clipboard.\n\n{directUrl}\n\n"
            + clipboardError?.Message,
            "Imgur upload completed",
            MessageBoxButton.OK,
            MessageBoxImage.Warning);
    }

    private async Task ToggleEditorWithGuardAsync()
    {
        if (viewModel.IsFullscreenMode)
        {
            return;
        }

        if (viewModel.IsEditorMode)
        {
            await LeaveEditorAsync();
        }
        else if (viewModel.ShowEditorCommand.CanExecute(null))
        {
            viewModel.ShowEditorCommand.Execute(null);
        }
    }

    private async Task LeaveEditorAsync()
    {
        if (!await ConfirmEditorExitAsync())
        {
            return;
        }

        if (viewModel.ShowManagerCommand.CanExecute(null))
        {
            viewModel.ShowManagerCommand.Execute(null);
        }
    }

    private async Task NavigateByAsync(int offset)
    {
        var current = viewModel.SelectedPhoto;
        if (current is null)
        {
            return;
        }

        var index = viewModel.Photos.IndexOf(current);
        var targetIndex = index + offset;
        if (targetIndex < 0 || targetIndex >= viewModel.Photos.Count)
        {
            return;
        }

        if (viewModel.IsEditorMode && !await ConfirmEditorExitAsync())
        {
            return;
        }

        viewModel.SelectedPhoto = viewModel.Photos[targetIndex];
        if (viewModel.IsEditorMode)
        {
            viewModel.SelectedPhoto.BeginEditorSession();
        }
    }

    private async Task<bool> ConfirmEditorExitAsync()
    {
        var photo = viewModel.SelectedPhoto;
        if (photo is null)
        {
            return true;
        }

        if (!photo.IsEditorDirty)
        {
            photo.CompleteCopySession();
            return true;
        }

        var extension = Path.GetExtension(photo.Path);
        var dialog = new SaveChangesDialog(
            photo.FileName,
            extension.Equals(".jpg", StringComparison.OrdinalIgnoreCase)
            || extension.Equals(".jpeg", StringComparison.OrdinalIgnoreCase),
            isUnsavedImage: photo.IsTransient && photo.IsUnsaved)
        {
            Owner = this
        };
        dialog.ShowDialog();

        try
        {
            switch (dialog.Choice)
            {
                case SaveChangesChoice.Cancel:
                    return false;
                case SaveChangesChoice.Discard:
                    photo.DiscardEditorSession();
                    return true;
                case SaveChangesChoice.OverwriteOriginal:
                    if (photo.IsTransient)
                    {
                        viewModel.ReportStatus("Saving changes…");
                        await SaveTransientImageAsync(
                            photo,
                            photo.Path,
                            continueEditing: false);
                        return true;
                    }

                    if (!App.Services.ImageSaver.CanOverwrite(photo.Path))
                    {
                        MessageBox.Show(
                            this,
                            "This format cannot be overwritten yet. "
                            + "Use Save as copy and choose PNG, JPEG, TIFF, or BMP.",
                            "Cannot overwrite",
                            MessageBoxButton.OK,
                            MessageBoxImage.Information);
                        return false;
                    }

                    viewModel.ReportStatus("Saving changes…");
                    await App.Services.ImageSaver.SaveAsync(
                        photo.Path,
                        photo.Path,
                        photo.EditRecipe,
                        overwrite: true);
                    await photo.CompleteOverwriteAsync();
                    PreviewViewer.Reload();
                    refreshCatalogAfterEditorExit = true;
                    return true;
                case SaveChangesChoice.SaveAsCopy:
                    if (photo.IsTransient)
                    {
                        var pastedDestination =
                            ChoosePastedImageDestination(
                                photo.IsUnsaved ? null : photo.Path);
                        if (pastedDestination is null)
                        {
                            return false;
                        }

                        viewModel.ReportStatus("Saving pasted image…");
                        await SaveTransientImageAsync(
                            photo,
                            pastedDestination,
                            continueEditing: false);
                        return true;
                    }

                    var destination = ChooseCopyDestination(photo.Path);
                    if (destination is null)
                    {
                        return false;
                    }

                    viewModel.ReportStatus("Saving copy…");
                    await App.Services.ImageSaver.SaveAsync(
                        photo.Path,
                        destination,
                        photo.EditRecipe,
                        overwrite: File.Exists(destination));
                    photo.CompleteCopySession();
                    refreshCatalogAfterEditorExit = true;
                    return true;
                default:
                    return false;
            }
        }
        catch (Exception exception)
        {
            MessageBox.Show(
                this,
                $"The image could not be saved.\n\n{exception.Message}",
                "Save failed",
                MessageBoxButton.OK,
                MessageBoxImage.Error);
            return false;
        }
    }

    private static string? ChooseCopyDestination(string sourcePath)
    {
        var suggested = Services.ImageSaveService.BuildVersionCopyPath(
            sourcePath);
        var dialog = new SaveFileDialog
        {
            Title = "Save edited copy",
            InitialDirectory = Path.GetDirectoryName(suggested),
            FileName = Path.GetFileName(suggested),
            AddExtension = true,
            DefaultExt = Path.GetExtension(suggested),
            Filter =
                "JPEG image|*.jpg;*.jpeg|PNG image|*.png|"
                + "TIFF image|*.tif;*.tiff|Bitmap image|*.bmp",
            OverwritePrompt = true
        };
        dialog.FilterIndex = Path.GetExtension(suggested).ToLowerInvariant() switch
        {
            ".png" => 2,
            ".tif" or ".tiff" => 3,
            ".bmp" => 4,
            _ => 1
        };
        return dialog.ShowDialog() == true
            ? dialog.FileName
            : null;
    }

    private static string? ChoosePastedImageDestination(
        string? currentPath)
    {
        var hasCurrentPath = !string.IsNullOrWhiteSpace(currentPath);
        var initialDirectory = hasCurrentPath
            ? Path.GetDirectoryName(currentPath)
            : Environment.GetFolderPath(
                Environment.SpecialFolder.MyPictures);
        if (string.IsNullOrWhiteSpace(initialDirectory)
            || !Directory.Exists(initialDirectory))
        {
            initialDirectory = Environment.CurrentDirectory;
        }

        var fileName = hasCurrentPath
            ? Path.GetFileName(currentPath)
            : $"Pasted image {DateTime.Now:yyyy-MM-dd HHmmss}.jpg";
        var dialog = new SaveFileDialog
        {
            Title = "Save pasted image as JPEG",
            InitialDirectory = initialDirectory,
            FileName = fileName,
            AddExtension = true,
            DefaultExt = ".jpg",
            Filter = "JPEG image|*.jpg;*.jpeg",
            FilterIndex = 1,
            OverwritePrompt = true
        };
        if (dialog.ShowDialog() != true)
        {
            return null;
        }

        var extension = Path.GetExtension(dialog.FileName);
        return string.Equals(
                   extension,
                   ".jpg",
                   StringComparison.OrdinalIgnoreCase)
               || string.Equals(
                   extension,
                   ".jpeg",
                   StringComparison.OrdinalIgnoreCase)
            ? dialog.FileName
            : Path.ChangeExtension(dialog.FileName, ".jpg");
    }

    private async void OnPreviewKeyDown(
        object sender,
        KeyEventArgs eventArgs)
    {
        if (PhotoSearchBox.IsKeyboardFocusWithin)
        {
            return;
        }

        if (IsPasteImageShortcut(
                eventArgs.Key,
                Keyboard.Modifiers))
        {
            eventArgs.Handled = true;
            await PasteImageFromClipboardAsync();
            return;
        }

        if (viewModel.IsEditorMode
            && IsSaveAsShortcut(
                eventArgs.Key,
                Keyboard.Modifiers))
        {
            eventArgs.Handled = true;
            await SaveCurrentImageAsAsync();
            return;
        }

        if (viewModel.IsEditorMode
            && IsImgurUploadShortcut(
                eventArgs.Key,
                Keyboard.Modifiers))
        {
            eventArgs.Handled = true;
            await UploadCurrentImageToImgurAsync();
            return;
        }

        if (!viewModel.IsEditorMode
            && !viewModel.IsFullscreenMode
            && PhotoList.IsKeyboardFocusWithin
            && viewModel.SelectedPhoto is { } managerPhoto)
        {
            if (IsQuickFileCopyShortcut(
                    eventArgs.Key,
                    Keyboard.Modifiers)
                && HasLastCopyDestination())
            {
                eventArgs.Handled = true;
                await CopyFileToLastDestinationAsync(managerPhoto);
                return;
            }

            if (IsCopyImageShortcut(
                    eventArgs.Key,
                    Keyboard.Modifiers))
            {
                eventArgs.Handled = true;
                await CopyImageAsync(managerPhoto);
                return;
            }
        }

        if (viewModel.IsEditorMode
            && Keyboard.Modifiers.HasFlag(ModifierKeys.Control)
            && eventArgs.Key is Key.Z or Key.Y
            && viewModel.SelectedPhoto is { } editingPhoto)
        {
            var editCommand = eventArgs.Key == Key.Z
                ? editingPhoto.UndoEditCommand
                : editingPhoto.RedoEditCommand;
            if (editCommand.CanExecute(null))
            {
                editCommand.Execute(null);
            }

            eventArgs.Handled = true;
            return;
        }

        if (PreviewViewer.IsSelectionMode)
        {
            if (eventArgs.Key == Key.Escape)
            {
                if (PreviewViewer.HasSelection)
                {
                    PreviewViewer.ClearSelection();
                }
                else
                {
                    PreviewViewer.EndSelectionMode();
                }

                eventArgs.Handled = true;
                return;
            }

            if (eventArgs.Key == Key.Enter)
            {
                if (PreviewViewer.HasSelection)
                {
                    ApplyCropSelection();
                }

                eventArgs.Handled = true;
                return;
            }

            if (eventArgs.Key == Key.C
                && Keyboard.Modifiers.HasFlag(ModifierKeys.Control)
                && PreviewViewer.HasSelection)
            {
                eventArgs.Handled = true;
                await CopySelectionAsync();
                return;
            }
        }

        if (IsSelectShortcut(eventArgs.Key, Keyboard.Modifiers)
            && viewModel.IsEditorMode
            && !viewModel.IsFullscreenMode)
        {
            if (!PreviewViewer.IsSelectionMode)
            {
                PreviewViewer.IsSelectionMode = true;
            }

            PreviewViewer.Focus();
            eventArgs.Handled = true;
            return;
        }

        if (eventArgs.Key == Key.Escape && viewModel.IsFullscreenMode)
        {
            viewModel.ToggleFullscreenCommand.Execute(null);
            eventArgs.Handled = true;
            return;
        }

        if (eventArgs.Key == Key.Escape && viewModel.IsEditorMode)
        {
            await LeaveEditorAsync();
            eventArgs.Handled = true;
            return;
        }

        if (Keyboard.Modifiers == ModifierKeys.None
            && TryHandleViewerShortcut(eventArgs.Key))
        {
            eventArgs.Handled = true;
            return;
        }

        if (Keyboard.Modifiers == ModifierKeys.None
            && viewModel.SelectedPhoto is not null
            && TryGetRatingShortcut(eventArgs.Key, out var rating))
        {
            viewModel.SelectedPhoto.Rating = rating;
            eventArgs.Handled = true;
            return;
        }

        if ((viewModel.IsEditorMode || viewModel.IsFullscreenMode)
            && TryGetPhotoNavigationCommand(eventArgs.Key, out var command)
            && command.CanExecute(null))
        {
            command.Execute(null);
            eventArgs.Handled = true;
            return;
        }

        if (eventArgs.Key != Key.Enter || viewModel.SelectedPhoto is null)
        {
            return;
        }

        if (viewModel.IsEditorMode || viewModel.IsFullscreenMode)
        {
            if (!viewModel.IsFullscreenMode
                && PreviewViewer.IsKeyboardFocusWithin
                && viewModel.ShowManagerCommand.CanExecute(null))
            {
                await LeaveEditorAsync();
                eventArgs.Handled = true;
            }

            return;
        }

        if (PhotoList.IsKeyboardFocusWithin
            && viewModel.ShowEditorCommand.CanExecute(null))
        {
            viewModel.ShowEditorCommand.Execute(null);
            eventArgs.Handled = true;
        }
    }

    private bool TryHandleViewerShortcut(Key key)
    {
        switch (key)
        {
            case Key.NumPad0:
                PreviewViewer.FitToViewport();
                return true;
            case Key.Multiply:
                PreviewViewer.ShowActualSize();
                return true;
            case Key.Add:
                PreviewViewer.ZoomIn();
                return true;
            case Key.Subtract:
                PreviewViewer.ZoomOut();
                return true;
            default:
                return false;
        }
    }

    internal static bool TryGetRatingShortcut(Key key, out int rating)
    {
        rating = key switch
        {
            Key.Oem3 => 0,
            Key.D1 => 1,
            Key.D2 => 2,
            Key.D3 => 3,
            Key.D4 => 4,
            Key.D5 => 5,
            _ => -1
        };
        return rating >= 0;
    }

    internal static bool IsSelectShortcut(
        Key key,
        ModifierKeys modifiers) =>
        key == Key.C && modifiers == ModifierKeys.None;

    internal static bool IsCopyImageShortcut(
        Key key,
        ModifierKeys modifiers) =>
        key == Key.C && modifiers == ModifierKeys.Control;

    internal static bool IsQuickFileCopyShortcut(
        Key key,
        ModifierKeys modifiers) =>
        key == Key.C
        && modifiers == (ModifierKeys.Control | ModifierKeys.Shift);

    internal static bool IsPasteImageShortcut(
        Key key,
        ModifierKeys modifiers) =>
        key == Key.V && modifiers == ModifierKeys.Control;

    internal static bool IsSaveAsShortcut(
        Key key,
        ModifierKeys modifiers) =>
        key == Key.S
        && modifiers == (ModifierKeys.Control | ModifierKeys.Shift);

    internal static bool IsImgurUploadShortcut(
        Key key,
        ModifierKeys modifiers) =>
        key == Key.U && modifiers == ModifierKeys.Control;

    private bool TryGetPhotoNavigationCommand(
        Key key,
        out System.Windows.Input.ICommand command)
    {
        if (key is Key.Down or Key.Right or Key.PageDown)
        {
            command = GuardedNextCommand;
            return true;
        }

        if (key is Key.Up or Key.Left or Key.PageUp)
        {
            command = GuardedPreviousCommand;
            return true;
        }

        command = null!;
        return false;
    }

    private void OnViewModelPropertyChanged(
        object? sender,
        PropertyChangedEventArgs eventArgs)
    {
        if (eventArgs.PropertyName == nameof(MainViewModel.CurrentFolder))
        {
            ResetCatalogScrollPosition();
            return;
        }

        if (eventArgs.PropertyName == nameof(MainViewModel.SelectedPhoto))
        {
            copySelectionCancellation?.Cancel();
            return;
        }

        if (eventArgs.PropertyName != nameof(MainViewModel.IsEditorMode)
            && eventArgs.PropertyName != nameof(MainViewModel.IsFullscreenMode))
        {
            return;
        }

        if (eventArgs.PropertyName == nameof(MainViewModel.IsFullscreenMode))
        {
            ApplyFullscreenState();
        }

        if (eventArgs.PropertyName == nameof(MainViewModel.IsEditorMode)
            && viewModel.IsEditorMode)
        {
            viewModel.SelectedPhoto?.BeginEditorSession();
        }

        if (eventArgs.PropertyName == nameof(MainViewModel.IsEditorMode)
            && !viewModel.IsEditorMode)
        {
            PreviewViewer.EndSelectionMode();
            _ = RefreshCatalogAfterEditorExitAsync();
        }

        ApplyModeLayout();
        Dispatcher.BeginInvoke(
            () =>
            {
                if (viewModel.IsEditorMode || viewModel.IsFullscreenMode)
                {
                    PreviewViewer.Focus();
                }
                else
                {
                    PhotoList.Focus();
                    if (viewModel.SelectedPhoto is not null)
                    {
                        PhotoList.ScrollIntoView(viewModel.SelectedPhoto);
                    }
                }
            });
    }

    private async Task RefreshCatalogAfterEditorExitAsync()
    {
        if (!refreshCatalogAfterEditorExit
            || viewModel.CurrentFolder is not { } folder)
        {
            return;
        }

        refreshCatalogAfterEditorExit = false;
        await viewModel.LoadFolderAsync(
            folder,
            CancellationToken.None);
    }

    private void ResetCatalogScrollPosition()
    {
        if (FindScrollViewerOrDefault(PhotoList) is { } scrollViewer)
        {
            scrollViewer.ScrollToTop();
        }

        _ = Dispatcher.BeginInvoke(
            DispatcherPriority.Loaded,
            () => FindScrollViewerOrDefault(PhotoList)?.ScrollToTop());
    }

    private void ApplyFullscreenState()
    {
        if (viewModel.IsFullscreenMode)
        {
            layoutBeforeFullscreen = CaptureWindowLayout(forcePaneCapture: true);
            windowStyleBeforeFullscreen = WindowStyle;
            resizeModeBeforeFullscreen = ResizeMode;
            WindowStyle = WindowStyle.None;
            ResizeMode = ResizeMode.NoResize;
            WindowState = WindowState.Maximized;
            return;
        }

        if (layoutBeforeFullscreen is not { } layout)
        {
            return;
        }

        WindowState = WindowState.Normal;
        WindowStyle = windowStyleBeforeFullscreen;
        ResizeMode = resizeModeBeforeFullscreen;
        layoutBeforeFullscreen = null;
        ApplyWindowLayout(layout);
    }

    private void OnPaneSplitterDragCompleted(
        object sender,
        DragCompletedEventArgs eventArgs)
    {
        CapturePaneWidths();
        ScheduleLayoutSave();
    }

    private void ApplyWindowLayout(WindowLayoutState state)
    {
        var virtualWidth = Math.Max(MinWidth, SystemParameters.VirtualScreenWidth);
        var virtualHeight = Math.Max(MinHeight, SystemParameters.VirtualScreenHeight);
        var width = ClampFinite(state.Width, MinWidth, virtualWidth, Width);
        var height = ClampFinite(state.Height, MinHeight, virtualHeight, Height);
        var left = ClampFinite(
            state.Left,
            SystemParameters.VirtualScreenLeft - width + MinimumVisibleWindowWidth,
            SystemParameters.VirtualScreenLeft
            + SystemParameters.VirtualScreenWidth
            - MinimumVisibleWindowWidth,
            SystemParameters.VirtualScreenLeft);
        var top = ClampFinite(
            state.Top,
            SystemParameters.VirtualScreenTop,
            SystemParameters.VirtualScreenTop
            + SystemParameters.VirtualScreenHeight
            - VisibleTitleBarHeight,
            SystemParameters.VirtualScreenTop);

        Width = width;
        Height = height;
        Left = left;
        Top = top;
        WindowStartupLocation = WindowStartupLocation.Manual;

        var maximumNavigatorWidth = Math.Max(
            NavigatorMinWidth,
            width - CatalogMinWidth - ViewerMinWidth - (2 * SplitterWidth));
        navigatorPaneWidth = ClampFinite(
            state.NavigatorPaneWidth,
            NavigatorMinWidth,
            maximumNavigatorWidth,
            DefaultNavigatorWidth);
        var maximumCatalogWidth = Math.Max(
            CatalogMinWidth,
            width - navigatorPaneWidth - ViewerMinWidth - (2 * SplitterWidth));
        catalogPaneWidth = ClampFinite(
            state.CatalogPaneWidth,
            CatalogMinWidth,
            maximumCatalogWidth,
            DefaultCatalogWidth);
        ApplyModeLayout();

        WindowState = state.State == WindowState.Maximized
            ? WindowState.Maximized
            : WindowState.Normal;
    }

    private void ApplyModeLayout()
    {
        if (viewModel.IsEditorMode || viewModel.IsFullscreenMode)
        {
            CapturePaneWidths(force: true);
            NavigatorColumn.MinWidth = 0;
            CatalogColumn.MinWidth = 0;
            NavigatorColumn.Width = new GridLength(0);
            NavigatorSplitterColumn.Width = new GridLength(0);
            CatalogColumn.Width = new GridLength(0);
            CatalogSplitterColumn.Width = new GridLength(0);
            PreviewViewer.FitToViewport();
            return;
        }

        NavigatorColumn.MinWidth = NavigatorMinWidth;
        CatalogColumn.MinWidth = CatalogMinWidth;
        NavigatorColumn.Width = new GridLength(navigatorPaneWidth);
        NavigatorSplitterColumn.Width = new GridLength(SplitterWidth);
        CatalogColumn.Width = new GridLength(catalogPaneWidth);
        CatalogSplitterColumn.Width = new GridLength(SplitterWidth);
    }

    private void CapturePaneWidths(bool force = false)
    {
        if (!force && (viewModel.IsEditorMode || viewModel.IsFullscreenMode))
        {
            return;
        }

        if (NavigatorColumn.ActualWidth >= NavigatorMinWidth)
        {
            navigatorPaneWidth = NavigatorColumn.ActualWidth;
        }

        if (CatalogColumn.ActualWidth >= CatalogMinWidth)
        {
            catalogPaneWidth = CatalogColumn.ActualWidth;
        }
    }

    private void ScheduleLayoutSave()
    {
        if (!isLayoutRestored
            || isLayoutClosePending
            || isClosingAfterLayoutSave
            || viewModel.IsFullscreenMode)
        {
            return;
        }

        layoutSaveTimer.Stop();
        layoutSaveTimer.Start();
    }

    private async void OnLayoutSaveTimerTick(object? sender, EventArgs eventArgs)
    {
        layoutSaveTimer.Stop();
        await SaveLayoutAsync();
    }

    private async Task SaveLayoutAsync()
    {
        if (!isLayoutRestored)
        {
            return;
        }

        var state = CaptureWindowLayout();
        var json = JsonSerializer.Serialize(state);
        await layoutSaveGate.WaitAsync();
        try
        {
            await catalog.SetSettingAsync(
                WindowLayoutSetting,
                json);
        }
        catch
        {
            // Window-state persistence is best effort and must not interrupt work.
        }
        finally
        {
            layoutSaveGate.Release();
        }
    }

    private WindowLayoutState CaptureWindowLayout(bool forcePaneCapture = false)
    {
        if (viewModel.IsFullscreenMode && layoutBeforeFullscreen is not null)
        {
            return layoutBeforeFullscreen;
        }

        CapturePaneWidths(forcePaneCapture);
        var bounds = WindowState == WindowState.Normal
            ? new Rect(Left, Top, ActualWidth, ActualHeight)
            : RestoreBounds;
        var width = double.IsFinite(bounds.Width) && bounds.Width > 0
            ? bounds.Width
            : Width;
        var height = double.IsFinite(bounds.Height) && bounds.Height > 0
            ? bounds.Height
            : Height;

        return new WindowLayoutState(
            bounds.Left,
            bounds.Top,
            width,
            height,
            WindowState == WindowState.Maximized
                ? WindowState.Maximized
                : WindowState.Normal,
            navigatorPaneWidth,
            catalogPaneWidth);
    }

    private async void OnWindowClosing(
        object? sender,
        CancelEventArgs eventArgs)
    {
        if (isClosingAfterLayoutSave || !IsVisible || !isLayoutRestored)
        {
            return;
        }

        if (viewModel.IsEditorMode
            && viewModel.SelectedPhoto?.IsEditorDirty == true)
        {
            eventArgs.Cancel = true;
            if (isEditorExitPromptActive)
            {
                return;
            }

            isEditorExitPromptActive = true;
            var canClose = await ConfirmEditorExitAsync();
            isEditorExitPromptActive = false;
            if (canClose)
            {
                _ = Dispatcher.BeginInvoke(Close);
            }

            return;
        }

        if (isLayoutClosePending)
        {
            eventArgs.Cancel = true;
            return;
        }

        eventArgs.Cancel = true;
        isLayoutClosePending = true;
        layoutSaveTimer.Stop();
        await SaveLayoutAsync();
        try
        {
            await viewModel.SaveSessionAsync();
        }
        catch
        {
            // Session persistence is best effort and must not prevent closing.
        }
        isClosingAfterLayoutSave = true;
        _ = Dispatcher.BeginInvoke(
            () =>
            {
                if (IsVisible)
                {
                    Close();
                }
            });
    }

    private void OnWindowClosed(object? sender, EventArgs eventArgs)
    {
        layoutSaveTimer.Stop();
        copySelectionCancellation?.Cancel();
        copySelectionCancellation?.Dispose();
        imgurUploadCancellation?.Cancel();
        imgurUploadCancellation?.Dispose();
        viewModel.PropertyChanged -= OnViewModelPropertyChanged;
        layoutSaveGate.Dispose();
    }

    private void OnSourceInitialized(object? sender, EventArgs eventArgs)
    {
        if (SystemParameters.HighContrast)
        {
            return;
        }

        var handle = new WindowInteropHelper(this).Handle;
        var enabled = 1;
        if (DwmSetWindowAttribute(
                handle,
                DwmUseImmersiveDarkMode,
                ref enabled,
                sizeof(int)) != 0)
        {
            DwmSetWindowAttribute(
                handle,
                DwmUseImmersiveDarkModeLegacy,
                ref enabled,
                sizeof(int));
        }

        var caption = ToColorRef(0x11, 0x13, 0x18);
        var text = ToColorRef(0xF2, 0xF4, 0xF8);
        var border = ToColorRef(0x30, 0x35, 0x41);
        DwmSetWindowAttribute(handle, DwmCaptionColor, ref caption, sizeof(int));
        DwmSetWindowAttribute(handle, DwmTextColor, ref text, sizeof(int));
        DwmSetWindowAttribute(handle, DwmBorderColor, ref border, sizeof(int));
    }

    private static int ToColorRef(byte red, byte green, byte blue) =>
        red | (green << 8) | (blue << 16);

    private static double ClampFinite(
        double value,
        double minimum,
        double maximum,
        double fallback) =>
        double.IsFinite(value)
            ? Math.Clamp(value, minimum, maximum)
            : Math.Clamp(fallback, minimum, maximum);

    private static ScrollBar FindVerticalScrollBar(DependencyObject root)
    {
        for (var index = 0;
             index < VisualTreeHelper.GetChildrenCount(root);
             index++)
        {
            var child = VisualTreeHelper.GetChild(root, index);
            if (child is ScrollBar
                {
                    Orientation: Orientation.Vertical
                } scrollBar)
            {
                return scrollBar;
            }

            try
            {
                return FindVerticalScrollBar(child);
            }
            catch (InvalidOperationException)
            {
            }
        }

        throw new InvalidOperationException(
            "A vertical scrollbar was not created for the pane.");
    }

    private static ScrollViewer FindScrollViewer(DependencyObject root) =>
        FindScrollViewerOrDefault(root)
        ?? throw new InvalidOperationException(
            "A scroll viewer was not created for the catalogue.");

    private static ScrollViewer? FindScrollViewerOrDefault(DependencyObject root)
    {
        if (root is ScrollViewer scrollViewer)
        {
            return scrollViewer;
        }

        for (var index = 0;
             index < VisualTreeHelper.GetChildrenCount(root);
             index++)
        {
            var match = FindScrollViewerOrDefault(
                VisualTreeHelper.GetChild(root, index));
            if (match is not null)
            {
                return match;
            }
        }

        return null;
    }

    private static TextBlock FindTextBlock(
        DependencyObject root,
        string expectedText)
    {
        for (var index = 0;
             index < VisualTreeHelper.GetChildrenCount(root);
             index++)
        {
            var child = VisualTreeHelper.GetChild(root, index);
            if (child is TextBlock textBlock
                && string.Equals(
                    textBlock.Text,
                    expectedText,
                    StringComparison.Ordinal))
            {
                return textBlock;
            }

            try
            {
                return FindTextBlock(child, expectedText);
            }
            catch (InvalidOperationException)
            {
            }
        }

        throw new InvalidOperationException(
            $"A text block containing '{expectedText}' was not found.");
    }

    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(
        nint windowHandle,
        int attribute,
        ref int attributeValue,
        int attributeSize);

    private sealed record WindowLayoutState(
        double Left,
        double Top,
        double Width,
        double Height,
        WindowState State,
        double NavigatorPaneWidth,
        double CatalogPaneWidth);
}

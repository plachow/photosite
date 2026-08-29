using System.Collections.Specialized;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Animation;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using CommunityToolkit.Mvvm.Input;
using Microsoft.Win32;
using PhotoSite.Controls;
using PhotoSite.Dialogs;
using PhotoSite.Domain;
using PhotoSite.Infrastructure;
using PhotoSite.Services;
using PhotoSite.Services.Batch;
using PhotoSite.ViewModels;
using ShapePath = System.Windows.Shapes.Path;

namespace PhotoSite;

public partial class MainWindow : Window
{
    private const string WindowLayoutSetting = "window_layout_v1";
    private const string LastCopyDestinationSetting = "last_copy_destination";
    private const string LastMoveDestinationSetting = "last_move_destination";
    private const string LastBatchDestinationSetting = "last_batch_destination";
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
    private readonly SplitPaneController managerInfoSplit;
    private readonly SplitPaneController editorInfoSplit;
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
    private string? lastMoveDestination;
    private string? lastBatchDestination;

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
        managerInfoSplit = new SplitPaneController(
            ManagerPreviewRow,
            ManagerInfoSplitterRow,
            ManagerInfoRow,
            ManagerInfoSplitter,
            InfoPanelToggle,
            ManagerInfoExpandButton,
            ManagerInfoCollapseButton);
        managerInfoSplit.RatioChanged += ScheduleLayoutSave;
        editorInfoSplit = new SplitPaneController(
            EditorTabsRow,
            EditorInfoSplitterRow,
            EditorInfoRow,
            EditorInfoSplitter,
            EditorInfoToggle,
            EditorInfoExpandButton,
            EditorInfoCollapseButton);
        editorInfoSplit.RatioChanged += ScheduleLayoutSave;
        BuildLabelFilters();
        BuildLabelPicker();
        InitializeEditorPanel();
        InitializeAnnotationTools();
        layoutSaveTimer = new DispatcherTimer
        {
            Interval = TimeSpan.FromMilliseconds(350)
        };
        layoutSaveTimer.Tick += OnLayoutSaveTimerTick;
        DataContext = viewModel;
        DarkWindowChrome.Apply(this);
        LocationChanged += (_, _) => ScheduleLayoutSave();
        SizeChanged += (_, _) => ScheduleLayoutSave();
        StateChanged += (_, _) => ScheduleLayoutSave();
        Closing += OnWindowClosing;
        viewModel.PropertyChanged += OnViewModelPropertyChanged;
        viewModel.SelectionRevealRequested += OnSelectionRevealRequested;
        Closed += OnWindowClosed;
    }

    private void OnSelectionRevealRequested() =>
        Dispatcher.BeginInvoke(
            DispatcherPriority.Loaded,
            () =>
            {
                if (viewModel.SelectedPhoto is { } photo)
                {
                    PhotoList.ScrollIntoView(photo);
                }
            });

    public IAsyncRelayCommand GuardedPreviousCommand { get; }

    public IAsyncRelayCommand GuardedNextCommand { get; }

    public IAsyncRelayCommand GuardedToggleEditorCommand { get; }

    internal async Task<bool> PrepareForUpdateRestartAsync()
    {
        if (viewModel.EditorTabs.Any(tab => tab.IsEditorDirty)
            || (viewModel.IsEditorMode
                && viewModel.SelectedPhoto?.IsEditorDirty == true))
        {
            if (isEditorExitPromptActive)
            {
                return false;
            }

            isEditorExitPromptActive = true;
            var canRestart = await ConfirmAllEditorTabsAsync();
            isEditorExitPromptActive = false;
            if (!canRestart)
            {
                return false;
            }
        }

        layoutSaveTimer.Stop();
        await SaveLayoutAsync();
        try
        {
            await viewModel.SaveSessionAsync();
        }
        catch
        {
            // Session persistence is best effort and must not block an update.
        }

        copySelectionCancellation?.Cancel();
        imgurUploadCancellation?.Cancel();
        return true;
    }

    public async Task RestoreLayoutAsync()
    {
        try
        {
            if (bool.TryParse(
                    await catalog.GetSettingAsync(ShowFacesSetting),
                    out var showFaces)
                && showFaces)
            {
                _ = Dispatcher.BeginInvoke(() =>
                {
                    FacesOverlayButton.IsChecked = true;
                    PreviewViewer.ShowFaceOverlays = true;
                });
            }

            await LoadPersonFilterChoicesAsync();

            var savedCopyDestination = await catalog.GetSettingAsync(
                LastCopyDestinationSetting);
            if (!string.IsNullOrWhiteSpace(savedCopyDestination)
                && Directory.Exists(savedCopyDestination))
            {
                lastCopyDestination = Path.GetFullPath(savedCopyDestination);
            }

            var savedMoveDestination = await catalog.GetSettingAsync(
                LastMoveDestinationSetting);
            if (!string.IsNullOrWhiteSpace(savedMoveDestination)
                && Directory.Exists(savedMoveDestination))
            {
                lastMoveDestination = Path.GetFullPath(savedMoveDestination);
            }

            var savedBatchDestination = await catalog.GetSettingAsync(
                LastBatchDestinationSetting);
            if (!string.IsNullOrWhiteSpace(savedBatchDestination)
                && Directory.Exists(savedBatchDestination))
            {
                lastBatchDestination = Path.GetFullPath(savedBatchDestination);
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
        if (items.Length != 16
            || items.Any(item => item.Icon is null
                                 || !ReferenceEquals(item.Style, itemStyle))
            || separators.Length != 6
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

        if (PhotoList.SelectionMode != SelectionMode.Extended)
        {
            throw new InvalidOperationException(
                "The Manager gallery must use Explorer-style extended selection.");
        }

        var originalSelection = viewModel.SelectedPhoto;
        PhotoList.SelectAll();
        if (PhotoList.SelectedItems.Count != PhotoList.Items.Count)
        {
            throw new InvalidOperationException(
                "Select All must select every visible Manager thumbnail.");
        }

        var contextPhoto = PhotoList.Items
            .OfType<PhotoItemViewModel>()
            .First(photo => !ReferenceEquals(photo, viewModel.SelectedPhoto));
        if (PhotoList.ItemContainerGenerator.ContainerFromItem(contextPhoto)
                is not ListBoxItem contextItem)
        {
            throw new InvalidOperationException(
                "The context-click regression test needs a realized thumbnail.");
        }

        OnPhotoListPreviewMouseDown(
            PhotoList,
            new MouseButtonEventArgs(
                Mouse.PrimaryDevice,
                Environment.TickCount,
                MouseButton.Right)
            {
                RoutedEvent = Mouse.PreviewMouseDownEvent,
                Source = contextItem
            });
        if (PhotoList.SelectedItems.Count != PhotoList.Items.Count)
        {
            throw new InvalidOperationException(
                "Right-clicking a selected thumbnail must preserve the "
                + "Manager multi-selection.");
        }

        PhotoList.UnselectAll();
        OnPhotoListPreviewMouseDown(
            PhotoList,
            new MouseButtonEventArgs(
                Mouse.PrimaryDevice,
                Environment.TickCount,
                MouseButton.Right)
            {
                RoutedEvent = Mouse.PreviewMouseDownEvent,
                Source = contextItem
            });
        if (PhotoList.SelectedItems.Count != 1
            || !PhotoList.SelectedItems.Contains(contextPhoto)
            || !ReferenceEquals(viewModel.SelectedPhoto, contextPhoto))
        {
            throw new InvalidOperationException(
                "Right-clicking an unselected thumbnail must select only it.");
        }

        PhotoList.UnselectAll();
        viewModel.SelectedPhoto = originalSelection;

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
            || FindName("SelectionTotalsSection") is not FrameworkElement
            || FindName("GalleryTotalsSection") is not FrameworkElement
                totalsSection)
        {
            throw new InvalidOperationException(
                "Status sections must use stable widths, visible vector icons, "
                + "and explicit separators.");
        }

        var originalFlatVisibility = FlatFolderIcon.Visibility;
        var originalRecursiveVisibility = RecursiveTreeIcon.Visibility;
        var initialTotalsPosition = totalsSection.TranslatePoint(
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
            var toggledTotalsPosition = totalsSection.TranslatePoint(
                new Point(),
                this).X;
            if (Math.Abs(toggledTotalsPosition - initialTotalsPosition) > 0.1)
            {
                throw new InvalidOperationException(
                    "Switching between flat and recursive icons must not move "
                    + "the gallery and selection totals.");
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

    internal void ValidatePastedImageCropForSmokeTest(BitmapSource bitmap)
    {
        var originalSelection = viewModel.SelectedPhoto;
        var crop = new CropRegion(0.2, 0.25, 0.5, 0.5);
        var pasted = viewModel.OpenPastedImage(bitmap);
        UpdateLayout();
        PreviewViewer.IsSelectionMode = true;
        PreviewViewer.SetSelectionForSmokeTest(crop);
        var selectedBeforeAction = viewModel.SelectedPhoto;
        var selectionBeforeAction = PreviewViewer.SelectionRegion;
        CropSelectionButton.RaiseEvent(
            new RoutedEventArgs(Button.ClickEvent));
        UpdateLayout();

        string? failure = null;
        if (!ReferenceEquals(selectedBeforeAction, pasted))
        {
            failure = "The pasted document was not selected before Crop.";
        }
        else if (selectionBeforeAction is not { } appliedSelection
                 || !CropRegionsAreClose(appliedSelection, crop))
        {
            failure =
                $"Crop received selection {selectionBeforeAction?.ToString() ?? "null"}.";
        }
        else if (pasted.EditRecipe.Crop is not { } documentCrop
                 || !CropRegionsAreClose(documentCrop, appliedSelection))
        {
            failure = "The Crop action did not update the document recipe.";
        }
        else if (PreviewViewer.EditRecipe.Crop is not { } boundCrop
                 || !CropRegionsAreClose(boundCrop, appliedSelection))
        {
            failure =
                "The viewer binding did not receive the document crop recipe.";
        }
        else if (PreviewViewer.DisplayedRecipeForSmokeTest.Crop
                     is not { } displayedCrop
                 || !CropRegionsAreClose(displayedCrop, appliedSelection))
        {
            failure =
                "The pasted bitmap renderer did not adopt the crop recipe.";
        }
        else if (PreviewViewer.HasSelection)
        {
            failure = "The applied crop selection was not cleared.";
        }

        pasted.DiscardEditorSession();
        viewModel.ShowManagerCommand.Execute(null);
        if (!ReferenceEquals(viewModel.SelectedPhoto, originalSelection))
        {
            failure ??=
                "Leaving the crop repro did not restore the catalogue selection.";
        }

        if (failure is not null)
        {
            throw new InvalidOperationException(failure);
        }
    }

    private static bool CropRegionsAreClose(
        CropRegion first,
        CropRegion second) =>
        Math.Abs(first.X - second.X) < 0.0000001
        && Math.Abs(first.Y - second.Y) < 0.0000001
        && Math.Abs(first.Width - second.Width) < 0.0000001
        && Math.Abs(first.Height - second.Height) < 0.0000001;

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

    private void OnPhotoListSelectionChanged(
        object sender,
        SelectionChangedEventArgs eventArgs)
    {
        // The multi-selection belongs to the list, so the footer tally has to
        // be pushed into the view model whenever it moves.
        viewModel.UpdateSelectionTotals(
            PhotoList.SelectedItems.OfType<PhotoItemViewModel>().ToArray());
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
        if (eventArgs.OriginalSource is not DependencyObject source
            || ItemsControl.ContainerFromElement(PhotoList, source)
                is not ListBoxItem item
            || item.DataContext is not PhotoItemViewModel photo)
        {
            return;
        }

        if (eventArgs.ChangedButton == MouseButton.Right)
        {
            // Explorer keeps a multi-selection when its context menu is opened
            // over one of the selected items, but selects an unselected item by
            // itself before showing that menu.
            if (!item.IsSelected)
            {
                PhotoList.UnselectAll();
                item.IsSelected = true;
                viewModel.SelectedPhoto = photo;
            }

            item.Focus();
            return;
        }

        if (eventArgs.ChangedButton != MouseButton.Middle)
        {
            return;
        }

        PhotoList.UnselectAll();
        item.IsSelected = true;
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
        var items = contextMenu.Items.OfType<MenuItem>().ToArray();
        var lastDestinationItem = items
            .Single(item => Equals(item.Tag, "LastCopyDestination"));
        lastDestinationItem.Visibility = hasLastDestination
            ? Visibility.Visible
            : Visibility.Collapsed;
        if (!hasLastDestination)
        {
            lastDestinationItem.ToolTip = null;
        }
        else
        {
            var displayName = GetDestinationDisplayName(lastCopyDestination!);
            lastDestinationItem.Header = $"Copy to “{displayName}”";
            lastDestinationItem.ToolTip = lastCopyDestination;
        }

        var selectedCount = GetSelectedManagerPhotos().Count;
        SetMenuHeader(
            items,
            "CopyFiles",
            selectedCount == 1 ? "Copy file" : $"Copy {selectedCount:N0} files");
        SetMenuHeader(
            items,
            "CopyTo",
            selectedCount == 1 ? "Copy to…" : $"Copy {selectedCount:N0} files to…");
        SetMenuHeader(
            items,
            "MoveTo",
            selectedCount == 1 ? "Move to…" : $"Move {selectedCount:N0} files to…");
        SetMenuHeader(
            items,
            "DeleteFiles",
            selectedCount == 1
                ? "Move to Recycle Bin"
                : $"Move {selectedCount:N0} files to Recycle Bin");
        SetMenuHeader(
            items,
            "BatchConvert",
            selectedCount == 1
                ? "Batch convert…"
                : $"Batch convert {selectedCount:N0} photos…");
        SetMenuHeader(
            items,
            "DuplicateFiles",
            selectedCount == 1
                ? "Duplicate"
                : $"Duplicate {selectedCount:N0} files");
        SetMenuHeader(
            items,
            "AiDescribe",
            selectedCount == 1
                ? "Describe with AI…"
                : $"Describe {selectedCount:N0} photos with AI…");
        SetMenuHeader(
            items,
            "FindPeople",
            selectedCount == 1
                ? "Find people…"
                : $"Find people in {selectedCount:N0} photos…");
        // Renaming is a one-file operation; a bulk rename is what the batch
        // dialog is for, and it does it far better than a prompt could.
        items.Single(item => Equals(item.Tag, "RenameFile")).IsEnabled =
            selectedCount == 1;
        items.Single(item => Equals(item.Tag, "PasteFiles")).IsEnabled =
            HasClipboardFiles();
    }

    private void OnCopyFileNameClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        var photos = GetContextPhotos(sender);
        if (photos.Count > 0)
        {
            SetClipboardText(
                string.Join(Environment.NewLine, photos.Select(photo => photo.FileName)),
                photos.Count == 1
                    ? $"Copied filename: {photos[0].FileName}"
                    : $"Copied {photos.Count:N0} filenames");
        }
    }

    private void OnCopyFullPathClick(object sender, RoutedEventArgs eventArgs)
    {
        var photos = GetContextPhotos(sender);
        if (photos.Count > 0)
        {
            SetClipboardText(
                string.Join(Environment.NewLine, photos.Select(photo => photo.Path)),
                photos.Count == 1
                    ? $"Copied full path: {photos[0].FileName}"
                    : $"Copied {photos.Count:N0} full paths");
        }
    }

    private void OnCopyFilesClick(object sender, RoutedEventArgs eventArgs) =>
        CopyFilesToClipboard(GetContextPhotos(sender));

    private async void OnPasteFilesClick(
        object sender,
        RoutedEventArgs eventArgs) =>
        await PasteFilesFromClipboardAsync();

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

    private async void OnCopyFilesToClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        await ChooseAndTransferFilesAsync(
            GetContextPhotos(sender),
            PhotoFileTransferMode.Copy);
    }

    private async void OnMoveFilesToClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        await ChooseAndTransferFilesAsync(
            GetContextPhotos(sender),
            PhotoFileTransferMode.Move);
    }

    private async void OnCopyFileToLastDestinationClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        await CopyFilesToLastDestinationAsync(GetContextPhotos(sender));
    }

    private async void OnDeleteFilesClick(
        object sender,
        RoutedEventArgs eventArgs) =>
        await DeleteSelectedFilesAsync(GetContextPhotos(sender));

    private async void OnBatchConvertClick(
        object sender,
        RoutedEventArgs eventArgs) =>
        await RunBatchConversionAsync(GetContextPhotos(sender));

    private void OnAiDescribeClick(object sender, RoutedEventArgs eventArgs) =>
        RunAiTagging(GetContextPhotos(sender));

    private void RunAiTagging(IReadOnlyCollection<PhotoItemViewModel> photos)
    {
        var targets = photos
            .Where(photo => !photo.IsTransient && File.Exists(photo.Path))
            .DistinctBy(photo => photo.Path, StringComparer.OrdinalIgnoreCase)
            .ToArray();
        if (targets.Length == 0)
        {
            viewModel.ReportStatus("Select photos to describe first");
            return;
        }

        var dialog = new AiTagDialog(
            targets,
            App.Services.OllamaVision,
            catalog,
            App.Services.Previews,
            App.Services.Geolocator)
        {
            Owner = this
        };
        dialog.ShowDialog();

        if (dialog.DescribedCount > 0)
        {
            viewModel.ReportStatus(
                dialog.DescribedCount == 1
                    ? "AI described 1 photo"
                    : $"AI described {dialog.DescribedCount:N0} photos");
        }
    }

    private async Task RunBatchConversionAsync(
        IReadOnlyCollection<PhotoItemViewModel> photos)
    {
        var sources = photos
            .Where(photo => !photo.IsTransient && File.Exists(photo.Path))
            .DistinctBy(photo => photo.Path, StringComparer.OrdinalIgnoreCase)
            .Select(photo => new BatchSource(
                photo.Path,
                photo.EditRecipe,
                photo.TakenAtTicks,
                photo.Record.PixelWidth,
                photo.Record.PixelHeight))
            .ToArray();
        if (sources.Length == 0)
        {
            viewModel.ReportStatus("Select photos to convert first");
            return;
        }

        var dialog = new BatchDialog(
            sources,
            App.Services.BatchPresets,
            App.Services.Batch,
            lastBatchDestination
            ?? lastCopyDestination
            ?? viewModel.CurrentFolder)
        {
            Owner = this
        };
        dialog.ShowDialog();

        if (!dialog.DidWriteFiles)
        {
            return;
        }

        if (dialog.LastOutputDirectory is { } destination)
        {
            lastBatchDestination = destination;
            await PersistSettingAsync(LastBatchDestinationSetting, destination);
        }

        // Files written into the folder on screen have to appear in it.
        if (dialog.LastOutputDirectory is null
            || IsDestinationInCurrentCatalog(dialog.LastOutputDirectory))
        {
            await RefreshCurrentCatalogAsync();
        }

        viewModel.ReportStatus("Batch conversion finished");
    }

    private async Task PersistSettingAsync(string key, string value)
    {
        try
        {
            await catalog.SetSettingAsync(key, value);
        }
        catch (Exception exception)
        {
            viewModel.ReportStatus(
                $"The setting could not be saved: {exception.Message}");
        }
    }

    private static void SetMenuHeader(
        IEnumerable<MenuItem> items,
        string tag,
        string header) =>
        items.Single(item => Equals(item.Tag, tag)).Header = header;

    private IReadOnlyList<PhotoItemViewModel> GetContextPhotos(object sender)
    {
        if (sender is not MenuItem
            {
                DataContext: PhotoItemViewModel contextPhoto
            })
        {
            return [];
        }

        var selected = GetSelectedManagerPhotos();
        return selected.Contains(contextPhoto) ? selected : [contextPhoto];
    }

    private IReadOnlyList<PhotoItemViewModel> GetSelectedManagerPhotos()
    {
        var selected = PhotoList.SelectedItems
            .OfType<PhotoItemViewModel>()
            .Where(photo => !photo.IsTransient)
            .DistinctBy(photo => photo.Path, StringComparer.OrdinalIgnoreCase)
            .ToArray();
        if (selected.Length > 0)
        {
            return selected;
        }

        return viewModel.SelectedPhoto is { IsTransient: false } photo
            ? [photo]
            : [];
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

    private void CopyFilesToClipboard(
        IReadOnlyCollection<PhotoItemViewModel> photos)
    {
        var paths = photos
            .Select(photo => photo.Path)
            .Where(File.Exists)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToArray();
        if (paths.Length == 0)
        {
            viewModel.ReportStatus("No existing files are selected");
            return;
        }

        try
        {
            var data = CreateFileCopyDataObject(paths);
            Clipboard.SetDataObject(data, copy: true);
            viewModel.ReportStatus(
                paths.Length == 1
                    ? $"Copied file: {Path.GetFileName(paths[0])}"
                    : $"Copied {paths.Length:N0} files to the clipboard");
        }
        catch (ExternalException exception)
        {
            viewModel.ReportStatus(
                $"Cannot access the clipboard: {exception.Message}");
        }
    }

    internal static DataObject CreateFileCopyDataObject(
        IReadOnlyCollection<string> paths)
    {
        var files = new StringCollection();
        files.AddRange(paths.Select(Path.GetFullPath).ToArray());
        var data = new DataObject();
        data.SetFileDropList(files);
        data.SetData(
            "Preferred DropEffect",
            new MemoryStream(BitConverter.GetBytes(1)));
        return data;
    }

    private async Task PasteFilesFromClipboardAsync()
    {
        if (viewModel.CurrentFolder is not { } destinationDirectory)
        {
            viewModel.ReportStatus("Choose a destination folder first");
            return;
        }

        IReadOnlyList<string> paths;
        PhotoFileTransferMode mode;
        try
        {
            if (!Clipboard.ContainsFileDropList())
            {
                viewModel.ReportStatus("The clipboard does not contain files");
                return;
            }

            paths = Clipboard.GetFileDropList()
                .Cast<string>()
                .Where(File.Exists)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .ToArray();
            mode = GetClipboardDropEffect() == 2
                ? PhotoFileTransferMode.Move
                : PhotoFileTransferMode.Copy;
        }
        catch (ExternalException exception)
        {
            viewModel.ReportStatus(
                $"Cannot access the clipboard: {exception.Message}");
            return;
        }

        if (paths.Count == 0)
        {
            viewModel.ReportStatus(
                "The clipboard does not contain any existing files");
            return;
        }

        await TransferFilesAsync(
            paths,
            destinationDirectory,
            mode,
            forceCatalogRefresh: true);
    }

    private static int GetClipboardDropEffect()
    {
        var value = Clipboard.GetData("Preferred DropEffect");
        return value switch
        {
            MemoryStream stream when stream.Length >= sizeof(int) =>
                ReadDropEffect(stream),
            byte[] bytes when bytes.Length >= sizeof(int) =>
                BitConverter.ToInt32(bytes, 0),
            _ => 1
        };
    }

    private static int ReadDropEffect(MemoryStream stream)
    {
        var position = stream.Position;
        try
        {
            stream.Position = 0;
            var bytes = new byte[sizeof(int)];
            return stream.Read(bytes, 0, bytes.Length) == bytes.Length
                ? BitConverter.ToInt32(bytes, 0)
                : 1;
        }
        finally
        {
            stream.Position = position;
        }
    }

    private static bool HasClipboardFiles()
    {
        try
        {
            return Clipboard.ContainsFileDropList();
        }
        catch (ExternalException)
        {
            return false;
        }
    }

    private async Task ChooseAndTransferFilesAsync(
        IReadOnlyCollection<PhotoItemViewModel> photos,
        PhotoFileTransferMode mode)
    {
        var paths = photos
            .Select(photo => photo.Path)
            .Where(File.Exists)
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToArray();
        if (paths.Length == 0)
        {
            viewModel.ReportStatus("No existing files are selected");
            return;
        }

        var rememberedDestination = mode == PhotoFileTransferMode.Copy
            ? lastCopyDestination
            : lastMoveDestination;
        var initialDestination = Directory.Exists(rememberedDestination)
            ? rememberedDestination
            : Path.GetDirectoryName(paths[0]);
        var dialog = new FileDestinationDialog(
            mode,
            paths.Length,
            initialDestination)
        {
            Owner = this
        };
        if (dialog.ShowDialog() != true
            || dialog.DestinationDirectory is not { } destinationDirectory)
        {
            return;
        }

        await RememberDestinationAsync(mode, destinationDirectory);
        await TransferFilesAsync(
            paths,
            destinationDirectory,
            mode,
            forceCatalogRefresh: mode == PhotoFileTransferMode.Move);
    }

    private async Task CopyFilesToLastDestinationAsync(
        IReadOnlyCollection<PhotoItemViewModel> photos)
    {
        if (!HasLastCopyDestination())
        {
            return;
        }

        await TransferFilesAsync(
            photos.Select(photo => photo.Path).ToArray(),
            lastCopyDestination!,
            PhotoFileTransferMode.Copy,
            forceCatalogRefresh: false);
    }

    private async Task TransferFilesAsync(
        IReadOnlyCollection<string> sourcePaths,
        string destinationDirectory,
        PhotoFileTransferMode mode,
        bool forceCatalogRefresh)
    {
        if (!Directory.Exists(destinationDirectory))
        {
            viewModel.ReportStatus(
                $"Destination folder not found: {destinationDirectory}");
            return;
        }

        var verb = mode == PhotoFileTransferMode.Copy ? "Copying" : "Moving";
        var completed = 0;
        var skipped = 0;
        var mayHaveChangedFiles = false;
        var failures = new List<string>();
        var paths = sourcePaths
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToArray();
        viewModel.ReportStatus($"{verb} {paths.Length:N0} files…");

        foreach (var sourcePath in paths)
        {
            if (!File.Exists(sourcePath))
            {
                failures.Add($"{Path.GetFileName(sourcePath)}: file not found");
                continue;
            }

            var plan = PhotoFileOperations.Plan(
                sourcePath,
                destinationDirectory,
                mode);
            if (plan.IsNoOp)
            {
                skipped++;
                continue;
            }

            var overwrite = false;
            if (plan.ExistingDestinationPaths.Count > 0)
            {
                var conflictingPath = plan.ExistingDestinationPaths[0];
                var choice = MessageBox.Show(
                    this,
                    $"{conflictingPath} already exists.\n\nReplace it?\n\n"
                    + "Yes: replace · No: skip · Cancel: stop",
                    "Replace existing file?",
                    MessageBoxButton.YesNoCancel,
                    MessageBoxImage.Warning,
                    MessageBoxResult.No);
                if (choice == MessageBoxResult.Cancel)
                {
                    break;
                }

                if (choice == MessageBoxResult.No)
                {
                    skipped++;
                    continue;
                }

                overwrite = true;
            }

            try
            {
                mayHaveChangedFiles = true;
                await PhotoFileOperations.ExecuteAsync(plan, mode, overwrite);
                completed++;
            }
            catch (Exception exception) when (
                exception is IOException
                    or UnauthorizedAccessException
                    or NotSupportedException
                    or ArgumentException)
            {
                failures.Add(
                    $"{Path.GetFileName(sourcePath)}: {exception.Message}");
            }
        }

        if (mayHaveChangedFiles
            && (forceCatalogRefresh
                || IsDestinationInCurrentCatalog(destinationDirectory)))
        {
            await RefreshCurrentCatalogAsync();
        }

        var operation = mode == PhotoFileTransferMode.Copy ? "Copied" : "Moved";
        var status = $"{operation} {completed:N0} of {paths.Length:N0} files";
        if (skipped > 0)
        {
            status += $" · {skipped:N0} skipped";
        }

        if (failures.Count > 0)
        {
            status += $" · {failures.Count:N0} failed";
            MessageBox.Show(
                this,
                string.Join(Environment.NewLine, failures.Take(8))
                + (failures.Count > 8
                    ? $"\n…and {failures.Count - 8:N0} more"
                    : string.Empty),
                $"{operation} with errors",
                MessageBoxButton.OK,
                MessageBoxImage.Error);
        }

        viewModel.ReportStatus(status);
    }

    private async Task DeleteSelectedFilesAsync(
        IReadOnlyCollection<PhotoItemViewModel> photos)
    {
        if (photos.Count == 0)
        {
            return;
        }

        await viewModel.DeletePhotosAsync(photos);
    }

    private async Task RefreshCurrentCatalogAsync()
    {
        if (viewModel.CurrentFolder is { } currentFolder)
        {
            await viewModel.LoadFolderAsync(
                currentFolder,
                CancellationToken.None);
        }
    }

    private bool IsDestinationInCurrentCatalog(string destinationDirectory)
    {
        if (viewModel.CurrentFolder is not { } currentFolder)
        {
            return false;
        }

        var relative = Path.GetRelativePath(
            Path.GetFullPath(currentFolder),
            Path.GetFullPath(destinationDirectory));
        if (relative == ".")
        {
            return true;
        }

        return viewModel.IncludeSubfolders
            && relative != ".."
            && !relative.StartsWith(
                ".." + Path.DirectorySeparatorChar,
                StringComparison.Ordinal)
            && !Path.IsPathRooted(relative);
    }

    private async Task RememberDestinationAsync(
        PhotoFileTransferMode mode,
        string destinationDirectory)
    {
        var fullPath = Path.GetFullPath(destinationDirectory);
        if (mode == PhotoFileTransferMode.Copy)
        {
            lastCopyDestination = fullPath;
        }
        else
        {
            lastMoveDestination = fullPath;
        }

        try
        {
            await catalog.SetSettingAsync(
                mode == PhotoFileTransferMode.Copy
                    ? LastCopyDestinationSetting
                    : LastMoveDestinationSetting,
                fullPath);
        }
        catch (Exception exception)
        {
            viewModel.ReportStatus(
                "The destination could not be remembered: "
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

    private async void OnClosePreviewClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (viewModel.IsFullscreenMode)
        {
            if (viewModel.ToggleFullscreenCommand.CanExecute(null))
            {
                viewModel.ToggleFullscreenCommand.Execute(null);
            }

            return;
        }

        if (viewModel.IsEditorMode)
        {
            await LeaveEditorAsync();
        }
    }

    private void OnPeopleClick(object sender, RoutedEventArgs eventArgs) =>
        ShowPeopleDialog(GetSelectedManagerPhotos());

    private void OnPeopleContextClick(object sender, RoutedEventArgs eventArgs) =>
        ShowPeopleDialog(GetContextPhotos(sender));

    private void ShowPeopleDialog(IReadOnlyList<PhotoItemViewModel> selected)
    {
        // Like AI tagging, the scan covers the selection; the whole folder
        // is only swept when nothing is selected.
        var scope = selected.Count > 0
            ? selected
            : viewModel.AllPhotos.Where(photo => !photo.IsTransient).ToArray();
        var records = scope
            .Select(photo => photo.Record)
            .ToArray();
        var dialog = new PeopleDialog(
            records,
            App.Services.Faces,
            catalog,
            App.Services.Previews,
            scanningSelection: selected.Count > 0)
        {
            Owner = this
        };
        dialog.ShowDialog();
        // Keyword writes go through the outbox; exiftool's file rewrites come
        // back in through the folder watcher, so no manual refresh is needed.
        // People and assignments may have changed, though.
        peopleNames.Clear();
        _ = LoadPersonFilterChoicesAsync();
        _ = UpdateFaceOverlaysAsync();
        _ = viewModel.RefreshPhotoPeopleAsync();
    }

    private const string ShowFacesSetting = "show_faces";
    private readonly Dictionary<long, string> peopleNames = new();

    private async void OnFacesOverlayToggled(
        object sender,
        RoutedEventArgs eventArgs)
    {
        var show = FacesOverlayButton.IsChecked == true;
        PreviewViewer.ShowFaceOverlays = show;
        await PersistSettingAsync(ShowFacesSetting, show.ToString());
        await UpdateFaceOverlaysAsync();
    }

    /// <summary>
    /// Feeds the viewer the face frames of the photo on screen; names come
    /// from a people cache invalidated when the People window closes.
    /// </summary>
    private async Task UpdateFaceOverlaysAsync()
    {
        if (FacesOverlayButton.IsChecked != true
            || viewModel.SelectedPhoto is not { IsTransient: false } photo)
        {
            PreviewViewer.FaceOverlays = null;
            return;
        }

        try
        {
            var path = photo.Path;
            var faces = await catalog.GetFacesForPathAsync(path);
            if (peopleNames.Count == 0)
            {
                foreach (var person in await catalog.GetPeopleAsync())
                {
                    peopleNames[person.Id] = person.Name;
                }
            }

            var overlays = faces
                .Select(face => new Controls.FaceOverlay(
                    face.X,
                    face.Y,
                    face.Width,
                    face.Height,
                    ResolveOverlayName(face),
                    IsSuggestion: face.PersonId is null
                                  && face.SuggestedPersonId is not null))
                .ToArray();
            // The awaits above may have resumed off the dispatcher; the
            // viewer is only ever touched from it.
            _ = Dispatcher.BeginInvoke(() =>
            {
                if (string.Equals(
                        viewModel.SelectedPhoto?.Path,
                        path,
                        StringComparison.OrdinalIgnoreCase))
                {
                    PreviewViewer.FaceOverlays = overlays;
                }
            });
        }
        catch (Exception)
        {
            _ = Dispatcher.BeginInvoke(() => PreviewViewer.FaceOverlays = null);
        }
    }

    private string? ResolveOverlayName(Domain.FaceRecord face) =>
        face.PersonId is { } personId
        && peopleNames.TryGetValue(personId, out var name)
            ? name
            : face.SuggestedPersonId is { } suggestedId
              && peopleNames.TryGetValue(suggestedId, out var suggested)
                ? suggested
                : null;

    private async Task LoadPersonFilterChoicesAsync()
    {
        IReadOnlyList<PersonRecord> people;
        try
        {
            people = await catalog.GetPeopleAsync();
        }
        catch (Exception)
        {
            // The filter facet simply stays empty when the catalogue balks.
            return;
        }

        // The await resumed off the dispatcher; the chips have not.
        _ = Dispatcher.BeginInvoke(() =>
        {
            var checkedBefore = personFilterChips
                .Count(chip => chip.IsChecked == true);
            isFilterUiUpdating = true;
            try
            {
                RebuildPersonFilterChips(people);
            }
            finally
            {
                isFilterUiUpdating = false;
            }

            if (checkedBefore
                != personFilterChips.Count(chip => chip.IsChecked == true))
            {
                // A filtered person no longer exists.
                ApplyFilterFromUi();
            }
        });
    }

    private void OnManagerTabClick(object sender, RoutedEventArgs eventArgs) =>
        viewModel.ShowManagerTab();

    private void OnEditorTabClick(object sender, RoutedEventArgs eventArgs)
    {
        if (sender is FrameworkElement { Tag: PhotoItemViewModel photo })
        {
            viewModel.ActivateEditorTab(photo);
        }
    }

    private async void OnEditorTabCloseClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (sender is FrameworkElement { Tag: PhotoItemViewModel photo })
        {
            await CloseEditorTabAsync(photo);
        }
    }

    /// <summary>
    /// Ends one tab's session - prompting for unsaved edits with the photo
    /// brought on screen first - and removes the tab. False when the user
    /// cancelled and the tab stays open.
    /// </summary>
    private async Task<bool> CloseEditorTabAsync(PhotoItemViewModel photo)
    {
        if (photo.IsEditorSessionActive || photo.IsEditorDirty)
        {
            if (!viewModel.IsEditorMode
                || !ReferenceEquals(viewModel.SelectedPhoto, photo))
            {
                viewModel.ActivateEditorTab(photo);
            }

            if (!await ConfirmEditorExitAsync())
            {
                return false;
            }
        }

        viewModel.RemoveEditorTab(photo);
        return true;
    }

    /// <summary>
    /// Walks every open tab before the window closes or restarts, activating
    /// each dirty one so its save prompt shows the photo it asks about.
    /// </summary>
    private async Task<bool> ConfirmAllEditorTabsAsync()
    {
        foreach (var photo in viewModel.EditorTabs.ToArray())
        {
            if (!photo.IsEditorDirty)
            {
                continue;
            }

            viewModel.ActivateEditorTab(photo);
            if (!await ConfirmEditorExitAsync())
            {
                return false;
            }
        }

        // A dirty photo can sit outside the tabs only through an unforeseen
        // path; the old single-photo prompt still covers it.
        return !viewModel.IsEditorMode
               || viewModel.SelectedPhoto?.IsEditorDirty != true
               || await ConfirmEditorExitAsync();
    }

    private async Task LeaveEditorAsync()
    {
        if (viewModel.SelectedPhoto is { } photo)
        {
            await CloseEditorTabAsync(photo);
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
            // Paging through photos retargets the current tab rather than
            // opening a new tab per photograph passed.
            viewModel.ReplaceEditorTab(current, viewModel.SelectedPhoto);
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
        if (PhotoSearchBox.IsKeyboardFocusWithin
            || eventArgs.OriginalSource is TextBoxBase)
        {
            return;
        }

        var shortcutKey = eventArgs.Key == Key.System
            ? eventArgs.SystemKey
            : eventArgs.Key;
        if (!viewModel.IsEditorMode
            && !viewModel.IsFullscreenMode
            && PhotoList.IsKeyboardFocusWithin
            && IsPasteImageShortcut(shortcutKey, Keyboard.Modifiers))
        {
            eventArgs.Handled = true;
            await PasteFilesFromClipboardAsync();
            return;
        }

        if (IsPasteImageShortcut(
                shortcutKey,
                Keyboard.Modifiers))
        {
            eventArgs.Handled = true;
            await PasteImageFromClipboardAsync();
            return;
        }

        if (TryGetFolderNavigationCommand(shortcutKey, Keyboard.Modifiers)
            is { } navigation)
        {
            eventArgs.Handled = true;
            if (navigation.CanExecute(null))
            {
                navigation.Execute(null);
            }

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

        if (viewModel.IsEditorMode
            && IsExportShortcut(eventArgs.Key, Keyboard.Modifiers))
        {
            eventArgs.Handled = true;
            await ExportCurrentPhotoAsync();
            return;
        }

        if (viewModel.IsEditorMode
            && eventArgs.Key == Key.Delete
            && Keyboard.Modifiers == ModifierKeys.None
            && PreviewViewer.TryDeleteSelectedLayer())
        {
            eventArgs.Handled = true;
            return;
        }

        if (TryHandleAnnotationShortcut(eventArgs.Key))
        {
            eventArgs.Handled = true;
            return;
        }

        if (viewModel.IsEditorMode
            && eventArgs.Key == Key.B
            && Keyboard.Modifiers == ModifierKeys.None)
        {
            BeforeAfterButton.IsChecked = BeforeAfterButton.IsChecked != true;
            OnBeforeAfterChanged(BeforeAfterButton, new RoutedEventArgs());
            eventArgs.Handled = true;
            return;
        }

        if (!viewModel.IsEditorMode
            && !viewModel.IsFullscreenMode
            && PhotoList.IsKeyboardFocusWithin)
        {
            if (IsSelectAllShortcut(shortcutKey, Keyboard.Modifiers))
            {
                PhotoList.SelectAll();
                eventArgs.Handled = true;
                return;
            }

            if (IsQuickFileCopyShortcut(
                    shortcutKey,
                    Keyboard.Modifiers)
                && HasLastCopyDestination())
            {
                eventArgs.Handled = true;
                await CopyFilesToLastDestinationAsync(
                    GetSelectedManagerPhotos());
                return;
            }

            if (IsCopyFilesShortcut(
                    shortcutKey,
                    Keyboard.Modifiers))
            {
                eventArgs.Handled = true;
                CopyFilesToClipboard(GetSelectedManagerPhotos());
                return;
            }

            if (IsCopyToShortcut(shortcutKey, Keyboard.Modifiers))
            {
                eventArgs.Handled = true;
                await ChooseAndTransferFilesAsync(
                    GetSelectedManagerPhotos(),
                    PhotoFileTransferMode.Copy);
                return;
            }

            if (IsMoveToShortcut(shortcutKey, Keyboard.Modifiers))
            {
                eventArgs.Handled = true;
                await ChooseAndTransferFilesAsync(
                    GetSelectedManagerPhotos(),
                    PhotoFileTransferMode.Move);
                return;
            }

            if (IsBatchShortcut(shortcutKey, Keyboard.Modifiers))
            {
                eventArgs.Handled = true;
                await RunBatchConversionAsync(GetSelectedManagerPhotos());
                return;
            }

            if (IsImportShortcut(shortcutKey, Keyboard.Modifiers))
            {
                eventArgs.Handled = true;
                await ImportPhotosAsync();
                return;
            }

            if (IsCompareShortcut(shortcutKey, Keyboard.Modifiers))
            {
                eventArgs.Handled = true;
                CompareSelectedPhotos();
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

        if (eventArgs.Key == Key.Delete
            && Keyboard.Modifiers == ModifierKeys.None
            && viewModel.SelectedPhoto is { IsTransient: false })
        {
            eventArgs.Handled = true;
            if (!viewModel.IsEditorMode && !viewModel.IsFullscreenMode)
            {
                await DeleteSelectedFilesAsync(GetSelectedManagerPhotos());
            }
            else
            {
                await viewModel.DeleteSelectedPhotoAsync();
            }

            return;
        }

        if (eventArgs.Key == Key.F5
            && !viewModel.IsEditorMode
            && !viewModel.IsFullscreenMode
            && viewModel.CurrentFolder is { } currentFolder)
        {
            eventArgs.Handled = true;
            await viewModel.LoadFolderAsync(
                currentFolder,
                CancellationToken.None);
            return;
        }

        var directPhotoLaunchAction = GetDirectPhotoLaunchKeyAction(
            eventArgs.Key,
            viewModel.IsDirectPhotoLaunch,
            viewModel.IsEditorMode);
        if (directPhotoLaunchAction == DirectPhotoLaunchKeyAction.CloseWindow)
        {
            eventArgs.Handled = true;
            Close();
            return;
        }

        if (directPhotoLaunchAction == DirectPhotoLaunchKeyAction.OpenManager)
        {
            eventArgs.Handled = true;
            if (await ConfirmEditorExitAsync())
            {
                await viewModel.OpenSelectedPhotoFolderInManagerAsync();
            }

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
            && eventArgs.Key == Key.R
            && viewModel.SelectedPhoto is { } photoToRotate)
        {
            photoToRotate.RotateRightCommand.Execute(null);
            eventArgs.Handled = true;
            return;
        }

        if (Keyboard.Modifiers == ModifierKeys.None
            && eventArgs.Key == Key.F
            && viewModel.ToggleFullscreenCommand.CanExecute(null))
        {
            viewModel.ToggleFullscreenCommand.Execute(null);
            eventArgs.Handled = true;
            return;
        }

        if (Keyboard.Modifiers == ModifierKeys.None
            && viewModel.SelectedPhoto is not null
            && TryGetRatingShortcut(eventArgs.Key, out var rating))
        {
            ApplyToSelection(photo => photo.Rating = rating);
            ShowRatingFeedback(rating);
            eventArgs.Handled = true;
            return;
        }

        if (viewModel.SelectedPhoto is not null
            && TryHandleOrganizationShortcut(eventArgs.Key, Keyboard.Modifiers))
        {
            eventArgs.Handled = true;
            return;
        }

        if (eventArgs.Key == Key.F2
            && Keyboard.Modifiers == ModifierKeys.None
            && !viewModel.IsEditorMode
            && !viewModel.IsFullscreenMode)
        {
            eventArgs.Handled = true;
            await RenamePhotoAsync(GetSelectedManagerPhotos().FirstOrDefault());
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

    internal static DirectPhotoLaunchKeyAction GetDirectPhotoLaunchKeyAction(
        Key key,
        bool isDirectPhotoLaunch,
        bool isEditorMode)
    {
        if (!isDirectPhotoLaunch || !isEditorMode)
        {
            return DirectPhotoLaunchKeyAction.None;
        }

        return key switch
        {
            Key.Escape => DirectPhotoLaunchKeyAction.CloseWindow,
            Key.Enter => DirectPhotoLaunchKeyAction.OpenManager,
            _ => DirectPhotoLaunchKeyAction.None
        };
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

    /// <summary>
    /// Confirms a rating keystroke where the tiles and the info panel cannot:
    /// in fullscreen and the editor only the photograph is on screen, so the
    /// keystroke would otherwise land without any visible acknowledgement.
    /// </summary>
    private void ShowRatingFeedback(int rating)
    {
        if (!viewModel.IsFullscreenMode && !viewModel.IsEditorMode)
        {
            return;
        }

        if (viewModel.SelectedPhoto is not { IsTransient: false })
        {
            return;
        }

        ShowViewerOsd(rating > 0
            ? string.Concat(new string('★', rating), new string('☆', 5 - rating))
            : "Rating cleared");
    }

    private void ShowViewerOsd(string text)
    {
        ViewerOsdText.Text = text;
        ViewerOsd.Visibility = Visibility.Visible;
        var fade = new DoubleAnimation(1, 0, TimeSpan.FromMilliseconds(450))
        {
            BeginTime = TimeSpan.FromMilliseconds(900)
        };
        fade.Completed += (_, _) => ViewerOsd.Visibility = Visibility.Collapsed;
        ViewerOsd.BeginAnimation(OpacityProperty, fade);
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

    internal static bool IsCopyFilesShortcut(
        Key key,
        ModifierKeys modifiers) =>
        key == Key.C && modifiers == ModifierKeys.Control;

    internal static bool IsSelectAllShortcut(
        Key key,
        ModifierKeys modifiers) =>
        key == Key.A && modifiers == ModifierKeys.Control;

    internal static bool IsCopyToShortcut(
        Key key,
        ModifierKeys modifiers) =>
        key == Key.C && modifiers == ModifierKeys.Alt;

    internal static bool IsMoveToShortcut(
        Key key,
        ModifierKeys modifiers) =>
        key == Key.X && modifiers == ModifierKeys.Alt;

    internal static bool IsBatchShortcut(
        Key key,
        ModifierKeys modifiers) =>
        key == Key.B && modifiers == ModifierKeys.Control;

    internal static bool IsImportShortcut(
        Key key,
        ModifierKeys modifiers) =>
        key == Key.I && modifiers == ModifierKeys.Control;

    internal static bool IsCompareShortcut(
        Key key,
        ModifierKeys modifiers) =>
        key == Key.K && modifiers == ModifierKeys.Control;

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

    internal static bool IsExportShortcut(
        Key key,
        ModifierKeys modifiers) =>
        key == Key.E && modifiers == ModifierKeys.Control;

    /// <summary>
    /// Alt+Left/Right/Up move through the folder history the way a file
    /// browser does; they are deliberately kept away from the plain arrow
    /// keys, which step between photographs.
    /// </summary>
    private System.Windows.Input.ICommand? TryGetFolderNavigationCommand(
        Key key,
        ModifierKeys modifiers)
    {
        if (modifiers != ModifierKeys.Alt
            || viewModel.IsEditorMode
            || viewModel.IsFullscreenMode)
        {
            return null;
        }

        return key switch
        {
            Key.Left => viewModel.GoBackCommand,
            Key.Right => viewModel.GoForwardCommand,
            Key.Up => viewModel.GoUpCommand,
            _ => null
        };
    }

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
            AttachEditorTarget(viewModel.SelectedPhoto);
            _ = UpdateFaceOverlaysAsync();
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
            viewModel.EnsureEditorTabForSelection();
            viewModel.SelectedPhoto?.BeginEditorSession();
        }

        if (eventArgs.PropertyName == nameof(MainViewModel.IsEditorMode)
            && !viewModel.IsEditorMode)
        {
            PreviewViewer.EndSelectionMode();
            _ = RefreshCatalogAfterEditorExitAsync();
        }

        ApplyModeLayout();
        OnEditorModeChanged();
        Dispatcher.BeginInvoke(
            DispatcherPriority.Loaded,
            () =>
            {
                if (viewModel.IsEditorMode || viewModel.IsFullscreenMode)
                {
                    PreviewViewer.Focus();
                }
                else
                {
                    FocusSelectedPhotoInCatalog();
                }
            });
    }

    /// <summary>
    /// Brings the manager back to the photo the user was looking at: the
    /// selected tile is scrolled into view and takes keyboard focus, so
    /// culling continues right where fullscreen or the editor left off.
    /// </summary>
    private void FocusSelectedPhotoInCatalog()
    {
        if (viewModel.SelectedPhoto is not { } photo)
        {
            PhotoList.Focus();
            return;
        }

        // The catalog column was collapsed a moment ago; the tile panel must
        // re-measure at its restored width before the scroll target and the
        // item container exist.
        PhotoList.UpdateLayout();
        PhotoList.ScrollIntoView(photo);
        PhotoList.UpdateLayout();
        if (PhotoList.ItemContainerGenerator.ContainerFromItem(photo)
            is ListBoxItem container)
        {
            container.Focus();
        }
        else
        {
            PhotoList.Focus();
        }
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

            // An already-maximized window keeps its taskbar-respecting bounds
            // when the chrome is removed, so drop to Normal first and let the
            // borderless maximize below claim the whole screen.
            if (WindowState == WindowState.Maximized)
            {
                WindowState = WindowState.Normal;
            }

            WindowStyle = WindowStyle.None;
            ResizeMode = ResizeMode.NoResize;
            WindowState = WindowState.Maximized;
            return;
        }

        if (layoutBeforeFullscreen is not { } layout)
        {
            return;
        }

        layoutBeforeFullscreen = null;

        // Normal first, so the restored chrome gets fresh maximized bounds
        // that respect the taskbar again.
        WindowState = WindowState.Normal;
        WindowStyle = windowStyleBeforeFullscreen;
        ResizeMode = resizeModeBeforeFullscreen;

        // Fullscreen never moved the window, so only the pane widths need to
        // come back; re-deriving them through ApplyWindowLayout would clamp
        // them against the restore bounds even when the window returns
        // maximized, squeezing the catalog pane.
        navigatorPaneWidth = layout.NavigatorPaneWidth;
        catalogPaneWidth = layout.CatalogPaneWidth;
        ApplyModeLayout();

        if (layout.State == WindowState.Maximized)
        {
            WindowState = WindowState.Maximized;
        }
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

        // A window that comes back maximized is wider than its restore
        // bounds, so the panes must be clamped against the screen it will
        // actually occupy, not the normal-state width.
        var paneClampWidth = state.State == WindowState.Maximized
            ? Math.Max(width, SystemParameters.WorkArea.Width)
            : width;
        var maximumNavigatorWidth = Math.Max(
            NavigatorMinWidth,
            paneClampWidth - CatalogMinWidth - ViewerMinWidth
            - (2 * SplitterWidth));
        navigatorPaneWidth = ClampFinite(
            state.NavigatorPaneWidth,
            NavigatorMinWidth,
            maximumNavigatorWidth,
            DefaultNavigatorWidth);
        var maximumCatalogWidth = Math.Max(
            CatalogMinWidth,
            paneClampWidth - navigatorPaneWidth - ViewerMinWidth
            - (2 * SplitterWidth));
        catalogPaneWidth = ClampFinite(
            state.CatalogPaneWidth,
            CatalogMinWidth,
            maximumCatalogWidth,
            DefaultCatalogWidth);
        managerInfoSplit.Ratio = state.ManagerInfoPaneRatio;
        editorInfoSplit.Ratio = state.EditorInfoPaneRatio;
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

            // The adjustment panel belongs to Editor only; fullscreen is for
            // looking at the photograph, not working on it.
            var showsEditorPanel = viewModel.IsEditorMode
                                   && !viewModel.IsFullscreenMode;
            EditorPanelColumn.Width = showsEditorPanel
                ? new GridLength(editorPanelWidth)
                : new GridLength(0);
            EditorSplitterColumn.Width = showsEditorPanel
                ? new GridLength(SplitterWidth)
                : new GridLength(0);

            // Both viewer modes want the whole column for the photograph;
            // the Manager's info split waits underneath for the way back.
            managerInfoSplit.Suspend();
            PreviewViewer.FitToViewport();
            return;
        }

        NavigatorColumn.MinWidth = NavigatorMinWidth;
        CatalogColumn.MinWidth = CatalogMinWidth;
        NavigatorColumn.Width = new GridLength(navigatorPaneWidth);
        NavigatorSplitterColumn.Width = new GridLength(SplitterWidth);
        CatalogColumn.Width = new GridLength(catalogPaneWidth);
        CatalogSplitterColumn.Width = new GridLength(SplitterWidth);
        EditorPanelColumn.Width = new GridLength(0);
        EditorSplitterColumn.Width = new GridLength(0);
        managerInfoSplit.Resume();
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
            catalogPaneWidth,
            managerInfoSplit.Ratio,
            editorInfoSplit.Ratio);
    }

    private async void OnWindowClosing(
        object? sender,
        CancelEventArgs eventArgs)
    {
        if (isClosingAfterLayoutSave || !IsVisible || !isLayoutRestored)
        {
            return;
        }

        if (viewModel.EditorTabs.Any(tab => tab.IsEditorDirty)
            || (viewModel.IsEditorMode
                && viewModel.SelectedPhoto?.IsEditorDirty == true))
        {
            eventArgs.Cancel = true;
            if (isEditorExitPromptActive)
            {
                return;
            }

            isEditorExitPromptActive = true;
            var canClose = await ConfirmAllEditorTabsAsync();
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

    private sealed record WindowLayoutState(
        double Left,
        double Top,
        double Width,
        double Height,
        WindowState State,
        double NavigatorPaneWidth,
        double CatalogPaneWidth,
        double ManagerInfoPaneRatio = 0.5,
        double EditorInfoPaneRatio = 0.5);
}

internal enum DirectPhotoLaunchKeyAction
{
    None,
    CloseWindow,
    OpenManager
}

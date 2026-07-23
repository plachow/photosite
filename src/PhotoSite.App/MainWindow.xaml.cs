using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Interop;
using System.Windows.Threading;
using PhotoSite.Infrastructure;
using PhotoSite.ViewModels;

namespace PhotoSite;

public partial class MainWindow : Window
{
    private const int DwmUseImmersiveDarkMode = 20;
    private const int DwmUseImmersiveDarkModeLegacy = 19;
    private const int DwmBorderColor = 34;
    private const int DwmCaptionColor = 35;
    private const int DwmTextColor = 36;
    private const string WindowLayoutSetting = "window_layout_v1";
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
    private double navigatorPaneWidth = DefaultNavigatorWidth;
    private double catalogPaneWidth = DefaultCatalogWidth;
    private bool isLayoutRestored;
    private bool isLayoutClosePending;
    private bool isClosingAfterLayoutSave;

    public MainWindow(
        MainViewModel viewModel,
        PhotoCatalogRepository catalog)
    {
        InitializeComponent();
        this.viewModel = viewModel;
        this.catalog = catalog;
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

    public async Task RestoreLayoutAsync()
    {
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

    private void OnPreviewKeyDown(object sender, KeyEventArgs eventArgs)
    {
        if (eventArgs.Key == Key.Escape && viewModel.IsEditorMode)
        {
            if (RatingComboBox.IsDropDownOpen)
            {
                return;
            }

            if (viewModel.ShowManagerCommand.CanExecute(null))
            {
                // A dirty-edit confirmation can be inserted at this boundary later.
                viewModel.ShowManagerCommand.Execute(null);
                eventArgs.Handled = true;
            }

            return;
        }

        if (eventArgs.Key != Key.Enter || viewModel.SelectedPhoto is null)
        {
            return;
        }

        if (viewModel.IsEditorMode)
        {
            if (PreviewViewer.IsKeyboardFocusWithin
                && viewModel.ShowManagerCommand.CanExecute(null))
            {
                viewModel.ShowManagerCommand.Execute(null);
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

    private void OnViewModelPropertyChanged(
        object? sender,
        PropertyChangedEventArgs eventArgs)
    {
        if (eventArgs.PropertyName != nameof(MainViewModel.IsEditorMode))
        {
            return;
        }

        ApplyModeLayout();
        Dispatcher.BeginInvoke(
            () =>
            {
                if (viewModel.IsEditorMode)
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
        if (viewModel.IsEditorMode)
        {
            CapturePaneWidths();
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

    private void CapturePaneWidths()
    {
        if (viewModel.IsEditorMode)
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
            || isClosingAfterLayoutSave)
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

    private WindowLayoutState CaptureWindowLayout()
    {
        CapturePaneWidths();
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

        if (isLayoutClosePending)
        {
            eventArgs.Cancel = true;
            return;
        }

        eventArgs.Cancel = true;
        isLayoutClosePending = true;
        layoutSaveTimer.Stop();
        await SaveLayoutAsync();
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

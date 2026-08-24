using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;

namespace PhotoSite.Controls;

/// <summary>
/// Drives one vertical split between a main pane and the info pane under
/// it. The splitter drags to any ratio, the chevron buttons step the pane
/// through folded / split / whole column, and the header's ⓘ toggle stays
/// the fold switch it always was — every affordance moves the same state.
/// </summary>
internal sealed class SplitPaneController
{
    private const double DefaultRatio = 0.5;
    private const double MinimumRatio = 0.1;
    private const double MaximumRatio = 0.9;
    private const double MinimumPaneHeight = 90;
    private const double SplitBarHeight = 13;

    private readonly RowDefinition topRow;
    private readonly RowDefinition barRow;
    private readonly RowDefinition paneRow;
    private readonly GridSplitter splitter;
    private readonly ToggleButton foldToggle;
    private readonly Button expandButton;
    private readonly Button collapseButton;
    private double ratio = DefaultRatio;
    private bool isMaximized;
    private bool isSuspended;

    public SplitPaneController(
        RowDefinition topRow,
        RowDefinition barRow,
        RowDefinition paneRow,
        GridSplitter splitter,
        ToggleButton foldToggle,
        Button expandButton,
        Button collapseButton)
    {
        this.topRow = topRow;
        this.barRow = barRow;
        this.paneRow = paneRow;
        this.splitter = splitter;
        this.foldToggle = foldToggle;
        this.expandButton = expandButton;
        this.collapseButton = collapseButton;
        splitter.DragCompleted += OnSplitterDragCompleted;
        foldToggle.Checked += OnFoldToggleChanged;
        foldToggle.Unchecked += OnFoldToggleChanged;
        expandButton.Click += (_, _) => Expand();
        collapseButton.Click += (_, _) => Collapse();
        ApplyState();
    }

    /// <summary>Raised when a drag settles on a new ratio, so the host can
    /// persist the layout.</summary>
    public event Action? RatioChanged;

    /// <summary>The pane's share of the split space; survives folding and
    /// maximizing so restoring returns to it.</summary>
    public double Ratio
    {
        get => ratio;
        set
        {
            ratio = double.IsFinite(value)
                ? Math.Clamp(value, MinimumRatio, MaximumRatio)
                : DefaultRatio;
            if (!isSuspended)
            {
                ApplyState();
            }
        }
    }

    private bool IsPaneOpen => foldToggle.IsChecked == true;

    /// <summary>One step up: folded → split → the whole column.</summary>
    public void Expand()
    {
        if (!IsPaneOpen)
        {
            // The toggle's Checked handler applies the restored split.
            foldToggle.IsChecked = true;
            return;
        }

        if (!isMaximized)
        {
            isMaximized = true;
            ApplyState();
        }
    }

    /// <summary>One step down: whole column → split → folded.</summary>
    public void Collapse()
    {
        if (isMaximized)
        {
            isMaximized = false;
            ApplyState();
            return;
        }

        if (IsPaneOpen)
        {
            foldToggle.IsChecked = false;
        }
    }

    /// <summary>Hands the rows to the editor or fullscreen, which needs the
    /// whole column; the split state survives for the way back.</summary>
    public void Suspend()
    {
        isSuspended = true;
        topRow.MinHeight = 0;
        paneRow.MinHeight = 0;
        topRow.Height = new GridLength(1, GridUnitType.Star);
        barRow.Height = new GridLength(0);
        paneRow.Height = new GridLength(0);
    }

    public void Resume()
    {
        isSuspended = false;
        ApplyState();
    }

    private void OnFoldToggleChanged(object sender, RoutedEventArgs eventArgs)
    {
        isMaximized = false;
        if (!isSuspended)
        {
            ApplyState();
        }
    }

    private void OnSplitterDragCompleted(
        object sender,
        DragCompletedEventArgs eventArgs)
    {
        if (isSuspended || !IsPaneOpen)
        {
            return;
        }

        var total = topRow.ActualHeight + paneRow.ActualHeight;
        if (total <= 0)
        {
            return;
        }

        ratio = Math.Clamp(
            paneRow.ActualHeight / total,
            MinimumRatio,
            MaximumRatio);
        isMaximized = false;
        ApplyState();
        RatioChanged?.Invoke();
    }

    private void ApplyState()
    {
        barRow.Height = new GridLength(SplitBarHeight);
        splitter.IsEnabled = IsPaneOpen;
        expandButton.IsEnabled = !IsPaneOpen || !isMaximized;
        collapseButton.IsEnabled = IsPaneOpen;

        if (!IsPaneOpen)
        {
            topRow.MinHeight = 0;
            paneRow.MinHeight = 0;
            topRow.Height = new GridLength(1, GridUnitType.Star);
            paneRow.Height = GridLength.Auto;
            return;
        }

        if (isMaximized)
        {
            topRow.MinHeight = 0;
            paneRow.MinHeight = MinimumPaneHeight;
            topRow.Height = new GridLength(0, GridUnitType.Star);
            paneRow.Height = new GridLength(1, GridUnitType.Star);
            return;
        }

        topRow.MinHeight = MinimumPaneHeight;
        paneRow.MinHeight = MinimumPaneHeight;
        topRow.Height = new GridLength(1 - ratio, GridUnitType.Star);
        paneRow.Height = new GridLength(ratio, GridUnitType.Star);
    }
}

using System.ComponentModel;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using PhotoSite.Controls;
using PhotoSite.Domain;
using PhotoSite.Services.Imaging;
using PhotoSite.ViewModels;

namespace PhotoSite;

/// <summary>
/// The Editor half of the window: the live preview panel, Auto Fix, the white
/// balance eyedropper, crop ratios and before/after.
/// </summary>
public partial class MainWindow
{
    private const double EditorPanelWidth = 318;

    private static readonly (string Label, double Ratio)[] CropRatios =
    [
        ("Free", 0),
        ("Original", -1),
        ("1:1", 1),
        ("4:3", 4 / 3d),
        ("3:2", 3 / 2d),
        ("16:9", 16 / 9d),
        ("3:4", 3 / 4d),
        ("2:3", 2 / 3d)
    ];

    private readonly List<ToggleButton> cropRatioButtons = [];
    private CancellationTokenSource? histogramCancellation;
    private AdjustmentsViewModel? attachedAdjustments;
    private PhotoItemViewModel? attachedPhoto;
    private double editorPanelWidth = EditorPanelWidth;

    private void InitializeEditorPanel()
    {
        HistogramChannelBox.ItemsSource = new[]
        {
            "RGB", "Red", "Green", "Blue", "Luminance"
        };
        HistogramChannelBox.SelectedIndex = 0;

        foreach (var (label, ratio) in CropRatios)
        {
            var button = new ToggleButton
            {
                Content = label,
                Tag = ratio,
                MinWidth = 52,
                Height = 26,
                Margin = new Thickness(0, 0, 4, 4),
                Style = (Style)FindResource("EditorSelectToggleButtonStyle")
            };
            button.Click += OnCropRatioClick;
            cropRatioButtons.Add(button);
        }

        var rows = new WrapPanel();
        foreach (var button in cropRatioButtons)
        {
            rows.Children.Add(button);
        }

        CropRatioPanel.Children.Add(rows);
        cropRatioButtons[0].IsChecked = true;

        PreviewViewer.PreviewRendered += (_, _) => UpdateHistogram();
        PreviewViewer.PreviewColorPicked += OnPreviewColorPicked;
        EyedropperButton.Checked += (_, _) =>
            PreviewViewer.IsColorPickerMode = true;
        EyedropperButton.Unchecked += (_, _) =>
            PreviewViewer.IsColorPickerMode = false;
    }

    private void OnCropRatioClick(object sender, RoutedEventArgs eventArgs)
    {
        if (sender is not ToggleButton clicked)
        {
            return;
        }

        foreach (var button in cropRatioButtons)
        {
            button.IsChecked = ReferenceEquals(button, clicked);
        }

        PreviewViewer.SelectionAspectRatio = (double)clicked.Tag;
        if (!PreviewViewer.IsSelectionMode)
        {
            PreviewViewer.IsSelectionMode = true;
        }

        PreviewViewer.Focus();
    }

    private void OnClearCropClick(object sender, RoutedEventArgs eventArgs)
    {
        viewModel.SelectedPhoto?.ResetCrop();
        PreviewViewer.ClearSelection();
        PreviewViewer.FitToViewport();
        viewModel.ReportStatus("Crop cleared");
    }

    private void OnHistogramChannelChanged(
        object sender,
        SelectionChangedEventArgs eventArgs)
    {
        Histogram.Channel = HistogramChannelBox.SelectedIndex switch
        {
            1 => HistogramChannel.Red,
            2 => HistogramChannel.Green,
            3 => HistogramChannel.Blue,
            4 => HistogramChannel.Luminance,
            _ => HistogramChannel.Rgb
        };
    }

    private async void UpdateHistogram()
    {
        if (!viewModel.IsEditorMode)
        {
            return;
        }

        histogramCancellation?.Cancel();
        histogramCancellation?.Dispose();
        histogramCancellation = new CancellationTokenSource();
        await Histogram.UpdateAsync(
                PreviewViewer.DisplayedBitmap,
                histogramCancellation.Token)
            .ConfigureAwait(false);
        await Dispatcher.InvokeAsync(UpdateClippingText);
    }

    private void UpdateClippingText()
    {
        var data = Histogram.Data;
        if (data.Total == 0)
        {
            ClippingText.Text = string.Empty;
            return;
        }

        var warnings = new List<string>(2);
        if (data.ShadowClippedFraction > 0.001)
        {
            warnings.Add(
                $"shadows {data.ShadowClippedFraction:P1} clipped");
        }

        if (data.HighlightClippedFraction > 0.001)
        {
            warnings.Add(
                $"highlights {data.HighlightClippedFraction:P1} clipped");
        }

        ClippingText.Text = warnings.Count == 0
            ? "No clipping"
            : string.Join(" · ", warnings);
    }

    /// <summary>
    /// Keeps the Auto Fix and eyedropper commands pointed at whichever photo
    /// is open, so switching photos in the editor does not silently correct
    /// the previous one.
    /// </summary>
    private void AttachEditorTarget(PhotoItemViewModel? photo)
    {
        if (ReferenceEquals(attachedPhoto, photo))
        {
            return;
        }

        if (attachedAdjustments is not null)
        {
            attachedAdjustments.AutoFixRequested -= OnAutoFixRequested;
            attachedAdjustments.AutoWhiteBalanceRequested -=
                OnAutoWhiteBalanceRequested;
        }

        attachedPhoto = photo;
        attachedAdjustments = photo?.Adjustments;
        if (attachedAdjustments is not null)
        {
            attachedAdjustments.AutoFixRequested += OnAutoFixRequested;
            attachedAdjustments.AutoWhiteBalanceRequested +=
                OnAutoWhiteBalanceRequested;
        }
    }

    private void OnAutoFixRequested(object? sender, EventArgs eventArgs) =>
        ApplyMeasuredAdjustment(
            AutoFixMode.Full,
            "Auto Fix applied - every value it chose is on a slider");

    private void OnAutoWhiteBalanceRequested(object? sender, EventArgs eventArgs) =>
        ApplyMeasuredAdjustment(
            AutoFixMode.WhiteBalanceOnly,
            "Auto white balance applied");

    private enum AutoFixMode
    {
        Full,
        WhiteBalanceOnly
    }

    private void ApplyMeasuredAdjustment(AutoFixMode mode, string status)
    {
        if (viewModel.SelectedPhoto is not { } photo
            || PreviewViewer.OriginalBitmap is not { } source)
        {
            return;
        }

        try
        {
            // Measured from the unadjusted frame, so pressing Auto Fix twice
            // gives the same answer instead of compounding its own output.
            var buffer = PixelBuffer.FromBitmap(source);
            var current = photo.EditRecipe.Adjustments;
            var analysed = AutoFixAnalyzer.Analyze(buffer, current);
            var applied = mode == AutoFixMode.Full
                ? analysed
                : current with
                {
                    Temperature = analysed.Temperature,
                    Tint = analysed.Tint
                };
            photo.Adjustments.ApplyMeasured(applied);
            viewModel.ReportStatus(status);
        }
        catch (Exception exception)
        {
            viewModel.ReportStatus($"Auto correction failed: {exception.Message}");
        }
    }

    private void OnPreviewColorPicked(
        object? sender,
        (double Red, double Green, double Blue) sample)
    {
        EyedropperButton.IsChecked = false;
        if (viewModel.SelectedPhoto is not { } photo)
        {
            return;
        }

        var (temperature, tint) = AdjustmentsViewModel.SolveWhiteBalance(
            sample.Red,
            sample.Green,
            sample.Blue);
        photo.Adjustments.ApplyMeasured(
            photo.EditRecipe.Adjustments with
            {
                Temperature = temperature,
                Tint = tint
            });
        viewModel.ReportStatus(
            $"White balance set from the picked colour ({temperature:0}, {tint:0})");
    }

    private void OnBeforeAfterChanged(object sender, RoutedEventArgs eventArgs)
    {
        PreviewViewer.ComparisonMode = BeforeAfterButton.IsChecked == true
            ? SplitCompareButton.IsChecked == true
                ? PreviewComparisonMode.Split
                : PreviewComparisonMode.Original
            : PreviewComparisonMode.Edited;
    }

    private void OnFlipVerticalClick(object sender, RoutedEventArgs eventArgs) =>
        viewModel.SelectedPhoto?.FlipVertical();

    private void OnFiltersClick(object sender, RoutedEventArgs eventArgs)
    {
        if (viewModel.SelectedPhoto is not { } photo
            || PreviewViewer.OriginalBitmap is not { } source)
        {
            return;
        }

        var dialog = new Dialogs.FilterDialog(source)
        {
            Owner = this
        };
        if (dialog.ShowDialog() != true || dialog.Result is not { } filter)
        {
            return;
        }

        // Filters stack, so the new one joins the recipe rather than replacing
        // it; Undo removes exactly this step.
        photo.SetFilters([.. photo.EditRecipe.Filters, filter]);
        viewModel.ReportStatus(
            $"Applied {filter.DisplayName} · Ctrl+Z removes it");
    }

    private async void OnExportClick(object sender, RoutedEventArgs eventArgs) =>
        await ExportCurrentPhotoAsync();

    private async Task ExportCurrentPhotoAsync()
    {
        if (viewModel.SelectedPhoto is not { IsTransient: false } photo
            || !File.Exists(photo.Path))
        {
            viewModel.ReportStatus("Save the image before exporting it");
            return;
        }

        var dialog = new Dialogs.BatchDialog(
            [
                new Services.Batch.BatchSource(
                    photo.Path,
                    photo.EditRecipe,
                    photo.TakenAtTicks,
                    photo.Record.PixelWidth,
                    photo.Record.PixelHeight)
            ],
            App.Services.BatchPresets,
            App.Services.Batch,
            lastBatchDestination
            ?? Path.GetDirectoryName(photo.Path),
            Services.Batch.BatchPresetStore.ExportKind)
        {
            Owner = this
        };
        dialog.ShowDialog();

        if (dialog.DidWriteFiles && dialog.LastOutputDirectory is { } destination)
        {
            lastBatchDestination = destination;
            await PersistSettingAsync(LastBatchDestinationSetting, destination);
            viewModel.ReportStatus($"Exported to {destination}");
        }
    }

    private void SyncCropRatioButtons()
    {
        var ratio = PreviewViewer.SelectionAspectRatio;
        foreach (var button in cropRatioButtons)
        {
            button.IsChecked = (double)button.Tag == ratio;
        }
    }

    private void OnEditorSplitterDragCompleted(
        object sender,
        System.Windows.Controls.Primitives.DragCompletedEventArgs eventArgs)
    {
        if (EditorPanelColumn.ActualWidth > 200)
        {
            editorPanelWidth = EditorPanelColumn.ActualWidth;
        }
    }

    internal void ValidateEditorPanelForSmokeTest()
    {
        if (Content is not UIElement content)
        {
            throw new InvalidOperationException("The main window has no content.");
        }

        var photo = viewModel.SelectedPhoto
            ?? throw new InvalidOperationException(
                "The editor test needs a selected photo.");

        viewModel.ShowEditorCommand.Execute(null);
        content.Measure(new Size(1500, 900));
        content.Arrange(new Rect(0, 0, 1500, 900));
        content.UpdateLayout();

        if (EditorPanelColumn.ActualWidth < 200)
        {
            throw new InvalidOperationException(
                "The editor panel must open with the editor.");
        }

        if (!ReferenceEquals(EditorPanel.DataContext, photo))
        {
            throw new InvalidOperationException(
                "The editor panel must follow the edited photo.");
        }

        photo.BeginEditorSession();
        var adjustments = photo.Adjustments;
        adjustments.Exposure = 0.5;
        adjustments.Exposure = 0.6;
        if (photo.EditRecipe.Adjustments.Exposure != 0.6)
        {
            throw new InvalidOperationException(
                "An adjustment slider must write through to the recipe.");
        }

        if (!photo.IsEditorDirty)
        {
            throw new InvalidOperationException(
                "Adjusting a photo must mark the editor dirty.");
        }

        photo.UndoEditCommand.Execute(null);
        if (photo.EditRecipe.Adjustments.Exposure != 0)
        {
            throw new InvalidOperationException(
                "Consecutive moves of one slider must undo as a single step, "
                + $"got {photo.EditRecipe.Adjustments.Exposure}.");
        }

        if (adjustments.Exposure != 0)
        {
            throw new InvalidOperationException(
                "Undo must be reflected back onto the slider panel.");
        }

        adjustments.Contrast = 20;
        adjustments.Saturation = -30;
        photo.UndoEditCommand.Execute(null);
        if (photo.EditRecipe.Adjustments.Contrast != 20
            || photo.EditRecipe.Adjustments.Saturation != 0)
        {
            throw new InvalidOperationException(
                "Different sliders must be separate undo steps.");
        }

        ValidateLayerEditingForSmokeTest(photo);

        photo.DiscardEditorSession();
        viewModel.ShowManagerCommand.Execute(null);
        content.UpdateLayout();
        if (EditorPanelColumn.ActualWidth > 0)
        {
            throw new InvalidOperationException(
                "Leaving the editor must close its panel.");
        }
    }

    private void OnEditorModeChanged()
    {
        AttachEditorTarget(viewModel.SelectedPhoto);
        RefreshLayerList();
        if (viewModel.IsEditorMode)
        {
            UpdateHistogram();
        }
        else
        {
            PreviewViewer.IsColorPickerMode = false;
            PreviewViewer.ComparisonMode = PreviewComparisonMode.Edited;
            SetAnnotationTool(AnnotationTool.None);
            EyedropperButton.IsChecked = false;
            BeforeAfterButton.IsChecked = false;
            SplitCompareButton.IsChecked = false;
        }
    }
}

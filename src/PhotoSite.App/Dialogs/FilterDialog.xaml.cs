using System.Windows;
using System.Windows.Controls;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using PhotoSite.Domain;
using PhotoSite.Services.Imaging;

namespace PhotoSite.Dialogs;

public partial class FilterDialog : Window
{
    /// <summary>
    /// The preview works on a reduced copy: a filter dialog has to react while
    /// a slider is being dragged, and blurring 24 megapixels cannot.
    /// </summary>
    private const int PreviewLongestSide = 1100;

    private readonly BitmapSource previewSource;
    private readonly DispatcherTimer renderTimer;
    private CancellationTokenSource? renderCancellation;
    private bool isLoading;

    internal FilterDialog(BitmapSource source)
    {
        InitializeComponent();
        previewSource = ImageRenderer.Resize(source, PreviewLongestSide);

        renderTimer = new DispatcherTimer
        {
            Interval = TimeSpan.FromMilliseconds(90)
        };
        renderTimer.Tick += (_, _) =>
        {
            renderTimer.Stop();
            _ = RenderPreviewAsync();
        };

        FilterList.ItemsSource = Enum.GetValues<PhotoFilterKind>()
            .Select(FilterStep.CreateDefault)
            .ToArray();
        FilterList.DisplayMemberPath = nameof(FilterStep.DisplayName);
        FilterList.SelectedIndex = 0;

        AmountSlider.ValueChanged += OnParameterChanged;
        RadiusSlider.ValueChanged += OnParameterChanged;
        ThresholdSlider.ValueChanged += OnParameterChanged;
        BeforeButton.Checked += (_, _) => ShowBefore(true);
        BeforeButton.Unchecked += (_, _) => ShowBefore(false);

        Loaded += (_, _) => _ = RenderPreviewAsync();
        Closed += (_, _) =>
        {
            renderTimer.Stop();
            renderCancellation?.Cancel();
            renderCancellation?.Dispose();
        };
    }

    /// <summary>The filter the user confirmed, or null when cancelled.</summary>
    public FilterStep? Result { get; private set; }

    private FilterStep BuildStep() =>
        FilterList.SelectedItem is FilterStep selected
            ? selected with
            {
                Amount = AmountSlider.Value,
                Radius = RadiusSlider.Value,
                Threshold = ThresholdSlider.Value
            }
            : FilterStep.CreateDefault(PhotoFilterKind.Sharpen);

    private void OnFilterSelectionChanged(
        object sender,
        SelectionChangedEventArgs eventArgs)
    {
        if (FilterList.SelectedItem is not FilterStep selected)
        {
            return;
        }

        isLoading = true;
        try
        {
            var defaults = FilterStep.CreateDefault(selected.Kind);
            ParameterHeader.Text = defaults.DisplayName.ToUpperInvariant();
            AmountSlider.Value = defaults.Amount;
            RadiusSlider.Value = defaults.Radius;
            ThresholdSlider.Value = defaults.Threshold;
            RadiusSlider.Label = defaults.RadiusLabel;
            RadiusSlider.Visibility = defaults.UsesRadius
                ? Visibility.Visible
                : Visibility.Collapsed;
            ThresholdSlider.Visibility = defaults.UsesThreshold
                ? Visibility.Visible
                : Visibility.Collapsed;
        }
        finally
        {
            isLoading = false;
        }

        ScheduleRender();
    }

    private void OnParameterChanged(
        object? sender,
        RoutedPropertyChangedEventArgs<double> eventArgs)
    {
        if (!isLoading)
        {
            ScheduleRender();
        }
    }

    private void ScheduleRender()
    {
        renderTimer.Stop();
        renderTimer.Start();
    }

    private void ShowBefore(bool before)
    {
        PreviewBadge.Text = before ? "BEFORE" : "AFTER";
        if (before)
        {
            PreviewImage.Source = previewSource;
        }
        else
        {
            _ = RenderPreviewAsync();
        }
    }

    private async Task RenderPreviewAsync()
    {
        if (BeforeButton.IsChecked == true)
        {
            return;
        }

        renderCancellation?.Cancel();
        renderCancellation?.Dispose();
        renderCancellation = new CancellationTokenSource();
        var token = renderCancellation.Token;
        var step = BuildStep();

        PreviewStatus.Text = "Rendering…";
        try
        {
            var rendered = await Task.Run(
                () =>
                {
                    var buffer = PixelBuffer.FromBitmap(previewSource);
                    ImageFilters.Apply(buffer, step, token);
                    return buffer.ToBitmap();
                },
                token);
            if (token.IsCancellationRequested)
            {
                return;
            }

            PreviewImage.Source = rendered;
            PreviewStatus.Text = step.DisplayName;
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception exception)
        {
            PreviewStatus.Text = exception.Message;
        }
    }

    private void OnApplyClick(object sender, RoutedEventArgs eventArgs)
    {
        Result = BuildStep();
        DialogResult = true;
    }
}

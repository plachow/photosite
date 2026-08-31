using System.Globalization;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using Microsoft.Win32;
using PhotoSite.Domain;
using PhotoSite.Infrastructure;
using PhotoSite.Services.Batch;

namespace PhotoSite.Dialogs;

public partial class BatchDialog : Window
{
    private readonly IReadOnlyList<BatchSource> sources;
    private readonly BatchPresetStore presetStore;
    private readonly BatchProcessor processor;
    private readonly string presetKind;
    private CancellationTokenSource? runCancellation;
    private bool isLoading;
    private bool isRunning;

    internal BatchDialog(
        IReadOnlyList<BatchSource> sources,
        BatchPresetStore presetStore,
        BatchProcessor processor,
        string? initialDirectory,
        string presetKind = BatchPresetStore.BatchKind,
        BatchPreset? initialPreset = null)
    {
        this.sources = sources;
        this.presetStore = presetStore;
        this.processor = processor;
        this.presetKind = presetKind;
        InitializeComponent();
        DarkWindowChrome.Apply(this);

        if (presetKind == BatchPresetStore.ExportKind)
        {
            Title = "Export";
            RunButton.Content = "Export";
        }

        PopulateChoices();
        ApplyPreset(
            initialPreset
            ?? new BatchPreset
            {
                Name = "Current settings",
                OutputDirectory = initialDirectory
            });
        Loaded += async (_, _) => await LoadPresetsAsync();
    }

    /// <summary>Set once a run has written at least one file.</summary>
    public bool DidWriteFiles { get; private set; }

    public string? LastOutputDirectory { get; private set; }

    private void PopulateChoices()
    {
        isLoading = true;
        FormatBox.ItemsSource = new[]
        {
            new Choice<ImageOutputFormat>(ImageOutputFormat.Jpeg, "JPEG"),
            new Choice<ImageOutputFormat>(ImageOutputFormat.Png, "PNG"),
            new Choice<ImageOutputFormat>(ImageOutputFormat.WebP, "WebP"),
            new Choice<ImageOutputFormat>(ImageOutputFormat.Tiff, "TIFF"),
            new Choice<ImageOutputFormat>(ImageOutputFormat.Bmp, "BMP"),
            new Choice<ImageOutputFormat>(
                ImageOutputFormat.KeepOriginal,
                "Keep original format")
        };
        ResizeModeBox.ItemsSource = new[]
        {
            new Choice<BatchResizeMode>(BatchResizeMode.None, "Do not resize"),
            new Choice<BatchResizeMode>(BatchResizeMode.LongestSide, "Longest side"),
            new Choice<BatchResizeMode>(BatchResizeMode.ShortestSide, "Shortest side"),
            new Choice<BatchResizeMode>(BatchResizeMode.Width, "Width"),
            new Choice<BatchResizeMode>(BatchResizeMode.Height, "Height"),
            new Choice<BatchResizeMode>(BatchResizeMode.Percentage, "Percentage")
        };
        NameSourceBox.ItemsSource = new[]
        {
            new Choice<BatchNameSource>(
                BatchNameSource.OriginalName,
                "Original file name"),
            new Choice<BatchNameSource>(BatchNameSource.CustomText, "Custom text"),
            new Choice<BatchNameSource>(BatchNameSource.DateTaken, "Date taken")
        };
        OverwriteBox.ItemsSource = new[]
        {
            new Choice<BatchOverwritePolicy>(
                BatchOverwritePolicy.RenameUnique,
                "Add a number"),
            new Choice<BatchOverwritePolicy>(BatchOverwritePolicy.Skip, "Skip"),
            new Choice<BatchOverwritePolicy>(
                BatchOverwritePolicy.Overwrite,
                "Overwrite")
        };
        MetadataBox.ItemsSource = new[]
        {
            new Choice<BatchMetadataPolicy>(
                BatchMetadataPolicy.Preserve,
                "Keep all metadata"),
            new Choice<BatchMetadataPolicy>(
                BatchMetadataPolicy.RemoveLocation,
                "Keep, but remove GPS"),
            new Choice<BatchMetadataPolicy>(
                BatchMetadataPolicy.RemoveAll,
                "Remove all metadata")
        };
        isLoading = false;
    }

    private async Task LoadPresetsAsync()
    {
        try
        {
            var presets = await presetStore.LoadAsync(presetKind);
            PresetList.ItemsSource = presets;
            PresetList.DisplayMemberPath = nameof(BatchPreset.Name);
        }
        catch (Exception exception)
        {
            SummaryText.Text = $"Presets could not be loaded: {exception.Message}";
        }
    }

    private void OnPresetSelectionChanged(
        object sender,
        SelectionChangedEventArgs eventArgs)
    {
        if (isLoading || PresetList.SelectedItem is not BatchPreset preset)
        {
            return;
        }

        // A preset describes what to do, not where the user happens to be
        // working right now, so a stored blank folder must not wipe the
        // destination they already chose.
        var directory = string.IsNullOrWhiteSpace(preset.OutputDirectory)
            ? OutputDirectoryBox.Text
            : preset.OutputDirectory;
        ApplyPreset(preset with { OutputDirectory = directory });
    }

    private void ApplyPreset(BatchPreset preset)
    {
        isLoading = true;
        try
        {
            SelectChoice(FormatBox, preset.Format);
            QualitySlider.Value = Math.Clamp(preset.Quality, 1, 100);
            SelectChoice(ResizeModeBox, preset.ResizeMode);
            ResizeValueBox.Text = preset.ResizeValue.ToString(
                CultureInfo.InvariantCulture);
            AllowEnlargeBox.IsChecked = preset.AllowEnlarge;
            SharpenSlider.Value = Math.Clamp(preset.SharpenAmount, 0, 100);
            SelectChoice(NameSourceBox, preset.NameSource);
            NameDetailBox.Text = preset.NameSource == BatchNameSource.DateTaken
                ? preset.DateFormat
                : preset.CustomName;
            PrefixBox.Text = preset.Prefix;
            SuffixBox.Text = preset.Suffix;
            NumberingBox.IsChecked = preset.UseSequentialNumbering;
            NumberStartBox.Text = preset.NumberStart.ToString(
                CultureInfo.InvariantCulture);
            NumberDigitsBox.Text = preset.NumberDigits.ToString(
                CultureInfo.InvariantCulture);
            OutputDirectoryBox.Text = preset.OutputDirectory ?? string.Empty;
            UseSourceDirectoryBox.IsChecked = preset.UseSourceDirectory;
            DateSubfolderBox.IsChecked = preset.CreateSubfolderByDate;
            SelectChoice(OverwriteBox, preset.OverwritePolicy);
            SelectChoice(MetadataBox, preset.MetadataPolicy);
            ApplyEditsBox.IsChecked = preset.ApplyEdits;
        }
        finally
        {
            isLoading = false;
        }

        UpdateSummary();
    }

    internal BatchPreset BuildPreset(string name = "Current settings") =>
        new()
        {
            Name = name,
            Format = GetChoice(FormatBox, ImageOutputFormat.Jpeg),
            Quality = (int)Math.Round(QualitySlider.Value),
            ResizeMode = GetChoice(ResizeModeBox, BatchResizeMode.None),
            ResizeValue = ParseInt(ResizeValueBox.Text, 2048),
            AllowEnlarge = AllowEnlargeBox.IsChecked == true,
            SharpenAmount = SharpenSlider.Value,
            NameSource = GetChoice(NameSourceBox, BatchNameSource.OriginalName),
            CustomName =
                GetChoice(NameSourceBox, BatchNameSource.OriginalName)
                == BatchNameSource.CustomText
                    ? NameDetailBox.Text
                    : "photo",
            DateFormat =
                GetChoice(NameSourceBox, BatchNameSource.OriginalName)
                == BatchNameSource.DateTaken
                    ? NameDetailBox.Text
                    : "yyyy-MM-dd_HHmmss",
            Prefix = PrefixBox.Text,
            Suffix = SuffixBox.Text,
            UseSequentialNumbering = NumberingBox.IsChecked == true,
            NumberStart = ParseInt(NumberStartBox.Text, 1),
            NumberDigits = Math.Clamp(ParseInt(NumberDigitsBox.Text, 3), 1, 9),
            OutputDirectory = string.IsNullOrWhiteSpace(OutputDirectoryBox.Text)
                ? null
                : OutputDirectoryBox.Text.Trim().Trim('"'),
            UseSourceDirectory = UseSourceDirectoryBox.IsChecked == true,
            CreateSubfolderByDate = DateSubfolderBox.IsChecked == true,
            OverwritePolicy = GetChoice(
                OverwriteBox,
                BatchOverwritePolicy.RenameUnique),
            MetadataPolicy = GetChoice(MetadataBox, BatchMetadataPolicy.Preserve),
            ApplyEdits = ApplyEditsBox.IsChecked == true
        };

    private void OnSettingChanged(object sender, RoutedEventArgs eventArgs)
    {
        if (!isLoading)
        {
            UpdateSummary();
        }
    }

    private void OnSettingChanged(
        object sender,
        RoutedPropertyChangedEventArgs<double> eventArgs)
    {
        if (!isLoading)
        {
            UpdateSummary();
        }
    }

    /// <summary>
    /// Recomputes the plan on every keystroke so the summary line - and the
    /// sample file name - always describe exactly what Convert would do.
    /// </summary>
    private void UpdateSummary()
    {
        if (SummaryText is null)
        {
            return;
        }

        var preset = BuildPreset();
        UpdateFieldVisibility(preset);

        if (sources.Count == 0)
        {
            SummaryText.Text = "No photos are selected.";
            SampleText.Text = string.Empty;
            RunButton.IsEnabled = false;
            return;
        }

        var plan = BatchPlanner.Plan(sources, preset);
        var parts = new List<string>
        {
            $"{plan.WriteCount:N0} of {sources.Count:N0} photos"
        };
        if (plan.SkipCount > 0)
        {
            parts.Add($"{plan.SkipCount:N0} skipped");
        }

        if (plan.OverwriteCount > 0)
        {
            parts.Add($"{plan.OverwriteCount:N0} will be overwritten");
        }

        parts.Add(DescribeResize(preset));
        parts.Add(
            preset.Format == ImageOutputFormat.KeepOriginal
                ? "original format"
                : preset.Format.ToString().ToUpperInvariant()
                  + (preset.SupportsQuality ? $" q{preset.Quality}" : string.Empty));

        SummaryText.Text = string.Join(" · ", parts);

        var first = plan.Items.FirstOrDefault(item => !item.IsSkipped);
        SampleText.Text = first is null
            ? "Every photo would be skipped with these settings."
            : $"First output: {first.DestinationPath}";
        RunButton.IsEnabled = !isRunning && plan.WriteCount > 0;
    }

    private static string DescribeResize(BatchPreset preset) =>
        preset.ResizeMode switch
        {
            BatchResizeMode.None => "original size",
            BatchResizeMode.Percentage => $"{preset.ResizeValue}%",
            BatchResizeMode.Width => $"width {preset.ResizeValue} px",
            BatchResizeMode.Height => $"height {preset.ResizeValue} px",
            BatchResizeMode.ShortestSide =>
                $"shortest side {preset.ResizeValue} px",
            _ => $"longest side {preset.ResizeValue} px"
        };

    private void UpdateFieldVisibility(BatchPreset preset)
    {
        var showQuality = preset.SupportsQuality;
        QualityLabel.Visibility = showQuality
            ? Visibility.Visible
            : Visibility.Hidden;
        QualitySlider.IsEnabled = showQuality;

        var resizes = preset.ResizeMode != BatchResizeMode.None;
        ResizeValueBox.IsEnabled = resizes;
        AllowEnlargeBox.IsEnabled = resizes;
        ResizeUnitLabel.Text = preset.ResizeMode == BatchResizeMode.Percentage
            ? "%"
            : "px";

        NameDetailLabel.Text = preset.NameSource switch
        {
            BatchNameSource.CustomText => "Text",
            BatchNameSource.DateTaken => "Date format",
            _ => string.Empty
        };
        var needsDetail = preset.NameSource != BatchNameSource.OriginalName;
        NameDetailBox.Visibility = needsDetail
            ? Visibility.Visible
            : Visibility.Hidden;
        NameDetailLabel.Visibility = NameDetailBox.Visibility;

        NumberStartBox.IsEnabled = preset.UseSequentialNumbering;
        NumberDigitsBox.IsEnabled = preset.UseSequentialNumbering;
        OutputDirectoryBox.IsEnabled = !preset.UseSourceDirectory;
    }

    private void OnBrowseClick(object sender, RoutedEventArgs eventArgs)
    {
        var dialog = new OpenFolderDialog
        {
            Title = "Choose the output folder",
            Multiselect = false
        };
        if (Directory.Exists(OutputDirectoryBox.Text))
        {
            dialog.InitialDirectory = OutputDirectoryBox.Text;
        }

        if (dialog.ShowDialog(this) == true)
        {
            UseSourceDirectoryBox.IsChecked = false;
            OutputDirectoryBox.Text = dialog.FolderName;
        }
    }

    private async void OnSavePresetClick(object sender, RoutedEventArgs eventArgs)
    {
        var suggested = PresetList.SelectedItem is BatchPreset selected
            ? selected.Name
            : "My preset";
        var dialog = new TextPromptDialog(
            "Save preset",
            "Preset name",
            suggested)
        {
            Owner = this
        };
        if (dialog.ShowDialog() != true
            || string.IsNullOrWhiteSpace(dialog.Value))
        {
            return;
        }

        try
        {
            await presetStore.SaveAsync(
                BuildPreset(dialog.Value.Trim()),
                presetKind);
            await LoadPresetsAsync();
        }
        catch (Exception exception)
        {
            SummaryText.Text = $"The preset could not be saved: {exception.Message}";
        }
    }

    private async void OnDeletePresetClick(object sender, RoutedEventArgs eventArgs)
    {
        if (PresetList.SelectedItem is not BatchPreset preset)
        {
            return;
        }

        if (MessageBox.Show(
                this,
                $"Delete the preset “{preset.Name}”?",
                "Delete preset",
                MessageBoxButton.YesNo,
                MessageBoxImage.Question,
                MessageBoxResult.No) != MessageBoxResult.Yes)
        {
            return;
        }

        try
        {
            await presetStore.DeleteAsync(preset.Name, presetKind);
            await LoadPresetsAsync();
        }
        catch (Exception exception)
        {
            SummaryText.Text =
                $"The preset could not be deleted: {exception.Message}";
        }
    }

    private async void OnRunClick(object sender, RoutedEventArgs eventArgs)
    {
        if (isRunning)
        {
            runCancellation?.Cancel();
            return;
        }

        var preset = BuildPreset();
        if (!preset.UseSourceDirectory)
        {
            if (string.IsNullOrWhiteSpace(preset.OutputDirectory))
            {
                MessageBox.Show(
                    this,
                    "Choose an output folder, or save next to the originals.",
                    "No destination",
                    MessageBoxButton.OK,
                    MessageBoxImage.Warning);
                return;
            }

            try
            {
                Directory.CreateDirectory(preset.OutputDirectory);
            }
            catch (Exception exception)
            {
                MessageBox.Show(
                    this,
                    $"The output folder could not be created.\n\n{exception.Message}",
                    "No destination",
                    MessageBoxButton.OK,
                    MessageBoxImage.Error);
                return;
            }
        }

        var plan = BatchPlanner.Plan(sources, preset);
        if (plan.OverwriteCount > 0
            && MessageBox.Show(
                this,
                $"{plan.OverwriteCount:N0} existing file(s) will be replaced.\n\n"
                + "Continue?",
                "Replace existing files?",
                MessageBoxButton.YesNo,
                MessageBoxImage.Warning,
                MessageBoxResult.No) != MessageBoxResult.Yes)
        {
            return;
        }

        runCancellation = new CancellationTokenSource();
        SetRunningState(true);
        var progress = new Progress<BatchProgress>(report =>
        {
            BatchProgress.Value = report.Fraction * 100;
            SummaryText.Text = report.CurrentFile.Length == 0
                ? "Finishing…"
                : $"{report.Completed:N0} / {report.Total:N0} · {report.CurrentFile}";
        });

        try
        {
            var outcome = await processor.RunAsync(
                plan,
                preset,
                progress,
                runCancellation.Token);
            DidWriteFiles |= outcome.Written > 0;
            LastOutputDirectory = preset.UseSourceDirectory
                ? null
                : preset.OutputDirectory;
            SummaryText.Text = outcome.Describe();
            SampleText.Text = outcome.Errors.Count == 0
                ? "Done."
                : string.Join(" · ", outcome.Errors.Take(3));
            if (outcome.Errors.Count > 0)
            {
                MessageBox.Show(
                    this,
                    string.Join(Environment.NewLine, outcome.Errors.Take(12))
                    + (outcome.Errors.Count > 12
                        ? $"{Environment.NewLine}…and {outcome.Errors.Count - 12:N0} more"
                        : string.Empty),
                    "Batch finished with errors",
                    MessageBoxButton.OK,
                    MessageBoxImage.Warning);
            }
        }
        catch (Exception exception)
        {
            SummaryText.Text = $"The batch failed: {exception.Message}";
        }
        finally
        {
            runCancellation?.Dispose();
            runCancellation = null;
            SetRunningState(false);
        }
    }

    private void SetRunningState(bool running)
    {
        isRunning = running;
        BatchProgress.Visibility = running ? Visibility.Visible : Visibility.Collapsed;
        BatchProgress.Value = 0;
        RunButton.Content = running
            ? "Stop"
            : presetKind == BatchPresetStore.ExportKind
                ? "Export"
                : "Convert";
        RunButton.IsEnabled = true;
        CloseButton.IsEnabled = !running;
        PresetList.IsEnabled = !running;
        if (!running)
        {
            UpdateSummary();
        }
    }

    private void OnCloseClick(object sender, RoutedEventArgs eventArgs)
    {
        DialogResult = DidWriteFiles;
    }

    protected override void OnClosing(System.ComponentModel.CancelEventArgs e)
    {
        if (isRunning)
        {
            // Cancelling mid-file would leave a partial output behind, so the
            // window stays open until the current photograph is finished.
            e.Cancel = true;
            runCancellation?.Cancel();
            return;
        }

        base.OnClosing(e);
    }

    private static void SelectChoice<T>(Selector box, T value)
        where T : struct, Enum
    {
        foreach (var item in box.ItemsSource.OfType<Choice<T>>())
        {
            if (EqualityComparer<T>.Default.Equals(item.Value, value))
            {
                box.SelectedItem = item;
                return;
            }
        }

        box.SelectedIndex = 0;
    }

    private static T GetChoice<T>(Selector box, T fallback)
        where T : struct, Enum =>
        box.SelectedItem is Choice<T> choice ? choice.Value : fallback;

    private static int ParseInt(string? text, int fallback) =>
        int.TryParse(
            text,
            NumberStyles.Integer,
            CultureInfo.InvariantCulture,
            out var value)
            ? value
            : fallback;

    private sealed record Choice<T>(T Value, string Label)
        where T : struct, Enum
    {
        public override string ToString() => Label;
    }
}

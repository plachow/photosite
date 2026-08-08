using System.Windows;
using System.Windows.Controls;
using Microsoft.Win32;
using PhotoSite.Services;
using PhotoSite.ViewModels;

namespace PhotoSite.Dialogs;

public partial class ImportDialog : Window
{
    private readonly ImportService importer;
    private CancellationTokenSource? runCancellation;
    private ImportPlan? plan;
    private bool isRunning;

    internal ImportDialog(
        ImportService importer,
        string? initialDestination)
    {
        this.importer = importer;
        InitializeComponent();

        DestinationBox.Text = initialDestination
            ?? Environment.GetFolderPath(Environment.SpecialFolder.MyPictures);
        DateFolderBox.Text = @"yyyy\yyyy-MM-dd";
        RenamePatternBox.Text = "yyyyMMdd_HHmmss";

        BuildDetectedSources();
        Loaded += (_, _) => UpdateSummary();
    }

    public bool DidImport { get; private set; }

    public string? ImportedInto { get; private set; }

    /// <summary>
    /// Offers whatever looks like a camera or a card up front, so the common
    /// case is one click rather than a folder hunt.
    /// </summary>
    private void BuildDetectedSources()
    {
        var sources = ImportService.FindRemovableSources();
        if (sources.Count == 0)
        {
            DetectedSourcesPanel.Children.Add(new TextBlock
            {
                Text = "No camera or memory card was detected.",
                FontSize = 11,
                Foreground = (System.Windows.Media.Brush)FindResource(
                    "MutedTextBrush")
            });
            return;
        }

        foreach (var (label, path) in sources)
        {
            var button = new Button
            {
                Content = label,
                Tag = path,
                Padding = new Thickness(9, 3, 9, 3),
                Margin = new Thickness(0, 0, 5, 0),
                ToolTip = path
            };
            button.Click += (_, _) =>
            {
                SourceBox.Text = path;
                UpdateSummary();
            };
            DetectedSourcesPanel.Children.Add(button);
        }
    }

    private ImportOptions BuildOptions() => new(
        SourceBox.Text.Trim().Trim('"'),
        DestinationBox.Text.Trim().Trim('"'),
        IncludeSubfoldersBox.IsChecked == true,
        OrganizeByDateBox.IsChecked == true,
        DateFolderBox.Text,
        RenameBox.IsChecked == true,
        RenamePatternBox.Text,
        SkipImportedBox.IsChecked == true,
        BackupBox.IsChecked == true && !string.IsNullOrWhiteSpace(BackupBox2.Text)
            ? BackupBox2.Text.Trim().Trim('"')
            : null,
        MoveBox.IsChecked == true);

    private void OnSettingChanged(object sender, RoutedEventArgs eventArgs) =>
        UpdateSummary();

    private void OnSettingChanged(object sender, TextChangedEventArgs eventArgs) =>
        UpdateSummary();

    private void UpdateSummary()
    {
        if (SummaryText is null || isRunning)
        {
            return;
        }

        DateFolderBox.IsEnabled = OrganizeByDateBox.IsChecked == true;
        RenamePatternBox.IsEnabled = RenameBox.IsChecked == true;
        BackupBox2.IsEnabled = BackupBox.IsChecked == true;

        var options = BuildOptions();
        if (!Directory.Exists(options.SourceDirectory))
        {
            SummaryText.Text = "Choose a source folder, camera or memory card.";
            SampleText.Text = string.Empty;
            ImportButton.IsEnabled = false;
            plan = null;
            return;
        }

        if (string.IsNullOrWhiteSpace(options.DestinationDirectory))
        {
            SummaryText.Text = "Choose where the photos should be imported.";
            SampleText.Text = string.Empty;
            ImportButton.IsEnabled = false;
            plan = null;
            return;
        }

        try
        {
            plan = importer.Plan(options);
        }
        catch (Exception exception)
        {
            SummaryText.Text = $"The source could not be read: {exception.Message}";
            ImportButton.IsEnabled = false;
            return;
        }

        var parts = new List<string>
        {
            $"{plan.NewCount:N0} new photo(s)"
        };
        if (plan.DuplicateCount > 0)
        {
            parts.Add($"{plan.DuplicateCount:N0} already imported");
        }

        parts.Add(PhotoItemViewModel.FormatFileSize(plan.TotalBytes));
        if (options.BackupDirectory is not null)
        {
            parts.Add("plus a second copy");
        }

        SummaryText.Text = string.Join(" · ", parts);
        var first = plan.Candidates.FirstOrDefault(item => !item.IsDuplicate);
        SampleText.Text = first is null
            ? "Nothing new to import from this source."
            : $"First file: {first.DestinationPath}";
        ImportButton.IsEnabled = plan.NewCount > 0;
    }

    private void OnBrowseSourceClick(object sender, RoutedEventArgs eventArgs) =>
        BrowseInto(SourceBox, "Choose the source folder");

    private void OnBrowseDestinationClick(
        object sender,
        RoutedEventArgs eventArgs) =>
        BrowseInto(DestinationBox, "Choose the destination folder");

    private void OnBrowseBackupClick(object sender, RoutedEventArgs eventArgs)
    {
        BackupBox.IsChecked = true;
        BrowseInto(BackupBox2, "Choose the backup folder");
    }

    private void BrowseInto(TextBox target, string title)
    {
        var dialog = new OpenFolderDialog
        {
            Title = title,
            Multiselect = false
        };
        if (Directory.Exists(target.Text))
        {
            dialog.InitialDirectory = target.Text;
        }

        if (dialog.ShowDialog(this) == true)
        {
            target.Text = dialog.FolderName;
            UpdateSummary();
        }
    }

    private async void OnImportClick(object sender, RoutedEventArgs eventArgs)
    {
        if (isRunning)
        {
            runCancellation?.Cancel();
            return;
        }

        var options = BuildOptions();
        if (plan is null || plan.NewCount == 0)
        {
            return;
        }

        if (options.MoveInsteadOfCopy
            && MessageBox.Show(
                this,
                $"{plan.NewCount:N0} file(s) will be copied and then deleted "
                + "from the source.\n\nContinue?",
                "Delete the originals?",
                MessageBoxButton.YesNo,
                MessageBoxImage.Warning,
                MessageBoxResult.No) != MessageBoxResult.Yes)
        {
            return;
        }

        runCancellation = new CancellationTokenSource();
        SetRunningState(true);
        var progress = new Progress<ImportProgress>(report =>
        {
            ImportProgressBar.Value = report.Total == 0
                ? 0
                : report.Completed * 100d / report.Total;
            SummaryText.Text = report.CurrentFile.Length == 0
                ? "Finishing…"
                : $"{report.Completed:N0} / {report.Total:N0} · {report.CurrentFile}";
        });

        try
        {
            var outcome = await importer.RunAsync(
                plan,
                options,
                progress,
                runCancellation.Token);
            DidImport |= outcome.Imported > 0;
            ImportedInto = outcome.DestinationDirectory;
            SummaryText.Text =
                $"Imported {outcome.Imported:N0} · skipped {outcome.Skipped:N0}"
                + (outcome.Failed > 0 ? $" · failed {outcome.Failed:N0}" : string.Empty);
            SampleText.Text = outcome.Errors.Count == 0
                ? "Done."
                : string.Join(" · ", outcome.Errors.Take(3));
        }
        catch (OperationCanceledException)
        {
            SummaryText.Text = "Import cancelled.";
        }
        catch (Exception exception)
        {
            SummaryText.Text = $"The import failed: {exception.Message}";
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
        ImportProgressBar.Visibility = running
            ? Visibility.Visible
            : Visibility.Collapsed;
        ImportProgressBar.Value = 0;
        ImportButton.Content = running ? "Stop" : "Import";
        CloseButton.IsEnabled = !running;
        if (!running)
        {
            UpdateSummary();
        }
    }

    private void OnCloseClick(object sender, RoutedEventArgs eventArgs) =>
        DialogResult = DidImport;

    protected override void OnClosing(System.ComponentModel.CancelEventArgs e)
    {
        if (isRunning)
        {
            e.Cancel = true;
            runCancellation?.Cancel();
            return;
        }

        base.OnClosing(e);
    }
}

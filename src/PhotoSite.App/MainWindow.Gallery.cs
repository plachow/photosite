using System.Globalization;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using PhotoSite.Dialogs;
using PhotoSite.Domain;
using PhotoSite.ViewModels;

namespace PhotoSite;

/// <summary>
/// The Manager half of the window: gallery filtering, organisation and the
/// file operations that act on the selected photographs.
/// </summary>
public partial class MainWindow
{
    private readonly List<CheckBox> labelFilterBoxes = [];
    private readonly List<System.Windows.Controls.Primitives.ToggleButton>
        personFilterChips = [];
    private bool isFilterUiUpdating;

    /// <summary>
    /// One toggle chip per named person; several checked chips mean "all of
    /// them together on the photo". Selections survive a rebuild as long as
    /// the person still exists.
    /// </summary>
    private void RebuildPersonFilterChips(IReadOnlyList<PersonRecord> people)
    {
        var checkedIds = personFilterChips
            .Where(chip => chip.IsChecked == true)
            .Select(chip => ((PersonRecord)chip.Tag).Id)
            .ToHashSet();
        personFilterChips.Clear();
        PersonFilterPanel.Children.Clear();

        foreach (var person in people)
        {
            var chip = new System.Windows.Controls.Primitives.ToggleButton
            {
                Tag = person,
                IsChecked = checkedIds.Contains(person.Id),
                Style = (Style)FindResource("PersonChipToggleStyle"),
                ToolTip = $"{person.Name} · {person.FaceCount:N0} faces",
                Content = BuildPersonChipContent(person.Id, person.Name)
            };
            System.Windows.Automation.AutomationProperties.SetName(
                chip,
                $"Filter by {person.Name}");
            chip.Click += OnFilterChanged;
            personFilterChips.Add(chip);
            PersonFilterPanel.Children.Add(chip);
        }

        if (people.Count == 0)
        {
            PersonFilterPanel.Children.Add(new TextBlock
            {
                FontSize = 11,
                Foreground = (Brush)FindResource("MutedTextBrush"),
                Text = "No people named yet"
            });
        }
    }

    internal static StackPanel BuildPersonChipContent(long personId, string name)
    {
        var panel = new StackPanel { Orientation = Orientation.Horizontal };
        panel.Children.Add(new System.Windows.Shapes.Ellipse
        {
            Width = 8,
            Height = 8,
            Margin = new Thickness(0, 1, 5, 0),
            VerticalAlignment = VerticalAlignment.Center,
            Fill = Infrastructure.PersonBrushes.Get(personId)
        });
        panel.Children.Add(new TextBlock
        {
            FontSize = 11,
            Text = name
        });
        return panel;
    }

    private void BuildLabelFilters()
    {
        foreach (var label in Enum.GetValues<ColorLabel>())
        {
            if (label == ColorLabel.None)
            {
                continue;
            }

            var box = new CheckBox
            {
                Margin = new Thickness(0, 0, 8, 0),
                Tag = label,
                ToolTip = label.ToString(),
                Content = new Border
                {
                    Width = 15,
                    Height = 15,
                    CornerRadius = new CornerRadius(4),
                    Background = Infrastructure.PhotoLabelBrushes.Get(label)
                }
            };
            box.Click += OnFilterChanged;
            labelFilterBoxes.Add(box);
            LabelFilterPanel.Children.Add(box);
        }
    }

    /// <summary>
    /// Ctrl+wheel resizes the thumbnails instead of scrolling, matching the
    /// gesture the viewer already uses for zoom.
    /// </summary>
    private void OnPhotoListPreviewMouseWheel(
        object sender,
        MouseWheelEventArgs eventArgs)
    {
        if ((Keyboard.Modifiers & ModifierKeys.Control) == 0
            || !viewModel.IsGridView)
        {
            return;
        }

        viewModel.ThumbnailSize += eventArgs.Delta > 0 ? 24 : -24;
        eventArgs.Handled = true;
    }

    /// <summary>
    /// The five label swatches in the info panel. Clicking the label a photo
    /// already carries clears it, so one row of buttons both sets and unsets.
    /// </summary>
    private void BuildLabelPicker()
    {
        foreach (var label in Enum.GetValues<ColorLabel>())
        {
            if (label == ColorLabel.None)
            {
                continue;
            }

            var button = new Button
            {
                Width = 20,
                Height = 20,
                Margin = new Thickness(0, 0, 4, 0),
                Padding = new Thickness(0),
                Background = Infrastructure.PhotoLabelBrushes.Get(label),
                BorderBrush = new SolidColorBrush(Color.FromRgb(0x4A, 0x51, 0x60)),
                Tag = label,
                ToolTip = $"{label} label"
            };
            System.Windows.Automation.AutomationProperties.SetName(
                button,
                $"{label} colour label");
            button.Click += OnColorLabelClick;
            LabelPickerPanel.Children.Add(button);
        }
    }

    private void OnColorLabelClick(object sender, RoutedEventArgs eventArgs)
    {
        if (sender is not Button { Tag: ColorLabel label })
        {
            return;
        }

        var reference = viewModel.SelectedPhoto;
        var target = reference?.ColorLabel == label ? ColorLabel.None : label;
        ApplyToSelection(photo => photo.ColorLabel = target);
    }

    private void OnPickClick(object sender, RoutedEventArgs eventArgs) =>
        ToggleFlag(PhotoFlag.Picked);

    private void OnRejectClick(object sender, RoutedEventArgs eventArgs) =>
        ToggleFlag(PhotoFlag.Rejected);

    private void OnOpenMapClick(object sender, RoutedEventArgs eventArgs)
    {
        if (viewModel.SelectedPhoto?.MapUrl is not { } url)
        {
            return;
        }

        try
        {
            System.Diagnostics.Process.Start(
                new System.Diagnostics.ProcessStartInfo(url)
                {
                    UseShellExecute = true
                });
        }
        catch (Exception exception)
        {
            viewModel.ReportStatus($"The map could not be opened: {exception.Message}");
        }
    }

    private void OnFilterChanged(object sender, RoutedEventArgs eventArgs) =>
        ApplyFilterFromUi();

    private void OnFilterChanged(
        object sender,
        SelectionChangedEventArgs eventArgs) =>
        ApplyFilterFromUi();

    private void OnFilterChanged(object sender, TextChangedEventArgs eventArgs) =>
        ApplyFilterFromUi();

    private void ApplyFilterFromUi()
    {
        if (isFilterUiUpdating)
        {
            return;
        }

        var labels = labelFilterBoxes
            .Where(box => box.IsChecked == true)
            .Select(box => (ColorLabel)box.Tag)
            .ToHashSet();

        var flags = new HashSet<PhotoFlag>();
        if (PickedFilterBox.IsChecked == true)
        {
            flags.Add(PhotoFlag.Picked);
        }

        if (RejectedFilterBox.IsChecked == true)
        {
            flags.Add(PhotoFlag.Rejected);
        }

        var formats = FormatFilterItems.Items
            .OfType<string>()
            .Where(format => IsFormatChecked(format))
            .ToHashSet(StringComparer.OrdinalIgnoreCase);

        var cameras = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        if (CameraFilterBox.SelectedItem is string camera)
        {
            cameras.Add(camera);
        }

        var lenses = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        if (LensFilterBox.SelectedItem is string lens)
        {
            lenses.Add(lens);
        }

        viewModel.Filter = new PhotoFilterCriteria
        {
            MinimumRating = viewModel.MinimumRating,
            SearchText = viewModel.SearchText,
            ColorLabels = labels,
            Flags = flags,
            Formats = formats,
            Cameras = cameras,
            Lenses = lenses,
            PersonIds = personFilterChips
                .Where(chip => chip.IsChecked == true)
                .Select(chip => ((PersonRecord)chip.Tag).Id)
                .ToHashSet(),
            PersonNames = personFilterChips
                .Where(chip => chip.IsChecked == true)
                .Select(chip => ((PersonRecord)chip.Tag).Name)
                .ToArray(),
            Orientation = LandscapeOrientationBox.IsChecked == true
                ? PhotoOrientation.Landscape
                : PortraitOrientationBox.IsChecked == true
                    ? PhotoOrientation.Portrait
                    : PhotoOrientation.Unknown,
            TakenFrom = ParseFilterDate(DateFromBox.Text, endOfPeriod: false),
            TakenTo = ParseFilterDate(DateToBox.Text, endOfPeriod: true),
            HideRejected = HideRejectedBox.IsChecked == true
        };
    }

    private bool IsFormatChecked(string format)
    {
        var container = FormatFilterItems.ItemContainerGenerator
            .ContainerFromItem(format);
        return FindChild<CheckBox>(container)?.IsChecked == true;
    }

    /// <summary>
    /// Accepts a year, a year and month, or a full date, and widens the "to"
    /// bound to the end of whatever period was typed - "2025" as an end date
    /// has to mean the last moment of 2025, not its first.
    /// </summary>
    internal static DateTime? ParseFilterDate(string? text, bool endOfPeriod)
    {
        var trimmed = text?.Trim();
        if (string.IsNullOrEmpty(trimmed))
        {
            return null;
        }

        if (DateTime.TryParseExact(
                trimmed,
                "yyyy",
                CultureInfo.InvariantCulture,
                DateTimeStyles.None,
                out var year))
        {
            return endOfPeriod ? year.AddYears(1).AddTicks(-1) : year;
        }

        if (DateTime.TryParseExact(
                trimmed,
                ["yyyy-MM", "yyyy/MM", "MM.yyyy"],
                CultureInfo.InvariantCulture,
                DateTimeStyles.None,
                out var month))
        {
            return endOfPeriod ? month.AddMonths(1).AddTicks(-1) : month;
        }

        if (DateTime.TryParse(
                trimmed,
                CultureInfo.CurrentCulture,
                DateTimeStyles.None,
                out var date)
            || DateTime.TryParse(
                trimmed,
                CultureInfo.InvariantCulture,
                DateTimeStyles.None,
                out date))
        {
            return endOfPeriod ? date.Date.AddDays(1).AddTicks(-1) : date.Date;
        }

        return null;
    }

    private void OnClearFilterClick(object sender, RoutedEventArgs eventArgs)
    {
        isFilterUiUpdating = true;
        try
        {
            foreach (var box in labelFilterBoxes)
            {
                box.IsChecked = false;
            }

            PickedFilterBox.IsChecked = false;
            RejectedFilterBox.IsChecked = false;
            HideRejectedBox.IsChecked = false;
            AnyOrientationBox.IsChecked = true;
            CameraFilterBox.SelectedItem = null;
            LensFilterBox.SelectedItem = null;
            foreach (var chip in personFilterChips)
            {
                chip.IsChecked = false;
            }
            DateFromBox.Text = string.Empty;
            DateToBox.Text = string.Empty;
            foreach (var format in FormatFilterItems.Items.OfType<string>())
            {
                var container = FormatFilterItems.ItemContainerGenerator
                    .ContainerFromItem(format);
                if (FindChild<CheckBox>(container) is { } box)
                {
                    box.IsChecked = false;
                }
            }

            viewModel.MinimumRating = 0;
        }
        finally
        {
            isFilterUiUpdating = false;
        }

        ApplyFilterFromUi();
    }

    private async void OnImportClick(object sender, RoutedEventArgs eventArgs) =>
        await ImportPhotosAsync();

    private async Task ImportPhotosAsync()
    {
        var dialog = new ImportDialog(
            App.Services.Importer,
            viewModel.CurrentFolder)
        {
            Owner = this
        };
        dialog.ShowDialog();

        if (!dialog.DidImport || dialog.ImportedInto is not { } destination)
        {
            return;
        }

        // Land the user in what they just imported rather than leaving them
        // in whatever folder they happened to be browsing.
        await viewModel.NavigateToAsync(destination);
        viewModel.ReportStatus($"Imported into {destination}");
    }

    private void OnCompareClick(object sender, RoutedEventArgs eventArgs) =>
        CompareSelectedPhotos();

    private void CompareSelectedPhotos()
    {
        var selection = GetSelectedManagerPhotos()
            .Where(photo => File.Exists(photo.Path))
            .Take(CompareWindow.MaximumPhotos)
            .ToArray();
        if (selection.Length < 2)
        {
            viewModel.ReportStatus(
                "Select two to four photos to compare them side by side");
            return;
        }

        new CompareWindow(selection)
        {
            Owner = this
        }.ShowDialog();
    }

    private async void OnBatchToolbarClick(
        object sender,
        RoutedEventArgs eventArgs) =>
        await RunBatchConversionAsync(GetSelectedManagerPhotos());

    private async void OnCreateFolderClick(
        object sender,
        RoutedEventArgs eventArgs) =>
        await CreateFolderAsync();

    private async Task CreateFolderAsync()
    {
        if (viewModel.CurrentFolder is not { } parent || !Directory.Exists(parent))
        {
            viewModel.ReportStatus("Open a folder first");
            return;
        }

        var dialog = new TextPromptDialog(
            "New folder",
            $"Create a folder inside {parent}",
            "New folder",
            "Create")
        {
            Owner = this
        };
        if (dialog.ShowDialog() != true || dialog.Value is not { } name)
        {
            return;
        }

        try
        {
            var created = Directory.CreateDirectory(Path.Combine(parent, name));
            viewModel.ReportStatus($"Created folder: {created.Name}");
            await viewModel.DirectoryTree.SelectPathAsync(
                parent,
                notifySelection: false);
        }
        catch (Exception exception) when (
            exception is IOException
                or UnauthorizedAccessException
                or ArgumentException
                or NotSupportedException)
        {
            MessageBox.Show(
                this,
                $"The folder could not be created.\n\n{exception.Message}",
                "New folder",
                MessageBoxButton.OK,
                MessageBoxImage.Error);
        }
    }

    private async void OnRenamePhotoClick(
        object sender,
        RoutedEventArgs eventArgs) =>
        await RenamePhotoAsync(GetContextPhotos(sender).FirstOrDefault());

    private async Task RenamePhotoAsync(PhotoItemViewModel? photo)
    {
        if (photo is null || !File.Exists(photo.Path))
        {
            return;
        }

        var dialog = new TextPromptDialog(
            "Rename",
            "New file name",
            photo.FileName,
            "Rename")
        {
            Owner = this
        };
        if (dialog.ShowDialog() != true || dialog.Value is not { } name)
        {
            return;
        }

        var directory = Path.GetDirectoryName(photo.Path)!;
        var target = Path.Combine(directory, name);
        if (string.Equals(target, photo.Path, StringComparison.OrdinalIgnoreCase))
        {
            return;
        }

        try
        {
            if (File.Exists(target))
            {
                MessageBox.Show(
                    this,
                    $"{name} already exists in this folder.",
                    "Rename",
                    MessageBoxButton.OK,
                    MessageBoxImage.Warning);
                return;
            }

            File.Move(photo.Path, target);
            MoveSidecarWithPhoto(photo.Path, target);
            viewModel.ReportStatus($"Renamed to {Path.GetFileName(target)}");
            await RefreshCurrentCatalogAsync();
        }
        catch (Exception exception) when (
            exception is IOException
                or UnauthorizedAccessException
                or ArgumentException
                or NotSupportedException)
        {
            MessageBox.Show(
                this,
                $"The file could not be renamed.\n\n{exception.Message}",
                "Rename",
                MessageBoxButton.OK,
                MessageBoxImage.Error);
        }
    }

    private async void OnDuplicatePhotoClick(
        object sender,
        RoutedEventArgs eventArgs) =>
        await DuplicatePhotosAsync(GetContextPhotos(sender));

    private async Task DuplicatePhotosAsync(
        IReadOnlyCollection<PhotoItemViewModel> photos)
    {
        var duplicated = 0;
        var failures = new List<string>();
        foreach (var photo in photos.Where(photo => File.Exists(photo.Path)))
        {
            var directory = Path.GetDirectoryName(photo.Path)!;
            var plan = Services.PhotoFileOperations.Plan(
                photo.Path,
                directory,
                Services.PhotoFileTransferMode.Copy);
            try
            {
                await Services.PhotoFileOperations.ExecuteAsync(
                    plan,
                    Services.PhotoFileTransferMode.Copy,
                    overwrite: false);
                duplicated++;
            }
            catch (Exception exception) when (
                exception is IOException or UnauthorizedAccessException)
            {
                failures.Add($"{photo.FileName}: {exception.Message}");
            }
        }

        if (duplicated > 0)
        {
            await RefreshCurrentCatalogAsync();
        }

        viewModel.ReportStatus(
            failures.Count == 0
                ? $"Duplicated {duplicated:N0} file(s)"
                : $"Duplicated {duplicated:N0} · {failures.Count:N0} failed");
    }

    private static void MoveSidecarWithPhoto(string sourcePath, string targetPath)
    {
        if (!Services.ExifToolMetadataWriter.UsesSidecar(
                Path.GetExtension(sourcePath)))
        {
            return;
        }

        var sourceSidecar = Services.ExifToolMetadataWriter.GetSidecarPath(sourcePath);
        if (!File.Exists(sourceSidecar))
        {
            return;
        }

        try
        {
            File.Move(
                sourceSidecar,
                Services.ExifToolMetadataWriter.GetSidecarPath(targetPath));
        }
        catch (Exception exception) when (
            exception is IOException or UnauthorizedAccessException)
        {
            // Losing the sidecar move is recoverable; the photo already moved.
        }
    }

    /// <summary>
    /// Applies a rating, colour label or flag to everything currently
    /// selected, which is what makes keyboard culling worthwhile.
    /// </summary>
    private void ApplyToSelection(Action<PhotoItemViewModel> action)
    {
        var targets = viewModel.IsEditorMode || viewModel.IsFullscreenMode
            ? viewModel.SelectedPhoto is { IsTransient: false } current
                ? [current]
                : Array.Empty<PhotoItemViewModel>()
            : GetSelectedManagerPhotos().ToArray();
        foreach (var photo in targets)
        {
            action(photo);
        }
    }

    private bool TryHandleOrganizationShortcut(Key key, ModifierKeys modifiers)
    {
        if (modifiers != ModifierKeys.None)
        {
            return false;
        }

        switch (key)
        {
            case Key.D6:
                ApplyToSelection(photo => photo.ColorLabel = ColorLabel.Red);
                return true;
            case Key.D7:
                ApplyToSelection(photo => photo.ColorLabel = ColorLabel.Yellow);
                return true;
            case Key.D8:
                ApplyToSelection(photo => photo.ColorLabel = ColorLabel.Green);
                return true;
            case Key.D9:
                ApplyToSelection(photo => photo.ColorLabel = ColorLabel.Blue);
                return true;
            case Key.D0:
                ApplyToSelection(photo => photo.ColorLabel = ColorLabel.None);
                return true;
            case Key.P:
                ToggleFlag(PhotoFlag.Picked);
                return true;
            case Key.X:
                ToggleFlag(PhotoFlag.Rejected);
                return true;
            default:
                return false;
        }
    }

    private void ToggleFlag(PhotoFlag flag)
    {
        // Pressing the same key again clears the flag, so one key both marks
        // and un-marks during a fast culling pass.
        var reference = viewModel.SelectedPhoto;
        var target = reference?.Flag == flag ? PhotoFlag.None : flag;
        ApplyToSelection(photo => photo.Flag = target);
        viewModel.ReportStatus(
            target switch
            {
                PhotoFlag.Picked => "Marked as picked",
                PhotoFlag.Rejected => "Marked as rejected",
                _ => "Flag cleared"
            });
    }

    internal void ValidateManagerChromeForSmokeTest()
    {
        var size = new Size(1500, 900);
        if (Content is not UIElement content)
        {
            throw new InvalidOperationException("The main window has no content.");
        }

        content.Measure(size);
        content.Arrange(new Rect(size));
        content.UpdateLayout();

        if (!ReferenceEquals(InfoPanel.DataContext, viewModel.SelectedPhoto))
        {
            throw new InvalidOperationException(
                "The information panel must follow the gallery selection.");
        }

        var tilePanel = FindChild<Controls.VirtualizingTilePanel>(PhotoList)
            ?? throw new InvalidOperationException(
                "The gallery did not create its tile panel.");
        if (Math.Abs(tilePanel.ItemWidth - viewModel.ThumbnailSize) > 0.5)
        {
            throw new InvalidOperationException(
                "Tile width must follow the thumbnail size setting.");
        }

        var originalSize = viewModel.ThumbnailSize;
        viewModel.ThumbnailSize = originalSize + 60;
        content.UpdateLayout();
        if (Math.Abs(tilePanel.ItemWidth - viewModel.ThumbnailSize) > 0.5)
        {
            throw new InvalidOperationException(
                "Resizing the thumbnails must re-measure the tile panel.");
        }

        viewModel.ThumbnailSize = MainViewModel.MaximumThumbnailSize + 1000;
        if (viewModel.ThumbnailSize > MainViewModel.MaximumThumbnailSize)
        {
            throw new InvalidOperationException(
                "The thumbnail size must stay inside its supported range.");
        }

        viewModel.ThumbnailSize = originalSize;

        var gridTemplate = PhotoList.ItemTemplate;
        viewModel.ViewMode = GalleryViewMode.Details;
        content.UpdateLayout();
        if (ReferenceEquals(PhotoList.ItemTemplate, gridTemplate))
        {
            throw new InvalidOperationException(
                "Switching to Details must swap the gallery row template.");
        }

        viewModel.ViewMode = GalleryViewMode.Grid;
        content.UpdateLayout();
        if (!ReferenceEquals(PhotoList.ItemTemplate, gridTemplate))
        {
            throw new InvalidOperationException(
                "Switching back to Grid must restore the tile template.");
        }

        var navigationHeight = NavigationBar.ActualHeight;
        if (navigationHeight <= 0)
        {
            throw new InvalidOperationException(
                "The navigation bar must be visible in Manager.");
        }

        // Changing the filter re-presents from the indexed photos, which this
        // harness populated directly, so the gallery is restored afterwards.
        var presentedPhotos = viewModel.Photos.ToArray();
        var selection = viewModel.SelectedPhoto;
        try
        {
            viewModel.Filter = PhotoFilterCriteria.None with
            {
                ColorLabels = new HashSet<ColorLabel> { ColorLabel.Red }
            };
            if (!viewModel.IsFilterActive || viewModel.FilterLabel == "Filter")
            {
                throw new InvalidOperationException(
                    "An active filter must be visible on the filter button.");
            }

            OnClearFilterClick(this, new RoutedEventArgs());
            if (viewModel.IsFilterActive)
            {
                throw new InvalidOperationException(
                    "Clearing the filter must reset every facet.");
            }
        }
        finally
        {
            viewModel.Photos.ReplaceRange(presentedPhotos);
            viewModel.SelectedPhoto = selection;
            content.UpdateLayout();
        }
    }

    private static IEnumerable<T> FindChildren<T>(DependencyObject? root)
        where T : DependencyObject
    {
        if (root is null)
        {
            yield break;
        }

        for (var index = 0;
             index < VisualTreeHelper.GetChildrenCount(root);
             index++)
        {
            var child = VisualTreeHelper.GetChild(root, index);
            if (child is T match)
            {
                yield return match;
            }

            foreach (var nested in FindChildren<T>(child))
            {
                yield return nested;
            }
        }
    }

    private static T? FindChild<T>(DependencyObject? root)
        where T : DependencyObject
    {
        if (root is null)
        {
            return null;
        }

        for (var index = 0;
             index < VisualTreeHelper.GetChildrenCount(root);
             index++)
        {
            var child = VisualTreeHelper.GetChild(root, index);
            if (child is T match)
            {
                return match;
            }

            if (FindChild<T>(child) is { } nested)
            {
                return nested;
            }
        }

        return null;
    }
}

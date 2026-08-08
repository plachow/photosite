using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using PhotoSite.Controls;
using PhotoSite.Domain;
using PhotoSite.ViewModels;

namespace PhotoSite.Dialogs;

/// <summary>
/// Side-by-side comparison for culling: two to four photographs at once with
/// synchronized pan and zoom, and the rating and reject keys of the manager.
/// </summary>
public partial class CompareWindow : Window
{
    internal const int MaximumPhotos = 4;

    private readonly List<PhotoItemViewModel> photos;
    private readonly List<PhotoViewer> viewers = [];
    private readonly List<Border> frames = [];
    private int focusedIndex;
    private bool isSynchronizing;

    internal CompareWindow(IReadOnlyList<PhotoItemViewModel> selection)
    {
        photos = selection.Take(MaximumPhotos).ToList();
        InitializeComponent();
        BuildPanes();
        Loaded += (_, _) => UpdateStatus();
    }

    private void BuildPanes()
    {
        ViewerGrid.Children.Clear();
        viewers.Clear();
        frames.Clear();
        ViewerGrid.Columns = photos.Count <= 1 ? 1 : 2;

        for (var index = 0; index < photos.Count; index++)
        {
            var photo = photos[index];
            var viewer = new PhotoViewer
            {
                SourcePath = photo.Path,
                EditRecipe = photo.EditRecipe,
                Tag = index
            };
            viewer.ViewStateChanged += OnViewerViewStateChanged;
            viewer.MouseLeftButtonDown += OnViewerPressed;
            viewers.Add(viewer);

            var caption = new TextBlock
            {
                Margin = new Thickness(8, 5, 8, 5),
                FontSize = 12,
                Foreground = new SolidColorBrush(
                    Color.FromRgb(0xF2, 0xF4, 0xF8)),
                TextTrimming = TextTrimming.CharacterEllipsis
            };
            caption.SetBinding(
                TextBlock.TextProperty,
                new System.Windows.Data.Binding(nameof(PhotoItemViewModel.FileName))
                {
                    Source = photo
                });

            var badge = new TextBlock
            {
                Margin = new Thickness(0, 5, 10, 5),
                FontSize = 12,
                Foreground = new SolidColorBrush(
                    Color.FromRgb(0xFF, 0xD3, 0x6A))
            };
            badge.SetBinding(
                TextBlock.TextProperty,
                new System.Windows.Data.Binding(nameof(PhotoItemViewModel.RatingText))
                {
                    Source = photo
                });

            var header = new DockPanel { LastChildFill = true };
            DockPanel.SetDock(badge, Dock.Right);
            header.Children.Add(badge);
            header.Children.Add(caption);

            var layout = new Grid();
            layout.RowDefinitions.Add(new RowDefinition
            {
                Height = GridLength.Auto
            });
            layout.RowDefinitions.Add(new RowDefinition());
            layout.Children.Add(new Border
            {
                Background = new SolidColorBrush(
                    Color.FromRgb(0x19, 0x1C, 0x23)),
                Child = header
            });
            Grid.SetRow(viewer, 1);
            layout.Children.Add(viewer);

            var frame = new Border
            {
                Margin = new Thickness(3),
                BorderThickness = new Thickness(2),
                BorderBrush = Brushes.Transparent,
                Child = layout
            };
            frames.Add(frame);
            ViewerGrid.Children.Add(frame);
        }

        UpdateFocusHighlight();
    }

    private void OnViewerPressed(object sender, MouseButtonEventArgs eventArgs)
    {
        if (sender is PhotoViewer { Tag: int index })
        {
            focusedIndex = index;
            UpdateFocusHighlight();
            UpdateStatus();
        }
    }

    private void UpdateFocusHighlight()
    {
        for (var index = 0; index < frames.Count; index++)
        {
            frames[index].BorderBrush = index == focusedIndex
                ? (Brush)FindResource("AccentBrush")
                : Brushes.Transparent;
        }
    }

    private void OnViewerViewStateChanged(object? sender, EventArgs eventArgs)
    {
        if (isSynchronizing
            || SyncBox.IsChecked != true
            || sender is not PhotoViewer source)
        {
            return;
        }

        // Guarded, because applying the state to the others makes each of
        // them raise the same event straight back.
        isSynchronizing = true;
        try
        {
            var state = source.ViewState;
            foreach (var viewer in viewers)
            {
                if (!ReferenceEquals(viewer, source))
                {
                    viewer.ApplyViewState(state);
                }
            }
        }
        finally
        {
            isSynchronizing = false;
        }
    }

    private void OnFitClick(object sender, RoutedEventArgs eventArgs) =>
        ForEachViewer(viewer => viewer.FitToViewport());

    private void OnActualSizeClick(object sender, RoutedEventArgs eventArgs) =>
        ForEachViewer(viewer => viewer.ShowActualSize());

    private void ForEachViewer(Action<PhotoViewer> action)
    {
        isSynchronizing = true;
        try
        {
            foreach (var viewer in viewers)
            {
                action(viewer);
            }
        }
        finally
        {
            isSynchronizing = false;
        }
    }

    private void OnComparePreviewKeyDown(object sender, KeyEventArgs eventArgs)
    {
        if (eventArgs.Key == Key.Escape)
        {
            Close();
            eventArgs.Handled = true;
            return;
        }

        if (photos.Count == 0)
        {
            return;
        }

        var focused = photos[Math.Clamp(focusedIndex, 0, photos.Count - 1)];
        switch (eventArgs.Key)
        {
            case Key.D0 when Keyboard.Modifiers == ModifierKeys.None:
                ForEachViewer(viewer => viewer.FitToViewport());
                break;
            case Key.D1 when Keyboard.Modifiers == ModifierKeys.Control:
                ForEachViewer(viewer => viewer.ShowActualSize());
                break;
            case Key.P:
                focused.Flag = focused.Flag == PhotoFlag.Picked
                    ? PhotoFlag.None
                    : PhotoFlag.Picked;
                break;
            case Key.X:
                focused.Flag = focused.Flag == PhotoFlag.Rejected
                    ? PhotoFlag.None
                    : PhotoFlag.Rejected;
                break;
            case Key.Tab:
                focusedIndex = (focusedIndex + 1) % photos.Count;
                UpdateFocusHighlight();
                break;
            case Key.Delete:
                RemoveFocused();
                break;
            default:
                if (MainWindow.TryGetRatingShortcut(eventArgs.Key, out var rating)
                    && Keyboard.Modifiers == ModifierKeys.None)
                {
                    focused.Rating = rating;
                    break;
                }

                return;
        }

        eventArgs.Handled = true;
        UpdateStatus();
    }

    private void RemoveFocused()
    {
        if (photos.Count <= 1)
        {
            Close();
            return;
        }

        photos.RemoveAt(focusedIndex);
        focusedIndex = Math.Clamp(focusedIndex, 0, photos.Count - 1);
        BuildPanes();
    }

    private void UpdateStatus()
    {
        if (photos.Count == 0)
        {
            StatusText.Text = string.Empty;
            return;
        }

        var focused = photos[Math.Clamp(focusedIndex, 0, photos.Count - 1)];
        var flag = focused.Flag switch
        {
            PhotoFlag.Picked => "picked",
            PhotoFlag.Rejected => "rejected",
            _ => "unflagged"
        };
        StatusText.Text =
            $"{focused.FileName} · {focused.Rating} ★ · {flag} · "
            + $"{focused.DimensionsText ?? "unknown size"} · {focused.FileSizeText}";
    }
}

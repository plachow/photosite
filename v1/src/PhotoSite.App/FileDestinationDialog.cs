using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using Microsoft.Win32;
using PhotoSite.Infrastructure;
using PhotoSite.Services;

namespace PhotoSite;

internal sealed class FileDestinationDialog : Window
{
    private readonly TextBox destinationBox;
    private readonly Button confirmButton;

    public FileDestinationDialog(
        PhotoFileTransferMode mode,
        int fileCount,
        string? initialDestination)
    {
        var action = mode == PhotoFileTransferMode.Copy ? "Copy" : "Move";
        Title = $"{action} to…";
        Width = 620;
        Height = 235;
        MinWidth = 620;
        MinHeight = 235;
        MaxWidth = 620;
        MaxHeight = 235;
        ResizeMode = ResizeMode.NoResize;
        WindowStartupLocation = WindowStartupLocation.CenterOwner;
        ShowInTaskbar = false;
        DarkWindowChrome.Apply(this);

        var panel = new Grid
        {
            Margin = new Thickness(24, 20, 24, 18)
        };
        panel.RowDefinitions.Add(new RowDefinition());
        panel.RowDefinitions.Add(new RowDefinition
        {
            Height = GridLength.Auto
        });

        var content = new StackPanel();
        content.Children.Add(new TextBlock
        {
            Text = $"{action} {FormatFileCount(fileCount)} to…",
            FontSize = 20,
            FontWeight = FontWeights.SemiBold,
            Foreground = Brushes.White
        });
        content.Children.Add(new TextBlock
        {
            Margin = new Thickness(0, 9, 0, 0),
            Text = "Destination folder",
            FontSize = 12,
            Foreground = new SolidColorBrush(Color.FromRgb(200, 205, 216))
        });

        var destinationRow = new Grid
        {
            Margin = new Thickness(0, 5, 0, 0)
        };
        destinationRow.ColumnDefinitions.Add(new ColumnDefinition());
        destinationRow.ColumnDefinitions.Add(new ColumnDefinition
        {
            Width = GridLength.Auto
        });
        destinationBox = new TextBox
        {
            Padding = new Thickness(8, 6, 8, 6),
            VerticalContentAlignment = VerticalAlignment.Center,
            FontSize = 13,
            Text = initialDestination ?? string.Empty
        };
        destinationBox.TextChanged += (_, _) => UpdateConfirmState();
        destinationRow.Children.Add(destinationBox);

        var browseButton = new Button
        {
            Content = "Browse…",
            MinWidth = 92,
            Margin = new Thickness(8, 0, 0, 0)
        };
        browseButton.Click += OnBrowseClick;
        Grid.SetColumn(browseButton, 1);
        destinationRow.Children.Add(browseButton);
        content.Children.Add(destinationRow);
        panel.Children.Add(content);

        var buttons = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            HorizontalAlignment = HorizontalAlignment.Right
        };
        Grid.SetRow(buttons, 1);
        confirmButton = new Button
        {
            Content = action,
            MinWidth = 96,
            IsDefault = true
        };
        confirmButton.Click += OnConfirmClick;
        buttons.Children.Add(confirmButton);
        buttons.Children.Add(new Button
        {
            Content = "Cancel",
            MinWidth = 90,
            Margin = new Thickness(8, 0, 0, 0),
            IsCancel = true
        });
        panel.Children.Add(buttons);
        Content = panel;

        Loaded += (_, _) =>
        {
            destinationBox.Focus();
            destinationBox.SelectAll();
            UpdateConfirmState();
        };
    }

    public string? DestinationDirectory { get; private set; }

    internal static string FormatFileCount(int count) =>
        count == 1 ? "1 file" : $"{count:N0} files";

    private void OnBrowseClick(object sender, RoutedEventArgs eventArgs)
    {
        var dialog = new OpenFolderDialog
        {
            Title = "Choose destination folder",
            Multiselect = false,
            InitialDirectory = ResolveExistingDirectory(destinationBox.Text)
        };
        if (dialog.ShowDialog(this) == true)
        {
            destinationBox.Text = dialog.FolderName;
            destinationBox.CaretIndex = destinationBox.Text.Length;
        }
    }

    private void OnConfirmClick(object sender, RoutedEventArgs eventArgs)
    {
        var candidate = destinationBox.Text.Trim().Trim('"');
        string fullPath;
        try
        {
            fullPath = Path.GetFullPath(candidate);
        }
        catch (Exception exception) when (
            exception is ArgumentException
                or NotSupportedException
                or PathTooLongException)
        {
            ShowInvalidDestination(exception.Message);
            return;
        }

        if (!Directory.Exists(fullPath))
        {
            ShowInvalidDestination("The folder does not exist.");
            return;
        }

        DestinationDirectory = fullPath;
        DialogResult = true;
    }

    private void UpdateConfirmState()
    {
        if (confirmButton is not null)
        {
            confirmButton.IsEnabled = !string.IsNullOrWhiteSpace(
                destinationBox.Text);
        }
    }

    private void ShowInvalidDestination(string message)
    {
        MessageBox.Show(
            this,
            $"Choose an existing destination folder.\n\n{message}",
            "Invalid destination",
            MessageBoxButton.OK,
            MessageBoxImage.Warning);
        destinationBox.Focus();
        destinationBox.SelectAll();
    }

    private static string? ResolveExistingDirectory(string? path)
    {
        var candidate = path?.Trim().Trim('"');
        if (!string.IsNullOrWhiteSpace(candidate)
            && Directory.Exists(candidate))
        {
            return Path.GetFullPath(candidate);
        }

        return null;
    }
}

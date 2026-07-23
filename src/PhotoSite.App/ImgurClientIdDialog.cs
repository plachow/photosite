using System.Diagnostics;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;
using System.Windows.Media;

namespace PhotoSite;

public sealed class ImgurClientIdDialog : Window
{
    private readonly TextBox clientIdBox;

    public ImgurClientIdDialog()
    {
        Title = "Connect Imgur";
        Width = 560;
        Height = 270;
        MinWidth = 560;
        MinHeight = 270;
        MaxWidth = 560;
        MaxHeight = 270;
        ResizeMode = ResizeMode.NoResize;
        WindowStartupLocation = WindowStartupLocation.CenterOwner;
        ShowInTaskbar = false;

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
            Text = "Imgur Client ID",
            FontSize = 20,
            FontWeight = FontWeights.SemiBold,
            Foreground = Brushes.White
        });
        content.Children.Add(new TextBlock
        {
            Margin = new Thickness(0, 9, 0, 0),
            Text = "Anonymous uploads require a Client ID from a registered "
                + "Imgur application. It is stored only in PhotoSite's local "
                + "settings on this computer.",
            TextWrapping = TextWrapping.Wrap,
            FontSize = 12,
            Foreground = new SolidColorBrush(Color.FromRgb(200, 205, 216))
        });
        var registrationText = new TextBlock
        {
            Margin = new Thickness(0, 7, 0, 0),
            FontSize = 12
        };
        var registrationLink = new Hyperlink(
            new Run("Create an Imgur API application"))
        {
            NavigateUri = new Uri(
                "https://api.imgur.com/oauth2/addclient"),
            Foreground = new SolidColorBrush(
                Color.FromRgb(139, 200, 255))
        };
        registrationLink.RequestNavigate += (_, eventArgs) =>
        {
            try
            {
                Process.Start(new ProcessStartInfo(eventArgs.Uri.AbsoluteUri)
                {
                    UseShellExecute = true
                });
            }
            catch
            {
            }

            eventArgs.Handled = true;
        };
        registrationText.Inlines.Add(registrationLink);
        content.Children.Add(registrationText);
        clientIdBox = new TextBox
        {
            Margin = new Thickness(0, 13, 0, 0),
            Padding = new Thickness(8, 6, 8, 6),
            FontSize = 13
        };
        content.Children.Add(clientIdBox);
        panel.Children.Add(content);

        var buttons = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            HorizontalAlignment = HorizontalAlignment.Right
        };
        Grid.SetRow(buttons, 1);
        var saveButton = new Button
        {
            Content = "Save and upload",
            MinWidth = 126,
            IsDefault = true,
            IsEnabled = false
        };
        saveButton.Click += (_, _) =>
        {
            ClientId = clientIdBox.Text.Trim();
            DialogResult = true;
        };
        clientIdBox.TextChanged += (_, _) =>
            saveButton.IsEnabled = !string.IsNullOrWhiteSpace(clientIdBox.Text);
        buttons.Children.Add(saveButton);
        buttons.Children.Add(new Button
        {
            Content = "Cancel",
            MinWidth = 90,
            Margin = new Thickness(8, 0, 0, 0),
            IsCancel = true
        });
        panel.Children.Add(buttons);
        Content = panel;
        Loaded += (_, _) => clientIdBox.Focus();
    }

    public string? ClientId { get; private set; }
}

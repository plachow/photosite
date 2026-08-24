using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using PhotoSite.Infrastructure;

namespace PhotoSite.Dialogs;

/// <summary>
/// A one-line prompt used wherever PhotoSite needs a name: preset names,
/// new folders and renames.
/// </summary>
internal sealed class TextPromptDialog : Window
{
    private readonly TextBox valueBox;

    public TextPromptDialog(
        string title,
        string label,
        string? initialValue = null,
        string confirmText = "OK")
    {
        Title = title;
        Width = 460;
        SizeToContent = SizeToContent.Height;
        ResizeMode = ResizeMode.NoResize;
        WindowStartupLocation = WindowStartupLocation.CenterOwner;
        ShowInTaskbar = false;
        DarkWindowChrome.Apply(this);
        // Derived windows do not inherit the implicit Window style, so the
        // dark theme has to be stated here.
        SetResourceReference(BackgroundProperty, "WindowBrush");
        Foreground = new SolidColorBrush(Color.FromRgb(0xF2, 0xF4, 0xF8));

        var panel = new StackPanel
        {
            Margin = new Thickness(22, 18, 22, 16)
        };
        panel.Children.Add(new TextBlock
        {
            Text = label,
            FontSize = 12,
            Foreground = new SolidColorBrush(Color.FromRgb(200, 205, 216))
        });

        valueBox = new TextBox
        {
            Margin = new Thickness(0, 6, 0, 0),
            FontSize = 13,
            Text = initialValue ?? string.Empty
        };
        valueBox.TextChanged += (_, _) => UpdateConfirmState();
        panel.Children.Add(valueBox);

        var buttons = new StackPanel
        {
            Margin = new Thickness(0, 16, 0, 0),
            Orientation = Orientation.Horizontal,
            HorizontalAlignment = HorizontalAlignment.Right
        };
        ConfirmButton = new Button
        {
            Content = confirmText,
            MinWidth = 96,
            IsDefault = true
        };
        ConfirmButton.Click += (_, _) =>
        {
            Value = valueBox.Text.Trim();
            DialogResult = true;
        };
        buttons.Children.Add(ConfirmButton);
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
            valueBox.Focus();
            // Selecting the stem rather than the whole value means typing
            // straight away replaces the name but keeps the extension.
            var extension = initialValue is null
                ? -1
                : initialValue.LastIndexOf('.');
            if (extension > 0)
            {
                valueBox.Select(0, extension);
            }
            else
            {
                valueBox.SelectAll();
            }

            UpdateConfirmState();
        };
    }

    public Button ConfirmButton { get; }

    public string? Value { get; private set; }

    private void UpdateConfirmState() =>
        ConfirmButton.IsEnabled = !string.IsNullOrWhiteSpace(valueBox.Text);
}

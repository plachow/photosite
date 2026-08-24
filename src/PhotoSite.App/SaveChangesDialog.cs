using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using PhotoSite.Infrastructure;

namespace PhotoSite;

public enum SaveChangesChoice
{
    Cancel,
    OverwriteOriginal,
    SaveAsCopy,
    Discard
}

public sealed class SaveChangesDialog : Window
{
    public SaveChangesDialog(
        string fileName,
        bool jpegWillReencode,
        bool isUnsavedImage = false)
    {
        Title = isUnsavedImage ? "Unsaved image" : "Unsaved edits";
        Width = 570;
        Height = 235;
        MinWidth = 570;
        MinHeight = 235;
        MaxWidth = 570;
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

        var text = new StackPanel();
        text.Children.Add(new TextBlock
        {
            Text = "Save changes?",
            FontSize = 20,
            FontWeight = FontWeights.SemiBold,
            Foreground = Brushes.White
        });
        text.Children.Add(new TextBlock
        {
            Margin = new Thickness(0, 9, 0, 0),
            Text = fileName,
            FontSize = 13,
            Foreground = new SolidColorBrush(Color.FromRgb(200, 205, 216)),
            TextTrimming = TextTrimming.CharacterEllipsis
        });
        text.Children.Add(new TextBlock
        {
            Margin = new Thickness(0, 8, 0, 0),
            Text = isUnsavedImage
                ? "This pasted image has not been saved yet."
                : jpegWillReencode
                    ? "Crop or pixel export will re-encode this JPEG."
                    : "Your original has not been changed yet.",
            FontSize = 12,
            Foreground = new SolidColorBrush(
                jpegWillReencode
                    ? Color.FromRgb(255, 211, 106)
                    : Color.FromRgb(146, 153, 167))
        });
        panel.Children.Add(text);

        var buttons = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            HorizontalAlignment = HorizontalAlignment.Right
        };
        Grid.SetRow(buttons, 1);
        if (!isUnsavedImage)
        {
            buttons.Children.Add(CreateButton(
                "Overwrite original",
                SaveChangesChoice.OverwriteOriginal));
        }

        buttons.Children.Add(CreateButton(
            isUnsavedImage ? "Save as…" : "Save as copy…",
            SaveChangesChoice.SaveAsCopy));
        buttons.Children.Add(CreateButton(
            "Discard",
            SaveChangesChoice.Discard));
        buttons.Children.Add(CreateButton(
            "Cancel",
            SaveChangesChoice.Cancel,
            isCancel: true));
        panel.Children.Add(buttons);
        Content = panel;
    }

    public SaveChangesChoice Choice { get; private set; } =
        SaveChangesChoice.Cancel;

    private Button CreateButton(
        string text,
        SaveChangesChoice choice,
        bool isCancel = false)
    {
        var button = new Button
        {
            Content = text,
            MinWidth = 104,
            Margin = new Thickness(4, 0, 0, 0),
            IsCancel = isCancel
        };
        button.Click += (_, _) =>
        {
            Choice = choice;
            DialogResult = choice != SaveChangesChoice.Cancel;
        };
        return button;
    }
}

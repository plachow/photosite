using System.Windows.Media;
using PhotoSite.Domain;

namespace PhotoSite.Infrastructure;

/// <summary>
/// One frozen brush per colour label. Tiles are recycled constantly while
/// scrolling, so allocating a brush per bind would be pure garbage.
/// </summary>
public static class PhotoLabelBrushes
{
    private static readonly Dictionary<ColorLabel, Brush> Brushes =
        Enum.GetValues<ColorLabel>()
            .ToDictionary(
                label => label,
                label => Create(label.ToHexColor()));

    public static Brush Get(ColorLabel label) =>
        Brushes.TryGetValue(label, out var brush)
            ? brush
            : Brushes[ColorLabel.None];

    private static Brush Create(string hex)
    {
        var color = (Color)ColorConverter.ConvertFromString(hex);
        var brush = new SolidColorBrush(color);
        brush.Freeze();
        return brush;
    }
}

using System.Globalization;
using System.Windows.Data;
using System.Windows.Media;
using PhotoSite.Domain;

namespace PhotoSite.Infrastructure;

/// <summary>
/// A stable colour per person, drawn from a palette tuned for the dark
/// theme. The person's id picks the hue, so the same person wears the same
/// colour on every badge, filter chip and tagging button.
/// </summary>
public static class PersonBrushes
{
    private static readonly Brush[] Palette =
    [
        Create("#E8B84A"),
        Create("#8BC8FF"),
        Create("#A8D88F"),
        Create("#FF9A9A"),
        Create("#C9A9FF"),
        Create("#6FD8C8"),
        Create("#FFB077"),
        Create("#F0A6C8"),
        Create("#B8D86F"),
        Create("#9FB6FF"),
        Create("#D8B48F"),
        Create("#8FD8F0")
    ];

    public static Brush Get(long personId) =>
        Palette[(int)((ulong)personId % (ulong)Palette.Length)];

    private static Brush Create(string hex)
    {
        var brush = new SolidColorBrush(
            (Color)ColorConverter.ConvertFromString(hex));
        brush.Freeze();
        return brush;
    }
}

/// <summary>Turns a bound person (tag or record) into their colour.</summary>
public sealed class PersonBrushConverter : IValueConverter
{
    public object Convert(
        object? value,
        Type targetType,
        object? parameter,
        CultureInfo culture) =>
        value switch
        {
            PersonTag tag => PersonBrushes.Get(tag.Id),
            PersonRecord person => PersonBrushes.Get(person.Id),
            long id => PersonBrushes.Get(id),
            _ => Brushes.Transparent
        };

    public object ConvertBack(
        object? value,
        Type targetType,
        object? parameter,
        CultureInfo culture) =>
        throw new NotSupportedException();
}

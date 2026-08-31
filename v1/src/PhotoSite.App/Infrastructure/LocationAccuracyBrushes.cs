using System.Windows.Media;
using PhotoSite.Domain;

namespace PhotoSite.Infrastructure;

/// <summary>
/// The Map button's verdict dot, in the same hues as the colour labels so
/// the traffic light speaks the palette the rest of the app already uses.
/// </summary>
public static class LocationAccuracyBrushes
{
    private static readonly Brush Precise = Create("#6FBF5B");
    private static readonly Brush Approximate = Create("#E8B84A");
    private static readonly Brush Poor = Create("#E5533D");

    public static Brush Get(LocationAccuracy accuracy) => accuracy switch
    {
        LocationAccuracy.Precise => Precise,
        LocationAccuracy.Approximate => Approximate,
        LocationAccuracy.Poor => Poor,
        _ => Brushes.Transparent
    };

    private static Brush Create(string hex)
    {
        var brush = new SolidColorBrush(
            (Color)ColorConverter.ConvertFromString(hex));
        brush.Freeze();
        return brush;
    }
}

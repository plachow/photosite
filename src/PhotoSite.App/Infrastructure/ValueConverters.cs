using System.Globalization;
using System.Windows;
using System.Windows.Data;
using PhotoSite.Domain;

namespace PhotoSite.Infrastructure;

/// <summary>
/// Collapses a row whose value is missing, which is what keeps the info panel
/// free of empty "Lens:" labels for photos that never recorded one.
/// </summary>
public sealed class NullToCollapsedConverter : IValueConverter
{
    public object Convert(
        object? value,
        Type targetType,
        object? parameter,
        CultureInfo culture) =>
        value switch
        {
            null => Visibility.Collapsed,
            string text when string.IsNullOrWhiteSpace(text) => Visibility.Collapsed,
            _ => Visibility.Visible
        };

    public object ConvertBack(
        object? value,
        Type targetType,
        object? parameter,
        CultureInfo culture) =>
        throw new NotSupportedException();
}

public sealed class BooleanToVisibilityConverter : IValueConverter
{
    public object Convert(
        object? value,
        Type targetType,
        object? parameter,
        CultureInfo culture)
    {
        var flag = value is true;
        if (parameter is string text
            && text.Equals("invert", StringComparison.OrdinalIgnoreCase))
        {
            flag = !flag;
        }

        return flag ? Visibility.Visible : Visibility.Collapsed;
    }

    public object ConvertBack(
        object? value,
        Type targetType,
        object? parameter,
        CultureInfo culture) =>
        value is Visibility.Visible;
}

/// <summary>
/// Renders a sort field with the wording used in the UI rather than its
/// identifier, so the dropdown reads "Date taken" and not "TakenAt".
/// </summary>
public sealed class SortFieldNameConverter : IValueConverter
{
    public object Convert(
        object? value,
        Type targetType,
        object? parameter,
        CultureInfo culture) =>
        value is PhotoSortField field
            ? field.ToDisplayName()
            : string.Empty;

    public object ConvertBack(
        object? value,
        Type targetType,
        object? parameter,
        CultureInfo culture) =>
        throw new NotSupportedException();
}

/// <summary>
/// Compares a bound enum to the value named in the converter parameter, which
/// is how the view-mode and orientation toggles show their state.
/// </summary>
public sealed class EnumEqualsConverter : IValueConverter
{
    public object Convert(
        object? value,
        Type targetType,
        object? parameter,
        CultureInfo culture) =>
        value is not null
        && parameter is string name
        && string.Equals(value.ToString(), name, StringComparison.Ordinal);

    public object ConvertBack(
        object? value,
        Type targetType,
        object? parameter,
        CultureInfo culture) =>
        value is true && parameter is string name
            ? Enum.Parse(targetType, name)
            : Binding.DoNothing;
}

using System.Globalization;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;

namespace PhotoSite.Controls;

/// <summary>
/// One labelled adjustment row: name, slider, current value.
/// </summary>
/// <remarks>
/// Double-clicking the row returns the slider to its neutral value, which is
/// the fastest way to undo a single experiment without losing the rest of the
/// edit and is what every photo editor trains its users to expect.
/// </remarks>
public sealed class AdjustmentSlider : Control
{
    public static readonly DependencyProperty LabelProperty =
        DependencyProperty.Register(
            nameof(Label),
            typeof(string),
            typeof(AdjustmentSlider),
            new PropertyMetadata(string.Empty));

    public static readonly DependencyProperty ValueProperty =
        DependencyProperty.Register(
            nameof(Value),
            typeof(double),
            typeof(AdjustmentSlider),
            new FrameworkPropertyMetadata(
                0d,
                FrameworkPropertyMetadataOptions.BindsTwoWayByDefault,
                OnValueChanged));

    public static readonly DependencyProperty MinimumProperty =
        DependencyProperty.Register(
            nameof(Minimum),
            typeof(double),
            typeof(AdjustmentSlider),
            new PropertyMetadata(-100d));

    public static readonly DependencyProperty MaximumProperty =
        DependencyProperty.Register(
            nameof(Maximum),
            typeof(double),
            typeof(AdjustmentSlider),
            new PropertyMetadata(100d));

    public static readonly DependencyProperty DefaultValueProperty =
        DependencyProperty.Register(
            nameof(DefaultValue),
            typeof(double),
            typeof(AdjustmentSlider),
            new PropertyMetadata(0d));

    public static readonly DependencyProperty ValueFormatProperty =
        DependencyProperty.Register(
            nameof(ValueFormat),
            typeof(string),
            typeof(AdjustmentSlider),
            new PropertyMetadata("0"));

    private static readonly DependencyPropertyKey DisplayValuePropertyKey =
        DependencyProperty.RegisterReadOnly(
            nameof(DisplayValue),
            typeof(string),
            typeof(AdjustmentSlider),
            new PropertyMetadata("0"));

    public static readonly DependencyProperty DisplayValueProperty =
        DisplayValuePropertyKey.DependencyProperty;

    static AdjustmentSlider()
    {
        DefaultStyleKeyProperty.OverrideMetadata(
            typeof(AdjustmentSlider),
            new FrameworkPropertyMetadata(typeof(AdjustmentSlider)));
    }

    public AdjustmentSlider()
    {
        MouseDoubleClick += OnRowDoubleClick;
    }

    public string Label
    {
        get => (string)GetValue(LabelProperty);
        set => SetValue(LabelProperty, value);
    }

    public double Value
    {
        get => (double)GetValue(ValueProperty);
        set => SetValue(ValueProperty, value);
    }

    public double Minimum
    {
        get => (double)GetValue(MinimumProperty);
        set => SetValue(MinimumProperty, value);
    }

    public double Maximum
    {
        get => (double)GetValue(MaximumProperty);
        set => SetValue(MaximumProperty, value);
    }

    public double DefaultValue
    {
        get => (double)GetValue(DefaultValueProperty);
        set => SetValue(DefaultValueProperty, value);
    }

    public string ValueFormat
    {
        get => (string)GetValue(ValueFormatProperty);
        set => SetValue(ValueFormatProperty, value);
    }

    public string DisplayValue => (string)GetValue(DisplayValueProperty);

    private void OnRowDoubleClick(object sender, MouseButtonEventArgs eventArgs)
    {
        Value = DefaultValue;
        eventArgs.Handled = true;
    }

    /// <summary>
    /// Raised after <see cref="Value"/> changes, for dialogs that drive a live
    /// preview off the slider rather than binding it to a view-model.
    /// </summary>
    public event EventHandler<RoutedPropertyChangedEventArgs<double>>? ValueChanged;

    private static void OnValueChanged(
        DependencyObject dependencyObject,
        DependencyPropertyChangedEventArgs eventArgs)
    {
        var slider = (AdjustmentSlider)dependencyObject;
        var oldValue = (double)eventArgs.OldValue;
        var newValue = (double)eventArgs.NewValue;
        slider.SetValue(
            DisplayValuePropertyKey,
            newValue.ToString(slider.ValueFormat, CultureInfo.CurrentCulture));
        slider.ValueChanged?.Invoke(
            slider,
            new RoutedPropertyChangedEventArgs<double>(oldValue, newValue));
    }
}

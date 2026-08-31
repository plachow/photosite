using System.Globalization;
using System.Windows;
using System.Windows.Input;
using System.Windows.Media;

namespace PhotoSite.Controls;

public sealed class RatingPicker : FrameworkElement
{
    private const double CellWidth = 23;
    private const double StarRadius = 10;
    private const double InnerRadius = 4.6;
    private const double PreferredHeight = 24;
    private static readonly Brush NumberBrush = CreateBrush("#17191E");
    private static readonly Brush SelectionBrush = CreateBrush("#F2F4F8");
    private static readonly Brush HoverBrush = CreateBrush("#FFFFFF");
    private static readonly Brush FocusBrush = CreateBrush("#67B7FF");
    private static readonly Brush[] RatingBrushes =
    [
        CreateBrush("#F2F4F8"),
        CreateBrush("#FFF6C2"),
        CreateBrush("#FFE99A"),
        CreateBrush("#FFDD72"),
        CreateBrush("#FFD047"),
        CreateBrush("#F5B82E")
    ];

    private int hoveredRating = -1;

    public static readonly DependencyProperty ValueProperty =
        DependencyProperty.Register(
            nameof(Value),
            typeof(int),
            typeof(RatingPicker),
            new FrameworkPropertyMetadata(
                0,
                FrameworkPropertyMetadataOptions.BindsTwoWayByDefault
                | FrameworkPropertyMetadataOptions.AffectsRender,
                null,
                CoerceRating));

    public static readonly DependencyProperty ShowUnratedProperty =
        DependencyProperty.Register(
            nameof(ShowUnrated),
            typeof(bool),
            typeof(RatingPicker),
            new FrameworkPropertyMetadata(
                true,
                FrameworkPropertyMetadataOptions.AffectsMeasure
                | FrameworkPropertyMetadataOptions.AffectsRender));

    public static readonly DependencyProperty UnratedToolTipProperty =
        DependencyProperty.Register(
            nameof(UnratedToolTip),
            typeof(string),
            typeof(RatingPicker),
            new PropertyMetadata("No rating"));

    public static readonly DependencyProperty RatingToolTipFormatProperty =
        DependencyProperty.Register(
            nameof(RatingToolTipFormat),
            typeof(string),
            typeof(RatingPicker),
            new PropertyMetadata("Rating: {0}"));

    public RatingPicker()
    {
        Cursor = Cursors.Hand;
        Focusable = true;
        SnapsToDevicePixels = true;
        UseLayoutRounding = true;
    }

    public int Value
    {
        get => (int)GetValue(ValueProperty);
        set => SetValue(ValueProperty, value);
    }

    public bool ShowUnrated
    {
        get => (bool)GetValue(ShowUnratedProperty);
        set => SetValue(ShowUnratedProperty, value);
    }

    public string UnratedToolTip
    {
        get => (string)GetValue(UnratedToolTipProperty);
        set => SetValue(UnratedToolTipProperty, value);
    }

    public string RatingToolTipFormat
    {
        get => (string)GetValue(RatingToolTipFormatProperty);
        set => SetValue(RatingToolTipFormatProperty, value);
    }

    protected override Size MeasureOverride(Size availableSize)
    {
        var count = ShowUnrated ? 6 : 5;
        return new Size(count * CellWidth, PreferredHeight);
    }

    protected override void OnRender(DrawingContext drawingContext)
    {
        base.OnRender(drawingContext);

        var firstRating = ShowUnrated ? 0 : 1;
        var centerY = Math.Min(RenderSize.Height, PreferredHeight) / 2 - 0.5;
        var dpi = VisualTreeHelper.GetDpi(this).PixelsPerDip;

        for (var rating = firstRating; rating <= 5; rating++)
        {
            var index = rating - firstRating;
            var centerX = index * CellWidth + CellWidth / 2;
            var isSelected = rating == Value;
            var isHovered = rating == hoveredRating;
            var opacity = isSelected ? 1.0 : isHovered ? 0.88 : 0.58;
            var outline = isHovered
                ? new Pen(HoverBrush, 1.1)
                : isSelected
                    ? new Pen(SelectionBrush, 1)
                    : new Pen(RatingBrushes[rating], 0.65);

            drawingContext.PushOpacity(opacity);
            drawingContext.DrawGeometry(
                RatingBrushes[rating],
                outline,
                CreateStarGeometry(centerX, centerY));

            var number = new FormattedText(
                rating.ToString(CultureInfo.InvariantCulture),
                CultureInfo.CurrentUICulture,
                FlowDirection.LeftToRight,
                new Typeface("Segoe UI Semibold"),
                9,
                NumberBrush,
                dpi);
            drawingContext.DrawText(
                number,
                new Point(
                    centerX - number.Width / 2,
                    centerY - number.Height / 2 - 0.2));
            drawingContext.Pop();

            if (isSelected)
            {
                drawingContext.DrawRoundedRectangle(
                    IsKeyboardFocused ? FocusBrush : SelectionBrush,
                    null,
                    new Rect(centerX - 4, RenderSize.Height - 1.8, 8, 1.4),
                    0.7,
                    0.7);
            }
        }
    }

    protected override void OnMouseMove(MouseEventArgs eventArgs)
    {
        base.OnMouseMove(eventArgs);
        var rating = RatingFromPoint(eventArgs.GetPosition(this));
        if (rating == hoveredRating)
        {
            return;
        }

        hoveredRating = rating;
        ToolTip = rating switch
        {
            0 => UnratedToolTip,
            > 0 when !ShowUnrated && rating == Value => UnratedToolTip,
            > 0 => string.Format(
                CultureInfo.CurrentUICulture,
                RatingToolTipFormat,
                rating),
            _ => null
        };
        InvalidateVisual();
    }

    protected override void OnMouseLeave(MouseEventArgs eventArgs)
    {
        base.OnMouseLeave(eventArgs);
        hoveredRating = -1;
        ToolTip = null;
        InvalidateVisual();
    }

    protected override void OnMouseLeftButtonDown(MouseButtonEventArgs eventArgs)
    {
        base.OnMouseLeftButtonDown(eventArgs);
        Focus();

        var rating = RatingFromPoint(eventArgs.GetPosition(this));
        if (rating < 0)
        {
            return;
        }

        Value = !ShowUnrated && rating == Value ? 0 : rating;
        eventArgs.Handled = true;
    }

    protected override void OnKeyDown(KeyEventArgs eventArgs)
    {
        base.OnKeyDown(eventArgs);

        var firstRating = ShowUnrated ? 0 : 1;
        var nextRating = eventArgs.Key switch
        {
            Key.Left or Key.Down => Math.Max(firstRating, Value - 1),
            Key.Right or Key.Up => Math.Min(5, Math.Max(firstRating, Value + 1)),
            Key.Home => firstRating,
            Key.End => 5,
            Key.Delete or Key.Back when !ShowUnrated => 0,
            _ => -1
        };

        if (nextRating < 0)
        {
            return;
        }

        Value = nextRating;
        eventArgs.Handled = true;
    }

    protected override void OnGotKeyboardFocus(KeyboardFocusChangedEventArgs eventArgs)
    {
        base.OnGotKeyboardFocus(eventArgs);
        InvalidateVisual();
    }

    protected override void OnLostKeyboardFocus(KeyboardFocusChangedEventArgs eventArgs)
    {
        base.OnLostKeyboardFocus(eventArgs);
        InvalidateVisual();
    }

    private int RatingFromPoint(Point point)
    {
        if (point.X < 0 || point.Y < 0
            || point.X >= RenderSize.Width || point.Y >= RenderSize.Height)
        {
            return -1;
        }

        var firstRating = ShowUnrated ? 0 : 1;
        var rating = firstRating + (int)(point.X / CellWidth);
        return rating <= 5 ? rating : -1;
    }

    private static object CoerceRating(DependencyObject element, object value) =>
        Math.Clamp((int)value, 0, 5);

    private static StreamGeometry CreateStarGeometry(double centerX, double centerY)
    {
        var geometry = new StreamGeometry();
        using var context = geometry.Open();

        for (var pointIndex = 0; pointIndex < 10; pointIndex++)
        {
            var radius = pointIndex % 2 == 0 ? StarRadius : InnerRadius;
            var angle = -Math.PI / 2 + pointIndex * Math.PI / 5;
            var point = new Point(
                centerX + Math.Cos(angle) * radius,
                centerY + Math.Sin(angle) * radius);

            if (pointIndex == 0)
            {
                context.BeginFigure(point, true, true);
            }
            else
            {
                context.LineTo(point, true, false);
            }
        }

        geometry.Freeze();
        return geometry;
    }

    private static SolidColorBrush CreateBrush(string color)
    {
        var brush = new SolidColorBrush((Color)ColorConverter.ConvertFromString(color));
        brush.Freeze();
        return brush;
    }
}

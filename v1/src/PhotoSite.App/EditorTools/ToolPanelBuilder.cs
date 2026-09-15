using System.Windows;
using System.Windows.Automation;
using System.Windows.Controls;
using PhotoSite.Controls;

namespace PhotoSite.EditorTools;

/// <summary>
/// Declares a tool's settings panel as a list of rows bound to the fields of
/// its settings record. Every row reads through a getter and writes through
/// a <c>with</c> expression, so the tool's settings stay one immutable value
/// and the panel re-reads itself whenever that value changes - after a
/// preset is chosen, a reset, or an Auto button.
/// </summary>
internal sealed class ToolPanelBuilder<TSettings>
    where TSettings : class
{
    private readonly EditTool<TSettings> tool;
    private readonly StackPanel panel = new();
    private readonly List<Action> refreshers = [];
    private FrameworkElement? last;
    private bool isRefreshing;

    public ToolPanelBuilder(EditTool<TSettings> tool)
    {
        this.tool = tool;
    }

    public ToolPanelBuilder<TSettings> Header(string text) =>
        Add(new TextBlock
        {
            Text = text.ToUpperInvariant(),
            Style = (Style)Application.Current.FindResource("SectionHeaderStyle"),
            Margin = new Thickness(0, panel.Children.Count == 0 ? 0 : 12, 0, 4)
        });

    public ToolPanelBuilder<TSettings> Slider(
        string label,
        double minimum,
        double maximum,
        Func<TSettings, double> get,
        Func<TSettings, double, TSettings> set,
        string format = "0")
    {
        var slider = new AdjustmentSlider
        {
            Label = label,
            Minimum = minimum,
            Maximum = maximum,
            ValueFormat = format,
            DefaultValue = get(tool.Defaults),
            Value = get(tool.Settings)
        };
        slider.ValueChanged += (_, eventArgs) =>
        {
            if (!isRefreshing)
            {
                tool.Settings = set(tool.Settings, eventArgs.NewValue);
            }
        };
        refreshers.Add(() => slider.Value = get(tool.Settings));
        return Add(slider);
    }

    public ToolPanelBuilder<TSettings> Choice(
        string label,
        IReadOnlyList<string> options,
        Func<TSettings, int> get,
        Func<TSettings, int, TSettings> set)
    {
        var row = new Grid { Margin = new Thickness(0, 4, 0, 6) };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(96) });
        row.ColumnDefinitions.Add(new ColumnDefinition());
        row.Children.Add(new TextBlock
        {
            Text = label,
            VerticalAlignment = VerticalAlignment.Center,
            Style = (Style)Application.Current.FindResource("FieldLabelStyle")
        });
        var combo = new ComboBox
        {
            ItemsSource = options,
            SelectedIndex = Math.Clamp(get(tool.Settings), 0, options.Count - 1),
            Height = 26,
            Padding = new Thickness(6, 0, 6, 0)
        };
        AutomationProperties.SetName(combo, label);
        Grid.SetColumn(combo, 1);
        row.Children.Add(combo);
        combo.SelectionChanged += (_, _) =>
        {
            if (!isRefreshing && combo.SelectedIndex >= 0)
            {
                tool.Settings = set(tool.Settings, combo.SelectedIndex);
            }
        };
        refreshers.Add(() =>
            combo.SelectedIndex = Math.Clamp(get(tool.Settings), 0, options.Count - 1));
        return Add(row);
    }

    public ToolPanelBuilder<TSettings> Toggle(
        string label,
        Func<TSettings, bool> get,
        Func<TSettings, bool, TSettings> set)
    {
        var box = new CheckBox
        {
            Content = label,
            IsChecked = get(tool.Settings),
            Margin = new Thickness(0, 6, 0, 2)
        };
        box.Click += (_, _) =>
        {
            if (!isRefreshing)
            {
                tool.Settings = set(tool.Settings, box.IsChecked == true);
            }
        };
        refreshers.Add(() => box.IsChecked = get(tool.Settings));
        return Add(box);
    }

    public ToolPanelBuilder<TSettings> Button(
        string label,
        Action onClick,
        string? toolTip = null)
    {
        var button = new Button
        {
            Content = label,
            ToolTip = toolTip,
            MinWidth = 78,
            Padding = new Thickness(8, 4, 8, 4),
            Margin = new Thickness(0, 6, 0, 4),
            HorizontalAlignment = HorizontalAlignment.Left
        };
        button.Click += (_, _) => onClick();
        return Add(button);
    }

    public ToolPanelBuilder<TSettings> Note(string text) =>
        Add(new TextBlock
        {
            Text = text,
            TextWrapping = TextWrapping.Wrap,
            FontSize = 11,
            Margin = new Thickness(0, 8, 0, 0),
            Foreground = (System.Windows.Media.Brush)Application.Current.FindResource(
                "MutedTextBrush")
        });

    /// <summary>
    /// A control the tool builds itself - a histogram, a curve editor -
    /// with an optional refresh that re-reads it from the settings.
    /// </summary>
    public ToolPanelBuilder<TSettings> Element(
        FrameworkElement element,
        Action? refresh = null)
    {
        if (refresh is not null)
        {
            refreshers.Add(refresh);
        }

        return Add(element);
    }

    /// <summary>
    /// Shows the row added last only while the predicate holds - a radius
    /// that only one sharpening type uses.
    /// </summary>
    public ToolPanelBuilder<TSettings> VisibleWhen(Func<TSettings, bool> predicate)
    {
        var element = last
            ?? throw new InvalidOperationException("VisibleWhen needs a row before it.");
        void Apply() => element.Visibility = predicate(tool.Settings)
            ? Visibility.Visible
            : Visibility.Collapsed;
        Apply();
        refreshers.Add(Apply);
        return this;
    }

    /// <summary>
    /// Whether the panel is currently pushing settings into its controls,
    /// for custom elements that must not echo the change back.
    /// </summary>
    public bool IsRefreshing => isRefreshing;

    public FrameworkElement Build()
    {
        tool.SettingsChanged += (_, _) => Refresh();
        return panel;
    }

    private void Refresh()
    {
        if (isRefreshing)
        {
            return;
        }

        isRefreshing = true;
        try
        {
            foreach (var refresh in refreshers)
            {
                refresh();
            }
        }
        finally
        {
            isRefreshing = false;
        }
    }

    private ToolPanelBuilder<TSettings> Add(FrameworkElement element)
    {
        panel.Children.Add(element);
        last = element;
        return this;
    }
}

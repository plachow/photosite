using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Media;
using PhotoSite.Controls;
using PhotoSite.Domain;

namespace PhotoSite;

/// <summary>
/// The editor's annotation layers: the drawing tools, their appearance and
/// the layer list.
/// </summary>
public partial class MainWindow
{
    private static readonly (AnnotationTool Tool, string Glyph, string Tip)[]
        AnnotationTools =
        [
            (AnnotationTool.Select, "⬈", "Select and move objects (V)"),
            (AnnotationTool.Arrow, "↗", "Arrow (A)"),
            (AnnotationTool.Line, "／", "Line (L)"),
            (AnnotationTool.Rectangle, "▭", "Rectangle (S)"),
            (AnnotationTool.Ellipse, "◯", "Ellipse (O)"),
            (AnnotationTool.Text, "T", "Text (T)"),
            (AnnotationTool.Freehand, "✎", "Freehand drawing (D)")
        ];

    private static readonly uint[] AnnotationColors =
    [
        0xFFFF3B30, 0xFFFFCC00, 0xFF34C759, 0xFF0A84FF,
        0xFFAF52DE, 0xFFFFFFFF, 0xFF111318
    ];

    private readonly List<ToggleButton> annotationToolButtons = [];
    private readonly List<Button> annotationColorButtons = [];
    private uint annotationColor = AnnotationColors[0];
    private bool isLayerUiUpdating;

    private void InitializeAnnotationTools()
    {
        foreach (var (tool, glyph, tip) in AnnotationTools)
        {
            var button = new ToggleButton
            {
                Content = glyph,
                Tag = tool,
                Width = 34,
                Height = 30,
                Margin = new Thickness(0, 0, 0, 3),
                FontFamily = new FontFamily("Segoe UI Symbol"),
                ToolTip = tip,
                Style = (Style)FindResource("EditorSelectToggleButtonStyle")
            };
            System.Windows.Automation.AutomationProperties.SetName(button, tip);
            button.Click += OnAnnotationToolClick;
            annotationToolButtons.Add(button);
            AnnotationToolPanel.Children.Add(button);
        }

        foreach (var color in AnnotationColors)
        {
            var button = new Button
            {
                Width = 22,
                Height = 22,
                Margin = new Thickness(0, 0, 4, 0),
                Padding = new Thickness(0),
                Tag = color,
                Background = CreateBrush(color),
                BorderBrush = new SolidColorBrush(
                    Color.FromRgb(0x4A, 0x51, 0x60))
            };
            System.Windows.Automation.AutomationProperties.SetName(
                button,
                "Annotation colour");
            button.Click += OnAnnotationColorClick;
            annotationColorButtons.Add(button);
            AnnotationColorPanel.Children.Add(button);
        }

        StrokeWidthSlider.Value = 4;
        LayerOpacitySlider.Value = 100;
        FontSizeSlider.Value = 5;
        StrokeWidthSlider.ValueChanged += (_, _) => OnLayerAppearanceChanged(
            StrokeWidthSlider,
            new RoutedEventArgs());
        LayerOpacitySlider.ValueChanged += (_, _) => OnLayerAppearanceChanged(
            LayerOpacitySlider,
            new RoutedEventArgs());
        FontSizeSlider.ValueChanged += (_, _) => OnLayerAppearanceChanged(
            FontSizeSlider,
            new RoutedEventArgs());

        PreviewViewer.LayerCommitted += OnLayerCommitted;
        PreviewViewer.LayerSelectionChanged += OnCanvasLayerSelectionChanged;
        PreviewViewer.LayerDeleteRequested += (_, id) => DeleteLayer(id);
        UpdateAnnotationTemplate();
        SyncAnnotationToolButtons();
    }

    private static Brush CreateBrush(uint argb)
    {
        var brush = new SolidColorBrush(
            Color.FromArgb(
                (byte)((argb >> 24) & 0xFF),
                (byte)((argb >> 16) & 0xFF),
                (byte)((argb >> 8) & 0xFF),
                (byte)(argb & 0xFF)));
        brush.Freeze();
        return brush;
    }

    private void OnAnnotationToolClick(object sender, RoutedEventArgs eventArgs)
    {
        if (sender is not ToggleButton { Tag: AnnotationTool tool })
        {
            return;
        }

        SetAnnotationTool(
            PreviewViewer.ActiveTool == tool ? AnnotationTool.None : tool);
    }

    private void SetAnnotationTool(AnnotationTool tool)
    {
        PreviewViewer.ActiveTool = tool;
        if (tool != AnnotationTool.None)
        {
            // Crop selection and object drawing both own the left button, so
            // arming one has to disarm the other.
            PreviewViewer.EndSelectionMode();
            PreviewViewer.IsColorPickerMode = false;
            EyedropperButton.IsChecked = false;
        }

        SyncAnnotationToolButtons();
        PreviewViewer.Focus();
    }

    private void SyncAnnotationToolButtons()
    {
        foreach (var button in annotationToolButtons)
        {
            button.IsChecked = (AnnotationTool)button.Tag
                               == PreviewViewer.ActiveTool;
        }

        UpdateToolOptionsPanel();
    }

    /// <summary>
    /// The floating tool-options card beside the toolbox appears only while
    /// it has something to configure: an armed drawing tool or a selected
    /// object. Its text section shows up just for the text tool and text
    /// layers.
    /// </summary>
    private void UpdateToolOptionsPanel()
    {
        var hasSubject = PreviewViewer.ActiveTool != AnnotationTool.None
                         || PreviewViewer.SelectedLayerId is not null;
        ToolOptionsPanel.Visibility =
            viewModel.IsEditorMode && !viewModel.IsFullscreenMode && hasSubject
                ? Visibility.Visible
                : Visibility.Collapsed;
        ToolOptionsHeader.Text = PreviewViewer.ActiveTool != AnnotationTool.None
            ? PreviewViewer.ActiveTool.ToString().ToUpperInvariant()
            : "SELECTION";
        TextOptionsPanel.Visibility =
            PreviewViewer.ActiveTool == AnnotationTool.Text
            || GetSelectedLayer() is TextLayer
                ? Visibility.Visible
                : Visibility.Collapsed;

        // A placed image has no stroke, fill or colour; only its opacity
        // is worth a control.
        var isImage = PreviewViewer.ActiveTool is AnnotationTool.None or AnnotationTool.Select
                      && GetSelectedLayer() is ImageLayer;
        var shapeControls = isImage ? Visibility.Collapsed : Visibility.Visible;
        AnnotationColorPanel.Visibility = shapeControls;
        StrokeWidthSlider.Visibility = shapeControls;
        FillShapeBox.Visibility = shapeControls;
        if (isImage)
        {
            ToolOptionsHeader.Text = "IMAGE";
        }
    }

    private void OnAnnotationColorClick(object sender, RoutedEventArgs eventArgs)
    {
        if (sender is not Button { Tag: uint color })
        {
            return;
        }

        annotationColor = color;
        foreach (var button in annotationColorButtons)
        {
            button.BorderThickness = new Thickness(
                (uint)button.Tag == color ? 2 : 1);
        }

        UpdateAnnotationTemplate();
        ApplyAppearanceToSelectedLayer();
    }

    /// <summary>
    /// Keeps the "next object" template in step with the panel, so a shape is
    /// created with the colour and weight already on screen rather than
    /// needing to be restyled after every draw.
    /// </summary>
    private void UpdateAnnotationTemplate()
    {
        var strokeWidth = StrokeWidthSlider.Value / 1000;
        var opacity = LayerOpacitySlider.Value / 100;
        PreviewViewer.NewLayerTemplate = new TextLayer
        {
            StrokeColor = annotationColor,
            FillColor = FillShapeBox.IsChecked == true
                ? (annotationColor & 0x00FFFFFF) | 0x55000000
                : 0,
            BackgroundColor = TextBackgroundBox.IsChecked == true
                ? 0xC0111318
                : 0,
            StrokeWidth = strokeWidth,
            Opacity = opacity,
            FontSize = FontSizeSlider.Value / 100,
            Text = string.IsNullOrWhiteSpace(LayerTextBox.Text)
                ? "Text"
                : LayerTextBox.Text
        };
    }

    private void OnLayerAppearanceChanged(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (isLayerUiUpdating)
        {
            return;
        }

        UpdateAnnotationTemplate();
        ApplyAppearanceToSelectedLayer();
    }

    private void OnLayerTextChanged(object sender, TextChangedEventArgs eventArgs)
    {
        if (isLayerUiUpdating)
        {
            return;
        }

        UpdateAnnotationTemplate();
        ApplyAppearanceToSelectedLayer();
    }

    private void ApplyAppearanceToSelectedLayer()
    {
        if (GetSelectedLayer() is not { } layer
            || viewModel.SelectedPhoto is not { } photo)
        {
            return;
        }

        var strokeWidth = StrokeWidthSlider.Value / 1000;
        var opacity = LayerOpacitySlider.Value / 100;
        var fill = FillShapeBox.IsChecked == true
            ? (annotationColor & 0x00FFFFFF) | 0x55000000
            : 0u;

        AnnotationLayer updated = layer switch
        {
            TextLayer text => PreviewViewer.MeasureText(
                text with
                {
                    StrokeColor = annotationColor,
                    Opacity = opacity,
                    FontSize = FontSizeSlider.Value / 100,
                    BackgroundColor = TextBackgroundBox.IsChecked == true
                        ? 0xC0111318
                        : 0,
                    Text = string.IsNullOrEmpty(LayerTextBox.Text)
                        ? " "
                        : LayerTextBox.Text
                }),
            ShapeLayer shape => shape with
            {
                StrokeColor = annotationColor,
                FillColor = fill,
                StrokeWidth = strokeWidth,
                Opacity = opacity
            },
            _ => layer with
            {
                StrokeColor = annotationColor,
                StrokeWidth = strokeWidth,
                Opacity = opacity
            }
        };

        photo.SetLayers(photo.EditRecipe.WithLayer(updated).Layers);
        RefreshLayerList();
    }

    private void OnLayerCommitted(object? sender, AnnotationLayer layer)
    {
        if (viewModel.SelectedPhoto is not { } photo)
        {
            return;
        }

        var stored = layer is TextLayer text
            ? PreviewViewer.MeasureText(text)
            : layer;
        photo.SetLayers(photo.EditRecipe.WithLayer(stored).Layers);
        RefreshLayerList();
        viewModel.ReportStatus(
            $"{stored.Name} added · it stays editable until you export");
    }

    private void OnCanvasLayerSelectionChanged(object? sender, string? layerId)
    {
        SyncAnnotationToolButtons();
        RefreshLayerList();
    }

    private AnnotationLayer? GetSelectedLayer() =>
        viewModel.SelectedPhoto?.EditRecipe.Layers
            .FirstOrDefault(layer => layer.Id == PreviewViewer.SelectedLayerId);

    private void RefreshLayerList()
    {
        isLayerUiUpdating = true;
        try
        {
            var layers = viewModel.SelectedPhoto?.EditRecipe.Layers
                ?? [];
            // Topmost first, matching what the canvas draws last.
            LayerList.ItemsSource = layers.Reverse().ToArray();
            PreviewViewer.Layers = layers;
            LayerList.SelectedItem = layers.FirstOrDefault(
                layer => layer.Id == PreviewViewer.SelectedLayerId);

            if (GetSelectedLayer() is { } selected)
            {
                StrokeWidthSlider.Value = Math.Clamp(
                    selected.StrokeWidth * 1000,
                    1,
                    30);
                LayerOpacitySlider.Value = Math.Clamp(
                    selected.Opacity * 100,
                    5,
                    100);
                if (selected is TextLayer text)
                {
                    FontSizeSlider.Value = Math.Clamp(text.FontSize * 100, 1, 25);
                    LayerTextBox.Text = text.Text;
                    TextBackgroundBox.IsChecked = (text.BackgroundColor >> 24) > 0;
                }

                if (selected is ShapeLayer shape)
                {
                    FillShapeBox.IsChecked = (shape.FillColor >> 24) > 0;
                }
            }

            LayerTextBox.IsEnabled = GetSelectedLayer() is TextLayer
                                     || PreviewViewer.ActiveTool
                                     == AnnotationTool.Text;
            FontSizeSlider.IsEnabled = LayerTextBox.IsEnabled;
            UpdateToolOptionsPanel();
        }
        finally
        {
            isLayerUiUpdating = false;
        }
    }

    private void OnLayerListSelectionChanged(
        object sender,
        SelectionChangedEventArgs eventArgs)
    {
        if (isLayerUiUpdating)
        {
            return;
        }

        PreviewViewer.SelectedLayerId =
            (LayerList.SelectedItem as AnnotationLayer)?.Id;
        if (PreviewViewer.ActiveTool == AnnotationTool.None)
        {
            SetAnnotationTool(AnnotationTool.Select);
        }

        RefreshLayerList();
    }

    private void OnLayerVisibilityClick(object sender, RoutedEventArgs eventArgs)
    {
        if (sender is not CheckBox { Tag: string layerId }
            || viewModel.SelectedPhoto is not { } photo)
        {
            return;
        }

        var layer = photo.EditRecipe.Layers
            .FirstOrDefault(item => item.Id == layerId);
        if (layer is null)
        {
            return;
        }

        photo.SetLayers(
            photo.EditRecipe
                .WithLayer(layer with { IsVisible = !layer.IsVisible })
                .Layers);
        RefreshLayerList();
    }

    private void OnLayerMoveUpClick(object sender, RoutedEventArgs eventArgs) =>
        MoveSelectedLayer(1);

    private void OnLayerMoveDownClick(object sender, RoutedEventArgs eventArgs) =>
        MoveSelectedLayer(-1);

    private void MoveSelectedLayer(int offset)
    {
        if (PreviewViewer.SelectedLayerId is not { } id
            || viewModel.SelectedPhoto is not { } photo)
        {
            return;
        }

        photo.SetLayers(photo.EditRecipe.WithLayerMoved(id, offset).Layers);
        RefreshLayerList();
    }

    private void OnLayerDuplicateClick(object sender, RoutedEventArgs eventArgs)
    {
        if (GetSelectedLayer() is not { } layer
            || viewModel.SelectedPhoto is not { } photo)
        {
            return;
        }

        // Offset slightly so the copy is visible instead of hiding exactly
        // behind the original.
        var copy = layer.Translate(0.02, 0.02) with
        {
            Id = Guid.NewGuid().ToString("N"),
            Name = layer.Name + " copy"
        };
        photo.SetLayers(photo.EditRecipe.WithLayer(copy).Layers);
        PreviewViewer.SelectedLayerId = copy.Id;
        RefreshLayerList();
    }

    private void OnLayerDeleteClick(object sender, RoutedEventArgs eventArgs) =>
        DeleteLayer(PreviewViewer.SelectedLayerId);

    private void DeleteLayer(string? layerId)
    {
        if (layerId is null || viewModel.SelectedPhoto is not { } photo)
        {
            return;
        }

        photo.SetLayers(photo.EditRecipe.WithoutLayer(layerId).Layers);
        PreviewViewer.SelectedLayerId = null;
        RefreshLayerList();
    }

    internal void ValidateLayerEditingForSmokeTest(
        ViewModels.PhotoItemViewModel photo)
    {
        SetAnnotationTool(AnnotationTool.Arrow);
        if (PreviewViewer.ActiveTool != AnnotationTool.Arrow
            || PreviewViewer.IsSelectionMode)
        {
            throw new InvalidOperationException(
                "Arming a drawing tool must disarm the crop selection.");
        }

        var arrow = new ShapeLayer
        {
            Name = "Arrow",
            Shape = ShapeKind.Arrow,
            X1 = 0.1,
            Y1 = 0.1,
            X2 = 0.7,
            Y2 = 0.5
        };
        OnLayerCommitted(this, arrow);
        if (photo.EditRecipe.Layers.Count != 1)
        {
            throw new InvalidOperationException(
                "Drawing an object must add it to the recipe.");
        }

        PreviewViewer.SelectedLayerId = arrow.Id;
        RefreshLayerList();
        OnLayerDuplicateClick(this, new RoutedEventArgs());
        if (photo.EditRecipe.Layers.Count != 2)
        {
            throw new InvalidOperationException(
                "Duplicating a layer must add a second one.");
        }

        var duplicate = photo.EditRecipe.Layers[1];
        if (duplicate.Id == arrow.Id)
        {
            throw new InvalidOperationException(
                "A duplicated layer must get its own identity.");
        }

        OnLayerMoveDownClick(this, new RoutedEventArgs());
        if (photo.EditRecipe.Layers[0].Id != duplicate.Id)
        {
            throw new InvalidOperationException(
                "Sending a layer back must reorder the recipe.");
        }

        DeleteLayer(duplicate.Id);
        if (photo.EditRecipe.Layers.Count != 1
            || photo.EditRecipe.Layers[0].Id != arrow.Id)
        {
            throw new InvalidOperationException(
                "Deleting a layer must leave the others alone.");
        }

        photo.UndoEditCommand.Execute(null);
        if (photo.EditRecipe.Layers.Count != 2)
        {
            throw new InvalidOperationException(
                "Layer edits must take part in undo.");
        }

        while (photo.UndoEditCommand.CanExecute(null))
        {
            photo.UndoEditCommand.Execute(null);
        }

        if (photo.EditRecipe.Layers.Count != 0)
        {
            throw new InvalidOperationException(
                "Undoing every step must remove the layers again.");
        }

        SetAnnotationTool(AnnotationTool.None);
        RefreshLayerList();
    }

    private bool TryHandleAnnotationShortcut(System.Windows.Input.Key key)
    {
        if (!viewModel.IsEditorMode
            || System.Windows.Input.Keyboard.Modifiers
            != System.Windows.Input.ModifierKeys.None)
        {
            return false;
        }

        var tool = key switch
        {
            System.Windows.Input.Key.V => AnnotationTool.Select,
            System.Windows.Input.Key.A => AnnotationTool.Arrow,
            System.Windows.Input.Key.L => AnnotationTool.Line,
            // Deliberately not R: that stays "rotate" everywhere, the way the
            // rest of the shortcut set expects.
            System.Windows.Input.Key.S => AnnotationTool.Rectangle,
            System.Windows.Input.Key.O => AnnotationTool.Ellipse,
            System.Windows.Input.Key.T => AnnotationTool.Text,
            System.Windows.Input.Key.D => AnnotationTool.Freehand,
            _ => AnnotationTool.None
        };
        if (tool == AnnotationTool.None)
        {
            return false;
        }

        SetAnnotationTool(tool);
        return true;
    }
}

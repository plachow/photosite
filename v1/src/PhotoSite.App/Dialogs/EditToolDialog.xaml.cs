using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using PhotoSite.Domain;
using PhotoSite.EditorTools;
using PhotoSite.Infrastructure;
using PhotoSite.Services.Imaging;

namespace PhotoSite.Dialogs;

/// <summary>
/// The one window every editor tool opens in. It owns what all tools share
/// - the preset strip, the live preview, the before toggle, OK and Cancel -
/// and hosts the tool's own settings panel beside the preview.
/// </summary>
/// <remarks>
/// The preview renders the whole recipe with the tool applied, on a reduced
/// copy of the photograph, through the same renderer the canvas and the
/// exporter use. That is slower than filtering a bare bitmap but it is the
/// only way the preview can show sharpening on top of a curve the way the
/// export will.
/// </remarks>
public partial class EditToolDialog : Window
{
    /// <summary>
    /// The preview works on a reduced copy: a tool has to react while a
    /// slider is being dragged, and blurring 24 megapixels cannot.
    /// </summary>
    private const int PreviewLongestSide = 1100;

    private static readonly RenderRequest PreviewRequest = new(IncludeLayers: false);

    private readonly EditTool tool;
    private readonly ToolPresetStore? presets;
    private readonly BitmapSource fullSource;
    private readonly EditRecipe baseRecipe;
    private readonly EditToolContext context;
    private readonly DispatcherTimer renderTimer;
    private CancellationTokenSource? renderCancellation;
    private BitmapSource? displayed;
    private bool isLoadingPreset;
    private bool suppressPresetReset;

    internal EditToolDialog(
        EditTool tool,
        BitmapSource source,
        EditRecipe baseRecipe,
        ToolPresetStore? presets)
    {
        InitializeComponent();
        DarkWindowChrome.Apply(this);

        this.tool = tool;
        this.presets = presets;
        this.baseRecipe = baseRecipe;
        fullSource = source;
        Title = tool.Title;
        SettingsHeader.Text = tool.Title.ToUpperInvariant();
        HintText.Text = tool.Hint;
        HintText.Visibility = string.IsNullOrEmpty(tool.Hint)
            ? Visibility.Collapsed
            : Visibility.Visible;
        GridButton.Visibility = tool.OffersGrid
            ? Visibility.Visible
            : Visibility.Collapsed;
        if (tool.AcceptsPreviewPick)
        {
            PreviewImage.Cursor = Cursors.Cross;
            PreviewImage.ToolTip = "Click a neutral grey area of the photograph";
        }

        var previewSource = ImageRenderer.Resize(source, PreviewLongestSide);
        context = new EditToolContext(
            previewSource,
            baseRecipe,
            recipe => ImageRenderer.Render(previewSource, recipe, PreviewRequest),
            source.PixelWidth,
            source.PixelHeight);

        renderTimer = new DispatcherTimer
        {
            Interval = TimeSpan.FromMilliseconds(90)
        };
        renderTimer.Tick += (_, _) =>
        {
            renderTimer.Stop();
            _ = RenderPreviewAsync();
        };

        if (tool.StartsFromRecipe)
        {
            tool.LoadFrom(baseRecipe);
        }

        EditorHost.Content = tool.CreateEditor(context);
        tool.SettingsChanged += OnToolSettingsChanged;

        Loaded += async (_, _) =>
        {
            await LoadPresetsAsync();
            ScheduleRender();
        };
        Closed += (_, _) =>
        {
            renderTimer.Stop();
            renderCancellation?.Cancel();
            renderCancellation?.Dispose();
            tool.SettingsChanged -= OnToolSettingsChanged;
        };
    }

    /// <summary>The recipe the user confirmed, or null when cancelled.</summary>
    public EditRecipe? Result { get; private set; }

    /// <summary>The tool this window is showing, for tests and status text.</summary>
    internal EditTool Tool => tool;

    internal FrameworkElement? EditorContent => EditorHost.Content as FrameworkElement;

    internal int PresetCount => PresetBox.Items.Count;

    private enum PresetKind
    {
        Default,
        LastUsed,
        Saved
    }

    private sealed record PresetEntry(string Name, PresetKind Kind, string? Payload);

    private void OnToolSettingsChanged(object? sender, EventArgs eventArgs)
    {
        if (!isLoadingPreset && !suppressPresetReset)
        {
            // A hand-moved slider is no longer the preset it started from.
            suppressPresetReset = true;
            PresetBox.SelectedIndex = -1;
            suppressPresetReset = false;
            DeletePresetButton.IsEnabled = false;
        }

        ScheduleRender();
    }

    private async Task LoadPresetsAsync(string? select = null)
    {
        var entries = new List<PresetEntry>
        {
            new("<Default>", PresetKind.Default, null)
        };

        if (presets is not null)
        {
            try
            {
                if (await presets.LoadLastUsedAsync(tool.Id) is { Length: > 0 } lastUsed)
                {
                    entries.Add(new PresetEntry("<Last used>", PresetKind.LastUsed, lastUsed));
                }

                foreach (var (name, payload) in await presets.LoadAsync(tool.Id))
                {
                    entries.Add(new PresetEntry(name, PresetKind.Saved, payload));
                }
            }
            catch (Exception exception)
            {
                FooterText.Text = $"Presets could not be loaded: {exception.Message}";
            }
        }

        suppressPresetReset = true;
        try
        {
            PresetBox.ItemsSource = entries;
            if (select is not null)
            {
                PresetBox.SelectedItem = entries.FirstOrDefault(entry =>
                    entry.Kind == PresetKind.Saved
                    && string.Equals(entry.Name, select, StringComparison.OrdinalIgnoreCase));
            }
            else if (!tool.StartsFromRecipe)
            {
                // A filter starts from what was used last time, which is
                // what makes "sharpen it like the previous one" one click.
                var lastUsed = entries.FirstOrDefault(entry => entry.Kind == PresetKind.LastUsed);
                if (lastUsed is not null)
                {
                    ApplyPreset(lastUsed);
                    PresetBox.SelectedItem = lastUsed;
                }
            }
        }
        finally
        {
            suppressPresetReset = false;
        }
    }

    private void OnPresetSelectionChanged(
        object sender,
        SelectionChangedEventArgs eventArgs)
    {
        DeletePresetButton.IsEnabled =
            PresetBox.SelectedItem is PresetEntry { Kind: PresetKind.Saved };
        if (suppressPresetReset || PresetBox.SelectedItem is not PresetEntry entry)
        {
            return;
        }

        ApplyPreset(entry);
    }

    private void ApplyPreset(PresetEntry entry)
    {
        isLoadingPreset = true;
        try
        {
            if (entry.Payload is null)
            {
                tool.Reset();
            }
            else if (!tool.TryDeserialize(entry.Payload))
            {
                FooterText.Text = $"The preset “{entry.Name}” could not be read.";
            }
        }
        finally
        {
            isLoadingPreset = false;
        }
    }

    private async void OnSavePresetClick(object sender, RoutedEventArgs eventArgs)
    {
        if (presets is null)
        {
            return;
        }

        var suggested = PresetBox.SelectedItem is PresetEntry { Kind: PresetKind.Saved } selected
            ? selected.Name
            : "My preset";
        var dialog = new TextPromptDialog("Save preset", "Preset name", suggested)
        {
            Owner = this
        };
        if (dialog.ShowDialog() != true || string.IsNullOrWhiteSpace(dialog.Value))
        {
            return;
        }

        var name = dialog.Value.Trim();
        try
        {
            await presets.SaveAsync(tool.Id, name, tool.Serialize());
            await LoadPresetsAsync(select: name);
            FooterText.Text = $"Preset “{name}” saved";
        }
        catch (Exception exception)
        {
            FooterText.Text = $"The preset could not be saved: {exception.Message}";
        }
    }

    private async void OnDeletePresetClick(object sender, RoutedEventArgs eventArgs)
    {
        if (presets is null
            || PresetBox.SelectedItem is not PresetEntry { Kind: PresetKind.Saved } entry)
        {
            return;
        }

        if (MessageBox.Show(
                this,
                $"Delete the preset “{entry.Name}”?",
                "Delete preset",
                MessageBoxButton.YesNo,
                MessageBoxImage.Question) != MessageBoxResult.Yes)
        {
            return;
        }

        try
        {
            await presets.DeleteAsync(tool.Id, entry.Name);
            await LoadPresetsAsync();
            FooterText.Text = $"Preset “{entry.Name}” deleted";
        }
        catch (Exception exception)
        {
            FooterText.Text = $"The preset could not be deleted: {exception.Message}";
        }
    }

    private void OnResetClick(object sender, RoutedEventArgs eventArgs)
    {
        suppressPresetReset = true;
        try
        {
            PresetBox.SelectedIndex = 0;
        }
        finally
        {
            suppressPresetReset = false;
        }

        ApplyPreset(new PresetEntry("<Default>", PresetKind.Default, null));
    }

    private void OnLivePreviewChanged(object sender, RoutedEventArgs eventArgs) =>
        ScheduleRender();

    private void OnBeforeToggled(object sender, RoutedEventArgs eventArgs)
    {
        PreviewBadge.Text = BeforeButton.IsChecked == true ? "BEFORE" : "AFTER";
        ScheduleRender();
    }

    private void OnGridToggled(object sender, RoutedEventArgs eventArgs) =>
        GridOverlay.Visibility = GridButton.IsChecked == true
            ? Visibility.Visible
            : Visibility.Collapsed;

    private void OnPreviewMouseDown(object sender, MouseButtonEventArgs eventArgs)
    {
        if (!tool.AcceptsPreviewPick
            || displayed is null
            || PreviewImage.ActualWidth <= 0
            || PreviewImage.ActualHeight <= 0)
        {
            return;
        }

        var position = eventArgs.GetPosition(PreviewImage);
        tool.OnPreviewPicked(
            context,
            Math.Clamp(position.X / PreviewImage.ActualWidth, 0, 1),
            Math.Clamp(position.Y / PreviewImage.ActualHeight, 0, 1),
            displayed);
        eventArgs.Handled = true;
    }

    private void ScheduleRender()
    {
        renderTimer.Stop();
        renderTimer.Start();
    }

    private async Task RenderPreviewAsync()
    {
        renderCancellation?.Cancel();
        renderCancellation?.Dispose();
        renderCancellation = new CancellationTokenSource();
        var token = renderCancellation.Token;

        var showBase = BeforeButton.IsChecked == true || LivePreviewBox.IsChecked != true;
        var recipe = showBase ? baseRecipe : tool.Apply(baseRecipe);
        var (outputWidth, outputHeight) = ImageRenderer.MeasureOutput(
            fullSource.PixelWidth,
            fullSource.PixelHeight,
            recipe);

        PreviewStatus.Text = "Rendering…";
        try
        {
            var rendered = await Task.Run(
                () => showBase
                    ? context.BaseRender
                    : ImageRenderer.Render(
                        context.Original,
                        recipe,
                        PreviewRequest,
                        token),
                token);
            if (token.IsCancellationRequested)
            {
                return;
            }

            displayed = rendered;
            PreviewImage.Source = rendered;
            PreviewStatus.Text = $"{outputWidth} × {outputHeight} px";
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception exception)
        {
            PreviewStatus.Text = exception.Message;
        }
    }

    private async void OnOkClick(object sender, RoutedEventArgs eventArgs)
    {
        Result = tool.Apply(baseRecipe);
        if (presets is not null)
        {
            try
            {
                await presets.SaveLastUsedAsync(tool.Id, tool.Serialize());
            }
            catch
            {
                // Remembering the settings is a convenience; the edit itself
                // must not fail over it.
            }
        }

        DialogResult = true;
    }
}

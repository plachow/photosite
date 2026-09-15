using System.Text.Json;
using System.Windows;
using System.Windows.Media.Imaging;
using PhotoSite.Domain;

namespace PhotoSite.EditorTools;

/// <summary>
/// One editor tool: a named, presettable change to the recipe with a live
/// preview. Every menu entry that opens a window - sharpen, levels, curves,
/// resize - is one of these, and <see cref="Dialogs.EditToolDialog"/> is the
/// one window they all open in, so presets, the preview, the before toggle
/// and OK/Cancel behave the same everywhere.
/// </summary>
/// <remarks>
/// A tool never touches pixels itself. It holds a settings value, turns it
/// into a recipe through <see cref="Apply"/>, and the dialog renders that
/// recipe through <see cref="Services.Imaging.ImageRenderer"/> - the same
/// path the canvas, the exporter and the batch use - which is what makes the
/// preview truthful. Confirming the dialog applies the same recipe to the
/// photo as one undo step.
/// </remarks>
internal abstract class EditTool
{
    /// <summary>
    /// Stable identifier used as the preset kind and the last-used key, so a
    /// rename of the title never loses anyone's presets.
    /// </summary>
    public abstract string Id { get; }

    public abstract string Title { get; }

    /// <summary>One sentence under the settings saying what the tool does.</summary>
    public virtual string? Hint => null;

    /// <summary>
    /// Whether the tool opens on the values the recipe already holds rather
    /// than on the last used ones. Adjustment tools do - opening Levels shows
    /// the current levels - while a filter that appends a step starts from
    /// what was used last time.
    /// </summary>
    public virtual bool StartsFromRecipe => false;

    /// <summary>
    /// Whether the dialog offers a grid over the preview, which is how a
    /// horizon or a bulging line is judged.
    /// </summary>
    public virtual bool OffersGrid => false;

    /// <summary>
    /// Whether clicking the preview means something to the tool - the white
    /// balance eyedropper.
    /// </summary>
    public virtual bool AcceptsPreviewPick => false;

    public event EventHandler? SettingsChanged;

    /// <summary>The recipe with this tool's settings applied.</summary>
    public abstract EditRecipe Apply(EditRecipe recipe);

    /// <summary>
    /// Reads the settings back out of a recipe. Only meaningful for tools
    /// with <see cref="StartsFromRecipe"/>; the default keeps the settings.
    /// </summary>
    public virtual void LoadFrom(EditRecipe recipe)
    {
    }

    public abstract void Reset();

    /// <summary>Whether the settings are the defaults, for the preset strip.</summary>
    public abstract bool IsAtDefaults { get; }

    public abstract string Serialize();

    public abstract bool TryDeserialize(string json);

    /// <summary>Builds the settings panel shown beside the preview.</summary>
    public abstract FrameworkElement CreateEditor(EditToolContext context);

    /// <summary>
    /// A click on the preview at a point normalized to 0..1 of the rendered
    /// image; see <see cref="AcceptsPreviewPick"/>.
    /// </summary>
    public virtual void OnPreviewPicked(
        EditToolContext context,
        double x,
        double y,
        BitmapSource displayed)
    {
    }

    protected void RaiseSettingsChanged() =>
        SettingsChanged?.Invoke(this, EventArgs.Empty);
}

/// <summary>
/// A tool whose settings are one immutable record, which gives it value
/// equality, JSON presets and a reset for free.
/// </summary>
internal abstract class EditTool<TSettings> : EditTool
    where TSettings : class
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = false
    };

    private TSettings settings;

    protected EditTool(TSettings defaults)
    {
        Defaults = defaults;
        settings = defaults;
    }

    public TSettings Defaults { get; }

    public TSettings Settings
    {
        get => settings;
        set
        {
            ArgumentNullException.ThrowIfNull(value);
            if (Equals(settings, value))
            {
                return;
            }

            settings = value;
            RaiseSettingsChanged();
        }
    }

    public override void Reset() => Settings = Defaults;

    public override bool IsAtDefaults => Equals(settings, Defaults);

    public override string Serialize() =>
        JsonSerializer.Serialize(settings, JsonOptions);

    public override bool TryDeserialize(string json)
    {
        try
        {
            if (JsonSerializer.Deserialize<TSettings>(json, JsonOptions) is { } loaded)
            {
                Settings = loaded;
                return true;
            }
        }
        catch (JsonException)
        {
            // A preset written by a newer or older shape of the tool is
            // simply not offered; it must never crash the dialog.
        }

        return false;
    }

    public override FrameworkElement CreateEditor(EditToolContext context) =>
        Describe(new ToolPanelBuilder<TSettings>(this), context).Build();

    /// <summary>
    /// Declares the controls of the settings panel. Most tools are a handful
    /// of sliders; the ones that need more add their own elements.
    /// </summary>
    protected abstract ToolPanelBuilder<TSettings> Describe(
        ToolPanelBuilder<TSettings> panel,
        EditToolContext context);
}

/// <summary>
/// What a tool's editor may reach while the dialog is open: the photograph
/// at preview size and the recipe the tool is being applied on top of.
/// </summary>
internal sealed class EditToolContext
{
    private BitmapSource? baseRender;

    public EditToolContext(
        BitmapSource original,
        EditRecipe baseRecipe,
        Func<EditRecipe, BitmapSource> render,
        int fullWidth,
        int fullHeight)
    {
        Original = original;
        BaseRecipe = baseRecipe;
        Render = render;
        FullWidth = fullWidth;
        FullHeight = fullHeight;
    }

    /// <summary>The pixel size of the photograph itself, before any recipe.</summary>
    public int FullWidth { get; }

    public int FullHeight { get; }

    /// <summary>
    /// The photograph at preview size with nothing applied - what Auto
    /// buttons measure, so pressing one twice gives the same answer.
    /// </summary>
    public BitmapSource Original { get; }

    /// <summary>The recipe as it was when the dialog opened.</summary>
    public EditRecipe BaseRecipe { get; }

    /// <summary>Renders a recipe at preview size, synchronously.</summary>
    public Func<EditRecipe, BitmapSource> Render { get; }

    /// <summary>
    /// The preview as the canvas showed it before this tool: the base
    /// recipe rendered once and kept.
    /// </summary>
    public BitmapSource BaseRender => baseRender ??= Render(BaseRecipe);
}

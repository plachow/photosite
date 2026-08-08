using System.Runtime.CompilerServices;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using PhotoSite.Domain;
using PhotoSite.Services.Imaging;

namespace PhotoSite.ViewModels;

/// <summary>
/// The editor's adjustment panel. Every slider reads and writes the recipe of
/// the photo it wraps, so the canvas, the histogram, an export and a batch run
/// all see the same values - nothing is stored twice.
/// </summary>
public sealed class AdjustmentsViewModel : ObservableObject
{
    private readonly PhotoItemViewModel photo;

    public AdjustmentsViewModel(PhotoItemViewModel photo)
    {
        this.photo = photo;
        AutoFixCommand = new RelayCommand(
            () => AutoFixRequested?.Invoke(this, EventArgs.Empty));
        AutoWhiteBalanceCommand = new RelayCommand(
            () => AutoWhiteBalanceRequested?.Invoke(this, EventArgs.Empty));
        ResetToneCommand = new RelayCommand(ResetTone);
        ResetDetailCommand = new RelayCommand(ResetDetail);
    }

    /// <summary>
    /// Raised instead of handled here: Auto Fix has to measure the pixels
    /// currently on the canvas, which only the window can reach.
    /// </summary>
    public event EventHandler? AutoFixRequested;

    public event EventHandler? AutoWhiteBalanceRequested;

    public IRelayCommand AutoFixCommand { get; }

    public IRelayCommand AutoWhiteBalanceCommand { get; }

    public IRelayCommand ResetToneCommand { get; }

    public IRelayCommand ResetDetailCommand { get; }

    private PhotoAdjustments Current => photo.EditRecipe.Adjustments;

    public double Exposure
    {
        get => Current.Exposure;
        set => Set(Current with { Exposure = value });
    }

    public double Contrast
    {
        get => Current.Contrast;
        set => Set(Current with { Contrast = value });
    }

    public double Brightness
    {
        get => Current.Brightness;
        set => Set(Current with { Brightness = value });
    }

    public double Highlights
    {
        get => Current.Highlights;
        set => Set(Current with { Highlights = value });
    }

    public double Shadows
    {
        get => Current.Shadows;
        set => Set(Current with { Shadows = value });
    }

    public double Whites
    {
        get => Current.Whites;
        set => Set(Current with { Whites = value });
    }

    public double Blacks
    {
        get => Current.Blacks;
        set => Set(Current with { Blacks = value });
    }

    public double Clarity
    {
        get => Current.Clarity;
        set => Set(Current with { Clarity = value });
    }

    public double Saturation
    {
        get => Current.Saturation;
        set => Set(Current with { Saturation = value });
    }

    public double Vibrance
    {
        get => Current.Vibrance;
        set => Set(Current with { Vibrance = value });
    }

    public double Temperature
    {
        get => Current.Temperature;
        set => Set(Current with { Temperature = value });
    }

    public double Tint
    {
        get => Current.Tint;
        set => Set(Current with { Tint = value });
    }

    public double Gamma
    {
        get => Current.Gamma;
        set => Set(Current with { Gamma = value });
    }

    public double BlackPoint
    {
        get => Current.BlackPoint;
        set => Set(Current with { BlackPoint = value });
    }

    public double WhitePoint
    {
        get => Current.WhitePoint;
        set => Set(Current with { WhitePoint = value });
    }

    public double MidPoint
    {
        get => Current.MidPoint;
        set => Set(Current with { MidPoint = value });
    }

    public double SharpenAmount
    {
        get => Current.SharpenAmount;
        set => Set(Current with { SharpenAmount = value });
    }

    public double SharpenRadius
    {
        get => Current.SharpenRadius;
        set => Set(Current with { SharpenRadius = value });
    }

    public double SharpenThreshold
    {
        get => Current.SharpenThreshold;
        set => Set(Current with { SharpenThreshold = value });
    }

    public double LuminanceNoiseReduction
    {
        get => Current.LuminanceNoiseReduction;
        set => Set(Current with { LuminanceNoiseReduction = value });
    }

    public double ColorNoiseReduction
    {
        get => Current.ColorNoiseReduction;
        set => Set(Current with { ColorNoiseReduction = value });
    }

    public double Vignette
    {
        get => Current.Vignette;
        set => Set(Current with { Vignette = value });
    }

    public double LensDistortion
    {
        get => Current.LensDistortion;
        set => Set(Current with { LensDistortion = value });
    }

    public double LensVignetting
    {
        get => Current.LensVignetting;
        set => Set(Current with { LensVignetting = value });
    }

    public double ChromaticAberration
    {
        get => Current.ChromaticAberration;
        set => Set(Current with { ChromaticAberration = value });
    }

    // Geometry lives on the recipe rather than in the adjustments, but the
    // panel presents it alongside them.
    public double StraightenAngle
    {
        get => photo.EditRecipe.StraightenAngle;
        set => SetGeometry(
            value,
            photo.EditRecipe.PerspectiveVertical,
            photo.EditRecipe.PerspectiveHorizontal);
    }

    public double PerspectiveVertical
    {
        get => photo.EditRecipe.PerspectiveVertical;
        set => SetGeometry(
            photo.EditRecipe.StraightenAngle,
            value,
            photo.EditRecipe.PerspectiveHorizontal);
    }

    public double PerspectiveHorizontal
    {
        get => photo.EditRecipe.PerspectiveHorizontal;
        set => SetGeometry(
            photo.EditRecipe.StraightenAngle,
            photo.EditRecipe.PerspectiveVertical,
            value);
    }

    public bool HasAnyAdjustment => !Current.IsNeutral;

    /// <summary>
    /// Applies a measured result - Auto Fix or the eyedropper - as one edit.
    /// </summary>
    public void ApplyMeasured(PhotoAdjustments adjustments)
    {
        photo.SetAdjustments(adjustments);
        NotifyAll();
    }

    public PhotoAdjustments Snapshot() => Current;

    private void Set(
        PhotoAdjustments adjustments,
        [CallerMemberName] string? propertyName = null)
    {
        if (adjustments == Current)
        {
            return;
        }

        photo.SetAdjustments(adjustments, propertyName);
        OnPropertyChanged(propertyName);
        OnPropertyChanged(nameof(HasAnyAdjustment));
    }

    private void SetGeometry(
        double straighten,
        double vertical,
        double horizontal,
        [CallerMemberName] string? propertyName = null)
    {
        photo.SetGeometry(straighten, vertical, horizontal, propertyName);
        OnPropertyChanged(propertyName);
    }

    private void ResetTone()
    {
        photo.SetAdjustments(Current.WithNeutralTone());
        NotifyAll();
    }

    private void ResetDetail()
    {
        photo.SetAdjustments(
            Current with
            {
                SharpenAmount = 0,
                SharpenRadius = 1,
                SharpenThreshold = 0,
                LuminanceNoiseReduction = 0,
                ColorNoiseReduction = 0,
                Clarity = 0,
                ChromaticAberration = 0,
                Vignette = 0,
                LensVignetting = 0,
                LensDistortion = 0
            });
        NotifyAll();
    }

    /// <summary>
    /// Re-reads every slider from the recipe, which is what keeps the panel
    /// truthful after an undo, a redo or a preset being applied.
    /// </summary>
    public void NotifyAll() => OnPropertyChanged(string.Empty);

    /// <summary>
    /// Derives a temperature and tint that neutralize the sampled patch. The
    /// white-balance eyedropper is exactly this: "make what I clicked grey".
    /// </summary>
    internal static (double Temperature, double Tint) SolveWhiteBalance(
        double red,
        double green,
        double blue)
    {
        if (red < 2 || green < 2 || blue < 2)
        {
            return (0, 0);
        }

        var average = (red + green + blue) / 3;
        var redGain = average / red;
        var greenGain = average / green;
        var blueGain = average / blue;
        return (
            Math.Round(Math.Clamp((redGain - blueGain) / 0.7 * 100, -100, 100)),
            Math.Round(Math.Clamp((1 - greenGain) / 0.28 * 100, -100, 100)));
    }

    internal static PhotoAdjustments AutoFix(
        PhotoAdjustments current,
        PixelBuffer buffer) =>
        AutoFixAnalyzer.Analyze(buffer, current);
}

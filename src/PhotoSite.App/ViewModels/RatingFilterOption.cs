using CommunityToolkit.Mvvm.ComponentModel;

namespace PhotoSite.ViewModels;

public sealed class RatingFilterOption : ObservableObject
{
    private bool isActive;

    public RatingFilterOption(int minimumRating, string glyph, string toolTip)
    {
        MinimumRating = minimumRating;
        Glyph = glyph;
        ToolTip = toolTip;
    }

    public int MinimumRating { get; }

    public string Glyph { get; }

    public string ToolTip { get; }

    public bool IsActive
    {
        get => isActive;
        internal set => SetProperty(ref isActive, value);
    }
}

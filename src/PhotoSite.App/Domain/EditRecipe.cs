namespace PhotoSite.Domain;

public enum QuarterRotation
{
    None = 0,
    Clockwise90 = 1,
    Clockwise180 = 2,
    Clockwise270 = 3
}

public sealed record EditRecipe(
    QuarterRotation Rotation = QuarterRotation.None,
    bool FlipHorizontal = false)
{
    public static EditRecipe Empty { get; } = new();

    public EditRecipe RotateClockwise() =>
        this with { Rotation = (QuarterRotation)(((int)Rotation + 1) % 4) };

    public EditRecipe RotateCounterClockwise() =>
        this with { Rotation = (QuarterRotation)(((int)Rotation + 3) % 4) };
}

namespace PhotoSite.Domain;

/// <summary>
/// Everything the gallery filter bar can narrow by. An empty collection means
/// "do not filter on this facet", so <see cref="None"/> shows the whole
/// folder and each facet composes with the others by conjunction.
/// </summary>
public sealed record PhotoFilterCriteria
{
    public static PhotoFilterCriteria None { get; } = new();

    public int MinimumRating { get; init; }

    public IReadOnlySet<ColorLabel> ColorLabels { get; init; } =
        new HashSet<ColorLabel>();

    public IReadOnlySet<PhotoFlag> Flags { get; init; } =
        new HashSet<PhotoFlag>();

    /// <summary>Lower-case extensions including the dot, e.g. ".jpg".</summary>
    public IReadOnlySet<string> Formats { get; init; } =
        new HashSet<string>(StringComparer.OrdinalIgnoreCase);

    public IReadOnlySet<string> Cameras { get; init; } =
        new HashSet<string>(StringComparer.OrdinalIgnoreCase);

    public IReadOnlySet<string> Lenses { get; init; } =
        new HashSet<string>(StringComparer.OrdinalIgnoreCase);

    public PhotoOrientation Orientation { get; init; } = PhotoOrientation.Unknown;

    public DateTime? TakenFrom { get; init; }

    public DateTime? TakenTo { get; init; }

    public string? SearchText { get; init; }

    /// <summary>The person whose photos to show, from the face catalogue.</summary>
    public long? PersonId { get; init; }

    /// <summary>Carried alongside the id so the filter label can name them.</summary>
    public string? PersonName { get; init; }

    /// <summary>
    /// Excludes rejects unless the user asked to see them, so a culling pass
    /// visibly shrinks the gallery as it goes.
    /// </summary>
    public bool HideRejected { get; init; }

    public bool IsActive =>
        MinimumRating > 0
        || ColorLabels.Count > 0
        || Flags.Count > 0
        || Formats.Count > 0
        || Cameras.Count > 0
        || Lenses.Count > 0
        || Orientation != PhotoOrientation.Unknown
        || TakenFrom is not null
        || TakenTo is not null
        || HideRejected
        || PersonId is not null
        || !string.IsNullOrWhiteSpace(SearchText);

    /// <summary>
    /// A short human description used by the filter button so an active
    /// filter is never invisible.
    /// </summary>
    public string Describe()
    {
        if (!IsActive)
        {
            return "Filter";
        }

        var parts = new List<string>(6);
        if (MinimumRating > 0)
        {
            parts.Add($"★{MinimumRating}+");
        }

        if (ColorLabels.Count > 0)
        {
            parts.Add($"{ColorLabels.Count} label(s)");
        }

        if (Flags.Count > 0)
        {
            parts.Add(string.Join("/", Flags.Select(flag => flag.ToString())));
        }

        if (Formats.Count > 0)
        {
            parts.Add(string.Join(
                "/",
                Formats.Select(format => format.TrimStart('.').ToUpperInvariant())));
        }

        if (Cameras.Count > 0)
        {
            parts.Add(Cameras.Count == 1 ? Cameras.First() : $"{Cameras.Count} cameras");
        }

        if (Lenses.Count > 0)
        {
            parts.Add(Lenses.Count == 1 ? Lenses.First() : $"{Lenses.Count} lenses");
        }

        if (Orientation != PhotoOrientation.Unknown)
        {
            parts.Add(Orientation.ToString());
        }

        if (TakenFrom is not null || TakenTo is not null)
        {
            parts.Add("date");
        }

        if (PersonId is not null)
        {
            parts.Add(PersonName ?? "person");
        }

        if (HideRejected)
        {
            parts.Add("no rejects");
        }

        return string.Join(" · ", parts);
    }

    public bool Equals(PhotoFilterCriteria? other) =>
        other is not null
        && MinimumRating == other.MinimumRating
        && ColorLabels.SetEquals(other.ColorLabels)
        && Flags.SetEquals(other.Flags)
        && Formats.SetEquals(other.Formats)
        && Cameras.SetEquals(other.Cameras)
        && Lenses.SetEquals(other.Lenses)
        && Orientation == other.Orientation
        && Nullable.Equals(TakenFrom, other.TakenFrom)
        && Nullable.Equals(TakenTo, other.TakenTo)
        && HideRejected == other.HideRejected
        && PersonId == other.PersonId
        && string.Equals(SearchText, other.SearchText, StringComparison.Ordinal);

    public override int GetHashCode() =>
        HashCode.Combine(
            MinimumRating,
            ColorLabels.Count,
            Flags.Count,
            Formats.Count,
            Orientation,
            TakenFrom,
            HideRejected,
            SearchText);
}

using PhotoSite.Domain;

namespace PhotoSite.Services.Batch;

internal sealed record BatchSource(
    string Path,
    EditRecipe Recipe,
    long? TakenAtTicks,
    int? PixelWidth,
    int? PixelHeight);

internal sealed record BatchPlanItem(
    BatchSource Source,
    string DestinationPath,
    bool DestinationExists,
    bool IsSkipped,
    string? SkipReason);

internal sealed record BatchPlan(
    IReadOnlyList<BatchPlanItem> Items,
    string? OutputDirectory)
{
    public int WriteCount => Items.Count(item => !item.IsSkipped);

    public int SkipCount => Items.Count(item => item.IsSkipped);

    public int OverwriteCount =>
        Items.Count(item => !item.IsSkipped && item.DestinationExists);
}

/// <summary>
/// Works out every destination path before a single file is written, so the
/// batch dialog can show a real summary - including how many files would be
/// overwritten - and the user can still change their mind.
/// </summary>
internal static class BatchPlanner
{
    public static BatchPlan Plan(
        IReadOnlyList<BatchSource> sources,
        BatchPreset preset)
    {
        var items = new List<BatchPlanItem>(sources.Count);
        // Names are reserved across the whole plan, not just against the disk,
        // so two sources that would produce the same output name cannot
        // silently overwrite each other mid-run.
        var claimed = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var number = preset.NumberStart;

        foreach (var source in sources)
        {
            var directory = ResolveDirectory(source, preset);
            if (directory is null)
            {
                items.Add(new BatchPlanItem(
                    source,
                    string.Empty,
                    false,
                    true,
                    "No output folder"));
                continue;
            }

            var fileName = BuildFileName(source, preset, number);
            number++;

            var destination = Path.Combine(directory, fileName);
            var existsOnDisk = File.Exists(destination);
            var collides = existsOnDisk || claimed.Contains(destination);

            if (collides && preset.OverwritePolicy == BatchOverwritePolicy.Skip)
            {
                items.Add(new BatchPlanItem(
                    source,
                    destination,
                    true,
                    true,
                    "Already exists"));
                continue;
            }

            if (collides && preset.OverwritePolicy == BatchOverwritePolicy.RenameUnique)
            {
                destination = MakeUnique(destination, claimed);
                existsOnDisk = false;
            }

            if (PathsEqual(destination, source.Path)
                && preset.OverwritePolicy != BatchOverwritePolicy.Overwrite)
            {
                // Writing over the source is only ever done on purpose.
                destination = MakeUnique(destination, claimed);
                existsOnDisk = false;
            }

            claimed.Add(destination);
            items.Add(new BatchPlanItem(
                source,
                destination,
                existsOnDisk,
                false,
                null));
        }

        return new BatchPlan(items, preset.OutputDirectory);
    }

    private static string? ResolveDirectory(
        BatchSource source,
        BatchPreset preset)
    {
        var root = preset.UseSourceDirectory
            ? Path.GetDirectoryName(source.Path)
            : preset.OutputDirectory;
        if (string.IsNullOrWhiteSpace(root))
        {
            return null;
        }

        if (!preset.CreateSubfolderByDate)
        {
            return root;
        }

        var taken = source.TakenAtTicks is { } ticks
            ? new DateTime(ticks)
            : File.Exists(source.Path)
                ? File.GetLastWriteTime(source.Path)
                : DateTime.Now;
        return Path.Combine(root, taken.ToString("yyyy-MM-dd"));
    }

    internal static string BuildFileName(
        BatchSource source,
        BatchPreset preset,
        int number)
    {
        var stem = preset.NameSource switch
        {
            BatchNameSource.CustomText => Sanitize(preset.CustomName),
            BatchNameSource.DateTaken => BuildDateStem(source, preset),
            _ => Path.GetFileNameWithoutExtension(source.Path)
        };

        if (string.IsNullOrWhiteSpace(stem))
        {
            stem = Path.GetFileNameWithoutExtension(source.Path);
        }

        var builder = Sanitize(preset.Prefix) + stem + Sanitize(preset.Suffix);
        if (preset.UseSequentialNumbering)
        {
            var digits = Math.Clamp(preset.NumberDigits, 1, 9);
            builder += preset.NumberSeparator
                       + number.ToString(new string('0', digits));
        }

        return builder + preset.GetExtension(Path.GetExtension(source.Path));
    }

    private static string BuildDateStem(BatchSource source, BatchPreset preset)
    {
        var taken = source.TakenAtTicks is { } ticks
            ? new DateTime(ticks)
            : File.Exists(source.Path)
                ? File.GetLastWriteTime(source.Path)
                : DateTime.Now;
        try
        {
            return Sanitize(taken.ToString(preset.DateFormat));
        }
        catch (FormatException)
        {
            return taken.ToString("yyyy-MM-dd_HHmmss");
        }
    }

    /// <summary>
    /// Strips the characters Windows rejects so a user-typed prefix or date
    /// format cannot produce a path that simply fails to write.
    /// </summary>
    internal static string Sanitize(string? value)
    {
        if (string.IsNullOrEmpty(value))
        {
            return string.Empty;
        }

        var invalid = Path.GetInvalidFileNameChars();
        return new string(
            value.Where(character => !invalid.Contains(character)).ToArray());
    }

    private static string MakeUnique(
        string destination,
        HashSet<string> claimed)
    {
        var directory = Path.GetDirectoryName(destination) ?? string.Empty;
        var stem = Path.GetFileNameWithoutExtension(destination);
        var extension = Path.GetExtension(destination);
        for (var index = 1; ; index++)
        {
            var candidate = Path.Combine(
                directory,
                $"{stem} ({index}){extension}");
            if (!File.Exists(candidate) && !claimed.Contains(candidate))
            {
                return candidate;
            }
        }
    }

    private static bool PathsEqual(string first, string second) =>
        string.Equals(
            Path.GetFullPath(first),
            Path.GetFullPath(second),
            StringComparison.OrdinalIgnoreCase);
}

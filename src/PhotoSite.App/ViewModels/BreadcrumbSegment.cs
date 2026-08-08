using CommunityToolkit.Mvvm.Input;

namespace PhotoSite.ViewModels;

public sealed record BreadcrumbSegment(
    string Name,
    string FullPath,
    IRelayCommand OpenCommand,
    bool IsLast)
{
    /// <summary>
    /// Splits a path into clickable segments, keeping the drive root whole so
    /// "D:\" stays a single crumb rather than becoming "D" and an empty one.
    /// </summary>
    public static IReadOnlyList<BreadcrumbSegment> Build(
        string? path,
        Func<string, IRelayCommand> commandFactory)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            return [];
        }

        string fullPath;
        try
        {
            fullPath = Path.GetFullPath(path);
        }
        catch (Exception exception) when (
            exception is ArgumentException
                or NotSupportedException
                or PathTooLongException)
        {
            return [];
        }

        var root = Path.GetPathRoot(fullPath);
        if (string.IsNullOrEmpty(root))
        {
            return [];
        }

        var segments = new List<BreadcrumbSegment>();
        var relative = Path.TrimEndingDirectorySeparator(fullPath)[root.Length..];
        var accumulated = root;
        segments.Add(new BreadcrumbSegment(
            Path.TrimEndingDirectorySeparator(root),
            root,
            commandFactory(root),
            relative.Length == 0));

        var parts = relative.Split(
            [Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar],
            StringSplitOptions.RemoveEmptyEntries);
        for (var index = 0; index < parts.Length; index++)
        {
            accumulated = Path.Combine(accumulated, parts[index]);
            segments.Add(new BreadcrumbSegment(
                parts[index],
                accumulated,
                commandFactory(accumulated),
                index == parts.Length - 1));
        }

        return segments;
    }
}

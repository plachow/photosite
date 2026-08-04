using System.Collections.Concurrent;

namespace PhotoSite.Services;

/// <summary>
/// Marks files the app itself is about to rewrite (exiftool metadata writes)
/// so the folder watcher does not treat its own edit as an external change,
/// re-read the photo and refresh the gallery for nothing.
/// </summary>
internal static class SelfWriteGuard
{
    private static readonly TimeSpan SuppressionWindow = TimeSpan.FromSeconds(10);
    private static readonly ConcurrentDictionary<string, DateTime> marks =
        new(StringComparer.OrdinalIgnoreCase);

    public static void Mark(string path) =>
        marks[path] = DateTime.UtcNow + SuppressionWindow;

    public static bool ShouldIgnore(string path)
    {
        if (!marks.TryGetValue(path, out var until))
        {
            return false;
        }

        if (DateTime.UtcNow < until)
        {
            return true;
        }

        marks.TryRemove(path, out _);
        return false;
    }
}

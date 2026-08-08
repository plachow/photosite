namespace PhotoSite.ViewModels;

/// <summary>
/// Browser-style back/forward history for the folder the gallery is showing.
/// </summary>
/// <remarks>
/// Only a navigation the user actually asked for is recorded. Restoring the
/// last folder at startup, or re-reading the current one after a batch run,
/// must not push an entry - otherwise Back would appear to do nothing.
/// </remarks>
internal sealed class FolderHistory
{
    private const int MaxEntries = 64;

    private readonly List<string> entries = [];
    private int position = -1;

    public bool CanGoBack => position > 0;

    public bool CanGoForward => position >= 0 && position < entries.Count - 1;

    public string? Current => position >= 0 && position < entries.Count
        ? entries[position]
        : null;

    public void Record(string path)
    {
        var normalized = Normalize(path);
        if (string.Equals(Current, normalized, StringComparison.OrdinalIgnoreCase))
        {
            return;
        }

        // Navigating somewhere new abandons whatever was ahead of us.
        if (position < entries.Count - 1)
        {
            entries.RemoveRange(position + 1, entries.Count - position - 1);
        }

        entries.Add(normalized);
        if (entries.Count > MaxEntries)
        {
            entries.RemoveAt(0);
        }

        position = entries.Count - 1;
    }

    public string? GoBack()
    {
        if (!CanGoBack)
        {
            return null;
        }

        position--;
        return entries[position];
    }

    public string? GoForward()
    {
        if (!CanGoForward)
        {
            return null;
        }

        position++;
        return entries[position];
    }

    /// <summary>
    /// Drops an entry that no longer exists, e.g. a folder deleted while it
    /// was still in the history, and reports the nearest surviving one.
    /// </summary>
    public string? RemoveMissing()
    {
        for (var index = entries.Count - 1; index >= 0; index--)
        {
            if (!Directory.Exists(entries[index]))
            {
                entries.RemoveAt(index);
                if (position >= index)
                {
                    position--;
                }
            }
        }

        position = Math.Clamp(position, -1, entries.Count - 1);
        return Current;
    }

    private static string Normalize(string path) =>
        Path.TrimEndingDirectorySeparator(Path.GetFullPath(path));
}

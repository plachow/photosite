using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;

namespace PhotoSite.ViewModels;

public sealed class DirectoryTreeViewModel : ObservableObject
{
    private readonly Action<string> directorySelected;
    private bool suppressSelection;
    private DirectoryNodeViewModel? selectedNode;

    public DirectoryTreeViewModel(Action<string> directorySelected)
    {
        this.directorySelected = directorySelected;
        BuildRoots();
    }

    public ObservableCollection<DirectoryNodeViewModel> Roots { get; } = new();

    public DirectoryNodeViewModel? SelectedNode
    {
        get => selectedNode;
        private set => SetProperty(ref selectedNode, value);
    }

    public async Task<bool> SelectPathAsync(
        string path,
        bool notifySelection)
    {
        if (!Directory.Exists(path))
        {
            return false;
        }

        var targetPath = NormalizePath(path);
        var root = Roots
            .Where(node => IsAncestorOrSame(node.FullPath, targetPath))
            .OrderByDescending(node => node.FullPath.Length)
            .FirstOrDefault();
        if (root is null)
        {
            return false;
        }

        var current = root;
        var relative = Path.GetRelativePath(root.FullPath, targetPath);
        if (relative != ".")
        {
            foreach (var segment in relative.Split(
                         [Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar],
                         StringSplitOptions.RemoveEmptyEntries))
            {
                current.IsExpanded = true;
                await current.EnsureChildrenLoadedAsync();
                var next = current.Children.FirstOrDefault(
                    child => !child.IsPlaceholder
                             && string.Equals(
                                 Path.GetFileName(child.FullPath),
                                 segment,
                                 StringComparison.OrdinalIgnoreCase));
                if (next is null)
                {
                    return false;
                }

                current = next;
            }
        }

        suppressSelection = !notifySelection;
        try
        {
            if (current.IsSelected)
            {
                SelectedNode = current;
                if (notifySelection)
                {
                    directorySelected(current.FullPath);
                }
            }
            else
            {
                current.IsSelected = true;
            }
        }
        finally
        {
            suppressSelection = false;
        }

        return true;
    }

    private void BuildRoots()
    {
        var paths = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        AddKnownFolder(
            "Pictures",
            Environment.GetFolderPath(Environment.SpecialFolder.MyPictures),
            "🖼",
            paths);
        AddKnownFolder(
            "Desktop",
            Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory),
            "▣",
            paths);
        AddKnownFolder(
            "Home",
            Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
            "⌂",
            paths);

        foreach (var drive in DriveInfo.GetDrives())
        {
            var path = drive.RootDirectory.FullName;
            if (!paths.Add(NormalizePath(path)))
            {
                continue;
            }

            Roots.Add(
                new DirectoryNodeViewModel(
                    GetDriveName(drive),
                    path,
                    "▤",
                    OnNodeSelected));
        }
    }

    private void AddKnownFolder(
        string name,
        string path,
        string glyph,
        HashSet<string> paths)
    {
        if (string.IsNullOrWhiteSpace(path) || !Directory.Exists(path))
        {
            return;
        }

        var normalized = NormalizePath(path);
        if (paths.Add(normalized))
        {
            Roots.Add(
                new DirectoryNodeViewModel(
                    name,
                    normalized,
                    glyph,
                    OnNodeSelected));
        }
    }

    private void OnNodeSelected(DirectoryNodeViewModel node)
    {
        SelectedNode = node;
        if (!suppressSelection)
        {
            directorySelected(node.FullPath);
        }
    }

    private static string GetDriveName(DriveInfo drive)
    {
        try
        {
            return drive.IsReady && !string.IsNullOrWhiteSpace(drive.VolumeLabel)
                ? $"{drive.VolumeLabel} ({drive.Name.TrimEnd('\\')})"
                : drive.Name;
        }
        catch (IOException)
        {
            return drive.Name;
        }
        catch (UnauthorizedAccessException)
        {
            return drive.Name;
        }
    }

    private static bool IsAncestorOrSame(string ancestor, string path)
    {
        var normalizedAncestor = NormalizePath(ancestor);
        var normalizedPath = NormalizePath(path);
        if (string.Equals(
                normalizedAncestor,
                normalizedPath,
                StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        var prefix = normalizedAncestor.EndsWith(Path.DirectorySeparatorChar)
            ? normalizedAncestor
            : normalizedAncestor + Path.DirectorySeparatorChar;
        return normalizedPath.StartsWith(prefix, StringComparison.OrdinalIgnoreCase);
    }

    private static string NormalizePath(string path)
    {
        var fullPath = Path.GetFullPath(path);
        var root = Path.GetPathRoot(fullPath);
        return string.Equals(fullPath, root, StringComparison.OrdinalIgnoreCase)
            ? fullPath
            : fullPath.TrimEnd(
                Path.DirectorySeparatorChar,
                Path.AltDirectorySeparatorChar);
    }
}

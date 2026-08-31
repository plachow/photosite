using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;

namespace PhotoSite.ViewModels;

public sealed class DirectoryNodeViewModel : ObservableObject
{
    private readonly Action<DirectoryNodeViewModel> selected;
    private readonly bool isPlaceholder;
    private Task? loadTask;
    private bool isExpanded;
    private bool isSelected;
    private bool isLoading;

    public DirectoryNodeViewModel(
        string displayName,
        string fullPath,
        string glyph,
        Action<DirectoryNodeViewModel> selected)
        : this(displayName, fullPath, glyph, selected, isPlaceholder: false)
    {
        Children.Add(CreatePlaceholder(selected));
    }

    private DirectoryNodeViewModel(
        string displayName,
        string fullPath,
        string glyph,
        Action<DirectoryNodeViewModel> selected,
        bool isPlaceholder)
    {
        DisplayName = displayName;
        FullPath = fullPath;
        Glyph = glyph;
        this.selected = selected;
        this.isPlaceholder = isPlaceholder;
    }

    public string DisplayName { get; }

    public string FullPath { get; }

    public string Glyph { get; }

    public ObservableCollection<DirectoryNodeViewModel> Children { get; } = new();

    public bool IsPlaceholder => isPlaceholder;

    public bool IsLoading
    {
        get => isLoading;
        private set => SetProperty(ref isLoading, value);
    }

    public bool IsExpanded
    {
        get => isExpanded;
        set
        {
            if (SetProperty(ref isExpanded, value) && value)
            {
                _ = EnsureChildrenLoadedAsync();
            }
        }
    }

    public bool IsSelected
    {
        get => isSelected;
        set
        {
            if (SetProperty(ref isSelected, value) && value && !IsPlaceholder)
            {
                selected(this);
            }
        }
    }

    public Task EnsureChildrenLoadedAsync()
    {
        if (IsPlaceholder)
        {
            return Task.CompletedTask;
        }

        return loadTask ??= LoadChildrenAsync();
    }

    private async Task LoadChildrenAsync()
    {
        IsLoading = true;
        try
        {
            var directories = await Task.Run(ReadDirectories);
            Children.Clear();
            foreach (var directory in directories)
            {
                Children.Add(
                    new DirectoryNodeViewModel(
                        GetDisplayName(directory),
                        directory,
                        "📁",
                        selected));
            }
        }
        finally
        {
            IsLoading = false;
        }
    }

    private IReadOnlyList<string> ReadDirectories()
    {
        try
        {
            return Directory
                .EnumerateDirectories(FullPath)
                .Where(IsVisibleDirectory)
                .OrderBy(
                    GetDisplayName,
                    StringComparer.CurrentCultureIgnoreCase)
                .ToArray();
        }
        catch (IOException)
        {
            return [];
        }
        catch (UnauthorizedAccessException)
        {
            return [];
        }
    }

    private static bool IsVisibleDirectory(string path)
    {
        try
        {
            var attributes = File.GetAttributes(path);
            return (attributes & (FileAttributes.Hidden | FileAttributes.System)) == 0;
        }
        catch (IOException)
        {
            return false;
        }
        catch (UnauthorizedAccessException)
        {
            return false;
        }
    }

    private static string GetDisplayName(string path)
    {
        var name = Path.GetFileName(
            path.TrimEnd(
                Path.DirectorySeparatorChar,
                Path.AltDirectorySeparatorChar));
        return string.IsNullOrWhiteSpace(name) ? path : name;
    }

    private static DirectoryNodeViewModel CreatePlaceholder(
        Action<DirectoryNodeViewModel> selected) =>
        new("Loading…", string.Empty, string.Empty, selected, isPlaceholder: true);
}

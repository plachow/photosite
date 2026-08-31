namespace PhotoSite.Infrastructure;

public sealed class AppPaths
{
    public AppPaths()
    {
        DataDirectory = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "PhotoSite");
        ThumbnailDirectory = Path.Combine(DataDirectory, "thumbnails");
        DatabasePath = Path.Combine(DataDirectory, "catalogue.db");
    }

    public string DataDirectory { get; }

    public string ThumbnailDirectory { get; }

    public string DatabasePath { get; }

    public void EnsureCreated()
    {
        Directory.CreateDirectory(DataDirectory);
        Directory.CreateDirectory(ThumbnailDirectory);
    }
}

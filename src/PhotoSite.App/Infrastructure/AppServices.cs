using PhotoSite.Services;

namespace PhotoSite.Infrastructure;

public sealed class AppServices
{
    public AppServices()
    {
        Paths = new AppPaths();
        Catalog = new PhotoCatalogRepository(Paths.DatabasePath);
        Indexer = new PhotoIndexer([Paths.DataDirectory]);
        Thumbnails = new ThumbnailService(Paths.ThumbnailDirectory);
        Previews = new PreviewService();
    }

    public AppPaths Paths { get; }

    public PhotoCatalogRepository Catalog { get; }

    public PhotoIndexer Indexer { get; }

    public ThumbnailService Thumbnails { get; }

    public PreviewService Previews { get; }

    public async Task InitializeAsync()
    {
        Paths.EnsureCreated();
        await Catalog.InitializeAsync();
    }
}

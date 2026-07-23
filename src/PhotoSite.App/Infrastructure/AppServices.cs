using System.Net.Http;
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
        ImageSaver = new ImageSaveService(Previews);
        ImgurUploader = new ImgurUploadService(
            new HttpClient
            {
                Timeout = TimeSpan.FromMinutes(2)
            });
    }

    public AppPaths Paths { get; }

    public PhotoCatalogRepository Catalog { get; }

    public PhotoIndexer Indexer { get; }

    public ThumbnailService Thumbnails { get; }

    public PreviewService Previews { get; }

    public ImageSaveService ImageSaver { get; }

    public ImgurUploadService ImgurUploader { get; }

    public async Task InitializeAsync()
    {
        Paths.EnsureCreated();
        await Catalog.InitializeAsync();
    }
}

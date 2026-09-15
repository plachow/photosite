using System.Net.Http;
using PhotoSite.Services;
using PhotoSite.Services.Batch;

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
        // A large vision model answers in tens of seconds, and the very first
        // call also waits for Ollama to load the model into memory.
        OllamaVision = new OllamaVisionService(
            new HttpClient
            {
                Timeout = TimeSpan.FromMinutes(10)
            });
        // The ONNX sessions load lazily on the first detection, so carrying
        // the engine costs nothing until the People window is used.
        Faces = new Services.Faces.FaceEngine();
        Updates = new AppUpdateService();
        MetadataWriter = new ExifToolMetadataWriter();
        Geolocator = new ExifToolGeolocator();
        MetadataOutbox = new MetadataOutboxProcessor(Catalog, MetadataWriter);
        BatchPresets = new BatchPresetStore(Catalog);
        ToolPresets = new EditorTools.ToolPresetStore(Catalog);
        Batch = new BatchProcessor(Previews, MetadataWriter);
        Importer = new ImportService();
    }

    public AppPaths Paths { get; }

    public PhotoCatalogRepository Catalog { get; }

    public PhotoIndexer Indexer { get; }

    public ThumbnailService Thumbnails { get; }

    public PreviewService Previews { get; }

    public ImageSaveService ImageSaver { get; }

    public ImgurUploadService ImgurUploader { get; }

    public OllamaVisionService OllamaVision { get; }

    public Services.Faces.FaceEngine Faces { get; }

    public AppUpdateService Updates { get; }

    internal ExifToolMetadataWriter MetadataWriter { get; }

    public ExifToolGeolocator Geolocator { get; }

    internal MetadataOutboxProcessor MetadataOutbox { get; }

    internal BatchPresetStore BatchPresets { get; }

    internal EditorTools.ToolPresetStore ToolPresets { get; }

    internal BatchProcessor Batch { get; }

    internal ImportService Importer { get; }

    public async Task InitializeAsync()
    {
        Paths.EnsureCreated();
        // Registering the bundled libwebp decoder up front means WebP files
        // browse and open like any other photograph, not only on machines
        // where the optional Windows codec happens to be installed.
        ImageEncoder.EnsureCodecsRegistered();
        await Catalog.InitializeAsync();
        MetadataOutbox.Start();
    }
}

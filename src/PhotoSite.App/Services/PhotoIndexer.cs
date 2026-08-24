using System.Threading.Channels;
using PhotoSite.Domain;

namespace PhotoSite.Services;

public readonly record struct PhotoScanResult(
    PhotoRecord Record,
    bool RequiresUpsert);

public sealed class PhotoIndexer
{
    private static readonly HashSet<string> SupportedExtensions =
        new(StringComparer.OrdinalIgnoreCase)
        {
            ".jpg", ".jpeg", ".png", ".bmp", ".gif", ".tif", ".tiff",
            ".webp", ".heic", ".heif", ".jxr", ".wdp", ".dng",
            ".cr2", ".cr3", ".nef", ".arw", ".orf", ".rw2", ".raf", ".pef"
        };

    private readonly string[] excludedDirectories;

    public PhotoIndexer(IEnumerable<string>? excludedDirectories = null)
    {
        this.excludedDirectories = excludedDirectories?
            .Select(Path.GetFullPath)
            .Select(path => path.TrimEnd(
                Path.DirectorySeparatorChar,
                Path.AltDirectorySeparatorChar))
            .ToArray() ?? [];
    }

    internal static bool IsSupportedFile(string path) =>
        SupportedExtensions.Contains(Path.GetExtension(path));

    public IAsyncEnumerable<PhotoScanResult> ScanAsync(
        string rootPath,
        long scanId,
        bool includeSubfolders,
        CancellationToken cancellationToken,
        IReadOnlyDictionary<string, PhotoRecord>? cachedRecords = null)
    {
        var channel = Channel.CreateBounded<PhotoScanResult>(
            new BoundedChannelOptions(256)
            {
                FullMode = BoundedChannelFullMode.Wait,
                SingleReader = true,
                SingleWriter = true
            });

        _ = Task.Run(
            async () =>
            {
                try
                {
                    foreach (var path in EnumerateFiles(
                                 rootPath,
                                 includeSubfolders,
                                 cancellationToken))
                    {
                        cancellationToken.ThrowIfCancellationRequested();
                        if (!IsSupportedFile(path))
                        {
                            continue;
                        }

                        try
                        {
                            var file = new FileInfo(path);
                            if (cachedRecords is not null
                                && cachedRecords.TryGetValue(
                                    file.FullName,
                                    out var cached)
                                && CanReuseMetadata(file, cached))
                            {
                                await channel.Writer.WriteAsync(
                                    new PhotoScanResult(
                                        cached,
                                        RequiresUpsert: false),
                                    cancellationToken);
                                continue;
                            }

                            var record = CreateRecord(file, rootPath, scanId);
                            await channel.Writer.WriteAsync(
                                new PhotoScanResult(
                                    record,
                                    RequiresUpsert: true),
                                cancellationToken);
                        }
                        catch (IOException)
                        {
                            // A file can disappear while a directory is being scanned.
                        }
                        catch (UnauthorizedAccessException)
                        {
                            // Continue with the remaining accessible files.
                        }
                    }

                    channel.Writer.TryComplete();
                }
                catch (Exception exception)
                {
                    channel.Writer.TryComplete(exception);
                }
            },
            CancellationToken.None);

        return channel.Reader.ReadAllAsync(cancellationToken);
    }

    /// <summary>
    /// The single place a file on disk turns into a catalogue row, shared by
    /// the folder scan, the live folder watcher and a directly opened photo so
    /// none of them can drift into recording fewer fields than the others.
    /// </summary>
    internal static PhotoRecord CreateRecord(
        FileInfo file,
        string rootPath,
        long scanId)
    {
        var metadata = PhotoMetadataReader.ReadAll(file.FullName);
        return new PhotoRecord(
            file.FullName,
            rootPath,
            file.Name,
            file.Extension,
            file.Length,
            file.LastWriteTimeUtc.Ticks,
            metadata.Rating,
            scanId,
            metadata.TakenAt.Ticks,
            metadata.TakenAt.Source,
            MetadataVersion: PhotoMetadataReader.CurrentVersion,
            metadata.Title,
            metadata.Description,
            metadata.Latitude,
            metadata.Longitude,
            metadata.ColorLabel,
            PhotoFlag.None,
            metadata.Keywords,
            metadata.PixelWidth,
            metadata.PixelHeight,
            metadata.Camera,
            metadata.Lens,
            metadata.FocalLength,
            metadata.Aperture,
            metadata.ExposureSeconds,
            metadata.Iso,
            GpsErrorMeters: metadata.GpsErrorMeters,
            GpsFixAgeSeconds: metadata.GpsFixAgeSeconds,
            GpsProcessingMethod: metadata.GpsProcessingMethod,
            GpsAltitude: metadata.GpsAltitude);
    }

    internal static bool CanReuseMetadata(
        FileInfo file,
        PhotoRecord cached) =>
        cached.MetadataVersion >= PhotoMetadataReader.CurrentVersion
        && cached.Length == file.Length
        && cached.ModifiedUtcTicks == file.LastWriteTimeUtc.Ticks
        && string.Equals(
            cached.FileName,
            file.Name,
            StringComparison.Ordinal)
        && string.Equals(
            cached.Extension,
            file.Extension,
            StringComparison.OrdinalIgnoreCase);

    private IEnumerable<string> EnumerateFiles(
        string rootPath,
        bool includeSubfolders,
        CancellationToken cancellationToken)
    {
        var pending = new Stack<string>();
        pending.Push(Path.GetFullPath(rootPath));

        while (pending.TryPop(out var directory))
        {
            cancellationToken.ThrowIfCancellationRequested();
            if (IsExcluded(directory))
            {
                continue;
            }

            string[] files;
            string[] directories;
            try
            {
                files = Directory.GetFiles(directory);
                directories = includeSubfolders
                    ? Directory.GetDirectories(directory)
                    : [];
            }
            catch (IOException)
            {
                continue;
            }
            catch (UnauthorizedAccessException)
            {
                continue;
            }

            foreach (var file in files)
            {
                cancellationToken.ThrowIfCancellationRequested();
                yield return file;
            }

            foreach (var childDirectory in directories)
            {
                cancellationToken.ThrowIfCancellationRequested();
                try
                {
                    if ((File.GetAttributes(childDirectory) & FileAttributes.ReparsePoint) == 0
                        && !IsExcluded(childDirectory))
                    {
                        pending.Push(childDirectory);
                    }
                }
                catch (IOException)
                {
                }
                catch (UnauthorizedAccessException)
                {
                }
            }
        }
    }

    private bool IsExcluded(string path)
    {
        foreach (var excludedDirectory in excludedDirectories)
        {
            if (path.Equals(excludedDirectory, StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }

            if (path.StartsWith(
                    excludedDirectory + Path.DirectorySeparatorChar,
                    StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }
        }

        return false;
    }
}

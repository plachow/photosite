using System.IO;
using System.Text.Json;
using PhotoSite.Domain;
using PhotoSite.Infrastructure;

namespace PhotoSite.Services;

/// <summary>
/// Drains the metadata_outbox table in the background and writes the
/// queued rating/title/description/location changes into the photo
/// files (or XMP sidecars) through exiftool.
/// </summary>
internal sealed class MetadataOutboxProcessor : IDisposable
{
    private const int MaxAttempts = 8;
    private static readonly TimeSpan RetryDelay = TimeSpan.FromMinutes(2);

    private readonly PhotoCatalogRepository catalog;
    private readonly ExifToolMetadataWriter writer;
    private readonly SemaphoreSlim signal = new(0, 1);
    private readonly CancellationTokenSource shutdown = new();
    private Task? worker;
    private int retryScheduled;

    public MetadataOutboxProcessor(
        PhotoCatalogRepository catalog,
        ExifToolMetadataWriter writer)
    {
        this.catalog = catalog;
        this.writer = writer;
    }

    public void Start()
    {
        if (worker is not null)
        {
            return;
        }

        catalog.MetadataOutboxChanged += Trigger;
        worker = Task.Run(RunAsync);
        Trigger();
    }

    public void Trigger()
    {
        try
        {
            signal.Release();
        }
        catch (SemaphoreFullException)
        {
            // A drain is already pending; one signal is enough.
        }
    }

    private async Task RunAsync()
    {
        var token = shutdown.Token;
        try
        {
            while (!token.IsCancellationRequested)
            {
                await signal.WaitAsync(token);
                await DrainAsync(token);
            }
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested)
        {
            // Normal shutdown; unwritten entries stay queued for the next run.
        }
    }

    private async Task DrainAsync(CancellationToken cancellationToken)
    {
        if (!writer.IsAvailable)
        {
            return;
        }

        var pending = await catalog.GetPendingMetadataAsync(
            MaxAttempts,
            cancellationToken);
        if (pending.Count == 0)
        {
            return;
        }

        var anyFailed = false;
        foreach (var group in pending.GroupBy(
                     entry => entry.Path,
                     StringComparer.OrdinalIgnoreCase))
        {
            cancellationToken.ThrowIfCancellationRequested();
            var ids = group.Select(entry => entry.Id).ToArray();
            var path = group.Key;
            if (!File.Exists(path))
            {
                await catalog.DeleteMetadataOutboxEntriesAsync(
                    ids,
                    cancellationToken);
                continue;
            }

            var payload = MergePayload(group);
            // exiftool rewrites the file in place, which the folder watcher
            // would otherwise report as an external change.
            SelfWriteGuard.Mark(path);
            var result = await writer.WriteAsync(
                path,
                payload,
                cancellationToken);
            if (result.Success)
            {
                await catalog.DeleteMetadataOutboxEntriesAsync(
                    ids,
                    cancellationToken);
                await RefreshFileStampAsync(path, cancellationToken);
                SelfWriteGuard.Mark(path);
            }
            else
            {
                anyFailed = true;
                await catalog.IncrementMetadataOutboxAttemptsAsync(
                    ids,
                    cancellationToken);
            }
        }

        if (anyFailed)
        {
            ScheduleRetry(cancellationToken);
        }
    }

    /// <summary>
    /// Aligns the catalogue's file stamp with the file rewritten by
    /// exiftool so the next folder scan does not treat our own metadata
    /// write as an external change.
    /// </summary>
    private async Task RefreshFileStampAsync(
        string path,
        CancellationToken cancellationToken)
    {
        var file = new FileInfo(path);
        file.Refresh();
        if (file.Exists)
        {
            await catalog.UpdateFileStampAsync(
                path,
                file.Length,
                file.LastWriteTimeUtc.Ticks,
                cancellationToken);
        }
    }

    private void ScheduleRetry(CancellationToken cancellationToken)
    {
        if (Interlocked.Exchange(ref retryScheduled, 1) == 1)
        {
            return;
        }

        _ = Task.Run(
            async () =>
            {
                try
                {
                    await Task.Delay(RetryDelay, cancellationToken);
                }
                finally
                {
                    Interlocked.Exchange(ref retryScheduled, 0);
                }

                Trigger();
            },
            cancellationToken);
    }

    internal static MetadataWritePayload MergePayload(
        IEnumerable<MetadataOutboxEntry> entries)
    {
        var payload = new MetadataWritePayload();
        foreach (var entry in entries.OrderBy(item => item.Id))
        {
            try
            {
                using var document = JsonDocument.Parse(entry.PayloadJson);
                var root = document.RootElement;
                payload = entry.Kind switch
                {
                    "rating" => payload with
                    {
                        Rating = root.GetProperty("rating").GetInt32()
                    },
                    "title" => payload with
                    {
                        TitleChanged = true,
                        Title = ReadString(root, "title")
                    },
                    "description" => payload with
                    {
                        DescriptionChanged = true,
                        Description = ReadString(root, "description")
                    },
                    "location" => payload with
                    {
                        LocationChanged = true,
                        Latitude = ReadDouble(root, "latitude"),
                        Longitude = ReadDouble(root, "longitude")
                    },
                    "label" => payload with
                    {
                        LabelChanged = true,
                        Label = ReadString(root, "label")
                    },
                    "keywords" => payload with
                    {
                        KeywordsChanged = true,
                        Keywords = ReadString(root, "keywords")
                    },
                    _ => payload
                };
            }
            catch (Exception exception)
                when (exception is JsonException or KeyNotFoundException)
            {
                // A malformed queue entry is skipped; it gets deleted with
                // the rest of its group after a successful write.
            }
        }

        return payload;
    }

    private static string? ReadString(JsonElement root, string name) =>
        root.TryGetProperty(name, out var value)
        && value.ValueKind == JsonValueKind.String
            ? value.GetString()
            : null;

    private static double? ReadDouble(JsonElement root, string name) =>
        root.TryGetProperty(name, out var value)
        && value.ValueKind == JsonValueKind.Number
            ? value.GetDouble()
            : null;

    public void Dispose()
    {
        catalog.MetadataOutboxChanged -= Trigger;
        shutdown.Cancel();
        shutdown.Dispose();
    }
}

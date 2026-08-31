using System.Text.Json;
using PhotoSite.Domain;
using PhotoSite.Infrastructure;

namespace PhotoSite.Services.Batch;

/// <summary>
/// Persists batch and export presets in the catalogue database.
/// </summary>
internal sealed class BatchPresetStore
{
    internal const string BatchKind = "batch";
    internal const string ExportKind = "export";
    private const string SeededSetting = "batch_presets_seeded_v1";

    private readonly PhotoCatalogRepository catalog;

    public BatchPresetStore(PhotoCatalogRepository catalog)
    {
        this.catalog = catalog;
    }

    public async Task<IReadOnlyList<BatchPreset>> LoadAsync(
        string kind = BatchKind,
        CancellationToken cancellationToken = default)
    {
        await SeedBuiltInPresetsAsync(cancellationToken);
        var stored = await catalog.GetPresetsAsync(kind, cancellationToken);
        var presets = new List<BatchPreset>(stored.Count);
        foreach (var (name, payload) in stored)
        {
            try
            {
                if (JsonSerializer.Deserialize<BatchPreset>(payload) is { } preset)
                {
                    presets.Add(preset with { Name = name });
                }
            }
            catch (JsonException)
            {
                // One unreadable preset must not hide the rest of the list.
            }
        }

        return presets;
    }

    public Task SaveAsync(
        BatchPreset preset,
        string kind = BatchKind,
        CancellationToken cancellationToken = default) =>
        catalog.SavePresetAsync(
            kind,
            preset.Name,
            JsonSerializer.Serialize(preset),
            cancellationToken);

    public Task DeleteAsync(
        string name,
        string kind = BatchKind,
        CancellationToken cancellationToken = default) =>
        catalog.DeletePresetAsync(kind, name, cancellationToken);

    /// <summary>
    /// Writes the starter presets exactly once. A user who deletes "Facebook
    /// export" should not find it back the next time the app starts.
    /// </summary>
    private async Task SeedBuiltInPresetsAsync(CancellationToken cancellationToken)
    {
        var seeded = await catalog.GetSettingAsync(SeededSetting, cancellationToken);
        if (bool.TryParse(seeded, out var done) && done)
        {
            return;
        }

        foreach (var preset in BatchPreset.BuiltIn)
        {
            await catalog.SavePresetAsync(
                BatchKind,
                preset.Name,
                JsonSerializer.Serialize(preset),
                cancellationToken);
        }

        await catalog.SetSettingAsync(
            SeededSetting,
            bool.TrueString,
            cancellationToken);
    }
}

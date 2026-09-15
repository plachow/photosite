using PhotoSite.Infrastructure;

namespace PhotoSite.EditorTools;

/// <summary>
/// Named presets and the last used settings of every editor tool, kept in
/// the catalogue's presets table under a kind of their own per tool. The
/// payload is whatever the tool serialized; the store never looks inside.
/// </summary>
internal sealed class ToolPresetStore
{
    private const string KindPrefix = "tool:";
    private const string LastUsedPrefix = "tool_last_used:";

    private readonly PhotoCatalogRepository catalog;

    public ToolPresetStore(PhotoCatalogRepository catalog)
    {
        this.catalog = catalog;
    }

    public Task<IReadOnlyList<(string Name, string Payload)>> LoadAsync(
        string toolId,
        CancellationToken cancellationToken = default) =>
        catalog.GetPresetsAsync(KindPrefix + toolId, cancellationToken);

    public Task SaveAsync(
        string toolId,
        string name,
        string payload,
        CancellationToken cancellationToken = default) =>
        catalog.SavePresetAsync(KindPrefix + toolId, name, payload, cancellationToken);

    public Task DeleteAsync(
        string toolId,
        string name,
        CancellationToken cancellationToken = default) =>
        catalog.DeletePresetAsync(KindPrefix + toolId, name, cancellationToken);

    public Task<string?> LoadLastUsedAsync(
        string toolId,
        CancellationToken cancellationToken = default) =>
        catalog.GetSettingAsync(LastUsedPrefix + toolId, cancellationToken);

    public Task SaveLastUsedAsync(
        string toolId,
        string payload,
        CancellationToken cancellationToken = default) =>
        catalog.SetSettingAsync(LastUsedPrefix + toolId, payload, cancellationToken);
}

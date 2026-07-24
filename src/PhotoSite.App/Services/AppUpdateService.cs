using Velopack;
using Velopack.Sources;

namespace PhotoSite.Services;

public sealed class AppUpdateService
{
    private const string ReleaseRepository =
        "https://github.com/plachow/photosite";

    private readonly UpdateManager updateManager = new(
        new GithubSource(
            ReleaseRepository,
            accessToken: null,
            prerelease: false));

    public bool CanCheckForUpdates => updateManager.IsInstalled;

    public async Task<DownloadedAppUpdate?> CheckAndDownloadAsync(
        Action<int>? progress = null,
        CancellationToken cancellationToken = default)
    {
        if (!CanCheckForUpdates)
        {
            return null;
        }

        if (updateManager.UpdatePendingRestart is { } pendingUpdate)
        {
            return new DownloadedAppUpdate(pendingUpdate);
        }

        var update = await updateManager.CheckForUpdatesAsync();
        if (update is null)
        {
            return null;
        }

        await updateManager.DownloadUpdatesAsync(
            update,
            progress,
            cancellationToken);
        return new DownloadedAppUpdate(update.TargetFullRelease);
    }

    public void ApplyAndRestart(DownloadedAppUpdate update)
    {
        ArgumentNullException.ThrowIfNull(update);
        updateManager.ApplyUpdatesAndRestart(update.Release);
    }
}

public sealed record DownloadedAppUpdate(VelopackAsset Release)
{
    public string Version => Release.Version.ToString();
}

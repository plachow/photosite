namespace PhotoSite.Services;

internal enum PhotoFileTransferMode
{
    Copy,
    Move
}

internal sealed record PhotoFileTransferPlan(
    string SourcePath,
    string DestinationPath,
    string? SourceSidecarPath,
    string? DestinationSidecarPath)
{
    public bool IsNoOp => string.Equals(
        SourcePath,
        DestinationPath,
        StringComparison.OrdinalIgnoreCase);

    public IReadOnlyList<string> ExistingDestinationPaths
    {
        get
        {
            var paths = new List<string>(2);
            if (!IsNoOp && File.Exists(DestinationPath))
            {
                paths.Add(DestinationPath);
            }

            if (SourceSidecarPath is not null
                && DestinationSidecarPath is not null
                && !string.Equals(
                    SourceSidecarPath,
                    DestinationSidecarPath,
                    StringComparison.OrdinalIgnoreCase)
                && File.Exists(DestinationSidecarPath))
            {
                paths.Add(DestinationSidecarPath);
            }

            return paths;
        }
    }
}

internal static class PhotoFileOperations
{
    public static PhotoFileTransferPlan Plan(
        string sourcePath,
        string destinationDirectory,
        PhotoFileTransferMode mode)
    {
        var fullSourcePath = Path.GetFullPath(sourcePath);
        var fullDestinationDirectory = Path.GetFullPath(destinationDirectory);
        var destinationPath = Path.Combine(
            fullDestinationDirectory,
            Path.GetFileName(fullSourcePath));
        var sourceSidecarPath = GetExistingSidecarPath(fullSourcePath);

        if (mode == PhotoFileTransferMode.Copy
            && PathsEqual(fullSourcePath, destinationPath))
        {
            destinationPath = BuildUniqueCopyPath(
                fullSourcePath,
                fullDestinationDirectory,
                sourceSidecarPath is not null);
        }

        var destinationSidecarPath = sourceSidecarPath is null
            ? null
            : ExifToolMetadataWriter.GetSidecarPath(destinationPath);
        return new PhotoFileTransferPlan(
            fullSourcePath,
            destinationPath,
            sourceSidecarPath,
            destinationSidecarPath);
    }

    public static async Task ExecuteAsync(
        PhotoFileTransferPlan plan,
        PhotoFileTransferMode mode,
        bool overwrite,
        CancellationToken cancellationToken = default)
    {
        if (plan.IsNoOp)
        {
            return;
        }

        await Task.Run(
            () =>
            {
                cancellationToken.ThrowIfCancellationRequested();
                EnsureDestinationsAvailable(plan, overwrite);
                TransferFile(
                    plan.SourcePath,
                    plan.DestinationPath,
                    mode,
                    overwrite);

                if (plan.SourceSidecarPath is not null
                    && plan.DestinationSidecarPath is not null
                    && File.Exists(plan.SourceSidecarPath))
                {
                    cancellationToken.ThrowIfCancellationRequested();
                    TransferFile(
                        plan.SourceSidecarPath,
                        plan.DestinationSidecarPath,
                        mode,
                        overwrite);
                }
            },
            cancellationToken);
    }

    internal static string BuildUniqueCopyPath(
        string sourcePath,
        string destinationDirectory,
        bool includeSidecar = false)
    {
        var fullDestinationDirectory = Path.GetFullPath(destinationDirectory);
        var stem = Path.GetFileNameWithoutExtension(sourcePath);
        var extension = Path.GetExtension(sourcePath);
        for (var copyNumber = 1; ; copyNumber++)
        {
            var suffix = copyNumber == 1
                ? " - Copy"
                : $" - Copy ({copyNumber})";
            var candidate = Path.Combine(
                fullDestinationDirectory,
                stem + suffix + extension);
            if (!File.Exists(candidate)
                && (!includeSidecar
                    || !File.Exists(
                        ExifToolMetadataWriter.GetSidecarPath(candidate))))
            {
                return candidate;
            }
        }
    }

    private static string? GetExistingSidecarPath(string sourcePath)
    {
        if (!ExifToolMetadataWriter.UsesSidecar(Path.GetExtension(sourcePath)))
        {
            return null;
        }

        var sidecarPath = ExifToolMetadataWriter.GetSidecarPath(sourcePath);
        return File.Exists(sidecarPath) ? sidecarPath : null;
    }

    private static void EnsureDestinationsAvailable(
        PhotoFileTransferPlan plan,
        bool overwrite)
    {
        if (overwrite)
        {
            return;
        }

        var existing = plan.ExistingDestinationPaths.FirstOrDefault();
        if (existing is not null)
        {
            throw new IOException($"The destination file already exists: {existing}");
        }
    }

    private static void TransferFile(
        string sourcePath,
        string destinationPath,
        PhotoFileTransferMode mode,
        bool overwrite)
    {
        if (mode == PhotoFileTransferMode.Copy)
        {
            File.Copy(sourcePath, destinationPath, overwrite);
        }
        else
        {
            File.Move(sourcePath, destinationPath, overwrite);
        }
    }

    private static bool PathsEqual(string first, string second) =>
        string.Equals(
            Path.GetFullPath(first),
            Path.GetFullPath(second),
            StringComparison.OrdinalIgnoreCase);
}

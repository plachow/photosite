using System.Globalization;
using System.Security.Cryptography;

namespace PhotoSite.Services;

internal sealed record ImportOptions(
    string SourceDirectory,
    string DestinationDirectory,
    bool IncludeSubfolders = true,
    bool OrganizeByDate = true,
    string DateFolderFormat = "yyyy\\\\yyyy-MM-dd",
    bool RenameOnImport = false,
    string RenamePattern = "yyyyMMdd_HHmmss",
    bool SkipAlreadyImported = true,
    string? BackupDirectory = null,
    bool MoveInsteadOfCopy = false);

internal sealed record ImportCandidate(
    string SourcePath,
    string DestinationPath,
    long Length,
    DateTime TakenAt,
    bool IsDuplicate);

internal sealed record ImportPlan(
    IReadOnlyList<ImportCandidate> Candidates)
{
    public int NewCount => Candidates.Count(item => !item.IsDuplicate);

    public int DuplicateCount => Candidates.Count(item => item.IsDuplicate);

    public long TotalBytes =>
        Candidates.Where(item => !item.IsDuplicate).Sum(item => item.Length);
}

internal sealed record ImportProgress(
    int Completed,
    int Total,
    string CurrentFile);

internal sealed record ImportOutcome(
    int Imported,
    int Skipped,
    int Failed,
    IReadOnlyList<string> Errors,
    string DestinationDirectory);

/// <summary>
/// Copies photographs off a camera or memory card into the library.
/// </summary>
internal sealed class ImportService
{
    /// <summary>
    /// Builds the full plan before anything is copied so the dialog can state
    /// how many files are new, how many are already in the library, and how
    /// much will be written.
    /// </summary>
    public ImportPlan Plan(ImportOptions options)
    {
        var candidates = new List<ImportCandidate>();
        if (!Directory.Exists(options.SourceDirectory))
        {
            return new ImportPlan(candidates);
        }

        var claimed = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var files = EnumerateSourceFiles(options).OrderBy(
            path => path,
            StringComparer.OrdinalIgnoreCase);

        foreach (var path in files)
        {
            FileInfo file;
            try
            {
                file = new FileInfo(path);
                if (!file.Exists)
                {
                    continue;
                }
            }
            catch (Exception exception) when (
                exception is IOException or UnauthorizedAccessException)
            {
                continue;
            }

            var takenAt = ReadCaptureDate(file);
            var directory = ResolveDestinationDirectory(options, takenAt);
            var fileName = BuildFileName(file, takenAt, options);
            var destination = Path.Combine(directory, fileName);

            var duplicate = options.SkipAlreadyImported
                            && IsAlreadyImported(file, destination);
            if (!duplicate)
            {
                destination = MakeUnique(destination, claimed);
                claimed.Add(destination);
            }

            candidates.Add(new ImportCandidate(
                file.FullName,
                destination,
                file.Length,
                takenAt,
                duplicate));
        }

        return new ImportPlan(candidates);
    }

    public async Task<ImportOutcome> RunAsync(
        ImportPlan plan,
        ImportOptions options,
        IProgress<ImportProgress>? progress = null,
        CancellationToken cancellationToken = default)
    {
        var work = plan.Candidates.Where(item => !item.IsDuplicate).ToArray();
        var errors = new List<string>();
        var imported = 0;
        var failed = 0;

        for (var index = 0; index < work.Length; index++)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var candidate = work[index];
            progress?.Report(new ImportProgress(
                index,
                work.Length,
                Path.GetFileName(candidate.SourcePath)));

            try
            {
                await Task.Run(
                    () => ImportOne(candidate, options),
                    cancellationToken);
                imported++;
            }
            catch (Exception exception) when (
                exception is IOException
                    or UnauthorizedAccessException
                    or NotSupportedException)
            {
                failed++;
                errors.Add(
                    $"{Path.GetFileName(candidate.SourcePath)}: {exception.Message}");
            }
        }

        progress?.Report(new ImportProgress(
            work.Length,
            work.Length,
            string.Empty));
        return new ImportOutcome(
            imported,
            plan.DuplicateCount,
            failed,
            errors,
            options.DestinationDirectory);
    }

    private static void ImportOne(ImportCandidate candidate, ImportOptions options)
    {
        Directory.CreateDirectory(
            Path.GetDirectoryName(candidate.DestinationPath)!);

        // The backup is written first and from the original: if anything goes
        // wrong later, the card is still the only copy that could be lost.
        if (!string.IsNullOrWhiteSpace(options.BackupDirectory))
        {
            var relative = Path.GetRelativePath(
                options.DestinationDirectory,
                candidate.DestinationPath);
            var backupPath = Path.Combine(options.BackupDirectory, relative);
            Directory.CreateDirectory(Path.GetDirectoryName(backupPath)!);
            File.Copy(candidate.SourcePath, backupPath, overwrite: true);
        }

        File.Copy(candidate.SourcePath, candidate.DestinationPath, overwrite: false);
        CopySidecar(candidate);

        if (options.MoveInsteadOfCopy)
        {
            File.Delete(candidate.SourcePath);
        }
    }

    private static void CopySidecar(ImportCandidate candidate)
    {
        if (!ExifToolMetadataWriter.UsesSidecar(
                Path.GetExtension(candidate.SourcePath)))
        {
            return;
        }

        var sourceSidecar = ExifToolMetadataWriter.GetSidecarPath(
            candidate.SourcePath);
        if (File.Exists(sourceSidecar))
        {
            File.Copy(
                sourceSidecar,
                ExifToolMetadataWriter.GetSidecarPath(candidate.DestinationPath),
                overwrite: false);
        }
    }

    private static IEnumerable<string> EnumerateSourceFiles(ImportOptions options)
    {
        var option = options.IncludeSubfolders
            ? SearchOption.AllDirectories
            : SearchOption.TopDirectoryOnly;
        IEnumerable<string> files;
        try
        {
            files = Directory.EnumerateFiles(
                options.SourceDirectory,
                "*",
                new EnumerationOptions
                {
                    RecurseSubdirectories = options.IncludeSubfolders,
                    IgnoreInaccessible = true,
                    AttributesToSkip = FileAttributes.System
                });
        }
        catch (Exception exception) when (
            exception is IOException or UnauthorizedAccessException)
        {
            yield break;
        }

        foreach (var file in files)
        {
            if (PhotoIndexer.IsSupportedFile(file))
            {
                yield return file;
            }
        }
    }

    internal static string ResolveDestinationDirectory(
        ImportOptions options,
        DateTime takenAt)
    {
        if (!options.OrganizeByDate)
        {
            return options.DestinationDirectory;
        }

        string folder;
        try
        {
            folder = takenAt.ToString(
                options.DateFolderFormat,
                CultureInfo.InvariantCulture);
        }
        catch (FormatException)
        {
            folder = takenAt.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture);
        }

        return Path.Combine(options.DestinationDirectory, folder);
    }

    internal static string BuildFileName(
        FileInfo file,
        DateTime takenAt,
        ImportOptions options)
    {
        if (!options.RenameOnImport)
        {
            return file.Name;
        }

        string stem;
        try
        {
            stem = takenAt.ToString(
                options.RenamePattern,
                CultureInfo.InvariantCulture);
        }
        catch (FormatException)
        {
            stem = takenAt.ToString("yyyyMMdd_HHmmss", CultureInfo.InvariantCulture);
        }

        var invalid = Path.GetInvalidFileNameChars();
        stem = new string(stem.Where(c => !invalid.Contains(c)).ToArray());
        return stem + file.Extension;
    }

    /// <summary>
    /// Treats a file as already imported when a file of the same name and
    /// byte length is already at the destination and its first and last blocks
    /// hash the same. Hashing whole RAW files would make planning an import of
    /// a full card take minutes for no practical gain.
    /// </summary>
    internal static bool IsAlreadyImported(FileInfo source, string destinationPath)
    {
        var destination = new FileInfo(destinationPath);
        if (!destination.Exists || destination.Length != source.Length)
        {
            return false;
        }

        try
        {
            return SampleHash(source.FullName) == SampleHash(destinationPath);
        }
        catch (Exception exception) when (
            exception is IOException or UnauthorizedAccessException)
        {
            return false;
        }
    }

    private static string SampleHash(string path)
    {
        const int blockSize = 64 * 1024;
        using var stream = File.OpenRead(path);
        var buffer = new byte[blockSize * 2];
        var read = stream.Read(buffer, 0, blockSize);
        if (stream.Length > blockSize)
        {
            stream.Seek(-Math.Min(blockSize, stream.Length), SeekOrigin.End);
            read += stream.Read(buffer, read, blockSize);
        }

        return Convert.ToHexString(
            SHA256.HashData(buffer.AsSpan(0, read)));
    }

    private static DateTime ReadCaptureDate(FileInfo file)
    {
        var metadata = PhotoMetadataReader.ReadTakenAt(file.FullName);
        return metadata.Ticks is { } ticks
            ? new DateTime(ticks)
            : file.LastWriteTime;
    }

    private static string MakeUnique(string path, HashSet<string> claimed)
    {
        if (!File.Exists(path) && !claimed.Contains(path))
        {
            return path;
        }

        var directory = Path.GetDirectoryName(path) ?? string.Empty;
        var stem = Path.GetFileNameWithoutExtension(path);
        var extension = Path.GetExtension(path);
        for (var index = 1; ; index++)
        {
            var candidate = Path.Combine(
                directory,
                $"{stem}_{index}{extension}");
            if (!File.Exists(candidate) && !claimed.Contains(candidate))
            {
                return candidate;
            }
        }
    }

    /// <summary>
    /// Removable drives that look like a camera or a card, offered as the
    /// source so the common case needs no browsing at all.
    /// </summary>
    public static IReadOnlyList<(string Label, string Path)> FindRemovableSources()
    {
        var sources = new List<(string, string)>();
        foreach (var drive in DriveInfo.GetDrives())
        {
            try
            {
                if (!drive.IsReady
                    || drive.DriveType is not (DriveType.Removable or DriveType.Fixed))
                {
                    continue;
                }

                var dcim = Path.Combine(drive.RootDirectory.FullName, "DCIM");
                if (Directory.Exists(dcim))
                {
                    var label = string.IsNullOrWhiteSpace(drive.VolumeLabel)
                        ? drive.Name
                        : $"{drive.VolumeLabel} ({drive.Name.TrimEnd('\\')})";
                    sources.Add(($"{label} · DCIM", dcim));
                }
                else if (drive.DriveType == DriveType.Removable)
                {
                    sources.Add((drive.Name, drive.RootDirectory.FullName));
                }
            }
            catch (Exception exception) when (
                exception is IOException or UnauthorizedAccessException)
            {
                // A card removed while the list was being built.
            }
        }

        return sources;
    }
}

namespace PhotoSite.Domain;

public sealed record PhotoRecord(
    string Path,
    string RootPath,
    string FileName,
    string Extension,
    long Length,
    long ModifiedUtcTicks,
    int Rating,
    long ScanId,
    long? TakenAtTicks = null,
    PhotoDateSource TakenAtSource = PhotoDateSource.None,
    int MetadataVersion = 0,
    string? Title = null,
    string? Description = null,
    double? Latitude = null,
    double? Longitude = null);

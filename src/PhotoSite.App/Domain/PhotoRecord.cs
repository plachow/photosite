namespace PhotoSite.Domain;

public sealed record PhotoRecord(
    string Path,
    string RootPath,
    string FileName,
    string Extension,
    long Length,
    long ModifiedUtcTicks,
    int Rating,
    long ScanId);

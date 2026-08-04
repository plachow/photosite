namespace PhotoSite.Domain;

public sealed record MetadataOutboxEntry(
    long Id,
    string Path,
    string Kind,
    string PayloadJson,
    int Attempts);

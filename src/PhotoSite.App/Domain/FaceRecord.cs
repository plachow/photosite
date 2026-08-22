namespace PhotoSite.Domain;

/// <summary>
/// One detected face in the catalogue: where it sits in its photograph
/// (normalized 0..1 of the source frame), how confident the detector was,
/// the recognition embedding, and the person it has been assigned to.
/// </summary>
public sealed record FaceRecord(
    long Id,
    string Path,
    double X,
    double Y,
    double Width,
    double Height,
    double Confidence,
    float[] Embedding,
    long? PersonId = null,
    long? SuggestedPersonId = null);

/// <summary>A named person and how many faces the catalogue holds for them.</summary>
public sealed record PersonRecord(
    long Id,
    string Name,
    int FaceCount);

/// <summary>
/// A person appearing on one photograph - the payload of gallery badges,
/// the info panel's People row and the person filter.
/// </summary>
public sealed record PersonTag(long Id, string Name);

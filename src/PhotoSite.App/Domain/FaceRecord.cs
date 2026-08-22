namespace PhotoSite.Domain;

/// <summary>
/// One detected face in the catalogue: where it sits in its photograph
/// (normalized 0..1 of the source frame), how confident the detector was,
/// the recognition embedding, the person it has been assigned to, and the
/// expression scores (0..1 probabilities; null until an expression scan).
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
    long? SuggestedPersonId = null,
    double? Smile = null,
    double? EyesOpen = null);

/// <summary>
/// One freshly detected face before the catalogue assigns it an id - the
/// payload of a face scan write.
/// </summary>
public sealed record FaceObservation(
    double X,
    double Y,
    double Width,
    double Height,
    double Confidence,
    float[] Embedding,
    long? PersonId = null,
    long? SuggestedPersonId = null,
    double? Smile = null,
    double? EyesOpen = null);

/// <summary>
/// How one photograph's faces scored on expression, aggregated for the
/// gallery badges and the smile/eyes filter. Scored counts only faces the
/// expression models have seen; a face scanned before the models existed
/// contributes to <see cref="FaceCount"/> alone.
/// </summary>
public sealed record ExpressionSummary(
    int FaceCount,
    int ScoredCount,
    int SmilingCount,
    int EyesOpenCount)
{
    /// <summary>Everyone on the photo is scored and smiling.</summary>
    public bool AllSmiling =>
        FaceCount > 0 && SmilingCount == FaceCount;

    /// <summary>Everyone on the photo is scored with both eyes open.</summary>
    public bool AllEyesOpen =>
        FaceCount > 0 && EyesOpenCount == FaceCount;

    /// <summary>At least one scored face is not smiling.</summary>
    public bool AnyNotSmiling => SmilingCount < ScoredCount;

    /// <summary>At least one scored face has its eyes closed.</summary>
    public bool AnyEyesClosed => EyesOpenCount < ScoredCount;
}

/// <summary>
/// The thresholds that turn an expression probability into a verdict, in one
/// place so the scan, the aggregate query and the UI agree.
/// </summary>
public static class FaceExpression
{
    public const double SmileThreshold = 0.5;

    public const double EyesOpenThreshold = 0.5;
}

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

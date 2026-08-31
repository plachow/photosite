namespace PhotoSite.Domain;

public enum PhotoSortField
{
    TakenAt,
    FileName,
    Rating,
    DateModified,
    FileSize,
    Dimensions
}

public static class PhotoSortFields
{
    public static string ToDisplayName(this PhotoSortField field) => field switch
    {
        PhotoSortField.TakenAt => "Date taken",
        PhotoSortField.FileName => "File name",
        PhotoSortField.Rating => "Rating",
        PhotoSortField.DateModified => "Date modified",
        PhotoSortField.FileSize => "File size",
        PhotoSortField.Dimensions => "Dimensions",
        _ => field.ToString()
    };

    public static IReadOnlyList<PhotoSortField> All { get; } =
        Enum.GetValues<PhotoSortField>();
}

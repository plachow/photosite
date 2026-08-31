namespace PhotoSite.Domain;

public enum ImageOutputFormat
{
    KeepOriginal,
    Jpeg,
    Png,
    WebP,
    Tiff,
    Bmp
}

public enum BatchResizeMode
{
    None,
    Width,
    Height,
    LongestSide,
    ShortestSide,
    Percentage
}

public enum BatchNameSource
{
    OriginalName,
    CustomText,
    DateTaken
}

public enum BatchOverwritePolicy
{
    Skip,
    Overwrite,
    RenameUnique
}

public enum BatchMetadataPolicy
{
    Preserve,
    RemoveAll,
    RemoveLocation
}

/// <summary>
/// A reusable batch conversion: what to do, where to put it and what to call
/// the result. The same shape backs the export dialog, so "export this photo"
/// and "convert these 150" cannot drift apart.
/// </summary>
public sealed record BatchPreset
{
    public string Name { get; init; } = "Untitled preset";

    // Destination.
    public string? OutputDirectory { get; init; }

    public bool UseSourceDirectory { get; init; }

    public bool CreateSubfolderByDate { get; init; }

    public BatchOverwritePolicy OverwritePolicy { get; init; } =
        BatchOverwritePolicy.RenameUnique;

    // Format.
    public ImageOutputFormat Format { get; init; } = ImageOutputFormat.Jpeg;

    public int Quality { get; init; } = 88;

    // Geometry.
    public BatchResizeMode ResizeMode { get; init; } = BatchResizeMode.None;

    public int ResizeValue { get; init; } = 2048;

    public bool AllowEnlarge { get; init; }

    /// <summary>Output sharpening, applied after the downscale.</summary>
    public double SharpenAmount { get; init; }

    // Naming.
    public BatchNameSource NameSource { get; init; } = BatchNameSource.OriginalName;

    public string CustomName { get; init; } = "photo";

    public string DateFormat { get; init; } = "yyyy-MM-dd_HHmmss";

    public string Prefix { get; init; } = string.Empty;

    public string Suffix { get; init; } = string.Empty;

    public bool UseSequentialNumbering { get; init; }

    public int NumberStart { get; init; } = 1;

    public int NumberDigits { get; init; } = 3;

    public string NumberSeparator { get; init; } = "_";

    // Content.
    public BatchMetadataPolicy MetadataPolicy { get; init; } =
        BatchMetadataPolicy.Preserve;

    /// <summary>
    /// Whether the saved non-destructive edits are baked into the output.
    /// Off means "convert the original untouched", which is what an archival
    /// format conversion wants.
    /// </summary>
    public bool ApplyEdits { get; init; } = true;

    public bool SupportsQuality =>
        Format is ImageOutputFormat.Jpeg or ImageOutputFormat.WebP;

    public string GetExtension(string sourceExtension) => Format switch
    {
        ImageOutputFormat.Jpeg => ".jpg",
        ImageOutputFormat.Png => ".png",
        ImageOutputFormat.WebP => ".webp",
        ImageOutputFormat.Tiff => ".tif",
        ImageOutputFormat.Bmp => ".bmp",
        _ => string.IsNullOrEmpty(sourceExtension) ? ".jpg" : sourceExtension
    };

    /// <summary>
    /// The starting points offered on a fresh install. They are ordinary
    /// presets - the user can edit or delete any of them.
    /// </summary>
    public static IReadOnlyList<BatchPreset> BuiltIn { get; } =
    [
        new BatchPreset
        {
            Name = "Facebook export",
            Format = ImageOutputFormat.Jpeg,
            Quality = 85,
            ResizeMode = BatchResizeMode.LongestSide,
            ResizeValue = 2048,
            SharpenAmount = 35,
            MetadataPolicy = BatchMetadataPolicy.RemoveLocation,
            Suffix = "_fb"
        },
        new BatchPreset
        {
            Name = "Web gallery",
            Format = ImageOutputFormat.WebP,
            Quality = 82,
            ResizeMode = BatchResizeMode.LongestSide,
            ResizeValue = 1600,
            SharpenAmount = 30,
            MetadataPolicy = BatchMetadataPolicy.RemoveAll,
            NameSource = BatchNameSource.OriginalName
        },
        new BatchPreset
        {
            Name = "Original quality JPEG",
            Format = ImageOutputFormat.Jpeg,
            Quality = 97,
            ResizeMode = BatchResizeMode.None,
            MetadataPolicy = BatchMetadataPolicy.Preserve
        },
        new BatchPreset
        {
            Name = "Small email photos",
            Format = ImageOutputFormat.Jpeg,
            Quality = 78,
            ResizeMode = BatchResizeMode.LongestSide,
            ResizeValue = 1200,
            SharpenAmount = 40,
            MetadataPolicy = BatchMetadataPolicy.RemoveAll,
            Suffix = "_small"
        }
    ];
}

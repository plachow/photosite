namespace PhotoSite.ViewModels;

public enum GalleryViewMode
{
    /// <summary>Thumbnail tiles, sized by the gallery's size slider.</summary>
    Grid,

    /// <summary>
    /// One compact row per photo with the details that a grid cannot show at
    /// a glance - date, dimensions, size and camera.
    /// </summary>
    Details
}

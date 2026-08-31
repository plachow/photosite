namespace PhotoSite.Domain;

public enum PhotoDateSource
{
    None = 0,
    ExifDateTimeOriginal = 1,
    ExifDateTimeDigitized = 2,
    XmpCreateDate = 3
}

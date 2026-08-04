using System.IO;
using System.Runtime.InteropServices;

namespace PhotoSite.Services;

/// <summary>
/// Sends files to the Windows Recycle Bin (no permanent deletion).
/// </summary>
internal static class RecycleBin
{
    private const uint FO_DELETE = 0x0003;
    private const ushort FOF_SILENT = 0x0004;
    private const ushort FOF_NOCONFIRMATION = 0x0010;
    private const ushort FOF_ALLOWUNDO = 0x0040;
    private const ushort FOF_NOERRORUI = 0x0400;

    public static void MoveToRecycleBin(string path)
    {
        var operation = new SHFILEOPSTRUCT
        {
            wFunc = FO_DELETE,
            // The buffer must be double-null terminated; the marshaller
            // appends the second terminator after this explicit one.
            pFrom = path + "\0",
            fFlags = FOF_ALLOWUNDO
                     | FOF_NOCONFIRMATION
                     | FOF_SILENT
                     | FOF_NOERRORUI
        };

        var result = SHFileOperation(ref operation);
        if (result != 0 || operation.fAnyOperationsAborted)
        {
            throw new IOException(
                $"The file could not be moved to the Recycle Bin "
                + $"(error 0x{result:X}): {path}");
        }
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct SHFILEOPSTRUCT
    {
        public IntPtr hwnd;
        public uint wFunc;
        [MarshalAs(UnmanagedType.LPWStr)] public string pFrom;
        [MarshalAs(UnmanagedType.LPWStr)] public string? pTo;
        public ushort fFlags;
        [MarshalAs(UnmanagedType.Bool)] public bool fAnyOperationsAborted;
        public IntPtr hNameMappings;
        [MarshalAs(UnmanagedType.LPWStr)] public string? lpszProgressTitle;
    }

    [DllImport(
        "shell32.dll",
        CharSet = CharSet.Unicode,
        EntryPoint = "SHFileOperationW")]
    private static extern int SHFileOperation(ref SHFILEOPSTRUCT lpFileOp);
}

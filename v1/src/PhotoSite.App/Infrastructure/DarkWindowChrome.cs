using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Interop;

namespace PhotoSite.Infrastructure;

/// <summary>
/// Paints a window's Windows title bar and frame in the app's dark palette,
/// so dialogs match the main window instead of popping up with the system's
/// light chrome. Call <see cref="Apply"/> once from the constructor; users
/// running high contrast keep the system look.
/// </summary>
public static class DarkWindowChrome
{
    private const int UseImmersiveDarkMode = 20;
    private const int UseImmersiveDarkModeLegacy = 19;
    private const int BorderColor = 34;
    private const int CaptionColor = 35;
    private const int TextColor = 36;

    public static void Apply(Window window) =>
        window.SourceInitialized += (_, _) =>
        {
            if (SystemParameters.HighContrast)
            {
                return;
            }

            var handle = new WindowInteropHelper(window).Handle;
            var enabled = 1;
            if (DwmSetWindowAttribute(
                    handle,
                    UseImmersiveDarkMode,
                    ref enabled,
                    sizeof(int)) != 0)
            {
                DwmSetWindowAttribute(
                    handle,
                    UseImmersiveDarkModeLegacy,
                    ref enabled,
                    sizeof(int));
            }

            var caption = ToColorRef(0x11, 0x13, 0x18);
            var text = ToColorRef(0xF2, 0xF4, 0xF8);
            var border = ToColorRef(0x30, 0x35, 0x41);
            DwmSetWindowAttribute(
                handle,
                CaptionColor,
                ref caption,
                sizeof(int));
            DwmSetWindowAttribute(handle, TextColor, ref text, sizeof(int));
            DwmSetWindowAttribute(handle, BorderColor, ref border, sizeof(int));
        };

    private static int ToColorRef(byte red, byte green, byte blue) =>
        red | (green << 8) | (blue << 16);

    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(
        nint windowHandle,
        int attribute,
        ref int attributeValue,
        int attributeSize);
}

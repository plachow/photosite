// Změří dnešní WPF galerii stejnou metodikou jako spikes/grid-wgpu, aby bylo
// proti čemu ta rustovská čísla poměřovat.
//
// Do produkčního kódu se nesahá. Katalog dostane vlastní cestu konstruktorem
// (nic v aplikaci nesahá na App.Services.Catalog), takže ostré catalogue.db
// zůstane netknuté. Jediné, co přeteče, je cache náhledů — ThumbnailPresenter
// si sahá pro App.Services.Thumbnails staticky. Je to čistá cache klíčovaná
// hashem cesty a času zápisu, takže nemůže nic pokazit, a benchmark ji po
// sobě umí smazat.
//
// OnStartup se vyvolává až z Application.Run(), takže ho obejdeme vlastním
// dispatcherem: resources z App.xaml se načtou, ale inicializace ostrých
// služeb ani metadata outbox se nespustí. VelopackApp.Build().Run() zavolat
// musíme — AppServices staví AppUpdateService, jehož UpdateManager bez toho
// hodí výjimku, a ta se ztratí v holém catchi ThumbnailPresenteru.

using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Threading;
using PhotoSite.Controls;
using PhotoSite.Infrastructure;
using PhotoSite.Services;
using PhotoSite.ViewModels;

namespace PhotoSite.GridBench;

internal enum Scenario
{
    Slow,
    Fling,
    Jump,
}

internal static class Program
{
    /// <summary>Rozpočet na snímek, na který se hraje: 120 Hz.</summary>
    private const double BudgetMs = 1000.0 / 120.0;

    /// <summary>Délka scénáře ve virtuálních snímcích: 6 s při 120 Hz.</summary>
    private const int FramesPerScenario = 720;

    /// <summary>Rozjezdové snímky, které se do statistiky nepočítají.</summary>
    private const int WarmupFrames = 60;

    private static double TileSize = 220;

    /// <summary>Bez scrollování — izoluje, co stojí samotné překreslování.</summary>
    internal static bool Still;

    /// <summary>Bez procházení vizuálního stromu — izoluje cenu měřidla.</summary>
    internal static bool NoSample;

    internal static int Frames = FramesPerScenario;

    internal static string? Shot;

    [STAThread]
    private static int Main(string[] args)
    {
        var library = Argument(args, "--library") ?? @"E:\PhotoSiteBench\Library";
        var data = Argument(args, "--data") ?? @"E:\PhotoSiteBench\wpf";
        var rescan = args.Contains("--rescan");
        Still = args.Contains("--still");
        Shot = Argument(args, "--shot");
        if (Argument(args, "--tile") is { } tile)
        {
            TileSize = double.Parse(tile, CultureInfo.InvariantCulture);
        }

        NoSample = args.Contains("--nosample");
        if (Argument(args, "--frames") is { } frameCount)
        {
            Frames = int.Parse(frameCount, CultureInfo.InvariantCulture);
        }

        Directory.CreateDirectory(data);
        var databasePath = Path.Combine(data, "catalogue.db");
        if (rescan && File.Exists(databasePath))
        {
            File.Delete(databasePath);
        }

        // Bez tohohle hodí pole updateManager v AppServices výjimku
        // ("No VelopackLocator has been set"), ThumbnailPresenter ji spolkne
        // holým catchem a v galerii se prostě nikdy neobjeví jediný náhled.
        Velopack.VelopackApp.Build().Run();

        Console.OutputEncoding = System.Text.Encoding.UTF8;
        Console.WriteLine($"katalog:  {databasePath}");
        Console.WriteLine($"knihovna: {library}");
        Console.WriteLine(
            "ostrý katalog v %LOCALAPPDATA% se nepoužívá; "
            + "náhledy jdou do sdílené cache a jdou pak smazat");

        // Skutečná App, protože MainWindow potřebuje resources z App.xaml a
        // jejich root element je právě ona. InitializeComponent je jen načte;
        // OnStartup vyvolává až Application.Run(), který nikdy nezavoláme.
        var application = new global::PhotoSite.App
        {
            ShutdownMode = ShutdownMode.OnExplicitShutdown,
        };
        application.InitializeComponent();

        var repository = new PhotoCatalogRepository(databasePath);
        repository.InitializeAsync().GetAwaiter().GetResult();
        var indexer = new PhotoIndexer([data]);
        var viewModel = new MainViewModel(repository, indexer)
        {
            IncludeSubfolders = true,
            ThumbnailSize = TileSize,
        };
        var window = new MainWindow(viewModel, repository);
        window.RestoreLayoutAsync().GetAwaiter().GetResult();
        window.WindowState = WindowState.Normal;
        window.Width = 1920;
        window.Height = 1080;
        window.Left = 0;
        window.Top = 0;
        window.Show();

        // Výchozí rozvržení dá galerii 401 px, tedy jediný sloupec. Pro
        // srovnání s celoobrazovkovou mřížkou v Rustu jí dáme, co jde:
        // strom složek pryč, náhledový panel na své minimum.
        if (window.FindName("NavigatorColumn") is ColumnDefinition navigator)
        {
            navigator.Width = new GridLength(0);
        }

        if (window.FindName("NavigatorSplitterColumn") is ColumnDefinition navigatorSplitter)
        {
            navigatorSplitter.Width = new GridLength(0);
        }

        if (window.FindName("CatalogColumn") is ColumnDefinition catalogColumn)
        {
            // Hvězdička by se s náhledovým panelem podělila půl na půl a
            // vyšly by tři sloupce. Absolutní šířka nechá panelu jeho minimum
            // a galerii dá sedm sloupců, tedy skoro tolik dlaždic na snímek,
            // kolik jich kreslí rustovská mřížka.
            catalogColumn.Width = new GridLength(1920 - 320 - 40);
        }

        window.UpdateLayout();

        var runner = new Runner(window, viewModel, library);
        window.Dispatcher.InvokeAsync(
            () => runner.RunAsync(),
            DispatcherPriority.ApplicationIdle);

        Dispatcher.Run();
        return runner.ExitCode;
    }

    private static string? Argument(string[] args, string name)
    {
        var at = Array.IndexOf(args, name);
        return at >= 0 && at + 1 < args.Length ? args[at + 1] : null;
    }

    private sealed class Runner(MainWindow window, MainViewModel viewModel, string library)
    {
        private readonly List<double> intervals = new(4096);
        private readonly Stopwatch clock = Stopwatch.StartNew();

        private VirtualizingTilePanel? panel;
        private ListBox? list;
        private double lastTimestamp;
        private int frames;
        private int scenarioIndex;
        private double blankSum;
        private int blankSamples;
        private double extent;
        private ulong rng = 0x2545F4914F6CDD1D;
        private double nextJumpFrame;

        private static readonly Scenario[] Plan =
            [Scenario.Slow, Scenario.Fling, Scenario.Jump];

        public int ExitCode { get; private set; }

        public async Task RunAsync()
        {
            try
            {
                await ScanAsync();
                if (!Locate())
                {
                    Fail("galerii se nepodařilo najít ve vizuálním stromu");
                    return;
                }

                Console.WriteLine();
                Console.WriteLine("— geometrie —");
                // Tier 2 = plná akcelerace. Cokoliv nižšího znamená, že se
                // kreslí softwarově a všechna čísla níž jsou o něčem jiném.
                Console.WriteLine(
                    $"  render tier  {System.Windows.Media.RenderCapability.Tier >> 16}"
                    + $", režim {System.Windows.Media.RenderOptions.ProcessRenderMode}");
                Console.WriteLine($"  okno         {window.ActualWidth:0}×{window.ActualHeight:0}, viditelné {window.IsVisible}");
                Console.WriteLine($"  ListBox      {list!.ActualWidth:0}×{list.ActualHeight:0}, položek {list.Items.Count}");
                Console.WriteLine($"  panel        {panel!.ActualWidth:0}×{panel.ActualHeight:0}, dětí {panel.Children.Count}");
                Console.WriteLine($"  viewport     {panel.ViewportWidth:0}×{panel.ViewportHeight:0}");
                Console.WriteLine($"  obsah        {panel.ExtentWidth:0}×{panel.ExtentHeight:0} px");
                Console.WriteLine($"  ItemWidth    {panel.ItemWidth:0}, ItemHeight {panel.ItemHeight:0}");
                var sloupců = panel.ItemWidth > 0
                    ? Math.Max(1, (int)(panel.ViewportWidth / panel.ItemWidth))
                    : 0;
                Console.WriteLine($"  sloupců      {sloupců}");
                if (panel.Children.Count > 0)
                {
                    var first = FindDescendant<ThumbnailPresenter>(panel.Children[0]);
                    Console.WriteLine(
                        $"  presenter    {(first is null ? "NENALEZEN" : "nalezen")}"
                        + (first is null ? "" : $", Source {(first.Source is null ? "null" : "je")}, SourcePath {first.SourcePath ?? "null"}"));
                }

                // ThumbnailPresenter má holý catch, takže výjimka odsud je
                // neviditelná a projeví se jen tím, že Source zůstane null.
                // Zkusíme tu cestu ručně, ať je vidět.
                try
                {
                    var services = global::PhotoSite.App.Services;
                    Console.WriteLine("  AppServices  postavené");
                    var probe = viewModel.Photos[0].Record.Path;
                    var thumbnail = await services.Thumbnails.GetOrCreateAsync(
                        probe, 204, 204, CancellationToken.None);
                    Console.WriteLine(
                        $"  náhled       {thumbnail} ({(File.Exists(thumbnail) ? "existuje" : "CHYBÍ")})");
                }
                catch (Exception exception)
                {
                    Console.WriteLine(
                        $"  náhled       VÝJIMKA {exception.GetType().Name}: {exception.Message}");
                    if (exception.InnerException is { } inner)
                    {
                        Console.WriteLine($"               vnitřní {inner.GetType().Name}: {inner.Message}");
                    }
                }

                if (Environment.GetCommandLineArgs().Contains("--diagnose"))
                {
                    Console.WriteLine();
                    Console.WriteLine("jen diagnostika, končím");
                    Application.Current.Shutdown();
                    Dispatcher.CurrentDispatcher.InvokeShutdown();
                    return;
                }

                extent = Math.Max(1, panel.ExtentHeight - panel.ViewportHeight);

                for (scenarioIndex = 0; scenarioIndex < Plan.Length; scenarioIndex++)
                {
                    Begin();
                    await PumpAsync();
                    Report();
                }

                Console.WriteLine();
                Console.WriteLine("hotovo");
                Application.Current.Shutdown();
                Dispatcher.CurrentDispatcher.InvokeShutdown();
            }
            catch (Exception exception)
            {
                Fail(exception.ToString());
            }
        }

        /// <summary>
        /// Naindexuje knihovnu přes skutečný PhotoIndexer a naplní galerii.
        /// Tohle je zároveň protějšek k času packeru z rustovského spiku.
        /// </summary>
        private async Task ScanAsync()
        {
            Console.WriteLine();
            Console.WriteLine("— sken knihovny —");
            var started = Stopwatch.StartNew();
            await viewModel.LoadFolderAsync(library, CancellationToken.None);
            var seconds = started.Elapsed.TotalSeconds;
            var count = viewModel.Photos.Count;
            Console.WriteLine(
                $"  {count} fotek za {seconds:0.0} s = {count / seconds:0} fotek/s");
        }

        private bool Locate()
        {
            list = window.FindName("PhotoList") as ListBox;
            if (list is null)
            {
                return false;
            }

            // Panel vzniká až s prvním layoutem, takže si ho vynutíme.
            list.UpdateLayout();
            panel = FindDescendant<VirtualizingTilePanel>(list);
            return panel is not null;
        }

        private void Begin()
        {
            intervals.Clear();
            frames = 0;
            blankSum = 0;
            blankSamples = 0;
            nextJumpFrame = 0;
            lastTimestamp = clock.Elapsed.TotalMilliseconds;
            panel!.SetVerticalOffset(0);
            Console.WriteLine();
            Console.WriteLine($"— {Name(Plan[scenarioIndex])} —");
        }

        /// <summary>
        /// Jeden virtuální snímek = jeden krok scrollu plus vynucený layout,
        /// změřený synchronně. Tohle je práce, kterou musí odvést vlákno UI,
        /// než se dá nakreslit další snímek — a je to jediné číslo, které jde
        /// férově porovnat s časem snímku v Rustu.
        ///
        /// Původní verze poháněla scroll zevnitř CompositionTarget.Rendering.
        /// To vypadalo jako měření, ale vycházelo stejně (106,6 ms) pro 12 i
        /// pro 108 dlaždic — bylo to kvantování na sedm vsynců, ne zátěž.
        /// </summary>
        private async Task PumpAsync()
        {
            for (frames = 1; frames <= Frames; frames++)
            {
                var started = clock.Elapsed.TotalMilliseconds;
                if (!Still)
                {
                    Drive();
                }

                window.UpdateLayout();
                var cost = clock.Elapsed.TotalMilliseconds - started;
                if (frames > WarmupFrames)
                {
                    intervals.Add(cost);
                }

                if (!NoSample && frames % 6 == 0)
                {
                    Sample();
                }

                // Snímek uprostřed flingu, protějšek k tomu z Rustu.
                if (Shot is not null
                    && Plan[scenarioIndex] == Scenario.Fling
                    && frames == Frames / 2)
                {
                    Capture(Shot);
                }

                // Pustit ke slovu dispatcher, ať doběhnou pokračování
                // asynchronního načítání náhledů. Bez toho by se galerie
                // nikdy nedoplnila a měřilo by se prázdno.
                await Dispatcher.Yield(DispatcherPriority.Background);
            }
        }

        /// <summary>
        /// Posun scrollu na virtuálních 120 Hz — stejná vzdálenost na snímek
        /// jako v rustovském spiku, bez ohledu na to, jak rychle se doopravdy
        /// kreslí. WPF je zamčené na vsync, takže reálný čas běží pomaleji,
        /// ale práce připadající na jeden snímek je totožná.
        /// </summary>
        private void Drive()
        {
            var offset = panel!.VerticalOffset;
            switch (Plan[scenarioIndex])
            {
                case Scenario.Slow:
                    panel.SetVerticalOffset(offset + 240.0 / 120.0);
                    break;
                case Scenario.Fling:
                    panel.SetVerticalOffset(offset + extent / Frames);
                    break;
                case Scenario.Jump:
                    if (frames >= nextJumpFrame)
                    {
                        rng ^= rng << 13;
                        rng ^= rng >> 7;
                        rng ^= rng << 17;
                        panel.SetVerticalOffset((rng >> 11) / (double)(1UL << 53) * extent);
                        // 0,3 s virtuálního času, stejně jako v Rustu.
                        nextJumpFrame = frames + 36;
                    }

                    break;
            }
        }

        /// <summary>
        /// Kolik realizovaných dlaždic právě nemá obrázek. Tohle je protějšek
        /// ke sloupci „ostrých dlaždic“ — jenže WPF nemá co nakreslit místo
        /// toho, takže prázdná dlaždice je opravdu prázdná.
        /// </summary>
        private void Sample()
        {
            if (panel is null || panel.Children.Count == 0)
            {
                return;
            }

            var blank = 0;
            var total = 0;
            foreach (UIElement child in panel.Children)
            {
                var thumbnail = FindDescendant<ThumbnailPresenter>(child);
                if (thumbnail is null)
                {
                    continue;
                }

                total++;
                if (thumbnail.Source is null)
                {
                    blank++;
                }
            }

            if (total > 0)
            {
                blankSum += (double)blank / total;
                blankSamples++;
            }
        }

        private void Report()
        {
            if (intervals.Count == 0)
            {
                return;
            }

            var sorted = intervals.ToArray();
            Array.Sort(sorted);
            double At(double q) => sorted[(int)((sorted.Length - 1) * q)];

            var median = At(0.50);
            var over = sorted.Count(value => value > BudgetMs);
            var sharp = blankSamples > 0 ? 1.0 - blankSum / blankSamples : 0;

            Console.WriteLine(
                string.Create(
                    CultureInfo.InvariantCulture,
                    $"  snímků {sorted.Length}   p50 {median:0.00} ms   "
                    + $"p95 {At(0.95):0.00}   p99 {At(0.99):0.00}   "
                    + $"p99.9 {At(0.999):0.00}   max {sorted[^1]:0.00}"));
            Console.WriteLine(
                string.Create(
                    CultureInfo.InvariantCulture,
                    $"  nad rozpočet {BudgetMs:0.0} ms: {over} "
                    + $"({100.0 * over / sorted.Length:0.00} %)   "
                    + $"ostrých dlaždic {sharp * 100:0} %"));
        }

        private void Fail(string message)
        {
            Console.Error.WriteLine(message);
            ExitCode = 1;
            Application.Current.Shutdown();
            Dispatcher.CurrentDispatcher.InvokeShutdown();
        }

        /// <summary>
        /// Vyfotí okno přes RenderTargetBitmap — tedy skutečný vizuální strom,
        /// ne zvláštní režim, který by kreslil něco jiného.
        /// </summary>
        private void Capture(string path)
        {
            var bitmap = new System.Windows.Media.Imaging.RenderTargetBitmap(
                (int)window.ActualWidth,
                (int)window.ActualHeight,
                96,
                96,
                System.Windows.Media.PixelFormats.Pbgra32);
            bitmap.Render(window);
            var encoder = new System.Windows.Media.Imaging.PngBitmapEncoder();
            encoder.Frames.Add(
                System.Windows.Media.Imaging.BitmapFrame.Create(bitmap));
            using var file = File.Create(path);
            encoder.Save(file);
            Console.WriteLine($"  snímek uložen: {path}");
        }

        private static string Name(Scenario scenario) => scenario switch
        {
            Scenario.Slow => "pomalý scroll",
            Scenario.Fling => "fling přes celou knihovnu",
            _ => "skoky",
        };

        private static T? FindDescendant<T>(DependencyObject root)
            where T : DependencyObject
        {
            var count = VisualTreeHelper.GetChildrenCount(root);
            for (var i = 0; i < count; i++)
            {
                var child = VisualTreeHelper.GetChild(root, i);
                if (child is T match)
                {
                    return match;
                }

                var deeper = FindDescendant<T>(child);
                if (deeper is not null)
                {
                    return deeper;
                }
            }

            return null;
        }
    }
}

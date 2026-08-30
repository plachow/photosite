# grid-wpf

Změří **dnešní galerii** stejnou metodikou jako [grid-wgpu](../grid-wgpu/README.md),
aby bylo proti čemu ta rustovská čísla poměřovat.

```bash
dotnet build spikes/grid-wpf/PhotoSite.GridBench.csproj -c Release -p:OutDir=<jinam>\
PhotoSite.GridBench.exe --library "E:\PhotoSiteBench\Library" --data "E:\PhotoSiteBench\wpf"
```

Přepínače: `--rescan` zahodí katalog, `--diagnose` vypíše geometrii a skončí,
`--still` neposouvá, `--nosample` nepočítá prázdné dlaždice, `--tile`, `--frames`,
`--shot <png>`.

## Do produkčního kódu se nesahá

Měří se **skutečné `MainWindow`** — pravý `VirtualizingTilePanel`, pravý
`ThumbnailPresenter`, pravá dlaždicová šablona s hvězdičkami a odznaky.

* **Ostrý katalog se nepoužije.** V celé aplikaci není jediný výskyt
  `App.Services.Catalog`; repozitář vždycky teče konstruktorem, takže benchmark
  podstrčí vlastní `catalogue.db`.
* **`OnStartup` se nespustí**, protože ho vyvolává až `Application.Run()`.
  Stačí `InitializeComponent()` a vlastní `Dispatcher.Run()`.
* **`VelopackApp.Build().Run()` zavolat musíme.** Bez toho hodí pole
  `updateManager` v `AppServices` výjimku, `ThumbnailPresenter` ji spolkne
  svým `catch { Source = null; }` a v galerii se nikdy neobjeví jediný náhled.
* **Cache náhledů přeteče** do `%LOCALAPPDATA%`, protože `ThumbnailPresenter`
  si pro `App.Services.Thumbnails` sahá staticky. Je to čistá cache klíčovaná
  hashem cesty a času zápisu, takže na cizí fotky sáhnout nemůže. Tenhle běh
  jich vyrobil 52 915 (1 393 MB) a byly po měření smazány podle času vzniku.

## Co se měří

`SetVerticalOffset` plus vynucený `UpdateLayout()`, změřené synchronně. To je
práce, kterou musí vlákno UI odvést, než se dá nakreslit další snímek.

**Nezahrnuje to vykreslení na GPU ani present**, kdežto rustovské číslo je celý
čas snímku včetně nich. WPF je tím pádem spíš podhodnocené než nadhodnocené.

Scroll běží na virtuálních 120 Hz, tedy stejná vzdálenost na snímek jako
v Rustu — WPF je zamčené na vsync, takže reálný čas plyne pomaleji, ale práce
připadající na jeden krok je totožná.

## Naměřeno

Stejný stroj, okno 1920×1080, galerie 1541×934 px, dlaždice 220 px → **7 sloupců,
42 realizovaných dlaždic** (rustovská mřížka jich má 8 a ~38). Mediány ze tří běhů.

| scénář | p50 | p95 | p99 | p99.9 | max | nad 8,3 ms | ostrých |
|---|---:|---:|---:|---:|---:|---:|---:|
| pomalý scroll | 1,40 ms | 1,82 | **34,31** | 45,37 | 58,96 | 1,21 % | 100 % |
| fling | 128,03 ms | 179,07 | **210,93** | 224,45 | 233,69 | 100 % | **0 %** |
| skoky | 0,00 ms | 0,00 | **141,07** | 188,11 | 205,43 | 2,73 % | 100 % |

U skoků je p50 nula, protože se skáče jen každý 36. krok a mezi skoky není co
přepočítávat. Rozhodující je tam p99.

### Sken

| | |
|---|---|
| první sken 57 606 fotek | **208,9 s** (276 fotek/s), jen metadata |
| opakovaný sken | **1,8 s** (32 000 fotek/s) |

Ta inkrementální detekce podle délky a času zápisu funguje přesně, jak má.
Náhledy se přitom negenerují — ty vznikají až za běhu, když dlaždice doopravdy
někdo uvidí.

## Tři chyby v měřidle, které stojí za zapsání

První tři sady čísel z tohohle spiku byly nesmysl. Všechny tři chyby byly v
benchmarku, ne v aplikaci:

1. **Nezavolaný Velopack** → `AppServices` házelo výjimku → nevznikl jediný
   náhled → „0 % ostrých“ ve všech scénářích. Nebyl to výsledek, byla to
   chybějící data, protože `Sample()` na prázdném panelu vrací nulu.
2. **Galerie měla 401 px**, tedy jeden sloupec z 57 606 řádků. Výchozí
   rozvržení jí dá úzký proužek.
3. **Scroll poháněný zevnitř `CompositionTarget.Rendering`.** Vypadalo to jako
   měření a dávalo to působivých 106 ms na snímek — jenže **stejných 106,6 ms
   pro 12, 42 i 108 dlaždic**. Konstanta napříč devítinásobkem práce není
   zátěž, je to kvantování na sedm vsynců. Vyvolávat layout zevnitř render
   passu znamená měřit hodiny, ne aplikaci.

Že je ta oprava správná, se pozná podle toho, že nová metrika **škáluje**:
0,54 ms při 12 dlaždicích, 0,93 při 42, 1,88 při 108. A nehybná galerie stojí
nulu, jak má.

# proto

Proklikatelný prototyp: strom složek, mřížka diapozitivů, plný náhled.
Rust, egui nad wgpu — Metal, Vulkan i DX12 z téhož kódu, jedna binárka
19 MB bez runtime.

```bash
cargo run --release
cargo run --release -- --folder "E:\PhotoSiteBench\Library" --recursive --theme 2
```

Přepínače jsou jen pro pohodlí při zkoušení: `--folder`, `--recursive`,
`--theme 0..2`, `--shot <png>` (vyfotí okno a skončí — jinak by nešlo ověřit,
že to opravdu kreslí, co si myslím).

## Co umí

* **Strom složek** vlevo, děti se načítají až při rozbalení. Kořeny jsou disky
  a domovská složka.
* **Mřížka** uprostřed s ručně virtualizovaným scrollem — kreslí se jen
  viditelné řádky, takže na počtu fotek nezáleží.
* **Rekurzivně** přepíná mezi jednou složkou a celým podstromem.
* **Plný náhled** vpravo po kliknutí; než se dekóduje, ukáže se zvětšený
  náhled z mřížky, takže panel nikdy neproblikne prázdnotou.
* **Tři motivy** v jedné struktuře. Přepnutí je výměna palety, ne převazování
  stylů — barvy si odsud berou i posuvníky, combo a rámy dlaždic.

## Načítání: dvě chyby a jedno měření

První verze loaderu byla pomalá tak, že prototyp neobstál při běžném použití —
vzít scrollbar, hodit ho do dolní třetiny a pustit znamenalo pět až sedm sekund
prázdných dlaždic. Za to mohly dvě chyby a jedno špatné rozhodnutí:

1. **Neomezená FIFO fronta bez rušení.** Při tažení projedou viewportem tisíce
   fotek a každá se do fronty zařadí. Po puštění se těch patnáct viditelných
   ocitne až za několika tisíci mrtvými požadavky. Stará WPF verze tohle nemá,
   protože `ThumbnailPresenter` ruší práci v `Unloaded`.
   Oprava: **místo fronty seznam přání, který se každý snímek přepíše** na to,
   co je právě vidět. Co z něj vypadne, nikdo nedekóduje.
2. **Zdvojené dekódování.** Vlákno dlaždici dokončí, hlavní vlákno ji ještě
   nestihne převzít — a příští snímek ji napíše do přání znovu. Při třiceti
   vláknech se práce znásobí. Oprava: množina `done` a překreslení hned, ne za
   60 ms.
3. **Škálování v DCT doméně skoro nic neušetří.** Měřeno na 60 fotkách:

   | | |
   |---|---|
   | čtení souboru (3,8 MB) | 5,2 ms |
   | `jpeg-decoder`, scale 640 | 31,9 ms → 851 kpx |
   | `jpeg-decoder`, scale 320 | 30,6 ms → 213 kpx |
   | `zune-jpeg`, **plný** dekód | 42,0 ms → **13 140 kpx** |

   Čtyřikrát míň výstupních pixelů, ale jen o 4 % rychleji: entropické
   dekódování všech koeficientů se udělá tak jako tak a to je ten čas. Stará
   WPF verze je rychlá proto, že WIC to umí v SIMD.

   Oprava, která zabrala nejvíc: **skoro každá fotka nese v EXIFu vlastní
   náhled 160×120.** Přečíst prvních 128 kB souboru a dekódovat ho stojí
   zlomek milisekundy místo pětatřiceti. Dlaždice se proto plní nejdřív tímhle
   a teprve pak se doostřují.

### Výsledek

Nasimulované tažení do dvou třetin knihovny 57 606 fotek a puštění:

| | před | po |
|---|---:|---:|
| žádná prázdná dlaždice | 5–7 s | **60 ms** |
| všechny dlaždice ostré | 5–7 s | **156 ms** |
| průměr na jeden dekód | 70,6 ms | 21,8 ms |

**Během samotného tažení jsou dlaždice pořád prázdné** a to se bez
předpočítaných dat obejít nedá — přesně na to je rezidentní 32px vrstva
z [grid-wgpu](../grid-wgpu/README.md), která ale potřebuje připravený balík.

## Naměřeno mimochodem

| | |
|---|---|
| vylistování 7 558 fotek (jedna složka) | **4 ms** |
| vylistování 57 606 fotek (celá knihovna rekurzivně) | **36 ms** |
| binárka | 19 MB, žádný runtime |

Nic se nepředpočítává a není tu žádný katalog. Náhledy se dekódují za běhu na
30 vláknech, v DCT doméně na osminu — plné rozlišení nikdy nevznikne, a proto
se to stíhá.

## Dlaždice

Rám v barvě `tile`, pod fotkou tmavší plocha `well`, pod ní proužek s názvem
v `caption`. Plastičnost dělá **jeden pixel** světla nahoře a vlevo a jeden
pixel stínu dole a vpravo. Víc by z toho udělalo tlačítko.

Název je jednořádkový s výpustkou, ne zalomený.

## Multiplatformnost

Ověřeno `cargo check` na tři cíle z jednoho stroje (typová kontrola linker
nepotřebuje, takže cross-check jde odkudkoliv):

```bash
cargo check --target x86_64-unknown-linux-gnu
cargo check --target aarch64-apple-darwin
cargo check
```

Všechny tři projdou beze změny zdrojáku. Dvě věci to ale stálo:

* **`eframe` s `default-features = false` vypne i `x11` a `wayland`** a Linux
  pak skončí na `compile_error!("The platform you're compiling for is not
  supported by winit")`. Musí se dopsat ručně.
* **Kořeny stromu jsou jediné místo, kde na platformě záleží.** Windows mají
  písmena disků, macOS `/Volumes`, Linux připojené svazky pod `/media`,
  `/run/media/$USER` a `/mnt`.

Co bude potřeba na cílovém stroji:

| | |
|---|---|
| Linux | ovladač Vulkanu (mesa stačí) a X11 nebo Wayland; `libxkbcommon` |
| macOS | nic, Metal je součástí systému — ale podepsat a notarizovat |
| Windows | nic, Vulkan i DX12 jsou v ovladači |

Fonty si egui nese vlastní, takže žádný fontconfig ani systémové písmo. Dialogy
souborů zatím nejsou; až budou, `rfd` je nativní na všech třech.

## Co tu není

Katalog, metadata, hvězdičky, štítky, vícenásobný výběr, klávesnice, editor,
obličeje, EXIF panel. Je to skořápka na proklikání, ne aplikace.

Dvě věci, na které jsem narazil a stojí za zapamatování:

* `▸` a `▾` **v základním fontu egui nejsou** a vyjdou jako prázdné čtverečky.
  Trojúhelníky ve stromu se proto kreslí, nepíšou.
* eframe 0.36 změnilo `App::update(ctx)` na `App::ui(ui)` a egui sjednotilo
  `SidePanel` i `TopBottomPanel` do jednoho `Panel`. Návody staršího data
  neprojdou.

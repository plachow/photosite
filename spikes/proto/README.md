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

## Co tu není

Katalog, metadata, hvězdičky, štítky, vícenásobný výběr, klávesnice, editor,
obličeje, EXIF panel. Je to skořápka na proklikání, ne aplikace.

Dvě věci, na které jsem narazil a stojí za zapamatování:

* `▸` a `▾` **v základním fontu egui nejsou** a vyjdou jako prázdné čtverečky.
  Trojúhelníky ve stromu se proto kreslí, nepíšou.
* eframe 0.36 změnilo `App::update(ctx)` na `App::ui(ui)` a egui sjednotilo
  `SidePanel` i `TopBottomPanel` do jednoho `Panel`. Návody staršího data
  neprojdou.

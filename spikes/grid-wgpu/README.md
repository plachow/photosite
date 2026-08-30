# grid-wgpu

Mřížka náhledů na wgpu, která nedělá nic než scroll. Měří strop, ne aplikaci.

```bash
cargo run --release --bin pack -- "E:\PhotoSiteBench\Library" "E:\PhotoSiteBench\pack"
cargo run --release -- --bench
cargo run --release            # interaktivně, kolečkem myši, +/- mění velikost dlaždic
```

## Jak to je postavené

Dvě vrstvy nad jedním balíkem dat:

* **Rezidentní** — všech 57 606 fotek ve 32 px, ve čtyřech atlasových stránkách
  4096×4096 (256 MB VRAM). Nahraje se celá při startu a už se jí nikdo nedotkne.
  `mips.pack` je zapsaný rovnou v rozvržení atlasu, takže start je `write_texture`
  z mmapu a **nula práce na CPU**.
* **Ostrá** — 256px náhledy v LRU cache 2 048 atlasových buněk, dekódované na
  30 vláknech z mmapnutého `thumbs.pack` a nahrávané se stropem 24 nahrání na
  snímek.

Kreslí se **dvěma draw cally na celý viewport**: jeden instancovaný pro dlaždice,
které mají ostrou verzi, druhý pro zbytek. Ne jeden na dlaždici.

## Naměřeno

Ryzen 9 9950X3D (16 j. / 32 vl.), RTX 5090, Vulkan, okno 1920×1080, dlaždice
220 px → 8 sloupců, 7 201 řádků, 1,63 Mpx obsahu. Tři běhy, uvedeny mediány.

### Příprava balíku (jednorázově)

| | |
|---|---|
| vstup | 57 606 JPEGů, 246,5 GB |
| čas | **681 s** (11,4 min), 85 fotek/s, 370 MB/s |
| nečitelných | 3 |
| `thumbs.pack` | 807 MB |
| `mips.pack` | 256 MB (4 stránky atlasu) |
| `index.bin` | 1,4 MB |

Dekóduje se rovnou v DCT doméně na 1/8, takže plné rozlišení nikdy nevznikne.

### Start

| | |
|---|---|
| nahrání rezidentní vrstvy (256 MB) | **105 ms** |
| do prvního snímku, teplá cache | **~590 ms** |
| do prvního snímku, studená cache | 1 305 ms |

### Scroll

Scénáře běží na **virtuálních 120 Hz**: scroll se posouvá o 1/120 s bez ohledu
na to, jak rychle se doopravdy kreslí. Bez toho by platilo, že čím rychlejší
renderer, tím menší krok na snímek — a tím snadněji dekodéry stíhají. Takový
benchmark měří sám sebe a vždycky vyjde hezky.

| scénář | p50 | p95 | p99 | p99.9 | max | nad 8,3 ms | ostrých |
|---|---:|---:|---:|---:|---:|---:|---:|
| pomalý scroll (240 px/s) | 0,13 ms | 0,20 | **0,50** | 0,74 | 0,75 | 0 z 660 | 100 % |
| fling (celá knihovna za 6 s) | 0,28 ms | 1,43 | **2,14** | 3,21 | 3,50 | 0 z 660 | 0 % |
| skoky (20× náhodně) | 0,15 ms | 0,81 | **1,40** | 1,97 | 2,07 | 0 z 660 | 89 % |

Fling znamená 2 260 px na snímek, tedy deset řádků a osmdesát nových fotek
každých 8,3 ms. Dekódovat se z toho nestihne nic — proto **0 % ostrých**.

## Co je na tom podstatné

**Ta nula ve sloupci „ostrých" u flingu je ten výsledek.** Ne to, že se dekód
nestíhá — ten se nestíhat nemůže, to je fyzika. Podstatné je, že se u toho
nestalo vůbec nic: žádné prázdné dlaždice, žádné zahozené snímky, p99 na
2,14 ms z 8,3ms rozpočtu. Nese to rezidentní vrstva a stojí to jednu texturu.

Jak to při tom vypadá, je vidět na `snimek-fling.png` — je to rozmazané, ale
jsou to rozpoznatelné fotky. `snimek-ostry.png` je totéž při pomalém scrollu.
Oba snímky jsou vyčtené z framebufferu stejnou cestou, kterou se měří, ne
z nějakého zvláštního režimu.

Druhý závěr: **1,63 milionu pixelů obsahu a 57 606 položek nedělá scrollu vůbec
nic.** Seřazený seznam se drží celý v paměti, viditelné okno je průchod plochým
polem, a počet dlaždic na obrazovce je konstantní bez ohledu na velikost
knihovny. Nic v té smyčce neroste s počtem fotek.

## Kde jsou ty výsledky slabé

* Není proti čemu měřit. Baseline z dnešního WPF listu chybí.
* Jen Windows a Vulkan. Metal a Linux nezměřeny.
* Mřížka nic neumí — žádný výběr, žádná metadata, žádné popisky pod dlaždicemi,
  žádné překreslení při změně velikosti okna za běhu. Reálná aplikace přidá
  práci na snímek, i když u kreslení té mřížky jí moc nepřidá.
* LRU cache buněk se prochází lineárně (2 048 položek na jedno nahrání). Při
  24 nahráních na snímek je to zanedbatelné, ale v produkci by to chtělo haldu.
* Rezidentní vrstva je 32 px. Při 64 px by vypadala výrazně líp a stála 1 GB
  VRAM místo 256 MB — to je rozhodnutí, které stojí za změření samo o sobě.

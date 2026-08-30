# Spiky

Vyhoditelný kód. Nic tady se neudržuje, nic se nereviduje, nic z toho se nesmí
dostat do produkce. **Výstupem spiku jsou naměřená čísla, ne kód** — kód je jen
stvrzenka, aby šlo za půl roku dohledat, na základě čeho padlo rozhodnutí.

## Mřížka náhledů: dnešek proti stropu

Otázka: **udrží se plynulý scroll nad desítkami tisíc fotek, když se mřížka
kreslí přes GPU atlas místo přes widgety?**

Dva prototypy nad toutéž knihovnou 57 606 JPEGů (246,5 GB), stejný stroj
(Ryzen 9 9950X3D, RTX 5090), stejné scénáře, scroll na virtuálních 120 Hz.

* [grid-wgpu](grid-wgpu/README.md) — holá mřížka na wgpu, atlas a rezidentní
  vrstva. Měří **strop**.
* [grid-wpf](grid-wpf/README.md) — **skutečné `MainWindow`** dnešní aplikace.
  Měří **dnešek**.

### p99 práce na jeden krok scrollu

| scénář | dnešek (WPF) | strop (wgpu) | poměr |
|---|---:|---:|---:|
| pomalý scroll | 34,31 ms | 0,50 ms | 69× |
| fling přes celou knihovnu | 210,93 ms | 2,14 ms | 99× |
| skoky | 141,07 ms | 1,40 ms | 101× |

### Kolik dlaždic má obrázek

| scénář | dnešek | strop |
|---|---:|---:|
| pomalý scroll | 100 % | 100 % |
| **fling** | **0 %** | **0 %**, ale kreslí rezidentní 32px vrstvu |
| skoky | 100 % | 89 % |

Tenhle druhý řádek je celý ten rozdíl a je vidět na dvou snímcích:
[grid-wpf/snimek-fling.png](grid-wpf/snimek-fling.png) je 42 černých obdélníků
s názvy souborů, [grid-wgpu/snimek-fling.png](grid-wgpu/snimek-fling.png) je 42
rozmazaných, ale rozpoznatelných fotografií. Obojí ve stejném okamžiku téhož
scénáře, kdy nestíhá dekódovat ani jeden.

### Sken a příprava

| | dnešek | strop |
|---|---|---|
| první otevření složky | **208,9 s** (jen metadata) | **681 s** (metadata i všechny náhledy) |
| opakované otevření | **1,8 s** | 0,6 s |
| náhledy | vznikají za běhu, když je někdo uvidí | hotové předem, 807 MB + 256 MB |

Dnešek je rychlejší na otevření, protože drahou část odkládá — a pak ji platí
při každém scrollu znovu. Rust zaplatí jedenáct minut jednou a pak nikdy.

## proto — proklikatelná skořápka

[proto](proto/README.md) je třetí spike: strom složek, mřížka diapozitivů a
plný náhled v egui nad wgpu. Není to měření, je to na osahání — jestli ta
architektura unese skutečné ovládání a jestli tmavě šedé fotografické
prostředí vypadá, jak má.

Celá knihovna 57 606 fotek se vylistuje **za 36 ms**, binárka má 19 MB a
nepotřebuje runtime. Katalog ani předpočítané náhledy tam nejsou vůbec —
dekóduje se za běhu.

## Co z toho plyne

Ustálený pomalý scroll zvládá dnešní galerie dobře: 1,40 ms mediánu je hluboko
v rozpočtu. Problém je až v tom, co se stane, když se musí **vyrobit nové
dlaždice** — a to je přesně fling a skok. Tam WPF platí zhruba 1,5 ms za každou
nově realizovanou dlaždici (vygenerovat kontejner, rozbalit šablonu, poslat
požadavek na náhled), kdežto v atlasu je dlaždice jen položka v instančním
bufferu a pixely už dávno leží na GPU.

Rozdíl tedy není v jazyce a není ani v tom, jak rychle se dá kreslit. Je
v tom, že jedna architektura má co nakreslit vždycky a druhá ne.

## Co se tím pořád ještě neví

* **Změřeno jen na Windows a Vulkanu.** Metal na Macu a Vulkan na Linuxu se
  chovají jinak, a macOS je prioritní platforma. Balík náhledů má 1,1 GB,
  takže se dá odnést na flashce.
* **Druhý toolkit nestojí.** Bez C++/Qt 6 se neví, kolik z toho stropu ukrojí
  dospělý toolkit se scene graphem, který si atlasuje sám.
* **Rustovská mřížka nic neumí** — žádný výběr, žádná metadata, žádné popisky.
  Reálná aplikace práci na snímek přidá, i když samotnému kreslení mřížky moc ne.
* **Čísla nejsou úplně souměřitelná.** WPF se měří jen layout bez vykreslení
  na GPU, Rust celý čas snímku včetně present. WPF je tím spíš podhodnocené.

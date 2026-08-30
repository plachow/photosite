# Spiky

Vyhoditelný kód. Nic tady se neudržuje, nic se nereviduje, nic z toho se nesmí
dostat do produkce. **Výstupem spiku jsou naměřená čísla, ne kód** — kód je jen
stvrzenka, aby šlo za půl roku dohledat, na základě čeho padlo rozhodnutí.

## grid-wgpu — strop pro mřížku náhledů

Otázka: **udrží se plynulý scroll nad desítkami tisíc fotek, když se mřížka
kreslí přes GPU atlas místo přes widgety?**

Odpověď: ano, a s obrovskou rezervou. Podrobnosti v
[grid-wgpu/README.md](grid-wgpu/README.md).

| scénář | p99 času snímku | nad rozpočet 120 Hz | ostrých dlaždic |
|---|---:|---:|---:|
| pomalý scroll | 0,50 ms | 0 z 660 | 100 % |
| fling přes celou knihovnu | 2,14 ms | 0 z 660 | 0 % |
| skoky | 1,40 ms | 0 z 660 | 89 % |

57 606 fotek, RTX 5090, Vulkan. Mediány p99 ze tří běhů.

### Co se tím ještě neví

* **Baseline z dnešního WPF listu chybí.** Bez něj je to absolutní číslo bez
  měřítka. Změřit ho znamená naindexovat testovací knihovnu, což by zapsalo
  57 606 řádků do ostrého `catalogue.db` — čeká to na úpravu `AppPaths`, aby
  šla cesta ke katalogu přebít.
* **Změřeno jen na Windows a Vulkanu.** Metal na Macu a Vulkan na Linuxu se
  chovají jinak; balík náhledů má 1,1 GB, takže se dá odnést na flash disku.
* **Druhý prototyp (C++ / Qt 6) nestojí.** Bez něj se neví, kolik z toho stropu
  ukrojí dospělý toolkit.

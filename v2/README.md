# PhotoSite v2

Zelená louka. Rust, egui nad wgpu, Windows / macOS / Linux z jednoho zdrojáku.

```bash
cd v2
cargo run --release -p photosite-ui              # aplikace
cargo run --release -p photosite-cli -- doctor   # kde co leží
cargo test --workspace                           # 50 testů, bez okna a bez GPU
```

## Rozvržení

```
crates/
  photosite-core     doména, katalog + migrace, cesty, nastavení, log, úlohy, příkazy
  photosite-image    dekódování, zmenšování, EXIF
  photosite-ui       egui — jediná crate, která ví o GPU
  photosite-cli      headless: pustí celou pipeline bez okna
```

**Jádro ani obrázková crate nesmí mít v grafu závislostí `egui`, `eframe`,
`wgpu` ani `winit`.** Hlídá to test `crates/photosite-core/tests/bez_ui.rs`,
který se ptá `cargo tree`, ne zdrojáků — nepřímá závislost by se v `use`
řádcích nepoznala. Není to čistota pro čistotu: ztráta téhle hranice je jediný
důvod, proč byl port v1 drahý.

CLI existuje kvůli témuž. Na CI runneru není obrazovka ani GPU, ale sken,
katalog i migrace se otestovat musí.

## Zásady, které stály za to

Každá z nich je z konkrétní chyby, ne z příručky.

**Cesty jdou přebít.** `--data <složka>` nebo `PHOTOSITE_DATA` přesměruje data,
nastavení, cache i log pod jeden kořen. Ve v1 byla cesta natvrdo a znamenalo to,
že se nedalo nic změřit ani vyzkoušet jinak než na ostrých datech.

**Nic neselže mlčky.** Úloha, která spadne, si to nese ve stavu a jde do logu.
Poškozené nastavení se odloží stranou a nezahodí. Pád nechá hlášení. Prototyp
kvůli jednomu spolknutému výsledku nevykreslil jediný náhled a nikde o tom
nebylo ani slovo.

**Práce na pozadí není fronta.** [`Wishlist`](crates/photosite-core/src/jobs.rs)
se každý snímek přepíše na to, co je právě vidět; co z něj vypadne, se nikdy
neudělá. Fronta znamenala sedm sekund prázdných dlaždic po puštění scrollbaru,
protože se čekalo na tisíce mrtvých požadavků.

**Migrace od první tabulky.** Bez nich se nedá vydat druhá verze. Katalog
z novějšího buildu se odmítne otevřít místo toho, aby se poškodil.

**Registr příkazů od začátku.** Zkratky, tlačítka i budoucí menu čtou z jednoho
seznamu. Dodělat ho do hotového UI znamená projít každé tlačítko zvlášť.

**Zápisy po dávkách.** Řádek na transakci vypadá nevinně; sken 7 558 fotek
s ním trval 44 s, s jednou transakcí na tisíc řádků 0,3 s.

## Stav

Hotové zázemí, žádné fotografické funkce. Mřížka, strom, náhled a tři motivy
jsou z prototypu, aby bylo co spustit.

| | |
|---|---|
| testů | 50 (včetně 6 000 fuzz případů na EXIF) |
| sken 7 558 fotek | 0,3 s; opakovaně 0,1 s |
| otevření složky v UI | 7 558 fotek, žádná prázdná dlaždice do 160 ms |
| `cargo clippy -D warnings` | čisté |

Cross-check z Windows projde pro `photosite-image` na oba cíle. Zbytek ne,
protože `libsqlite3-sys` s `bundled` kompiluje céčko a to potřebuje cizí
toolchain — **skutečné ověření pro Linux a macOS dělá CI**, kde jsou nativní
runnery.

## Co chybí a ví se o tom

Otevírací dialog (`rfd`), sledování změn na disku (`notify`), jediná instance,
lokalizace, přístupnost, podpis a notarizace pro macOS, automatické aktualizace.
Nic z toho nevyžaduje přepisovat, co je hotové.

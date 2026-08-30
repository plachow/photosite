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

## Lokalizace

Výchozí a zároveň záložní jazyk je **en-US**; co v jiném překladu chybí, se
vezme z angličtiny, aby na obrazovce nikdy nezůstal holý klíč. Balíček je
`crates/photosite-core/i18n/<jazyk>/photosite.ftl`, přepnout se dá
`--lang cs-CZ` nebo v nastavení.

Formát je [Fluent](https://projectfluent.org), a to schválně: čeština má tři
plurálové tvary (*1 fotka, 2 fotky, 5 fotek*) a formát, který to neumí, by se
musel později přepsat i se všemi voláními. Zaokrouhlování čísel je proto taky
v balíčku (`NUMBER($ms, maximumFractionDigits: 0)`), ne v kódu — kolik
desetinných míst se ukáže, je rozhodnutí jazyka.

**V UI ani v CLI není jediný natvrdo psaný text** a hlídají to dva testy
v `crates/photosite-ui/tests/preklady.rs`: jeden ověří, že každý použitý klíč
v balíčku existuje, druhý prochází zdrojáky a hlásí literál předaný widgetu.
Třetí test kontroluje samotné hledání — měřidlo, které si přestane všímat, je
horší než žádné.

Registr příkazů ani palety motivů neobsahují texty, jen klíče. Přidat jazyk
tedy znamená přidat jeden `.ftl` a jeden řádek do `i18n::available()`.

## Nastavení

Čtyři zásady, každá z nich se zpětně zavádí draho:

1. **Výchozí hodnoty jsou v kódu, jednou.** `Default` je jediný zdroj pravdy.
2. **Do souboru jde jen to, co se liší.** Uložit celý strom znamená zmrazit
   dnešní výchozí hodnoty u každého, kdo aplikaci jednou spustil. Když se je
   rozmyslíme, dostane je i on.
3. **Všechno má cestu.** `gallery.tile_size` se dá číst i zapsat jako text,
   takže obrazovka nastavení je vygenerovaná z popisu polí (`TUNABLES`), ne
   psaná ručně. Přidat volbu znamená přidat řádek.
4. **Reset je první třída.** Jedna položka, celá skupina, nebo všechno. Poloha
   okna a naposledy otevřená složka se resetem nemyslí — to nejsou předvolby.

Sady (`PRESETS`) jsou hotové kombinace pro lidi, kteří nechtějí nic ladit:
*Comfortable*, *Compact*, *Large tiles*, *Modest machine*. Jsou to data, ne kód.

**V UI nezůstala jediná konstanta ovlivňující vzhled nebo chování** — mezera
mezi dlaždicemi, poměr stran, výška proužku, počet nahrání za snímek, strop
textur, počet vláken i délka nečinné pauzy jsou v nastavení. Test
`popis_poli_pokryva_presne_to_co_v_nastaveni_je` hlídá, že se popis a
skutečnost nerozejdou v žádném směru.

## Motivy

Barvy jsou **data**, ne konstanty v kódu. Dnes vestavěné (`dark`, `light`,
`grey`, `sepia`, `seabreeze`), ale právě proto, že jsou serializovatelné,
půjde je časem načíst ze souboru, aniž by se sáhlo na cokoliv, co kreslí.
`appearance.theme = "automatic"` jde podle systému a `theme_dark` s
`theme_light` říkají, který motiv pro který režim.

Jádro o egui neví: `Color` je trojice bajtů, převod si dělá vrstva UI.

Jedna past, která tu byla a je opravená: `ctx.set_visuals` zapisuje jen do
slotu právě zvoleného motivu. Při startu systém ještě nestihl ohlásit režim,
takže se zapsalo do tmavého — a jakmile dorazilo „světlo", egui přeplo na
světlý slot se svými vlastními barvami a okno nastavení svítilo bíle uprostřed
tmavé aplikace. Zapisuje se proto do obou.

## Stav

Hotové zázemí, žádné fotografické funkce. Mřížka, strom, náhled a tři motivy
jsou z prototypu, aby bylo co spustit.

| | |
|---|---|
| testů | 77 (včetně 6 000 fuzz případů na EXIF) |
| sken 7 558 fotek | 0,3 s; opakovaně 0,1 s |
| otevření složky v UI | 7 558 fotek, žádná prázdná dlaždice do 160 ms |
| `cargo clippy -D warnings` | čisté |
| motivů | 5 plus automatika podle systému |
| položek nastavení | 27, obrazovka vygenerovaná z popisu |

Cross-check z Windows projde pro `photosite-image` na oba cíle. Zbytek ne,
protože `libsqlite3-sys` s `bundled` kompiluje céčko a to potřebuje cizí
toolchain — **skutečné ověření pro Linux a macOS dělá CI**, kde jsou nativní
runnery.

## Co chybí a ví se o tom

Otevírací dialog (`rfd`), sledování změn na disku (`notify`), jediná instance,
přístupnost, podpis a notarizace pro macOS, automatické aktualizace. Z jazyků
zatím jen angličtina — cs-CZ je první na řadě.
Nic z toho nevyžaduje přepisovat, co je hotové.

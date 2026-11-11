# NX Launcher: piano di setup

Un'app per Mac che gestisce una libreria di giochi costruiti con [nxnative](https://github.com/seedds/nxnative), li compila dall'interfaccia (senza terminale), ne regola le impostazioni e li avvia.

---

## 0. Come funziona nxnative e che cosa deve fare il launcher

nxnative **non è un emulatore**: lo script `scripts/build_ipa_from_nsp.py` trasforma un gioco (NSP/XCI/NSZ, `.nup` o una cartella di dump) in una **app Mac** (`--platform mac-app`) o in un **IPA** da installare via sideload. Il codice del gioco gira nativo su Apple silicon e la grafica passa su Metal.

Il repo ha un contratto apposta per i front end: [`docs/frontends.md`](https://github.com/seedds/nxnative/blob/main/docs/frontends.md). Il launcher si appoggia solo a quello, senza patch a nxnative. Le interfacce sono cinque:

| # | Interfaccia | Cosa usa il launcher |
|---|---|---|
| 1 | **Build** | `python3 scripts/build_ipa_from_nsp.py <cartella> --progress json --platform mac-app --work-dir <dir> -o <out>` stampa **una riga JSON per evento** su stdout (`stage`, `step`, `progress`, `stage_end`, `game`, `shaders_stale`, `say`). Exit code: `0` ok, `1` errore (l'ultimo `say` comincia con `[RED] `), `2` argomenti sbagliati (solo stderr, nessun JSON), `130` interrotto. |
| 2 | **Lista impostazioni** | `<App>.app/Contents/Resources/dm/launcher.json` (schema 1). Ogni riga ha `env`, `kind` (`choice`, `range` o `toggle`), `section`, `label`, `default` e i campi opzionali `level`, `needs`, `base`. JSON Schema: `docs/launcher.schema.json`. |
| 3 | **Scelte salvate** | Dominio preferenze `org.nxnative.<titleID>`, chiave `launcher.<ENV>` (stringa). Si salvano **solo i valori diversi dal default**. Per le cloud save c'è la chiave `cloudSaves.folder`. |
| 4 | **Avvio** | Esegue `<App>.app/Contents/MacOS/nxnative` con `DM_LAUNCHER=0` (salta la finestra impostazioni interna) e con eventuali override **temporanei** nell'ambiente. Exit: `0` uscita normale, `70` GPU revocata da macOS, `134` abort. Log in `~/Library/Logs/nxnative/<titleID>.log`, salvataggi in `~/Library/Application Support/nxnative/<titleID>/save/`. Il titleID è `DMTitleID` nell'`Info.plist`. |
| 5 | **Extra** | Cheat (`…/<titleID>/cheats/<buildID>.txt` + `.enabled`), cartella cloud save, testi in italiano del prompt Home (`DM_HOME_QUIT_TEXT`). |

> **Esistono già dei front end**, tutti in Swift: [NX2Apple](https://github.com/LNPANDY/NX2Apple) (circa 6k ★), [NX Play](https://github.com/varyform/nx-native-wrapper) e [LumaNX](https://github.com/Xc1Ym/LumaNX). Vale la pena guardarli per idee e UX. Il nostro è un progetto per imparare e per avere il launcher fatto a modo nostro.

---

## 1. Electron o Rust? → **Tauri 2** (backend Rust + UI web)

| | Electron | **Tauri 2** | Rust puro (egui/iced) | SwiftUI |
|---|---|---|---|---|
| Dimensione app | ~150–250 MB | **~10–15 MB** | ~10 MB | ~5 MB |
| RAM a riposo | 200–400 MB | **40–80 MB** | 30 MB | 30 MB |
| UI a griglia con copertine | facile (web) | **facile (web)** | laboriosa | facile |
| Processi, JSONL, plist, CFPreferences | Node + `child_process`; CFPreferences solo via `defaults` | **`tokio::process`, `serde`, crate `plist`, `core-foundation`** (API nativa) | come Tauri | nativo |
| Multipiattaforma | sì | sì | sì | no |
| Curva di apprendimento | bassa | media (Rust) | alta | media |

**Perché Tauri:**
1. **nxnative gira solo su Mac con Apple silicon**, quindi il vantaggio principale di Electron (lo stesso Chromium su ogni sistema) qui non serve. Tauri usa WKWebView, che è già nel sistema.
2. **Il launcher resta aperto mentre il gioco gira.** I giochi Switch 2 usano molta RAM e GPU, e Electron occuperebbe 300 MB inutilmente.
3. **La parte difficile è il "sistema"**: gestire processi figli, leggere l'output JSONL in streaming, inoltrare i segnali, leggere i plist e scrivere CFPreferences. In Rust tutto questo è tipizzato e diretto, senza passare da comandi shell.
4. **La UI resta web** (React + TS), quindi la griglia giochi, i form delle impostazioni e le progress bar sono veloci da fare.

---

## 2. Prerequisiti: stato di questo Mac (verificato l'11/11/2026)

| Requisito | Stato |
|---|---|
| macOS 26.6+ su Apple silicon | ✅ macOS 27.0.1, arm64 |
| Xcode 27.2 (compilatore Metal degli shader) | ✅ 27.2 selezionato |
| Python 3.12+ **arm64** (non Rosetta) | ✅ 3.12.0 arm64 (python.org) |
| Homebrew | ✅ 7.0.12 |
| Node + pnpm (UI) | ✅ Node 24.12, pnpm 12.15 |
| **Rust** | ❌ **da installare** |
| Spazio disco (≈ 2× il gioco + 10 GB per build) | ⚠️ 186 GB liberi: basta per uno o due giochi grossi. Meglio un SSD esterno per le cartelle di build. |

---

## 3. Setup passo per passo

### 3.1 Rust
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
```bash
rustup default stable && cargo --version
```

### 3.2 Motore nxnative (una volta sola)
```bash
xcodebuild -downloadComponent MetalToolchain
```
```bash
brew install xcodegen zstd glslang
```
```bash
git submodule add https://github.com/seedds/nxnative.git engine
```
```bash
python3 -m venv engine-venv && engine-venv/bin/pip install -r engine/.github/requirements/tools.txt
```
```bash
engine-venv/bin/pip install backports.zstd
```
`tools.txt` non lo contiene, ma lo script di build lo richiede con Python prima della 3.14 (vedi il comando di installazione nel README di nxnative). Il controllo «Moduli Python del motore» nella scheda Ambiente lo verifica.
nxnative **non ha release né tag**: il front end fissa un commit e lo aggiorna a mano. Il submodule git fa esattamente questo. Per aggiornare:
```bash
git -C engine fetch && git -C engine log --oneline HEAD..origin/main --grep='^Front-ends:'
```
Le righe `Front-ends: break|interface|new` indicano cosa cambia per il launcher.

### 3.3 Scaffold dell'app Tauri
```bash
pnpm create tauri-app
```
Risposte: nome `nx-launcher`, frontend **TypeScript**, package manager **pnpm**, framework **React**. Poi:
```bash
pnpm tauri add dialog && pnpm tauri add opener && pnpm tauri add store
```
Crate Rust in `src-tauri/Cargo.toml`: `serde`, `serde_json`, `tokio` (features `process`, `io-util`, `macros`), `plist`, `core-foundation`, `nix` (segnali), `anyhow`, `thiserror`.
UI: Tailwind + shadcn/ui.

### 3.4 Avvio in sviluppo
```bash
pnpm tauri dev
```

---

## 4. Architettura

```
┌────────────────── UI (React + TS dentro WKWebView) ──────────────────┐
│  Libreria (griglia)  ·  Dettaglio gioco  ·  Build  ·  Impostazioni  │
└──────────────┬──────────────────────────────────▲────────────────────┘
      invoke() │                                  │ Channel<Event> (stream)
┌──────────────▼──────────────────────────────────┴────────────────────┐
│                       Core Rust (src-tauri)                          │
│  library   builder   launcher_json   prefs   runner   engine         │
└────┬──────────┬────────────┬───────────┬─────────┬────────┬──────────┘
     │ scan     │ spawn      │ read      │ CFPrefs │ spawn  │ git / check
     ▼          ▼            ▼           ▼         ▼        ▼
  cartelle  build_ipa_    launcher    org.nxnative. Contents/  engine/
  giochi    from_nsp.py   .json       <titleID>    MacOS/
            (JSONL)                                nxnative
```

| Modulo Rust | Responsabilità |
|---|---|
| `library.rs` | `library.json` in `~/Library/Application Support/dev.nxlauncher.desktop/`; importa le `.app` esistenti (legge `Info.plist` → `DMTitleID`, nome, icona); aggiunge sorgenti (cartella NSP, `.nup`, dump). |
| `builder.rs` | Lancia Python con `--progress json`, legge stdout riga per riga, **converte ogni riga in un enum `BuildEvent`** e lo invia alla UI su un `Channel`. Cattura stderr. Annulla con SIGINT (exit 130). Una build alla volta. |
| `launcher_json.rs` | Tipi serde per lo schema 1 di `launcher.json`. Ignora i campi sconosciuti e accetta sezioni nuove. |
| `prefs.rs` | Legge e scrive `launcher.<ENV>` con `CFPreferencesSetAppValue` sul dominio `org.nxnative.<titleID>`. Rimuove la chiave quando il valore torna al default. |
| `runner.rs` | Avvia il gioco con `DM_LAUNCHER=0` e gli override; segue lo stato in esecuzione; traduce l'exit code in un messaggio; tiene il conto del tempo di gioco; apre il log. |
| `gamelog.rs` | Legge il log del gioco in esecuzione un pezzo al secondo (solo il file di questa sessione: il launcher del gioco rinomina il precedente in `.prev.log`) e ne ricava la fase, la finestra coperta e le shader mancanti; `runner.rs` lo manda alla UI con l'evento `game-status` solo quando cambia. |
| `engine.rs` | Controlli all'avvio (Python arm64, Xcode 27.2, `xcodegen`, `glslang`, `zstd`, spazio disco), commit fissato, flusso di aggiornamento. |
| `covers.rs` | Copertine da internet con `/usr/bin/curl`: per i giochi Switch (title ID `01…`) da Nlib per title ID (`api.nlib.cc/nx/<ID>/icon`, dati di blawar/titledb), altrimenti dalla ricerca dell'eShop europeo per nome (`searching.nintendo-europe.com`), presa solo se console e nome coincidono. Salvate in `icons/cover-<ID>.jpg|png`. |
| `archive.rs` | La cartella archivio: una sottocartella per gioco, `<nome> [<title ID>]`, con gioco base, update e DLC come li vuole lo script (un gioco per cartella; prima NSP/XCI, poi NUP, poi dump). Riconosce i pacchetti dai tag `[title ID][vN]` del nome, i NUP dal loro indice (intestazione e record dei pacchetti, nome dal NACP se c'è), i dump da `main.npdm` e dai nomi delle cartelle `updates/<versione>`, `dlc/<title ID>`. Segnala ciò che una build non prende (più giochi in una cartella, due NUP, file senza tag). |
| `importer.rs` | Porta i file nell'archivio da cartelle, file e archivi `.zip`, `.rar`, `.7z` (li apre `/usr/bin/tar`, cioè bsdtar). Gli archivi si estraggono in una cartella nascosta dentro l'archivio, così poi i file si spostano invece di copiarsi; sullo stesso volume la copia è un clone APFS, altrimenti passa da un nome `.part` nascosto. I pacchetti senza tag li legge `nx_extract --contents` con prod.keys, e da lì ricevono i tag. Gira su un thread suo, con avanzamento (`archive-import`) e Annulla. Un archivio svuotato senza errori si elimina solo se i file si spostano o se era già nell'archivio. |
| `titledb.rs` | `versions.txt` di blawar/titledb (circa 3 MB, scaricato intero, al massimo una volta al giorno): l'ultimo update e i DLC di ogni gioco, confrontati in locale con l'archivio. |

**Modello dati, una voce di libreria:**
```ts
type Game = {
  id: string;              // uuid locale
  titleId: string;         // DMTitleID, es. "0400C3F00006E000"
  name: string; version?: string;
  source?: { kind: "nsp" | "nup" | "dump"; path: string };
  appPath?: string;        // .app costruita
  ipaPath?: string;
  iconPath?: string;       // copia nostra: icons/<titleId>.jpg
  workDir?: string;        // --work-dir per questo gioco
  lastBuild?: { ok: boolean; at: string; engineCommit: string };
  lastPlayed?: string; playSeconds: number;
};
```

---

## 5. Roadmap per fasi

### Fase 0: setup (½ giornata) ✅
Sezione 3 completa. Traguardo: `pnpm tauri dev` apre una finestra vuota e il motore compila un gioco **da terminale**.

### Fase 1: libreria e avvio (MVP) ✅
Per provarla senza un gioco vero: `scripts/make-fake-library.sh` crea in `fixtures/fake-games/` tre app finte con la stessa struttura di quelle di nxnative, con le liste di impostazioni reali scritte da `settings.py` del motore. Una sola: `scripts/make-fake-game.sh`.

- Import di una `.app` già costruita (drag & drop o pulsante) → lettura di `Info.plist` → griglia con icona e nome.
- **Play** → spawn di `Contents/MacOS/nxnative`, badge "In gioco", exit code tradotto (`70` → "macOS ha revocato la GPU", `134` → "crash, apri il log").
- Mentre il gioco gira, la scheda dice cosa sta facendo, letto dal suo log (`gamelog.rs`): «Avvio del gioco…», «Caricamento…», «Preparazione della grafica…» con un orologio (e la nota sul primo avvio su questo Mac, il più lungo), «Finestra coperta» e le shader che l'app non ha. Per provarlo: `FAKE_BOOT=10 scripts/make-fake-game.sh <cartella> <title ID> <nome> 0 30` crea un gioco finto che scrive le stesse righe del motore, con 10 secondi di preparazione al primo avvio.
- "Mostra log", "Mostra salvataggi", "Mostra nel Finder".
- **Traguardo:** si avvia dal launcher un gioco costruito da CLI.

### Fase 2: impostazioni per gioco ✅
- Lettura di `launcher.json` → form dinamico: `choice` → select, `range` → slider (con `base` mostra la risoluzione, es. 0.75 → 1200×675), `toggle` → switch.
- Prima le righe `level: "basic"`, le altre sotto "Avanzate". Le righe con `needs` si vedono solo quando la loro condizione è vera.
- Salvataggio solo dei valori diversi dal default; pulsante "Ripristina default testati".
- "Solo per questa partita": override nell'ambiente, senza scrivere le preferenze.
- **Traguardo:** si cambia il frame rate dal launcher e il gioco lo applica, anche se poi lo apri dal Finder.

### Fase 3: build dall'interfaccia ✅
Per provarla senza un gioco: avvia l'app con `open --env NXL_BUILD_SCRIPT=$PWD/scripts/fake-build.py "src-tauri/target/debug/bundle/macos/NX Launcher.app"`. `scripts/fake-build.py` riproduce gli eventi di una build vera (nomi delle fasi reali, `game`, `warn`, Annulla con exit 130) e alla fine crea un'app finta; una sorgente con "fail" nel nome si ferma con un errore. Senza la variabile si usa lo script vero di nxnative.
- Aggiungi gioco: cartella o `.nup`; selettore per `prod.keys` (**si salva solo il percorso**, il file non viene mai copiato).
- Controllo dello spazio prima di partire.
- Vista build: barra per stage (`k`/`n`, dove `n` può crescere), barra di `progress` (indeterminata quando `frac` è `null`), ETA, log dei `say` (rossi quelli `[RED] `), pulsante Annulla.
- Evento `game` → **copia subito l'icona** (in una build IPA senza `--keep-extracted` l'originale viene cancellato a fine build).
- Fine build: exit code + stderr; con `0` aggiorna `appPath` e mette il gioco in libreria.
- **Traguardo:** da `.nup` a gioco avviabile senza aprire il terminale.

### Fase 4: extra ✅
Tab **Impostazioni** (cloud save, messaggi in italiano, prod.keys), pagina **Cheat…** per gioco, build **IPA** nella scheda Compila, ricerca e ordinamento in libreria. `scripts/make-fake-library.sh` crea anche un file di cheat d'esempio in `fixtures/cheats/`, da importare nella pagina Cheat… di «Avventura di Prova».
- Cartella cloud save unica per tutti i giochi (`cloudSaves.folder` in ogni dominio).
- Gestione cheat: import di `<buildID>.txt` e spunte in `.enabled`, scritte **prima** dell'avvio.
- Prompt Home in italiano tramite `DM_HOME_QUIT_TEXT`.
- Export IPA (con `--romfs separate` per i giochi grandi).
- Statistiche di gioco, ultimo giocato, ordinamento e filtri.
- **Copertine** (`covers.rs`): all'avvio del launcher, dopo un import e dopo una build, per i giochi che non ne hanno una. Prima Nlib per title ID (solo Switch: titledb conosce appena un centinaio di giochi Switch 2), poi l'eShop europeo per nome, filtrato per console (`nintendoswitch…` / `nintendoswitch2`) e con il nome che deve coincidere (o finire in «Nintendo Switch 2 Edition»): meglio nessuna copertina che quella sbagliata. Un gioco non trovato si richiede dopo una settimana, o subito con «Cerca la copertina». La copertina passa anche alla build come `--fallback-icon`: un `.nup` o un dump senza icona propria la usa come icona dell'app. Spegnibili in Impostazioni. Test dal vivo: `cargo test covers -- --ignored`.

### Fase 5: aggiornamento del motore ✅
Nella scheda **Ambiente**: «Cerca aggiornamenti», le note `Front-ends:`, cosa cambia per il launcher, «Aggiorna» e «Torna alla versione precedente». Il controllo del compilatore Metal legge il requisito da `dm/gfx/tools/dmsh_chain.lock` del motore. Dopo un aggiornamento, `git add engine && git commit` fissa la nuova versione in questo repo.
- Pulsante "Aggiorna nxnative": fetch → mostra le righe `Front-ends:` e il diff di `docs/launcher.schema.json` → conferma → checkout.
- I giochi la cui ultima build usava un commit diverso ricevono il badge "Ricompila consigliato".

### Fase 6: archivio ✅
Scheda **Archivio**: la cartella si sceglie lì o in Impostazioni. «Importa file…», «Importa cartella…» o trascinamento sulla scheda; «Sposta i file invece di copiarli»; «Metti in ordine» per i file lasciati sciolti nella cartella archivio. Per ogni gioco: formato, dimensione, gioco base, update presente e ultimo esistente, DLC presenti e mancanti (da titledb), problemi per la build, «Compila» (apre la scheda Compila con la sua cartella) e «Finder». Per gli update e i DLC mancanti il launcher indica solo che cosa manca: si copiano dalla propria console (DBI, nxdumptool) e si importano.
Per provarla senza giochi: `scripts/make-fake-archive.sh` crea `fixtures/fake-archive/` (da scegliere come archivio) e `fixtures/archive-inbox/` (zip, 7z con un NUP, un dump, un file senza tag) con file segnaposto di pochi byte. Test dal vivo dell'elenco: `cargo test titledb -- --ignored`.

---

## 6. Insidie note (dal contratto e dall'ambiente)

1. **PATH:** un'app aperta dal Finder **non eredita il PATH della shell**, quindi `xcodegen`, `glslang` e `zstd` (in `/opt/homebrew/bin`) non vengono trovati. Quando lanci Python, imposta esplicitamente `PATH=/opt/homebrew/bin:/usr/bin:/bin:…`.
2. **Niente App Sandbox:** un'app sandboxed non può scrivere nel dominio preferenze `org.nxnative.*`. Va distribuita fuori dal Mac App Store (firma ad-hoc o Developer ID).
3. **Python arm64:** sotto Rosetta lo script si ferma. Controllalo all'avvio con `platform.machine()`.
4. **Exit code 2 = nessun JSON:** cattura sempre stderr, altrimenti l'errore si perde.
5. **I log del gioco in esecuzione non sono un'API:** cambiano formulazione senza avviso. Il launcher ne legge poche righe solo per mostrare cosa sta facendo il gioco (`gamelog.rs`, con il file del motore da cui viene ogni riga), mai per decidere qualcosa. Se una riga cambia, lo stato si limita a mostrare meno: «Avvio» e «Caricamento» passano comunque a «In gioco» dopo 5 minuti e 60 secondi. Dopo un aggiornamento del motore conviene ricontrollare quelle stringhe.
6. **Cheat:** l'host li legge solo all'avvio o con ⌘R. Si scrivono prima del Play.
7. **Chiavi e giochi:** il launcher non include, non copia e non invia mai `prod.keys` né file di gioco. Usa solo i percorsi scelti dall'utente.
8. **Archivi divisi in più parti** (`.part1.rar`, `.7z.001`, `.r00`): bsdtar non li apre. Il launcher lo dice e chiede di estrarli con Keka o The Unarchiver. Lo stesso per gli archivi con password.
9. **titledb non è un'API ufficiale e conosce pochi giochi Switch 2:** un gioco che non conosce appare come «titledb non conosce il gioco», mai come completo. Se l'elenco non si scarica, resta quello del giorno prima.
10. **Le fonti delle copertine non sono API ufficiali:** Nlib è un servizio della community e la ricerca dell'eShop europeo non è documentata. Se una cambia, il gioco resta senza copertina: niente si rompe, e i test `covers` (incluso quello dal vivo) dicono quale delle due ha smesso di rispondere. Ricevono solo il title ID (Nlib) o il nome del gioco (Nintendo).

---

## 7. Struttura del repo

```
nxnative/                  ← questa cartella = progetto launcher
├─ PLAN.md
├─ engine/                 ← submodule seedds/nxnative @ commit fissato
├─ engine-venv/            ← venv Python del motore (in .gitignore)
├─ src/                    ← UI React
│  ├─ routes/  Library.tsx · GameDetail.tsx · BuildView.tsx · Settings.tsx
│  ├─ components/SettingsForm.tsx   ← disegna launcher.json
│  └─ lib/ipc.ts                    ← wrapper tipizzati di invoke/Channel
└─ src-tauri/
   ├─ tauri.conf.json
   └─ src/
      ├─ lib.rs  library.rs  builder.rs  launcher_json.rs
      ├─ archive.rs  importer.rs  titledb.rs
      └─ prefs.rs  runner.rs  engine.rs
```

> Consiglio: rinomina la cartella in `nx-launcher` per non confonderla con il motore (`engine/`).

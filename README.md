# NX Launcher

Un launcher per Mac con Apple silicon per i giochi costruiti con [nxnative](https://github.com/seedds/nxnative): libreria, build dall'interfaccia, impostazioni per gioco, cheat, salvataggi in cloud, un archivio dei file dei propri giochi con gli update e i DLC che mancano.

Il launcher non contiene né scarica giochi, chiavi o file di sistema: lavora solo con i file che hai copiato dalla tua console.

## Installazione

1. Dalla [pagina delle release](https://github.com/bizzul/nx-launcher/releases/latest) scarica `NX-Launcher.app.tar.gz`, aprilo e sposta `NX Launcher.app` in Applicazioni.
2. L'app non è firmata da Apple: la prima volta macOS la blocca. Toglile la quarantena:
   ```bash
   xattr -dr com.apple.quarantine "/Applications/NX Launcher.app"
   ```
3. Nella scheda **Ambiente**, «Installa il motore» scarica nxnative e prepara il suo ambiente Python (circa 1 GB). Servono anche Xcode e alcuni strumenti di Homebrew: la stessa scheda dice cosa manca e come installarlo.

Da lì l'app si aggiorna da sola, e aggiorna il motore: Impostazioni › Aggiornamenti.

## Sviluppo

Tauri 2 (Rust) con un'interfaccia React. Setup, architettura e decisioni sono in [PLAN.md](PLAN.md).

```bash
git clone --recurse-submodules https://github.com/bizzul/nx-launcher.git
```

```bash
pnpm install && pnpm tauri dev
```

La versione di sviluppo usa il motore del sottomodulo `engine/` e il venv `engine-venv/` (PLAN.md, 3.2). Le versioni si pubblicano con `scripts/release.sh`.

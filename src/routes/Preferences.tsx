import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { KeysRow } from "../components/KeysRow";
import {
  configGet,
  configSetArchive,
  configSetCloudFolder,
  configSetCovers,
  configSetItalianPrompts,
  configSetKeys,
  configSetTitledb,
  type Config,
} from "../lib/ipc";

/** The launcher's own settings: for every game, not one. */
export default function Preferences({ gameCount, onError }: { gameCount: number; onError: (m: string) => void }) {
  const [config, setConfig] = useState<Config | null>(null);
  const [saved, setSaved] = useState<string | null>(null);

  useEffect(() => {
    configGet().then(setConfig, (e) => onError(String(e)));
  }, [onError]);

  const apply = async (fn: () => Promise<Config>, message?: string) => {
    try {
      setConfig(await fn());
      if (message) {
        setSaved(message);
        setTimeout(() => setSaved(null), 3000);
      }
    } catch (e) {
      onError(String(e));
    }
  };

  const pickCloud = async () => {
    const path = await open({ directory: true, title: "Cartella per i salvataggi in cloud" });
    if (typeof path === "string") apply(() => configSetCloudFolder(path), applied(gameCount));
  };

  const pickArchive = async () => {
    const path = await open({ directory: true, title: "Cartella archivio: una sottocartella per ogni gioco" });
    if (typeof path === "string") apply(() => configSetArchive(path), "Cartella archivio impostata");
  };

  const pickKeys = async () => {
    const path = await open({ title: "Scegli il tuo prod.keys", filters: [{ name: "prod.keys", extensions: ["keys"] }] });
    if (typeof path === "string") apply(() => configSetKeys(path));
  };

  if (!config) return <div className="flex-1" />;

  return (
    <div className="flex-1 overflow-y-auto">
      <div className="mx-auto max-w-3xl px-6 py-8">
        <div className="flex items-baseline justify-between">
          <h2 className="text-lg font-semibold">Impostazioni</h2>
          {saved && <span className="text-sm text-emerald-600 dark:text-emerald-400">✓ {saved}</span>}
        </div>

        <section className="mt-6 rounded-xl border border-neutral-200 p-5 dark:border-neutral-800">
          <h3 className="font-medium">Salvataggi in cloud</h3>
          <p className="mt-1 text-sm text-neutral-500">
            Una cartella per tutti i giochi: ogni gioco ci tiene il suo salvataggio e lo sincronizza a ogni avvio,
            anche se lo apri dal Finder. Se il salvataggio è cambiato da entrambe le parti, vince il più recente e
            l'altro resta in una copia di backup.
          </p>
          <p className="mt-3 truncate font-mono text-xs" title={config.cloudSavesFolder ?? ""}>
            {config.cloudSavesFolder ?? <span className="text-neutral-500">disattivati</span>}
          </p>
          <div className="mt-3 flex flex-wrap gap-2">
            <button onClick={() => apply(() => configSetCloudFolder("icloud"), applied(gameCount))} className="btn">
              Usa iCloud Drive
            </button>
            <button onClick={pickCloud} className="btn">
              Scegli cartella…
            </button>
            {config.cloudSavesFolder && (
              <button
                onClick={() => apply(() => configSetCloudFolder(null), "Salvataggi in cloud disattivati")}
                className="btn"
              >
                Disattiva
              </button>
            )}
          </div>
        </section>

        <section className="mt-4 flex items-start gap-4 rounded-xl border border-neutral-200 p-5 dark:border-neutral-800">
          <div className="flex-1">
            <h3 className="font-medium">Messaggi del gioco in italiano</h3>
            <p className="mt-1 text-sm text-neutral-500">
              Il messaggio «Tieni premuto per uscire» del tasto Home e quello che chiede altri controller. Vale per
              le partite avviate da qui.
            </p>
          </div>
          <Switch on={config.italianPrompts} onToggle={() => apply(() => configSetItalianPrompts(!config.italianPrompts))} />
        </section>

        <section className="mt-4 flex items-start gap-4 rounded-xl border border-neutral-200 p-5 dark:border-neutral-800">
          <div className="flex-1">
            <h3 className="font-medium">Copertine da internet</h3>
            <p className="mt-1 text-sm text-neutral-500">
              Cerca la copertina dei giochi in libreria: per i giochi Switch il title ID va a api.nlib.cc (dati di
              titledb), per quelli Switch 2, o se lì non c'è, il nome del gioco va alla ricerca dell'eShop europeo
              di Nintendo. Non parte nient'altro. Spente, le copertine già scaricate restano.
            </p>
          </div>
          <Switch on={config.covers} onToggle={() => apply(() => configSetCovers(!config.covers))} />
        </section>

        <section className="mt-4 rounded-xl border border-neutral-200 p-5 dark:border-neutral-800">
          <h3 className="font-medium">Cartella archivio</h3>
          <p className="mt-1 text-sm text-neutral-500">
            Dove tieni i file dei tuoi giochi, anche su un disco esterno: una sottocartella per gioco, con gioco base,
            update e DLC. La scheda Archivio importa lì file, cartelle, .zip, .rar e .7z.
          </p>
          <p className="mt-3 truncate font-mono text-xs" title={config.archiveFolder ?? ""}>
            {config.archiveFolder ?? <span className="text-neutral-500">non impostata</span>}
          </p>
          <div className="mt-3 flex flex-wrap gap-2">
            <button onClick={pickArchive} className="btn">
              {config.archiveFolder ? "Cambia…" : "Scegli cartella…"}
            </button>
            {config.archiveFolder && (
              <button onClick={() => apply(() => configSetArchive(null), "Cartella archivio dimenticata")} className="btn">
                Dimentica
              </button>
            )}
          </div>
        </section>

        <section className="mt-4 flex items-start gap-4 rounded-xl border border-neutral-200 p-5 dark:border-neutral-800">
          <div className="flex-1">
            <h3 className="font-medium">Update e DLC mancanti</h3>
            <p className="mt-1 text-sm text-neutral-500">
              Una volta al giorno scarica l'elenco pubblico di titledb (circa 3 MB, da GitHub) e lo confronta con
              l'archivio. Viene scaricato tutto l'elenco, quindi non parte niente sui tuoi giochi. Conosce pochi giochi
              Switch 2.
            </p>
          </div>
          <Switch on={config.titledb} onToggle={() => apply(() => configSetTitledb(!config.titledb))} />
        </section>

        <KeysRow keys={config.prodKeys} onPick={pickKeys} onClear={() => apply(() => configSetKeys(null))} />
      </div>
    </div>
  );
}

const applied = (n: number) => `Cartella applicata a ${n === 1 ? "1 gioco" : `${n} giochi`}`;

function Switch({ on, onToggle }: { on: boolean; onToggle: () => void }) {
  return (
    <button
      role="switch"
      aria-checked={on}
      onClick={onToggle}
      className={`relative mt-1 h-6 w-10 shrink-0 rounded-full transition-colors ${
        on ? "bg-sky-500" : "bg-neutral-300 dark:bg-neutral-700"
      }`}
    >
      <span
        className={`absolute top-0.5 left-0.5 size-5 rounded-full bg-white shadow transition-transform ${
          on ? "translate-x-4" : ""
        }`}
      />
    </button>
  );
}

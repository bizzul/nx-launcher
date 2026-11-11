import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { KeysRow } from "../components/KeysRow";
import { configGet, configSetCloudFolder, configSetItalianPrompts, configSetKeys, type Config } from "../lib/ipc";

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
          <button
            role="switch"
            aria-checked={config.italianPrompts}
            onClick={() => apply(() => configSetItalianPrompts(!config.italianPrompts))}
            className={`relative mt-1 h-6 w-10 shrink-0 rounded-full transition-colors ${
              config.italianPrompts ? "bg-sky-500" : "bg-neutral-300 dark:bg-neutral-700"
            }`}
          >
            <span
              className={`absolute top-0.5 left-0.5 size-5 rounded-full bg-white shadow transition-transform ${
                config.italianPrompts ? "translate-x-4" : ""
              }`}
            />
          </button>
        </section>

        <KeysRow keys={config.prodKeys} onPick={pickKeys} onClear={() => apply(() => configSetKeys(null))} />
      </div>
    </div>
  );
}

const applied = (n: number) => `Cartella applicata a ${n === 1 ? "1 gioco" : `${n} giochi`}`;

import { useEffect, useState } from "react";
import {
  appInfo,
  appRestart,
  appUpdateCheck,
  appUpdateInstall,
  configSetAutoUpdate,
  onAppUpdateProgress,
  type AppInfo,
  type AppUpdate,
  type Config,
} from "../lib/ipc";

/** The launcher's version, its updates and the engine's, in Impostazioni. */
export function UpdatesSection({
  config,
  onConfig,
  onError,
}: {
  config: Config;
  onConfig: (c: Config) => void;
  onError: (m: string) => void;
}) {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [found, setFound] = useState<AppUpdate | null | undefined>(undefined);
  const [busy, setBusy] = useState<"check" | "install" | null>(null);
  const [progress, setProgress] = useState<[number, number | null] | null>(null);

  useEffect(() => {
    appInfo().then(setInfo, () => {});
    const unlisten = onAppUpdateProgress((done, total) => setProgress([done, total]));
    return () => {
      unlisten.then((off) => off());
    };
  }, []);

  const check = async () => {
    setBusy("check");
    try {
      setFound(await appUpdateCheck());
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const install = async () => {
    setBusy("install");
    try {
      const version = await appUpdateInstall();
      setInfo((i) => (i ? { ...i, ready: version } : i));
      setFound(null);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
      setProgress(null);
    }
  };

  const toggle = async (app: boolean, engine: boolean) => {
    try {
      onConfig(await configSetAutoUpdate(app, engine));
    } catch (e) {
      onError(String(e));
    }
  };

  if (!info) return null;
  const pct = progress && progress[1] ? Math.round((progress[0] / progress[1]) * 100) : null;

  return (
    <section className="mt-4 rounded-xl border border-neutral-200 p-5 dark:border-neutral-800">
      <div className="flex items-baseline justify-between gap-4">
        <h3 className="font-medium">Aggiornamenti</h3>
        <span className="font-mono text-xs text-neutral-500">NX Launcher {info.version}</span>
      </div>

      {info.ready ? (
        <div className="mt-3 flex items-center gap-3 rounded-lg bg-emerald-500/10 p-3 text-sm text-emerald-800 dark:text-emerald-200">
          <p className="flex-1">La versione {info.ready} è installata: parte al prossimo avvio.</p>
          <button onClick={() => appRestart().catch((e) => onError(String(e)))} className="btn">
            Riavvia ora
          </button>
        </div>
      ) : (
        <div className="mt-3 flex flex-wrap items-center gap-3 text-sm">
          <button onClick={check} disabled={busy !== null} className="btn">
            {busy === "check" ? "Controllo…" : "Cerca aggiornamenti"}
          </button>
          {found === null && <span className="text-neutral-500">Hai già l'ultima versione</span>}
          {found && (
            <>
              <span>Versione {found.version} disponibile</span>
              <button onClick={install} disabled={busy !== null} className="btn btn-primary">
                {busy === "install" ? (pct !== null ? `Scarico… ${pct}%` : "Scarico…") : "Installa"}
              </button>
            </>
          )}
        </div>
      )}
      {found?.notes && <p className="mt-2 text-xs whitespace-pre-line text-neutral-500">{found.notes}</p>}

      <label className="mt-4 flex items-start gap-3 text-sm">
        <input
          type="checkbox"
          className="mt-1"
          checked={config.appAutoUpdate}
          onChange={(e) => toggle(e.target.checked, config.engineAutoUpdate)}
        />
        <span>
          Aggiorna il launcher da solo
          <span className="block text-xs text-neutral-500">
            All'avvio e una volta al giorno controlla le versioni pubblicate su github.com/bizzul/nx-launcher, scarica
            quella nuova, ne verifica la firma e la installa: parte al riavvio.
          </span>
        </span>
      </label>
      <label className="mt-3 flex items-start gap-3 text-sm">
        <input
          type="checkbox"
          className="mt-1"
          checked={config.engineAutoUpdate}
          onChange={(e) => toggle(config.appAutoUpdate, e.target.checked)}
        />
        <span>
          Aggiorna il motore da solo
          <span className="block text-xs text-neutral-500">
            Segue nxnative, mai durante una build o una partita. Gli aggiornamenti che cambiano qualcosa per il
            launcher, o che vogliono un altro Xcode, li segnala soltanto: li applichi in Ambiente.
          </span>
        </span>
      </label>
      {!info.automatic && (
        <p className="mt-3 text-xs text-neutral-500">
          Questa è la versione di sviluppo: si aggiorna con git nella cartella del progetto, e usa il motore di lì.
        </p>
      )}
    </section>
  );
}

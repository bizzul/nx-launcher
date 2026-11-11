import { useEffect, useRef, useState } from "react";
import { engineInstall, engineInstallStatus, onEngineInstall, type InstallSnapshot } from "../lib/ipc";

/** The installed app's first step: nxnative and its Python venv in the launcher's data folder. */
export function EngineInstall({ root, onInstalled, onError }: { root: string; onInstalled: () => void; onError: (m: string) => void }) {
  const [snap, setSnap] = useState<InstallSnapshot | null>(null);
  const logRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    engineInstallStatus().then(setSnap, () => {});
    const unlisten = onEngineInstall((s) => {
      setSnap(s);
      if (!s.running && !s.error) onInstalled();
    });
    return () => {
      unlisten.then((off) => off());
    };
  }, [onInstalled]);

  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, [snap?.log.length]);

  const start = async () => {
    try {
      setSnap(await engineInstall());
    } catch (e) {
      onError(String(e));
    }
  };

  const running = snap?.running ?? false;
  return (
    <section className="mb-8 rounded-xl border border-sky-300 bg-sky-50/60 p-5 dark:border-sky-900 dark:bg-sky-950/30">
      <h3 className="font-medium">Installa il motore</h3>
      <p className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
        Serve per compilare i giochi (giocare a quelli già pronti funziona anche senza). Il launcher scarica nxnative e
        prepara il suo ambiente Python nella propria cartella, circa 1 GB; poi lo tiene aggiornato da solo.
      </p>
      <p className="mt-2 truncate font-mono text-xs text-neutral-500" title={root}>
        {root}
      </p>

      {snap?.error && !running && (
        <p className="mt-3 rounded-lg bg-red-500/10 p-3 text-sm text-red-700 dark:text-red-300">{snap.error}</p>
      )}

      {running ? (
        <div className="mt-4">
          <p className="truncate text-sm">{snap?.step}</p>
          <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-neutral-200 dark:bg-neutral-800">
            {snap?.frac == null ? (
              <div className="indeterminate h-full w-1/3 rounded-full bg-sky-500" />
            ) : (
              <div className="h-full rounded-full bg-sky-500 transition-[width]" style={{ width: `${snap.frac * 100}%` }} />
            )}
          </div>
        </div>
      ) : (
        <button onClick={start} className="btn btn-primary mt-4">
          {snap?.error ? "Riprova" : "Installa il motore"}
        </button>
      )}

      {snap && snap.log.length > 0 && (
        <div ref={logRef} className="mt-3 max-h-40 overflow-y-auto font-mono text-[11px] leading-relaxed text-neutral-500">
          {snap.log.map((l, i) => (
            <p key={i}>{l}</p>
          ))}
        </div>
      )}
    </section>
  );
}

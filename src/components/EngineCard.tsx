import { useCallback, useEffect, useState } from "react";
import { engineApply, engineCheck, engineInfo, type Applied, type EngineState, type UpdateInfo } from "../lib/ipc";

const NOTE_KIND = {
  break: ["Rottura", "bg-red-500/15 text-red-700 dark:text-red-300"],
  interface: ["Interfaccia", "bg-amber-500/15 text-amber-700 dark:text-amber-300"],
  new: ["Novità", "bg-sky-500/15 text-sky-700 dark:text-sky-300"],
  note: ["Nota", "bg-neutral-500/15 text-neutral-600 dark:text-neutral-300"],
} as const;

/** The engine's version, and moving it forward (or back) with what changes. */
export function EngineCard({ onError, onChanged }: { onError: (m: string) => void; onChanged: () => void }) {
  const [info, setInfo] = useState<EngineState | null>(null);
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [busy, setBusy] = useState<"check" | "apply" | null>(null);
  const [applied, setApplied] = useState<Applied | null>(null);
  const [confirmRollback, setConfirmRollback] = useState(false);

  const load = useCallback(() => engineInfo().then(setInfo, (e) => onError(String(e))), [onError]);
  useEffect(() => {
    load();
  }, [load]);

  const check = async () => {
    setBusy("check");
    setApplied(null);
    try {
      setUpdate(await engineCheck());
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const apply = async (rollback: boolean) => {
    setBusy("apply");
    setConfirmRollback(false);
    try {
      setApplied(await engineApply(rollback));
      setUpdate(null);
      await load();
      onChanged();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
    }
  };

  if (!info) return null;

  return (
    <section className="mb-8 rounded-xl border border-neutral-200 p-5 dark:border-neutral-800">
      <div className="flex items-start gap-4">
        <div className="min-w-0 flex-1">
          <h3 className="font-medium">Motore nxnative</h3>
          <p className="mt-1 text-sm">
            <span className="font-mono">{info.short}</span> · {info.date}
          </p>
          <p className="mt-0.5 truncate text-xs text-neutral-500" title={info.subject}>
            {info.subject}
          </p>
        </div>
        <button onClick={check} disabled={busy !== null} className="btn">
          {busy === "check" ? "Controllo…" : "Cerca aggiornamenti"}
        </button>
      </div>

      {info.modified && (
        <p className="mt-3 rounded-lg bg-amber-500/10 p-3 text-xs text-amber-700 dark:text-amber-300">
          Il motore ha modifiche locali nei suoi file: il launcher non lo aggiorna finché ci sono.
        </p>
      )}

      {applied && (
        <div className="mt-4 rounded-lg bg-emerald-500/10 p-3 text-sm text-emerald-800 dark:text-emerald-200">
          <p className="font-medium">
            Ora alla versione <span className="font-mono">{applied.short}</span> ({applied.date}).
          </p>
          {applied.pip && <p className="mt-1 text-xs whitespace-pre-wrap">{applied.pip}</p>}
          <p className="mt-1 text-xs">
            Per fissarla nel repository del launcher: <code className="font-mono">git add engine && git commit</code>.
            I giochi da ricompilare sono segnati in libreria.
          </p>
        </div>
      )}

      {update && update.upToDate && <p className="mt-4 text-sm text-neutral-500">È già l'ultima versione.</p>}

      {update && !update.upToDate && (
        <div className="mt-4 space-y-4 text-sm">
          <p>
            <span className="font-medium">{update.totalCommits} modifiche nuove</span> fino a{" "}
            <span className="font-mono">{update.toShort}</span>.
          </p>
          {update.diverged && (
            <p className="rounded-lg bg-amber-500/10 p-3 text-xs text-amber-700 dark:text-amber-300">
              La versione attuale non è sul ramo main: aggiornando, i suoi commit non sarebbero più in uso.
            </p>
          )}
          {update.metalNeeded && (
            <p className="rounded-lg bg-red-500/10 p-3 text-xs text-red-700 dark:text-red-300">
              La nuova versione richiede un altro compilatore Metal ({update.metalNeeded}): dopo l'aggiornamento le
              build si fermano finché non installi e selezioni l'Xcode che lo contiene.
            </p>
          )}

          {update.notes.length > 0 && (
            <div>
              <h4 className="mb-2 text-xs font-semibold tracking-wide text-neutral-500 uppercase">
                Note per i front end
              </h4>
              <ul className="space-y-2">
                {update.notes.map((n, i) => {
                  const [label, tone] = NOTE_KIND[n.kind] ?? NOTE_KIND.note;
                  return (
                    <li key={i} className="flex gap-2">
                      <span className={`h-fit shrink-0 rounded px-1.5 py-0.5 text-[11px] font-medium ${tone}`}>{label}</span>
                      <span className="text-xs leading-relaxed">
                        {n.text} <span className="font-mono text-neutral-500">({n.commit})</span>
                      </span>
                    </li>
                  );
                })}
              </ul>
            </div>
          )}

          <div>
            <h4 className="mb-2 text-xs font-semibold tracking-wide text-neutral-500 uppercase">
              Cosa cambia per il launcher
            </h4>
            <ul className="list-disc space-y-1 pl-5 text-xs">
              {update.shaderChain && <li>Nuova catena di shader: ogni gioco le ricostruisce alla prossima build.</li>}
              {update.requirements && <li>Le dipendenze Python cambiano: verranno reinstallate.</li>}
              {update.settingsList && <li>La lista delle impostazioni dei giochi cambia (valida dalla prossima build).</li>}
              {update.envSwitches && <li>Cambiano le variabili d'ambiente del motore.</li>}
              {update.frontendsDoc && <li>Cambia la guida per i front end (docs/frontends.md).</li>}
              {update.scriptOptions.length > 0 && (
                <li>
                  Opzioni dello script di build:
                  <pre className="mt-1 overflow-x-auto font-mono text-[11px] leading-relaxed">
                    {update.scriptOptions.join("\n")}
                  </pre>
                </li>
              )}
              {update.schemaDiff && (
                <li>
                  <details>
                    <summary className="cursor-pointer">Lo schema di launcher.json cambia</summary>
                    <pre className="mt-1 max-h-60 overflow-auto rounded bg-neutral-100 p-2 font-mono text-[11px] dark:bg-neutral-900">
                      {update.schemaDiff}
                    </pre>
                  </details>
                </li>
              )}
              {!update.shaderChain &&
                !update.requirements &&
                !update.settingsList &&
                !update.envSwitches &&
                !update.frontendsDoc &&
                !update.scriptOptions.length &&
                !update.schemaDiff && <li>Niente che il launcher debba sapere: solo correzioni al motore.</li>}
            </ul>
          </div>

          <details>
            <summary className="cursor-pointer text-xs text-neutral-500">
              Elenco delle modifiche ({update.commits.length}
              {update.commits.length < update.totalCommits ? ` di ${update.totalCommits}` : ""})
            </summary>
            <ul className="mt-2 max-h-64 space-y-1 overflow-y-auto text-xs">
              {update.commits.map((c) => (
                <li key={c.short} className="flex gap-2">
                  <span className="shrink-0 font-mono text-neutral-500">{c.short}</span>
                  <span className="shrink-0 text-neutral-500">{c.date}</span>
                  <span className="truncate" title={c.subject}>
                    {c.subject}
                  </span>
                </li>
              ))}
            </ul>
          </details>

          <button onClick={() => apply(false)} disabled={busy !== null || info.modified} className="btn btn-primary">
            {busy === "apply" ? "Aggiornamento…" : `Aggiorna a ${update.toShort}`}
          </button>
        </div>
      )}

      {info.previous && !update && (
        <div className="mt-4 text-xs">
          {confirmRollback ? (
            <span className="flex items-center gap-2">
              Tornare a <span className="font-mono">{info.previous.slice(0, 8)}</span>?
              <button onClick={() => apply(true)} className="btn" disabled={busy !== null}>
                Torna indietro
              </button>
              <button onClick={() => setConfirmRollback(false)} className="btn">
                Annulla
              </button>
            </span>
          ) : (
            <button onClick={() => setConfirmRollback(true)} className="text-neutral-500 hover:underline">
              Torna alla versione precedente ({info.previous.slice(0, 8)})
            </button>
          )}
        </div>
      )}
    </section>
  );
}

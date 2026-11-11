import { useEffect, useState } from "react";
import { doctor, type DoctorReport } from "../lib/ipc";

/** Phase 0's environment check: can this Mac build and run nxnative games? */
export default function Doctor() {
  const [report, setReport] = useState<DoctorReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  async function runDoctor() {
    setRunning(true);
    setError(null);
    try {
      setReport(await doctor());
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  }

  useEffect(() => {
    runDoctor();
  }, []);

  const failed = report?.checks.filter((c) => !c.ok).length ?? 0;

  return (
    <div className="flex-1 overflow-y-auto">
      <div className="mx-auto max-w-3xl px-6 py-8">
        <div className="mb-6 flex items-end justify-between gap-4">
          <div>
            <h2 className="text-lg font-semibold">Ambiente</h2>
            <p className="mt-0.5 text-sm text-neutral-500">
              Quello che serve a nxnative per compilare e avviare i giochi
            </p>
          </div>
          <button onClick={runDoctor} disabled={running} className="btn">
            {running ? "Controllo…" : "Ricontrolla"}
          </button>
        </div>

        {error && (
          <p className="mb-4 rounded-md bg-red-500/10 p-3 text-sm text-red-600 dark:text-red-400">
            {error}
          </p>
        )}

        {report && (
          <>
            <p
              className={`mb-4 text-sm font-medium ${failed ? "text-amber-600 dark:text-amber-400" : "text-emerald-600 dark:text-emerald-400"}`}
            >
              {failed
                ? `${failed} ${failed === 1 ? "problema" : "problemi"} da risolvere`
                : "Tutto pronto per compilare e avviare i giochi"}
            </p>

            <ul className="divide-y divide-neutral-200 overflow-hidden rounded-lg border border-neutral-200 dark:divide-neutral-800 dark:border-neutral-800">
              {report.checks.map((c) => (
                <li key={c.id} className="flex gap-3 px-4 py-3">
                  <span
                    aria-label={c.ok ? "ok" : "da sistemare"}
                    className={`mt-1.5 size-2.5 shrink-0 rounded-full ${c.ok ? "bg-emerald-500" : "bg-red-500"}`}
                  />
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-medium">{c.label}</p>
                    <p className="truncate font-mono text-xs text-neutral-500" title={c.detail}>
                      {c.detail || "—"}
                    </p>
                    {c.fix && (
                      <code className="mt-2 block select-all rounded bg-neutral-100 px-2 py-1 font-mono text-xs dark:bg-neutral-900">
                        {c.fix}
                      </code>
                    )}
                  </div>
                </li>
              ))}
            </ul>

            <details className="mt-6 text-sm">
              <summary className="cursor-pointer text-neutral-500">PATH dei processi figli</summary>
              <ol className="mt-2 space-y-0.5 font-mono text-xs text-neutral-500">
                {report.childPath.split(":").map((dir, i) => (
                  <li key={i}>{dir || "(vuoto)"}</li>
                ))}
              </ol>
            </details>
          </>
        )}
      </div>
    </div>
  );
}

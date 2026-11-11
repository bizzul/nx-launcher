import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { GameIcon } from "../components/GameIcon";
import {
  cheatsGet,
  cheatsImport,
  cheatsMaster,
  cheatsReveal,
  cheatsSet,
  type CheatEntry,
  type CheatFile,
  type CheatsView,
  type Game,
} from "../lib/ipc";

type Props = { game: Game; onBack: () => void; onError: (m: string) => void };

/** The game's cheats before it starts: the same files its Cheats panel uses. */
export default function Cheats({ game, onBack, onError }: Props) {
  const [view, setView] = useState<CheatsView | null>(null);

  const run = async (fn: () => Promise<CheatsView>) => {
    try {
      setView(await fn());
    } catch (e) {
      onError(String(e));
    }
  };

  useEffect(() => {
    run(() => cheatsGet(game.titleId));
  }, [game.titleId]);

  const importFile = async () => {
    const path = await open({ title: "File di cheat (formato Atmosphère)", filters: [{ name: "Cheat", extensions: ["txt"] }] });
    if (typeof path === "string") run(() => cheatsImport(game.titleId, path));
  };

  const toggle = (file: CheatFile, key: string) => {
    const on = new Set(file.enabled);
    if (on.has(key)) on.delete(key);
    else on.add(key);
    run(() => cheatsSet(game.titleId, file.buildId, [...on]));
  };

  const current = view?.files.find((f) => f.current);
  const others = view?.files.filter((f) => !f.current) ?? [];
  const running = game.runningSince !== null;

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div className="flex items-center gap-4 border-b border-neutral-200 px-6 py-4 dark:border-neutral-800">
        <button onClick={onBack} className="text-sm text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-100">
          ← Libreria
        </button>
        <GameIcon game={game} className="w-10" />
        <div className="min-w-0">
          <h2 className="truncate font-semibold">{game.name}</h2>
          <p className="text-xs text-neutral-500">Cheat</p>
        </div>
        <div className="ml-auto flex gap-2">
          <button onClick={importFile} className="btn btn-primary">
            Importa file…
          </button>
          <button onClick={() => cheatsReveal(game.titleId).catch((e) => onError(String(e)))} className="btn">
            Apri cartella
          </button>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-2xl px-6 py-6">
          {!view ? null : (
            <>
              <p className="text-xs text-neutral-500">
                Versione dell'app: <span className="font-mono">{view.buildId ?? "build ID non leggibile"}</span>
                {" · "}
                {running
                  ? "il gioco è aperto: le modifiche valgono premendo ⌘R nella sua finestra, o dal prossimo avvio"
                  : "le modifiche valgono dal prossimo avvio"}
              </p>

              {view.safeMode && (
                <p className="mt-4 rounded-lg bg-amber-500/10 p-3 text-sm text-amber-700 dark:text-amber-300">
                  L'ultima partita si è chiusa con i cheat attivi: il gioco ripartirà con tutti i cheat spenti finché
                  non li ripristini dal pannello Cheat nella finestra del gioco.
                </p>
              )}

              {current ? (
                <>
                  <div className="mt-5 flex items-center justify-between rounded-lg border border-neutral-200 px-4 py-3 dark:border-neutral-800">
                    <div>
                      <p className="text-sm font-medium">Cheat attivi</p>
                      <p className="text-xs text-neutral-500">
                        Spento, nessun cheat gira; le scelte qui sotto restano salvate.
                      </p>
                    </div>
                    <Switch on={view.masterOn} onClick={() => run(() => cheatsMaster(game.titleId, !view.masterOn))} />
                  </div>
                  <CheatList file={current} dimmed={!view.masterOn} onToggle={(k) => toggle(current, k)} />
                </>
              ) : (
                <p className="mt-6 rounded-lg bg-neutral-100 p-4 text-sm text-neutral-600 dark:bg-neutral-900 dark:text-neutral-400">
                  Nessun file di cheat per questa versione del gioco. Importa un file <code>.txt</code> in formato
                  Atmosphère: se il suo nome non è un build ID, verrà salvato come{" "}
                  <code className="font-mono">{view.buildId ?? "<build ID>"}.txt</code>, la versione di quest'app.
                </p>
              )}

              {others.length > 0 && (
                <section className="mt-8">
                  <h3 className="mb-2 text-xs font-semibold tracking-wide text-neutral-500 uppercase">
                    File per altre versioni del gioco
                  </h3>
                  <ul className="space-y-1 text-sm text-neutral-500">
                    {others.map((f) => (
                      <li key={f.buildId} className="font-mono text-xs">
                        {f.buildId}.txt · {f.entries.filter((e) => e.kind === "cheat").length} cheat (non si caricano su
                        questa versione)
                      </li>
                    ))}
                  </ul>
                </section>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function CheatList({ file, dimmed, onToggle }: { file: CheatFile; dimmed: boolean; onToggle: (key: string) => void }) {
  if (file.error) return <p className="mt-4 text-sm text-red-600">{file.error}</p>;
  const on = new Set(file.enabled);
  // Rows under the section that holds them; the section markers themselves are not shown.
  const rows = file.entries.filter((e) => !e.name.startsWith("--Section"));
  const groups: [string | null, CheatEntry[]][] = [];
  for (const e of rows) {
    const last = groups[groups.length - 1];
    if (last && last[0] === e.section) last[1].push(e);
    else groups.push([e.section, [e]]);
  }
  return (
    <div className={`mt-4 ${dimmed ? "opacity-50" : ""}`}>
      {groups.map(([section, entries], i) => (
        <section key={i} className="mb-4">
          {section && (
            <h3 className="mb-1 text-xs font-semibold tracking-wide text-neutral-500 uppercase">{section}</h3>
          )}
          <ul className="divide-y divide-neutral-200 rounded-lg border border-neutral-200 dark:divide-neutral-800 dark:border-neutral-800">
            {entries.map((e) => (
              <li key={e.key} className="flex items-center gap-3 px-4 py-2.5 text-sm">
                {e.kind === "label" ? (
                  <span className="text-xs text-neutral-500 italic">{e.name}</span>
                ) : (
                  <>
                    <span className="flex-1">
                      {e.key}
                      {e.kind === "master" && <span className="ml-2 text-xs text-neutral-500">master · sempre attivo</span>}
                    </span>
                    {e.kind === "cheat" && <Switch on={on.has(e.key)} onClick={() => onToggle(e.key)} />}
                  </>
                )}
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

function Switch({ on, onClick }: { on: boolean; onClick: () => void }) {
  return (
    <button
      role="switch"
      aria-checked={on}
      onClick={onClick}
      className={`relative h-6 w-10 shrink-0 rounded-full transition-colors ${on ? "bg-sky-500" : "bg-neutral-300 dark:bg-neutral-700"}`}
    >
      <span className={`absolute top-0.5 left-0.5 size-5 rounded-full bg-white shadow transition-transform ${on ? "translate-x-4" : ""}`} />
    </button>
  );
}

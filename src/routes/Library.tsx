import { useMemo, useState } from "react";
import { GameIcon } from "../components/GameIcon";
import { formatDuration, formatWhen } from "../lib/format";
import type { Game } from "../lib/ipc";

type Props = {
  games: Game[];
  selectedId: string | null;
  onSelect: (titleId: string) => void;
  onAdd: () => void;
};

type Sort = "name" | "recent" | "time" | "added";

const SORTS: Record<Sort, [string, (a: Game, b: Game) => number]> = {
  name: ["Nome", (a, b) => a.name.localeCompare(b.name, "it")],
  recent: ["Giocati di recente", (a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0)],
  time: ["Tempo di gioco", (a, b) => b.playSeconds - a.playSeconds],
  added: ["Aggiunti di recente", (a, b) => b.addedAt - a.addedAt],
};

/** A per-viewer convenience, so it lives in localStorage (never required). */
function storedSort(): Sort {
  try {
    const s = localStorage.getItem("library.sort");
    return s && s in SORTS ? (s as Sort) : "name";
  } catch {
    return "name";
  }
}

export default function Library({ games, selectedId, onSelect, onAdd }: Props) {
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>(storedSort);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return games
      .filter((g) => !q || g.name.toLowerCase().includes(q) || g.titleId.toLowerCase().includes(q))
      .sort(SORTS[sort][1]);
  }, [games, query, sort]);

  const total = games.reduce((s, g) => s + g.playSeconds, 0);

  if (games.length === 0) {
    return (
      <div className="grid flex-1 place-items-center p-8">
        <div className="max-w-sm text-center">
          <div className="mx-auto mb-4 grid size-14 place-items-center rounded-2xl border-2 border-dashed border-neutral-300 text-2xl text-neutral-400 dark:border-neutral-700">
            +
          </div>
          <h2 className="font-semibold">Nessun gioco ancora</h2>
          <p className="mt-1 text-sm text-neutral-500">
            Trascina qui le app create da nxnative con <code className="font-mono text-xs">--platform mac-app</code>,
            oppure sceglile dal disco. Per crearne una dai file del gioco usa la scheda Compila.
          </p>
          <button onClick={onAdd} className="btn btn-primary mt-5">
            Aggiungi gioco
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div className="flex items-center gap-3 px-6 pt-5">
        <input
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Cerca"
          className="w-56 rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-sm outline-none focus:border-sky-500 dark:border-neutral-700 dark:bg-neutral-900"
        />
        <select
          value={sort}
          onChange={(e) => {
            const s = e.target.value as Sort;
            setSort(s);
            try {
              localStorage.setItem("library.sort", s);
            } catch {
              /* no storage: the choice lasts until the window closes */
            }
          }}
          className="rounded-md border border-neutral-300 bg-white px-2 py-1.5 text-sm dark:border-neutral-700 dark:bg-neutral-900"
        >
          {Object.entries(SORTS).map(([k, [label]]) => (
            <option key={k} value={k}>
              {label}
            </option>
          ))}
        </select>
        <p className="ml-auto text-xs text-neutral-500">
          {games.length === 1 ? "1 gioco" : `${games.length} giochi`}
          {total > 0 && ` · ${formatDuration(total)} di gioco in tutto`}
        </p>
      </div>

      <div className="flex-1 overflow-y-auto p-6">
        {shown.length === 0 ? (
          <p className="text-sm text-neutral-500">Nessun gioco corrisponde a «{query}».</p>
        ) : (
          <ul className="grid grid-cols-[repeat(auto-fill,minmax(8.5rem,1fr))] gap-x-4 gap-y-6">
            {shown.map((g) => (
              <li key={g.titleId}>
                <button
                  onClick={() => onSelect(g.titleId)}
                  className={`group w-full rounded-xl p-2 text-left outline-none transition-colors focus-visible:ring-2 focus-visible:ring-sky-500 ${
                    g.titleId === selectedId
                      ? "bg-neutral-200/70 dark:bg-neutral-800/70"
                      : "hover:bg-neutral-100 dark:hover:bg-neutral-900"
                  }`}
                >
                  <GameIcon game={g} className="w-full shadow-sm" />
                  <p className="mt-2 truncate text-sm font-medium" title={g.name}>
                    {g.name}
                  </p>
                  <p className="truncate text-xs text-neutral-500">
                    {g.runningSince ? (
                      <span className="font-medium text-emerald-600 dark:text-emerald-400">● In gioco</span>
                    ) : sort === "time" && g.playSeconds ? (
                      formatDuration(g.playSeconds)
                    ) : g.lastPlayed ? (
                      formatWhen(g.lastPlayed)
                    ) : (
                      "Mai giocato"
                    )}
                  </p>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

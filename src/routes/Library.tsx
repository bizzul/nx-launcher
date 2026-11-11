import { GameIcon } from "../components/GameIcon";
import { formatWhen } from "../lib/format";
import type { Game } from "../lib/ipc";

type Props = {
  games: Game[];
  selectedId: string | null;
  onSelect: (titleId: string) => void;
  onAdd: () => void;
};

export default function Library({ games, selectedId, onSelect, onAdd }: Props) {
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
            oppure sceglile dal disco.
          </p>
          <button onClick={onAdd} className="btn btn-primary mt-5">
            Aggiungi gioco
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <ul className="grid grid-cols-[repeat(auto-fill,minmax(8.5rem,1fr))] gap-x-4 gap-y-6">
        {games.map((g) => (
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
    </div>
  );
}

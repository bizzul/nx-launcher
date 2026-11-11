import { useState } from "react";
import { GameIcon } from "../components/GameIcon";
import { useNow } from "../hooks/useLibrary";
import { exitMessage, formatDuration, formatWhen, type Tone } from "../lib/format";
import { gameLaunch, gameReveal, gameStop, libraryRemove, type Game } from "../lib/ipc";

type Props = {
  game: Game;
  onChanged: (game: Game) => void;
  onRemoved: () => void;
  onError: (message: string) => void;
};

const TONES: Record<Tone, string> = {
  neutral: "bg-neutral-100 text-neutral-600 dark:bg-neutral-900 dark:text-neutral-400",
  warn: "bg-amber-500/10 text-amber-700 dark:text-amber-300",
  error: "bg-red-500/10 text-red-700 dark:text-red-300",
};

export default function GameDetail({ game, onChanged, onRemoved, onError }: Props) {
  const [confirm, setConfirm] = useState<"stop" | "remove" | null>(null);
  const now = useNow(15_000);
  const running = game.runningSince !== null;
  const last = game.lastExit && exitMessage(game.lastExit);

  const act = (fn: () => Promise<unknown>) => async () => {
    setConfirm(null);
    try {
      await fn();
    } catch (e) {
      onError(String(e));
    }
  };

  const play = (settingsWindow: boolean) =>
    act(async () => onChanged(await gameLaunch(game.titleId, settingsWindow)));

  return (
    <aside className="flex w-80 shrink-0 flex-col overflow-y-auto border-l border-neutral-200 dark:border-neutral-800">
      <div className="p-6">
        <GameIcon game={game} className="w-28 shadow-md" />
        <h2 className="mt-4 text-lg leading-snug font-semibold">{game.name}</h2>
        <p className="mt-0.5 font-mono text-xs text-neutral-500">
          {game.version && `v${game.version} · `}
          {game.titleId}
        </p>

        {/* Play / stop */}
        <div className="mt-5">
          {!running ? (
            <>
              <button onClick={play(false)} className="btn btn-primary w-full py-2">
                ▶ Gioca
              </button>
              <button
                onClick={play(true)}
                className="mt-2 w-full text-center text-xs text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200"
              >
                Gioca con la finestra impostazioni del gioco
              </button>
            </>
          ) : confirm === "stop" ? (
            <div className="rounded-lg bg-red-500/10 p-3 text-sm">
              <p className="text-red-700 dark:text-red-300">
                Terminare il gioco? I progressi non salvati andranno persi.
              </p>
              <div className="mt-3 flex gap-2">
                <button onClick={act(() => gameStop(game.titleId))} className="btn btn-danger flex-1">
                  Termina
                </button>
                <button onClick={() => setConfirm(null)} className="btn flex-1">
                  Annulla
                </button>
              </div>
            </div>
          ) : (
            <>
              <p className="mb-2 text-sm font-medium text-emerald-600 dark:text-emerald-400">
                ● In gioco da {formatDuration(Math.max(0, now - game.runningSince!))}
              </p>
              <button onClick={() => setConfirm("stop")} className="btn w-full">
                ■ Termina
              </button>
            </>
          )}
        </div>

        {last && !running && (
          <p className={`mt-4 rounded-lg p-3 text-xs leading-relaxed ${TONES[last.tone]}`}>
            <span className="font-medium">Ultima sessione:</span> {last.text}
          </p>
        )}

        <dl className="mt-5 grid grid-cols-2 gap-3 text-sm">
          <div>
            <dt className="text-xs text-neutral-500">Tempo di gioco</dt>
            <dd>{game.playSeconds ? formatDuration(game.playSeconds) : "—"}</dd>
          </div>
          <div>
            <dt className="text-xs text-neutral-500">Ultima partita</dt>
            <dd>{game.lastPlayed ? formatWhen(game.lastPlayed) : "—"}</dd>
          </div>
        </dl>

        <div className="mt-5 flex flex-col gap-1 border-t border-neutral-200 pt-4 dark:border-neutral-800">
          <LinkButton onClick={act(() => gameReveal(game.titleId, "app"))}>Mostra nel Finder</LinkButton>
          <LinkButton onClick={act(() => gameReveal(game.titleId, "log"))}>Apri il log</LinkButton>
          <LinkButton onClick={act(() => gameReveal(game.titleId, "saves"))}>Apri i salvataggi</LinkButton>
        </div>
      </div>

      <div className="mt-auto border-t border-neutral-200 p-6 pt-4 dark:border-neutral-800">
        {confirm === "remove" ? (
          <div className="text-sm">
            <p className="text-neutral-600 dark:text-neutral-400">
              Togliere {game.name} dalla libreria? L'app e i salvataggi restano dove sono.
            </p>
            <div className="mt-3 flex gap-2">
              <button
                onClick={act(async () => {
                  await libraryRemove(game.titleId);
                  onRemoved();
                })}
                className="btn btn-danger flex-1"
              >
                Togli
              </button>
              <button onClick={() => setConfirm(null)} className="btn flex-1">
                Annulla
              </button>
            </div>
          </div>
        ) : (
          <button
            onClick={() => setConfirm("remove")}
            disabled={running}
            className="text-xs text-neutral-500 hover:text-red-600 disabled:opacity-40"
          >
            Togli dalla libreria
          </button>
        )}
      </div>
    </aside>
  );
}

function LinkButton({ onClick, children }: { onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      onClick={onClick}
      className="-mx-2 rounded-md px-2 py-1.5 text-left text-sm hover:bg-neutral-100 dark:hover:bg-neutral-900"
    >
      {children}
    </button>
  );
}

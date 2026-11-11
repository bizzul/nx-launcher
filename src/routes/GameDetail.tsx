import { useState } from "react";
import { GameIcon } from "../components/GameIcon";
import { useNow } from "../hooks/useLibrary";
import { exitMessage, formatClock, formatDuration, formatWhen, type Tone } from "../lib/format";
import {
  gameFindCover,
  gameLaunch,
  gameReveal,
  gameStop,
  libraryRemove,
  type Game,
  type GameStatus,
} from "../lib/ipc";

/** Where a cover came from, said under the title ID. */
const COVER_SOURCES: Record<NonNullable<Game["coverSource"]>, string> = {
  nlib: "titledb (Nlib)",
  nintendoEu: "Nintendo eShop",
};

type Props = {
  game: Game;
  /** Why rebuilding with the current engine is worth it (none: up to date or unknown). */
  rebuildHints?: string[];
  /** Builds again from the recorded source, with the last build's options. */
  onRebuild: (sourcePath: string, options: Game["buildOptions"]) => void;
  onOpenSettings: () => void;
  onOpenCheats: () => void;
  onChanged: (game: Game) => void;
  onRemoved: () => void;
  onError: (message: string) => void;
};

const TONES: Record<Tone, string> = {
  neutral: "bg-neutral-100 text-neutral-600 dark:bg-neutral-900 dark:text-neutral-400",
  warn: "bg-amber-500/10 text-amber-700 dark:text-amber-300",
  error: "bg-red-500/10 text-red-700 dark:text-red-300",
};

export default function GameDetail({
  game,
  rebuildHints,
  onRebuild,
  onOpenSettings,
  onOpenCheats,
  onChanged,
  onRemoved,
  onError,
}: Props) {
  const [confirm, setConfirm] = useState<"stop" | "remove" | null>(null);
  const [findingCover, setFindingCover] = useState(false);
  const running = game.runningSince !== null;
  /** Starting or preparing: its clock ticks every second. */
  const busy = running && !!game.status && game.status.phase !== "playing";
  const now = useNow(busy ? 1000 : 15_000);
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

  const findCover = act(async () => {
    setFindingCover(true);
    try {
      onChanged(await gameFindCover(game.titleId));
    } finally {
      setFindingCover(false);
    }
  });

  return (
    <aside className="flex w-80 shrink-0 flex-col overflow-y-auto border-l border-neutral-200 dark:border-neutral-800">
      <div className="p-6">
        <GameIcon game={game} className="w-28 shadow-md" />
        <h2 className="mt-4 text-lg leading-snug font-semibold">{game.name}</h2>
        <p className="mt-0.5 font-mono text-xs text-neutral-500">
          {game.version && `v${game.version} · `}
          {game.titleId}
        </p>
        {game.coverPath && game.coverSource && (
          <p className="mt-0.5 text-[11px] text-neutral-400">Copertina: {COVER_SOURCES[game.coverSource]}</p>
        )}

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
                {busy ? "● Avvio in corso" : `● In gioco da ${formatDuration(Math.max(0, now - game.runningSince!))}`}
              </p>
              {game.status && (
                <RunStatus
                  status={game.status}
                  since={game.runningSince!}
                  now={now}
                  seenShaders={!!game.buildOptions?.seenShaders}
                />
              )}
              <button onClick={() => setConfirm("stop")} className="btn w-full">
                ■ Termina
              </button>
            </>
          )}
        </div>

        {rebuildHints && rebuildHints.length > 0 && (
          <div className="mt-4 rounded-lg bg-amber-500/10 p-3 text-xs text-amber-800 dark:text-amber-200">
            <p className="font-medium">↻ Ricompila consigliato</p>
            <ul className="mt-1 list-disc pl-4">
              {rebuildHints.map((h) => (
                <li key={h}>{h}</li>
              ))}
            </ul>
            {game.sourcePath && !running && (
              <button onClick={() => onRebuild(game.sourcePath!, game.buildOptions)} className="btn mt-2 w-full">
                Ricompila
              </button>
            )}
          </div>
        )}

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
          <LinkButton onClick={onOpenSettings}>Impostazioni…</LinkButton>
          <LinkButton onClick={onOpenCheats}>Cheat…</LinkButton>
          {game.sourcePath && !running && (
            <LinkButton onClick={() => onRebuild(game.sourcePath!, game.buildOptions)}>
              Ricompila{game.buildOptions?.seenShaders ? " (aggiunge le shader mancanti)" : ""}…
            </LinkButton>
          )}
          {!game.coverPath && (
            <LinkButton onClick={findingCover ? () => {} : findCover}>
              {findingCover ? "Cerco la copertina…" : "Cerca la copertina"}
            </LinkButton>
          )}
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

/** What the running game is doing, from its log (gamelog.rs). */
function RunStatus({
  status,
  since,
  now,
  seenShaders,
}: {
  status: GameStatus;
  /** When the session started (Unix seconds). */
  since: number;
  now: number;
  /** Built with "Solo le shader usate": "Ricompila" adds the missing ones. */
  seenShaders: boolean;
}) {
  const rows: { tone: Tone; text: string; note?: string }[] = [];
  const clock = (from: number) => formatClock(Math.max(0, now - from));
  switch (status.phase) {
    case "starting":
      rows.push({ tone: "neutral", text: `Avvio del gioco… ${clock(since)}` });
      break;
    case "loading":
      rows.push({ tone: "neutral", text: `Caricamento… ${clock(since)}` });
      break;
    case "preparing":
      rows.push({
        tone: "neutral",
        text: `Preparazione della grafica… ${clock(status.prepSince ?? since)}`,
        note: status.firstLaunch
          ? "Primo avvio su questo Mac: di solito qualche minuto. Il lavoro resta salvato, i prossimi avvii sono più rapidi."
          : "Il gioco mostra una schermata di avanzamento finché non ha finito.",
      });
      break;
  }
  if (status.windowCovered) {
    rows.push({ tone: "warn", text: "Finestra coperta: macOS rallenta il gioco finché non torna visibile." });
  }
  const n = status.missingShaders;
  if (n > 0) {
    rows.push(
      seenShaders
        ? {
            tone: "neutral",
            text: `Il gioco ha chiesto ${n.toLocaleString("it")} shader che questa build non ha («Solo le shader usate»): «Ricompila», a gioco chiuso, le aggiunge.`,
          }
        : { tone: "warn", text: `${n.toLocaleString("it")} shader non si sono caricate: i dettagli sono nel log.` },
    );
  }
  if (!rows.length) return null;
  return (
    <div className="mb-3 flex flex-col gap-2">
      {rows.map((r) => (
        <p key={r.text.slice(0, 24)} className={`rounded-lg p-3 text-xs leading-relaxed ${TONES[r.tone]}`}>
          <span className={r.note ? "font-medium" : ""}>{r.text}</span>
          {r.note && <span className="mt-1 block">{r.note}</span>}
        </p>
      ))}
    </div>
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

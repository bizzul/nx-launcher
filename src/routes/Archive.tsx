import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { GameIcon } from "../components/GameIcon";
import { formatBytes, formatWhen } from "../lib/format";
import {
  archiveImport,
  archiveImportCancel,
  archiveImportDismiss,
  archiveImportStatus,
  archiveList,
  archiveReveal,
  configSetArchive,
  onArchiveChanged,
  onArchiveImport,
  titledbUpdate,
  type ArchiveGame,
  type ArchiveView,
  type Game,
  type ImportSnapshot,
} from "../lib/ipc";

type Props = {
  /** The library, for the games' covers and the "in libreria" mark. */
  library: Game[];
  /** Paths dropped on the window while this page is open. */
  dropped: string[] | null;
  onDropTaken: () => void;
  onBuild: (folder: string) => void;
  onError: (message: string) => void;
};

type Filter = "all" | "missing";

const FORMATS = { package: "NSP / XCI", nup: "NUP", dump: "Dump" };

/** What the import pickers offer: game files and the archives that hold them. */
const IMPORT_EXTENSIONS = ["nsp", "xci", "nsz", "xcz", "nup", "zip", "rar", "7z"];

function stored<T extends string>(key: string, fallback: T, allowed: readonly T[]): T {
  try {
    const v = localStorage.getItem(key) as T | null;
    return v && allowed.includes(v) ? v : fallback;
  } catch {
    return fallback;
  }
}

function store(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* no storage: the choice lasts until the window closes */
  }
}

const lacks = (g: ArchiveGame) =>
  (!g.hasBase && !g.standalone) || g.updateMissing || g.missingDlc.length > 0 || g.issues.length > 0;

/** The archive: one folder per game, with its base game, updates and DLC. */
export default function Archive({ library, dropped, onDropTaken, onBuild, onError }: Props) {
  const [view, setView] = useState<ArchiveView | null>(null);
  const [snap, setSnap] = useState<ImportSnapshot | null>(null);
  const [moving, setMoving] = useState(() => stored("archive.moving", "copy", ["copy", "move"] as const) === "move");
  const [filter, setFilter] = useState<Filter>(() => stored("archive.filter", "all", ["all", "missing"] as const));
  const [query, setQuery] = useState("");
  const [open_, setOpen] = useState<string | null>(null);
  const [titledbBusy, setTitledbBusy] = useState(false);
  const [titledbError, setTitledbError] = useState<string | null>(null);
  const triedTitledb = useRef(false);

  const reload = useCallback(async () => {
    try {
      setView(await archiveList());
    } catch (e) {
      onError(String(e));
    }
  }, [onError]);

  const refreshTitledb = useCallback(async () => {
    setTitledbBusy(true);
    setTitledbError(null);
    try {
      await titledbUpdate();
      await reload();
    } catch (e) {
      setTitledbError(String(e));
    } finally {
      setTitledbBusy(false);
    }
  }, [reload]);

  useEffect(() => {
    reload();
    archiveImportStatus().then(setSnap, () => {});
    const unlisten = [onArchiveChanged(reload), onArchiveImport(setSnap)];
    return () => {
      unlisten.forEach((u) => u.then((off) => off()));
    };
  }, [reload]);

  // titledb's list a day old (or never downloaded): fetched once in the background.
  useEffect(() => {
    if (view?.titledbOn && view.titledbStale && !triedTitledb.current) {
      triedTitledb.current = true;
      refreshTitledb();
    }
  }, [view, refreshTitledb]);

  const startImport = useCallback(
    async (paths: string[], move = moving) => {
      if (!paths.length) return;
      try {
        setSnap(await archiveImport(paths, move));
      } catch (e) {
        onError(String(e));
      }
    },
    [moving, onError],
  );

  useEffect(() => {
    if (!dropped) return;
    onDropTaken();
    startImport(dropped);
  }, [dropped, onDropTaken, startImport]);

  const pickRoot = async () => {
    const path = await open({ directory: true, title: "Cartella archivio: una sottocartella per ogni gioco" });
    if (typeof path !== "string") return;
    try {
      await configSetArchive(path);
      await reload();
    } catch (e) {
      onError(String(e));
    }
  };

  const pickFiles = async () => {
    const picked = await open({
      multiple: true,
      title: "File di gioco o archivi da importare",
      filters: [{ name: "Giochi e archivi", extensions: IMPORT_EXTENSIONS }],
    });
    if (picked?.length) startImport(picked);
  };

  const pickFolders = async () => {
    const picked = await open({ multiple: true, directory: true, title: "Cartelle con i file di gioco" });
    if (picked?.length) startImport(picked);
  };

  const byId = useMemo(() => new Map(library.map((g) => [g.titleId, g])), [library]);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (view?.games ?? []).filter(
      (g) =>
        (filter === "all" || lacks(g)) &&
        (!q || g.name.toLowerCase().includes(q) || g.titleId.toLowerCase().includes(q)),
    );
  }, [view, filter, query]);

  if (!view) return <div className="flex-1" />;

  if (!view.root) {
    return (
      <div className="grid flex-1 place-items-center p-8">
        <div className="max-w-md text-center">
          <h2 className="font-semibold">Scegli la cartella archivio</h2>
          <p className="mt-2 text-sm text-neutral-500">
            Una cartella per i file dei tuoi giochi, anche su un disco esterno. Dentro, ogni gioco ha la sua
            sottocartella con il gioco base, gli update e i DLC, pronta da compilare. Importi file, cartelle e archivi
            .zip, .rar e .7z: il launcher li riconosce e li mette nella cartella giusta.
          </p>
          <button onClick={pickRoot} className="btn btn-primary mt-5">
            Scegli cartella…
          </button>
        </div>
      </div>
    );
  }

  const missingCount = view.games.filter(lacks).length;
  const busy = snap?.running ?? false;

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div className="border-b border-neutral-200 px-6 py-4 dark:border-neutral-800">
        <div className="flex items-center gap-3">
          <div className="min-w-0 flex-1">
            <h2 className="font-semibold">Archivio</h2>
            <p className="truncate font-mono text-xs text-neutral-500" title={view.root}>
              {view.root}
            </p>
          </div>
          <button onClick={() => archiveReveal().catch((e) => onError(String(e)))} className="btn">
            Mostra nel Finder
          </button>
          <button onClick={pickRoot} className="btn">
            Cambia…
          </button>
        </div>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <button onClick={pickFiles} disabled={busy} className="btn btn-primary">
            Importa file…
          </button>
          <button onClick={pickFolders} disabled={busy} className="btn">
            Importa cartella…
          </button>
          <label className="ml-2 flex items-center gap-2 text-sm text-neutral-600 dark:text-neutral-400">
            <input
              type="checkbox"
              checked={moving}
              onChange={(e) => {
                setMoving(e.target.checked);
                store("archive.moving", e.target.checked ? "move" : "copy");
              }}
            />
            Sposta i file invece di copiarli
          </label>
          <span className="text-xs text-neutral-500">· anche .zip, .rar e .7z, o trascinali qui</span>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto px-6 py-4">
        {snap && <ImportPanel snap={snap} onError={onError} onDismiss={() => archiveImportDismiss().then(() => setSnap(null))} />}

        {view.loose.length > 0 && (
          <div className="mb-4 flex items-center gap-3 rounded-lg border border-amber-300 bg-amber-50 px-4 py-3 text-sm dark:border-amber-800 dark:bg-amber-950/40">
            <p className="flex-1">
              {view.loose.length === 1 ? "1 file di gioco è" : `${view.loose.length} file di gioco sono`} nella cartella
              archivio, fuori da una cartella di gioco: la build non li trova.
            </p>
            <button onClick={() => startImport(view.loose, true)} disabled={busy} className="btn">
              Metti in ordine
            </button>
          </div>
        )}

        <TitledbLine
          view={view}
          busy={titledbBusy}
          error={titledbError}
          onRefresh={refreshTitledb}
        />

        {view.games.length === 0 ? (
          <p className="mt-6 text-sm text-neutral-500">
            Nessun gioco nell'archivio. Importa i file che hai copiato dalla tua console: NSP, XCI, NSZ, XCZ, NUP o una
            cartella dump (base/, updates/, dlc/).
          </p>
        ) : (
          <>
            <div className="mt-4 flex items-center gap-3">
              <input
                type="search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Cerca"
                className="w-56 rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-sm outline-none focus:border-sky-500 dark:border-neutral-700 dark:bg-neutral-900"
              />
              <div className="flex rounded-md border border-neutral-300 p-0.5 text-sm dark:border-neutral-700">
                {(
                  [
                    ["all", `Tutti (${view.games.length})`],
                    ["missing", `Manca qualcosa (${missingCount})`],
                  ] as const
                ).map(([k, label]) => (
                  <button
                    key={k}
                    onClick={() => {
                      setFilter(k);
                      store("archive.filter", k);
                    }}
                    className={`rounded px-2.5 py-1 ${filter === k ? "bg-neutral-200 font-medium dark:bg-neutral-800" : "text-neutral-500"}`}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>
            {shown.length === 0 ? (
              <p className="mt-6 text-sm text-neutral-500">
                {filter === "missing" && !query ? "A nessun gioco manca qualcosa." : `Nessun gioco corrisponde a «${query}».`}
              </p>
            ) : (
              <ul className="mt-4 divide-y divide-neutral-200 rounded-xl border border-neutral-200 dark:divide-neutral-800 dark:border-neutral-800">
                {shown.map((g) => (
                  <GameRow
                    key={g.folder}
                    game={g}
                    titledbOn={view.titledbOn}
                    inLibrary={byId.get(g.titleId) ?? null}
                    expanded={open_ === g.folder}
                    onToggle={() => setOpen(open_ === g.folder ? null : g.folder)}
                    onBuild={() => onBuild(g.folder)}
                    onReveal={() => archiveReveal(g.titleId || undefined).catch((e) => onError(String(e)))}
                  />
                ))}
              </ul>
            )}
          </>
        )}
      </div>
    </div>
  );
}

function TitledbLine({
  view,
  busy,
  error,
  onRefresh,
}: {
  view: ArchiveView;
  busy: boolean;
  error: string | null;
  onRefresh: () => void;
}) {
  if (!view.titledbOn) {
    return (
      <p className="text-xs text-neutral-500">
        Il controllo di update e DLC mancanti è spento (Impostazioni).
      </p>
    );
  }
  return (
    <p className="text-xs text-neutral-500">
      Update e DLC esistenti secondo titledb
      {view.titledbAt ? ` · elenco di ${formatWhen(view.titledbAt)}` : " · elenco non ancora scaricato"}
      {" · "}
      <button onClick={onRefresh} disabled={busy} className="underline hover:text-neutral-900 dark:hover:text-neutral-100">
        {busy ? "Scarico…" : "Aggiorna elenco"}
      </button>
      {error && <span className="ml-2 text-red-600 dark:text-red-400">{error}</span>}
    </p>
  );
}

type Tone = "ok" | "warn" | "bad" | "neutral";

const CHIP: Record<Tone, string> = {
  ok: "bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300",
  warn: "bg-amber-100 text-amber-800 dark:bg-amber-950 dark:text-amber-300",
  bad: "bg-red-100 text-red-800 dark:bg-red-950 dark:text-red-300",
  neutral: "bg-neutral-100 text-neutral-700 dark:bg-neutral-800 dark:text-neutral-300",
};

function Chip({ tone, children }: { tone: Tone; children: React.ReactNode }) {
  return <span className={`rounded-full px-2 py-0.5 text-xs font-medium ${CHIP[tone]}`}>{children}</span>;
}

/** What the folder holds against what titledb says exists. */
function chips(g: ArchiveGame, titledbOn: boolean): [Tone, string][] {
  const out: [Tone, string][] = [];
  out.push(
    g.hasBase
      ? ["ok", "Gioco base"]
      : g.standalone
        ? ["ok", "Update completo: basta da solo"]
        : ["bad", "Manca il gioco base"],
  );
  if (g.known) {
    // titledb may know the game but none of its updates (most Switch 2 games): yours is not "missing".
    if (g.latestUpdate === null) out.push(g.update === null ? ["ok", "Nessun update esistente"] : ["ok", `Update v${g.update}`]);
    else if (!g.updateMissing) out.push(["ok", `Update v${g.update} (l'ultimo)`]);
    else if (g.update === null) out.push(["warn", `Manca l'update v${g.latestUpdate}`]);
    else out.push(["warn", `Update v${g.update}, l'ultimo è v${g.latestUpdate}`]);
    const total = g.dlc.length + g.missingDlc.length;
    if (total > 0) out.push([g.missingDlc.length ? "warn" : "ok", `DLC ${g.dlc.length} di ${total}`]);
  } else {
    out.push(["neutral", g.update === null ? "Nessun update" : `Update v${g.update}`]);
    if (g.dlc.length) out.push(["neutral", `${g.dlc.length} DLC`]);
    if (titledbOn) out.push(["neutral", "titledb non conosce il gioco"]);
  }
  return out;
}

function GameRow({
  game: g,
  titledbOn,
  inLibrary,
  expanded,
  onToggle,
  onBuild,
  onReveal,
}: {
  game: ArchiveGame;
  titledbOn: boolean;
  inLibrary: Game | null;
  expanded: boolean;
  onToggle: () => void;
  onBuild: () => void;
  onReveal: () => void;
}) {
  const icon = {
    name: g.name,
    titleId: g.titleId,
    coverPath: inLibrary?.coverPath ?? null,
    iconPath: inLibrary?.iconPath ?? null,
  };
  return (
    <li className="px-4 py-3">
      <div className="flex items-start gap-3">
        <GameIcon game={icon} className="w-12 shrink-0" />
        <div className="min-w-0 flex-1">
          <button onClick={onToggle} className="max-w-full truncate text-left font-medium hover:underline" title={g.folder}>
            {g.name}
          </button>
          <p className="text-xs text-neutral-500">
            <span className="font-mono">{g.titleId || "title ID sconosciuto"}</span>
            {g.format && ` · ${FORMATS[g.format]}`} · {formatBytes(g.sizeBytes)}
            {inLibrary && " · in libreria"}
          </p>
          <div className="mt-1.5 flex flex-wrap gap-1.5">
            {chips(g, titledbOn).map(([tone, text]) => (
              <Chip key={text} tone={tone}>
                {text}
              </Chip>
            ))}
          </div>
          {g.issues.map((issue) => (
            <p key={issue} className="mt-1.5 text-xs text-amber-700 dark:text-amber-400">
              ⚠ {issue}
            </p>
          ))}
          {expanded && <Details game={g} />}
        </div>
        <div className="flex shrink-0 gap-2">
          <button onClick={onReveal} className="btn">
            Finder
          </button>
          <button onClick={onBuild} disabled={!g.format} className="btn btn-primary">
            Compila
          </button>
        </div>
      </div>
    </li>
  );
}

const KIND_NAMES = { base: "Gioco base", update: "Update", dlc: "DLC" };

function Details({ game: g }: { game: ArchiveGame }) {
  const lacking = g.updateMissing || g.missingDlc.length > 0;
  return (
    <div className="mt-3 rounded-lg bg-neutral-50 p-3 text-xs dark:bg-neutral-900">
      <table className="w-full">
        <tbody>
          {g.pieces.map((p) => (
            <tr key={`${p.titleId}-${p.version}-${p.file}`}>
              <td className="py-0.5 pr-3 whitespace-nowrap">{KIND_NAMES[p.kind]}</td>
              <td className="py-0.5 pr-3 font-mono whitespace-nowrap">
                {p.kind === "dlc" ? p.titleId : `v${p.version}`}
              </td>
              <td className="max-w-0 truncate py-0.5 text-neutral-500" title={p.file}>
                {p.file || "cartella dump"}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {lacking && (
        <div className="mt-3 border-t border-neutral-200 pt-3 dark:border-neutral-800">
          {g.updateMissing && (
            <p>
              Manca l'update <span className="font-mono">v{g.latestUpdate}</span>
              {g.latestUpdate !== null && <span className="text-neutral-500"> (title ID {updateId(g.titleId)})</span>}
            </p>
          )}
          {g.missingDlc.length > 0 && (
            <p className="mt-1">
              DLC mancanti: <span className="font-mono">{g.missingDlc.join(", ")}</span>
            </p>
          )}
          <p className="mt-2 text-neutral-500">
            Se li hai sulla console, copiali con il tuo strumento di dump (DBI, nxdumptool) e importali qui: finiscono
            in questa cartella da soli.
          </p>
        </div>
      )}
    </div>
  );
}

/** The update's title ID: the base game's with …800 at the end. */
const updateId = (titleId: string) => (titleId.length === 16 ? `${titleId.slice(0, 13)}800` : "");

function ImportPanel({
  snap,
  onDismiss,
  onError,
}: {
  snap: ImportSnapshot;
  onDismiss: () => void;
  onError: (m: string) => void;
}) {
  const logRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, [snap.log.length]);

  const TONES = {
    info: "text-neutral-500",
    ok: "text-emerald-700 dark:text-emerald-400",
    warn: "text-amber-700 dark:text-amber-400",
    error: "text-red-700 dark:text-red-400",
  };

  return (
    <div className="mb-4 rounded-xl border border-neutral-200 p-4 dark:border-neutral-800">
      <div className="flex items-center gap-3">
        <h3 className="flex-1 text-sm font-medium">
          {snap.running
            ? snap.moving
              ? "Importazione (sposto i file)"
              : "Importazione (copio i file)"
            : snap.cancelled
              ? "Importazione annullata"
              : "Importazione finita"}
        </h3>
        {snap.running ? (
          <button onClick={() => archiveImportCancel().catch((e) => onError(String(e)))} className="btn btn-danger">
            Annulla
          </button>
        ) : (
          <button onClick={onDismiss} className="btn">
            Chiudi
          </button>
        )}
      </div>
      {snap.running && (
        <>
          <p className="mt-2 truncate text-sm text-neutral-600 dark:text-neutral-400">{snap.step}</p>
          <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-neutral-200 dark:bg-neutral-800">
            {snap.frac === null ? (
              <div className="indeterminate h-full w-1/3 rounded-full bg-sky-500" />
            ) : (
              <div className="h-full rounded-full bg-sky-500 transition-[width]" style={{ width: `${snap.frac * 100}%` }} />
            )}
          </div>
        </>
      )}
      {snap.log.length > 0 && (
        <div ref={logRef} className="mt-3 max-h-48 overflow-y-auto font-mono text-xs leading-relaxed">
          {snap.log.map((l, i) => (
            <p key={i} className={TONES[l.tone]}>
              {l.text}
            </p>
          ))}
        </div>
      )}
    </div>
  );
}

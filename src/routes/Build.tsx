import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import { KeysRow } from "../components/KeysRow";
import { useNow } from "../hooks/useLibrary";
import { formatBytes, formatClock } from "../lib/format";
import {
  buildCancel,
  buildDismiss,
  buildPreflight,
  buildReveal,
  buildRevealOutput,
  buildStart,
  buildStatus,
  configGet,
  configSetKeys,
  fileSrc,
  gameLaunch,
  onBuildUpdated,
  type BuildOptions,
  type BuildProgress,
  type BuildSnapshot,
  type Preflight,
  type Stage,
} from "../lib/ipc";

type Props = {
  /** A folder or .nup dropped on the window, to check and build. */
  pendingSource: string | null;
  onSourceTaken: () => void;
  onOpenGame: (titleId: string) => void;
  onError: (message: string) => void;
};

const KIND_NAMES = { nsp: "NSP / XCI / NSZ", nup: "NUP (Switch 2)", dump: "Cartella dump" };

/** Building a game from the UI: pick the source, check it, follow the build. */
export default function Build({ pendingSource, onSourceTaken, onOpenGame, onError }: Props) {
  const [snap, setSnap] = useState<BuildSnapshot | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [keys, setKeys] = useState<string | null>(null);
  const [pre, setPre] = useState<Preflight | null>(null);
  const [checking, setChecking] = useState(false);
  const [opts, setOpts] = useState<BuildOptions>({ target: "mac", romfsSeparate: false });

  useEffect(() => {
    buildStatus().then((s) => {
      setSnap(s);
      setLoaded(true);
    });
    configGet().then((c) => setKeys(c.prodKeys));
    const unlisten = onBuildUpdated(setSnap);
    return () => {
      unlisten.then((off) => off());
    };
  }, []);

  const check = useCallback(
    async (path: string, options: BuildOptions = opts) => {
      setChecking(true);
      try {
        setPre(await buildPreflight(path, options));
      } catch (e) {
        onError(String(e));
      } finally {
        setChecking(false);
      }
    },
    [onError, opts],
  );

  const changeOpts = (next: BuildOptions) => {
    setOpts(next);
    if (pre) check(pre.source.path, next); // an IPA has its own requirements
  };

  useEffect(() => {
    if (pendingSource && loaded && !snap) {
      check(pendingSource);
      onSourceTaken();
    }
  }, [pendingSource, loaded, snap, check, onSourceTaken]);

  const pick = async (directory: boolean) => {
    const path = await open(
      directory
        ? { directory: true, title: "Cartella del gioco (NSP, XCI, NSZ, dump o con un .nup)" }
        : { title: "Container NUP", filters: [{ name: "NUP", extensions: ["nup"] }] },
    );
    if (typeof path === "string") check(path);
  };

  const pickKeys = async () => {
    const path = await open({ title: "Scegli il tuo prod.keys", filters: [{ name: "prod.keys", extensions: ["keys"] }] });
    if (typeof path !== "string") return;
    try {
      setKeys((await configSetKeys(path)).prodKeys);
      if (pre) check(pre.source.path);
    } catch (e) {
      onError(String(e));
    }
  };

  const start = async (path: string, options: BuildOptions = opts) => {
    try {
      setSnap(await buildStart(path, options));
      setPre(null);
    } catch (e) {
      onError(String(e));
    }
  };

  const dismiss = async () => {
    await buildDismiss();
    setSnap(null);
  };

  return (
    <div className="flex-1 overflow-y-auto">
      <div className="mx-auto max-w-3xl px-6 py-8">
        {!loaded ? null : snap ? (
          <BuildView
            snap={snap}
            onRetry={() => start(snap.source.path, snap.options)}
            onDismiss={dismiss}
            onOpenGame={onOpenGame}
            onError={onError}
          />
        ) : (
          <>
            <h2 className="text-lg font-semibold">Compila un gioco</h2>
            <p className="mt-1 text-sm text-neutral-500">
              Scegli i file del tuo gioco: nxnative ne fa un'app per Mac che compare nella libreria. La prima build
              richiede 30–60 minuti; le successive riusano il lavoro già fatto.
            </p>

            <div className="mt-6 flex gap-3">
              <button onClick={() => pick(true)} disabled={checking} className="btn btn-primary">
                Cartella del gioco…
              </button>
              <button onClick={() => pick(false)} disabled={checking} className="btn">
                File .nup…
              </button>
              <span className="self-center text-xs text-neutral-500">oppure trascinali sulla finestra</span>
            </div>

            <KeysRow keys={keys} onPick={pickKeys} onClear={async () => setKeys((await configSetKeys(null)).prodKeys)} />

            {checking && <p className="mt-6 text-sm text-neutral-500">Controllo…</p>}
            {pre && !checking && (
              <PreflightCard
                pre={pre}
                opts={opts}
                onOpts={changeOpts}
                onStart={() => start(pre.source.path)}
                onCancel={() => setPre(null)}
              />
            )}
          </>
        )}
      </div>
    </div>
  );
}

function PreflightCard({
  pre,
  opts,
  onOpts,
  onStart,
  onCancel,
}: {
  pre: Preflight;
  opts: BuildOptions;
  onOpts: (o: BuildOptions) => void;
  onStart: () => void;
  onCancel: () => void;
}) {
  const s = pre.source;
  const blocked = pre.blockers.length > 0;
  const targets = [
    ["mac", "App per Mac"],
    ["ios", "IPA per iPhone e iPad"],
  ] as const;
  return (
    <div className="mt-6 rounded-xl border border-neutral-200 p-5 dark:border-neutral-800">
      <div className="flex items-center justify-between gap-4">
        <h3 className="font-semibold">{s.name}</h3>
        <div className="inline-flex rounded-lg bg-neutral-200/70 p-0.5 dark:bg-neutral-800" role="radiogroup">
          {targets.map(([t, label]) => (
            <button
              key={t}
              role="radio"
              aria-checked={opts.target === t}
              onClick={() => onOpts({ ...opts, target: t })}
              className={`rounded-md px-3 py-1 text-sm ${
                opts.target === t ? "bg-white font-medium shadow-sm dark:bg-neutral-600" : "text-neutral-500"
              }`}
            >
              {label}
            </button>
          ))}
        </div>
      </div>
      {opts.target === "ios" && (
        <label className="mt-3 flex items-start gap-2 text-sm">
          <input
            type="checkbox"
            checked={opts.romfsSeparate}
            onChange={(e) => onOpts({ ...opts, romfsSeparate: e.target.checked })}
            className="mt-0.5 accent-sky-500"
          />
          <span>
            File del gioco separati (<code className="font-mono text-xs">--romfs separate</code>)
            <span className="block text-xs text-neutral-500">
              Per i giochi troppo grandi da installare: la build ti dirà il comando per copiarli sul dispositivo
              una volta sola. Serve un iPhone con A17 Pro o più recente, o un iPad con M1 o più recente.
            </span>
          </span>
        </label>
      )}
      <dl className="mt-3 grid grid-cols-[8rem_1fr] gap-x-4 gap-y-1.5 text-sm">
        <dt className="text-neutral-500">Tipo</dt>
        <dd>{KIND_NAMES[s.kind]}</dd>
        <dt className="text-neutral-500">Dimensione</dt>
        <dd>{formatBytes(s.sizeBytes)}</dd>
        {s.files.length > 0 && (
          <>
            <dt className="text-neutral-500">File</dt>
            <dd className="font-mono text-xs leading-relaxed break-all">{s.files.join("\n")}</dd>
          </>
        )}
        <dt className="text-neutral-500">Spazio</dt>
        <dd>
          {formatBytes(pre.freeBytes)} liberi, ne servono circa {formatBytes(pre.neededBytes)}
          {s.previousBuild && <span className="text-neutral-500"> (c'è già una build precedente da riusare)</span>}
        </dd>
        <dt className="text-neutral-500">App e build</dt>
        <dd className="truncate font-mono text-xs leading-5" title={s.buildDir}>
          {s.buildDir}
        </dd>
      </dl>

      {[...pre.blockers.map((t) => ["error", t] as const), ...pre.warnings.map((t) => ["warn", t] as const)].map(
        ([tone, text]) => (
          <p
            key={text}
            className={`mt-3 rounded-lg p-3 text-sm ${
              tone === "error"
                ? "bg-red-500/10 text-red-700 dark:text-red-300"
                : "bg-amber-500/10 text-amber-700 dark:text-amber-300"
            }`}
          >
            {text}
          </p>
        ),
      )}

      <div className="mt-5 flex gap-2">
        <button onClick={onStart} disabled={blocked} className="btn btn-primary">
          {pre.warnings.length && !blocked ? "Compila comunque" : "Compila"}
        </button>
        <button onClick={onCancel} className="btn">
          Annulla
        </button>
      </div>
    </div>
  );
}

function BuildView({
  snap,
  onRetry,
  onDismiss,
  onOpenGame,
  onError,
}: {
  snap: BuildSnapshot;
  onRetry: () => void;
  onDismiss: () => void;
  onOpenGame: (titleId: string) => void;
  onError: (message: string) => void;
}) {
  const now = useNow(1000);
  const [confirmCancel, setConfirmCancel] = useState(false);
  const running = snap.status === "running";
  const current = [...snap.stages].reverse().find((s) => s.status === "running");
  const doneStages = snap.stages.filter((s) => s.status !== "running").length;
  const elapsed = (snap.finishedAt ?? now) - snap.startedAt;
  const title = snap.game?.name || snap.source.name;

  const safely = (fn: () => Promise<unknown>) => () => fn().catch((e) => onError(String(e)));

  return (
    <div>
      <div className="flex items-center gap-4">
        {snap.game?.icon ? (
          <img src={fileSrc(snap.game.icon)} alt="" className="size-16 rounded-[22%] shadow" />
        ) : (
          <div className="grid size-16 place-items-center rounded-[22%] bg-neutral-200 text-2xl dark:bg-neutral-800">⚙︎</div>
        )}
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-lg font-semibold">{title}</h2>
          <p className="font-mono text-xs text-neutral-500">
            {snap.game ? `${snap.game.titleId}${snap.game.version ? ` · v${snap.game.version}` : ""} · ` : ""}
            {formatClock(elapsed)}
          </p>
        </div>
        {running &&
          (confirmCancel ? (
            <div className="flex items-center gap-2 text-sm">
              <span className="text-neutral-500">La prossima riprenderà da qui.</span>
              <button onClick={safely(buildCancel)} className="btn btn-danger">
                Annulla build
              </button>
              <button onClick={() => setConfirmCancel(false)} className="btn">
                Continua
              </button>
            </div>
          ) : (
            <button onClick={() => setConfirmCancel(true)} className="btn">
              Annulla…
            </button>
          ))}
      </div>

      {running && (
        <div className="mt-6 rounded-xl border border-neutral-200 p-5 dark:border-neutral-800">
          <div className="flex items-baseline justify-between text-sm">
            <p className="font-medium">
              {current ? current.name : "Preparazione…"}
              {snap.step && <span className="font-normal text-neutral-500"> · {snap.step}</span>}
            </p>
            <p className="text-neutral-500 tabular-nums">
              {snap.totalStages ? `fase ${Math.min(doneStages + 1, snap.totalStages)} di ${snap.totalStages}` : ""}
            </p>
          </div>
          <Bar frac={snap.progress ? snap.progress.frac : current ? null : 0} />
          <ProgressLine p={snap.progress} />
          <div className="mt-4 h-1 overflow-hidden rounded-full bg-neutral-200 dark:bg-neutral-800">
            <div
              className="h-full bg-neutral-400 transition-[width] dark:bg-neutral-500"
              style={{ width: `${snap.totalStages ? (doneStages / snap.totalStages) * 100 : 0}%` }}
            />
          </div>
        </div>
      )}

      {snap.status === "succeeded" && snap.options.target === "ios" && (
        <Banner tone="ok" title="IPA pronto">
          Installalo sul tuo iPhone o iPad con AltStore, Sideloadly o LiveContainer; se lo strumento offre
          «Increased Memory Limit», attivalo: i giochi più grandi ne hanno bisogno.
          {snap.options.romfsSeparate && (
            <span className="mt-1 block">
              Hai scelto i file separati: il comando per copiarli sul dispositivo è nei messaggi qui sotto.
            </span>
          )}
          <div className="mt-3 flex gap-2">
            <button onClick={safely(buildRevealOutput)} className="btn btn-primary">
              Mostra l'IPA nel Finder
            </button>
            <button onClick={onDismiss} className="btn">
              Nuova build
            </button>
          </div>
        </Banner>
      )}
      {snap.status === "succeeded" && snap.options.target === "mac" && (
        <Banner tone="ok" title="Pronto!">
          {title} è nella libreria.
          <div className="mt-3 flex gap-2">
            {snap.imported && (
              <>
                <button
                  onClick={safely(async () => {
                    await gameLaunch(snap.imported!, false);
                    onOpenGame(snap.imported!);
                  })}
                  className="btn btn-primary"
                >
                  ▶ Gioca
                </button>
                <button onClick={() => onOpenGame(snap.imported!)} className="btn">
                  Vai alla libreria
                </button>
              </>
            )}
            <button onClick={onDismiss} className="btn">
              Nuova build
            </button>
          </div>
        </Banner>
      )}
      {snap.status === "failed" && (
        <Banner tone="error" title="La build si è fermata">
          <span className="whitespace-pre-wrap">{snap.error}</span>
          {snap.stderr.length > 0 && (
            <details className="mt-2">
              <summary className="cursor-pointer text-xs">Output di errore (stderr)</summary>
              <pre className="mt-1 max-h-40 overflow-auto text-xs whitespace-pre-wrap">{snap.stderr.join("\n")}</pre>
            </details>
          )}
          <div className="mt-3 flex gap-2">
            <button onClick={safely(buildReveal)} className="btn">
              Apri i log
            </button>
            <button onClick={onRetry} className="btn">
              Riprova
            </button>
            <button onClick={onDismiss} className="btn">
              Nuova build
            </button>
          </div>
        </Banner>
      )}
      {snap.status === "cancelled" && (
        <Banner tone="neutral" title="Build annullata">
          La prossima build di questo gioco riprenderà da dove si è fermata.
          <div className="mt-3 flex gap-2">
            <button onClick={onRetry} className="btn">
              Riprendi
            </button>
            <button onClick={onDismiss} className="btn">
              Nuova build
            </button>
          </div>
        </Banner>
      )}

      {snap.stages.length > 0 && <StageList stages={snap.stages} />}
      <MessageLog messages={snap.messages} />
    </div>
  );
}

function Bar({ frac }: { frac: number | null }) {
  return (
    <div className="mt-3 h-2 overflow-hidden rounded-full bg-neutral-200 dark:bg-neutral-800">
      {frac === null ? (
        <div className="indeterminate h-full w-1/3 rounded-full bg-sky-500" />
      ) : (
        <div className="h-full rounded-full bg-sky-500 transition-[width] duration-500" style={{ width: `${frac * 100}%` }} />
      )}
    </div>
  );
}

function ProgressLine({ p }: { p: BuildProgress | null }) {
  if (!p) return <p className="mt-2 h-4" />;
  const count =
    p.done === null
      ? ""
      : p.unit === "B"
        ? `${formatBytes(p.done)}${p.total ? ` di ${p.est ? "~" : ""}${formatBytes(p.total)}` : ""}`
        : `${p.done.toLocaleString("it")}${p.total ? `/${p.est ? "~" : ""}${p.total.toLocaleString("it")}` : ""}${p.unit ? ` ${p.unit}` : ""}`;
  return (
    <p className="mt-2 flex justify-between text-xs text-neutral-500 tabular-nums">
      <span>{[count, p.note].filter(Boolean).join(" · ")}</span>
      <span>
        {p.frac !== null && `${Math.round(p.frac * 100)}%`}
        {p.eta !== null && ` · ancora ${formatClock(p.eta)}`}
      </span>
    </p>
  );
}

const STAGE_ICON = { running: "◌", ok: "✓", warn: "⚠", failed: "✗", stopped: "–" };
const STAGE_COLOR = {
  running: "text-sky-500",
  ok: "text-emerald-500",
  warn: "text-amber-500",
  failed: "text-red-500",
  stopped: "text-neutral-400",
};

function StageList({ stages }: { stages: Stage[] }) {
  return (
    <ol className="mt-6 space-y-1 text-sm">
      {stages.map((s, i) => (
        <li key={`${s.k}-${i}`} className="flex gap-3">
          <span className={`w-4 text-center ${STAGE_COLOR[s.status]} ${s.status === "running" ? "animate-spin" : ""}`}>
            {STAGE_ICON[s.status]}
          </span>
          <span className="flex-1">
            {s.name}
            {s.summary && <span className="text-neutral-500"> · {s.summary}</span>}
          </span>
          {s.elapsed !== null && <span className="text-xs text-neutral-500 tabular-nums">{formatClock(s.elapsed)}</span>}
        </li>
      ))}
    </ol>
  );
}

function MessageLog({ messages }: { messages: BuildSnapshot["messages"] }) {
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  useEffect(() => {
    const el = ref.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [messages]);
  if (!messages.length) return null;
  return (
    <div
      ref={ref}
      onScroll={(e) => {
        const el = e.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
      }}
      className="mt-6 max-h-56 overflow-y-auto rounded-lg bg-neutral-100 p-3 font-mono text-xs leading-relaxed dark:bg-neutral-900"
    >
      {messages.map((m, i) => (
        <p key={i} className={`whitespace-pre-wrap ${m.red ? "text-red-600 dark:text-red-400" : "text-neutral-600 dark:text-neutral-400"}`}>
          {m.text}
        </p>
      ))}
    </div>
  );
}

function Banner({ tone, title, children }: { tone: "ok" | "error" | "neutral"; title: string; children: React.ReactNode }) {
  const tones = {
    ok: "bg-emerald-500/10 text-emerald-800 dark:text-emerald-200",
    error: "bg-red-500/10 text-red-800 dark:text-red-200",
    neutral: "bg-neutral-100 text-neutral-700 dark:bg-neutral-900 dark:text-neutral-300",
  };
  return (
    <div className={`mt-6 rounded-xl p-5 text-sm ${tones[tone]}`}>
      <p className="mb-1 font-semibold">{title}</p>
      {children}
    </div>
  );
}

import { useEffect, useMemo, useState } from "react";
import { GameIcon } from "../components/GameIcon";
import { describe, SettingControl } from "../components/SettingControl";
import { gameLaunch, settingsGet, settingsSave, type Game, type GameSettings } from "../lib/ipc";
import {
  changed,
  effective,
  needsMet,
  same,
  sections,
  SECTION_NAMES,
  type Choices,
} from "../lib/settings";

type Props = {
  game: Game;
  onBack: () => void;
  onLaunched: (game: Game) => void;
  onError: (message: string) => void;
};

/** A game's settings, as its own settings window shows them, kept in the
 *  same place (the app's preferences), so both always agree. */
export default function GameSettingsView({ game, onBack, onLaunched, onError }: Props) {
  const [data, setData] = useState<GameSettings | null>(null);
  const [draft, setDraft] = useState<Choices>({});
  const [showAll, setShowAll] = useState(false);
  const [busy, setBusy] = useState(false);
  const [flash, setFlash] = useState<string | null>(null);

  const load = (d: GameSettings) => {
    setData(d);
    setDraft(effective(d));
  };

  useEffect(() => {
    settingsGet(game.titleId).then(load, (e) => onError(String(e)));
  }, [game.titleId, onError]);

  const saved = useMemo(() => (data ? effective(data) : {}), [data]);
  const dirty = data ? changed(data, draft, saved) : [];
  const running = game.runningSince !== null;
  const hasBasic = data?.settings.some((s) => s.level === "basic") ?? false;
  const advanced = data?.settings.filter((s) => hasBasic && s.level !== "basic").length ?? 0;

  const run = async (fn: () => Promise<void>) => {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const save = () =>
    run(async () => {
      load(await settingsSave(game.titleId, draft));
      setFlash("Impostazioni salvate");
      setTimeout(() => setFlash(null), 2500);
    });

  // This start only: the changed rows go in the launch environment, nothing is saved.
  const tryOnce = () =>
    run(async () => {
      const overrides = Object.fromEntries(dirty.map((s) => [s.env, draft[s.env]!]));
      onLaunched(await gameLaunch(game.titleId, false, overrides));
    });

  const play = () => run(async () => onLaunched(await gameLaunch(game.titleId, false)));

  const resetAll = () =>
    data &&
    setDraft({
      ...draft,
      ...Object.fromEntries(data.settings.filter((s) => !data.fixed[s.env]).map((s) => [s.env, s.default])),
    });

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <div className="flex items-center gap-4 border-b border-neutral-200 px-6 py-4 dark:border-neutral-800">
        <button onClick={onBack} className="text-sm text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-100">
          ← Libreria
        </button>
        <GameIcon game={game} className="w-10" />
        <div className="min-w-0">
          <h2 className="truncate font-semibold">{game.name}</h2>
          <p className="text-xs text-neutral-500">Impostazioni</p>
        </div>
        <div className="ml-auto flex items-center gap-3">
          {flash && <span className="text-sm text-emerald-600 dark:text-emerald-400">✓ {flash}</span>}
          {data?.settings.length ? (
            <button onClick={resetAll} disabled={busy} className="btn">
              Ripristina default testati
            </button>
          ) : null}
          <button onClick={play} disabled={busy || running || dirty.length > 0} className="btn btn-primary">
            {running ? "● In gioco" : "▶ Gioca"}
          </button>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto">
        <div className="mx-auto max-w-2xl px-6 py-6">
          {!data ? null : data.schema === 0 ? (
            <Empty>Questo gioco non ha impostazioni: parte così com'è.</Empty>
          ) : !data.supported ? (
            <Empty>
              Questo gioco usa un formato di impostazioni più nuovo (schema {data.schema}) di quello che il
              launcher conosce. Aggiorna il launcher, oppure usa «Gioca con la finestra impostazioni del gioco».
            </Empty>
          ) : (
            <>
              {sections(data.settings).map(([section, rows]) => {
                const drawn = rows.filter(
                  (s) => (showAll || !hasBasic || s.level === "basic") && needsMet(s, data.settings, draft),
                );
                if (!drawn.length) return null;
                return (
                  <section key={section} className="mb-8">
                    <h3 className="mb-2 text-xs font-semibold tracking-wide text-neutral-500 uppercase">
                      {SECTION_NAMES[section] ?? section}
                    </h3>
                    <ul className="divide-y divide-neutral-200 rounded-lg border border-neutral-200 dark:divide-neutral-800 dark:border-neutral-800">
                      {drawn.map((s) => {
                        const fixed = data.fixed[s.env];
                        const value = draft[s.env] ?? s.default;
                        const isDefault = same(s, value, s.default);
                        return (
                          <li key={s.env} className={`flex items-center gap-4 px-4 py-3 ${s.needs ? "pl-9" : ""}`}>
                            <div className="min-w-0 flex-1">
                              <p className="flex items-center gap-2 text-sm font-medium">
                                {s.label}
                                {!fixed && !isDefault && (
                                  <button
                                    onClick={() => setDraft({ ...draft, [s.env]: s.default })}
                                    title={`Torna al default testato (${describe(s, s.default)})`}
                                    className="text-xs font-normal text-sky-600 hover:underline dark:text-sky-400"
                                  >
                                    ↺ default
                                  </button>
                                )}
                              </p>
                              {s.note && <p className="mt-0.5 text-xs text-neutral-500">{s.note}</p>}
                              {fixed && (
                                <p className="mt-1 text-xs text-amber-700 dark:text-amber-400">
                                  Fissato da{" "}
                                  {fixed.source === "player"
                                    ? "env.txt in Application Support"
                                    : "env.txt dentro l'app (deciso alla build)"}
                                </p>
                              )}
                            </div>
                            <SettingControl
                              setting={s}
                              value={value}
                              disabled={!!fixed || busy}
                              onChange={(v) => setDraft({ ...draft, [s.env]: v })}
                            />
                          </li>
                        );
                      })}
                    </ul>
                  </section>
                );
              })}
              {advanced > 0 && (
                <button
                  onClick={() => setShowAll(!showAll)}
                  className="text-sm text-sky-600 hover:underline dark:text-sky-400"
                >
                  {showAll ? "Mostra solo le principali" : `Mostra tutte le impostazioni (${advanced} in più)`}
                </button>
              )}
            </>
          )}
        </div>
      </div>

      {dirty.length > 0 && (
        <div className="flex items-center gap-3 border-t border-neutral-200 bg-white/80 px-6 py-3 backdrop-blur dark:border-neutral-800 dark:bg-neutral-950/80">
          <p className="text-sm text-neutral-600 dark:text-neutral-400">
            {dirty.length === 1 ? "1 modifica non salvata" : `${dirty.length} modifiche non salvate`}
          </p>
          <div className="ml-auto flex gap-2">
            <button onClick={() => setDraft(saved)} disabled={busy} className="btn">
              Annulla
            </button>
            <button
              onClick={tryOnce}
              disabled={busy || running}
              title="Avvia il gioco con queste impostazioni senza salvarle"
              className="btn"
            >
              ▶ Prova una volta
            </button>
            <button onClick={save} disabled={busy} className="btn btn-primary">
              Salva
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return <p className="rounded-lg bg-neutral-100 p-4 text-sm text-neutral-600 dark:bg-neutral-900 dark:text-neutral-400">{children}</p>;
}

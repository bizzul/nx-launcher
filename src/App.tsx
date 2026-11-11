import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import { useLibrary } from "./hooks/useLibrary";
import { libraryImport } from "./lib/ipc";
import Build from "./routes/Build";
import Doctor from "./routes/Doctor";
import GameDetail from "./routes/GameDetail";
import GameSettingsView from "./routes/GameSettings";
import Library from "./routes/Library";

type View = "library" | "settings" | "build" | "doctor";

export default function App() {
  const [view, setView] = useState<View>("library");
  const { games, refresh, update } = useLibrary();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  /** A game folder or .nup dropped on the window, handed to the build view. */
  const [buildSource, setBuildSource] = useState<string | null>(null);
  const [toasts, setToasts] = useState<{ id: number; text: string }[]>([]);

  const toast = useCallback((text: string) => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t, { id, text }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 7000);
  }, []);

  const importPaths = useCallback(
    async (paths: string[]) => {
      try {
        const report = await libraryImport(paths);
        await refresh();
        report.errors.forEach(toast);
        if (report.added.length) {
          setView("library");
          setSelectedId(report.added[0]!.titleId);
        }
      } catch (e) {
        toast(String(e));
      }
    },
    [refresh, toast],
  );

  const pickApps = async () => {
    const picked = await open({
      multiple: true,
      title: "Scegli i giochi creati da nxnative",
      filters: [{ name: "Giochi nxnative", extensions: ["app"] }],
    });
    if (picked?.length) importPaths(picked);
  };

  // Dropped on the window: built apps (.app) join the library; a game's
  // folder or a .nup goes to the build view.
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "enter" || payload.type === "over") setDragging(true);
      else {
        setDragging(false);
        if (payload.type !== "drop") return;
        const apps = payload.paths.filter((p) => p.endsWith(".app"));
        const sources = payload.paths.filter((p) => !p.endsWith(".app"));
        if (apps.length) importPaths(apps);
        if (sources.length) {
          setBuildSource(sources[0]!);
          setView("build");
        }
      }
    });
    return () => {
      unlisten.then((off) => off());
    };
  }, [importPaths]);

  const selected = games?.find((g) => g.titleId === selectedId) ?? null;

  return (
    <div className="flex h-screen flex-col">
      <header className="flex h-13 shrink-0 items-center gap-6 border-b border-neutral-200 px-5 dark:border-neutral-800">
        <h1 className="font-semibold tracking-tight">NX Launcher</h1>
        <nav className="flex gap-1 text-sm">
          <Tab active={view === "library" || view === "settings"} onClick={() => setView("library")}>
            Libreria{games?.length ? <span className="ml-1.5 text-neutral-400">{games.length}</span> : null}
          </Tab>
          <Tab active={view === "build"} onClick={() => setView("build")}>
            Compila
          </Tab>
          <Tab active={view === "doctor"} onClick={() => setView("doctor")}>
            Ambiente
          </Tab>
        </nav>
        <button onClick={pickApps} className="btn btn-primary ml-auto">
          Aggiungi gioco
        </button>
      </header>

      <main className="flex min-h-0 flex-1">
        {view === "doctor" ? (
          <Doctor />
        ) : view === "build" ? (
          <Build
            pendingSource={buildSource}
            onSourceTaken={() => setBuildSource(null)}
            onOpenGame={(titleId) => {
              setSelectedId(titleId);
              setView("library");
            }}
            onError={toast}
          />
        ) : view === "settings" && selected ? (
          <GameSettingsView
            key={selected.titleId}
            game={selected}
            onBack={() => setView("library")}
            onLaunched={update}
            onError={toast}
          />
        ) : games === null ? null : (
          <>
            <Library games={games} selectedId={selectedId} onSelect={setSelectedId} onAdd={pickApps} />
            {selected && (
              <GameDetail
                key={selected.titleId}
                game={selected}
                onOpenSettings={() => setView("settings")}
                onChanged={update}
                onRemoved={() => {
                  setSelectedId(null);
                  refresh();
                }}
                onError={toast}
              />
            )}
          </>
        )}
      </main>

      {dragging && (
        <div className="pointer-events-none fixed inset-3 grid place-items-center rounded-2xl border-2 border-dashed border-sky-500 bg-sky-500/10 backdrop-blur-[2px]">
          <p className="rounded-full bg-sky-500 px-4 py-2 text-sm font-medium text-white">
            Rilascia per aggiungere alla libreria
          </p>
        </div>
      )}

      <div className="fixed bottom-4 left-1/2 flex w-[min(32rem,calc(100%-2rem))] -translate-x-1/2 flex-col gap-2">
        {toasts.map((t) => (
          <p
            key={t.id}
            role="alert"
            className="rounded-lg bg-neutral-900 px-4 py-2.5 text-sm text-white shadow-lg dark:bg-neutral-100 dark:text-neutral-900"
          >
            {t.text}
          </p>
        ))}
      </div>
    </div>
  );
}

function Tab({ active, onClick, children }: { active: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      onClick={onClick}
      className={`rounded-md px-3 py-1.5 ${
        active
          ? "bg-neutral-200/70 font-medium dark:bg-neutral-800"
          : "text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-100"
      }`}
    >
      {children}
    </button>
  );
}

// Typed wrappers around the Rust commands in src-tauri/src/lib.rs.
// Keep these types in step with the Rust structs (serde camelCase).
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// ── Environment check ────────────────────────────────────────────────────────

export type Check = {
  id: string;
  label: string;
  ok: boolean;
  detail: string;
  fix: string | null;
};

export type DoctorReport = {
  checks: Check[];
  childPath: string;
};

export const doctor = () => invoke<DoctorReport>("doctor");

// ── The engine's install and the updates (install.rs, autoupdate.rs) ────────

export type EngineStatus = {
  installed: boolean;
  /** In the launcher's data folder, installed and updated by the launcher; else the project's. */
  managed: boolean;
  root: string;
};

export type InstallSnapshot = {
  running: boolean;
  step: string;
  /** 0–1, or null when the work cannot be counted. */
  frac: number | null;
  log: string[];
  error: string | null;
  finishedAt: number | null;
};

export const engineStatus = () => invoke<EngineStatus>("engine_status");
export const engineInstall = () => invoke<InstallSnapshot>("engine_install");
export const engineInstallStatus = () => invoke<InstallSnapshot | null>("engine_install_status");
export const onEngineInstall = (cb: (snap: InstallSnapshot) => void) =>
  listen<InstallSnapshot>("engine-install", (e) => cb(e.payload));

export type AppInfo = {
  version: string;
  /** The installed app: it updates itself. */
  automatic: boolean;
  /** A newer version installed over this one, waiting for a restart. */
  ready: string | null;
};
export type AppUpdate = { version: string; notes: string | null; date: string | null };
/** What the engine's automatic update did (applied, or only announced with the reason). */
export type EngineUpdate = { applied: boolean; short: string; reason: string | null; pip: boolean };

export const appInfo = () => invoke<AppInfo>("app_info");
export const appUpdateCheck = () => invoke<AppUpdate | null>("app_update_check");
/** Downloads, verifies and installs the newer version; resolves to it (null: none). */
export const appUpdateInstall = () => invoke<string | null>("app_update_install");
/** Restarts into the installed version; rejects while a build, import or install runs. */
export const appRestart = () => invoke<void>("app_restart");
export const onAppUpdateReady = (cb: (version: string) => void) =>
  listen<string>("app-update-ready", (e) => cb(e.payload));
export const onAppUpdateProgress = (cb: (done: number, total: number | null) => void) =>
  listen<[number, number | null]>("app-update-progress", (e) => cb(e.payload[0], e.payload[1]));
export const onEngineUpdate = (cb: (u: EngineUpdate) => void) =>
  listen<EngineUpdate>("engine-update", (e) => cb(e.payload));

// ── Library ──────────────────────────────────────────────────────────────────

export type ExitKind =
  | { type: "quit" }
  | { type: "stopped" }
  | { type: "gpuRevoked" }
  | { type: "launcherError" }
  | { type: "crashed"; signal: number }
  | { type: "other" };

export type ExitInfo = {
  kind: ExitKind;
  code: number | null;
  /** Unix seconds */
  at: number;
  seconds: number;
};

export type Game = {
  titleId: string;
  name: string;
  version: string;
  bundleId: string;
  appPath: string;
  iconPath: string | null;
  addedAt: number;
  lastPlayed: number | null;
  playSeconds: number;
  lastExit: ExitInfo | null;
  /** Set while the game runs (Unix seconds). */
  runningSince: number | null;
  /** What the running game is doing, read from its log (null: not running). */
  status: GameStatus | null;
  /** The engine commit this launcher built it with (null: built elsewhere). */
  builtWith: string | null;
  /** The game files it was built from, for "Ricompila". */
  sourcePath: string | null;
  /** The options of that build, which "Ricompila" uses again. */
  buildOptions: BuildOptions | null;
  /** The cover found on the internet; shown instead of the app's icon. */
  coverPath: string | null;
  /** nlib: by title ID (titledb) · nintendoEu: Nintendo of Europe's eShop, by name. */
  coverSource: "nlib" | "nintendoEu" | null;
  /** When the sources were last asked (Unix seconds), found or not. */
  coverChecked: number | null;
};

/**
 * starting: the app's launcher runs, the game's code not yet ·
 * loading: the game's code runs, nothing on screen yet ·
 * preparing: the progress screen (shaders and pipelines before the first frame) ·
 * playing: a frame has been shown.
 */
export type GamePhase = "starting" | "loading" | "preparing" | "playing";

export type GameStatus = {
  phase: GamePhase;
  /** No pipeline list of this Mac yet: the first start, the long one. */
  firstLaunch: boolean;
  /** When the preparation started (Unix seconds). */
  prepSince: number | null;
  /** Another window covers the game's: macOS slows it down meanwhile. */
  windowCovered: boolean;
  /** Distinct shaders the game asked for that the app does not carry. */
  missingShaders: number;
};

export type ImportReport = { added: Game[]; errors: string[] };
export type RevealTarget = "app" | "log" | "saves";

export const libraryList = () => invoke<Game[]>("library_list");
export const libraryImport = (paths: string[]) =>
  invoke<ImportReport>("library_import", { paths });
export const libraryRemove = (titleId: string) =>
  invoke<void>("library_remove", { titleId });

/** `overrides`: settings for this start only (launch environment, not saved). */
export const gameLaunch = (
  titleId: string,
  settingsWindow: boolean,
  overrides?: Record<string, string>,
) => invoke<Game>("game_launch", { titleId, settingsWindow, overrides });
export const gameStop = (titleId: string) => invoke<void>("game_stop", { titleId });
export const gameReveal = (titleId: string, target: RevealTarget) =>
  invoke<void>("game_reveal", { titleId, target });
/** Asks the cover sources now; rejects with a message when none has the game. */
export const gameFindCover = (titleId: string) => invoke<Game>("game_find_cover", { titleId });

/** A game's build folder (<title ID>_build): the script's cache, which the app does not need. */
export type BuildFolder = {
  path: string;
  sizeBytes: number;
  /** About what deleting it gives back: on the app's APFS volume the app shares the rest. */
  freesBytes: number;
  /** The app's files are clones of the build's (same APFS volume). */
  shared: boolean;
};

/** null: the game has no build folder (freed, or built elsewhere). */
export const gameBuildFolder = (titleId: string) => invoke<BuildFolder | null>("game_build_folder", { titleId });
/** Deletes the game's build folder (its provisional profile stays); resolves to about the bytes freed. */
export const gameFreeBuildFolder = (titleId: string) => invoke<number>("game_free_build_folder", { titleId });

/** Emitted by Rust when a game this launcher started exits. */
export const onGameExited = (cb: (game: Game) => void) =>
  listen<Game>("game-exited", (e) => cb(e.payload));

/** Emitted by Rust when what a running game is doing changes (read from its log). */
export const onGameStatus = (cb: (titleId: string, status: GameStatus) => void) =>
  listen<{ titleId: string; status: GameStatus }>("game-status", (e) =>
    cb(e.payload.titleId, e.payload.status),
  );

// ── Settings (docs/frontends.md, "The settings list") ───────────────────────

export type Setting = {
  env: string;
  section: string;
  label: string;
  note: string;
  /** The tested default, as the value the variable takes. */
  default: string;
  /** "basic": shown first. */
  level?: string;
  /** "<VAR>=<value>": drawn only while the row that sets <VAR> has that value. */
  needs?: string;
  /** Render scale only: the 3D views' size the scale multiplies. */
  base?: { docked: [number, number]; handheld: [number, number] };
} & (
  | { kind: "choice"; options: [string, string][] }
  | { kind: "range"; min: number; max: number; step: number }
  | { kind: "toggle"; on: string; off: string }
);

/** A setting an env.txt sets: the game uses it whatever the saved choice. */
export type Fixed = { value: string; source: "player" | "app" };

export type GameSettings = {
  /** 0 when the app carries no settings list. */
  schema: number;
  supported: boolean;
  settings: Setting[];
  /** Saved choices this build offers (only those that differ from the default). */
  saved: Record<string, string>;
  fixed: Record<string, Fixed>;
};

export const settingsGet = (titleId: string) =>
  invoke<GameSettings>("settings_get", { titleId });
/** Saves the choices; a value at its tested default removes the saved key. */
export const settingsSave = (titleId: string, choices: Record<string, string>) =>
  invoke<GameSettings>("settings_save", { titleId, choices });

// ── Launcher settings ───────────────────────────────────────────────────────

export type Config = {
  prodKeys: string | null;
  cloudSavesFolder: string | null;
  italianPrompts: boolean;
  /** Look up covers on the internet (title ID to Nlib, name to Nintendo's eShop search). */
  covers: boolean;
  /** The archive: one folder per game with its base game, updates and DLC. */
  archiveFolder: string | null;
  /** Download titledb's list to say which updates and DLC the archive lacks. */
  titledb: boolean;
  /** The installed app updates itself from the launcher's GitHub releases. */
  appAutoUpdate: boolean;
  /** The installed app's engine follows nxnative's main (not updates that break the launcher). */
  engineAutoUpdate: boolean;
};
export const configGet = () => invoke<Config>("config_get");
export const configSetCovers = (on: boolean) => invoke<Config>("config_set_covers", { on });
/** Remembers only the path of prod.keys; `null` forgets it. */
export const configSetKeys = (path: string | null) => invoke<Config>("config_set_keys", { path });
/** A folder path, "icloud" for iCloud Drive › nxnative Saves, or null to turn cloud saves off. */
export const configSetCloudFolder = (path: string | null) =>
  invoke<Config>("config_set_cloud_folder", { path });
export const configSetItalianPrompts = (on: boolean) =>
  invoke<Config>("config_set_italian_prompts", { on });
/** The archive's folder; null forgets it (its files stay). */
export const configSetArchive = (path: string | null) => invoke<Config>("config_set_archive", { path });
export const configSetTitledb = (on: boolean) => invoke<Config>("config_set_titledb", { on });
export const configSetAutoUpdate = (appOn: boolean, engineOn: boolean) =>
  invoke<Config>("config_set_auto_update", { appOn, engineOn });

// ── The archive (archive.rs, importer.rs, titledb.rs) ───────────────────────

export type ArchivePiece = {
  kind: "base" | "update" | "dlc";
  titleId: string;
  version: number;
  /** The file it is in; empty for the dump in the game's folder itself. */
  file: string;
};

export type ArchiveGame = {
  folder: string;
  /** The base game's title ID; empty when no file says it. */
  titleId: string;
  name: string;
  sizeBytes: number;
  /** What a build of the folder reads: packages over a NUP over a dump. */
  format: "package" | "nup" | "dump" | null;
  pieces: ArchivePiece[];
  hasBase: boolean;
  /** The newest update in the folder. */
  update: number | null;
  dlc: string[];
  /** titledb lists the game; the three fields below are only filled then. */
  known: boolean;
  latestUpdate: number | null;
  updateMissing: boolean;
  missingDlc: string[];
  issues: string[];
};

export type ArchiveView = {
  root: string | null;
  games: ArchiveGame[];
  /** Game files lying in the archive's folder itself. */
  loose: string[];
  titledbOn: boolean;
  /** When titledb's list was downloaded (Unix seconds). */
  titledbAt: number | null;
  /** Older than a day, or missing. */
  titledbStale: boolean;
};

export type ImportLine = { text: string; tone: "info" | "ok" | "warn" | "error" };

export type ImportSnapshot = {
  running: boolean;
  cancelled: boolean;
  moving: boolean;
  step: string;
  /** 0–1, or null when the work cannot be measured. */
  frac: number | null;
  log: ImportLine[];
  games: string[];
  added: number;
  skipped: number;
  failed: number;
  startedAt: number;
  finishedAt: number | null;
};

export const archiveList = () => invoke<ArchiveView>("archive_list");
/** Downloads titledb's whole list (about 3 MB). */
export const titledbUpdate = () => invoke<void>("titledb_update");
/** Files, folders, .zip / .rar / .7z into the archive; `moving` leaves no copy behind. */
export const archiveImport = (paths: string[], moving: boolean) =>
  invoke<ImportSnapshot>("archive_import", { paths, moving });
export const archiveImportStatus = () => invoke<ImportSnapshot | null>("archive_import_status");
export const archiveImportCancel = () => invoke<void>("archive_import_cancel");
export const archiveImportDismiss = () => invoke<void>("archive_import_dismiss");
/** Opens the archive's folder, or a game's (worked out in Rust from the title ID). */
export const archiveReveal = (titleId?: string) => invoke<void>("archive_reveal", { titleId: titleId ?? null });

export const onArchiveImport = (cb: (snap: ImportSnapshot) => void) =>
  listen<ImportSnapshot>("archive-import", (e) => cb(e.payload));
export const onArchiveChanged = (cb: () => void) => listen("archive-changed", () => cb());

// ── The engine's version (docs/frontends.md, "Following main") ──────────────

export type EngineState = {
  commit: string;
  short: string;
  date: string;
  subject: string;
  modified: boolean;
  previous: string | null;
};

export type FrontEndNote = { commit: string; kind: "break" | "interface" | "new" | "note"; text: string };

export type UpdateInfo = {
  from: string;
  to: string;
  toShort: string;
  upToDate: boolean;
  diverged: boolean;
  totalCommits: number;
  commits: { short: string; date: string; subject: string }[];
  notes: FrontEndNote[];
  frontendsDoc: boolean;
  schemaDiff: string | null;
  settingsList: boolean;
  scriptOptions: string[];
  envSwitches: boolean;
  shaderChain: boolean;
  requirements: boolean;
  metalNeeded: string | null;
};

export type Applied = {
  commit: string;
  short: string;
  date: string;
  subject: string;
  modified: boolean;
  previous: string;
  pip: string | null;
};

export const engineInfo = () => invoke<EngineState>("engine_info");
/** Fetches nxnative's main and says what moving to it changes. */
export const engineCheck = () => invoke<UpdateInfo>("engine_check");
/** To main's fetched commit, or back to the one before the last update. */
export const engineApply = (rollback: boolean) => invoke<Applied>("engine_apply", { rollback });
export const engineRebuildHints = () => invoke<Record<string, string[]>>("engine_rebuild_hints");

// ── Cheats (docs/frontends.md, "Cheats") ────────────────────────────────────

export type CheatEntry = {
  /** As <build ID>.enabled writes it: "Name", "Name #2", … */
  key: string;
  name: string;
  kind: "master" | "cheat" | "label";
  section: string | null;
};

export type CheatFile = {
  buildId: string;
  /** The file of the build this app runs. */
  current: boolean;
  entries: CheatEntry[];
  enabled: string[];
  error: string | null;
};

export type CheatsView = {
  folder: string;
  buildId: string | null;
  files: CheatFile[];
  masterOn: boolean;
  safeMode: boolean;
};

export const cheatsGet = (titleId: string) => invoke<CheatsView>("cheats_get", { titleId });
export const cheatsSet = (titleId: string, buildId: string, enabled: string[]) =>
  invoke<CheatsView>("cheats_set", { titleId, buildId, enabled });
export const cheatsMaster = (titleId: string, on: boolean) => invoke<CheatsView>("cheats_master", { titleId, on });
export const cheatsImport = (titleId: string, path: string) => invoke<CheatsView>("cheats_import", { titleId, path });
export const cheatsReveal = (titleId: string) => invoke<void>("cheats_reveal", { titleId });

// ── Building (docs/frontends.md, "Building: the NSP script") ────────────────

export type SourceInfo = {
  path: string;
  kind: "nsp" | "nup" | "dump";
  name: string;
  needsKeys: boolean;
  sizeBytes: number;
  files: string[];
  buildDir: string;
  previousBuild: boolean;
  /** Read from the file names, when they carry it. */
  titleId: string | null;
};

export type Preflight = {
  source: SourceInfo;
  keys: string | null;
  /** Shaders the profile lists for --seen-shaders (null: no list, or game not known yet). */
  seenList: number | null;
  freeBytes: number;
  neededBytes: number;
  blockers: string[];
  warnings: string[];
};

export type Stage = {
  k: number;
  name: string;
  status: "running" | "ok" | "warn" | "failed" | "stopped";
  summary: string;
  elapsed: number | null;
};

export type BuildProgress = {
  done: number | null;
  total: number | null;
  unit: string;
  est: boolean;
  note: string;
  /** 0–0.99, or null when the work cannot be counted. */
  frac: number | null;
  eta: number | null;
  elapsed: number;
};

export type BuildOptions = {
  /** "mac": a Mac app for the library; "ios": an IPA to sideload. */
  target: "mac" | "ios";
  /** IPA only: --romfs separate, for a game too big to install. */
  romfsSeparate: boolean;
  /** Mac only: --seen-shaders, only the shaders the playtests used. */
  seenShaders: boolean;
};

export const DEFAULT_BUILD_OPTIONS: BuildOptions = { target: "mac", romfsSeparate: false, seenShaders: false };

export type BuildSnapshot = {
  source: SourceInfo;
  options: BuildOptions;
  status: "running" | "succeeded" | "failed" | "cancelled";
  startedAt: number;
  finishedAt: number | null;
  error: string | null;
  stages: Stage[];
  totalStages: number;
  step: string;
  progress: BuildProgress | null;
  game: {
    titleId: string;
    name: string;
    version: string;
    out: string | null;
    icon: string | null;
  } | null;
  messages: { t: number; text: string; red: boolean }[];
  stderr: string[];
  imported: string | null;
};

export const buildPreflight = (path: string, options: BuildOptions) =>
  invoke<Preflight>("build_preflight", { path, options });
export const buildStart = (path: string, options: BuildOptions) =>
  invoke<BuildSnapshot>("build_start", { path, options });
/** Shows the file the last build wrote (the IPA, or the Mac app) in Finder. */
export const buildRevealOutput = () => invoke<void>("build_reveal_output");
export const buildStatus = () => invoke<BuildSnapshot | null>("build_status");
export const buildCancel = () => invoke<void>("build_cancel");
export const buildDismiss = () => invoke<void>("build_dismiss");
/** Opens the build's logs folder (worked out in Rust from the current build). */
export const buildReveal = () => invoke<void>("build_reveal");

export const onBuildUpdated = (cb: (snap: BuildSnapshot) => void) =>
  listen<BuildSnapshot>("build-updated", (e) => cb(e.payload));
export const onLibraryChanged = (cb: () => void) => listen("library-changed", () => cb());

export const fileSrc = (path: string) => convertFileSrc(path);

/** The game's picture, served by Tauri's asset protocol: its cover when one
 * was found, else the cached PNG of the app's icon. */
export const iconSrc = (game: Pick<Game, "coverPath" | "iconPath">) => {
  const path = game.coverPath ?? game.iconPath;
  return path ? convertFileSrc(path) : null;
};

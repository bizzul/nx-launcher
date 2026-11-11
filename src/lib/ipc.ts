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

/** Emitted by Rust when a game this launcher started exits. */
export const onGameExited = (cb: (game: Game) => void) =>
  listen<Game>("game-exited", (e) => cb(e.payload));

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

export type Config = { prodKeys: string | null };
export const configGet = () => invoke<Config>("config_get");
/** Remembers only the path of prod.keys; `null` forgets it. */
export const configSetKeys = (path: string | null) => invoke<Config>("config_set_keys", { path });

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
};

export type Preflight = {
  source: SourceInfo;
  keys: string | null;
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

export type BuildSnapshot = {
  source: SourceInfo;
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

export const buildPreflight = (path: string) => invoke<Preflight>("build_preflight", { path });
export const buildStart = (path: string) => invoke<BuildSnapshot>("build_start", { path });
export const buildStatus = () => invoke<BuildSnapshot | null>("build_status");
export const buildCancel = () => invoke<void>("build_cancel");
export const buildDismiss = () => invoke<void>("build_dismiss");
/** Opens the build's logs folder (worked out in Rust from the current build). */
export const buildReveal = () => invoke<void>("build_reveal");

export const onBuildUpdated = (cb: (snap: BuildSnapshot) => void) =>
  listen<BuildSnapshot>("build-updated", (e) => cb(e.payload));
export const onLibraryChanged = (cb: () => void) => listen("library-changed", () => cb());

export const fileSrc = (path: string) => convertFileSrc(path);

/** The cached PNG icon, served by Tauri's asset protocol. */
export const iconSrc = (game: Game) =>
  game.iconPath ? convertFileSrc(game.iconPath) : null;

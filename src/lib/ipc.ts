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

/** The cached PNG icon, served by Tauri's asset protocol. */
export const iconSrc = (game: Game) =>
  game.iconPath ? convertFileSrc(game.iconPath) : null;

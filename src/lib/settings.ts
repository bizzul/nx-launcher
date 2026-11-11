// The rules for drawing a settings list (docs/frontends.md, "The settings list").
import type { GameSettings, Setting } from "./ipc";

export type Choices = Record<string, string>;

/** Decimals a range shows: as many as its step has (0.05 → 2). */
export const decimals = (step: number) => (String(step).split(".")[1] ?? "").length;

/** Two values of a setting are the same choice; for a range, "1" is "1.0". */
export function same(s: Setting, a: string | undefined, b: string | undefined) {
  if (a === undefined || b === undefined) return a === b;
  if (s.kind === "range") return Math.abs(parseFloat(a) - parseFloat(b)) < s.step / 2;
  return a === b;
}

/** What the game would use for each setting: env file, then saved choice, then default. */
export function effective(data: GameSettings): Choices {
  return Object.fromEntries(
    data.settings.map((s) => [s.env, data.fixed[s.env]?.value ?? data.saved[s.env] ?? s.default]),
  );
}

/** Sections in the order of their first rows, each with its rows in list order. */
export function sections(rows: Setting[]): [string, Setting[]][] {
  const out = new Map<string, Setting[]>();
  for (const s of rows) out.set(s.section, [...(out.get(s.section) ?? []), s]);
  return [...out];
}

/** Whether a row is drawn: its `needs` holds in the current choices. A row
 *  naming a variable the list does not have stays drawn. */
export function needsMet(s: Setting, all: Setting[], choices: Choices) {
  if (!s.needs) return true;
  const [env, value] = s.needs.split("=", 2) as [string, string];
  if (!all.some((r) => r.env === env)) return true;
  return choices[env] === value;
}

/** The rows that differ between two sets of choices (env files' rows never do). */
export function changed(data: GameSettings, a: Choices, b: Choices) {
  return data.settings.filter((s) => !data.fixed[s.env] && !same(s, a[s.env], b[s.env]));
}

export const SECTION_NAMES: Record<string, string> = {
  Game: "Gioco",
  Controls: "Controlli",
  Performance: "Prestazioni",
};

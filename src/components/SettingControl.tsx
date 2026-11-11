import { decimals } from "../lib/settings";
import type { Setting } from "../lib/ipc";

type Props = {
  setting: Setting;
  value: string;
  disabled?: boolean;
  onChange: (value: string) => void;
};

/** The control for one row: segmented buttons or a menu for a choice, a
 *  slider for a range, a switch for a toggle. */
export function SettingControl({ setting: s, value, disabled, onChange }: Props) {
  switch (s.kind) {
    case "choice":
      if (s.options.length <= 3) {
        return (
          <div className="inline-flex rounded-lg bg-neutral-200/70 p-0.5 dark:bg-neutral-800" role="radiogroup">
            {s.options.map(([v, label]) => (
              <button
                key={v}
                role="radio"
                aria-checked={value === v}
                disabled={disabled}
                onClick={() => onChange(v)}
                className={`rounded-md px-3 py-1 text-sm transition-colors disabled:opacity-60 ${
                  value === v
                    ? "bg-white font-medium shadow-sm dark:bg-neutral-600"
                    : "text-neutral-600 hover:text-neutral-900 dark:text-neutral-400 dark:hover:text-neutral-100"
                }`}
              >
                {label}
              </button>
            ))}
          </div>
        );
      }
      return (
        <select
          value={value}
          disabled={disabled}
          onChange={(e) => onChange(e.target.value)}
          className="rounded-md border border-neutral-300 bg-white px-2 py-1 text-sm disabled:opacity-60 dark:border-neutral-700 dark:bg-neutral-900"
        >
          {s.options.map(([v, label]) => (
            <option key={v} value={v}>
              {label}
            </option>
          ))}
        </select>
      );

    case "range": {
      const n = parseFloat(value);
      const fmt = (x: number) => x.toFixed(decimals(s.step));
      return (
        <div className="flex w-64 flex-col items-end gap-1">
          <div className="flex w-full items-center gap-3">
            <input
              type="range"
              min={s.min}
              max={s.max}
              step={s.step}
              value={Number.isFinite(n) ? n : parseFloat(s.default)}
              disabled={disabled}
              onChange={(e) => onChange(fmt(parseFloat(e.target.value)))}
              className="flex-1 accent-sky-500 disabled:opacity-60"
            />
            <span className="w-12 text-right font-mono text-sm tabular-nums">{fmt(n)}</span>
          </div>
          {s.base && (
            <span className="font-mono text-xs text-neutral-500">
              {size(s.base.docked, n)} TV · {size(s.base.handheld, n)} portatile
            </span>
          )}
        </div>
      );
    }

    case "toggle": {
      const on = value === s.on;
      return (
        <button
          role="switch"
          aria-checked={on}
          disabled={disabled}
          onClick={() => onChange(on ? s.off : s.on)}
          className={`relative h-6 w-10 shrink-0 rounded-full transition-colors disabled:opacity-60 ${
            on ? "bg-sky-500" : "bg-neutral-300 dark:bg-neutral-700"
          }`}
        >
          <span
            className={`absolute top-0.5 left-0.5 size-5 rounded-full bg-white shadow transition-transform ${
              on ? "translate-x-4" : ""
            }`}
          />
        </button>
      );
    }
  }
}

const size = ([w, h]: [number, number], scale: number) =>
  `${Math.round(w * scale)}×${Math.round(h * scale)}`;

/** A value as the player reads it: an option's label, "Sì"/"No", or the number. */
export function describe(s: Setting, value: string) {
  if (s.kind === "choice") return s.options.find(([v]) => v === value)?.[1] ?? value;
  if (s.kind === "toggle") return value === s.on ? "Sì" : value === s.off ? "No" : value;
  return value;
}

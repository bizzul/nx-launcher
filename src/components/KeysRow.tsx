/** Where prod.keys is (only the path is kept), with pick and forget. */
export function KeysRow({ keys, onPick, onClear }: { keys: string | null; onPick: () => void; onClear: () => void }) {
  return (
    <div className="mt-6 flex items-center gap-3 rounded-lg border border-neutral-200 px-4 py-3 text-sm dark:border-neutral-800">
      <div className="min-w-0 flex-1">
        <p className="font-medium">prod.keys</p>
        <p className="truncate font-mono text-xs text-neutral-500" title={keys ?? ""}>
          {keys ?? "non impostato · serve solo per NSP, XCI e NSZ (non per NUP e dump)"}
        </p>
      </div>
      <button onClick={onPick} className="btn">
        {keys ? "Cambia…" : "Scegli…"}
      </button>
      {keys && (
        <button onClick={onClear} className="text-xs text-neutral-500 hover:text-red-600">
          Dimentica
        </button>
      )}
    </div>
  );
}

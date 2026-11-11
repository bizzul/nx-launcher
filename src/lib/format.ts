import type { ExitInfo } from "./ipc";

export function formatDuration(seconds: number): string {
  if (seconds < 60) return "meno di un minuto";
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return h ? `${h} h ${m} min` : `${m} min`;
}

/** "oggi, 14:30" · "ieri, 09:12" · "3 nov" */
export function formatWhen(unix: number): string {
  const d = new Date(unix * 1000);
  const time = d.toLocaleTimeString("it", { hour: "2-digit", minute: "2-digit" });
  const days = Math.round((startOfDay(new Date()) - startOfDay(d)) / 86_400_000);
  if (days === 0) return `oggi, ${time}`;
  if (days === 1) return `ieri, ${time}`;
  return d.toLocaleDateString("it", { day: "numeric", month: "short" });
}

const startOfDay = (d: Date) =>
  new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();

const SIGNALS: Record<number, string> = {
  6: "abort",
  9: "kill",
  10: "bus error",
  11: "segmentation fault",
  15: "terminate",
};

export type Tone = "neutral" | "warn" | "error";

/** What the player is told about how the last session ended. */
export function exitMessage(exit: ExitInfo): { tone: Tone; text: string } {
  const k = exit.kind;
  switch (k.type) {
    case "quit":
      return { tone: "neutral", text: `Chiuso normalmente dopo ${formatDuration(exit.seconds)}.` };
    case "stopped":
      return { tone: "neutral", text: "Terminato dal launcher." };
    case "gpuRevoked":
      return {
        tone: "warn",
        text: "macOS ha interrotto il lavoro del gioco sulla GPU (codice 70). Riavvialo; se si ripete, guarda il log.",
      };
    case "launcherError":
      return {
        tone: "error",
        text: "L'app si è fermata prima di avviare il gioco (codice 1): di solito mancano dei file e va ricompilata. Il log dice il motivo.",
      };
    case "crashed": {
      const name = SIGNALS[k.signal];
      return {
        tone: "error",
        text: `Il gioco si è chiuso per un errore (segnale ${k.signal}${name ? ` · ${name}` : ""}). Apri il log per i dettagli.`,
      };
    }
    case "other":
      return { tone: "warn", text: `Uscito con codice ${exit.code ?? "?"}.` };
  }
}

import { useCallback, useEffect, useState } from "react";
import { libraryList, onGameExited, type Game } from "../lib/ipc";

/** The library, kept current with the games' exits. */
export function useLibrary() {
  const [games, setGames] = useState<Game[] | null>(null);

  const refresh = useCallback(async () => {
    setGames(await libraryList());
  }, []);

  /** Replaces one entry with a fresher copy from Rust. */
  const update = useCallback((game: Game) => {
    setGames((gs) => gs?.map((g) => (g.titleId === game.titleId ? game : g)) ?? gs);
  }, []);

  useEffect(() => {
    refresh();
    const unlisten = onGameExited(update);
    return () => {
      unlisten.then((off) => off());
    };
  }, [refresh, update]);

  return { games, refresh, update };
}

/** The current time in Unix seconds, ticking every `ms`. */
export function useNow(ms: number) {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now() / 1000), ms);
    return () => clearInterval(t);
  }, [ms]);
  return now;
}

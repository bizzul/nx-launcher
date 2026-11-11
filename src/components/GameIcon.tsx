import { iconSrc, type Game } from "../lib/ipc";

/** The game's icon, or its initials on a tinted tile when it was packed without one. */
export function GameIcon({ game, className = "" }: { game: Game; className?: string }) {
  const src = iconSrc(game);
  if (src) {
    return (
      <img
        src={src}
        alt=""
        draggable={false}
        className={`aspect-square rounded-[22%] object-cover ${className}`}
      />
    );
  }
  const initials = game.name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]!.toUpperCase())
    .join("");
  const hue = [...game.titleId].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 0);
  // @container lets the initials scale with the tile (cqw = 1% of its width).
  return (
    <div className={`@container aspect-square ${className}`}>
      <div
        className="grid size-full place-items-center rounded-[22%] font-semibold text-white"
        style={{ background: `linear-gradient(135deg, hsl(${hue} 55% 45%), hsl(${(hue + 40) % 360} 60% 30%))` }}
      >
        <span className="text-[30cqw] leading-none">{initials}</span>
      </div>
    </div>
  );
}

import clsx from "clsx";

export function Aura({ size = 30, active = false }: { size?: number; active?: boolean }) {
  return (
    <span className={clsx("aura", active && "is-active")} style={{ width: size, height: size }}>
      <svg viewBox="0 0 48 48" width={size} height={size} aria-hidden="true">
        <circle className="aura-ring r3" cx="24" cy="24" r="21" />
        <circle className="aura-ring r2" cx="24" cy="24" r="15" />
        <circle className="aura-ring r1" cx="24" cy="24" r="9" />
        <circle className="aura-core" cx="24" cy="24" r="4.6" />
        <circle className="aura-spark" cx="22.2" cy="22.2" r="1.5" />
      </svg>
    </span>
  );
}

import clsx from "clsx";

export interface AuraProps {
  /** Box size in px (the SVG scales to its box). */
  size?: number;
  /** Listening/active state — lights the rings to neon and breathes. */
  active?: boolean;
  /** Drop the outermost ring for tight spaces (menubar/inline). */
  mini?: boolean;
}

/**
 * The Aura — Atmospeak's companion mark: concentric breathing rings around a
 * bright core (sound radiating into the atmosphere). Deliberately distinct from
 * the sibling "Sanctuary" product's solid moon-orb. Recolors via CSS variables;
 * shared by the dock, hub brand/hero, and onboarding rail/hero.
 */
export function Aura({ size = 30, active = false, mini = false }: AuraProps) {
  return (
    <span className={clsx("aura", active && "is-active")} style={{ width: size, height: size }}>
      <svg viewBox="0 0 48 48" width={size} height={size} aria-hidden="true">
        {!mini && <circle className="aura-ring r3" cx="24" cy="24" r="21" />}
        <circle className="aura-ring r2" cx="24" cy="24" r="15" />
        <circle className="aura-ring r1" cx="24" cy="24" r="9" />
        <circle className="aura-core" cx="24" cy="24" r="4.6" />
        <circle className="aura-spark" cx="22.2" cy="22.2" r="1.5" />
      </svg>
    </span>
  );
}

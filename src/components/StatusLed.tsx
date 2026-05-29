import clsx from "clsx";

type StatusLedTone = "idle" | "good" | "warn" | "hot";

interface StatusLedProps {
  tone: StatusLedTone;
  label: string;
}

export function StatusLed({ tone, label }: StatusLedProps) {
  return (
    <span className="status-led">
      <span className={clsx("status-led__light", `status-led__light--${tone}`)} />
      <span>{label}</span>
    </span>
  );
}

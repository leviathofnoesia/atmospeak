import type { ReactNode } from "react";

interface ToggleRowProps {
  icon: ReactNode;
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

export function ToggleRow({ icon, label, checked, onChange }: ToggleRowProps) {
  return (
    <label className="toggle-row">
      {icon}
      <span>{label}</span>
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.currentTarget.checked)}
      />
    </label>
  );
}

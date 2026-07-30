import type { ReactNode } from "react";

interface ToggleRowProps {
  icon: ReactNode;
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}

export function ToggleRow({ icon, label, checked, onChange, disabled }: ToggleRowProps) {
  return (
    <label className="toggle-row">
      {icon}
      <span>{label}</span>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.currentTarget.checked)}
      />
    </label>
  );
}

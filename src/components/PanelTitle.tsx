import type { ReactNode } from "react";

interface PanelTitleProps {
  icon: ReactNode;
  title: string;
}

export function PanelTitle({ icon, title }: PanelTitleProps) {
  return (
    <div className="panel-title">
      {icon}
      <h2>{title}</h2>
    </div>
  );
}

import clsx from "clsx";
import { Trash2 } from "lucide-react";

interface EditableRowProps {
  title: string;
  body: string;
  enabled: boolean;
  onToggle: () => void;
  onDelete: () => void;
}

export function EditableRow({ title, body, enabled, onToggle, onDelete }: EditableRowProps) {
  return (
    <article className={clsx("editable-row", !enabled && "is-muted")}>
      <div>
        <strong>{title}</strong>
        <p>{body}</p>
      </div>
      <div className="row-actions">
        <button className="button button--ghost" type="button" onClick={onToggle}>
          {enabled ? "On" : "Off"}
        </button>
        <button
          className="button button--ghost button--square"
          type="button"
          onClick={onDelete}
          aria-label="Delete"
          title="Delete"
        >
          <Trash2 size={17} />
        </button>
      </div>
    </article>
  );
}

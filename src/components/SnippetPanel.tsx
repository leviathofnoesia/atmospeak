import type { FormEvent } from "react";
import type { Snippet } from "../types/dictation";
import { EditableRow } from "./EditableRow";

interface SnippetPanelProps {
  snippets: Snippet[];
  draft: { id: string | null; trigger: string; body: string };
  setDraft: (draft: { id: string | null; trigger: string; body: string }) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => Promise<void>;
  onToggle: (snippet: Snippet) => Promise<void>;
  onDelete: (snippet: Snippet) => Promise<void>;
}

export function SnippetPanel({
  snippets,
  draft,
  setDraft,
  onSubmit,
  onToggle,
  onDelete,
}: SnippetPanelProps) {
  return (
    <div>
      <div className="hub__head">
        <div className="kick">P.04 / Snippets — say a little, paste a lot</div>
        <h1>Voice <em>triggers.</em></h1>
      </div>
      <section className="list-panel">
        <form className="inline-form" onSubmit={(event) => void onSubmit(event)}>
        <input
          value={draft.trigger}
          onChange={(event) => setDraft({ ...draft, trigger: event.currentTarget.value })}
          placeholder="spoken trigger"
        />
        <input
          value={draft.body}
          onChange={(event) => setDraft({ ...draft, body: event.currentTarget.value })}
          placeholder="expanded text"
        />
        <button className="button button--primary" type="submit">
          {draft.id ? "Save" : "Add"}
        </button>
        {draft.id ? (
          <button
            className="button button--ghost"
            type="button"
            onClick={() => setDraft({ id: null, trigger: "", body: "" })}
          >
            Cancel
          </button>
        ) : null}
        </form>
        {snippets.map((snippet) => (
          <EditableRow
            key={snippet.id}
            title={snippet.trigger}
            body={snippet.body}
            enabled={snippet.enabled}
            onEdit={() =>
              setDraft({
                id: snippet.id,
                trigger: snippet.trigger,
                body: snippet.body,
              })
            }
            onToggle={() => void onToggle(snippet)}
            onDelete={() => void onDelete(snippet)}
          />
        ))}
      </section>
    </div>
  );
}

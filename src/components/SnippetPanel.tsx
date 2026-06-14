import { Scissors } from "lucide-react";
import type { FormEvent } from "react";
import type { Snippet } from "../types/dictation";
import { EditableRow } from "./EditableRow";
import { PanelTitle } from "./PanelTitle";

interface SnippetPanelProps {
  snippets: Snippet[];
  draft: { trigger: string; body: string };
  setDraft: (draft: { trigger: string; body: string }) => void;
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
    <section className="list-panel">
      <PanelTitle icon={<Scissors size={22} />} title="Voice snippets" />
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
          Add
        </button>
      </form>
      {snippets.map((snippet) => (
        <EditableRow
          key={snippet.id}
          title={snippet.trigger}
          body={snippet.body}
          enabled={snippet.enabled}
          onToggle={() => void onToggle(snippet)}
          onDelete={() => void onDelete(snippet)}
        />
      ))}
    </section>
  );
}

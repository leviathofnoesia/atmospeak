import type { FormEvent } from "react";
import type { DictionaryEntry } from "../types/dictation";
import { EditableRow } from "./EditableRow";

interface DictionaryPanelProps {
  entries: DictionaryEntry[];
  draft: { id: string | null; phrase: string; replacement: string };
  setDraft: (draft: { id: string | null; phrase: string; replacement: string }) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => Promise<void>;
  onToggle: (entry: DictionaryEntry) => Promise<void>;
  onDelete: (entry: DictionaryEntry) => Promise<void>;
}

export function DictionaryPanel({
  entries,
  draft,
  setDraft,
  onSubmit,
  onToggle,
  onDelete,
}: DictionaryPanelProps) {
  return (
    <div>
      <div className="hub__head">
        <div className="kick">P.03 / Dictionary — your words, spelled your way</div>
        <h1>It learns your <em>names.</em></h1>
      </div>
      <section className="list-panel">
        <form className="inline-form" onSubmit={(event) => void onSubmit(event)}>
        <input
          value={draft.phrase}
          onChange={(event) => setDraft({ ...draft, phrase: event.currentTarget.value })}
          placeholder="heard phrase"
        />
        <input
          value={draft.replacement}
          onChange={(event) => setDraft({ ...draft, replacement: event.currentTarget.value })}
          placeholder="replacement"
        />
        <button className="button button--primary" type="submit">
          {draft.id ? "Save" : "Add"}
        </button>
        {draft.id ? (
          <button
            className="button button--ghost"
            type="button"
            onClick={() => setDraft({ id: null, phrase: "", replacement: "" })}
          >
            Cancel
          </button>
        ) : null}
        </form>
        {entries.map((entry) => (
          <EditableRow
            key={entry.id}
            title={entry.phrase}
            body={entry.replacement}
            enabled={entry.enabled}
            onEdit={() =>
              setDraft({
                id: entry.id,
                phrase: entry.phrase,
                replacement: entry.replacement,
              })
            }
            onToggle={() => void onToggle(entry)}
            onDelete={() => void onDelete(entry)}
          />
        ))}
      </section>
    </div>
  );
}

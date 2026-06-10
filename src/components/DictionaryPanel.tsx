import { BookOpen } from "lucide-react";
import type { FormEvent } from "react";
import type { DictionaryEntry } from "../types/dictation";
import { EditableRow } from "./EditableRow";
import { PanelTitle } from "./PanelTitle";

interface DictionaryPanelProps {
  entries: DictionaryEntry[];
  draft: { phrase: string; replacement: string };
  setDraft: (draft: { phrase: string; replacement: string }) => void;
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
    <section className="list-panel">
      <PanelTitle icon={<BookOpen size={22} />} title="Custom dictionary" />
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
          Add
        </button>
      </form>
      {entries.map((entry) => (
        <EditableRow
          key={entry.id}
          title={entry.phrase}
          body={entry.replacement}
          enabled={entry.enabled}
          onToggle={() => void onToggle(entry)}
          onDelete={() => void onDelete(entry)}
        />
      ))}
    </section>
  );
}

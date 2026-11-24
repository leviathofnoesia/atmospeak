import { useState } from "react";
import { copyText } from "../lib/api";
import type { TranscriptSession } from "../types/dictation";

export function TranscriptVersions({ session }: { session: TranscriptSession }) {
  const versions = [
    { id: "original", label: "Original", text: session.rawText },
    { id: "cleaned", label: "Cleaned", text: session.cleanedText },
    ...(session.polishedText?.trim() ? [{ id: "edited", label: "AI edited", text: session.polishedText }] : []),
  ];
  const [selected, setSelected] = useState("original");
  const [message, setMessage] = useState("");
  const version = versions.find((item) => item.id === selected) ?? versions[0];
  return (
    <details className="tx-versions">
      <summary>Compare transcript versions</summary>
      <p className="muted">Viewing a version does not change what was pasted or your saved preference.</p>
      <div className="tx__actions" role="group" aria-label="Transcript versions">
        {versions.map((item) => (
          <button key={item.id} type="button" className="pill-btn ghost"
            aria-pressed={version.id === item.id}
            onClick={() => { setSelected(item.id); setMessage(""); }}>{item.label}</button>
        ))}
      </div>
      <p aria-label={`${version.label} transcript`} style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
        {version.text || "No text was recovered for this version."}
      </p>
      <button type="button" className="pill-btn ghost" disabled={!version.text.trim()}
        onClick={async () => {
          try { setMessage(await copyText(version.text)); }
          catch (error) { setMessage(error instanceof Error ? error.message : String(error)); }
        }}>Copy {version.label.toLowerCase()}</button>
      {message && <p role="status">{message}</p>}
    </details>
  );
}

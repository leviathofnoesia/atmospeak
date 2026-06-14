import type { RuntimeEvent } from "../types/dictation";

interface RuntimeEventListProps {
  events: RuntimeEvent[];
}

export function RuntimeEventList({ events }: RuntimeEventListProps) {
  const recent = events.slice(0, 6);
  return (
    <div className="runtime-events" aria-label="Runtime event log">
      <p className="eyebrow">Runtime signal</p>
      {recent.length === 0 ? (
        <p>No shortcut events recorded in this session.</p>
      ) : (
        <ul>
          {recent.map((event) => (
            <li key={`${event.createdAt}-${event.kind}-${event.message}`}>
              <span>{event.kind}</span>
              <p>{event.message}</p>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

# Plan 009: Replace full AppSnapshot refresh with targeted state updates

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.

## Status

- **Priority**: P3
- **Effort**: M
- **Risk**: MED
- **Depends on**: 007 (component extraction — must be done first to avoid massive merge conflicts in App.tsx)
- **Category**: perf
- **Planned at**: commit `1bb6795`, 2026-06-10

## Why this matters

Every mutation action (save settings, add dictionary entry, delete snippet, polish session, update notes, etc.) calls `get_app_snapshot`, which does a `SELECT * FROM transcript_sessions ORDER BY created_at DESC LIMIT 100` plus dictionary, snippets, settings, and stats. For a user with 100 sessions, this is 5 queries per action. With multiple rapid actions (e.g., editing dictionary entries), this compounds. The fix: for write operations that only change one entity, return only the changed data and let the frontend merge it, skipping the full refresh.

This plan creates the foundation by adding thin targeted-return commands for the highest-frequency actions (dictionary upsert, snippet upsert, session notes update), while keeping `get_app_snapshot` as the fallback for initial load and less frequent operations.

## Current state

- `src-tauri/src/db/mod.rs:110-124` — `snapshot()` does 5 queries:
  ```rust
  pub fn snapshot(&self) -> Result<AppSnapshot> {
      let settings = self.load_settings()?;
      let dictionary = self.list_dictionary()?;
      let snippets = self.list_snippets()?;
      let sessions = self.list_sessions()?;
      let stats = calculate_stats(&sessions);
      Ok(AppSnapshot { settings, dictionary, snippets, sessions, stats })
  }
  ```

- `src-tauri/src/commands.rs:207-223` — `upsert_dictionary_entry`: Takes the full `AppState` lock, calls `upsert_dictionary_entry`, then calls `snapshot()` to return the entire new state.
- Same pattern for `upsert_snippet`, `update_session_notes`, `delete_dictionary_entry`, `delete_snippet`.

- `src/App.tsx:233-258` — `refresh()` calls `Promise.all([getAppSnapshot(), listMicrophones(), ...])` on every action.

The frontend always replaces the entire snapshot after any mutation, making even a single dictionary edit trigger a full DB read.

## Commands you will need

| Purpose   | Command                  | Expected on success |
|-----------|--------------------------|---------------------|
| Rust check| `& "$env:USERPROFILE\.cargo\bin\cargo.exe" check --manifest-path src-tauri/Cargo.toml` | exit 0 |
| Rust tests| `& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --manifest-path src-tauri/Cargo.toml` | all pass |
| Typecheck | `npx tsc --noEmit`       | exit 0              |
| Tests     | `bun run test`           | all pass            |

## Scope

**In scope**:
- `src-tauri/src/commands.rs` — change return types of high-frequency mutations to partial snapshots
- `src-tauri/src/db/mod.rs` — add targeted update methods
- `src/types/dictation.ts` — add partial update types
- `src/lib/api.ts` — update return types for targeted commands

**Out of scope**:
- `src/App.tsx` panel extractions (done in 007)
- `polish_transcript`/`polish_session` async changes (done in 008)
- Any other commands that are low-frequency (export, search, feedback)

## Git workflow

- Branch: `advisor/009-targeted-state-updates`
- Commit: `Replace full AppSnapshot refresh with targeted mutations for dictionary, snippets, and notes`
- Do NOT push or open a PR.

## Steps

### Step 1: Add targeted database methods

In `src-tauri/src/db/mod.rs`, add methods that return only the changed entity rather than a full snapshot:

```rust
pub fn upsert_dictionary_entry_and_return(&self, entry: &DictionaryEntry) -> Result<DictionaryEntry> {
    self.upsert_dictionary_entry(entry)?;
    Ok(entry.clone())
}

pub fn delete_dictionary_entry_and_return(&self, id: &str) -> Result<()> {
    self.delete_dictionary_entry(id)
}

pub fn upsert_snippet_and_return(&self, snippet: &Snippet) -> Result<Snippet> {
    self.upsert_snippet(snippet)?;
    Ok(snippet.clone())
}

pub fn delete_snippet_and_return(&self, id: &str) -> Result<()> {
    self.delete_snippet(id)
}

pub fn update_session_notes_and_return(&self, id: &str, notes: &str) -> Result<TranscriptSession> {
    self.update_session_notes(id, notes)?;
    self.get_session(id)
}
```

**Verify**: `& "$env:USERPROFILE\.cargo\bin\cargo.exe" check --manifest-path src-tauri/Cargo.toml` → compiles

### Step 2: Add PartialUpdate type to models

In `src-tauri/src/models.rs`, add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StateUpdate {
    DictionaryUpserted(DictionaryEntry),
    DictionaryDeleted { id: String },
    SnippetUpserted(Snippet),
    SnippetDeleted { id: String },
    SessionNotesUpdated(TranscriptSession),
}
```

**Verify**: `& "$env:USERPROFILE\.cargo\bin\cargo.exe" check --manifest-path src-tauri/Cargo.toml` → compiles

### Step 3: Update Tauri commands to return targeted updates

In `src-tauri/src/commands.rs`, update the high-frequency commands:

Change `upsert_dictionary_entry` from returning `AppSnapshot` to returning `DictionaryEntry`:
```rust
#[tauri::command]
pub fn upsert_dictionary_entry(
    state: State<'_, AppState>,
    mut entry: DictionaryEntry,
) -> CommandResult<DictionaryEntry> {
    if entry.id.trim().is_empty() {
        entry.id = Uuid::new_v4().to_string();
    }
    let database = state.database.lock();
    to_command_result(database.upsert_dictionary_entry_and_return(&entry))
}
```

Similarly update `delete_dictionary_entry` to return just the deleted ID:
```rust
#[tauri::command]
pub fn delete_dictionary_entry(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<String> {
    let database = state.database.lock();
    to_command_result(database.delete_dictionary_entry_and_return(&id).map(|_| id))
}
```

Do the same for `upsert_snippet` (return `Snippet`) and `delete_snippet` (return `String` id).

For `update_session_notes`:
```rust
#[tauri::command]
pub fn update_session_notes(
    state: State<'_, AppState>,
    id: String,
    notes: String,
) -> CommandResult<TranscriptSession> {
    let database = state.database.lock();
    to_command_result(database.update_session_notes_and_return(&id, &notes))
}
```

**Verify**: `& "$env:USERPROFILE\.cargo\bin\cargo.exe" check --manifest-path src-tauri/Cargo.toml` → compiles

### Step 4: Update TypeScript types and API layer

In `src/types/dictation.ts`, no new types are needed — the existing `DictionaryEntry`, `Snippet`, and `TranscriptSession` types are the return types.

In `src/lib/api.ts`, update the return types:

```typescript
export function upsertDictionaryEntry(entry: DictionaryEntry): Promise<DictionaryEntry> {
  return command("upsert_dictionary_entry", { entry }, () => {
    const saved = { ...entry, id: entry.id || crypto.randomUUID() };
    mockSnapshot = {
      ...mockSnapshot,
      dictionary: [saved, ...mockSnapshot.dictionary.filter((candidate) => candidate.id !== saved.id)],
    };
    return saved;  // Return just the entry, not the full snapshot
  });
}
```

Similarly update `deleteDictionaryEntry`, `upsertSnippet`, `deleteSnippet`, and `updateSessionNotes` to return just the changed entity.

**Verify**: `npx tsc --noEmit` → exit 0

### Step 5: Update App.tsx to merge targeted updates

In `App.tsx`, update the handlers for these actions to merge the returned entity into the local state instead of replacing the entire snapshot:

For `addDictionaryEntry`:
```typescript
const addDictionaryEntry = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    // ... validation ...
    const savedEntry = await upsertDictionaryEntry(entry);
    setSnapshot((prev) => prev ? {
        ...prev,
        dictionary: [savedEntry, ...prev.dictionary.filter((d) => d.id !== savedEntry.id)],
    } : null);
    // ... notice and draft reset ...
};
```

Apply the same pattern for `deleteDictionaryEntry`, `upsertSnippet`, `deleteSnippet`, and `updateSessionNotes`.

The mock fallbacks in `api.ts` also need to return just the entity and do the merge internally for the browser mock mode (since they don't have a real backend).

**Verify**: `npx tsc --noEmit` → exit 0
**Verify**: `bun run test` → all pass

### Step 6: Verify end-to-end

**Verify**: `& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --manifest-path src-tauri/Cargo.toml` → all pass
**Verify**: `bun run test` → all pass
**Verify**: `npx tsc --noEmit` → exit 0

## Test plan

- Existing database tests (`settings_round_trip`, `search_sessions_filters_text_dates_and_word_counts`, etc.) must continue passing.
- New test: in `src-tauri/src/db/mod.rs` tests, add a test for `upsert_dictionary_entry_and_return` that verifies it returns the entry with the generated ID.
- Frontend: `bun run test` must pass (the mock API in `api.ts` still works in browser mode).

## Done criteria

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` all pass
- [ ] `bun run test` all pass
- [ ] `npx tsc --noEmit` exits 0
- [ ] `upsert_dictionary_entry` command returns `DictionaryEntry` not `AppSnapshot`
- [ ] `upsert_snippet` command returns `Snippet` not `AppSnapshot`
- [ ] `delete_dictionary_entry` command returns `String` id
- [ ] `delete_snippet` command returns `String` id
- [ ] `update_session_notes` command returns `TranscriptSession` not `AppSnapshot`
- [ ] `App.tsx` handlers merge returned entities into local state without full refresh
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The Tauri `generate_handler!` macro doesn't support returning non-`AppSnapshot` types from these commands (it should — each command's return type is inferred independently).
- Any frontend mock fallback in `api.ts` breaks the merge logic (the mock must still work in browser mode).
- Component extraction (plan 007) hasn't landed yet — the App.tsx state management changes in this plan would conflict with the extraction.

## Maintenance notes

- Future commands that mutate data should follow the same pattern: return the changed entity, let the frontend merge.
- `get_app_snapshot` should still be used for initial load and low-frequency actions (save settings, polish session).
- If the frontend ever grows a proper state management layer (Zustand, Jotai, etc.), the targeted update pattern makes the transition natural — each command already returns just the relevant slice.
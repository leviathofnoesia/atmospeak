use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};

use crate::models::{
    AppSettings, AppSnapshot, DictationStats, DictionaryEntry, RuntimeEvent, Snippet,
    StreamingMetrics, TranscriptSession,
};
use crate::services::model_downloader::BUNDLED_MODEL_ID;

pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn open(app_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&app_dir).context("failed to create application data directory")?;
        let connection = Connection::open(app_dir.join("wind-speak.sqlite3"))
            .context("failed to open sqlite database")?;
        connection.create_scalar_function(
            "unicode_lower", 1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8 | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            |context| Ok(context.get::<String>(0)?.to_lowercase()),
        )?;
        let database = Self { connection };
        database.migrate()?;
        Ok(database)
    }

    fn migrate(&self) -> Result<()> {
        self.connection.execute_batch(
            r#"
            create table if not exists settings (
                key text primary key not null,
                value text not null
            );

            create table if not exists dictionary_entries (
                id text primary key not null,
                phrase text not null,
                replacement text not null,
                enabled integer not null,
                created_at text not null
            );

            create table if not exists snippets (
                id text primary key not null,
                trigger text not null,
                body text not null,
                enabled integer not null,
                created_at text not null
            );

            create table if not exists transcript_sessions (
                id text primary key not null,
                raw_text text not null,
                cleaned_text text not null,
                audio_path text not null,
                duration_ms integer not null,
                word_count integer not null,
                injected integer not null,
                source_application text,
                created_at text not null
            );

            create table if not exists runtime_events (
                id integer primary key autoincrement,
                kind text not null,
                message text not null,
                created_at text not null
            );

            create table if not exists dictation_metrics (
                session_id text primary key not null,
                backend text not null,
                model_id text not null,
                first_partial_ms integer,
                stop_ack_ms integer not null,
                finalize_ms integer not null,
                paste_ms integer not null,
                processed_during_recording_ms integer not null,
                tail_audio_ms integer not null,
                max_backlog_ms integer not null,
                audio_frames_dropped integer not null,
                fallback_reason text,
                created_at text not null
            );
            "#,
        )?;
        let has_source_application = self
            .connection
            .prepare("pragma table_info(transcript_sessions)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .iter()
            .any(|column| column == "source_application");
        if !has_source_application {
            self.connection.execute(
                "alter table transcript_sessions add column source_application text",
                [],
            )?;
        }
        let session_columns = self
            .connection
            .prepare("pragma table_info(transcript_sessions)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if !session_columns
            .iter()
            .any(|column| column == "polished_text")
        {
            self.connection.execute(
                "alter table transcript_sessions add column polished_text text",
                [],
            )?;
        }
        if !session_columns
            .iter()
            .any(|column| column == "prefer_polished")
        {
            self.connection.execute(
                "alter table transcript_sessions add column prefer_polished integer not null default 1",
                [],
            )?;
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Result<AppSnapshot> {
        let settings = self.load_settings()?;
        let dictionary = self.list_dictionary()?;
        let snippets = self.list_snippets()?;
        let sessions = self.list_sessions()?;
        let stats = calculate_stats(&sessions);

        Ok(AppSnapshot {
            settings,
            dictionary,
            snippets,
            sessions,
            stats,
        })
    }

    pub fn load_settings(&self) -> Result<AppSettings> {
        let serialized: Option<String> = self
            .connection
            .query_row("select value from settings where key = 'app'", [], |row| {
                row.get(0)
            })
            .optional()?;

        match serialized {
            Some(value) => serde_json::from_str(&value)
                .map(migrate_settings)
                .context("failed to deserialize application settings"),
            None => Ok(AppSettings::default()),
        }
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<()> {
        let value = serde_json::to_string(settings)?;
        self.connection.execute(
            "insert into settings (key, value) values ('app', ?1)
             on conflict(key) do update set value = excluded.value",
            params![value],
        )?;
        Ok(())
    }

    pub fn insert_runtime_events(&self, events: &[RuntimeEvent]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let transaction = self.connection.unchecked_transaction()?;
        {
            let mut statement = transaction.prepare(
                "insert into runtime_events (kind, message, created_at) values (?1, ?2, ?3)",
            )?;
            for event in events {
                statement.execute(params![
                    event.kind,
                    event.message,
                    event.created_at.to_rfc3339()
                ])?;
            }
        }
        transaction.execute(
            "delete from runtime_events where id not in (
                select id from runtime_events order by id desc limit 500
            )",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn list_runtime_events(&self, limit: usize) -> Result<Vec<RuntimeEvent>> {
        let mut statement = self.connection.prepare(
            "select kind, message, created_at
             from runtime_events
             order by id desc
             limit ?1",
        )?;
        let rows = statement.query_map(params![limit.min(500) as i64], |row| {
            Ok(RuntimeEvent {
                kind: row.get(0)?,
                message: row.get(1)?,
                created_at: parse_datetime(row.get::<_, String>(2)?),
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to load runtime events")
    }

    pub fn insert_dictation_metrics(&self, metrics: &StreamingMetrics) -> Result<()> {
        self.connection.execute(
            "insert into dictation_metrics (
                session_id, backend, model_id, first_partial_ms, stop_ack_ms,
                finalize_ms, paste_ms, processed_during_recording_ms, tail_audio_ms,
                max_backlog_ms, audio_frames_dropped, fallback_reason, created_at
             ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             on conflict(session_id) do update set
                backend = excluded.backend,
                model_id = excluded.model_id,
                first_partial_ms = excluded.first_partial_ms,
                stop_ack_ms = excluded.stop_ack_ms,
                finalize_ms = excluded.finalize_ms,
                paste_ms = excluded.paste_ms,
                processed_during_recording_ms = excluded.processed_during_recording_ms,
                tail_audio_ms = excluded.tail_audio_ms,
                max_backlog_ms = excluded.max_backlog_ms,
                audio_frames_dropped = excluded.audio_frames_dropped,
                fallback_reason = excluded.fallback_reason,
                created_at = excluded.created_at",
            params![
                metrics.session_id,
                format!("{:?}", metrics.backend).to_ascii_lowercase(),
                metrics.model_id,
                metrics.first_partial_ms.map(|value| value as i64),
                metrics.stop_ack_ms as i64,
                metrics.finalize_ms as i64,
                metrics.paste_ms as i64,
                metrics.processed_during_recording_ms as i64,
                metrics.tail_audio_ms as i64,
                metrics.max_backlog_ms as i64,
                metrics.audio_frames_dropped as i64,
                metrics.fallback_reason,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_dictionary(&self) -> Result<Vec<DictionaryEntry>> {
        let mut statement = self.connection.prepare(
            "select id, phrase, replacement, enabled, created_at
             from dictionary_entries
             order by phrase asc",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(DictionaryEntry {
                id: row.get(0)?,
                phrase: row.get(1)?,
                replacement: row.get(2)?,
                enabled: row.get::<_, i64>(3)? == 1,
                created_at: parse_datetime(row.get::<_, String>(4)?),
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to load dictionary entries")
    }

    pub fn upsert_dictionary_entry(&self, entry: &DictionaryEntry) -> Result<()> {
        self.connection.execute(
            "insert into dictionary_entries (id, phrase, replacement, enabled, created_at)
             values (?1, ?2, ?3, ?4, ?5)
             on conflict(id) do update set
                phrase = excluded.phrase,
                replacement = excluded.replacement,
                enabled = excluded.enabled",
            params![
                entry.id,
                entry.phrase,
                entry.replacement,
                bool_to_i64(entry.enabled),
                entry.created_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn delete_dictionary_entry(&self, id: &str) -> Result<()> {
        self.connection
            .execute("delete from dictionary_entries where id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_snippets(&self) -> Result<Vec<Snippet>> {
        let mut statement = self.connection.prepare(
            "select id, trigger, body, enabled, created_at
             from snippets
             order by trigger asc",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Snippet {
                id: row.get(0)?,
                trigger: row.get(1)?,
                body: row.get(2)?,
                enabled: row.get::<_, i64>(3)? == 1,
                created_at: parse_datetime(row.get::<_, String>(4)?),
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to load snippets")
    }

    pub fn upsert_snippet(&self, snippet: &Snippet) -> Result<()> {
        self.connection.execute(
            "insert into snippets (id, trigger, body, enabled, created_at)
             values (?1, ?2, ?3, ?4, ?5)
             on conflict(id) do update set
                trigger = excluded.trigger,
                body = excluded.body,
                enabled = excluded.enabled",
            params![
                snippet.id,
                snippet.trigger,
                snippet.body,
                bool_to_i64(snippet.enabled),
                snippet.created_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn delete_snippet(&self, id: &str) -> Result<()> {
        self.connection
            .execute("delete from snippets where id = ?1", params![id])?;
        Ok(())
    }

    pub fn insert_session(&self, session: &TranscriptSession) -> Result<()> {
        self.connection.execute(
            "insert into transcript_sessions
             (id, raw_text, cleaned_text, polished_text, prefer_polished, audio_path, duration_ms, word_count, injected, source_application, created_at)
             values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             on conflict(id) do update set
               raw_text = excluded.raw_text, cleaned_text = excluded.cleaned_text,
               polished_text = excluded.polished_text, prefer_polished = excluded.prefer_polished,
               audio_path = excluded.audio_path, duration_ms = excluded.duration_ms,
               word_count = excluded.word_count, injected = excluded.injected,
               source_application = excluded.source_application",
            params![
                session.id,
                session.raw_text,
                session.cleaned_text,
                session.polished_text,
                bool_to_i64(session.prefer_polished),
                session.audio_path,
                session.duration_ms as i64,
                session.word_count as i64,
                bool_to_i64(session.injected),
                session.source_application,
                session.created_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn list_sessions(&self) -> Result<Vec<TranscriptSession>> {
        let mut statement = self.connection.prepare(
            "select id, raw_text, cleaned_text, polished_text, prefer_polished, audio_path, duration_ms, word_count, injected, source_application, created_at
             from transcript_sessions
             order by created_at desc
             limit 100",
        )?;
        let rows = statement.query_map([], session_from_row)?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to load transcript sessions")
    }

    pub fn search_sessions(&self, query: &str, offset: u32) -> Result<Vec<TranscriptSession>> {
        let mut statement = self.connection.prepare(
            "select id, raw_text, cleaned_text, polished_text, prefer_polished, audio_path, duration_ms, word_count, injected, source_application, created_at
             from transcript_sessions
             where instr(unicode_lower(raw_text), unicode_lower(?1)) > 0 or instr(unicode_lower(cleaned_text), unicode_lower(?1)) > 0
                or instr(unicode_lower(coalesce(polished_text, '')), unicode_lower(?1)) > 0
             order by created_at desc, id desc limit 101 offset ?2",
        )?;
        let rows = statement.query_map(params![query.trim(), offset], session_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().context("failed to search transcript sessions")
    }

    pub fn update_session_polish_preference(
        &self,
        id: &str,
        prefer_polished: bool,
    ) -> Result<Option<TranscriptSession>> {
        let updated = self.connection.execute(
            "update transcript_sessions set prefer_polished = ?1 where id = ?2",
            params![bool_to_i64(prefer_polished), id],
        )?;
        if updated == 0 {
            return Ok(None);
        }
        self.get_session(id)
    }

    pub fn update_session_polished_text(
        &self,
        id: &str,
        polished_text: &str,
    ) -> Result<Option<TranscriptSession>> {
        let updated = self.connection.execute(
            "update transcript_sessions
             set polished_text = ?1, prefer_polished = 1, cleaned_text = cleaned_text,
                 word_count = ?2
             where id = ?3",
            params![
                polished_text,
                polished_text.split_whitespace().count() as i64,
                id
            ],
        )?;
        if updated == 0 {
            return Ok(None);
        }
        self.get_session(id)
    }

    /// Attach optional output only to the unchanged delivered transcript. Never recreate it.
    pub fn save_automatic_polish(&self, expected: &TranscriptSession, text: &str) -> Result<bool> {
        Ok(self.connection.execute(
            "update transcript_sessions set polished_text = ?1, word_count = ?2
             where id = ?3 and raw_text = ?4 and cleaned_text = ?5
               and polished_text is null and prefer_polished = 1",
            params![text, text.split_whitespace().count() as i64, expected.id,
                expected.raw_text, expected.cleaned_text],
        )? == 1)
    }

    pub fn latest_session(&self) -> Result<Option<TranscriptSession>> {
        let id = self.connection.query_row(
            "select id from transcript_sessions order by created_at desc, id desc limit 1",
            [], |row| row.get::<_, String>(0),
        ).optional()?;
        match id { Some(id) => self.get_session(&id), None => Ok(None) }
    }

    pub fn get_session(&self, id: &str) -> Result<Option<TranscriptSession>> {
        let mut statement = self.connection.prepare(
            "select id, raw_text, cleaned_text, polished_text, prefer_polished, audio_path, duration_ms, word_count, injected, source_application, created_at
             from transcript_sessions where id = ?1",
        )?;
        let session = statement
            .query_row(params![id], session_from_row)
            .optional()?;
        Ok(session)
    }

    pub fn delete_session(&self, id: &str) -> Result<Option<String>> {
        let transaction = self.connection.unchecked_transaction()?;
        let audio_path = transaction
            .query_row(
                "select audio_path from transcript_sessions where id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        transaction.execute("delete from transcript_sessions where id = ?1", params![id])?;
        transaction.execute(
            "delete from dictation_metrics where session_id = ?1",
            params![id],
        )?;
        transaction.commit()?;
        Ok(audio_path)
    }

    pub fn prune_sessions(&self, retention_days: u32) -> Result<Vec<String>> {
        if retention_days == 0 {
            return Ok(Vec::new());
        }
        let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
        let transaction = self.connection.unchecked_transaction()?;
        let mut statement = transaction
            .prepare("select audio_path from transcript_sessions where created_at < ?1")?;
        let audio_paths = statement
            .query_map(params![cutoff.to_rfc3339()], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        transaction.execute(
            "delete from transcript_sessions where created_at < ?1",
            params![cutoff.to_rfc3339()],
        )?;
        transaction.execute(
            "delete from dictation_metrics
             where session_id not in (select id from transcript_sessions)",
            [],
        )?;
        drop(statement);
        transaction.commit()?;
        Ok(audio_paths)
    }
}

fn session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TranscriptSession> {
    Ok(TranscriptSession {
        id: row.get(0)?,
        raw_text: row.get(1)?,
        cleaned_text: row.get(2)?,
        polished_text: row.get(3)?,
        prefer_polished: row.get::<_, i64>(4).unwrap_or(1) == 1,
        audio_path: row.get(5)?,
        duration_ms: row.get::<_, i64>(6)? as u64,
        word_count: row.get::<_, i64>(7)? as usize,
        injected: row.get::<_, i64>(8)? == 1,
        source_application: row.get(9)?,
        created_at: parse_datetime(row.get::<_, String>(10)?),
    })
}

fn bool_to_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn parse_datetime(value: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&value)
        .map(|datetime| datetime.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn calculate_stats(sessions: &[TranscriptSession]) -> DictationStats {
    let total_words = sessions
        .iter()
        .map(|session| session.word_count)
        .sum::<usize>();
    let total_duration_ms = sessions
        .iter()
        .map(|session| session.duration_ms)
        .sum::<u64>();
    let average_words_per_minute = if total_duration_ms == 0 {
        0.0
    } else {
        total_words as f32 / (total_duration_ms as f32 / 60_000.0)
    };

    DictationStats {
        total_sessions: sessions.len(),
        total_words,
        total_duration_ms,
        average_words_per_minute,
    }
}

fn migrate_settings(mut settings: AppSettings) -> AppSettings {
    settings.active_model_id = BUNDLED_MODEL_ID.to_string();
    if settings
        .hotkey
        .trim()
        .eq_ignore_ascii_case("Ctrl+Win+Space")
    {
        settings.hotkey = "Ctrl+Win".to_string();
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::DictationMode;
    use tempfile::tempdir;

    #[test]
    fn settings_round_trip() {
        let temp = tempdir().expect("tempdir");
        let database = Database::open(temp.path().to_path_buf()).expect("database");
        let mut settings = AppSettings::default();
        settings.mode = DictationMode::PushToTalk;
        settings.restore_clipboard = false;

        database.save_settings(&settings).expect("save settings");

        let loaded = database.load_settings().expect("load settings");
        assert_eq!(loaded.mode, DictationMode::PushToTalk);
        assert!(!loaded.restore_clipboard);
        assert_eq!(
            loaded.active_model_id,
            crate::services::model_downloader::BUNDLED_MODEL_ID
        );
    }

    /// Phase A locked settings to 12 fields and shipped blobs with exactly those
    /// keys. Adding the appearance block must not strand an existing profile.
    #[test]
    fn loads_settings_blobs_written_before_the_appearance_fields() {
        use crate::models::{Accent, DockShape, DockTheme, Motion, WaveStyle};

        let temp = tempdir().expect("tempdir");
        let database = Database::open(temp.path().to_path_buf()).expect("database");
        let legacy = r#"{
            "hotkey": "Ctrl+Win",
            "mode": "toggle",
            "microphoneName": null,
            "restoreClipboard": true,
            "autoInject": true,
            "cleanupEnabled": true,
            "startAtLogin": false,
            "onboardingComplete": true,
            "onboardingVersion": "phase-a-honest-mvp-v1",
            "advancedRuntimeEnabled": false,
            "advancedModelPath": "",
            "advancedWhisperCliPath": "",
            "modelSelectionMode": "manual",
            "transcriptionProfile": "fast",
            "accelerationPreference": "vulkan"
        }"#;
        database
            .connection
            .execute(
                "insert into settings (key, value) values ('app', ?1)
                 on conflict(key) do update set value = excluded.value",
                [legacy],
            )
            .expect("seed legacy settings");

        let loaded = database.load_settings().expect("load settings");

        // The pre-existing choices survive.
        assert_eq!(loaded.mode, DictationMode::Toggle);
        assert!(loaded.onboarding_complete);
        assert_eq!(
            loaded.active_model_id,
            crate::services::model_downloader::BUNDLED_MODEL_ID
        );
        // The new fields fall back to their defaults rather than failing the parse.
        assert_eq!(loaded.accent, Accent::Dusk);
        assert_eq!(loaded.dock_shape, DockShape::Orb);
        assert_eq!(loaded.wave_style, WaveStyle::Ribbon);
        assert_eq!(loaded.dock_theme, DockTheme::Dark);
        assert_eq!(loaded.motion, Motion::Lively);
        assert!(loaded.live_preview_enabled);
    }

    #[test]
    fn appearance_settings_round_trip() {
        use crate::models::{Accent, DockShape, Motion};

        let temp = tempdir().expect("tempdir");
        let database = Database::open(temp.path().to_path_buf()).expect("database");
        let mut settings = AppSettings::default();
        settings.accent = Accent::Lilac;
        settings.dock_shape = DockShape::Tape;
        settings.motion = Motion::Calm;

        database.save_settings(&settings).expect("save settings");

        let loaded = database.load_settings().expect("load settings");
        assert_eq!(loaded.accent, Accent::Lilac);
        assert_eq!(loaded.dock_shape, DockShape::Tape);
        assert_eq!(loaded.motion, Motion::Calm);
    }

    #[test]
    fn runtime_diagnostics_are_persisted_and_bounded() {
        let temp = tempdir().expect("tempdir");
        let database = Database::open(temp.path().to_path_buf()).expect("database");
        let batch: Vec<_> = (0..505)
            .map(|index| RuntimeEvent {
                kind: "shortcut-ack".to_string(),
                message: format!("gesture={index}"),
                created_at: Utc::now(),
            })
            .collect();
        database
            .insert_runtime_events(&batch[..1])
            .expect("single event");
        database
            .insert_runtime_events(&batch)
            .expect("batch events");
        database.insert_runtime_events(&[]).expect("empty batch");
        let events = database.list_runtime_events(500).expect("runtime events");
        assert_eq!(events.len(), 500);
        assert_eq!(events[0].message, "gesture=504");
        assert_eq!(events[499].message, "gesture=5");
    }

    #[test]
    fn migrates_legacy_default_hotkey_to_modifier_chord() {
        let temp = tempdir().expect("tempdir");
        let database = Database::open(temp.path().to_path_buf()).expect("database");
        let mut settings = AppSettings::default();
        settings.hotkey = "Ctrl+Win+Space".to_string();

        database.save_settings(&settings).expect("save settings");

        let loaded = database.load_settings().expect("load settings");
        assert_eq!(loaded.hotkey, "Ctrl+Win");
    }

    #[test]
    fn latest_recovery_respects_preference_and_never_substitutes_an_older_recording() {
        use crate::services::history_actions::copyable_text;
        let temp = tempdir().unwrap();
        let db = Database::open(temp.path().to_path_buf()).unwrap();
        assert!(db.latest_session().unwrap().is_none());
        let mut session = TranscriptSession {
            id: "old".into(), raw_text: "original".into(), cleaned_text: "Cleaned.".into(),
            polished_text: Some("Edited.".into()), prefer_polished: true,
            audio_path: "retained.wav".into(), duration_ms: 1000, word_count: 1,
            injected: false, source_application: None, created_at: Utc::now(),
        };
        assert_eq!(copyable_text(&session), Some("Edited."));
        session.prefer_polished = false;
        assert_eq!(copyable_text(&session), Some("Cleaned."));
        db.insert_session(&session).unwrap();
        session.id = "new".into();
        session.created_at += chrono::Duration::seconds(1);
        session.raw_text.clear(); session.cleaned_text.clear(); session.polished_text = None;
        db.insert_session(&session).unwrap();
        let latest = db.latest_session().unwrap().unwrap();
        assert_eq!(latest.id, "new");
        assert!(copyable_text(&latest).is_none());
        assert!(db.get_session("missing").unwrap().is_none());
        db.delete_session("new").unwrap();
        assert_eq!(db.latest_session().unwrap().unwrap().id, "old");
    }

    #[test]
    fn history_search_pages_retained_recordings_and_finds_original_text() {
        let temp = tempdir().unwrap();
        let db = Database::open(temp.path().to_path_buf()).unwrap();
        for index in 0..102 {
            db.insert_session(&TranscriptSession {
                id: format!("record-{index:03}"), raw_text: format!("Original token {index}"),
                cleaned_text: "Cleaned.".into(), polished_text: Some("Edited.".into()),
                prefer_polished: false, audio_path: "saved.wav".into(), duration_ms: 1000,
                word_count: 1, injected: false, source_application: None,
                created_at: Utc::now() + chrono::Duration::seconds(index),
            }).unwrap();
        }
        let first = db.search_sessions("", 0).unwrap();
        assert_eq!(first.len(), 101); // One extra row signals another page.
        let second = db.search_sessions("", 100).unwrap();
        assert_eq!(second.len(), 2);
        assert_eq!(second[1].id, "record-000");
        let original = db.search_sessions("ORIGINAL TOKEN 0", 0).unwrap();
        assert_eq!(original.len(), 1);
        assert_eq!(original[0].id, "record-000");
        assert_eq!(db.search_sessions("edited", 100).unwrap().len(), 2);
        assert!(db.search_sessions("absent", 0).unwrap().is_empty());
        assert!(db.get_session("record-000").unwrap().is_some());
        let mut unicode = db.get_session("record-000").unwrap().unwrap();
        unicode.raw_text = "Café".into();
        unicode.cleaned_text = "ÉCOLE".into();
        unicode.polished_text = Some("Привет".into());
        db.insert_session(&unicode).unwrap();
        for query in ["CAFÉ", "école", "ПРИВЕТ"] {
            let rows = db.search_sessions(query, 0).unwrap();
            assert_eq!(rows.len(), 1, "{query}");
            assert_eq!(rows[0].id, unicode.id);
        }
    }

    #[test]
    fn automatic_polish_preserves_newer_history_and_never_resurrects_deletions() {
        let temp = tempdir().expect("tempdir");
        let db = Database::open(temp.path().to_path_buf()).expect("database");
        let session = TranscriptSession {
            id: "delivered".into(), raw_text: "hello".into(), cleaned_text: "Hello.".into(),
            polished_text: None, prefer_polished: true, audio_path: "retained.wav".into(),
            duration_ms: 1000, word_count: 1, injected: true,
            source_application: Some("Notepad".into()), created_at: Utc::now(),
        };
        db.insert_session(&session).unwrap();
        assert!(db.save_automatic_polish(&session, "Hello there.").unwrap());
        let saved = db.get_session(&session.id).unwrap().unwrap();
        assert_eq!(saved.polished_text.as_deref(), Some("Hello there."));
        assert!(saved.injected);
        assert_eq!(saved.audio_path, session.audio_path);
        assert_eq!(saved.source_application, session.source_application);
        assert_eq!(saved.word_count, 2);
        assert!(!db.save_automatic_polish(&session, "Late duplicate").unwrap());

        db.insert_session(&session).unwrap();
        db.update_session_polish_preference(&session.id, false).unwrap();
        assert!(!db.save_automatic_polish(&session, "Unwanted edit").unwrap());
        assert!(!db.get_session(&session.id).unwrap().unwrap().prefer_polished);

        db.insert_session(&session).unwrap();
        db.update_session_polished_text(&session.id, "Explicit edit").unwrap();
        assert!(!db.save_automatic_polish(&session, "Older auto edit").unwrap());
        assert_eq!(db.get_session(&session.id).unwrap().unwrap().polished_text.as_deref(), Some("Explicit edit"));

        let mut changed = session.clone();
        changed.cleaned_text = "Corrected wording".into();
        db.insert_session(&changed).unwrap();
        assert!(!db.save_automatic_polish(&session, "Stale wording").unwrap());
        changed = session.clone();
        changed.raw_text = "new recognition".into();
        db.insert_session(&changed).unwrap();
        assert!(!db.save_automatic_polish(&session, "Stale recognition").unwrap());

        db.delete_session(&session.id).unwrap();
        assert!(!db.save_automatic_polish(&session, "Deleted content").unwrap());
        assert!(db.get_session(&session.id).unwrap().is_none());
    }

    #[test]
    fn session_source_application_round_trips_and_retention_prunes() {
        let temp = tempdir().expect("tempdir");
        let database = Database::open(temp.path().to_path_buf()).expect("database");
        let session = TranscriptSession {
            id: "old-session".to_string(),
            raw_text: "hello".to_string(),
            cleaned_text: "Hello.".to_string(),
            polished_text: None,
            prefer_polished: true,
            audio_path: temp.path().join("old.wav").to_string_lossy().to_string(),
            duration_ms: 1_000,
            word_count: 1,
            injected: true,
            source_application: Some("Notepad".to_string()),
            created_at: Utc::now() - chrono::Duration::days(40),
        };
        let mut checkpoint = session.clone();
        checkpoint.raw_text = "partial".to_string();
        checkpoint.cleaned_text = "Partial.".to_string();
        checkpoint.injected = false;
        database
            .insert_session(&checkpoint)
            .expect("save checkpoint");
        database
            .insert_session(&session)
            .expect("complete checkpoint");
        let loaded = database.list_sessions().expect("list sessions");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].raw_text, "hello");
        assert_eq!(loaded[0].audio_path, checkpoint.audio_path);
        assert!(loaded[0].injected);
        assert_eq!(loaded[0].source_application.as_deref(), Some("Notepad"));

        let pruned = database.prune_sessions(30).expect("prune sessions");
        assert_eq!(pruned, vec![session.audio_path]);
        assert!(
            database
                .list_sessions()
                .expect("list after prune")
                .is_empty()
        );
    }
}

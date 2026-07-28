use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};

use crate::models::{
    AppSettings, AppSnapshot, AsrBackend, DictationStats, DictionaryEntry, RuntimeEvent, Snippet,
    StreamingMetrics, TranscriptSession,
};

pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn open(app_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&app_dir).context("failed to create application data directory")?;
        let connection = Connection::open(app_dir.join("wind-speak.sqlite3"))
            .context("failed to open sqlite database")?;
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

    pub fn insert_runtime_event(&self, event: &RuntimeEvent) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "insert into runtime_events (kind, message, created_at) values (?1, ?2, ?3)",
            params![event.kind, event.message, event.created_at.to_rfc3339()],
        )?;
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

    pub fn automatic_model_candidate(
        &self,
        preferred_model_id: &str,
        backend: AsrBackend,
    ) -> Result<Option<String>> {
        let backend = match backend {
            AsrBackend::Vulkan => "vulkan",
            AsrBackend::Cpu => "cpu",
            AsrBackend::Host => "host",
            AsrBackend::Cli => "cli",
        };
        let preferred_is_healthy: bool = self.connection.query_row(
            "select count(*) >= 3
                    and avg(finalize_ms) <= 1500
                    and max(max_backlog_ms) < 2000
                    and max(audio_frames_dropped) = 0
             from dictation_metrics
             where model_id = ?1 and backend = ?2",
            params![preferred_model_id, backend],
            |row| row.get(0),
        )?;
        if preferred_is_healthy {
            return Ok(Some(preferred_model_id.to_string()));
        }
        self.connection
            .query_row(
                "select model_id
                 from dictation_metrics
                 where backend = ?1
                 group by model_id
                 having count(*) >= 3
                    and avg(finalize_ms) <= 1500
                    and max(max_backlog_ms) < 2000
                    and max(audio_frames_dropped) = 0
                 order by avg(finalize_ms) asc
                 limit 1",
                params![backend],
                |row| row.get(0),
            )
            .optional()
            .context("failed to select automatic transcription model")
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
             (id, raw_text, cleaned_text, audio_path, duration_ms, word_count, injected, source_application, created_at)
             values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                session.id,
                session.raw_text,
                session.cleaned_text,
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
            "select id, raw_text, cleaned_text, audio_path, duration_ms, word_count, injected, source_application, created_at
             from transcript_sessions
             order by created_at desc
             limit 100",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(TranscriptSession {
                id: row.get(0)?,
                raw_text: row.get(1)?,
                cleaned_text: row.get(2)?,
                audio_path: row.get(3)?,
                duration_ms: row.get::<_, i64>(4)? as u64,
                word_count: row.get::<_, i64>(5)? as usize,
                injected: row.get::<_, i64>(6)? == 1,
                source_application: row.get(7)?,
                created_at: parse_datetime(row.get::<_, String>(8)?),
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to load transcript sessions")
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
    }

    /// Phase A locked settings to 12 fields and shipped blobs with exactly those
    /// keys. Adding the appearance block must not strand an existing profile.
    #[test]
    fn loads_settings_blobs_written_before_the_appearance_fields() {
        use crate::models::{
            AccelerationPreference, Accent, DockShape, DockTheme, ModelSelectionMode, Motion,
            TranscriptionProfile, WaveStyle,
        };

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
            "advancedWhisperCliPath": ""
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
        // The new fields fall back to their defaults rather than failing the parse.
        assert_eq!(loaded.accent, Accent::Dusk);
        assert_eq!(loaded.dock_shape, DockShape::Orb);
        assert_eq!(loaded.wave_style, WaveStyle::Ribbon);
        assert_eq!(loaded.dock_theme, DockTheme::Dark);
        assert_eq!(loaded.motion, Motion::Lively);
        assert_eq!(loaded.model_selection_mode, ModelSelectionMode::Automatic);
        assert_eq!(loaded.transcription_profile, TranscriptionProfile::Balanced);
        assert_eq!(loaded.acceleration_preference, AccelerationPreference::Auto);
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
        for index in 0..505 {
            database
                .insert_runtime_event(&RuntimeEvent {
                    kind: "shortcut-ack".to_string(),
                    message: format!("gesture={index}"),
                    created_at: Utc::now(),
                })
                .expect("insert runtime event");
        }
        let events = database.list_runtime_events(500).expect("runtime events");
        assert_eq!(events.len(), 500);
        assert_eq!(events[0].message, "gesture=504");
        assert_eq!(events[499].message, "gesture=5");
    }

    #[test]
    fn automatic_model_policy_keeps_a_measured_healthy_preference() {
        use crate::models::{AsrBackend, StreamingMetrics};

        let temp = tempdir().expect("tempdir");
        let database = Database::open(temp.path().to_path_buf()).expect("database");
        for index in 0..3 {
            database
                .insert_dictation_metrics(&StreamingMetrics {
                    session_id: format!("session-{index}"),
                    backend: AsrBackend::Vulkan,
                    model_id: "distil-large-v3.5".to_string(),
                    first_partial_ms: Some(900),
                    stop_ack_ms: 20,
                    finalize_ms: 600,
                    paste_ms: 80,
                    processed_during_recording_ms: 9_500,
                    tail_audio_ms: 500,
                    max_backlog_ms: 500,
                    audio_frames_dropped: 0,
                    fallback_reason: None,
                })
                .expect("dictation metrics");
        }
        assert_eq!(
            database
                .automatic_model_candidate("distil-large-v3.5", AsrBackend::Vulkan)
                .expect("automatic candidate")
                .as_deref(),
            Some("distil-large-v3.5")
        );
    }

    #[test]
    fn automatic_model_policy_uses_only_the_requested_backend() {
        use crate::models::{AsrBackend, StreamingMetrics};

        let temp = tempdir().expect("tempdir");
        let database = Database::open(temp.path().to_path_buf()).expect("database");
        for index in 0..3 {
            database
                .insert_dictation_metrics(&StreamingMetrics {
                    session_id: format!("cpu-session-{index}"),
                    backend: AsrBackend::Cpu,
                    model_id: "distil-large-v3.5".to_string(),
                    first_partial_ms: Some(900),
                    stop_ack_ms: 20,
                    finalize_ms: 600,
                    paste_ms: 80,
                    processed_during_recording_ms: 9_500,
                    tail_audio_ms: 500,
                    max_backlog_ms: 500,
                    audio_frames_dropped: 0,
                    fallback_reason: None,
                })
                .expect("dictation metrics");
        }

        assert_eq!(
            database
                .automatic_model_candidate("distil-large-v3.5", AsrBackend::Cpu)
                .expect("cpu candidate")
                .as_deref(),
            Some("distil-large-v3.5")
        );
        assert_eq!(
            database
                .automatic_model_candidate("distil-large-v3.5", AsrBackend::Vulkan)
                .expect("vulkan candidate"),
            None
        );
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
    fn session_source_application_round_trips_and_retention_prunes() {
        let temp = tempdir().expect("tempdir");
        let database = Database::open(temp.path().to_path_buf()).expect("database");
        let session = TranscriptSession {
            id: "old-session".to_string(),
            raw_text: "hello".to_string(),
            cleaned_text: "Hello.".to_string(),
            audio_path: temp.path().join("old.wav").to_string_lossy().to_string(),
            duration_ms: 1_000,
            word_count: 1,
            injected: true,
            source_application: Some("Notepad".to_string()),
            created_at: Utc::now() - chrono::Duration::days(40),
        };
        database.insert_session(&session).expect("insert session");
        let loaded = database.list_sessions().expect("list sessions");
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

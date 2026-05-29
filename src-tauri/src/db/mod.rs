use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};

use crate::models::{
    AppSettings, AppSnapshot, DictationStats, DictionaryEntry, Snippet, TranscriptSession,
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
                created_at text not null
            );
            "#,
        )?;
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
            Some(value) => {
                serde_json::from_str(&value).context("failed to deserialize application settings")
            }
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
             (id, raw_text, cleaned_text, audio_path, duration_ms, word_count, injected, created_at)
             values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                session.id,
                session.raw_text,
                session.cleaned_text,
                session.audio_path,
                session.duration_ms as i64,
                session.word_count as i64,
                bool_to_i64(session.injected),
                session.created_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn list_sessions(&self) -> Result<Vec<TranscriptSession>> {
        let mut statement = self.connection.prepare(
            "select id, raw_text, cleaned_text, audio_path, duration_ms, word_count, injected, created_at
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
                created_at: parse_datetime(row.get::<_, String>(7)?),
            })
        })?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to load transcript sessions")
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
    let total_words = sessions.iter().map(|session| session.word_count).sum::<usize>();
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
}

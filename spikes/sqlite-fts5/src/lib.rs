//! Disposable legacy-equivalent FTS5 index probes.

use std::{collections::HashSet, path::Path, time::Duration};

use rusqlite::{Connection, Error, params};

pub const MAX_MESSAGE_CHARS: usize = 32_000;

pub fn open_index(path: &Path) -> Result<Connection, Error> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS indexed_sessions (
            session_id TEXT PRIMARY KEY,
            cwd TEXT NOT NULL,
            title TEXT,
            updated_at TEXT
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
            session_id UNINDEXED,
            ordinal UNINDEXED,
            role UNINDEXED,
            content,
            tokenize='unicode61'
        );
        ",
    )?;
    Ok(connection)
}

fn redact(text: &str) -> String {
    let mut output = text.to_owned();
    for prefix in ["Bearer ", "api_key=", "token=", "password="] {
        if let Some(start) = output
            .to_ascii_lowercase()
            .find(&prefix.to_ascii_lowercase())
        {
            let value_start = start + prefix.len();
            let end = output[value_start..]
                .find(char::is_whitespace)
                .map_or(output.len(), |offset| value_start + offset);
            output.replace_range(value_start..end, "[REDACTED]");
        }
    }
    output.chars().take(MAX_MESSAGE_CHARS).collect()
}

pub fn index_session(
    connection: &mut Connection,
    session_id: &str,
    cwd: &str,
    title: &str,
    updated_at: &str,
    messages: &[(&str, &str)],
) -> Result<(), Error> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO indexed_sessions(session_id,cwd,title,updated_at)
         VALUES (?1,?2,?3,?4)
         ON CONFLICT(session_id) DO UPDATE SET
         cwd=excluded.cwd,title=excluded.title,updated_at=excluded.updated_at",
        params![session_id, cwd, title, updated_at],
    )?;
    transaction.execute("DELETE FROM messages_fts WHERE session_id=?1", [session_id])?;
    for (ordinal, (role, text)) in messages.iter().enumerate() {
        if *role == "system" {
            continue;
        }
        let content = redact(text);
        if content.trim().is_empty() {
            continue;
        }
        transaction.execute(
            "INSERT INTO messages_fts(session_id,ordinal,role,content)
             VALUES (?1,?2,?3,?4)",
            params![session_id, ordinal as i64, role, content],
        )?;
    }
    transaction.commit()
}

pub fn term_search(connection: &Connection, query: &str) -> Result<Vec<String>, Error> {
    let mut statement = connection.prepare(
        "SELECT session_id, bm25(messages_fts) AS rank
         FROM messages_fts
         WHERE messages_fts MATCH ?1 AND role IN ('user','assistant')
         ORDER BY rank, session_id
         LIMIT 60",
    )?;
    let rows = statement
        .query_map([query], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    let mut seen = HashSet::new();
    Ok(rows
        .into_iter()
        .filter(|session_id| seen.insert(session_id.clone()))
        .take(20)
        .collect())
}

pub fn browse(connection: &Connection) -> Result<Vec<String>, Error> {
    let mut statement = connection.prepare(
        "SELECT session_id FROM indexed_sessions ORDER BY updated_at DESC, session_id LIMIT 20",
    )?;
    statement
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()
}

pub fn search_fail_soft(path: &Path, query: &str) -> Vec<String> {
    open_index(path)
        .and_then(|connection| term_search(&connection, query))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::mpsc, thread};
    use tempfile::tempdir;

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, Connection) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("index.sqlite3");
        let connection = open_index(&path).unwrap();
        (directory, path, connection)
    }

    #[test]
    fn fts5_is_compiled_and_runtime_detected() {
        let (_directory, _path, connection) = fixture();
        let enabled: i64 = connection
            .query_row(
                "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(enabled, 1);
        connection
            .execute("CREATE VIRTUAL TABLE probe USING fts5(value)", [])
            .unwrap();
    }

    #[test]
    fn legacy_schema_wal_busy_timeout_and_tokenizer_are_available() {
        let (_directory, _path, connection) = fixture();
        let mode: String = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        let timeout: i64 = connection
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode.to_ascii_lowercase(), "wal");
        assert_eq!(timeout, 5000);
        connection
            .execute(
                "INSERT INTO messages_fts VALUES ('s',0,'user','Café résumé')",
                [],
            )
            .unwrap();
        assert_eq!(term_search(&connection, "cafe").unwrap(), ["s"]);
    }

    #[test]
    fn index_excludes_system_bounds_and_redacts() {
        let (_directory, _path, mut connection) = fixture();
        let huge = format!("needle Bearer secret-token {}", "x".repeat(40_000));
        index_session(
            &mut connection,
            "s1",
            "/fixture",
            "Fixture",
            "2026-01-01T00:00:00Z",
            &[("system", "system-only-secret"), ("user", &huge)],
        )
        .unwrap();
        let rows: Vec<(String, i64)> = connection
            .prepare("SELECT content,length(content) FROM messages_fts")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].0.contains("secret-token"));
        assert!(rows[0].0.contains("[REDACTED]"));
        assert_eq!(rows[0].1, MAX_MESSAGE_CHARS as i64);
        let system_hits: i64 = connection
            .query_row(
                "SELECT count(*) FROM messages_fts WHERE content LIKE '%system-only-secret%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(system_hits, 0);
    }

    #[test]
    fn browse_and_bm25_term_search_are_semantically_stable() {
        let (_directory, _path, mut connection) = fixture();
        index_session(
            &mut connection,
            "old",
            "/fixture",
            "Old",
            "2026-01-01T00:00:00Z",
            &[("user", "rust rust migration"), ("assistant", "done")],
        )
        .unwrap();
        index_session(
            &mut connection,
            "new",
            "/fixture",
            "New",
            "2026-01-02T00:00:00Z",
            &[("user", "rust migration"), ("assistant", "rust")],
        )
        .unwrap();
        assert_eq!(browse(&connection).unwrap(), ["new", "old"]);
        let results = term_search(&connection, "rust AND migration").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.contains(&"old".to_owned()));
        assert!(results.contains(&"new".to_owned()));
    }

    #[test]
    fn lock_contention_respects_busy_timeout() {
        let (_directory, path, first) = fixture();
        first.execute_batch("BEGIN EXCLUSIVE;").unwrap();
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let second = open_index(&path).unwrap();
            sender
                .send(second.execute("INSERT INTO indexed_sessions VALUES ('x','/','', '')", []))
                .unwrap();
        });
        thread::sleep(Duration::from_millis(50));
        first.execute_batch("ROLLBACK;").unwrap();
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .is_ok()
        );
        worker.join().unwrap();
    }

    #[test]
    fn corrupt_index_is_nonfatal_and_rebuildable() {
        let (directory, path, connection) = fixture();
        drop(connection);
        fs::write(&path, b"not sqlite").unwrap();
        assert!(search_fail_soft(&path, "needle").is_empty());

        fs::remove_file(&path).unwrap();
        let mut rebuilt = open_index(&path).unwrap();
        index_session(
            &mut rebuilt,
            "rebuilt",
            "/fixture",
            "Rebuilt",
            "2026-01-01T00:00:00Z",
            &[("user", "needle")],
        )
        .unwrap();
        assert_eq!(term_search(&rebuilt, "needle").unwrap(), ["rebuilt"]);
        assert!(directory.path().exists());
    }
}

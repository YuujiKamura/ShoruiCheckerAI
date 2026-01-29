//! SQLite database for storing check results

use rusqlite::{Connection, Result as SqlResult};
use crate::CheckResult;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new() -> SqlResult<Self> {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("ShoruiChecker");

        std::fs::create_dir_all(&data_dir).ok();

        let db_path = data_dir.join("results.db");
        let conn = Connection::open(db_path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS check_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                file_name TEXT NOT NULL,
                checked_at TEXT NOT NULL,
                status TEXT NOT NULL,
                message TEXT NOT NULL,
                details TEXT
            )",
            [],
        )?;

        Ok(Self { conn })
    }

    pub fn save_result(&self, result: &CheckResult) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO check_results (file_path, file_name, checked_at, status, message, details)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                &result.file_path,
                &result.file_name,
                &result.checked_at,
                &result.status,
                &result.message,
                result.details.as_deref().unwrap_or(""),
            ],
        )?;
        Ok(())
    }

    pub fn get_recent_results(&self, limit: i32) -> SqlResult<Vec<CheckResult>> {
        let mut stmt = self.conn.prepare(
            "SELECT file_path, file_name, checked_at, status, message, details
             FROM check_results
             ORDER BY id DESC
             LIMIT ?1"
        )?;

        let results = stmt.query_map([limit], |row| {
            Ok(CheckResult {
                file_path: row.get(0)?,
                file_name: row.get(1)?,
                checked_at: row.get(2)?,
                status: row.get(3)?,
                message: row.get(4)?,
                details: row.get::<_, Option<String>>(5)?,
            })
        })?;

        results.collect()
    }
}

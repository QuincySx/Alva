// INPUT:  rusqlite, super::schema
// OUTPUT: run_migrations
// POS:    Schema migration runner — applies DDL for initial creation and future version upgrades.
//! Schema migration support.
//!
//! Currently only handles initial creation (version 1).
//! Future migrations can be added to the `MIGRATIONS` array.

use rusqlite::Connection;

use super::schema;

/// Current schema version. Bump when adding migrations.
const CURRENT_VERSION: i64 = 1;

/// Apply all necessary migrations to bring the database up to date.
pub fn run_migrations(conn: &Connection) -> Result<(), rusqlite::Error> {
    let current = get_version(conn)?;

    if current < 1 {
        // Initial schema creation
        for ddl in schema::ALL_DDL {
            conn.execute_batch(ddl)?;
        }
        set_version(conn, CURRENT_VERSION)?;
    }

    // Future: if current < 2 { ... }

    Ok(())
}

fn get_version(conn: &Connection) -> Result<i64, rusqlite::Error> {
    // The schema_version table may not exist yet on a fresh database.
    conn.execute_batch(schema::CREATE_SCHEMA_VERSION)?;

    let mut stmt = conn.prepare("SELECT version FROM schema_version LIMIT 1")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        Ok(row.get(0)?)
    } else {
        Ok(0)
    }
}

fn set_version(conn: &Connection, version: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM schema_version", [])?;
    conn.execute("INSERT INTO schema_version (version) VALUES (?1)", [version])?;
    Ok(())
}

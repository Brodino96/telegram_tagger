use rusqlite::{Connection, Result, params};

/// Represents a tracked user in a chat
#[derive(Debug, Clone)]
pub struct User {
    pub user_id: i64,
    pub first_name: String,
}

/// Initialize the database and create the users table if it doesn't exist
pub fn init_db() -> Result<Connection> {
    let conn = Connection::open("tagger.db")?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            chat_id INTEGER NOT NULL,
            user_id INTEGER NOT NULL,
            first_name TEXT NOT NULL,
            PRIMARY KEY (chat_id, user_id)
        )",
        [],
    )?;

    Ok(conn)
}

/// Insert or update a user in the database
pub fn upsert_user(conn: &Connection, chat_id: i64, user_id: i64, first_name: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO users (chat_id, user_id, first_name)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(chat_id, user_id) DO UPDATE SET
            first_name = excluded.first_name",
        params![chat_id, user_id, first_name],
    )?;

    Ok(())
}

/// Get all tracked users for a specific chat
pub fn get_users_for_chat(conn: &Connection, chat_id: i64) -> Result<Vec<User>> {
    let mut stmt = conn.prepare("SELECT user_id, first_name FROM users WHERE chat_id = ?1")?;

    let users = stmt.query_map([chat_id], |row| {
        Ok(User {
            user_id: row.get(0)?,
            first_name: row.get(1)?,
        })
    })?;

    users.collect()
}

/// Delete a user from a specific chat (when they leave)
pub fn delete_user(conn: &Connection, chat_id: i64, user_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM users WHERE chat_id = ?1 AND user_id = ?2",
        params![chat_id, user_id],
    )?;

    Ok(())
}

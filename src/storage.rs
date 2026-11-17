use crate::ollama::TestResult;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub const PAGE_SIZE: usize = 30;

#[derive(Default, Serialize, Deserialize, Clone)]
pub struct StoredMessage {
    pub role: String,
    pub content: String,
    pub attachments: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub info: String,
}

#[derive(Default, Serialize, Deserialize, Clone)]
pub struct ChatFile {
    pub id: String,
    pub title: String,
    pub model: String,
    pub messages: Vec<StoredMessage>,
    #[serde(default)]
    pub folders: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub mode: String,
}

thread_local! {
    static DB: RefCell<Option<Connection>> = const { RefCell::new(None) };
}

fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dream-llmchatbox")
}

enum ProfileDir {
    Base,
    Profile(PathBuf, bool),
    Closed,
}

static PROFILE_DIR: RwLock<ProfileDir> = RwLock::new(ProfileDir::Base);

pub fn base_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dream-llmchatbox")
}

pub fn data_dir() -> PathBuf {
    match &*PROFILE_DIR.read().unwrap_or_else(|e| e.into_inner()) {
        ProfileDir::Base => base_dir(),
        ProfileDir::Profile(dir, _) => dir.clone(),
        ProfileDir::Closed => base_dir().join("profiles").join(".closed"),
    }
}

fn import_old_chats() -> bool {
    match &*PROFILE_DIR.read().unwrap_or_else(|e| e.into_inner()) {
        ProfileDir::Base => true,
        ProfileDir::Profile(_, import) => *import,
        ProfileDir::Closed => false,
    }
}

fn is_closed() -> bool {
    matches!(
        &*PROFILE_DIR.read().unwrap_or_else(|e| e.into_inner()),
        ProfileDir::Closed
    )
}

pub fn set_profile_dir(dir: Option<PathBuf>, import_old: bool) {
    close();
    *PROFILE_DIR.write().unwrap_or_else(|e| e.into_inner()) = match dir {
        Some(dir) => ProfileDir::Profile(dir, import_old),
        None => ProfileDir::Closed,
    };
}

pub fn checkpoint() {
    DB.with(|cell| {
        if let Some(conn) = cell.borrow().as_ref() {
            let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        }
    });
}

pub fn close() {
    checkpoint();
    DB.with(|cell| {
        if let Some(conn) = cell.borrow_mut().take() {
            let _ = conn.close();
        }
    });
}

pub const DB_FILES: [&str; 3] = ["chats.db", "chats.db-wal", "chats.db-shm"];

pub fn move_old_database(old: &std::path::Path, new: &std::path::Path) {
    if old == new || !old.join("chats.db").exists() || new.join("chats.db").exists() {
        return;
    }
    let _ = fs::create_dir_all(new);
    for name in DB_FILES {
        let from = old.join(name);
        if !from.exists() {
            continue;
        }
        let to = new.join(name);
        if fs::rename(&from, &to).is_err() && fs::copy(&from, &to).is_ok() {
            let _ = fs::remove_file(&from);
        }
    }
}

fn with_db<T>(f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T, String> {
    DB.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            if is_closed() {
                return Err(crate::i18n::tr("The profile is locked"));
            }
            *slot = Some(open().map_err(|e| e.to_string())?);
        }
        let conn = slot.as_ref().ok_or(crate::i18n::tr("Database not open"))?;
        f(conn).map_err(|e| e.to_string())
    })
}

fn open() -> rusqlite::Result<Connection> {
    let dir = data_dir();
    let _ = fs::create_dir_all(&dir);
    let conn = Connection::open(dir.join("chats.db"))?;
    let auto_vacuum: i64 = conn.query_row("PRAGMA auto_vacuum", [], |row| row.get(0))?;
    if auto_vacuum != 2 {
        conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL; VACUUM;")?;
    }
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS chats (
             id TEXT PRIMARY KEY,
             title TEXT NOT NULL,
             model TEXT NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS chats_updated ON chats (updated_at DESC);
         CREATE TABLE IF NOT EXISTS messages (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             chat_id TEXT NOT NULL REFERENCES chats (id) ON DELETE CASCADE,
             position INTEGER NOT NULL,
             role TEXT NOT NULL,
             content TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS messages_chat ON messages (chat_id, position);
         CREATE TABLE IF NOT EXISTS meta (
             key TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS chat_activity (
             chat_id TEXT PRIMARY KEY,
             entries TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS model_tests (
             model TEXT PRIMARY KEY,
             vram INTEGER NOT NULL,
             ram INTEGER NOT NULL,
             tokens_per_sec REAL NOT NULL,
             load_secs REAL NOT NULL,
             tested_at INTEGER NOT NULL
         );",
    )?;
    let has_attachments: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('messages') WHERE name = 'attachments'")?
        .exists([])?;
    if !has_attachments {
        conn.execute_batch("ALTER TABLE messages ADD COLUMN attachments TEXT NOT NULL DEFAULT ''")?;
    }
    let has_status: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('messages') WHERE name = 'status'")?
        .exists([])?;
    if !has_status {
        conn.execute_batch("ALTER TABLE messages ADD COLUMN status TEXT NOT NULL DEFAULT ''")?;
    }
    let has_info: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('messages') WHERE name = 'info'")?
        .exists([])?;
    if !has_info {
        conn.execute_batch("ALTER TABLE messages ADD COLUMN info TEXT NOT NULL DEFAULT ''")?;
    }
    let has_group: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('chats') WHERE name = 'group_id'")?
        .exists([])?;
    if !has_group {
        conn.execute_batch("ALTER TABLE chats ADD COLUMN group_id TEXT NOT NULL DEFAULT ''")?;
    }
    let has_folders: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('chats') WHERE name = 'folders'")?
        .exists([])?;
    if !has_folders {
        conn.execute_batch("ALTER TABLE chats ADD COLUMN folders TEXT NOT NULL DEFAULT ''")?;
    }
    let has_mode: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('chats') WHERE name = 'mode'")?
        .exists([])?;
    if !has_mode {
        conn.execute_batch("ALTER TABLE chats ADD COLUMN mode TEXT NOT NULL DEFAULT ''")?;
    }
    let has_pinned: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('chats') WHERE name = 'pinned'")?
        .exists([])?;
    if !has_pinned {
        conn.execute_batch(
            "ALTER TABLE chats ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE chats ADD COLUMN sort_key REAL;",
        )?;
    }
    let has_think: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('chats') WHERE name = 'think'")?
        .exists([])?;
    let has_think_mode: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('chats') WHERE name = 'think_mode'")?
        .exists([])?;
    if !has_think_mode {
        conn.execute_batch("ALTER TABLE chats ADD COLUMN think_mode INTEGER NOT NULL DEFAULT 0")?;
        if has_think {
            conn.execute_batch("UPDATE chats SET think_mode = 2 WHERE think = 0")?;
        }
    }
    migrate_json_chats(&conn)?;
    Ok(conn)
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn new_id() -> String {
    now_millis().to_string()
}

#[derive(Deserialize)]
struct OldChatFile {
    id: String,
    title: String,
    model: String,
    messages: Vec<OldMessage>,
}

#[derive(Deserialize)]
struct OldMessage {
    role: String,
    content: String,
}

fn old_chats_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dream-llmchatbox")
        .join("chats")
}

fn migrate_json_chats(conn: &Connection) -> rusqlite::Result<()> {
    let done: bool = conn
        .prepare("SELECT 1 FROM meta WHERE key = 'json_migrated'")?
        .exists([])?;
    if done || !import_old_chats() {
        return Ok(());
    }
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('json_migrated', '1')",
        [],
    )?;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM chats", [], |row| row.get(0))?;
    if count > 0 {
        return Ok(());
    }
    let Ok(entries) = fs::read_dir(old_chats_dir()) else {
        return Ok(());
    };

    for entry in entries.flatten() {
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let Ok(json) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(chat) = serde_json::from_str::<OldChatFile>(&json) else {
            continue;
        };
        let chat = ChatFile {
            id: chat.id,
            title: chat.title,
            model: chat.model,
            messages: chat
                .messages
                .into_iter()
                .map(|m| StoredMessage {
                    role: m.role,
                    content: m.content,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        write_chat(conn, &chat, modified)?;
    }
    Ok(())
}

fn write_chat(conn: &Connection, chat: &ChatFile, updated_at: i64) -> rusqlite::Result<()> {
    conn.execute_batch("BEGIN")?;
    let result = (|| {
        conn.execute(
            "INSERT INTO chats (id, title, model, updated_at, folders, group_id, mode) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (id) DO UPDATE SET title = ?2, model = ?3, updated_at = ?4, folders = ?5, group_id = ?6, mode = ?7",
            params![chat.id, chat.title, chat.model, updated_at, chat.folders, chat.group, chat.mode],
        )?;
        conn.execute("DELETE FROM messages WHERE chat_id = ?1", params![chat.id])?;
        let mut insert = conn.prepare(
            "INSERT INTO messages (chat_id, position, role, content, attachments, status, info)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for (position, message) in chat.messages.iter().enumerate() {
            insert.execute(params![
                chat.id,
                position as i64,
                message.role,
                message.content,
                message.attachments,
                message.status,
                message.info
            ])?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => conn.execute_batch("COMMIT"),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

pub fn save_chat(chat: &ChatFile) -> Result<(), String> {
    with_db(|conn| write_chat(conn, chat, now_millis()))
}

pub fn chat_title(id: &str) -> Option<String> {
    with_db(|conn| {
        conn.query_row("SELECT title FROM chats WHERE id = ?1", params![id], |row| {
            row.get(0)
        })
        .optional()
    })
    .ok()
    .flatten()
}

pub fn load_chat(id: &str) -> Result<ChatFile, String> {
    with_db(|conn| {
        let (title, model, folders, group, mode): (String, String, String, String, String) = conn.query_row(
            "SELECT title, model, folders, group_id, mode FROM chats WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )?;
        let mut query = conn.prepare(
            "SELECT role, content, attachments, status, info FROM messages WHERE chat_id = ?1 ORDER BY position",
        )?;
        let messages = query
            .query_map(params![id], |row| {
                Ok(StoredMessage {
                    role: row.get(0)?,
                    content: row.get(1)?,
                    attachments: row.get(2)?,
                    status: row.get(3)?,
                    info: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(ChatFile {
            id: id.to_string(),
            title,
            model,
            messages,
            folders,
            group,
            mode,
        })
    })
}

pub fn rename_chat(id: &str, title: &str) -> Result<(), String> {
    with_db(|conn| {
        conn.execute(
            "UPDATE chats SET title = ?2 WHERE id = ?1",
            params![id, title],
        )
        .map(|_| ())
    })
}

pub fn save_activity(chat: &str, entries: &str) -> Result<(), String> {
    with_db(|conn| {
        conn.execute(
            "INSERT INTO chat_activity (chat_id, entries) VALUES (?1, ?2)
             ON CONFLICT (chat_id) DO UPDATE SET entries = ?2",
            params![chat, entries],
        )
        .map(|_| ())
    })
}

pub fn load_activity(chat: &str) -> String {
    with_db(|conn| {
        conn.query_row(
            "SELECT entries FROM chat_activity WHERE chat_id = ?1",
            params![chat],
            |row| row.get(0),
        )
        .optional()
    })
    .ok()
    .flatten()
    .unwrap_or_default()
}

pub fn delete_chat(id: &str) -> Result<(), String> {
    with_db(|conn| {
        conn.execute_batch("BEGIN")?;
        let result = conn
            .execute("DELETE FROM messages WHERE chat_id = ?1", params![id])
            .and_then(|_| conn.execute("DELETE FROM chat_activity WHERE chat_id = ?1", params![id]))
            .and_then(|_| conn.execute("DELETE FROM chats WHERE id = ?1", params![id]));
        match result {
            Ok(_) => conn.execute_batch("COMMIT")?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }
        free_space(conn);
        Ok(())
    })?;
    let _ = fs::remove_file(old_chats_dir().join(format!("{id}.json")));
    Ok(())
}

pub fn delete_all_chats() -> Result<(), String> {
    with_db(|conn| {
        conn.execute_batch("BEGIN; DELETE FROM messages; DELETE FROM chat_activity; DELETE FROM chats; COMMIT;")?;
        conn.execute_batch("VACUUM;")?;
        let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        Ok(())
    })?;
    let _ = fs::remove_dir_all(old_chats_dir());
    remove_leftovers();
    Ok(())
}

fn remove_leftovers() {
    if let Ok(entries) = fs::read_dir(std::env::temp_dir()) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("dream-llmchatbox-")
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    for folder in ["whisper", "voices"] {
        if let Ok(entries) = fs::read_dir(config_dir().join(folder)) {
            for entry in entries.flatten() {
                if entry.path().extension().is_some_and(|e| e == "part") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
}

pub fn data_size() -> u64 {
    DB_FILES
        .iter()
        .filter_map(|name| fs::metadata(data_dir().join(name)).ok())
        .map(|meta| meta.len())
        .sum()
}

fn free_space(conn: &Connection) {
    if let Ok(mut vacuum) = conn.prepare("PRAGMA incremental_vacuum")
        && let Ok(mut rows) = vacuum.query([])
    {
        while let Ok(Some(_)) = rows.next() {}
    }
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
}

pub fn count_chats() -> usize {
    with_db(|conn| conn.query_row("SELECT COUNT(*) FROM chats", [], |row| row.get::<_, i64>(0)))
        .map(|n| n as usize)
        .unwrap_or(0)
}

pub fn count_ungrouped() -> usize {
    with_db(|conn| {
        conn.query_row("SELECT COUNT(*) FROM chats WHERE group_id = ''", [], |row| {
            row.get::<_, i64>(0)
        })
    })
    .map(|n| n as usize)
    .unwrap_or(0)
}

pub fn list_chats(offset: usize) -> Vec<(String, String)> {
    with_db(|conn| {
        let mut query = conn.prepare(
            "SELECT id, title FROM chats WHERE group_id = '' ORDER BY pinned DESC, COALESCE(sort_key, updated_at) DESC LIMIT ?1 OFFSET ?2",
        )?;
        query
            .query_map(params![PAGE_SIZE as i64, offset as i64], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect()
    })
    .unwrap_or_default()
}

pub fn list_group_chats(group: &str) -> Vec<(String, String)> {
    with_db(|conn| {
        let mut query = conn
            .prepare("SELECT id, title FROM chats WHERE group_id = ?1 ORDER BY pinned DESC, COALESCE(sort_key, updated_at) DESC")?;
        query
            .query_map(params![group], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect()
    })
    .unwrap_or_default()
}

pub fn search_chats(query: &str) -> Vec<(String, String)> {
    let pattern = format!("%{}%", query.trim().replace('%', "\\%").replace('_', "\\_"));
    with_db(|conn| {
        let mut statement = conn.prepare(
            "SELECT id, title FROM chats WHERE title LIKE ?1 ESCAPE '\\'
             OR id IN (SELECT chat_id FROM messages WHERE content LIKE ?1 ESCAPE '\\')
             ORDER BY pinned DESC, updated_at DESC LIMIT 50",
        )?;
        statement.query_map(params![pattern], |row| Ok((row.get(0)?, row.get(1)?)))?.collect()
    })
    .unwrap_or_default()
}

pub fn chat_think(id: &str) -> i32 {
    with_db(|conn| conn.query_row("SELECT think_mode FROM chats WHERE id = ?1", params![id], |row| row.get::<_, i64>(0)))
        .map(|value| value as i32)
        .unwrap_or(0)
}

pub fn set_chat_think(id: &str, mode: i32) -> Result<(), String> {
    with_db(|conn| conn.execute("UPDATE chats SET think_mode = ?2 WHERE id = ?1", params![id, mode as i64]).map(|_| ()))
}

pub fn pinned_ids() -> Vec<String> {
    with_db(|conn| {
        let mut query = conn.prepare("SELECT id FROM chats WHERE pinned = 1")?;
        query.query_map([], |row| row.get(0))?.collect()
    })
    .unwrap_or_default()
}

pub fn set_pinned(id: &str, pinned: bool) -> Result<(), String> {
    with_db(|conn| {
        conn.execute(
            "UPDATE chats SET pinned = ?2, sort_key = NULL WHERE id = ?1",
            params![id, pinned as i64],
        )
        .map(|_| ())
    })
}

pub fn move_chat(id: &str, delta: i64) -> Result<(), String> {
    with_db(|conn| {
        let (group, pinned): (String, i64) = conn.query_row(
            "SELECT group_id, pinned FROM chats WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut query = conn.prepare(
            "SELECT id, COALESCE(sort_key, updated_at) FROM chats WHERE group_id = ?1 AND pinned = ?2 ORDER BY COALESCE(sort_key, updated_at) DESC",
        )?;
        let list: Vec<(String, f64)> = query
            .query_map(params![group, pinned], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let keys: Vec<f64> = list.iter().map(|(_, key)| *key).collect();
        let Some(from) = list.iter().position(|(chat, _)| chat == id) else {
            return Ok(());
        };
        if let Some(key) = moved_key(&keys, from, delta) {
            conn.execute("UPDATE chats SET sort_key = ?2 WHERE id = ?1", params![id, key])?;
        }
        Ok(())
    })
}

pub fn moved_key(keys: &[f64], from: usize, delta: i64) -> Option<f64> {
    let to = (from as i64 + delta).clamp(0, keys.len() as i64 - 1) as usize;
    if to == from {
        return None;
    }
    let mut others: Vec<f64> = keys.to_vec();
    others.remove(from);
    let above = to.checked_sub(1).and_then(|index| others.get(index)).copied();
    let below = others.get(to).copied();
    Some(match (above, below) {
        (Some(a), Some(b)) => (a + b) / 2.0,
        (None, Some(b)) => b + 1000.0,
        (Some(a), None) => a - 1000.0,
        (None, None) => return None,
    })
}

pub fn set_chat_group(id: &str, group: &str) -> Result<(), String> {
    with_db(|conn| {
        conn.execute("UPDATE chats SET group_id = ?2 WHERE id = ?1", params![id, group])
            .map(|_| ())
    })
}

pub fn group_chat_files(group: &str) -> Vec<ChatFile> {
    list_group_chats(group)
        .into_iter()
        .filter_map(|(id, _)| load_chat(&id).ok())
        .collect()
}

pub fn ungroup_all(group: &str) -> Result<(), String> {
    with_db(|conn| {
        conn.execute("UPDATE chats SET group_id = '' WHERE group_id = ?1", params![group])
            .map(|_| ())
    })
}

pub fn save_model_test(model: &str, test: &TestResult) -> Result<(), String> {
    with_db(|conn| {
        conn.execute(
            "INSERT INTO model_tests (model, vram, ram, tokens_per_sec, load_secs, tested_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (model) DO UPDATE SET vram = ?2, ram = ?3, tokens_per_sec = ?4,
             load_secs = ?5, tested_at = ?6",
            params![
                model,
                test.vram as i64,
                test.ram as i64,
                test.tokens_per_sec,
                test.load_secs,
                now_millis()
            ],
        )
        .map(|_| ())
    })
}

pub fn load_model_test(model: &str) -> Option<TestResult> {
    with_db(|conn| {
        conn.query_row(
            "SELECT vram, ram, tokens_per_sec, load_secs FROM model_tests WHERE model = ?1",
            params![model],
            |row| {
                Ok(TestResult {
                    vram: row.get::<_, i64>(0)? as u64,
                    ram: row.get::<_, i64>(1)? as u64,
                    tokens_per_sec: row.get(2)?,
                    load_secs: row.get(3)?,
                })
            },
        )
        .optional()
    })
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_order_keys() {
        let keys = [400.0, 300.0, 200.0, 100.0];
        assert_eq!(moved_key(&keys, 3, -3), Some(1400.0));
        assert_eq!(moved_key(&keys, 0, 1), Some(250.0));
        assert_eq!(moved_key(&keys, 0, 9), Some(-900.0));
        assert_eq!(moved_key(&keys, 1, 0), None);
    }

    #[test]
    fn delete_frees_disk_space() {
        let dir = std::env::temp_dir().join(format!("dlc-storage-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &dir);
            std::env::set_var("XDG_DATA_HOME", &dir);
        }

        let big = "x".repeat(5_000_000);
        save_chat(&ChatFile {
            id: "1".into(),
            title: "Big".into(),
            model: "m".into(),
            messages: vec![StoredMessage {
                role: "user".into(),
                content: "hi".into(),
                attachments: big,
                status: String::new(),
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
        with_db(|conn| conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))).unwrap();
        let db = dir.join("dream-llmchatbox").join("chats.db");
        let before = fs::metadata(&db).unwrap().len();

        delete_chat("1").unwrap();
        let after = fs::metadata(&db).unwrap().len();
        let leftover: i64 = with_db(|conn| {
            conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))
        })
        .unwrap();

        assert_eq!(leftover, 0);
        assert!(before > 4_000_000, "before = {before}");
        assert!(after < 1_000_000, "after = {after}");

        for id in ["2", "3"] {
            save_chat(&ChatFile {
                id: id.into(),
                title: "T".into(),
                model: "m".into(),
                messages: vec![StoredMessage {
                    role: "user".into(),
                    content: "x".repeat(2_000_000),
                    attachments: String::new(),
                    status: String::new(),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .unwrap();
        }
        assert_eq!(count_chats(), 2);
        delete_all_chats().unwrap();
        assert_eq!(count_chats(), 0);
        assert!(data_size() < 1_000_000, "size = {}", data_size());
        let _ = fs::remove_dir_all(&dir);
    }
}

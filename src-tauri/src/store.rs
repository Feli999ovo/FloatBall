//! 对话历史：本地 SQLite（应用数据目录下 history.db），不上传任何地方。

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConvSummary {
    pub id: i64,
    pub title: String,
    pub updated: i64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Msg {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub created: i64,
}

pub struct Store {
    conn: Connection,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn make_title(text: &str) -> String {
    let t: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s: String = t.chars().take(24).collect();
    if t.chars().count() > 24 {
        s.push('…');
    }
    if s.is_empty() {
        "新对话".into()
    } else {
        s
    }
}

impl Store {
    pub fn open(dir: &Path) -> rusqlite::Result<Store> {
        std::fs::create_dir_all(dir).ok();
        Self::from_conn(Connection::open(dir.join("history.db"))?)
    }

    pub fn from_conn(conn: Connection) -> rusqlite::Result<Store> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS conversations(
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               title TEXT NOT NULL,
               created INTEGER NOT NULL,
               updated INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS messages(
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               conv_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
               role TEXT NOT NULL,
               content TEXT NOT NULL,
               created INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS idx_msg_conv ON messages(conv_id, id);",
        )?;
        Ok(Store { conn })
    }

    /// 追加一条消息；conv_id 为空时新建对话（标题取第一句话）。返回对话 id。
    pub fn append(&self, conv_id: Option<i64>, role: &str, content: &str) -> rusqlite::Result<i64> {
        let t = now();
        let exists = match conv_id {
            Some(id) => self
                .conn
                .query_row("SELECT id FROM conversations WHERE id=?1", params![id], |r| r.get::<_, i64>(0))
                .optional()?,
            None => None,
        };
        let id = match exists {
            Some(id) => id,
            None => {
                self.conn.execute(
                    "INSERT INTO conversations(title,created,updated) VALUES(?1,?2,?2)",
                    params![make_title(content), t],
                )?;
                self.conn.last_insert_rowid()
            }
        };
        self.conn.execute(
            "INSERT INTO messages(conv_id,role,content,created) VALUES(?1,?2,?3,?4)",
            params![id, role, content, t],
        )?;
        self.conn.execute("UPDATE conversations SET updated=?2 WHERE id=?1", params![id, t])?;
        Ok(id)
    }

    pub fn list(&self, limit: i64) -> rusqlite::Result<Vec<ConvSummary>> {
        let mut st = self.conn.prepare(
            "SELECT c.id, c.title, c.updated, (SELECT COUNT(*) FROM messages m WHERE m.conv_id=c.id)
             FROM conversations c ORDER BY c.updated DESC, c.id DESC LIMIT ?1",
        )?;
        let rows = st.query_map(params![limit], |r| {
            Ok(ConvSummary { id: r.get(0)?, title: r.get(1)?, updated: r.get(2)?, count: r.get(3)? })
        })?;
        rows.collect()
    }

    pub fn load(&self, conv_id: i64) -> rusqlite::Result<Vec<Msg>> {
        let mut st = self
            .conn
            .prepare("SELECT role, content, created FROM messages WHERE conv_id=?1 ORDER BY id")?;
        let rows = st.query_map(params![conv_id], |r| {
            Ok(Msg { role: r.get(0)?, content: r.get(1)?, created: r.get(2)? })
        })?;
        rows.collect()
    }

    pub fn delete(&self, conv_id: i64) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM conversations WHERE id=?1", params![conv_id])?;
        Ok(())
    }

    pub fn clear(&self) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM conversations", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud() {
        let s = Store::from_conn(Connection::open_in_memory().unwrap()).unwrap();
        let id = s.append(None, "user", "帮我总结一下今天的待办事项和明天要准备的会议材料，越短越好，谢谢你").unwrap();
        s.append(Some(id), "assistant", "好的").unwrap();
        let id2 = s.append(None, "user", "第二个").unwrap();
        assert_ne!(id, id2);
        let l = s.list(10).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].id, id2);
        assert!(l[1].title.ends_with('…'));
        assert_eq!(l[1].count, 2);
        assert_eq!(s.load(id).unwrap().len(), 2);
        s.delete(id).unwrap();
        assert!(s.load(id).unwrap().is_empty());
        assert_eq!(s.list(10).unwrap().len(), 1);
        // 不存在的 id 会新建
        let id3 = s.append(Some(9999), "user", "x").unwrap();
        assert_ne!(id3, 9999);
        s.clear().unwrap();
        assert!(s.list(10).unwrap().is_empty());
    }
}
